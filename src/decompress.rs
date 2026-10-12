// decompress.rs -- decompression through the block engine reader
// For conditions of distribution and use, see LICENSE.md

use std::io::{self, BufRead, Read, Write};

use crate::Totals;
use crate::gzblock::Reader;

/// Counts what comes in from the input.
struct CountRead<R> {
    input: R,
    total: u64,
}

impl<R: Read> Read for CountRead<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.input.read(buf)?;
        self.total += n as u64;
        Ok(n)
    }
}

/// Decompress input to out, after any head bytes already taken from input, with threads as
/// Reader::new() takes them. Returns the bytes read and written, the head bytes counted as
/// read. An error from the block engine comes back as the io error's payload.
pub fn decompress_stream<R: Read, W: Write>(input: R, mut out: W, head: &[u8], threads: usize) -> io::Result<Totals> {
    let input = CountRead {
        input,
        total: head.len() as u64,
    };
    let mut reader = Reader::new(input, head, threads)?;
    let mut total_out = 0;

    loop {
        let piece = reader.fill_buf()?;
        let n = piece.len();
        if n == 0 {
            break;
        }
        total_out += n as u64;
        out.write_all(piece)?;
        reader.consume(n);
    }
    Ok(Totals {
        total_in: reader.get_ref().total,
        total_out,
    })
}
