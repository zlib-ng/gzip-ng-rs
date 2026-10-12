// decompress.rs -- decompression through the stream function
// For conditions of distribution and use, see LICENSE.md

mod common;

use std::io;

use common::{gzip, pattern};
use gzng::gzblock::{self, Code};
use gzng::{Compress, Totals};

/// A member cut into independent blocks, as --blocksize and --processes write it.
fn blocks(data: &[u8]) -> Vec<u8> {
    let params = Compress {
        block_size: 64 << 10,
        threads: 2,
        ..Compress::default()
    };
    let mut packed = Vec::new();
    gzng::compress_stream(data, &mut packed, &params).unwrap();
    packed
}

/// Decompress bytes through the stream function, after any head bytes handed over as already
/// read.
fn run(input: &[u8], threads: usize, head: &[u8]) -> io::Result<(Vec<u8>, Totals)> {
    let mut output = Vec::new();
    let totals = gzng::decompress_stream(input, &mut output, head, threads)?;
    Ok((output, totals))
}

#[test]
fn plain_gzip_roundtrips() {
    let data = pattern(1 << 20);
    let packed = gzip(&data, None);

    let (output, totals) = run(&packed, 0, &[]).unwrap();
    assert_eq!(data, output);
    assert_eq!(packed.len() as u64, totals.total_in);
    assert_eq!(data.len() as u64, totals.total_out);
}

#[test]
fn block_stream_roundtrips_on_threads() {
    let data = pattern(3 << 20);
    let packed = blocks(&data);

    let (output, totals) = run(&packed, 3, &[]).unwrap();
    assert_eq!(data, output);
    assert_eq!(packed.len() as u64, totals.total_in);
    assert_eq!(data.len() as u64, totals.total_out);
}

#[test]
fn members_concatenate() {
    let (first, second) = (pattern(100_000), pattern(70_000));
    let packed = [gzip(&first, None), gzip(&second, None)].concat();
    let expected = [first, second].concat();

    let (output, _) = run(&packed, 0, &[]).unwrap();
    assert_eq!(expected, output);
}

/// A gzip member with a BGZF size subfield in its header, recording what size makes of its length.
fn sized_member(data: &[u8], size: impl Fn(usize) -> usize) -> Vec<u8> {
    let member = gzip(data, None);
    let size = size(member.len() + 8) - 1;
    let mut sized = vec![0x1f, 0x8b, 8, 4, 0, 0, 0, 0, 0, 0xff, 6, 0, b'B', b'C', 2, 0];

    sized.extend_from_slice(&(size as u16).to_le_bytes());
    sized.extend_from_slice(&member[10..]);
    sized
}

/// BGZF, the way bgzip lays members down, each one's length in its header.
fn bgzf(data: &[u8]) -> Vec<u8> {
    data.chunks(0xff00)
        .flat_map(|piece| sized_member(piece, |len| len))
        .collect()
}

/// The engine's own error behind a failed stream.
fn engine_code(err: &io::Error) -> Option<Code> {
    let engine = err.get_ref().and_then(|inner| inner.downcast_ref::<gzblock::Error>());
    engine.map(gzblock::Error::code)
}

#[test]
fn sized_members_decode_whole() {
    let data = pattern(600_000);
    let packed = bgzf(&data);

    for threads in [1, 3] {
        let (output, totals) = run(&packed, threads, &[]).unwrap();
        assert_eq!(data, output, "threads {threads}");
        assert_eq!(packed.len() as u64, totals.total_in);
    }
}

#[test]
fn a_sized_member_cut_short_fails() {
    // The input ends inside a member whose header promised more, at the first member and at a
    // later one, inside the data and inside the header.
    let packed = bgzf(&pattern(600_000));

    for cut in [14, 500, packed.len() / 2, packed.len() - 9, packed.len() - 1] {
        for threads in [1, 3] {
            let err = run(&packed[..cut], threads, &[]).unwrap_err();
            assert_eq!(Some(Code::Buf), engine_code(&err), "cut {cut} threads {threads}: {err}");
        }
    }
}

#[test]
fn a_size_too_small_is_not_a_size() {
    // A size that could not hold the header and trailer says nothing, and the member decodes as
    // any other does.
    let data = pattern(300_000);
    let packed = sized_member(&data, |_| 1);

    for threads in [1, 3] {
        let (output, _) = run(&packed, threads, &[]).unwrap();
        assert_eq!(data, output, "threads {threads}");
    }
}

#[test]
fn no_output_still_counts() {
    let data = pattern(200_000);

    let totals = gzng::decompress_stream(&gzip(&data, None)[..], io::sink(), &[], 0).unwrap();
    assert_eq!(data.len() as u64, totals.total_out);
}

#[test]
fn other_data_passes_through() {
    let data = pattern(50_000);

    let (output, _) = run(&data, 0, &[]).unwrap();
    assert_eq!(data, output, "input that is not gzip comes out unchanged");
}

#[test]
fn trailing_garbage_is_ignored() {
    let data = pattern(50_000);
    let packed = [gzip(&data, None), b"not a member".to_vec()].concat();

    let (output, _) = run(&packed, 0, &[]).unwrap();
    assert_eq!(data, output);
}

#[test]
fn head_bytes_come_before_the_stream() {
    let data = pattern(50_000);
    let packed = gzip(&data, None);
    let (head, rest) = packed.split_at(2);

    let (output, totals) = run(rest, 0, head).unwrap();
    assert_eq!(data, output);
    assert_eq!(packed.len() as u64, totals.total_in, "the head bytes count as input");
}

#[test]
fn truncated_input_fails() {
    let packed = gzip(&pattern(300_000), None);

    let err = run(&packed[..packed.len() / 2], 0, &[]).unwrap_err();
    assert_eq!(Some(Code::Buf), engine_code(&err), "the engine's error rides along");
}

#[test]
fn empty_input_is_empty_output() {
    let (output, totals) = run(&[], 0, &[]).unwrap();
    assert!(output.is_empty());
    assert_eq!(0, totals.total_out);
}
