// options.rs -- gzip-ng command line options
// For conditions of distribution and use, see LICENSE.md

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;

use gzng::Strategy;
use gzng::gzblock::MAX_BLOCK;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// --decompress
    pub decompress: bool,
    /// --stdout
    pub stdout_mode: bool,
    /// --keep
    pub keep: bool,
    /// --force, overwrite outputs, write compressed data to a terminal
    pub force: bool,
    /// --recursive, descend into directories
    pub recursive: bool,
    /// --verbose, report each file processed
    pub verbose: bool,
    /// --quiet, suppress warnings
    pub quiet: bool,
    /// None by default, false with --no-name, true with --name
    pub name_mode: Option<bool>,
    /// None by default, false with --no-name or --no-time, true with --name or --time
    pub time_mode: Option<bool>,
    /// --list, list instead of decompressing
    pub list: bool,
    /// --test, check integrity without writing
    pub test_mode: bool,
    /// --rsyncable, rsync friendly output
    pub rsyncable: bool,
    /// --synchronous, fsync outputs before removing inputs
    pub synchronous: bool,
    /// -0 to -9
    pub level: i32,
    /// deflate strategy options
    pub strategy: Strategy,
    /// -T, copy without compressing
    pub transparent: bool,
    /// -A, no effect where text and binary io agree
    pub text_mode: bool,
    /// --blocksize, 0 writes one plain deflate stream
    pub block_size: u32,
    /// --processes, 0 picks the number of CPUs
    pub threads: usize,
    /// --processes was given, threads of 0 means every CPU, not the default
    pub threads_given: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            decompress: false,
            stdout_mode: false,
            keep: false,
            force: false,
            recursive: false,
            verbose: false,
            quiet: false,
            name_mode: None,
            time_mode: None,
            list: false,
            test_mode: false,
            rsyncable: false,
            synchronous: false,
            level: gzng::DEFAULT_LEVEL,
            strategy: Strategy::Default,
            transparent: false,
            text_mode: false,
            block_size: 0,
            threads: 0,
            threads_given: false,
        }
    }
}

/// What a command line that parsed asks for.
#[derive(Debug, PartialEq, Eq)]
pub enum Parsed {
    /// Process these files, the arguments that were not options, in order.
    Files(Vec<OsString>),
    /// The run is already complete, --help or --version.
    Done,
}

/// Parse a size with an optional K, M, or G suffix, a bare number counting KiB. Returns 0 when it
/// is not usable.
pub fn parse_size(arg: &str) -> u32 {
    let digits = arg.bytes().take_while(u8::is_ascii_digit).count();
    let shift = match &arg[digits..] {
        "" | "k" | "K" => 10, // a bare number counts KiB, the way pigz reads it
        "m" | "M" => 20,
        "g" | "G" => 30,
        _ => return 0,
    };

    // The bound is checked before the shift, which could otherwise wrap a huge count small.
    match arg[..digits].parse::<u64>() {
        Ok(v) if v != 0 && v <= u64::from(MAX_BLOCK >> shift) => (v << shift) as u32,
        _ => 0,
    }
}

// ===========================================================================
// Option table

#[derive(Clone, Copy, PartialEq, Eq)]
enum Opt {
    Stdout,
    Decompress,
    Force,
    Keep,
    Recursive,
    Verbose,
    Quiet,
    NoName,
    Name,
    NoTime,
    Time,
    List,
    Test,
    Rsyncable,
    Synchronous,
    Huffman,
    Rle,
    Filtered,
    Fixed,
    Transparent,
    Ascii,
    Independent,
    Blocksize,
    Processes,
    Fast,
    Best,
    Help,
    Version,
    License,
}

/// One row per option, in the order the help lists them. A row carries every spelling the option
/// answers to, so a spelling cannot exist in the parser without appearing in the help.
struct OptionDesc {
    id: Opt,
    /// None when there is no short form.
    letter: Option<char>,
    /// Long form without the dashes, None when there is none.
    name: Option<&'static str>,
    /// Second long spelling, or None.
    alias: Option<&'static str>,
    /// What the value is called, None when the option takes none.
    value: Option<&'static str>,
    help: &'static str,
}

const fn row(
    id: Opt,
    letter: Option<char>,
    name: Option<&'static str>,
    alias: Option<&'static str>,
    value: Option<&'static str>,
    help: &'static str,
) -> OptionDesc {
    OptionDesc {
        id,
        letter,
        name,
        alias,
        value,
        help,
    }
}

#[rustfmt::skip]
const OPTION_TABLE: &[OptionDesc] = &[
    row(Opt::Stdout,      Some('c'), Some("stdout"),      Some("to-stdout"),  None,         "write to standard output, keep the files"),
    row(Opt::Decompress,  Some('d'), Some("decompress"),  Some("uncompress"), None,         "decompress"),
    row(Opt::Force,       Some('f'), Some("force"),       None,               None,         "overwrite outputs, compress to a terminal"),
    row(Opt::Keep,        Some('k'), Some("keep"),        None,               None,         "keep input files"),
    row(Opt::Recursive,   Some('r'), Some("recursive"),   None,               None,         "descend into directories"),
    row(Opt::Verbose,     Some('v'), Some("verbose"),     None,               None,         "report each file processed"),
    row(Opt::Quiet,       Some('q'), Some("quiet"),       None,               None,         "suppress warnings"),
    row(Opt::NoName,      Some('n'), Some("no-name"),     None,               None,         "do not save or restore name and time"),
    row(Opt::Name,        Some('N'), Some("name"),        None,               None,         "save and restore name and time"),
    row(Opt::NoTime,      Some('m'), Some("no-time"),     None,               None,         "do not save or restore the time"),
    row(Opt::Time,        Some('M'), Some("time"),        None,               None,         "save and restore the time"),
    row(Opt::List,        Some('l'), Some("list"),        None,               None,         "list compressed file contents"),
    row(Opt::Test,        Some('t'), Some("test"),        None,               None,         "check integrity without writing"),
    row(Opt::Rsyncable,   None,      Some("rsyncable"),   None,               None,         "make a plain stream rsync friendly, blocks always are"),
    row(Opt::Synchronous, None,      Some("synchronous"), None,               None,         "write the output to storage before removing the input"),
    row(Opt::Huffman,     Some('H'), Some("huffman"),     None,               None,         "huffman only strategy"),
    row(Opt::Rle,         Some('U'), Some("rle"),         None,               None,         "run length strategy"),
    row(Opt::Filtered,    None,      Some("filtered"),    None,               None,         "filtered strategy"),
    row(Opt::Fixed,       None,      Some("fixed"),       None,               None,         "fixed codes strategy"),
    row(Opt::Transparent, Some('T'), None,                None,               None,         "store without compressing"),
    row(Opt::Ascii,       Some('A'), None,                None,               None,         "text mode, accepted for compatibility"),
    row(Opt::Independent, Some('i'), Some("independent"), None,               None,         "accepted for compatibility, blocks always are"),
    row(Opt::Blocksize,   Some('b'), Some("blocksize"),   None,               Some("size"), "average block size in KiB, or with a K, M, or G suffix"),
    row(Opt::Processes,   Some('p'), Some("processes"),   None,               Some("n"),    "threads to use, which asks for blocks, 0 picks the number of CPUs"),
    row(Opt::Fast,        None,      Some("fast"),        None,               None,         "compress faster, level 1"),
    row(Opt::Best,        None,      Some("best"),        None,               None,         "compress better, level 9"),
    row(Opt::Help,        Some('h'), Some("help"),        None,               None,         "show this help"),
    row(Opt::Version,     Some('V'), Some("version"),     None,               None,         "show the version"),
    row(Opt::License,     Some('L'), Some("license"),     None,               None,         "show the license"),
];

/// The left column of a help line, "-c --stdout" or "   --rsyncable" or "-b --blocksize size".
fn spellings(desc: &OptionDesc) -> String {
    let mut buf = match desc.letter {
        Some(letter) => format!("-{letter}"),
        None => "  ".to_string(),
    };

    if let Some(name) = desc.name {
        buf += &format!(" --{name}");
    }
    if let Some(value) = desc.value {
        buf += &format!(" {value}");
    }
    buf
}

pub fn usage(out: &mut dyn Write) -> io::Result<()> {
    let width = OPTION_TABLE.iter().map(|desc| spellings(desc).len()).max().unwrap_or(0);

    writeln!(out, "Usage: gzip-ng [options] [files...]")?;
    writeln!(
        out,
        "Compresses files in place. With no files, or -, filters stdin to stdout.\n"
    )?;
    for desc in OPTION_TABLE {
        writeln!(out, "  {:<width$}  {}", spellings(desc), desc.help)?;
    }
    writeln!(
        out,
        "  {:<width$}  compression level, {} by default",
        "-1 .. -9",
        gzng::DEFAULT_LEVEL
    )
}

fn find_letter(letter: char) -> Option<&'static OptionDesc> {
    OPTION_TABLE.iter().find(|desc| desc.letter == Some(letter))
}

fn find_name(name: &str) -> Option<&'static OptionDesc> {
    OPTION_TABLE
        .iter()
        .find(|desc| desc.name == Some(name) || desc.alias == Some(name))
}

// ===========================================================================
// Parsing

fn show_license(out: &mut dyn Write) -> io::Result<()> {
    writeln!(out, "gzip-ng {}, zlib license\n", gzng::version())?;
    writeln!(
        out,
        "This software is provided 'as-is', without any express or implied\n\
         warranty. In no event will the authors be held liable for any damages\n\
         arising from the use of this software.\n\n\
         Permission is granted to anyone to use this software for any purpose,\n\
         including commercial applications, and to alter it and redistribute it\n\
         freely, subject to the restrictions in LICENSE.md."
    )
}

/// The message for a bad command line.
fn bad(prog: &str, what: &str, arg: &str) -> String {
    format!("{prog}: {what} {arg}\nTry {prog} --help for options.")
}

impl Options {
    /// Preset options from the program name, gunzip, zcat, and gzcat act as gzip's aliases do.
    pub fn personas(&mut self, argv0: &OsStr) {
        match Path::new(argv0).file_name().and_then(OsStr::to_str) {
            Some("gunzip") => self.decompress = true,
            Some("zcat" | "gzcat") => {
                self.decompress = true;
                self.stdout_mode = true;
            }
            _ => {}
        }
    }

    /// Carry out one option. Returns true when the run is already complete, or the message for a
    /// bad value.
    fn apply(&mut self, desc: &OptionDesc, value: Option<&str>, prog: &str) -> Result<bool, String> {
        match desc.id {
            Opt::Stdout => self.stdout_mode = true,
            Opt::Decompress => self.decompress = true,
            Opt::Force => self.force = true,
            Opt::Keep => self.keep = true,
            Opt::Recursive => self.recursive = true,
            Opt::Verbose => self.verbose = true,
            Opt::Quiet => {
                self.quiet = true;
                self.verbose = false;
            }
            Opt::NoName => {
                self.name_mode = Some(false);
                self.time_mode = Some(false);
            }
            Opt::Name => {
                self.name_mode = Some(true);
                self.time_mode = Some(true);
            }
            Opt::NoTime => self.time_mode = Some(false),
            Opt::Time => self.time_mode = Some(true),
            Opt::List => self.list = true,
            Opt::Test => {
                self.test_mode = true;
                self.decompress = true;
            }
            Opt::Rsyncable => self.rsyncable = true,
            Opt::Synchronous => self.synchronous = true,
            Opt::Huffman => self.strategy = Strategy::HuffmanOnly,
            Opt::Rle => self.strategy = Strategy::Rle,
            Opt::Filtered => self.strategy = Strategy::Filtered,
            Opt::Fixed => self.strategy = Strategy::Fixed,
            Opt::Transparent => self.transparent = true,
            Opt::Ascii => self.text_mode = true,
            Opt::Independent => {} // pigz --independent, what the writer does anyway
            Opt::Fast => self.level = 1,
            Opt::Best => self.level = 9,
            Opt::Blocksize => {
                self.block_size = value.map_or(0, parse_size);
                if self.block_size == 0 {
                    return Err(bad(prog, "bad block size", value.unwrap_or("(missing)")));
                }
            }
            Opt::Processes => match value.and_then(|value| value.parse().ok()) {
                Some(n) if n <= 1024 => {
                    self.threads = n;
                    self.threads_given = true;
                }
                _ => return Err(bad(prog, "bad thread count", value.unwrap_or("(missing)"))),
            },
            // A closed stdout is no reason to fail a run that only prints.
            Opt::Help => {
                let _ = usage(&mut io::stdout());
                return Ok(true);
            }
            Opt::License => {
                let _ = show_license(&mut io::stdout());
                return Ok(true);
            }
            Opt::Version => {
                let _ = writeln!(
                    io::stdout(),
                    "gzip-ng {} (zlib-ng {})",
                    gzng::version(),
                    gzng::zlibng_version()
                );
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// A long option, its value taken from the next argument when it wants one.
    fn parse_long<'a>(
        &mut self,
        prog: &str,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a OsString>,
    ) -> Result<bool, String> {
        let Some(desc) = find_name(&arg[2..]) else {
            return Err(bad(prog, "unknown option", arg));
        };
        let value = if desc.value.is_some() {
            rest.next().map(|value| value.to_string_lossy())
        } else {
            None
        };

        self.apply(desc, value.as_deref(), prog)
    }

    /// A run of short options, any one of which may take the rest of the argument or the next one
    /// as its value, which ends the run.
    fn parse_shorts<'a>(
        &mut self,
        prog: &str,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a OsString>,
    ) -> Result<bool, String> {
        for (at, letter) in arg.char_indices().skip(1) {
            if let Some(digit) = letter.to_digit(10) {
                self.level = digit as i32;
                continue;
            }
            let Some(desc) = find_letter(letter) else {
                return Err(bad(prog, "unknown option", &format!("-{letter}")));
            };
            if desc.value.is_some() {
                let attached = &arg[at + letter.len_utf8()..];
                if !attached.is_empty() {
                    return self.apply(desc, Some(attached), prog);
                }
                let value = rest.next().map(|value| value.to_string_lossy());
                return self.apply(desc, value.as_deref(), prog);
            }
            if self.apply(desc, None, prog)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Parse the command line the way gzip does, options and files in any order, short options
    /// clustered, values attached or separate, -- ending the options. args starts with the
    /// program name. Returns the file arguments, or that the run is already complete, or the
    /// message for a bad command line.
    pub fn parse(&mut self, args: &[OsString]) -> Result<Parsed, String> {
        let prog = args.first().map(|prog| prog.to_string_lossy()).unwrap_or_default();
        let mut rest = args.iter().skip(1);
        let mut files = Vec::new();
        let mut no_more = false;

        while let Some(os_arg) = rest.next() {
            let bytes = os_arg.as_encoded_bytes();

            if no_more || bytes.first() != Some(&b'-') || bytes.len() == 1 {
                files.push(os_arg.clone());
                continue;
            }
            let arg = os_arg.to_string_lossy();
            let done = if bytes[1] == b'-' {
                if bytes.len() == 2 {
                    no_more = true;
                    continue;
                }
                self.parse_long(&prog, &arg, &mut rest)?
            } else {
                self.parse_shorts(&prog, &arg, &mut rest)?
            };
            if done {
                return Ok(Parsed::Done);
            }
        }
        Ok(Parsed::Files(files))
    }
}
