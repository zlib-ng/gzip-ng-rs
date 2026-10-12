// scanner.rs -- finding the empty stored block marker in compressed bytes
// For conditions of distribution and use, see LICENSE.md

/// The empty stored block that continues a marker, 5 more bytes of 00 00 00 FF FF.
pub fn scan_empty_block(p: &[u8]) -> bool {
    p.starts_with(&[0, 0, 0, 0xff, 0xff])
}

/// The check behind the zero filter, the marker's remaining bytes, and for the pair scan the
/// empty stored block behind them. PAIR is a constant at every call, so each caller gets its own
/// specialized scan.
///
/// # Safety
/// q + 3 must be readable, q + 8 for the pair scan.
#[inline(always)]
unsafe fn scan_hit<const PAIR: bool>(q: *const u8) -> bool {
    // SAFETY: the caller keeps these bytes in bounds.
    unsafe {
        if *q.add(1) != 0 || *q.add(2) != 0xff || *q.add(3) != 0xff {
            return false;
        }
        !PAIR || q.add(4).cast::<[u8; 5]>().read() == [0, 0, 0, 0xff, 0xff]
    }
}

/// The scalar scan, also the tail behind the vector ones. The first hit starting in [start, end),
/// or None.
///
/// # Safety
/// end + 3 must be within buf, end + 8 for the pair scan.
#[inline(always)]
unsafe fn scan_scalar<const PAIR: bool>(buf: &[u8], mut start: usize, end: usize) -> Option<usize> {
    while start < end {
        start += memchr::memchr(0, &buf[start..end])?;
        // SAFETY: start is short of end, which the caller keeps in bounds.
        if unsafe { scan_hit::<PAIR>(buf.as_ptr().add(start)) } {
            return Some(start);
        }
        start += 1;
    }
    None
}

// The first hit starting in [start, end), or None, bounds as in scan_scalar. The vector versions
// filter for the zero byte the way libc memchr does, one load and one reduction per 16 bytes, but
// stay inline so the roughly one hit per 256 bytes that compressed data produces costs a few
// cycles instead of a call boundary. Candidates get the full byte check, real markers are
// kilobytes apart.

#[cfg(all(feature = "simd", target_arch = "aarch64", target_feature = "neon"))]
#[inline(always)]
unsafe fn scan<const PAIR: bool>(buf: &[u8], mut start: usize, end: usize) -> Option<usize> {
    use core::arch::aarch64::*;

    let p = buf.as_ptr();
    while end - start >= 16 {
        // SAFETY: the 16 bytes at start end at or before end, and a candidate starts short of it.
        unsafe {
            let v = vld1q_u8(p.add(start));
            if vminvq_u8(v) == 0 {
                // four mask bits per lane, the usual movemask substitute
                let narrowed = vshrn_n_u16::<4>(vreinterpretq_u16_u8(vceqzq_u8(v)));
                let mut mask = vget_lane_u64::<0>(vreinterpret_u64_u8(narrowed));
                // The minimum already said a lane is zero, so the mask is tested only after use.
                loop {
                    let i = (mask.trailing_zeros() >> 2) as usize;
                    if scan_hit::<PAIR>(p.add(start + i)) {
                        return Some(start + i);
                    }
                    mask &= !(0xf << (i * 4));
                    if mask == 0 {
                        break;
                    }
                }
            }
        }
        start += 16;
    }
    // SAFETY: the caller's bounds are the scalar scan's own.
    unsafe { scan_scalar::<PAIR>(buf, start, end) }
}

#[cfg(all(
    feature = "simd",
    any(target_arch = "x86_64", target_arch = "x86"),
    target_feature = "sse2"
))]
#[inline(always)]
unsafe fn scan<const PAIR: bool>(buf: &[u8], mut start: usize, end: usize) -> Option<usize> {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::*;
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::*;

    let p = buf.as_ptr();
    while end - start >= 16 {
        // SAFETY: the 16 bytes at start end at or before end, and a candidate starts short of it.
        unsafe {
            let v = _mm_loadu_si128(p.add(start).cast());
            let mut mask = _mm_movemask_epi8(_mm_cmpeq_epi8(v, _mm_setzero_si128())) as u32;
            while mask != 0 {
                let i = mask.trailing_zeros() as usize;
                if scan_hit::<PAIR>(p.add(start + i)) {
                    return Some(start + i);
                }
                mask &= mask - 1;
            }
        }
        start += 16;
    }
    // SAFETY: the caller's bounds are the scalar scan's own.
    unsafe { scan_scalar::<PAIR>(buf, start, end) }
}

#[cfg(not(all(
    feature = "simd",
    any(
        all(target_arch = "aarch64", target_feature = "neon"),
        all(any(target_arch = "x86_64", target_arch = "x86"), target_feature = "sse2")
    )
)))]
#[inline(always)]
unsafe fn scan<const PAIR: bool>(buf: &[u8], start: usize, end: usize) -> Option<usize> {
    // SAFETY: the caller's bounds are the scalar scan's own.
    unsafe { scan_scalar::<PAIR>(buf, start, end) }
}

/// The first 00 00 FF FF at or after start, end bounding the last start position considered, or
/// None. NEON and SSE2 filter for the zero pair the way memchr filters for a byte. end + 3 must
/// be within buf.
pub fn scan_marker(buf: &[u8], start: usize, end: usize) -> Option<usize> {
    assert!(end + 3 <= buf.len());
    // SAFETY: the bound was just checked.
    unsafe { scan::<false>(buf, start, end) }
}

/// The first marker with an empty stored block right behind it, at or after start, end bounding
/// the last start position considered, or None. end + 8 must be within buf.
pub fn scan_marker_pair(buf: &[u8], start: usize, end: usize) -> Option<usize> {
    assert!(end + 8 <= buf.len());
    // SAFETY: the bound was just checked.
    unsafe { scan::<true>(buf, start, end) }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARKER: [u8; 4] = [0, 0, 0xff, 0xff];
    const PAIR: [u8; 9] = [0, 0, 0xff, 0xff, 0, 0, 0, 0xff, 0xff];

    /// Bytes with zeros sprinkled in, so the filter has candidates to turn down.
    fn noise(len: usize) -> Vec<u8> {
        let mut s = 0x2545_f491u32;
        (0..len)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                if s >> 28 == 0 { 0 } else { (s >> 16) as u8 | 1 }
            })
            .collect()
    }

    #[test]
    fn marker_is_found_at_every_offset() {
        for at in 0..100 {
            let mut buf = noise(200);
            buf[at..at + 4].copy_from_slice(&MARKER);
            assert_eq!(Some(at), scan_marker(&buf, 0, buf.len() - 3), "at {at}");
            assert_eq!(None, scan_marker(&buf, at + 1, buf.len() - 3), "past {at}");
            assert_eq!(None, scan_marker(&buf, 0, at), "short of {at}");
        }
    }

    #[test]
    fn pair_scan_passes_over_lone_markers() {
        for at in 0..100 {
            let mut buf = noise(200);
            buf[at..at + 4].copy_from_slice(&MARKER);
            buf[at + 40..at + 49].copy_from_slice(&PAIR);
            assert_eq!(Some(at + 40), scan_marker_pair(&buf, 0, buf.len() - 8), "at {at}");
        }
    }

    #[test]
    fn empty_block_wants_all_five_bytes() {
        assert!(scan_empty_block(&PAIR[4..]));
        assert!(!scan_empty_block(&PAIR[5..]));
        assert!(!scan_empty_block(&MARKER));
    }
}
