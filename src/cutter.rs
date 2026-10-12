// cutter.rs -- cutting candidate segments out of the compressed input
// For conditions of distribution and use, see LICENSE.md

use crate::buf::{Buf, OutOfMemory};
use crate::gzblock::MAX_BLOCK;
use crate::scanner::{scan_empty_block, scan_marker};

/// How a cut ended. Found put a segment in seg, Done is the input used up at its end, More wants
/// input fed before deciding, TooLarge held max_seg without finding a boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    Done,
    Found,
    More,
    TooLarge,
}

/// Boundaries are marker pairs. A lone marker is never one, since a sync flush writes the same
/// bytes as a full flush and keeps the dictionary.
#[derive(Default)]
pub struct Cutter {
    /// The coalescing target.
    block_size: usize,
    /// How much input to hold looking for one boundary.
    max_seg: usize,
    /// Bytes of the input already scanned for markers.
    scanned: usize,
    /// Rightmost pair end while coalescing small chunks, 0 when not.
    coal: usize,
    /// Segment most recently cut.
    pub seg: Buf,
    /// The segment ends the input, every other one ends with a marker pair.
    pub seg_last: bool,
}

impl Cutter {
    /// Set up for one member. The segment buffer carries over from the last one.
    pub fn init(&mut self, block_size: u32) {
        self.block_size = block_size as usize;
        // A pair-terminated block may be any size and coalescing gathers several, so this is a
        // memory bound rather than a property of the format.
        self.max_seg = block_size as usize * 4 + 1024;
        self.scanned = 0;
        self.coal = 0;
    }

    /// Move the first n bytes of the input into seg.
    fn cut(&mut self, buf: &mut Buf, n: usize, last: bool) -> Result<Cut, OutOfMemory> {
        self.seg.clear();
        self.seg.append(&buf.data()[..n])?;
        self.seg_last = last;
        buf.consume(n);
        self.scanned = 0;
        self.coal = 0;
        Ok(Cut::Found)
    }

    /// The blocks are larger than assumed, which happens when the probe's guess was low. A
    /// pair-terminated block is valid at any size, so raise the bound and rescan rather than give
    /// up. Returns false once the bound cannot grow.
    fn widen(&mut self) -> bool {
        if self.max_seg >= MAX_BLOCK as usize {
            return false;
        }
        self.max_seg = (self.max_seg * 2).min(MAX_BLOCK as usize);
        true
    }

    /// Cut the next candidate segment out of buf into seg and say how it went. eof means buf
    /// holds all the input there is.
    pub fn next(&mut self, buf: &mut Buf, eof: bool) -> Result<Cut, OutOfMemory> {
        loop {
            let data = buf.data();
            let len = data.len();
            let limit = len.saturating_sub(3);
            let hit = if self.scanned < limit {
                scan_marker(data, self.scanned, limit)
            } else {
                None
            };

            if let Some(hit) = hit {
                let mut n = hit + 4;
                let mut empties = 0;
                // Empty stored blocks right after the marker belong to this segment too, the
                // second one is what makes a boundary. Their bytes must be in hand.
                loop {
                    if n + 5 > len {
                        if eof {
                            break;
                        }
                        self.scanned = hit;
                        return Ok(Cut::More);
                    }
                    if !scan_empty_block(&data[n..]) {
                        break;
                    }
                    n += 5;
                    empties += 1;
                }
                if empties == 0 {
                    // a lone marker, a flush inside a block or data that happens to match
                    self.scanned = hit + 1;
                    continue;
                }
                if n < self.block_size {
                    // A small pair-terminated chunk. One slot per chunk costs more in handoff
                    // than inflation, so keep absorbing chunks until a block of input is in hand.
                    self.coal = n;
                    self.scanned = n;
                    continue;
                }
                if n > self.max_seg {
                    if self.coal != 0 {
                        return self.cut(buf, self.coal, false);
                    }
                    if self.widen() {
                        continue;
                    }
                    return Ok(Cut::TooLarge);
                }
                return self.cut(buf, n, false);
            }
            self.scanned = limit;
            if len > self.max_seg + 3 {
                if self.coal != 0 {
                    return self.cut(buf, self.coal, false);
                }
                if self.widen() {
                    continue;
                }
                return Ok(Cut::TooLarge);
            }
            if eof {
                if len == 0 {
                    return Ok(Cut::Done);
                }
                return self.cut(buf, len, true);
            }
            return Ok(Cut::More);
        }
    }

    /// Scanning starts over from byte `from` of the input, after the caller rebuilt it.
    pub fn rescan(&mut self, from: usize) {
        self.scanned = from;
        self.coal = 0;
    }
}
