// writer.rs -- the block writer
// For conditions of distribution and use, see LICENSE.md

mod common;

use common::{block_compress, block_read, sample_data, varied_data, whole_inflate};
use gzng::format::{self, Parse};
use gzng::gzblock::{Code, Writer};
use gzng::{DEFAULT_COMPRESSION, Strategy};

#[test]
fn header_is_an_ordinary_gzip_header() {
    let data = sample_data(200_000);
    let packed = block_compress(&data, 64 * 1024, 1);
    // Nothing marks the member as cut into blocks, a reader finds that out by looking.
    assert_eq!(Parse::Complete(10), format::header_parse(&packed).0);
    assert_eq!(0, packed[3] & 4, "no extra field");
}

#[test]
fn output_inflates_back() {
    let data = sample_data(1 << 20);
    let packed = block_compress(&data, 64 * 1024, 1);
    assert_eq!(data, whole_inflate(&packed, data.len()));
}

#[test]
fn threads_do_not_change_the_bytes() {
    let data = sample_data(1 << 20);
    let serial = block_compress(&data, 64 * 1024, 1);
    let threaded = block_compress(&data, 64 * 1024, 3);
    assert_eq!(serial, threaded);
}

#[test]
fn flush_and_params_inside_a_block() {
    let data = sample_data(300_000);
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, DEFAULT_COMPRESSION, Strategy::Default, 64 * 1024, 2).unwrap();
    w.write(&data[..100_000]).unwrap();
    w.flush().unwrap();
    w.set_params(1, Strategy::Default).unwrap();
    w.write(&data[100_000..]).unwrap();
    w.finish().unwrap();
    drop(w);
    assert_eq!(data, whole_inflate(&out, data.len()));
}

#[test]
fn meta_lands_in_the_header() {
    let data = sample_data(1000);
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, 6, Strategy::Default, 64 * 1024, 1).unwrap();
    w.meta(12345, Some(b"hello.txt")).unwrap();
    w.write(&data).unwrap();
    w.finish().unwrap();
    drop(w);
    assert!(out.len() > 31);
    assert_eq!(8, out[3], "the name flag, and nothing else");
    assert_eq!([0x39, 0x30, 0, 0], out[4..8]);
    assert_eq!(b"hello.txt\0", &out[10..20], "the name follows the fixed bytes");
    assert_eq!(data, block_read(&out, 1));
}

#[test]
fn settings_come_before_the_first_write() {
    let data = varied_data(300_000);
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, 6, Strategy::Default, 64 * 1024, 1).unwrap();
    w.write(&data).unwrap();
    assert!(w.rsyncable(true).is_err(), "the pool cannot be resized under a block");
    w.flush().unwrap();
    assert!(w.meta(1, None).is_err(), "the header is out");
    w.finish().unwrap();
    assert!(w.write(&data).is_err(), "the member is finished");
    w.finish().unwrap();
    drop(w);
    assert_eq!(data, whole_inflate(&out, data.len()));
}

#[test]
fn a_bad_block_size_is_refused() {
    assert!(Writer::new(Vec::new(), 6, Strategy::Default, 0, 1).is_err());
    assert!(Writer::new(Vec::new(), 6, Strategy::Default, gzng::gzblock::MAX_BLOCK + 1, 1).is_err());
}

#[test]
fn a_bad_level_is_refused() {
    for level in [-2, 10, 42] {
        let err = Writer::new(Vec::new(), level, Strategy::Default, 64 * 1024, 2)
            .err()
            .unwrap();
        assert_eq!(Code::Stream, err.code(), "level {level}: {err}");
    }

    // A refused change leaves the writer as it was.
    let data = sample_data(200_000);
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, 6, Strategy::Default, 64 * 1024, 2).unwrap();
    w.write(&data[..100_000]).unwrap();
    assert_eq!(Code::Stream, w.set_params(42, Strategy::Default).unwrap_err().code());
    w.write(&data[100_000..]).unwrap();
    w.finish().unwrap();
    drop(w);
    assert_eq!(data, whole_inflate(&out, data.len()));
}

fn rsync_compress(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, 6, Strategy::Default, 64 * 1024, 1).unwrap();
    w.rsyncable(true).unwrap();
    w.write(data).unwrap();
    w.finish().unwrap();
    drop(w);
    out
}

#[test]
fn rsyncable_realigns_after_an_edit() {
    let v1 = varied_data(4 << 20);
    let mut v2 = v1.clone();
    let at = v2.len() / 4;
    v2.splice(at..at, [0x55; 100]);

    let p1 = rsync_compress(&v1);
    let p2 = rsync_compress(&v2);
    assert_eq!(v1, block_read(&p1, 3));
    assert_eq!(v2, block_read(&p2, 3));

    // The 8 byte trailer always differs, crc and size, so compare ahead of it.
    let (n1, n2) = (p1.len() - 8, p2.len() - 8);
    let common = p1[..n1]
        .iter()
        .rev()
        .zip(p2[..n2].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    assert!(common > n1.min(n2) / 4, "tails did not re-align");
}
