// scanner.rs -- finding the empty stored block marker in compressed bytes
// For conditions of distribution and use, see LICENSE.md

use std::sync::LazyLock;

use memchr::memmem::Finder;

// The searchers are built once and shared by every scan.
static MARKER: LazyLock<Finder<'static>> = LazyLock::new(|| Finder::new(&[0, 0, 0xff, 0xff]));
static MARKER_PAIR: LazyLock<Finder<'static>> = LazyLock::new(|| Finder::new(&[0, 0, 0xff, 0xff, 0, 0, 0, 0xff, 0xff]));

/// The empty stored block that continues a marker, 5 more bytes of 00 00 00 FF FF.
pub fn scan_empty_block(p: &[u8]) -> bool {
    p.starts_with(&[0, 0, 0, 0xff, 0xff])
}

/// The first 00 00 FF FF at or after start, end bounding the last start position considered, or
/// None. end + 3 must be within buf.
pub fn scan_marker(buf: &[u8], start: usize, end: usize) -> Option<usize> {
    MARKER.find(&buf[start..end + 3]).map(|at| start + at)
}

/// The first marker with an empty stored block right behind it, at or after start, end bounding
/// the last start position considered, or None. end + 8 must be within buf.
pub fn scan_marker_pair(buf: &[u8], start: usize, end: usize) -> Option<usize> {
    MARKER_PAIR.find(&buf[start..end + 8]).map(|at| start + at)
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
