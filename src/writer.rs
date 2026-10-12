// writer.rs -- the parallel writer for gzip members made of independent deflate blocks
// For conditions of distribution and use, see LICENSE.md

use std::io::{self, Write};

use crate::codec::Status;
use crate::format::{self, Header};
use crate::gzblock::{Code, Error, MAX_BLOCK, Result};
use crate::pipeline::Pipeline;
use crate::pool::{Mode, Pool, Slot};
use crate::rolling;
use crate::zng::{self, Deflater, MAX_WBITS, Strategy};

/// Produces one gzip member whose deflate stream is cut into independent blocks of block_size
/// input bytes, deflated on nthreads threads.
pub struct Writer<W: Write> {
    write: W,
    block_size: u32,
    level: i32,
    strategy: Strategy,
    nthreads: usize,
    pipeline: Pipeline,
    /// Slot being filled.
    cur: Option<Slot>,
    crc: u32,
    total_in: u64,
    hdr_written: bool,
    finished: bool,
    /// The error code and message once failed.
    failed: Option<(Code, &'static str)>,
    mtime: u32,
    name: Vec<u8>,
    /// End blocks at rolling hash hits so edits stay local.
    rsyncable: bool,
    rsync_hash: u32,
    /// Strict mask while the block is short of block_size.
    rsync_mask_lo: u32,
    /// Loose mask once it is past.
    rsync_mask_hi: u32,
    /// A block is cut on size alone only here.
    rsync_max: usize,
    /// No early end before this much of the block is filled.
    rsync_min: usize,
}

/// Size the ring for blocks of up to in_size input bytes and start the workers.
fn pool_size(level: i32, strategy: Strategy, nthreads: usize, in_size: usize) -> Option<Pipeline> {
    let out_size = Deflater::new(level, -MAX_WBITS, strategy)?.bound(in_size) + 32;

    Pipeline::start(Mode::Deflate { level, strategy }, nthreads, in_size, out_size)
}

impl<W: Write> Writer<W> {
    /// nthreads of 0 picks the number of CPUs, 1 does the work on the calling thread.
    pub fn new(write: W, level: i32, strategy: Strategy, block_size: u32, nthreads: usize) -> Result<Self> {
        if block_size == 0 || block_size > MAX_BLOCK {
            return Err(Error::new(Code::Stream, "bad block size"));
        }
        let nthreads = if nthreads > 0 {
            nthreads
        } else {
            Pool::default_threads()
        };
        let pipeline = pool_size(level, strategy, nthreads, block_size as usize)
            .ok_or_else(|| Error::new(Code::Mem, "cannot start threads"))?;

        Ok(Self {
            write,
            block_size,
            level,
            strategy,
            nthreads,
            pipeline,
            cur: None,
            crc: 0,
            total_in: 0,
            hdr_written: false,
            finished: false,
            failed: None,
            mtime: 0,
            name: Vec::new(),
            rsyncable: false,
            rsync_hash: 0,
            rsync_mask_lo: 0,
            rsync_mask_hi: 0,
            rsync_max: 0,
            rsync_min: 0,
        })
    }

    /// What the writer writes to.
    pub fn get_ref(&self) -> &W {
        &self.write
    }

    // ===========================================================================
    // Errors, output, and the member header

    fn fail<T>(&mut self, code: Code, msg: &'static str, source: Option<io::Error>) -> Result<T> {
        self.failed = Some((code, msg));
        Err(match source {
            Some(source) => Error::with_source(code, msg, source),
            None => Error::new(code, msg),
        })
    }

    /// The error every call makes once the writer has failed.
    fn check_failed(&self) -> Result<()> {
        match self.failed {
            Some((code, msg)) => Err(Error::new(code, msg)),
            None => Ok(()),
        }
    }

    /// Writing is over once the writer has failed or finished.
    fn check_open(&self) -> Result<()> {
        self.check_failed()?;
        if self.finished {
            return Err(Error::new(Code::Stream, "the member is finished"));
        }
        Ok(())
    }

    /// The header takes its settings before the first write.
    fn check_unwritten(&self) -> Result<()> {
        self.check_failed()?;
        if self.hdr_written {
            return Err(Error::new(Code::Stream, "the header is already written"));
        }
        Ok(())
    }

    fn out(&mut self, buf: &[u8]) -> Result<()> {
        match self.write.write_all(buf) {
            Ok(()) => Ok(()),
            Err(err) => self.fail(Code::Errno, "write error", Some(err)),
        }
    }

    /// An ordinary gzip header, carrying only the name and time --name and --time ask for.
    /// Nothing in it marks the member as cut into blocks, a reader finds that out by scanning.
    fn header(&mut self) -> Result<()> {
        let mut buf = [0; format::HEADER_MAX];

        if self.hdr_written {
            return Ok(());
        }
        let n = format::header_build(
            &mut buf,
            &Header {
                mtime: self.mtime,
                name: Some(&self.name),
                level: self.level,
                strategy: self.strategy,
                member_size: 0,
            },
        );
        self.hdr_written = true;
        self.out(&buf[..n])
    }

    // ===========================================================================
    // Blocks through the pool

    /// Take the next free slot to fill, draining finished ones to make room.
    fn acquire(&mut self) -> Result<()> {
        while !self.pipeline.has_free() {
            self.drain()?;
        }
        let mut slot = self.pipeline.take();
        slot.in_len = 0;
        self.cur = Some(slot);
        Ok(())
    }

    fn submit(&mut self, last: bool) {
        if let Some(mut slot) = self.cur.take() {
            slot.last = last;
            slot.level = self.level;
            slot.strategy = self.strategy;
            self.pipeline.submit(slot);
        }
    }

    /// Write out the next compressed block in order.
    fn drain(&mut self) -> Result<()> {
        let index = self.pipeline.next_drain;
        let slot = self.pipeline.wait(index);

        if slot.status != Status::Full {
            return self.fail(Code::Stream, "deflate failed", None);
        }
        self.header()?;
        self.out(&slot.out[..slot.out_len])?;
        self.crc = zng::crc32_combine(self.crc, slot.crc, slot.in_len as u64);
        self.total_in += slot.in_len as u64;
        self.pipeline.pool.release(index, slot);
        self.pipeline.drained();
        Ok(())
    }

    fn drain_all(&mut self) -> Result<()> {
        while self.pipeline.has_pending() {
            self.drain()?;
        }
        Ok(())
    }

    /// End the block being filled early. A marker pair ends every block, which makes any length a
    /// valid one.
    fn cut(&mut self) {
        if self.cur.as_ref().is_some_and(|slot| slot.in_len != 0) {
            self.submit(false);
        }
    }

    // ===========================================================================
    // The writer object

    /// End blocks at rolling hash hits after half fill, content-defined boundaries for rsync,
    /// before the first write. The reader needs nothing special, pairs already carry any size.
    ///
    /// With --rsyncable the block size is a target rather than a ceiling. Boundaries are refused
    /// before half a block is buffered and forced at twice one, and between those FastCDC's
    /// normalized cut crowds them just past block_size, a strict mask that rarely hits while the
    /// block is short of the target and a loose one that hits quickly once it is past. That puts
    /// the average about a tenth above block_size with every boundary content-defined. Content
    /// the hash never hits, such as a run of one byte, is still cut at twice it.
    pub fn rsyncable(&mut self, on: bool) -> Result<()> {
        self.check_unwritten()?;
        if !on {
            self.rsyncable = false;
            return Ok(());
        }
        // The limit widens here, which takes a new ring, so no block can be in the old one.
        if self.cur.is_some() || self.pipeline.has_pending() {
            return Err(Error::new(Code::Stream, "input is already written"));
        }
        self.rsync_min = self.block_size as usize / 2;
        self.rsync_mask_lo = rolling::mask(self.block_size as usize * 4);
        self.rsync_mask_hi = rolling::mask(self.block_size as usize / 8);
        self.rsync_max = self.block_size as usize * 2;
        match pool_size(self.level, self.strategy, self.nthreads, self.rsync_max) {
            Some(pipeline) => self.pipeline = pipeline,
            None => return self.fail(Code::Mem, "cannot start threads", None),
        }
        self.rsyncable = true;
        Ok(())
    }

    /// Record a modification time and file name for the header, before the first write. A name of
    /// format::NAME_MAX bytes or longer is left out.
    pub fn meta(&mut self, mtime: u32, name: Option<&[u8]>) -> Result<()> {
        self.check_unwritten()?;
        self.mtime = mtime;
        if let Some(name) = name.filter(|name| name.len() < format::NAME_MAX) {
            self.name = name.to_vec();
        }
        Ok(())
    }

    /// A hash hit after the minimum fill ends the block there, so boundaries follow the content
    /// and an edit re-aligns at the next hit instead of shifting every block. The strict mask
    /// covers the input up to a full block_size and the loose mask what lies past it. Shortens
    /// take to end at the first hit. Returns true on a hit.
    fn rsync_cut(&mut self, fill: usize, buf: &[u8], take: &mut usize) -> bool {
        let block_size = self.block_size as usize;
        let first = if fill + 1 >= self.rsync_min {
            0
        } else {
            self.rsync_min - fill - 1
        };
        let lo_end = if fill >= block_size {
            0
        } else {
            (*take).min(block_size - fill)
        };
        let mut hit = rolling::find(&mut self.rsync_hash, self.rsync_mask_lo, &buf[..lo_end], first);

        if hit == lo_end && *take > lo_end {
            hit = rolling::find(&mut self.rsync_hash, self.rsync_mask_hi, &buf[..*take], lo_end);
        }
        if hit == *take {
            return false;
        }
        *take = hit + 1;
        true
    }

    pub fn write(&mut self, mut buf: &[u8]) -> Result<()> {
        self.check_open()?;
        while !buf.is_empty() {
            if self.cur.is_none() {
                self.acquire()?;
            }
            let limit = if self.rsyncable {
                self.rsync_max
            } else {
                self.block_size as usize
            };
            let fill = self.cur.as_ref().map_or(0, |slot| slot.in_len);
            let mut take = (limit - fill).min(buf.len());
            let hit = self.rsyncable && self.rsync_cut(fill, buf, &mut take);

            if let Some(slot) = &mut self.cur {
                slot.append(&buf[..take]);
            }
            buf = &buf[take..];
            if fill + take == limit || hit {
                self.submit(false);
            }
        }
        Ok(())
    }

    /// Settings for the blocks to come.
    pub fn set_params(&mut self, level: i32, strategy: Strategy) -> Result<()> {
        self.check_open()?;
        if level == self.level && strategy == self.strategy {
            return Ok(());
        }
        // Input already taken for the current block keeps the old settings, in a block that ends
        // here.
        self.cut();
        self.level = level;
        self.strategy = strategy;
        Ok(())
    }

    /// End the current block early and write everything out.
    pub fn flush(&mut self) -> Result<()> {
        self.check_open()?;
        self.cut();
        self.drain_all()?;
        self.header()
    }

    /// Write the last block and the trailer.
    pub fn finish(&mut self) -> Result<()> {
        self.check_failed()?;
        if self.finished {
            return Ok(());
        }
        // The last block ends the deflate stream, an empty one if the input ended on a boundary.
        if self.cur.is_none() {
            self.acquire()?;
        }
        self.submit(true);
        self.drain_all()?;
        self.out(&format::trailer_build(self.crc, self.total_in))?;
        self.finished = true;
        Ok(())
    }
}
