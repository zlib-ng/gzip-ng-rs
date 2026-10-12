// compress.rs -- compression through the stream function
// For conditions of distribution and use, see LICENSE.md

mod common;

use common::{pattern, whole_inflate};
use gzng::format::{self, Parse};
use gzng::{Compress, Totals};

/// Compress data through the stream function, returning what it wrote and what it counted.
fn compress_counted(data: &[u8], params: &Compress) -> (Vec<u8>, Totals) {
    let mut packed = Vec::new();
    let totals = gzng::compress_stream(data, &mut packed, params).unwrap();
    (packed, totals)
}

fn compress(data: &[u8], params: &Compress) -> Vec<u8> {
    compress_counted(data, params).0
}

/// Whether the stream carries the marker pair that ends an independent block.
fn has_block_pairs(packed: &[u8]) -> bool {
    packed
        .windows(9)
        .any(|window| window == [0, 0, 0xff, 0xff, 0, 0, 0, 0xff, 0xff])
}

#[test]
fn stream_roundtrips_through_inflate() {
    let data = pattern(1 << 20);

    let (packed, totals) = compress_counted(&data, &Compress::default());
    assert_eq!(data.len() as u64, totals.total_in);
    assert_eq!(packed.len() as u64, totals.total_out);
    assert!(packed.len() < data.len());
    assert_eq!(data, whole_inflate(&packed, data.len()));
}

#[test]
fn header_records_the_time_and_name() {
    let params = Compress {
        mtime: 0x5f6a_7b8c,
        name: Some(b"file.txt"),
        ..Compress::default()
    };
    let packed = compress(&pattern(1000), &params);
    let (parse, hdr) = format::header_parse(&packed);

    assert_ne!(Parse::Invalid, parse);
    assert_eq!(0x5f6a_7b8c, hdr.mtime);
    assert_eq!(Some(&b"file.txt"[..]), hdr.name);
    assert_eq!(format::OS_CODE, packed[9]);
}

#[test]
fn blocks_record_the_time_and_name_too() {
    let params = Compress {
        block_size: 64 << 10,
        mtime: 0x5f6a_7b8c,
        name: Some(b"file.txt"),
        ..Compress::default()
    };
    let (packed, totals) = compress_counted(&pattern(1 << 20), &params);
    let (_, hdr) = format::header_parse(&packed);

    assert_eq!(0x5f6a_7b8c, hdr.mtime);
    assert_eq!(Some(&b"file.txt"[..]), hdr.name);
    assert_eq!(format::OS_CODE, packed[9]);
    assert_eq!(1 << 20, totals.total_in);
    assert_eq!(packed.len() as u64, totals.total_out);
}

#[test]
fn a_plain_stream_has_no_blocks() {
    assert!(!has_block_pairs(&compress(&pattern(1 << 20), &Compress::default())));
}

#[test]
fn blocksize_cuts_independent_blocks() {
    let data = pattern(1 << 20);
    let params = Compress {
        block_size: 64 << 10,
        ..Compress::default()
    };

    let packed = compress(&data, &params);
    assert!(has_block_pairs(&packed));
    assert_eq!(
        data,
        whole_inflate(&packed, data.len()),
        "blocks still make one ordinary member"
    );
}

#[test]
fn levels_trade_size_for_time() {
    let data = pattern(1 << 20);
    let fast = Compress {
        level: 1,
        ..Compress::default()
    };
    let best = Compress {
        level: 9,
        ..Compress::default()
    };

    let (quick, small) = (compress(&data, &fast), compress(&data, &best));
    assert!(quick.len() > small.len());
    assert_eq!(data, whole_inflate(&quick, data.len()));
    assert_eq!(data, whole_inflate(&small, data.len()));
}

#[test]
fn rsyncable_is_deterministic_and_roundtrips() {
    let data = pattern(1 << 20);
    let params = Compress {
        rsyncable: true,
        ..Compress::default()
    };

    let (once, twice) = (compress(&data, &params), compress(&data, &params));
    assert_eq!(once, twice);
    assert_eq!(data, whole_inflate(&once, data.len()));
}

#[test]
fn empty_input_makes_an_empty_member() {
    let packed = compress(&[], &Compress::default());

    assert_eq!(
        format::HEADER_LEN + 2 + format::TRAILER_LEN,
        packed.len(),
        "the shortest deflate stream is two bytes"
    );
    assert!(whole_inflate(&packed, 0).is_empty());
}

#[test]
fn empty_input_makes_an_empty_block_member() {
    let params = Compress {
        block_size: 64 << 10,
        ..Compress::default()
    };
    let packed = compress(&[], &params);

    assert_eq!(format::HEADER_LEN + 2 + format::TRAILER_LEN, packed.len());
    assert!(whole_inflate(&packed, 0).is_empty());
}
