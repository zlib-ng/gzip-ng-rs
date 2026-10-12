// buf.rs -- growable byte buffer consumed from the front by offset
// For conditions of distribution and use, see LICENSE.md

use std::io::{self, Read};
use std::mem;

/// An allocation the buffer could not get.
#[derive(Debug)]
pub struct OutOfMemory;

/// The live data is data(), len bytes at p + offset, consuming data moves the offset and the
/// bytes move only when the space behind the offset is needed for a refill or an append. The
/// allocation is all of p.
#[derive(Default)]
pub struct Buf {
    p: Vec<u8>,
    len: usize,
    offset: usize,
}

/// Read once, going around again when a signal cut the read short.
pub fn read_some<R: Read>(read: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match read.read(buf) {
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

/// Read until buf is full or the input ends. Returns the bytes read.
pub fn read_full<R: Read>(read: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut len = 0;

    while len < buf.len() {
        let got = read_some(read, &mut buf[len..])?;
        if got == 0 {
            break;
        }
        len += got;
    }
    Ok(len)
}

impl Buf {
    pub const fn new() -> Self {
        Self {
            p: Vec::new(),
            len: 0,
            offset: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn data(&self) -> &[u8] {
        &self.p[self.offset..self.offset + self.len]
    }

    /// Move the live bytes back to the front of the allocation.
    fn compact(&mut self) {
        if self.offset != 0 {
            self.p.copy_within(self.offset..self.offset + self.len, 0);
            self.offset = 0;
        }
    }

    pub fn reserve(&mut self, need: usize) -> Result<(), OutOfMemory> {
        if self.offset + need > self.p.len() {
            self.compact();
        }
        if need > self.p.len() {
            let mut size = if self.p.is_empty() { 1 << 16 } else { self.p.len() };
            while size < need {
                size *= 2;
            }
            self.p.try_reserve_exact(size - self.p.len()).map_err(|_| OutOfMemory)?;
            self.p.resize(size, 0);
        }
        Ok(())
    }

    pub fn append(&mut self, data: &[u8]) -> Result<(), OutOfMemory> {
        self.reserve(self.len + data.len())?;
        let end = self.offset + self.len;
        self.p[end..end + data.len()].copy_from_slice(data);
        self.len += data.len();
        Ok(())
    }

    pub fn consume(&mut self, n: usize) {
        self.offset += n;
        self.len -= n;
        if self.len == 0 {
            self.offset = 0;
        }
    }

    pub fn clear(&mut self) {
        self.len = 0;
        self.offset = 0;
    }

    /// Trade allocations with p, which leaves holding the data, and return how many bytes that
    /// is. The buffer is left empty with p's old allocation for its next fill, no copy either way.
    pub fn swap(&mut self, p: &mut Vec<u8>) -> usize {
        self.compact();
        mem::swap(&mut self.p, p);
        mem::take(&mut self.len)
    }

    /// Read until the buffer holds at least want bytes or the input ends, which sets eof.
    pub fn fill<R: Read>(&mut self, read: &mut R, want: usize, eof: &mut bool) -> io::Result<()> {
        while self.len < want && !*eof {
            if self.offset + self.len == self.p.len() {
                if self.offset != 0 {
                    self.compact();
                } else {
                    self.reserve(self.p.len() + 1).map_err(|_| io::ErrorKind::OutOfMemory)?;
                }
            }
            let end = self.offset + self.len;
            let got = read_some(read, &mut self.p[end..])?;
            if got == 0 {
                *eof = true;
                break;
            }
            self.len += got;
        }
        Ok(())
    }
}
