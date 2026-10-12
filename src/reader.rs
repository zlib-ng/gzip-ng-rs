// reader.rs -- the parallel reader for gzip members made of independent deflate blocks
// For conditions of distribution and use, see LICENSE.md

use std::io::{self, Read};

use crate::IO_CHUNK;
use crate::buf::{Buf, read_some};
use crate::codec::Status;
use crate::cutter::{Cut, Cutter};
use crate::format::{self, Parse};
use crate::gzblock::{Code, Error, Result};
use crate::pipeline::Pipeline;
use crate::pool::{Mode, Pool, Slot};
use crate::scanner::scan_marker_pair;
use crate::zng::{self, Inflater, MAX_WBITS, Z_NO_FLUSH, Z_OK, Z_STREAM_END};

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Header,
    Passthru,
    Stream,
    Blocks,
    Members,
    MemberEnd,
    End,
    Error,
}

/// Sized members, BGZF, cut whole and decoded in parallel with headers and trailers included.
#[derive(Default)]
struct Members {
    /// Member group being assembled for a slot.
    seg: Buf,
    /// Whole members in seg.
    count: i32,
    /// The input's next bytes are not a sized member.
    done: bool,
}

/// Decodes gzip data, member by member. A member found to be cut into independent blocks is
/// inflated as blocks on nthreads threads at once. Other members, and every member when nthreads
/// is 1, are streamed through plain inflate. Input that is not gzip at all is passed through
/// unchanged and trailing garbage after the last member is ignored.
pub struct Reader<R: Read> {
    read: R,
    /// Input in hand, not yet consumed.
    in_buf: Buf,
    /// The input returned 0.
    in_eof: bool,

    /// Plain inflate.
    strm: Inflater,
    /// IO_CHUNK, output of strm, or bytes passed through.
    out_buf: Vec<u8>,

    state: State,
    nthreads: usize,
    /// gzip members finished so far.
    members: i32,
    /// The error code and message once failed.
    failed: Option<(Code, String)>,

    /// Where the next output to deliver starts, in the held slot's output or in out_buf.
    next: usize,
    /// Bytes available at next.
    have: usize,
    /// Slot to release once its output is consumed, with its place in the ring.
    slot: Option<(usize, Slot)>,

    memb: Members,
    cut: Cutter,
    /// This member's header, kept for the fallback.
    hdr: Vec<u8>,
    /// The cutter handed out the member's last segment.
    cut_all: bool,

    /// Started for the first member that needs it.
    pipeline: Option<Pipeline>,
    /// Running crc and length of the member's output.
    crc: u32,
    total_out: u64,
}

/// The coalescing target and how far the probe looks.
const PROBE_BLOCK: u32 = 128 << 10;
const PROBE_WINDOW: usize = 1 << 20;

impl<R: Read> Reader<R> {
    /// nthreads of 0 picks the number of CPUs. head holds bytes already taken from the input that
    /// come before what read returns.
    pub fn new(read: R, head: &[u8], nthreads: usize) -> Result<Self> {
        let strm = Inflater::new(MAX_WBITS + 16).ok_or_else(|| Error::new(Code::Mem, "out of memory"))?;
        let mut in_buf = Buf::new();

        in_buf
            .append(head)
            .map_err(|_| Error::new(Code::Mem, "out of memory"))?;
        Ok(Self {
            read,
            in_buf,
            in_eof: false,
            strm,
            out_buf: vec![0; IO_CHUNK],
            state: State::Header,
            nthreads: if nthreads > 0 {
                nthreads
            } else {
                Pool::default_threads()
            },
            members: 0,
            failed: None,
            next: 0,
            have: 0,
            slot: None,
            memb: Members::default(),
            cut: Cutter::default(),
            hdr: Vec::new(),
            cut_all: false,
            pipeline: None,
            crc: 0,
            total_out: 0,
        })
    }

    /// What the reader reads from.
    pub fn get_ref(&self) -> &R {
        &self.read
    }

    // ===========================================================================
    // Errors, input, and output handoff

    fn fail_from<T>(&mut self, code: Code, msg: impl Into<String>, source: Option<io::Error>) -> Result<T> {
        let msg = msg.into();

        self.failed = Some((code, msg.clone()));
        self.state = State::Error;
        Err(match source {
            Some(source) => Error::with_source(code, msg, source),
            None => Error::new(code, msg),
        })
    }

    fn fail<T>(&mut self, code: Code, msg: impl Into<String>) -> Result<T> {
        self.fail_from(code, msg, None)
    }

    fn fill(&mut self, want: usize) -> Result<()> {
        match self.in_buf.fill(&mut self.read, want, &mut self.in_eof) {
            Ok(()) => Ok(()),
            Err(err) => self.fail_from(Code::Errno, "read error", Some(err)),
        }
    }

    fn oom<T>(&mut self) -> Result<T> {
        self.fail(Code::Mem, "out of memory")
    }

    /// The pipeline, which every block and member state starts before entering.
    fn pipeline(&mut self) -> &mut Pipeline {
        self.pipeline
            .as_mut()
            .expect("the pipeline starts with the first member that needs it")
    }

    /// The next n bytes go out from the slot's output, or from out_buf without one.
    fn handout(&mut self, n: usize, slot: Option<(usize, Slot)>) {
        self.next = 0;
        self.have = n;
        self.slot = slot;
    }

    /// The output not yet delivered.
    fn pending(&self) -> &[u8] {
        let out = match &self.slot {
            Some((_, slot)) => &slot.out,
            None => &self.out_buf,
        };
        &out[self.next..self.next + self.have]
    }

    // ===========================================================================
    // Plain inflate and pass-through

    /// Plain inflate of a member, starting with whatever is in buf.
    fn start_stream(&mut self) {
        self.strm.reset();
        self.state = State::Stream;
    }

    fn stream(&mut self) -> Result<()> {
        if self.in_buf.is_empty() {
            self.fill(1)?;
            if self.in_buf.is_empty() {
                return self.fail(Code::Buf, "unexpected end of file");
            }
        }
        let step = self.strm.inflate(self.in_buf.data(), &mut self.out_buf, Z_NO_FLUSH);
        self.in_buf.consume(step.consumed);
        if step.rc != Z_OK && step.rc != Z_STREAM_END {
            let msg = format!("inflate failed: {}", self.strm.msg().unwrap_or("data error"));
            return self.fail(Code::Data, msg);
        }
        self.handout(step.produced, None);
        if step.rc == Z_STREAM_END {
            self.members += 1;
            self.state = State::Header;
        }
        Ok(())
    }

    /// Not gzip at all, copied through unchanged.
    fn passthru(&mut self) -> Result<()> {
        if !self.in_buf.is_empty() {
            let n = self.in_buf.len().min(IO_CHUNK);
            self.out_buf[..n].copy_from_slice(&self.in_buf.data()[..n]);
            self.in_buf.consume(n);
            self.handout(n, None);
            return Ok(());
        }
        if self.in_eof {
            self.state = State::End;
            return Ok(());
        }
        match read_some(&mut self.read, &mut self.out_buf) {
            Err(err) => self.fail_from(Code::Errno, "read error", Some(err)),
            Ok(0) => {
                self.in_eof = true;
                self.state = State::End;
                Ok(())
            }
            Ok(n) => {
                self.handout(n, None);
                Ok(())
            }
        }
    }

    // ===========================================================================
    // The block pipeline

    /// Cut the next segment, feeding the cutter until it can decide.
    fn next_cut(&mut self) -> Result<Cut> {
        loop {
            match self.cut.next(&mut self.in_buf, self.in_eof) {
                Ok(Cut::More) => self.fill(self.in_buf.len() + IO_CHUNK)?,
                Ok(cut) => return Ok(cut),
                Err(_) => return self.oom(), // the cutter's only failure is allocation
            }
        }
    }

    /// Start the pool for inflate, once, its ring shared by every later member.
    fn pipeline_start(&mut self, block_size: u32) -> Result<()> {
        if self.pipeline.is_some() {
            return Ok(());
        }
        // Segments are swapped in whole, so slots start without an in buffer.
        match Pipeline::start(Mode::Inflate, self.nthreads, 0, block_size as usize) {
            Some(pipeline) => {
                self.pipeline = Some(pipeline);
                Ok(())
            }
            None => self.fail(Code::Mem, "cannot start threads"),
        }
    }

    /// Enter block mode for a member whose header is the first hdr_len bytes of the input.
    fn start_blocks(&mut self, hdr_len: usize, block_size: u32) -> Result<()> {
        self.hdr.clear();
        self.hdr.extend_from_slice(&self.in_buf.data()[..hdr_len]);
        self.in_buf.consume(hdr_len);
        self.pipeline_start(block_size)?;
        self.cut.init(block_size);
        self.cut_all = false;
        self.pipeline().reset();
        self.crc = 0;
        self.total_out = 0;
        self.state = State::Blocks;
        Ok(())
    }

    /// Put all, every segment still in the ring, and the input in hand back at the front of the
    /// input, to be cut again.
    fn rewind(&mut self, mut all: Buf) -> Result<()> {
        let pipeline = self.pipeline();
        let (drain, submit) = (pipeline.next_drain, pipeline.next_submit);
        let mut whole = true;

        // Take back what no worker has started, newest first, so only claimed slots are waited on
        // and their wasted inflation is bounded by the worker count.
        for i in (drain..submit).rev() {
            if !pipeline.pool.cancel(i) {
                break;
            }
        }
        for i in drain..submit {
            let slot = pipeline.wait(i);
            whole &= all.append(&slot.input[..slot.in_len]).is_ok();
            pipeline.pool.release(i, slot);
        }
        pipeline.clear();
        if !whole || all.append(self.in_buf.data()).is_err() {
            return self.oom();
        }
        self.in_buf = all;
        self.cut.rescan(0);
        self.cut_all = false;
        Ok(())
    }

    /// Put the member back together and stream it through plain inflate instead. Only valid
    /// before any of its output was handed out.
    fn fallback(&mut self) -> Result<()> {
        let mut all = Buf::new();

        if all.append(&self.hdr).is_err() {
            return self.oom();
        }
        self.rewind(all)?;
        self.start_stream();
        Ok(())
    }

    /// Keep the pool fed, cutting segments into free slots until the ring is full or the input is
    /// done.
    fn produce(&mut self) -> Result<()> {
        while !self.cut_all {
            if !self.pipeline().has_free() {
                break;
            }
            match self.next_cut()? {
                Cut::Done => {
                    self.cut_all = true;
                    break;
                }
                Cut::TooLarge => {
                    let (drain, submit) = (self.pipeline().next_drain, self.pipeline().next_submit);
                    if submit == 0 && drain == 0 {
                        return self.fallback(); // no block structure at this size
                    }
                    return self.fail(Code::Data, format!("block {submit} is larger than the block size"));
                }
                Cut::Found | Cut::More => {}
            }
            let mut slot = self.pipeline().take();
            slot.swap_in(&mut self.cut.seg);
            slot.last = self.cut.seg_last;
            slot.members = 0;
            self.pipeline().submit(slot);
        }
        Ok(())
    }

    /// Hand out a finished block and account for it. The slot goes back once its output is
    /// consumed.
    fn block_out(&mut self, index: usize, slot: Slot) {
        self.crc = zng::crc32_combine(self.crc, slot.crc, slot.out_len as u64);
        self.total_out += slot.out_len as u64;
        self.pipeline().drained();
        self.handout(slot.out_len, Some((index, slot)));
    }

    /// Hand out the next block in order.
    fn drain(&mut self) -> Result<()> {
        let index = self.pipeline().next_drain;
        let slot = self.pipeline().wait(index);

        if slot.status == Status::Full && slot.in_used == slot.in_len && !slot.last {
            self.block_out(index, slot);
            return Ok(());
        }
        if slot.status == Status::End {
            // The final block. What followed it in its input, the trailer and possibly more
            // members, goes back to the front of the input.
            let mut rest = Buf::new();
            if rest.append(&slot.input[slot.in_used..slot.in_len]).is_err() {
                return self.oom();
            }
            self.block_out(index, slot);
            self.rewind(rest)?;
            self.state = State::MemberEnd;
            return Ok(());
        }
        if slot.status == Status::Overflow && index == 0 {
            self.pipeline().pool.restore(index, slot);
            return self.fallback();
        }
        if slot.status == Status::Short && !slot.last {
            // A chance marker cut the block short. Its input goes back to be cut again, scanning
            // on from past that marker, so the block ends at the next one instead.
            let skip = slot.in_len;
            self.pipeline().pool.restore(index, slot);
            self.rewind(Buf::new())?;
            self.cut.rescan(skip);
            return Ok(());
        }
        if slot.status == Status::Full {
            if slot.last {
                return self.fail(Code::Buf, "unexpected end of file");
            }
            return self.fail(Code::Data, format!("block {index} has trailing data"));
        }
        let code = if slot.status == Status::Short {
            Code::Buf
        } else {
            Code::Data
        };
        self.fail(code, format!("block {index} is {}", slot.status.name()))
    }

    fn blocks(&mut self) -> Result<()> {
        self.produce()?;
        if self.state != State::Blocks {
            return Ok(()); // fell back to plain inflate
        }
        if self.pipeline().has_pending() {
            return self.drain();
        }
        self.fail(Code::Buf, "unexpected end of file")
    }

    /// The 8 trailer bytes follow the final block, then the next member or the end.
    fn member_end_step(&mut self) -> Result<()> {
        self.fill(format::TRAILER_LEN)?;
        let Some(trailer) = self.in_buf.data().first_chunk() else {
            return self.fail(Code::Buf, "unexpected end of file");
        };
        let (want_crc, want_size) = format::trailer_parse(trailer);
        if self.crc != want_crc {
            return self.fail(Code::Data, "crc mismatch in the gzip trailer");
        }
        if want_size != self.total_out as u32 {
            return self.fail(Code::Data, "length mismatch in the gzip trailer");
        }
        self.in_buf.consume(format::TRAILER_LEN);
        self.members += 1;
        self.state = State::Header;
        Ok(())
    }

    // ===========================================================================
    // Member headers and the boundary probe

    /// Look for a marker pair in the first stretch of compressed data.
    fn probe(&mut self, hdr_len: usize) -> Result<bool> {
        self.fill(hdr_len + PROBE_WINDOW)?;
        let data = self.in_buf.data();
        let limit = data.len().min(hdr_len + PROBE_WINDOW);
        if limit < hdr_len + 9 {
            return Ok(false);
        }
        Ok(scan_marker_pair(data, hdr_len, limit - 8).is_some())
    }

    /// Enter sized-member mode. Members are cut whole, so the pipeline needs no cutter state.
    fn start_members(&mut self) -> Result<()> {
        self.pipeline_start(PROBE_BLOCK)?;
        self.memb.seg.clear();
        self.memb.count = 0;
        self.memb.done = false;
        self.pipeline().reset();
        self.state = State::Members;
        Ok(())
    }

    /// Gather whole sized members into seg until about a block of input is in hand. Returns true
    /// with a group to submit, false when the input's next bytes are not a sized member and the
    /// group holds nothing.
    fn members_next(&mut self) -> Result<bool> {
        loop {
            if self.memb.seg.len() >= PROBE_BLOCK as usize {
                return Ok(true);
            }
            let (parse, hdr) = format::header_parse(self.in_buf.data());
            let member_size = hdr.member_size as usize;
            let sized = matches!(parse, Parse::Complete(hdr_len) if member_size >= hdr_len + format::TRAILER_LEN);
            let in_len = self.in_buf.len();

            if sized && in_len < member_size && !self.in_eof {
                self.fill(member_size)?;
                continue;
            }
            if parse == Parse::More && !self.in_eof {
                self.fill(in_len + IO_CHUNK)?;
                continue;
            }
            if !sized || in_len < member_size {
                self.memb.done = true; // the header path decides what the rest is
                return Ok(!self.memb.seg.is_empty());
            }
            if self.memb.seg.append(&self.in_buf.data()[..member_size]).is_err() {
                return self.oom();
            }
            self.in_buf.consume(member_size);
            self.memb.count += 1;
        }
    }

    /// Keep the pool fed with member groups and hand out the next one in order.
    fn members_step(&mut self) -> Result<()> {
        while !self.memb.done {
            if !self.pipeline().has_free() {
                break;
            }
            if !self.members_next()? {
                break;
            }
            let mut slot = self.pipeline().take();
            slot.swap_in(&mut self.memb.seg);
            slot.members = self.memb.count;
            slot.last = false;
            self.memb.count = 0;
            self.pipeline().submit(slot);
        }
        if self.pipeline().has_pending() {
            let index = self.pipeline().next_drain;
            let slot = self.pipeline().wait(index);
            if slot.status != Status::Full {
                let msg = format!("member {} is {}", self.members, slot.status.name());
                return self.fail(Code::Data, msg);
            }
            self.members += slot.members;
            self.pipeline().drained();
            self.handout(slot.out_len, Some((index, slot)));
            return Ok(());
        }
        // Every group is drained and the input goes on with something else.
        self.state = State::Header;
        Ok(())
    }

    /// Decide how to decode what comes next: a gzip member in block mode or plain, pass-through
    /// for data that is not gzip, or the end.
    fn header(&mut self) -> Result<()> {
        let mut want = 1024;

        let (hdr_len, member_size) = loop {
            self.fill(want)?;
            let data = self.in_buf.data();
            if !format::is_gzip(data) {
                self.state = if data.is_empty() && self.in_eof {
                    State::End
                } else if self.members == 0 {
                    State::Passthru // not gzip, hand it back unchanged
                } else {
                    State::End // trailing garbage after a member, ignored
                };
                return Ok(());
            }
            match format::header_parse(data) {
                (Parse::Invalid, _) => return self.fail(Code::Data, "not in gzip format"),
                (Parse::Complete(hdr_len), hdr) => break (hdr_len, hdr.member_size),
                (Parse::More, _) => {}
            }
            if self.in_eof {
                return self.fail(Code::Buf, "unexpected end of file");
            }
            // A header that does not fit in a megabyte is not one worth buffering, inflate takes
            // it piece by piece.
            if want >= 1 << 20 {
                self.start_stream();
                return Ok(());
            }
            want *= 2;
        };
        // One thread gains nothing from blocks and pays for the scan and an extra copy, so plain
        // inflate streams the member.
        if self.nthreads == 1 {
            self.start_stream();
            return Ok(());
        }
        // A sized member's end is known before anything inflates, so members decode whole and in
        // parallel, with nothing probed, rewound, or inflated twice. That takes the whole member
        // in hand. One the input ends inside of is streamed, which reports where it ends, and a
        // size too small to hold the header and trailer is not a size at all.
        let member_size = member_size as usize;
        if member_size >= hdr_len + format::TRAILER_LEN {
            self.fill(member_size)?;
            if self.in_buf.len() < member_size {
                self.start_stream();
                return Ok(());
            }
            return self.start_members();
        }
        // An early marker pair means someone wrote independent chunks, the full flush behind a
        // pair resets the dictionary, so decode them in parallel. Anything else inflates serially
        // as before. A lone marker says nothing, a sync flush writes the same bytes and keeps the
        // dictionary.
        if !self.probe(hdr_len)? {
            self.start_stream();
            return Ok(());
        }
        self.start_blocks(hdr_len, PROBE_BLOCK)
    }

    // ===========================================================================
    // The reader object

    /// Advance until there is output to hand out or the data ends, which leaves have at 0.
    fn advance(&mut self) -> Result<()> {
        while self.have == 0 {
            // Output handed out earlier has been consumed, its slot can go back to the pool.
            if let Some((index, slot)) = self.slot.take() {
                self.pipeline().pool.release(index, slot);
            }
            match self.state {
                State::Header => self.header()?,
                State::Passthru => self.passthru()?,
                State::Stream => self.stream()?,
                State::Blocks => self.blocks()?,
                State::Members => self.members_step()?,
                State::MemberEnd => self.member_end_step()?,
                State::End => return Ok(()),
                State::Error => {
                    let (code, msg) = self.failed.clone().unwrap_or((Code::Stream, String::new()));
                    return Err(Error::new(code, msg));
                }
            }
        }
        Ok(())
    }

    /// Read until buf is full or the data ends. Returns the bytes read.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let mut done = 0;

        while done < buf.len() {
            self.advance()?;
            if self.have == 0 {
                break; // end of the data
            }
            let n = self.have.min(buf.len() - done);
            buf[done..done + n].copy_from_slice(&self.pending()[..n]);
            done += n;
            self.next += n;
            self.have -= n;
        }
        Ok(done)
    }

    /// Hand out the next piece of output without copying, bytes owned by the reader and valid
    /// until the next call. The piece is empty at the end of the data.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<&[u8]> {
        self.advance()?;
        if self.have == 0 {
            return Ok(&[]); // end of the data
        }
        let (next, have) = (self.next, self.have);
        // Consumed as far as the reader is concerned, the slot goes back on the next call.
        self.next += have;
        self.have = 0;
        let out = match &self.slot {
            Some((_, slot)) => &slot.out,
            None => &self.out_buf,
        };
        Ok(&out[next..next + have])
    }
}
