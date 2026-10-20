// codec.rs -- the deflate and inflate the pool runs over one slot at a time
// For conditions of distribution and use, see LICENSE.md

#[cfg(feature = "zopfli")]
use std::io::{self, Write};

#[cfg(feature = "zopfli")]
use zopfli::{BlockType, DeflateEncoder, Options};

use crate::gzblock::MAX_BLOCK;
use crate::pool::{Mode, Slot};
use crate::zng::{self, Deflater, Inflater, MAX_WBITS, Z_BLOCK, Z_BUF_ERROR, Z_FINISH, Z_FULL_FLUSH, Z_NO_FLUSH};
use crate::zng::{Z_OK, Z_STREAM_END, Z_SYNC_FLUSH};

#[cfg(feature = "zopfli")]
use crate::zng::Strategy;

/// How working a slot ended. For a segment being inflated, Full is a block whose output and
/// trailing marker were consumed, End is the block that ends the deflate stream, Short ran out of
/// input before either, Overflow wanted more output than a block may have, Error is invalid data.
/// For a block being deflated, Full is a complete block with all of its input consumed, Error is
/// anything else.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Full,
    End,
    Short,
    Overflow,
    Error,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Full => "complete",
            Status::End => "end of stream",
            Status::Short => "truncated",
            Status::Overflow => "larger than the block size",
            Status::Error => "corrupt",
        }
    }
}

/// The output is full. A block may take any size, so double the buffer, up to MAX_BLOCK. Returns
/// true with room to inflate into.
fn grow_output(slot: &mut Slot) -> bool {
    let have = slot.out.len();
    let size = (have * 2).min(MAX_BLOCK as usize);

    // Out of memory reads as a block too large to hold, which it is.
    if size <= have || slot.out.try_reserve_exact(size - have).is_err() {
        return false;
    }
    slot.out.resize(size, 0);
    true
}

/// Inflate one segment into the slot's output and say how it ended, with the bytes consumed and
/// produced. Every segment ends at a marker pair or the end of the stream and is a block at
/// whatever size it produces, since no chance pattern makes a pair, so its output grows on demand
/// and any clean end of the input completes it.
fn inflate_segment(strm: &mut Inflater, slot: &mut Slot) -> (Status, usize, usize) {
    let (mut in_pos, mut out_pos) = (0, 0);

    strm.reset2(-MAX_WBITS); // back to raw, a member slot may have rewrapped it
    loop {
        // A full buffer grows before inflate needs the room. One at the cap is left to inflate,
        // which needs none for the empty stored blocks of a marker.
        if out_pos == slot.out.len() {
            grow_output(slot);
        }
        // Z_BLOCK returns at every block boundary, where data_type reports the position.
        let mut step = strm.inflate(&slot.input[in_pos..slot.in_len], &mut slot.out[out_pos..], Z_BLOCK);
        in_pos += step.consumed;
        out_pos += step.produced;
        if step.rc == Z_OK && strm.data_type() & (64 | 128) == (64 | 128) {
            // past the final block, conclude the stream
            step = strm.inflate(&slot.input[in_pos..slot.in_len], &mut slot.out[out_pos..], Z_BLOCK);
            in_pos += step.consumed;
            out_pos += step.produced;
        }
        if step.rc == Z_STREAM_END {
            return (Status::End, in_pos, out_pos);
        }
        if step.rc == Z_BUF_ERROR && out_pos == slot.out.len() {
            return (Status::Overflow, in_pos, out_pos);
        }
        if step.rc != Z_OK {
            return (Status::Error, in_pos, out_pos);
        }
        // Complete once the input ends just past a deflate block on a byte boundary. Ending
        // anywhere else, the cut fell inside a block.
        if in_pos == slot.in_len {
            let whole = strm.data_type() & 128 != 0 && strm.data_type() & 7 == 0;
            return (if whole { Status::Full } else { Status::Short }, in_pos, out_pos);
        }
    }
}

/// Inflate whole gzip members back to back, zlib checking each header, crc, and length. Sized
/// members cut exactly, so the only clean end is all input consumed at a member's end.
fn inflate_members(strm: &mut Inflater, slot: &mut Slot) -> (Status, usize, usize) {
    let (mut in_pos, mut out_pos) = (0, 0);

    strm.reset2(MAX_WBITS + 16);
    loop {
        let step = strm.inflate(&slot.input[in_pos..slot.in_len], &mut slot.out[out_pos..], Z_NO_FLUSH);
        in_pos += step.consumed;
        out_pos += step.produced;
        if step.rc == Z_STREAM_END {
            if in_pos == slot.in_len {
                return (Status::Full, in_pos, out_pos);
            }
            strm.reset(); // the next member follows directly
            continue;
        }
        if step.rc != Z_OK {
            return (Status::Error, in_pos, out_pos);
        }
        if out_pos == slot.out.len() {
            if !grow_output(slot) {
                return (Status::Overflow, in_pos, out_pos);
            }
            continue;
        }
        if in_pos == slot.in_len {
            return (Status::Short, in_pos, out_pos); // input ended inside a member
        }
    }
}

/// Run a member slot. The crc is left to the per-member trailers zlib already checked.
fn run_members(strm: &mut Inflater, slot: &mut Slot) {
    (slot.status, slot.in_used, slot.out_len) = inflate_members(strm, slot);
    slot.crc = 0;
}

/// Inflate one slot and record what every exit of inflate_segment shares, the bytes consumed and
/// produced and the CRC of the output.
fn run_segment(strm: &mut Inflater, slot: &mut Slot) {
    (slot.status, slot.in_used, slot.out_len) = inflate_segment(strm, slot);
    slot.crc = zng::crc32(&slot.out[..slot.out_len]);
}

/// Deflate one block on a fresh raw stream. A full flush ends it on a byte boundary with the
/// empty stored block marker, the last block ends the deflate stream instead.
fn run_block(strm: &mut Deflater, slot: &mut Slot, out_size: usize) {
    let input = &slot.input[..slot.in_len];
    let out = &mut slot.out[..out_size];

    strm.reset();
    strm.params(slot.level, slot.strategy);
    let mut step = strm.deflate(input, out, if slot.last { Z_FINISH } else { Z_SYNC_FLUSH });
    let (mut in_used, mut out_len) = (step.consumed, step.produced);
    if !slot.last && step.rc == Z_OK {
        // the second marker makes it a boundary
        step = strm.deflate(&input[in_used..], &mut out[out_len..], Z_FULL_FLUSH);
        in_used += step.consumed;
        out_len += step.produced;
    }
    let done = if slot.last {
        step.rc == Z_STREAM_END
    } else {
        step.rc == Z_OK && in_used == slot.in_len && out_len != out_size
    };
    slot.out_len = out_len;
    slot.in_used = in_used;
    slot.status = if done { Status::Full } else { Status::Error };
    slot.crc = zng::crc32(input);
}

/// Deflate one block with zopfli. A block before the last ends on the marker pair rather than
/// the end of the stream, which zopfli has no call for until zopfli-rs/zopfli#91, so the empty
/// final block it ends with is rewritten.
#[cfg(feature = "zopfli")]
fn zopfli_block(input: &[u8], last: bool) -> io::Result<Vec<u8>> {
    let mut enc = DeflateEncoder::new(Options::default(), BlockType::Dynamic, Vec::new());

    enc.write_all(input)?;
    if last {
        return enc.finish();
    }
    // Any write pushes the pending input out as non-final blocks, so finish only adds an empty
    // final block, which zopfli writes fixed, as 1 10 0000000, padded with zeros.
    let _ = enc.write(&[])?;
    let mut out = enc.finish()?;

    // Its btype low bit is the highest bit set, and BFINAL the bit below it where the block
    // starts. Anything else at the end is not that block.
    let top = out
        .iter()
        .rposition(|&b| b != 0)
        .map(|at| at * 8 + 7 - out[at].leading_zeros() as usize);
    let Some(start) = top.and_then(|top| top.checked_sub(1)) else {
        return Err(io::Error::other("zopfli wrote no final block"));
    };
    if !(10..18).contains(&(out.len() * 8 - start)) || out[start / 8] & (1 << (start % 8)) == 0 {
        return Err(io::Error::other("zopfli ended on an unexpected block"));
    }
    out.truncate(start / 8 + 1);
    out[start / 8] &= (1 << (start % 8)) - 1;

    // In its place the marker pair, two empty stored blocks, the first padded to a byte.
    if start % 8 > 5 {
        out.push(0); // its 3 bit header spills into the next byte
    }
    out.extend_from_slice(&[0, 0, 0xff, 0xff, 0, 0, 0, 0xff, 0xff]);
    Ok(out)
}

/// Deflate one block with zopfli, done on the same terms as run_block.
#[cfg(feature = "zopfli")]
fn run_zopfli(slot: &mut Slot, out_size: usize) {
    let input = &slot.input[..slot.in_len];
    let out = zopfli_block(input, slot.last).ok().filter(|out| out.len() <= out_size);

    slot.out_len = 0;
    if let Some(out) = &out {
        slot.out[..out.len()].copy_from_slice(out);
        slot.out_len = out.len();
    }
    slot.in_used = slot.in_len;
    slot.status = if out.is_some() { Status::Full } else { Status::Error };
    slot.crc = zng::crc32(input);
}

/// One persistent stream per worker, deflate or inflate by the pool's mode.
pub enum Codec {
    Deflate(Deflater),
    Inflate(Inflater),
}

impl Codec {
    pub fn new(mode: Mode) -> Option<Self> {
        match mode {
            Mode::Deflate { level, strategy } => Deflater::new(level, -MAX_WBITS, strategy).map(Codec::Deflate),
            Mode::Inflate => Inflater::new(-MAX_WBITS).map(Codec::Inflate),
        }
    }

    pub fn run(&mut self, slot: &mut Slot, out_size: usize) {
        match self {
            #[cfg(feature = "zopfli")]
            Codec::Deflate(_) if slot.strategy == Strategy::Zopfli => run_zopfli(slot, out_size),
            Codec::Deflate(strm) => run_block(strm, slot, out_size),
            Codec::Inflate(strm) if slot.members != 0 => run_members(strm, slot),
            Codec::Inflate(strm) => run_segment(strm, slot),
        }
    }
}

#[cfg(all(test, feature = "zopfli"))]
mod tests {
    use super::*;

    /// Two blocks joined, the first ending on the marker pair, inflated as one raw stream.
    fn joined_inflate(first: &[u8], second: &[u8]) -> Vec<u8> {
        let mut stream = zopfli_block(first, false).unwrap();
        let mut strm = Inflater::new(-MAX_WBITS).unwrap();
        let mut out = vec![0; first.len() + second.len() + 1];

        assert!(stream.ends_with(&[0, 0, 0xff, 0xff, 0, 0, 0, 0xff, 0xff]));
        stream.extend(zopfli_block(second, true).unwrap());
        let step = strm.inflate(&stream, &mut out, Z_FINISH);
        assert_eq!(Z_STREAM_END, step.rc);
        assert_eq!(stream.len(), step.consumed);
        out.truncate(step.produced);
        out
    }

    #[test]
    fn zopfli_blocks_join_at_every_bit_offset() {
        let data: Vec<u8> = (0..4000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 27) as u8)
            .collect();

        // Lengths enough to end the first block at each of a byte's eight bit offsets.
        for len in (0..128).chain([3000]) {
            let (first, second) = (&data[..len], &data[len..len + 100]);
            assert_eq!(data[..len + 100], joined_inflate(first, second), "length {len}");
        }
    }
}
