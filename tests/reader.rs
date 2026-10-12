// reader.rs -- the block reader
// For conditions of distribution and use, see LICENSE.md

mod common;

use common::{MemIn, block_compress, block_params, block_read, gzip, sample_data, varied_data, whole_inflate};
use gzng::gzblock::{Code, Reader, Writer};
use gzng::sys::{Z_FULL_FLUSH, Z_SYNC_FLUSH};
use gzng::{Compress, Strategy};

#[test]
fn roundtrip_across_thread_counts() {
    let data = sample_data(1 << 20);
    let packed = block_compress(&data, 64 * 1024, 2);
    for nthreads in [0, 1, 3] {
        assert_eq!(data, block_read(&packed, nthreads), "nthreads {nthreads}");
    }
}

#[test]
fn zero_copy_handout() {
    let data = sample_data(500_000);
    let packed = block_compress(&data, 64 * 1024, 2);
    let input = MemIn {
        data: &packed,
        chunk: 65521,
    };
    let mut r = Reader::new(input, &[], 3).unwrap();
    let mut out = Vec::new();
    loop {
        let piece = r.next().unwrap();
        if piece.is_empty() {
            break;
        }
        out.extend_from_slice(piece);
    }
    assert!(r.next().unwrap().is_empty(), "the end stays the end");
    assert_eq!(data, out);
}

#[test]
fn plain_gzip_streams_through() {
    let data = sample_data(400_000);
    let packed = gzip(&data, None);
    assert_eq!(data, block_read(&packed, 0));
}

#[test]
fn concatenated_members() {
    let data = sample_data(300_000);
    let one = block_compress(&data, 64 * 1024, 1);
    let two = [&one[..], &one[..]].concat();
    let expect = [&data[..], &data[..]].concat();
    assert_eq!(expect, block_read(&two, 3));
}

#[test]
fn blocks_larger_than_the_probe_assumes_still_decode() {
    // Nothing records the block size, so the reader assumes one and finds the blocks far bigger.
    // They are pair-terminated, which makes them good at any size, so it has to make room rather
    // than give up part way through the member.
    let data = varied_data(6 << 20); // barely compressible, so the segments stay large
    let mut packed = Vec::new();
    // Content-defined ends, so the blocks vary and an early one can fit the reader's assumption
    // while a later one does not, which is past the point where it could start over.
    let params = Compress {
        rsyncable: true,
        ..block_params(6, 512 * 1024, 2)
    };
    let mut w = Writer::new(&mut packed, &params).unwrap();
    w.write(&data).unwrap();
    w.finish().unwrap();
    drop(w);

    for nthreads in [1, 3] {
        assert_eq!(data, block_read(&packed, nthreads), "nthreads {nthreads}");
    }
}

#[test]
fn false_pair_in_a_block_larger_than_the_probe_assumes() {
    // Stored data can hold the nine bytes of a marker pair by chance. The reader cuts there,
    // finds the piece incomplete, and inflates the real block again on its own thread, which has
    // to make room the same way a worker does when the block is larger than the size it assumed.
    let mut data = sample_data(1 << 20);
    data[300_000..300_009].copy_from_slice(&[0, 0, 0xff, 0xff, 0, 0, 0, 0xff, 0xff]);
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, &block_params(0, 512 * 1024, 1)).unwrap();
    w.write(&data).unwrap();
    w.finish().unwrap();
    drop(w);

    for nthreads in [1, 3] {
        assert_eq!(data, block_read(&out, nthreads), "nthreads {nthreads}");
    }
}

#[test]
fn lone_flush_markers_are_not_boundaries() {
    // A sync flush writes the same marker as a full flush and keeps the dictionary, so a member
    // of lone markers says nothing about independent blocks and has to inflate as one stream.
    let data = sample_data(1 << 20);
    for flush in [Z_SYNC_FLUSH, Z_FULL_FLUSH] {
        let packed = gzip(&data, Some(flush));

        for nthreads in [1, 3] {
            assert_eq!(data, block_read(&packed, nthreads), "flush {flush} nthreads {nthreads}");
        }
    }
}

#[test]
fn flushed_blocks_of_any_length() {
    // A flush or a settings change ends the block early, down to a single byte, and the reader
    // has to take each as a block.
    let data = varied_data(3 << 20);
    for rsync in [false, true] {
        let mut out = Vec::new();
        let params = Compress {
            rsyncable: rsync,
            ..block_params(6, 64 * 1024, 2)
        };
        let mut w = Writer::new(&mut out, &params).unwrap();
        let mut pos = 0;
        for cut in [1, 7, 5000, 70000, 200_001, data.len()] {
            w.write(&data[pos..cut]).unwrap();
            w.flush().unwrap();
            w.set_params(if cut % 2 != 0 { 1 } else { 9 }, Strategy::Default)
                .unwrap();
            pos = cut;
        }
        w.finish().unwrap();
        drop(w);
        assert_eq!(data, whole_inflate(&out, data.len()));
        for nthreads in [1, 3] {
            assert_eq!(data, block_read(&out, nthreads), "rsync {rsync} nthreads {nthreads}");
        }
    }
}

#[test]
fn head_bytes_come_first() {
    let data = sample_data(300_000);
    let packed = block_compress(&data, 64 * 1024, 2);
    let (head, rest) = packed.split_at(2);
    let mut r = Reader::new(rest, head, 3).unwrap();
    let mut out = vec![0; data.len() + 1];
    let got = r.read(&mut out).unwrap();
    assert_eq!(data, out[..got]);
}

#[test]
fn damage_is_reported_and_stays_reported() {
    let data = varied_data(2 << 20);
    let mut packed = block_compress(&data, 64 * 1024, 2);
    let at = packed.len() / 2;
    packed[at..at + 64].fill(0xa5);

    for nthreads in [1, 3] {
        let mut r = Reader::new(&packed[..], &[], nthreads).unwrap();
        let mut buf = vec![0; data.len() + 1];
        let err = r.read(&mut buf).unwrap_err();
        assert_eq!(Code::Data, err.code(), "nthreads {nthreads}: {err}");
        let again = r.read(&mut buf).unwrap_err();
        assert_eq!((err.code(), err.message()), (again.code(), again.message()));
    }
}

#[test]
fn a_changed_trailer_fails_the_member() {
    let data = sample_data(1 << 20);
    let good = block_compress(&data, 64 * 1024, 2);
    let n = good.len();

    for (at, what) in [(n - 8, "crc"), (n - 4, "length")] {
        let mut packed = good.clone();
        packed[at] ^= 1;
        let mut r = Reader::new(&packed[..], &[], 3).unwrap();
        let mut buf = vec![0; data.len() + 1];
        let err = r.read(&mut buf).unwrap_err();
        assert_eq!(Code::Data, err.code());
        assert!(err.message().contains(what), "{err}");
    }
}

#[test]
fn truncated_blocks_end_early() {
    let data = varied_data(2 << 20);
    let packed = block_compress(&data, 64 * 1024, 2);

    for nthreads in [1, 3] {
        let mut r = Reader::new(&packed[..packed.len() / 2], &[], nthreads).unwrap();
        let mut buf = vec![0; data.len() + 1];
        let err = r.read(&mut buf).unwrap_err();
        assert_eq!(Code::Buf, err.code(), "nthreads {nthreads}: {err}");
    }
}
