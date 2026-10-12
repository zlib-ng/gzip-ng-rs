// writer.rs -- the parallel writer for gzip members made of independent deflate blocks
// For conditions of distribution and use, see LICENSE.md

use std::io::{self, Write};

use crate::DEFAULT_COMPRESSION;
use crate::codec::Status;
use crate::compress::Compress;
use crate::format::{self, Header};
use crate::gzblock::{Code, Error, MAX_BLOCK, Result};
use crate::pipeline::Pipeline;
use crate::pool::{Mode, Pool, Slot};
use crate::rolling;
use crate::zng::{self, Deflater, MAX_WBITS, Strategy};

/// Ends blocks at rolling hash hits after half fill, content-defined boundaries for rsync. The
/// reader needs nothing special, pairs already carry any size.
///
/// The block size is a target here rather than a ceiling. Boundaries are refused before half a
/// block is buffered and forced at twice one, and between those FastCDC's normalized cut crowds
/// them just past block_size, a strict mask that rarely hits while the block is short of the
/// target and a loose one that hits quickly once it is past. That puts the average about a tenth
/// above block_size with every boundary content-defined. Content the hash never hits, such as a
/// run of one byte, is still cut at twice it.
struct Rsync {
    hash: u32,
    /// Strict mask while the block is short of block_size.
    mask_lo: u32,
    /// Loose mask once it is past.
    mask_hi: u32,
    /// No early end before this much of the block is filled.
    min: usize,
    /// A block is cut on size alone only here.
    max: usize,
}

impl Rsync {
    fn new(block_size: usize) -> Self {
        Self {
            hash: 0,
            mask_lo: rolling::mask(block_size * 4),
            mask_hi: rolling::mask(block_size / 8),
            min: block_size / 2,
            max: block_size * 2,
        }
    }

    /// A hash hit after the minimum fill ends the block there, so boundaries follow the content
    /// and an edit re-aligns at the next hit instead of shifting every block. The strict mask
    /// covers the input up to a full block_size and the loose mask what lies past it. Shortens
    /// take to end at the first hit. Returns true on a hit.
    fn cut(&mut self, block_size: usize, fill: usize, buf: &[u8], take: &mut usize) -> bool {
        let first = if fill + 1 >= self.min { 0 } else { self.min - fill - 1 };
        let lo_end = if fill >= block_size {
            0
        } else {
            (*take).min(block_size - fill)
        };
        let mut hit = rolling::find(&mut self.hash, self.mask_lo, &buf[..lo_end], first);

        if hit == lo_end && *take > lo_end {
            hit = rolling::find(&mut self.hash, self.mask_hi, &buf[..*take], lo_end);
        }
        if hit == *take {
            return false;
        }
        *take = hit + 1;
        true
    }
}

/// Produces one gzip member whose deflate stream is cut into independent blocks of block_size
/// input bytes, deflated on a pool of threads. The input goes in through Write, and finish() ends
/// the member.
pub struct Writer<W: Write> {
    write: W,
    block_size: u32,
    level: i32,
    strategy: Strategy,
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
    rsync: Option<Rsync>,
}

/// Start the workers on a ring sized for blocks of up to in_size input bytes.
fn start_pipeline(level: i32, strategy: Strategy, nthreads: usize, in_size: usize) -> Option<Pipeline> {
    let out_size = Deflater::new(level, -MAX_WBITS, strategy)?.bound(in_size) + 32;

    Pipeline::start(Mode::Deflate { level, strategy }, nthreads, in_size, out_size)
}

/// Deflate takes levels 0 to 9, and the one that leaves the choice to it.
fn check_level(level: i32) -> Result<()> {
    if !(DEFAULT_COMPRESSION..=9).contains(&level) {
        return Err(Error::new(Code::Stream, "bad level"));
    }
    Ok(())
}

impl<W: Write> Writer<W> {
    /// A member made the way params says, which has to name a block size. Threads of 0 picks the
    /// number of CPUs, 1 does the work on the calling thread. A name of format::NAME_MAX bytes or
    /// longer is left out.
    pub fn new(write: W, params: &Compress) -> Result<Self> {
        let block_size = params.block_size;

        if block_size == 0 || block_size > MAX_BLOCK {
            return Err(Error::new(Code::Stream, "bad block size"));
        }
        check_level(params.level)?;
        let nthreads = if params.threads > 0 {
            params.threads
        } else {
            Pool::default_threads()
        };
        let rsync = params.rsyncable.then(|| Rsync::new(block_size as usize));
        // A block that ends with the content can run past block_size, and the ring holds the
        // largest.
        let in_size = rsync.as_ref().map_or(block_size as usize, |rsync| rsync.max);
        let pipeline = start_pipeline(params.level, params.strategy, nthreads, in_size)
            .ok_or_else(|| Error::new(Code::Mem, "cannot start threads"))?;
        let name = params.name.filter(|name| name.len() < format::NAME_MAX);

        Ok(Self {
            write,
            block_size,
            level: params.level,
            strategy: params.strategy,
            pipeline,
            cur: None,
            crc: 0,
            total_in: 0,
            hdr_written: false,
            finished: false,
            failed: None,
            mtime: params.mtime,
            name: name.unwrap_or_default().to_vec(),
            rsync,
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

    /// Take all of buf into blocks, handing each to the pool as it fills or ends.
    fn put(&mut self, mut buf: &[u8]) -> Result<()> {
        let block_size = self.block_size as usize;

        self.check_open()?;
        while !buf.is_empty() {
            if self.cur.is_none() {
                self.acquire()?;
            }
            let limit = self.rsync.as_ref().map_or(block_size, |rsync| rsync.max);
            let fill = self.cur.as_ref().map_or(0, |slot| slot.in_len);
            let mut take = (limit - fill).min(buf.len());
            let hit = match &mut self.rsync {
                Some(rsync) => rsync.cut(block_size, fill, buf, &mut take),
                None => false,
            };

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

    // ===========================================================================
    // The writer object

    /// Settings for the blocks to come.
    pub fn set_params(&mut self, level: i32, strategy: Strategy) -> Result<()> {
        self.check_open()?;
        check_level(level)?;
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

/// An error from the block engine comes back as the io error's payload.
impl<W: Write> Write for Writer<W> {
    /// Takes all of buf, every time.
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.put(buf)?;
        Ok(buf.len())
    }

    /// End the current block early and write everything out, then flush the output itself.
    fn flush(&mut self) -> io::Result<()> {
        self.check_failed()?;
        if !self.finished {
            self.cut();
            self.drain_all()?;
            self.header()?;
        }
        match self.write.flush() {
            Ok(()) => Ok(()),
            Err(err) => Ok(self.fail(Code::Errno, "write error", Some(err))?),
        }
    }
}
