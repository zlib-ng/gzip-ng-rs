// main.rs -- gzip-ng command line tool
// For conditions of distribution and use, see LICENSE.md

use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt::Display;
use std::fs::{self, File, FileTimes, Metadata, OpenOptions};
use std::io::{self, IsTerminal, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, UNIX_EPOCH};

use gzng::format::{self, Parse};
use gzng::{Compress, Totals, gzblock};

mod options;

use options::{Options, Parsed};

/// Gzip exit statuses.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok = 0,
    Error = 1,
    Warning = 2,
}

// ===========================================================================
// Diagnostics

/// What the system calls an error, without the number Rust appends to it.
fn os_message(err: &io::Error) -> String {
    let msg = err.to_string();

    match msg.rfind(" (os error ") {
        Some(at) => msg[..at].to_string(),
        None => msg,
    }
}

/// Report a failure on path. An error from the block engine brings a message of its own, and
/// only the system's errors have anything to add about the path.
fn fail(path: impl Display, err: &io::Error) {
    let engine = gzblock::Error::from_io(err);
    let os = match engine {
        Some(engine) => {
            eprintln!("gzip-ng: {engine}");
            engine.io_error()
        }
        None => Some(err),
    };

    match os.filter(|os| os.raw_os_error().is_some()) {
        Some(os) => eprintln!("gzip-ng: {path}: {}", os_message(os)),
        None => eprintln!("gzip-ng: {path}: processing failed"),
    }
}

fn warn(opt: &Options, msg: impl Display) {
    if !opt.quiet {
        eprintln!("{msg}");
    }
}

/// The gzip --verbose report, the reduction for compression, the expansion basis for
/// decompression.
fn report(opt: &Options, name: impl Display, outname: Option<&Path>, totals: Totals) {
    let (basis, other) = if opt.decompress {
        (totals.total_out, totals.total_in)
    } else {
        (totals.total_in, totals.total_out)
    };
    let pct = if basis != 0 {
        100.0 * (1.0 - other as f64 / basis as f64)
    } else {
        0.0
    };

    if !opt.verbose || opt.quiet {
        return;
    }
    match outname {
        Some(outname) => eprintln!("{name}:\t{pct:5.1}% -- replaced with {}", outname.display()),
        None => eprintln!("{name}:\t{pct:5.1}%"),
    }
}

/// How a run that leaves its input in place went, under the name it is reported by. Returns Ok,
/// or Error with the error reported.
fn conclude(opt: &Options, name: impl Display, result: io::Result<Totals>) -> Status {
    match result {
        Ok(totals) => {
            report(opt, name, None, totals);
            Status::Ok
        }
        Err(err) => {
            fail(name, &err);
            Status::Error
        }
    }
}

// ===========================================================================
// File names

/// The suffix that names a gzip file.
const GZ_SUFFIX: &str = ".gz";

/// Whether path is named as a gzip file, by its suffix.
fn path_is_gzip(path: &OsStr) -> bool {
    let bytes = path.as_encoded_bytes();

    bytes.len() > GZ_SUFFIX.len() && bytes.ends_with(GZ_SUFFIX.as_bytes())
}

fn path_with_suffix(path: &OsStr) -> PathBuf {
    let mut suffixed = path.to_os_string();

    suffixed.push(GZ_SUFFIX);
    suffixed.into()
}

/// Compression turns file into file.gz, decompression file.gz into file, or file into file by
/// reading file.gz. Returns the input and output paths.
fn path_derive(path: &OsStr, decompress: bool) -> (PathBuf, PathBuf) {
    if !decompress {
        (path.into(), path_with_suffix(path))
    } else if path_is_gzip(path) {
        let bytes = path.as_encoded_bytes();
        // SAFETY: the cut falls right before the suffix, which is valid UTF-8 on its own.
        let stem = unsafe { OsStr::from_encoded_bytes_unchecked(&bytes[..bytes.len() - GZ_SUFFIX.len()]) };
        (path.into(), stem.into())
    } else {
        (path_with_suffix(path), path.into())
    }
}

/// A stored name as the platform spells file names.
fn stored_name(name: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        OsStr::from_bytes(name).to_os_string()
    }
    #[cfg(not(unix))]
    {
        String::from_utf8_lossy(name).into_owned().into()
    }
}

/// The output name for --name, the stored name placed in the input's directory.
fn path_from_stored(in_path: &Path, stored: &[u8]) -> Option<PathBuf> {
    let base = stored
        .rsplit(|&byte| byte == b'/')
        .next()
        .filter(|base| !base.is_empty())?;

    Some(in_path.with_file_name(stored_name(base)))
}

// ===========================================================================
// Input files

/// Open an input read-only, with its metadata. None with the error reported.
fn open_input(path: &Path) -> Option<(File, Metadata)> {
    match File::open(path).and_then(|input| input.metadata().map(|meta| (input, meta))) {
        Ok(opened) => Some(opened),
        Err(err) => {
            fail(path.display(), &err);
            None
        }
    }
}

/// The modification time and stored name from the header at the start of input, which is rewound
/// afterwards. The name is empty when absent. None when not gzip.
fn read_header(input: &mut File) -> Option<(u32, Vec<u8>)> {
    let mut buf = Vec::with_capacity(4096);

    let _ = Read::by_ref(input).take(4096).read_to_end(&mut buf);
    if input.seek(SeekFrom::Start(0)).is_err() || !format::is_gzip(&buf) {
        return None;
    }
    // A header that outruns the buffer still yields the fields that fit in it.
    let (parse, hdr) = format::header_parse(&buf);
    if parse == Parse::Invalid {
        return None;
    }
    let mut name = hdr.name.unwrap_or_default().to_vec();
    name.truncate(format::NAME_MAX - 1);
    Some((hdr.mtime, name))
}

// ===========================================================================
// Stream processing

/// Standard output as a plain file. Rust's own handle buffers by line, which compressed bytes
/// have no use for.
fn stdout_file() -> io::Result<File> {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        Ok(io::stdout().as_fd().try_clone_to_owned()?.into())
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        Ok(io::stdout().as_handle().try_clone_to_owned()?.into())
    }
}

/// -T writes the bytes through untouched.
fn copy_stream(mut input: impl Read, mut out: impl Write) -> io::Result<Totals> {
    let n = io::copy(&mut input, &mut out)?;

    Ok(Totals {
        total_in: n,
        total_out: n,
    })
}

fn run_stream(input: impl Read, out: impl Write, opt: &Options, mtime: u32, name: Option<&[u8]>) -> io::Result<Totals> {
    if opt.decompress {
        return gzng::decompress_stream(input, out, &[], opt.threads);
    }
    if opt.transparent {
        return copy_stream(input, out);
    }
    // Threads need blocks to work on, so --processes implies one. Without either this stays a
    // single deflate stream.
    let mut block_size = opt.block_size;
    if block_size == 0 && opt.threads_given && opt.threads != 1 {
        block_size = gzng::DEFAULT_BLOCK;
    }
    gzng::compress_stream(
        input,
        out,
        &Compress {
            level: opt.level,
            strategy: opt.strategy,
            block_size,
            threads: opt.threads,
            rsyncable: opt.rsyncable,
            mtime,
            name,
        },
    )
}

/// Under --test the first two bytes decide, since the reader passes anything but gzip through.
/// They come back for the reader to take first. None with a warning naming the input when they
/// are not gzip's.
fn test_peek(input: &mut impl Read, name: impl Display, opt: &Options) -> Option<Vec<u8>> {
    let mut head = Vec::with_capacity(2);

    let _ = input.by_ref().take(2).read_to_end(&mut head);
    if format::is_gzip(&head) {
        return Some(head);
    }
    warn(opt, format_args!("gzip-ng: {name}: not in gzip format"));
    None
}

/// Compressed bytes belong in a file or a pipe, gzip refuses a terminal without --force.
fn tty_guard(opt: &Options) -> Status {
    if !opt.decompress && !opt.transparent && !opt.force && io::stdout().is_terminal() {
        eprintln!("gzip-ng: compressed data not written to a terminal, use -f to force");
        return Status::Error;
    }
    Status::Ok
}

/// Filter stdin to stdout. Returns Ok, or Error with the error reported.
fn process_stdio(opt: &Options) -> Status {
    let mut stdin = io::stdin().lock();

    if tty_guard(opt) != Status::Ok {
        return Status::Error;
    }
    let result = if opt.test_mode {
        let Some(head) = test_peek(&mut stdin, "stdin", opt) else {
            return Status::Error;
        };
        gzng::decompress_stream(stdin, io::sink(), &head, opt.threads)
    } else {
        stdout_file().and_then(|stdout| run_stream(stdin, stdout, opt, 0, None))
    };
    conclude(opt, "stdin", result)
}

// ===========================================================================
// Output file attributes

/// gzip carries the input file's mode and times onto the output, and --name on decompression
/// prefers the time stored in the header.
fn copy_attrs(out: &File, ist: &Metadata, mtime: u32) {
    let modified = if mtime != 0 {
        Ok(UNIX_EPOCH + Duration::from_secs(mtime.into()))
    } else {
        ist.modified()
    };
    let mut times = FileTimes::new();

    if let Ok(accessed) = ist.accessed() {
        times = times.set_accessed(accessed);
    }
    if let Ok(modified) = modified {
        times = times.set_modified(modified);
    }
    let _ = out.set_times(times);
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        fs::Permissions::from_mode(ist.permissions().mode() & 0o7777)
    };
    #[cfg(not(unix))]
    let permissions = ist.permissions();
    let _ = out.set_permissions(permissions);
}

/// A new name is durable only once its directory is synced too. Windows has no way to ask, the
/// file's own flush in process_file is all --synchronous can give there.
fn sync_dir(out_path: &Path) {
    #[cfg(unix)]
    {
        let dir = out_path
            .parent()
            .filter(|dir| !dir.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if let Ok(dir) = File::open(dir) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = out_path;
}

// ===========================================================================
// Integrity testing

/// Check a file's integrity without writing anything, the --test verdict. Returns Ok, or Error
/// with the error reported.
fn test_file(mut input: File, path: &Path, opt: &Options) -> Status {
    let Some(head) = test_peek(&mut input, path.display(), opt) else {
        return Status::Error;
    };

    if let Err(err) = gzng::decompress_stream(input, io::sink(), &head, opt.threads) {
        fail(path.display(), &err);
        return Status::Error;
    }
    if opt.verbose && !opt.quiet {
        eprintln!("{}:\t OK", path.display());
    }
    Status::Ok
}

// ===========================================================================
// Listing

#[derive(Default)]
struct ListTotals {
    compressed: u64,
    uncompressed: u64,
    files: u32,
}

/// The header's time in local time, the way strftime's "%b %e %H:%M" shows it.
fn local_time(mtime: u32) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let t = mtime as libc::time_t;
    // SAFETY: a zeroed tm is a valid one, all integers and a null zone name.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };

    // SAFETY: both pointers are to locals of the types asked for.
    #[cfg(unix)]
    let converted = !unsafe { libc::localtime_r(&t, &mut tm) }.is_null();
    // SAFETY: both pointers are to locals of the types asked for.
    #[cfg(windows)]
    let converted = unsafe { libc::localtime_s(&mut tm, &t) } == 0;
    if !converted {
        return String::new();
    }
    let month = MONTHS[tm.tm_mon.rem_euclid(12) as usize];
    format!("{month} {:2} {:02}:{:02}", tm.tm_mday, tm.tm_hour, tm.tm_min)
}

/// The header row of the listing.
fn list_begin(opt: &Options) {
    let mut out = io::stdout().lock();

    if opt.verbose {
        let _ = write!(out, "method  crc      date   time  ");
    }
    let _ = writeln!(out, "  compressed uncompressed  ratio uncompressed_name");
}

fn list_row(opt: &Options, crc: u32, mtime: u32, compressed: u64, uncompressed: u64, name: impl Display) {
    let pct = if uncompressed != 0 {
        100.0 * (1.0 - compressed as f64 / uncompressed as f64)
    } else {
        0.0
    };
    let mut out = io::stdout().lock();

    if opt.verbose {
        let when = if mtime != 0 { local_time(mtime) } else { String::new() };
        let _ = write!(out, "defla {crc:08x} {when:<12}  ");
    }
    let _ = writeln!(out, "{compressed:12} {uncompressed:12} {pct:5.1}% {name}");
}

/// List one compressed file the way gzip --list does, accumulating totals. name is the file's
/// name without its suffix, shown unless --name prefers the stored one.
fn list_file(
    mut input: File,
    path: &Path,
    name: &Path,
    st: &Metadata,
    opt: &Options,
    totals: &mut ListTotals,
) -> Status {
    let mut tail = [0; format::TRAILER_LEN];

    if st.len() < (format::HEADER_LEN + format::TRAILER_LEN) as u64 {
        eprintln!("gzip-ng: {}: too short to be gzip", path.display());
        return Status::Error;
    }
    let Some((mtime, stored)) = read_header(&mut input) else {
        warn(opt, format_args!("gzip-ng: {}: not in gzip format", path.display()));
        return Status::Error;
    };
    // The trailer of the last member, the same 32-bit size gzip reports.
    let from_end = SeekFrom::End(-(format::TRAILER_LEN as i64));
    if input.seek(from_end).and_then(|_| input.read_exact(&mut tail)).is_err() {
        return Status::Error;
    }
    let (crc, size) = format::trailer_parse(&tail);
    let uncompressed = u64::from(size);
    let stored = stored_name(&stored);
    let name = if opt.name_mode == Some(true) && !stored.is_empty() {
        Path::new(&stored)
    } else {
        name
    };

    list_row(opt, crc, mtime, st.len(), uncompressed, name.display());
    totals.compressed += st.len();
    totals.uncompressed += uncompressed;
    totals.files += 1;
    Status::Ok
}

/// The totals row, once more than one file was listed.
fn list_end(opt: &Options, totals: &ListTotals) {
    if totals.files < 2 {
        return;
    }
    if opt.verbose {
        let _ = write!(io::stdout(), "                              ");
    }
    list_row(opt, 0, 0, totals.compressed, totals.uncompressed, "(totals)");
}

// ===========================================================================
// Processing a file

/// With --stdout the input file is left in place.
fn process_to_stdout(
    input: File,
    opt: &Options,
    in_path: &Path,
    store_mtime: u32,
    store_name: Option<&[u8]>,
) -> Status {
    if tty_guard(opt) != Status::Ok {
        return Status::Error;
    }
    let result = stdout_file().and_then(|stdout| run_stream(input, stdout, opt, store_mtime, store_name));
    conclude(opt, in_path.display(), result)
}

/// The time and name a new header records, subject to --no-name, --name, --no-time, and --time.
fn header_fields<'a>(in_path: &'a Path, ist: &Metadata, opt: &Options) -> (u32, Option<&'a [u8]>) {
    let mut mtime = 0;
    let mut name = None;

    if opt.name_mode != Some(false) {
        name = in_path.file_name().map(OsStr::as_encoded_bytes);
    }
    if opt.time_mode != Some(false) {
        let modified = ist
            .modified()
            .ok()
            .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok());
        mtime = modified.map_or(0, |since| since.as_secs() as u32);
    }
    (mtime, name)
}

/// Create the output, which has to be new without --force. An Err is the status to give up with,
/// the reason already reported.
fn create_output(out_path: &Path, opt: &Options) -> Result<File, Status> {
    let mut create = OpenOptions::new();

    create.write(true);
    if opt.force {
        create.create(true).truncate(true);
    } else {
        create.create_new(true);
    }
    match create.open(out_path) {
        Ok(out) => Ok(out),
        Err(err) if !opt.force && err.kind() == io::ErrorKind::AlreadyExists => {
            let out_path = out_path.display();
            warn(
                opt,
                format_args!("gzip-ng: {out_path} already exists, not overwritten, use -f"),
            );
            Err(Status::Warning)
        }
        Err(err) => {
            fail(out_path.display(), &err);
            Err(Status::Error)
        }
    }
}

/// Process input, named in_path, into the file named by out_path, which --name may change.
fn process_file(mut input: File, in_path: &Path, mut out_path: PathBuf, ist: &Metadata, opt: &Options) -> Status {
    // What a new header records, when compressing.
    let (store_mtime, store_name) = if opt.decompress {
        (0, None)
    } else {
        header_fields(in_path, ist, opt)
    };
    // The stored time to put on the output, when decompressing asks for it.
    let mut restore_mtime = 0;

    if opt.stdout_mode {
        return process_to_stdout(input, opt, in_path, store_mtime, store_name);
    }
    // With --name the stored name decides the output path, and --name or --time whether the
    // stored time applies.
    if opt.decompress {
        let Some((stored_mtime, stored)) = read_header(&mut input) else {
            warn(opt, format_args!("gzip-ng: {}: not in gzip format", in_path.display()));
            return Status::Error;
        };
        if opt.name_mode == Some(true)
            && let Some(path) = path_from_stored(in_path, &stored)
        {
            out_path = path;
        }
        if opt.time_mode == Some(true) {
            restore_mtime = stored_mtime;
        }
    }
    let mut out = match create_output(&out_path, opt) {
        Ok(out) => out,
        Err(status) => return status,
    };
    // --synchronous flushes the output before the input is unlinked. A failure names the file it
    // happened on.
    let written = run_stream(input, &mut out, opt, store_mtime, store_name)
        .map_err(|err| (in_path, err))
        .and_then(|totals| {
            if opt.synchronous {
                out.sync_all().map_err(|err| (out_path.as_path(), err))?;
            }
            Ok(totals)
        });
    let totals = match written {
        Ok(totals) => totals,
        Err((path, err)) => {
            fail(path.display(), &err);
            drop(out);
            let _ = fs::remove_file(&out_path);
            return Status::Error;
        }
    };
    copy_attrs(&out, ist, restore_mtime);
    drop(out);
    if opt.synchronous {
        sync_dir(&out_path);
    }
    report(opt, in_path.display(), Some(&out_path), totals);
    Status::Ok
}

// ===========================================================================
// The command line

/// The worse of two statuses, an error outranks a warning, which outranks success.
fn worse(rc: Status, r: Status) -> Status {
    if r == Status::Error || (r == Status::Warning && rc == Status::Ok) {
        r
    } else {
        rc
    }
}

/// One file, listed with --list, checked with --test, or processed. The input is opened and
/// closed here, and removed once processing replaced it.
fn run_file(path: &OsStr, opt: &Options, totals: &mut ListTotals) -> Status {
    let (in_path, out_path) = path_derive(path, opt.decompress || opt.list);
    let Some((input, st)) = open_input(&in_path) else {
        return Status::Error;
    };
    let rc = if opt.list {
        list_file(input, &in_path, &out_path, &st, opt, totals)
    } else if opt.test_mode {
        test_file(input, &in_path, opt)
    } else {
        process_file(input, &in_path, out_path, &st, opt)
    };

    // Processing replaces the input, unless --keep or --stdout leaves it in place.
    if rc == Status::Ok && !opt.list && !opt.test_mode && !opt.stdout_mode && !opt.keep {
        let _ = fs::remove_file(&in_path);
    }
    rc
}

/// Walk a directory for the files --recursive picks, the way gzip --recursive does.
fn run_dir(path: &Path, opt: &Options, totals: &mut ListTotals) -> Status {
    // Compression skips entries already suffixed, decompression and listing take only suffixed
    // entries, the way gzip --recursive chooses files.
    let want_suffix = opt.decompress || opt.list;
    let mut rc = Status::Ok;
    let dir = match fs::read_dir(path) {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("gzip-ng: {}: {}", path.display(), os_message(&err));
            return Status::Error;
        }
    };

    for entry in dir.flatten() {
        let sub = entry.path();
        // Symbolic links are not walked into, and the entry's own type is not one followed.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            rc = worse(rc, run_dir(&sub, opt, totals));
        } else if kind.is_file() && path_is_gzip(sub.as_os_str()) == want_suffix {
            rc = worse(rc, run_file(sub.as_os_str(), opt, totals));
        }
    }
    rc
}

/// One argument, a file, or a directory walked under --recursive and a warning without.
fn run_path(path: &OsStr, opt: &Options, totals: &mut ListTotals) -> Status {
    if fs::metadata(path).is_ok_and(|st| st.is_dir()) {
        if opt.recursive {
            return run_dir(Path::new(path), opt, totals);
        }
        warn(
            opt,
            format_args!("gzip-ng: {} is a directory, ignored", Path::new(path).display()),
        );
        return Status::Warning;
    }
    run_file(path, opt, totals)
}

fn run() -> Status {
    let args: Vec<OsString> = env::args_os().collect();
    let mut opt = Options::default();
    let mut totals = ListTotals::default();
    let mut rc = Status::Ok;

    if let Some(argv0) = args.first() {
        opt.personas(argv0);
    }
    let files = match opt.parse(&args) {
        Ok(Parsed::Files(files)) => files,
        Ok(Parsed::Done) => return Status::Ok,
        Err(msg) => {
            eprintln!("{msg}");
            return Status::Error;
        }
    };
    if files.is_empty() {
        if opt.list {
            eprintln!("gzip-ng: -l needs file arguments");
            return Status::Error;
        }
        return process_stdio(&opt);
    }
    if opt.list {
        list_begin(&opt);
    }
    for file in &files {
        let r = if file == "-" && !opt.list {
            process_stdio(&opt)
        } else {
            run_path(file, &opt, &mut totals)
        };
        rc = worse(rc, r);
    }
    if opt.list {
        list_end(&opt, &totals);
    }
    rc
}

fn main() -> ExitCode {
    // A reader that goes away ends the run quietly, as it does for gzip. Rust starts with the
    // signal ignored, which would turn a closed pipe into a write error instead.
    // SAFETY: the default disposition is always a valid one to set.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    ExitCode::from(run() as u8)
}
