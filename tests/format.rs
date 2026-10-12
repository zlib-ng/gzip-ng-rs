// format.rs -- the gzip header and trailer
// For conditions of distribution and use, see LICENSE.md

use gzng::format::{self, HEADER_MAX, Header, NAME_MAX, Parse};

fn basic<'a>() -> Header<'a> {
    Header {
        level: 6,
        ..Header::default()
    }
}

#[test]
fn build_parses_back() {
    let mut buf = [0; HEADER_MAX];
    let h = Header {
        mtime: 0x5f6a_7b8c,
        name: Some(b"some/file.txt"),
        ..basic()
    };

    let n = format::header_build(&mut buf, &h);
    assert!(n <= buf.len());
    assert_eq!(Parse::Complete(n), format::header_parse(&buf[..n]).0);
    assert_eq!([0x8c, 0x7b, 0x6a, 0x5f], buf[4..8]);
    assert_eq!(b"some/file.txt\0", &buf[n - 14..n]);
}

#[test]
fn a_bare_header_is_ten_bytes() {
    let mut buf = [0; HEADER_MAX];

    let n = format::header_build(&mut buf, &basic());
    assert_eq!(10, n);
    assert_eq!(0, buf[3], "no flags, so nothing follows the fixed bytes");
    assert_eq!(Parse::Complete(n), format::header_parse(&buf[..n]).0);
}

#[test]
fn a_name_too_long_is_left_out() {
    let mut buf = [0; HEADER_MAX];
    let huge = vec![b'x'; NAME_MAX + 10];
    let h = Header {
        name: Some(&huge),
        ..basic()
    };

    let n = format::header_build(&mut buf, &h);
    assert_eq!(10, n);
    assert_eq!(0, buf[3] & 8);
}

#[test]
fn the_longest_name_fits() {
    let mut buf = [0; HEADER_MAX];
    let long = vec![b'x'; NAME_MAX - 1];
    let h = Header {
        name: Some(&long),
        ..basic()
    };

    let n = format::header_build(&mut buf, &h);
    assert_eq!(HEADER_MAX, n);
    assert_eq!(Some(&long[..]), format::header_parse(&buf[..n]).1.name);
}

#[test]
fn partial_headers_ask_for_more() {
    let mut buf = [0; HEADER_MAX];
    let h = Header {
        name: Some(b"f"),
        ..basic()
    };
    let n = format::header_build(&mut buf, &h);

    for partial in 1..n {
        assert_eq!(Parse::More, format::header_parse(&buf[..partial]).0, "{partial}");
    }
    assert_eq!(Parse::Complete(n), format::header_parse(&buf[..n]).0);
}

#[test]
fn an_extra_field_is_walked_over() {
    // Someone else's subfield, which a header may carry and this one has no use for.
    let withextra = [0x1f, 0x8b, 8, 4, 0, 0, 0, 0, 0, 3, 6, 0, b'A', b'p', 2, 0, 0x11, 0x22];
    let (parse, hdr) = format::header_parse(&withextra);
    assert_eq!(Parse::Complete(withextra.len()), parse);
    assert_eq!(0, hdr.member_size);
}

#[test]
fn sized_members_report_their_length() {
    // BGZF counts the whole member, less one.
    let bgzf = [0x1f, 0x8b, 8, 4, 0, 0, 0, 0, 0, 3, 6, 0, b'B', b'C', 2, 0, 0x11, 0x22];
    let (parse, hdr) = format::header_parse(&bgzf);
    assert_eq!(Parse::Complete(bgzf.len()), parse);
    assert_eq!(0x2211 + 1, hdr.member_size);

    // MiGz counts only the deflate data, the header and trailer complete the member.
    let migz = [
        0x1f, 0x8b, 8, 4, 0, 0, 0, 0, 0, 3, 8, 0, b'M', b'Z', 4, 0, 0x00, 0x01, 0, 0,
    ];
    let (parse, hdr) = format::header_parse(&migz);
    assert_eq!(Parse::Complete(migz.len()), parse);
    assert_eq!(migz.len() as u32 + 0x100 + 8, hdr.member_size);
}

#[test]
fn parse_reports_the_time_and_name() {
    let mut buf = [0; HEADER_MAX];
    let h = Header {
        mtime: 0x5f6a_7b8c,
        name: Some(b"some/file.txt"),
        ..basic()
    };

    let n = format::header_build(&mut buf, &h);
    let (parse, got) = format::header_parse(&buf[..n]);
    assert_eq!(Parse::Complete(n), parse);
    assert_eq!(0x5f6a_7b8c, got.mtime);
    assert_eq!(Some(&b"some/file.txt"[..]), got.name);
    // The fixed bytes give up the time before the name has fully arrived.
    let (parse, got) = format::header_parse(&buf[..n - 1]);
    assert_eq!(Parse::More, parse);
    assert_eq!(0x5f6a_7b8c, got.mtime);
    assert_eq!(None, got.name);
}

#[test]
fn two_bytes_decide_the_magic() {
    let gz = [0x1f, 0x8b];
    assert!(format::is_gzip(&gz));
    assert!(!format::is_gzip(b"no"));
    assert!(!format::is_gzip(&gz[..1]), "one byte cannot say");
}

#[test]
fn other_data_is_rejected() {
    let junk = *b"not gzip\0\0\0\0\0\0\0\0";
    assert_eq!(Parse::Invalid, format::header_parse(&junk).0);
}

#[test]
fn values_are_stored_little_endian() {
    // The order is the format's, not the machine's, so it is spelled out here.
    let buf = format::trailer_build(0x1122_3344, 0xaabb_ccdd);

    assert_eq!([0x44, 0x33, 0x22, 0x11, 0xdd, 0xcc, 0xbb, 0xaa], buf);
    assert_eq!((0x1122_3344, 0xaabb_ccdd), format::trailer_parse(&buf));
}

#[test]
fn trailer_round_trips() {
    let buf = format::trailer_build(0xdead_beef, 0x1_0000_0007);
    let (crc, total) = format::trailer_parse(&buf);

    assert_eq!(0xdead_beef, crc);
    assert_eq!(7, total, "the length is recorded modulo 2^32");
}
