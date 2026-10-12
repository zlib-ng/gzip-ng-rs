// zng.rs -- the calls into zlib-ng, deflate, inflate, and crc over slices
// For conditions of distribution and use, see LICENSE.md

use std::ffi::{CStr, CString, c_ulong};
use std::mem;
use std::ptr;
use std::sync::Once;

use crate::sys as z;

pub use z::{Z_BLOCK, Z_BUF_ERROR, Z_FINISH, Z_FULL_FLUSH, Z_NO_FLUSH, Z_OK, Z_STREAM_END, Z_SYNC_FLUSH};

/// The largest window, which every stream here uses. Negated it asks for raw deflate, and 16 more
/// for the gzip wrapper.
pub const MAX_WBITS: i32 = 15;

const MEM_LEVEL: i32 = 8;

/// Deflate strategies, with zlib's own values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
#[repr(i32)]
pub enum Strategy {
    #[default]
    Default = 0,
    Filtered = 1,
    HuffmanOnly = 2,
    Rle = 3,
    Fixed = 4,
}

/// How one call into deflate or inflate returned and how far it moved through each buffer.
#[derive(Clone, Copy, Debug)]
pub struct Step {
    pub rc: i32,
    pub consumed: usize,
    pub produced: usize,
}

/// zlib measures its buffers in 32 bits, so a slice goes over a chunk at a time.
fn clamp_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// The first call into zlib-ng fills its function table, with nothing to keep two threads from
/// filling it at once, so that call is made here, alone.
fn functable_init() {
    static FILLED: Once = Once::new();

    FILLED.call_once(|| {
        // SAFETY: a stream of zeros is a valid one, as init expects, and it is ended only once
        // initialized.
        unsafe {
            let mut strm: z::zng_stream = mem::zeroed();
            if z::zng_inflateInit2(&mut strm, MAX_WBITS) == Z_OK {
                z::zng_inflateEnd(&mut strm);
            }
        }
    });
}

/// A stream starts out zeroed, and init fills in the allocator along with the state. It lives in
/// a box because the state points back at it.
fn stream_new(init: impl FnOnce(*mut z::zng_stream) -> i32) -> Option<Box<z::zng_stream>> {
    // SAFETY: a stream of zeros is a valid one, null pointers and no callbacks.
    let mut strm: Box<z::zng_stream> = Box::new(unsafe { mem::zeroed() });

    functable_init();
    if init(&mut *strm) != Z_OK {
        return None;
    }
    Some(strm)
}

/// Point the stream at the two buffers for one call and report how far each moved.
fn step(strm: &mut z::zng_stream, input: &[u8], out: &mut [u8], call: impl FnOnce(*mut z::zng_stream) -> i32) -> Step {
    let avail_in = clamp_u32(input.len());
    let avail_out = clamp_u32(out.len());

    strm.next_in = input.as_ptr();
    strm.avail_in = avail_in;
    strm.next_out = out.as_mut_ptr();
    strm.avail_out = avail_out;
    let rc = call(strm);
    Step {
        rc,
        consumed: (avail_in - strm.avail_in) as usize,
        produced: (avail_out - strm.avail_out) as usize,
    }
}

pub struct Inflater {
    strm: Box<z::zng_stream>,
}

// SAFETY: a stream owns its state and has no tie to the thread that made it.
unsafe impl Send for Inflater {}

impl Inflater {
    pub fn new(window_bits: i32) -> Option<Self> {
        // SAFETY: the stream is zeroed, as init expects.
        stream_new(|strm| unsafe { z::zng_inflateInit2(strm, window_bits) }).map(|strm| Self { strm })
    }

    pub fn reset(&mut self) {
        // SAFETY: the stream is initialized.
        unsafe { z::zng_inflateReset(&mut *self.strm) };
    }

    pub fn reset2(&mut self, window_bits: i32) {
        // SAFETY: the stream is initialized.
        unsafe { z::zng_inflateReset2(&mut *self.strm, window_bits) };
    }

    pub fn inflate(&mut self, input: &[u8], out: &mut [u8], flush: i32) -> Step {
        // SAFETY: the stream is initialized and its buffers are the two slices, at their lengths.
        step(&mut self.strm, input, out, |strm| unsafe {
            z::zng_inflate(strm, flush)
        })
    }

    /// Where the last inflate stopped. 128 is set at the end of a block, 64 while in the final
    /// block, and the low bits count the unused bits of the last byte.
    pub fn data_type(&self) -> i32 {
        self.strm.data_type
    }

    pub fn msg(&self) -> Option<&str> {
        if self.strm.msg.is_null() {
            return None;
        }
        // SAFETY: zlib-ng points msg at one of its own constant strings.
        unsafe { CStr::from_ptr(self.strm.msg) }.to_str().ok()
    }
}

impl Drop for Inflater {
    fn drop(&mut self) {
        // SAFETY: the stream is initialized.
        unsafe { z::zng_inflateEnd(&mut *self.strm) };
    }
}

pub struct Deflater {
    strm: Box<z::zng_stream>,
    // The gzip header and its name, which deflate reads through pointers until it has written
    // them out.
    head: Option<Box<z::zng_gz_header>>,
    name: Option<CString>,
}

// SAFETY: a stream owns its state and has no tie to the thread that made it.
unsafe impl Send for Deflater {}

impl Deflater {
    pub fn new(level: i32, window_bits: i32, strategy: Strategy) -> Option<Self> {
        // SAFETY: the stream is zeroed, as init expects.
        stream_new(|strm| unsafe {
            z::zng_deflateInit2(strm, level, z::Z_DEFLATED, window_bits, MEM_LEVEL, strategy as i32)
        })
        .map(|strm| Self {
            strm,
            head: None,
            name: None,
        })
    }

    pub fn reset(&mut self) {
        // SAFETY: the stream is initialized.
        unsafe { z::zng_deflateReset(&mut *self.strm) };
    }

    /// Settings for what is deflated next, on a stream fresh from a reset.
    pub fn params(&mut self, level: i32, strategy: Strategy) {
        // SAFETY: the stream is initialized, and with nothing deflated yet no buffer is touched.
        unsafe { z::zng_deflateParams(&mut *self.strm, level, strategy as i32) };
    }

    /// The most bytes deflating len bytes in one call can produce.
    pub fn bound(&mut self, len: usize) -> usize {
        // SAFETY: the stream is initialized.
        unsafe { z::zng_deflateBound(&mut *self.strm, len as c_ulong) as usize }
    }

    /// What the gzip wrapper's header records, before the first deflate. A name holding a zero
    /// byte is left out.
    pub fn set_header(&mut self, mtime: u32, name: Option<&[u8]>, os: u8) {
        self.name = name.and_then(|name| CString::new(name).ok());
        let head = self.head.insert(Box::new(z::zng_gz_header {
            text: 0,
            time: mtime as c_ulong,
            xflags: 0,
            os: i32::from(os),
            extra: ptr::null_mut(),
            extra_len: 0,
            extra_max: 0,
            name: self
                .name
                .as_ref()
                .map_or(ptr::null_mut(), |name| name.as_ptr().cast_mut().cast()),
            name_max: 0,
            comment: ptr::null_mut(),
            comm_max: 0,
            hcrc: 0,
            done: 0,
        }));
        // SAFETY: the stream is initialized, and the header and its name stay boxed in self.
        unsafe { z::zng_deflateSetHeader(&mut *self.strm, &mut **head) };
    }

    pub fn deflate(&mut self, input: &[u8], out: &mut [u8], flush: i32) -> Step {
        // SAFETY: the stream is initialized and its buffers are the two slices, at their lengths.
        step(&mut self.strm, input, out, |strm| unsafe {
            z::zng_deflate(strm, flush)
        })
    }
}

impl Drop for Deflater {
    fn drop(&mut self) {
        // SAFETY: the stream is initialized.
        unsafe { z::zng_deflateEnd(&mut *self.strm) };
    }
}

/// The crc32 of buf.
pub fn crc32(buf: &[u8]) -> u32 {
    // SAFETY: the pointer and length are the slice's own.
    unsafe { z::zng_crc32_z(0, buf.as_ptr(), buf.len()) }
}

// The crc polynomial, reflected.
const POLY: u32 = 0xedb8_8320;

/// a times b modulo the polynomial. a must not be zero.
const fn multmodp(a: u32, mut b: u32) -> u32 {
    let mut m = 1u32 << 31;
    let mut p = 0;

    loop {
        if a & m != 0 {
            p ^= b;
            if a & (m - 1) == 0 {
                break;
            }
        }
        m >>= 1;
        b = if b & 1 != 0 { (b >> 1) ^ POLY } else { b >> 1 };
    }
    p
}

// x^(2^n) modulo the polynomial, for each n a 32 bit shift count can take.
const X2N: [u32; 32] = {
    let mut table = [0; 32];
    let mut p = 1u32 << 30; // x^1
    let mut n = 1;

    table[0] = p;
    while n < 32 {
        p = multmodp(p, p);
        table[n] = p;
        n += 1;
    }
    table
};

/// x^(n * 2^k) modulo the polynomial.
fn x2nmodp(mut n: u64, mut k: usize) -> u32 {
    let mut p = 1u32 << 31; // x^0

    while n != 0 {
        if n & 1 != 0 {
            p = multmodp(X2N[k & 31], p);
        }
        n >>= 1;
        k += 1;
    }
    p
}

/// The crc32 of two runs of bytes laid end to end, from the crc of each and the length of the
/// second. It is the arithmetic of zlib-ng's crc32_combine, kept here because the width of that
/// function's length argument depends on how the library was built.
pub fn crc32_combine(crc1: u32, crc2: u32, len2: u64) -> u32 {
    multmodp(x2nmodp(len2, 3), crc1) ^ crc2
}

/// Version of the zlib-ng this was built against.
pub fn version() -> &'static str {
    // SAFETY: the version is a constant string in the library.
    unsafe { CStr::from_ptr(z::zlibng_version()) }.to_str().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combine_matches_the_crc_of_the_whole() {
        let data: Vec<u8> = (0..200_000u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();

        for cut in [0, 1, 7, 4096, 65_521, 199_999, 200_000] {
            let (a, b) = data.split_at(cut);
            assert_eq!(
                crc32(&data),
                crc32_combine(crc32(a), crc32(b), b.len() as u64),
                "cut {cut}"
            );
        }
    }
}
