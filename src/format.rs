// format.rs -- the bytes of the gzip header and trailer
// For conditions of distribution and use, see LICENSE.md

use crate::zng::Strategy;

/// The fixed bytes, before any optional field.
pub const HEADER_LEN: usize = 10;
/// crc32 and size, the member ending.
pub const TRAILER_LEN: usize = 8;

/// Longest file name stored in or read from a header, including the terminator.
pub const NAME_MAX: usize = 256;

/// Longest header header_build() can produce, the fixed ten bytes and the stored name.
pub const HEADER_MAX: usize = HEADER_LEN + NAME_MAX;

/// The header's OS byte, RFC 1952's code for the platform the member was made on.
pub const OS_CODE: u8 = if cfg!(windows) { 0 } else { 3 };

/// What a member header records.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Header<'a> {
    /// 0 stores no time.
    pub mtime: u32,
    /// None or empty stores no name.
    pub name: Option<&'a [u8]>,
    /// The extra flags byte reports how hard deflate worked.
    pub level: i32,
    pub strategy: Strategy,
    /// Whole member bytes when a BGZF BC or MiGz MZ subfield records them, 0 otherwise.
    pub member_size: u32,
}

/// How far header_parse() got.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parse {
    /// The whole header, this many bytes long.
    Complete(usize),
    /// More bytes are needed.
    More,
    /// This is not a gzip header.
    Invalid,
}

// gzip stores its fields little endian.
fn load_le16(buf: &[u8]) -> u16 {
    u16::from_le_bytes([buf[0], buf[1]])
}

fn load_le32(buf: &[u8]) -> u32 {
    u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
}

/// Whether buf starts with the gzip magic, which two bytes decide.
pub fn is_gzip(buf: &[u8]) -> bool {
    buf.starts_with(&[0x1f, 0x8b])
}

/// Lay a member header into buf. Returns its length.
pub fn header_build(buf: &mut [u8; HEADER_MAX], hdr: &Header) -> usize {
    // A name that does not fit is omitted rather than truncated.
    let name = hdr.name.filter(|name| !name.is_empty() && name.len() < NAME_MAX);
    let mut n = HEADER_LEN;

    buf[..HEADER_LEN].fill(0);
    buf[0] = 0x1f;
    buf[1] = 0x8b;
    buf[2] = 8; // deflate
    buf[3] = if name.is_some() { 8 } else { 0 }; // FNAME
    buf[4..8].copy_from_slice(&hdr.mtime.to_le_bytes());
    // Extra flags, 2 for the slowest deflate settings, 4 for the fastest.
    buf[8] = if hdr.level == 9 || hdr.strategy == Strategy::Zopfli {
        2
    } else if hdr.strategy >= Strategy::HuffmanOnly || (0..2).contains(&hdr.level) {
        4
    } else {
        0
    };
    buf[9] = OS_CODE;
    if let Some(name) = name {
        buf[n..n + name.len()].copy_from_slice(name);
        buf[n + name.len()] = 0;
        n += name.len() + 1;
    }
    n
}

/// Sized members make the next member's position known before anything is inflated. Returns the
/// whole member's length, or 0 when no subfield records a size.
fn extra_member_size(extra: &[u8], data_size: &mut u32) -> u32 {
    let mut pos = 0;

    while pos + 4 <= extra.len() {
        let sub = usize::from(load_le16(&extra[pos + 2..]));
        // BGZF (bgzip, BAM) records the whole member's length in BC.
        if extra[pos] == b'B' && extra[pos + 1] == b'C' && sub == 2 && pos + 6 <= extra.len() {
            return u32::from(load_le16(&extra[pos + 4..])) + 1;
        }
        // MiGz records only the deflate data's length in MZ, completed by the caller once the
        // whole header has been measured.
        if extra[pos] == b'M' && extra[pos + 1] == b'Z' && sub == 4 && pos + 8 <= extra.len() {
            *data_size = load_le32(&extra[pos + 4..]);
        }
        pos += 4 + sub;
    }
    0
}

/// Walk a member header, over any extra field, name, comment, and header crc it carries. The
/// header returned holds the time and the stored name, the name borrowed from buf, each as soon
/// as its bytes are in hand, so a header that is not all there still gives up what it has. Level
/// and strategy are not recorded.
pub fn header_parse(buf: &[u8]) -> (Parse, Header<'_>) {
    let mut hdr = Header::default();
    let mut pos = HEADER_LEN;
    let mut data_size = 0;
    let len = buf.len();

    if len < HEADER_LEN {
        return (Parse::More, hdr);
    }
    if !is_gzip(buf) || buf[2] != 8 {
        return (Parse::Invalid, hdr);
    }
    let flags = buf[3];
    if flags & 0xe0 != 0 {
        return (Parse::Invalid, hdr);
    }
    hdr.mtime = load_le32(&buf[4..]);

    if flags & 4 != 0 {
        // FEXTRA
        if len < pos + 2 {
            return (Parse::More, hdr);
        }
        let xlen = usize::from(load_le16(&buf[pos..]));
        pos += 2;
        let end = pos + xlen;
        if len < end {
            return (Parse::More, hdr);
        }
        hdr.member_size = extra_member_size(&buf[pos..end], &mut data_size);
        pos = end;
    }
    if flags & 8 != 0 {
        // FNAME
        let Some(name_len) = memchr::memchr(0, &buf[pos..]) else {
            return (Parse::More, hdr);
        };
        hdr.name = Some(&buf[pos..pos + name_len]);
        pos += name_len + 1;
    }
    if flags & 16 != 0 {
        // FCOMMENT
        let Some(comment_len) = memchr::memchr(0, &buf[pos..]) else {
            return (Parse::More, hdr);
        };
        pos += comment_len + 1;
    }
    if flags & 2 != 0 {
        // FHCRC
        if len < pos + 2 {
            return (Parse::More, hdr);
        }
        pos += 2;
    }
    // An MZ size counts only the deflate data, the header and trailer complete the member.
    if hdr.member_size == 0 && data_size != 0 {
        let member_size = pos as u64 + u64::from(data_size) + TRAILER_LEN as u64;
        if member_size < u64::from(u32::MAX) {
            hdr.member_size = member_size as u32;
        }
    }
    (Parse::Complete(pos), hdr)
}

/// The member trailer, crc32 of the uncompressed data and its length modulo 2^32, which is all
/// gzip records however large the member was.
pub fn trailer_build(crc: u32, total: u64) -> [u8; TRAILER_LEN] {
    let mut buf = [0; TRAILER_LEN];

    buf[..4].copy_from_slice(&crc.to_le_bytes());
    buf[4..].copy_from_slice(&(total as u32).to_le_bytes());
    buf
}

/// The crc and length a trailer records.
pub fn trailer_parse(buf: &[u8; TRAILER_LEN]) -> (u32, u32) {
    (load_le32(buf), load_le32(&buf[4..]))
}
