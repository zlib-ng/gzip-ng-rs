// tests.rs -- the command line parser
// For conditions of distribution and use, see LICENSE.md

use std::ffi::OsString;

use super::*;

/// The arguments as main hands them over, behind the program name.
fn args(list: &[&str]) -> Vec<OsString> {
    std::iter::once("gzip-ng")
        .chain(list.iter().copied())
        .map(OsString::from)
        .collect()
}

fn parse(list: &[&str]) -> (Options, Result<Parsed, String>) {
    let mut opt = Options::default();
    let parsed = opt.parse(&args(list));
    (opt, parsed)
}

/// The options a command line that parsed sets, with the files it names.
fn parsed(list: &[&str]) -> (Options, Vec<OsString>) {
    match parse(list) {
        (opt, Ok(Parsed::Files(files))) => (opt, files),
        (_, other) => panic!("{list:?} parsed to {other:?}"),
    }
}

#[test]
fn defaults() {
    let opt = Options::default();
    assert!(!opt.decompress);
    assert_eq!(7, opt.level);
    assert_eq!(0, opt.block_size);
    assert_eq!(0, opt.threads);
}

#[test]
fn files_and_options_any_order() {
    let (opt, files) = parsed(&["one", "-k", "two", "-d"]);
    assert_eq!(vec![OsString::from("one"), OsString::from("two")], files);
    assert!(opt.keep);
    assert!(opt.decompress);
}

#[test]
fn clustered_shorts() {
    let (opt, _) = parsed(&["-dck9"]);
    assert!(opt.decompress);
    assert!(opt.stdout_mode);
    assert!(opt.keep);
    assert_eq!(9, opt.level);
}

#[test]
fn attached_and_separate_values() {
    let (opt, _) = parsed(&["-p8", "-b", "64K"]);
    assert_eq!(8, opt.threads);
    assert_eq!(64 * 1024, opt.block_size);
}

#[test]
fn long_options() {
    let (opt, _) = parsed(&[
        "--decompress",
        "--stdout",
        "--keep",
        "--best",
        "--blocksize",
        "128K",
        "--processes",
        "4",
    ]);
    assert!(opt.decompress);
    assert!(opt.stdout_mode);
    assert!(opt.keep);
    assert_eq!(9, opt.level);
    assert_eq!(128 * 1024, opt.block_size);
    assert_eq!(4, opt.threads);
}

#[test]
fn double_dash_ends_options() {
    let (opt, files) = parsed(&["--", "-k"]);
    assert_eq!(vec![OsString::from("-k")], files);
    assert!(!opt.keep);
}

#[test]
fn dash_is_a_file() {
    let (_, files) = parsed(&["-"]);
    assert_eq!(vec![OsString::from("-")], files);
}

#[test]
fn force_flag() {
    assert!(parsed(&["-f"]).0.force);
    assert!(parsed(&["--force"]).0.force);
}

#[test]
fn help_short() {
    assert_eq!(Ok(Parsed::Done), parse(&["-h"]).1);
}

#[test]
fn synchronous_flag() {
    assert!(parsed(&["--synchronous"]).0.synchronous);
}

#[test]
fn rsyncable_flag() {
    assert!(parsed(&["--rsyncable"]).0.rsyncable);
}

#[test]
fn test_flag() {
    let (opt, _) = parsed(&["-t"]);
    assert!(opt.test_mode);
    assert!(opt.decompress);
}

#[test]
fn list_flag() {
    assert!(parsed(&["-l"]).0.list);
}

#[test]
fn name_modes() {
    assert_eq!(Some(false), parsed(&["-n"]).0.name_mode);
    assert_eq!(Some(true), parsed(&["--name"]).0.name_mode);
}

#[test]
fn time_modes() {
    let (opt, _) = parsed(&["-m"]);
    assert_eq!(Some(false), opt.time_mode);
    assert_eq!(None, opt.name_mode, "-m leaves the stored name alone");
    assert_eq!(Some(true), parsed(&["--time"]).0.time_mode);
    let (opt, _) = parsed(&["-n"]);
    assert_eq!(Some(false), opt.name_mode);
    assert_eq!(Some(false), opt.time_mode, "-n drops the time with the name");
}

#[test]
fn quiet_flag() {
    let (opt, _) = parsed(&["-v", "-q"]);
    assert!(opt.quiet);
    assert!(!opt.verbose);
}

#[test]
fn verbose_flag() {
    assert!(parsed(&["-v"]).0.verbose);
}

#[test]
fn recursive_flag() {
    assert!(parsed(&["-r"]).0.recursive);
}

#[test]
fn license() {
    assert_eq!(Ok(Parsed::Done), parse(&["-L"]).1);
}

#[test]
fn version_short() {
    assert_eq!(Ok(Parsed::Done), parse(&["-V"]).1);
}

#[test]
fn unknown_option_fails() {
    assert!(parse(&["-x"]).1.is_err());
    assert!(parse(&["--nonsense"]).1.is_err());
}

#[test]
fn bad_values_fail() {
    assert!(parse(&["-b"]).1.is_err(), "a block size has to follow");
    assert!(parse(&["-b", "0"]).1.is_err());
    assert!(parse(&["-p", "many"]).1.is_err());
    assert!(parse(&["-p", "1025"]).1.is_err());
    assert!(parse(&["-p-1"]).1.is_err());
}

#[test]
fn strategies() {
    let cases = [
        ("--filtered", Strategy::Filtered),
        ("-H", Strategy::HuffmanOnly),
        ("-U", Strategy::Rle),
        ("--fixed", Strategy::Fixed),
    ];
    for (arg, strategy) in cases {
        assert_eq!(strategy, parsed(&[arg]).0.strategy, "{arg}");
    }
}

#[test]
fn transparent_and_text() {
    let (opt, _) = parsed(&["-T", "-A"]);
    assert!(opt.transparent);
    assert!(opt.text_mode);
}

#[test]
fn size_parses() {
    assert_eq!(131072, parse_size("128K"));
    assert_eq!(1 << 20, parse_size("1M"));
    assert_eq!(0, parse_size("1G")); // over the engine cap
    assert_eq!(256 << 20, parse_size("256M"));
    assert_eq!(131072, parse_size("128"), "a bare number counts KiB");
    assert_eq!(4096 << 10, parse_size("4096"));
    assert_eq!(0, parse_size("262145"));
    assert_eq!(0, parse_size("18014398509481985K"), "a count that would wrap to 1K");
    assert_eq!(0, parse_size("0"));
    assert_eq!(0, parse_size("x"));
    assert_eq!(0, parse_size("12KB"));
    assert_eq!(0, parse_size("512M"));
}

#[test]
fn personas() {
    let mut opt = Options::default();
    opt.personas("/usr/bin/gunzip".as_ref());
    assert!(opt.decompress);
    assert!(!opt.stdout_mode);
    let mut opt = Options::default();
    opt.personas("zcat".as_ref());
    assert!(opt.decompress);
    assert!(opt.stdout_mode);
}
