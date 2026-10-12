// compress.rs -- serial gzip compression
// For conditions of distribution and use, see LICENSE.md

use std::io::{self, BufWriter, Read, Write};

use crate::buf::read_full;
use crate::format;
use crate::gzblock::Writer;
use crate::rolling;
use crate::zng::{Deflater, MAX_WBITS, Strategy, Z_BUF_ERROR, Z_FINISH, Z_NO_FLUSH, Z_OK, Z_STREAM_END, Z_SYNC_FLUSH};
use crate::{DEFAULT_LEVEL, IO_CHUNK, Totals};

/// What a block holds on average when nothing says otherwise.
pub const DEFAULT_BLOCK: u32 = 128 << 10;

const WRITE_BUFFER: usize = 64 * 1024;

/// How compress_stream() makes its one gzip member.
#[derive(Clone, Copy, Debug)]
pub struct Compress<'a> {
    pub level: i32,
    pub strategy: Strategy,
    /// 0 writes one plain deflate stream, anything else independent blocks of about this size.
    pub block_size: u32,
    /// Workers for the blocks, 0 picks the number of CPUs.
    pub threads: usize,
    /// Make the plain stream rsync friendly, block output always is.
    pub rsyncable: bool,
    /// The time the header records, 0 for none.
    pub mtime: u32,
    /// The file name the header records.
    pub name: Option<&'a [u8]>,
}

impl Default for Compress<'_> {
    fn default() -> Self {
        Self {
            level: DEFAULT_LEVEL,
            strategy: Strategy::Default,
            block_size: 0,
            threads: 0,
            rsyncable: false,
            mtime: 0,
            name: None,
        }
    }
}

/// Counts what goes through to the output.
struct CountWrite<W> {
    out: W,
    total: u64,
}

impl<W: Write> Write for CountWrite<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.out.write(buf)?;
        self.total += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }
}

/// Compress through the block engine, independent blocks sealed with marker pairs.
fn block_compress_stream<R: Read, W: Write>(mut input: R, out: W, params: &Compress) -> io::Result<Totals> {
    let out = CountWrite { out, total: 0 };
    let mut writer = Writer::new(out, params.level, params.strategy, params.block_size, params.threads)?;
    let mut buf = vec![0; IO_CHUNK];
    let mut total_in = 0;

    if params.mtime != 0 || params.name.is_some() {
        writer.meta(params.mtime, params.name)?;
    }
    // Boundaries always follow the content. It costs 0.02% of the output and keeps an edit
    // local, so it is not worth putting behind a flag.
    writer.rsyncable(true)?;
    loop {
        let n = read_full(&mut input, &mut buf)?;
        if n == 0 {
            break;
        }
        total_in += n as u64;
        writer.write(&buf[..n])?;
    }
    writer.finish()?;
    Ok(Totals {
        total_in,
        total_out: writer.get_ref().total,
    })
}

// How far apart --rsyncable puts its sync points in a plain stream, the spacing gzip and pigz
// both use. These are Z_SYNC_FLUSH points that let rsync resynchronise after an edit. They do
// not reset the dictionary and they do not start a block, so they cost a little ratio and
// nothing else. Block boundaries are a separate matter, spaced by --blocksize.
const RSYNC_SPAN: usize = 4096;

/// Push one span of input through deflate and write everything it produces. Returns the bytes
/// written. Each call into deflate gets the whole output buffer, since a flush that runs out of
/// room part way writes its marker again on the next call.
fn deflate_span<W: Write>(
    strm: &mut Deflater,
    out: &mut W,
    out_buf: &mut [u8],
    mut input: &[u8],
    flush: i32,
) -> io::Result<u64> {
    let mut total = 0;

    loop {
        let step = strm.deflate(input, out_buf, flush);
        if step.rc != Z_OK && step.rc != Z_STREAM_END && step.rc != Z_BUF_ERROR {
            return Err(io::Error::other("deflate failed"));
        }
        out.write_all(&out_buf[..step.produced])?;
        total += step.produced as u64;
        input = &input[step.consumed..];
        if input.is_empty() && step.produced != out_buf.len() {
            return Ok(total);
        }
    }
}

/// Compress input to out as one gzip member, plain deflate when the block size is 0 and
/// independent blocks of that size on threads workers otherwise, the plain stream rsync friendly
/// on request. Returns the bytes read and written. An error from the block engine comes back as
/// the io error's payload.
pub fn compress_stream<R: Read, W: Write>(mut input: R, out: W, params: &Compress) -> io::Result<Totals> {
    if params.block_size != 0 {
        return block_compress_stream(input, out, params);
    }

    // A flush every few kilobytes makes for small writes, gathered here the way stdio would.
    let mut out = BufWriter::with_capacity(WRITE_BUFFER, out);
    let mut strm = Deflater::new(params.level, MAX_WBITS + 16, params.strategy)
        .ok_or_else(|| io::Error::other("deflate failed"))?;
    let mut in_buf = vec![0; IO_CHUNK];
    let mut out_buf = vec![0; IO_CHUNK];
    let mut rsync_hash = 0;
    let rsync_mask = rolling::mask(RSYNC_SPAN);
    let mut totals = Totals::default();

    // Always set the header, even with nothing to record in it. Left to itself zlib-ng stamps
    // its own OS code, which on some platforms is a value RFC 1952 does not define.
    strm.set_header(params.mtime, params.name, format::OS_CODE);
    loop {
        let have = read_full(&mut input, &mut in_buf)?;
        let last = have < IO_CHUNK;
        let mut pos = 0;

        // An empty read still runs once, which is what emits Z_FINISH.
        loop {
            let mut span = have - pos;
            let mut flush = Z_NO_FLUSH;

            // With --rsyncable the span ends at the next rolling hash hit, with a flush there.
            if params.rsyncable {
                let hit = rolling::find(&mut rsync_hash, rsync_mask, &in_buf[pos..have], 0);
                if hit < span {
                    span = hit + 1;
                    flush = Z_SYNC_FLUSH;
                }
            }
            pos += span;
            if last && pos == have {
                flush = Z_FINISH;
            }
            let piece = &in_buf[pos - span..pos];
            totals.total_out += deflate_span(&mut strm, &mut out, &mut out_buf, piece, flush)?;
            if pos >= have {
                break;
            }
        }
        totals.total_in += have as u64;
        if last {
            break;
        }
    }
    out.flush()?;
    Ok(totals)
}
