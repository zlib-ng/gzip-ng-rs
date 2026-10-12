// mod.rs -- test data and in-memory streams shared by the tests
// For conditions of distribution and use, see LICENSE.md

#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::mem;

use gzng::gzblock::{Reader, Writer};
use gzng::sys as z;
use gzng::{Compress, DEFAULT_COMPRESSION};

pub const MAX_WBITS: i32 = 15;

pub fn sample_data(len: usize) -> Vec<u8> {
    let text = b"the quick brown fox jumps over the lazy dog ";
    (0..len).map(|i| text[i % 44] + ((i / 8191) % 7) as u8).collect()
}

/// Aperiodic but compressible bytes. Periodic sample data walks the rolling hash through a tiny
/// orbit that can miss the trigger entirely, which is exactly what a boundary test must not do.
pub fn varied_data(len: usize) -> Vec<u8> {
    let mut s = 0x1234_5678u32;
    (0..len)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            0x20 + ((s >> 24) & 0x3f) as u8
        })
        .collect()
}

pub fn pattern(len: usize) -> Vec<u8> {
    let text = b"a quick brown fox 0123456789 ";
    (0..len).map(|i| text[i % 29] + ((i / 4093) % 5) as u8).collect()
}

/// Settings for a block writer, the rest left at their defaults.
pub fn block_params(level: i32, block_size: u32, threads: usize) -> Compress<'static> {
    Compress {
        level,
        block_size,
        threads,
        ..Compress::default()
    }
}

pub fn block_compress(data: &[u8], block_size: u32, nthreads: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, &block_params(DEFAULT_COMPRESSION, block_size, nthreads)).unwrap();

    for chunk in data.chunks(65521) {
        w.write_all(chunk).unwrap();
    }
    w.finish().unwrap();
    drop(w);
    out
}

/// What zlib-ng makes of the whole stream, which has to be one member.
pub fn whole_inflate(packed: &[u8], expect: usize) -> Vec<u8> {
    let mut out = vec![0; expect + 64];

    // SAFETY: a stream of zeros is what init expects, and its buffers are the two vectors, at
    // their lengths.
    unsafe {
        let mut strm: z::zng_stream = mem::zeroed();
        assert_eq!(z::Z_OK, z::zng_inflateInit2(&mut strm, MAX_WBITS + 16));
        strm.next_in = packed.as_ptr();
        strm.avail_in = packed.len() as u32;
        strm.next_out = out.as_mut_ptr();
        strm.avail_out = out.len() as u32;
        assert_eq!(z::Z_STREAM_END, z::zng_inflate(&mut strm, z::Z_FINISH));
        out.truncate(strm.total_out);
        z::zng_inflateEnd(&mut strm);
    }
    out
}

/// One plain gzip member, the way gzip itself writes one, with a flush of the kind given after
/// every 64 KiB when one is.
pub fn gzip(data: &[u8], flush: Option<i32>) -> Vec<u8> {
    let step = if flush.is_some() { 65536 } else { data.len().max(1) };
    let mut packed = Vec::new();

    // SAFETY: a stream of zeros is what init expects, and its buffers are the two vectors, at
    // their lengths.
    unsafe {
        let mut strm: z::zng_stream = mem::zeroed();
        let rc = z::zng_deflateInit2(&mut strm, 6, z::Z_DEFLATED, MAX_WBITS + 16, 8, z::Z_DEFAULT_STRATEGY);
        assert_eq!(z::Z_OK, rc);
        // The bound is for one call, and each flush adds a marker to it.
        packed.resize(z::zng_deflateBound(&mut strm, data.len() as _) as usize + 1024, 0);
        strm.next_out = packed.as_mut_ptr();
        strm.avail_out = packed.len() as u32;
        for pos in (0..data.len().max(1)).step_by(step) {
            let n = step.min(data.len() - pos);
            let last = pos + n == data.len();
            strm.next_in = data[pos..].as_ptr();
            strm.avail_in = n as u32;
            match flush {
                Some(flush) if !last => assert_eq!(z::Z_OK, z::zng_deflate(&mut strm, flush)),
                _ => assert_eq!(z::Z_STREAM_END, z::zng_deflate(&mut strm, z::Z_FINISH)),
            }
        }
        packed.truncate(strm.total_out);
        z::zng_deflateEnd(&mut strm);
    }
    packed
}

/// Input from memory that gives up at most chunk bytes a read.
pub struct MemIn<'a> {
    pub data: &'a [u8],
    pub chunk: usize,
}

impl Read for MemIn<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(self.chunk).min(self.data.len());
        let (head, rest) = self.data.split_at(n);

        buf[..n].copy_from_slice(head);
        self.data = rest;
        Ok(n)
    }
}

pub fn block_read(packed: &[u8], nthreads: usize) -> Vec<u8> {
    let input = MemIn {
        data: packed,
        chunk: 65521,
    };
    let mut r = Reader::new(input, &[], nthreads).unwrap();
    let mut out = Vec::new();
    let mut buf = vec![0; 65521];

    loop {
        let got = r.read(&mut buf).unwrap();
        if got == 0 {
            break;
        }
        out.extend_from_slice(&buf[..got]);
    }
    out
}
