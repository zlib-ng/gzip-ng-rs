// cli.rs -- end-to-end checks of the command
// For conditions of distribution and use, see LICENSE.md

// Stream roundtrips with gzip cross-checks, and what the stream functions cannot cover, in-place
// and --keep semantics, headers and names, listing, testing, recursion, and that threads do not
// change the compressed bytes.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const EXE: &str = env!("CARGO_BIN_EXE_gzip-ng");

fn data() -> Vec<u8> {
    b"the quick brown fox jumps over the lazy dog 0123456789 ".repeat(20000)
}

/// A fresh directory for one test, so tests running side by side do not meet.
fn workdir(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{name}"));

    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn command(exe: impl AsRef<OsStr>, args: &[&OsStr], stdin: Option<&Path>) -> Output {
    let stdin = stdin.map_or_else(Stdio::null, |path| File::open(path).unwrap().into());

    Command::new(exe).args(args).stdin(stdin).output().unwrap()
}

/// Run the command on arguments, anything that names a path or an option.
macro_rules! gz {
    ($($arg:expr),* $(,)?) => {
        command(EXE, &[$(AsRef::<OsStr>::as_ref(&$arg)),*], None)
    };
}

/// Run the command as a filter over the file at stdin, giving back what it wrote.
fn filter(exe: &str, args: &[&str], stdin: &Path) -> Vec<u8> {
    let args: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let out = command(exe, &args, Some(stdin));

    assert!(
        out.status.success(),
        "{exe} {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// ===========================================================================
// Roundtrips through the stream paths, gzip cross-checks included

/// Compress the test data and decompress it back, and where gzip is installed check that it
/// reads the output and that its own output is read.
fn compress_and_verify(name: &str, compress_args: &[&str], decompress_args: &[&str]) {
    let dir = workdir(name);
    let (input, packed) = (dir.join("lorem.txt"), dir.join("lorem.gz"));
    let data = data();

    fs::write(&input, &data).unwrap();
    fs::write(&packed, filter(EXE, compress_args, &input)).unwrap();
    assert_eq!(
        data,
        filter(EXE, decompress_args, &packed),
        "roundtrip changed the data"
    );

    // Transparent writing does not use gzip format.
    if compress_args.contains(&"-T") || Command::new("gzip").arg("--version").output().is_err() {
        return;
    }
    assert_eq!(
        data,
        filter("gzip", &["-d"], &packed),
        "gzip read the output differently"
    );
    let from_gzip = dir.join("lorem-gzip.gz");
    fs::write(&from_gzip, filter("gzip", &["--stdout"], &input)).unwrap();
    assert_eq!(
        data,
        filter(EXE, decompress_args, &from_gzip),
        "gzip output read differently"
    );
}

const DECOMPRESS: &[&str] = &["-d", "-k", "-c"];

#[test]
fn stream_default() {
    compress_and_verify("default", &["-3", "-k", "-c"], DECOMPRESS);
}

#[test]
fn stream_level_1() {
    compress_and_verify("level-1", &["-1", "-k", "-c"], DECOMPRESS);
}

#[test]
fn stream_level_9() {
    compress_and_verify("level-9", &["-9", "-k", "-c"], DECOMPRESS);
}

#[test]
fn stream_rle() {
    compress_and_verify("rle", &["-U", "-k", "-c"], DECOMPRESS);
}

#[test]
fn stream_transparent() {
    compress_and_verify("transparent", &["-T", "-k", "-c"], DECOMPRESS);
}

#[test]
fn stream_blocks() {
    compress_and_verify("blocks", &["-b", "64K", "-k", "-c"], DECOMPRESS);
}

#[test]
fn stream_rsyncable() {
    compress_and_verify("rsyncable", &["--rsyncable", "-k", "-c"], DECOMPRESS);
}

#[test]
fn stream_rsyncable_blocks() {
    compress_and_verify(
        "rsyncable-blocks",
        &["--rsyncable", "-b", "64K", "-k", "-c"],
        DECOMPRESS,
    );
}

#[test]
fn stream_blocks_threads() {
    let compress = ["-b", "64K", "-p", "3", "-k", "-c"];
    compress_and_verify("blocks-threads", &compress, &["-d", "-p", "2", "-k", "-c"]);
}

// ===========================================================================
// The command on files

#[test]
fn in_place_consumes_the_input() {
    let dir = workdir("in-place");
    let (txt, gz) = (dir.join("data.txt"), dir.join("data.txt.gz"));

    // In place, the input is consumed in both directions.
    fs::write(&txt, data()).unwrap();
    assert!(gz!(txt).status.success());
    assert!(!txt.exists() && gz.exists(), "compress did not consume the input");
    assert!(gz!("-d", gz).status.success());
    assert!(!gz.exists(), "decompress did not consume the input");
    assert_eq!(data(), fs::read(&txt).unwrap(), "roundtrip changed the data");
}

#[test]
fn keep_and_stdout_keep_the_input() {
    let dir = workdir("keep");
    let txt = dir.join("k.txt");

    fs::write(&txt, data()).unwrap();
    assert!(gz!("-k", txt).status.success());
    assert!(
        txt.exists() && dir.join("k.txt.gz").exists(),
        "-k did not keep the input"
    );
    assert!(gz!("-c", txt).status.success());
    assert!(txt.exists(), "-c did not keep the input");
}

#[test]
fn transparent_copies_through() {
    let dir = workdir("transparent-file");
    let txt = dir.join("k.txt");

    // -T copies through untouched and decompression passes it back through.
    fs::write(&txt, data()).unwrap();
    let out = gz!("-T", "-c", txt);
    assert!(out.status.success());
    assert_eq!(data(), out.stdout, "-T did not copy through");
}

#[test]
fn thread_count_does_not_change_the_bytes() {
    let dir = workdir("threads-bytes");
    let txt = dir.join("k.txt");

    fs::write(&txt, data()).unwrap();
    let (p1, p3) = (
        gz!("-b", "64K", "-p", "1", "-c", txt),
        gz!("-b", "64K", "-p", "3", "-c", txt),
    );
    assert!(p1.status.success() && p3.status.success());
    assert_eq!(p1.stdout, p3.stdout, "thread count changed the compressed bytes");
}

#[test]
fn existing_output_needs_force() {
    let dir = workdir("force");
    let (txt, gz) = (dir.join("o.txt"), dir.join("o.txt.gz"));

    // Without --force an existing output is refused with a warning, --force overwrites it.
    fs::write(&txt, data()).unwrap();
    fs::write(&gz, "junk").unwrap();
    assert_eq!(Some(2), gz!("-k", txt).status.code());
    assert_eq!(
        b"junk",
        &fs::read(&gz).unwrap()[..],
        "existing output was overwritten without -f"
    );
    assert!(gz!("-f", "-k", txt).status.success());
    let out = gz!("-d", "-c", gz);
    assert!(out.status.success());
    assert_eq!(data(), out.stdout, "-f overwrite roundtrip failed");
}

#[test]
fn recursive_walks_directories() {
    let dir = workdir("recursive");
    let tree = dir.join("tree");

    // --recursive walks directories, compressing then restoring, a bare directory is only a
    // warning.
    fs::create_dir_all(tree.join("sub")).unwrap();
    fs::write(tree.join("a.txt"), data()).unwrap();
    fs::write(tree.join("sub/b.txt"), data()).unwrap();
    assert_eq!(Some(2), gz!(tree).status.code(), "directory without -r should warn");
    assert!(gz!("-r", tree).status.success());
    assert!(
        tree.join("a.txt.gz").exists() && tree.join("sub/b.txt.gz").exists(),
        "-r compress walk failed"
    );
    assert!(!tree.join("a.txt").exists());
    assert!(gz!("-d", "-r", tree).status.success());
    assert_eq!(data(), fs::read(tree.join("sub/b.txt")).unwrap(), "-d -r walk failed");
    assert!(!tree.join("a.txt.gz").exists());
}

#[test]
fn stored_name_comes_back() {
    let dir = workdir("stored-name");

    // The stored name comes back with --name, and --no-name stores nothing.
    fs::write(dir.join("named.txt"), data()).unwrap();
    assert!(gz!("-k", dir.join("named.txt")).status.success());
    fs::rename(dir.join("named.txt.gz"), dir.join("moved.gz")).unwrap();
    fs::remove_file(dir.join("named.txt")).unwrap();
    assert!(gz!("-d", "-N", dir.join("moved.gz")).status.success());
    assert!(dir.join("named.txt").exists(), "-N did not restore the stored name");

    fs::write(dir.join("anon.txt"), data()).unwrap();
    assert!(gz!("-n", "-k", dir.join("anon.txt")).status.success());
    fs::rename(dir.join("anon.txt.gz"), dir.join("plain.gz")).unwrap();
    assert!(gz!("-d", "-N", "-k", dir.join("plain.gz")).status.success());
    assert!(dir.join("plain").exists(), "-n left a name in the header");
}

#[test]
fn stored_time_comes_back() {
    let dir = workdir("stored-time");
    let (txt, gz) = (dir.join("timed.txt"), dir.join("timed.txt.gz"));
    let then = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000_000);

    // The output takes the input's time, and --name restores it from the header on the way back
    // whatever the compressed file's own time has become.
    fs::write(&txt, data()).unwrap();
    File::options()
        .write(true)
        .open(&txt)
        .unwrap()
        .set_modified(then)
        .unwrap();
    assert!(gz!(txt).status.success());
    assert_eq!(
        then,
        fs::metadata(&gz).unwrap().modified().unwrap(),
        "the output did not take the time"
    );
    File::options()
        .write(true)
        .open(&gz)
        .unwrap()
        .set_modified(then + std::time::Duration::from_secs(86400))
        .unwrap();
    assert!(gz!("-d", "-N", gz).status.success());
    assert_eq!(
        then,
        fs::metadata(&txt).unwrap().modified().unwrap(),
        "-N did not restore the stored time"
    );
}

#[test]
fn list_reports_size_and_name() {
    let dir = workdir("list");
    let txt = dir.join("listed.txt");

    // --list reports the trailer size and the derived name.
    fs::write(&txt, data()).unwrap();
    assert!(gz!("-k", txt).status.success());
    let out = gz!("-l", dir.join("listed.txt.gz"));
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    assert!(
        listing.contains(&data().len().to_string()),
        "-l listing was wrong: {listing}"
    );
    assert!(listing.contains("listed.txt"), "-l listing was wrong: {listing}");
}

#[test]
fn test_checks_integrity() {
    let dir = workdir("test");
    let txt = dir.join("listed.txt");

    // --test checks integrity without writing anything.
    fs::write(&txt, data()).unwrap();
    assert!(gz!("-k", txt).status.success());
    fs::write(dir.join("junk.gz"), "this is not gzip data at all").unwrap();
    assert!(gz!("-t", dir.join("listed.txt.gz")).status.success());
    assert!(!gz!("-t", dir.join("junk.gz")).status.success());
    assert!(dir.join("listed.txt.gz").exists());
}

#[test]
fn test_fails_damaged_data() {
    let dir = workdir("test-damaged");
    let (txt, gz) = (dir.join("d.txt"), dir.join("d.txt.gz"));

    fs::write(&txt, data()).unwrap();
    for args in [&["-k", "-f"][..], &["-k", "-f", "-b", "64K"]] {
        let mut exe = Command::new(EXE);
        assert!(exe.args(args).arg(&txt).status().unwrap().success());
        let mut packed = fs::read(&gz).unwrap();
        let at = packed.len() / 2;
        packed[at] ^= 0x10;
        fs::write(&gz, &packed).unwrap();
        assert_eq!(Some(1), gz!("-t", gz).status.code(), "{args:?} damage went unnoticed");
        fs::write(&gz, &packed[..at]).unwrap();
        assert_eq!(
            Some(1),
            gz!("-t", gz).status.code(),
            "{args:?} truncation went unnoticed"
        );
    }
}

#[test]
fn synchronous_roundtrips() {
    let dir = workdir("synchronous");
    let txt = dir.join("sync.txt");

    // --synchronous roundtrips with the data intact.
    fs::write(&txt, data()).unwrap();
    assert!(gz!("--synchronous", txt).status.success());
    assert!(gz!("--synchronous", "-d", dir.join("sync.txt.gz")).status.success());
    assert_eq!(data(), fs::read(&txt).unwrap(), "--synchronous roundtrip failed");
}

#[test]
fn stdout_stores_the_name() {
    let dir = workdir("stdout-name");
    let txt = dir.join("keptname.txt");

    // --stdout stores the name too, gzip does, so the header carries it either way.
    fs::write(&txt, data()).unwrap();
    let out = gz!("-c", txt);
    assert!(out.status.success());
    assert!(
        hex(&out.stdout[..64]).contains(&hex(b"keptname.txt")),
        "-c did not store the file name"
    );
}

#[test]
fn no_time_is_reproducible() {
    let dir = workdir("no-time");

    // --no-time keeps the name and zeroes the time, so the same content compresses to the same
    // bytes.
    fs::create_dir_all(dir.join("t1")).unwrap();
    fs::create_dir_all(dir.join("t2")).unwrap();
    fs::write(dir.join("t1/same.txt"), data()).unwrap();
    fs::write(dir.join("t2/same.txt"), data()).unwrap();
    let (m1, m2) = (
        gz!("-m", "-c", dir.join("t1/same.txt")),
        gz!("-m", "-c", dir.join("t2/same.txt")),
    );
    assert!(m1.status.success() && m2.status.success());
    assert_eq!(m1.stdout, m2.stdout, "-m output was not reproducible");
    assert_eq!([0; 4], m1.stdout[4..8], "-m should zero the time");
    assert!(
        hex(&m1.stdout[..20]).contains(&hex(b"same.txt")),
        "-m should keep the name"
    );
}

#[test]
fn threads_ask_for_blocks() {
    let dir = workdir("threads-blocks");
    let (txt, gz) = (dir.join("threads.txt"), dir.join("threads.gz"));

    // Threads ask for blocks, and what comes out reads back with no flags at all.
    fs::write(&txt, data()).unwrap();
    let packed = gz!("-p", "4", "-c", txt);
    assert!(packed.status.success());
    fs::write(&gz, &packed.stdout).unwrap();
    let out = gz!("-d", "-c", gz);
    assert!(out.status.success());
    assert_eq!(data(), out.stdout, "-p did not roundtrip");
    assert!(
        hex(&packed.stdout).contains("0000ffff000000ffff"),
        "-p produced no block boundaries"
    );
}

#[test]
fn every_path_writes_the_same_header() {
    let dir = workdir("os-byte");
    let txt = dir.join("os.txt");
    // Every path writes the same header. Left to itself zlib-ng stamps a platform OS code that
    // RFC 1952 does not define, so --no-name on a plain stream once disagreed with block output.
    // The code itself is the platform's, 0 on Windows and 3 elsewhere.
    let os_want = if cfg!(windows) { 0 } else { 3 };

    fs::write(&txt, data()).unwrap();
    for variant in [&["-n"][..], &["-n", "-b", "64K"], &["-n", "-p", "2"]] {
        let mut exe = Command::new(EXE);
        let out = exe.args(variant).arg("-c").arg(&txt).output().unwrap();
        assert!(out.status.success());
        assert_eq!(os_want, out.stdout[9], "{variant:?} wrote the wrong OS byte");
    }
}

/// A directory holding one compressed file and another a level down.
fn compressed_tree(dir: &Path) -> PathBuf {
    let tree = dir.join("ttree");

    fs::create_dir_all(tree.join("sub")).unwrap();
    fs::write(tree.join("one.txt"), data()).unwrap();
    fs::write(tree.join("sub/two.txt"), data()).unwrap();
    assert!(gz!("-r", tree).status.success());
    tree
}

#[test]
fn test_reaches_directories() {
    let tree = compressed_tree(&workdir("test-tree"));

    // --test reaches a directory the same way the other modes do, walking it under --recursive
    // and reporting it as a warning without.
    assert!(
        gz!("-t", "-r", tree).status.success(),
        "-t -r did not walk the directory"
    );
    assert_eq!(
        Some(2),
        gz!("-t", tree).status.code(),
        "-t on a directory without -r should warn"
    );
}

#[test]
fn test_fails_plain_stdin() {
    let dir = workdir("test-stdin");
    let orig = dir.join("data.orig");

    // --test fails input that is not gzip from stdin as well, where nothing may pass through.
    fs::write(&orig, data()).unwrap();
    let out = command(EXE, &[OsStr::new("-t")], Some(&orig));
    assert!(!out.status.success(), "--test passed plain input on stdin");
    assert!(out.stdout.is_empty());
}

#[test]
fn list_walks_directories() {
    let tree = compressed_tree(&workdir("list-tree"));

    // --list walks a directory under --recursive too, listing every suffixed file in it.
    let out = gz!("-l", "-r", tree);
    let listing = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success());
    for name in [
        Path::new("ttree").join("one.txt"),
        Path::new("ttree").join("sub").join("two.txt"),
    ] {
        assert!(
            listing.contains(&*name.to_string_lossy()),
            "-l -r did not list the directory: {listing}"
        );
    }
    assert!(listing.contains("(totals)"), "two files make a totals row: {listing}");
}

// Windows names the program with its suffix, which no persona answers to.
#[cfg(unix)]
#[test]
fn personas_follow_the_program_name() {
    let dir = workdir("personas");
    let (txt, gz) = (dir.join("p.txt"), dir.join("p.txt.gz"));

    // Installed as gunzip or zcat the command presets what gzip's aliases do.
    fs::write(&txt, data()).unwrap();
    assert!(gz!("-k", txt).status.success());
    std::os::unix::fs::symlink(EXE, dir.join("zcat")).unwrap();
    std::os::unix::fs::symlink(EXE, dir.join("gunzip")).unwrap();
    let out = command(dir.join("zcat"), &[gz.as_os_str()], None);
    assert!(out.status.success());
    assert_eq!(data(), out.stdout, "zcat did not decompress to stdout");
    assert!(gz.exists(), "zcat consumed its input");
    fs::remove_file(&txt).unwrap();
    assert!(command(dir.join("gunzip"), &[gz.as_os_str()], None).status.success());
    assert!(!gz.exists(), "gunzip kept its input");
    assert_eq!(data(), fs::read(&txt).unwrap(), "gunzip did not decompress in place");
}

#[test]
fn bad_command_lines_fail() {
    assert_eq!(Some(1), gz!("-x").status.code());
    assert_eq!(Some(1), gz!("--nonsense").status.code());
    assert_eq!(Some(1), gz!("-l").status.code(), "-l needs file arguments");
    assert_eq!(Some(1), gz!("no-such-file-anywhere").status.code());
    let help = gz!("--help");
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("--blocksize size"));
    let version = gz!("--version");
    assert!(String::from_utf8_lossy(&version.stdout).starts_with(&format!("gzip-ng {} (zlib-ng ", gzng::VERSION)));
}
