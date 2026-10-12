// rolling.rs -- content-defined boundaries for rsyncable output
// For conditions of distribution and use, see LICENSE.md

/// One random word per byte value, fixed here rather than generated so that boundaries do not
/// depend on the build.
#[rustfmt::skip]
pub const GEAR: [u32; 256] = [
    0xdc1b77ae, 0x64f0eeb9, 0x7b07ce91, 0x305f050c, 0x2ceb16e0, 0x97101dce, 0x9ad2e144, 0xd9aa792e,
    0xddaa4e85, 0x8f8ea9d3, 0x08f474ff, 0x2ead8547, 0x55bc79f8, 0x0e1fc49b, 0xb92199e8, 0xc5765079,
    0x353cfc38, 0xa32edabf, 0xfc5639b1, 0x92fb2dcf, 0x544b0ec7, 0xbcbb9b7e, 0x0f1a50d5, 0x80ae2120,
    0x0e1ecd02, 0x0d0981e8, 0xdc86b3d3, 0x6d0844c2, 0x3681da7f, 0x5b928e2c, 0x6c716e1e, 0xccb87021,
    0x9f07b27a, 0xb0ba91e4, 0xc72b4c36, 0x35f305b0, 0x7ac78fb3, 0x8e39b81d, 0x59a69ba1, 0xbfcbffff,
    0x2d1ce288, 0x572a15ed, 0x32b91149, 0x17a8ec88, 0x0cc88eab, 0xbc3a5b41, 0x6f0f3414, 0xe71a7567,
    0x63674cf8, 0xe375837d, 0x578eb22e, 0x4efa582e, 0xf3b363e9, 0x2e3a4ef3, 0xb809d78e, 0x46217f10,
    0x8947b5fe, 0x4a3ae29b, 0xbfd0273b, 0xed0a00eb, 0x3c0a45ad, 0x5bec25a9, 0x904bc2dd, 0x459ae6d8,
    0xd8f30b16, 0x7110b726, 0x543b14bc, 0xb3afb674, 0x992a5514, 0x65950d05, 0x5703572b, 0xf58b6a5f,
    0xa837793e, 0x50eabc72, 0x4dca1d57, 0x4899ff11, 0x16166c76, 0xcb74ca30, 0x21735587, 0x28a1e239,
    0x074c31b6, 0xbec4688d, 0xf8fd0016, 0x2a3e3640, 0x4fa79455, 0x6b65fa68, 0xb0d838d4, 0xf088256e,
    0xb4d1bf49, 0x7f30634d, 0x9adf073c, 0xbf023d5f, 0xf8daff4a, 0x3bb60a1f, 0xe1b6cc3f, 0x974be547,
    0x788637bd, 0x78b47987, 0xf614dc40, 0xab5917a8, 0x107db970, 0xa7e591e8, 0x42e87faa, 0x7cb7d0a0,
    0x93b41155, 0x17282941, 0x1d6eb56c, 0x9cc6d38e, 0x93e7f09c, 0x40481181, 0xfc10536a, 0x2b59a110,
    0xb25de04b, 0x482f7b77, 0xaa19ef48, 0xbc1efbb4, 0x01640cfd, 0x2103a6ef, 0xba59dbbf, 0x47cfbdd4,
    0x0f7180ea, 0x1d57d70a, 0x3df4865f, 0x9cc2d942, 0x68c29c22, 0xa1cd5d16, 0x950c6175, 0x3f3e42bd,
    0xb8e8620e, 0x6892f24f, 0x7faf4d50, 0x36eff6fe, 0x7436a02a, 0x833a63ba, 0x992cb64c, 0xc8b93e00,
    0x645d2dce, 0x808a9a75, 0x995531f7, 0x087d6303, 0xf3c9caff, 0xea06a464, 0x53361012, 0xd66a74b4,
    0x8d5497b6, 0x00252f2c, 0xf4dba3db, 0xa447b4c0, 0x58d68ff5, 0x7c3d9042, 0x98e09052, 0xb9081476,
    0x04349ea8, 0x213e49f7, 0xadc15b82, 0x69a0dc20, 0x729bfdea, 0xf8eb3204, 0x4b85228e, 0x41031bc7,
    0x2da461eb, 0xd74e810a, 0x1cdd4959, 0xbaeb7806, 0x3ba89ff1, 0x005ca96b, 0xc7b1d771, 0x69bcfe5a,
    0x6f51d1ec, 0x6d8dc769, 0xb51767a4, 0x0418c241, 0xa581230a, 0xf1ac9d29, 0x3dabc8a9, 0x55cb59e8,
    0x72a54713, 0xa3726687, 0x5fdf2267, 0x68dfe635, 0x624e2201, 0xbe9e598e, 0xd79af3ca, 0x6f5e5868,
    0x5af01374, 0x2faaf7e9, 0xbb42e4b2, 0xd30563d9, 0x1167ea91, 0xb6249a7c, 0xb3f56e3e, 0xa87bdea4,
    0xe4373180, 0x59e1b682, 0x7d32511b, 0x1d70ed6a, 0x1e71c6a6, 0xa0c0b3f9, 0x498e468d, 0x1c54594f,
    0x16ba1d39, 0x67de1f8c, 0x108e0b8a, 0x96288ccb, 0x887013f1, 0xe47a782d, 0xc0f1c836, 0x01ea4c72,
    0xe58cc453, 0x0f265892, 0x119be963, 0xad7f4ee4, 0x69765cdf, 0x6c83796b, 0xb30aa299, 0x88c5405c,
    0x9ffcaab0, 0xf60b6a39, 0x34f99df6, 0xb8c6fbae, 0xddf1a401, 0x24e629d8, 0xe2fd7570, 0xc21511f6,
    0x6b4a4337, 0xf7b9bb63, 0x28f35008, 0x21b037be, 0xf4827d03, 0x18fd760a, 0x1cbe96ad, 0x1b376669,
    0x862f25f8, 0x608edfdd, 0xb98511e7, 0x384ea650, 0x9ee03c4c, 0x6facd323, 0xb27b8d6c, 0x9fc9762b,
    0x915133d1, 0x01a2553b, 0x1727f425, 0xe9410ea4, 0xae5bac1c, 0xc01d7780, 0x131dc709, 0x8ebed168,
    0x8027b30d, 0x7c80d6be, 0x285d676d, 0xd4a6fbbb, 0x6b7252f5, 0x76e7c3b3, 0xbeef68cb, 0x9ddbcc9a,
];

/// Closest and furthest apart boundaries are ever asked to fall. Below the minimum a sync point
/// costs more than the sharing it buys, which is the spacing gzip and pigz settled on for
/// rsyncable output, and the maximum is where a 32 bit mask runs out of usable bits.
pub const MIN_SPAN: usize = 4096;
pub const MAX_SPAN: usize = 16 << 20;

/// Gear, the hash FastCDC uses, one shift and one table lookup per byte. The table spreads each
/// byte across the word, landing boundaries far closer to their intended spacing than a plain
/// shift and exclusive or, and old bytes age out of the top so no window is tracked. A mask of
/// 2^n - 1 asks for a boundary every 2^n bytes on average.
#[inline(always)]
fn add(hash: u32, byte: u8) -> u32 {
    (hash << 1).wrapping_add(GEAR[byte as usize])
}

#[inline(always)]
fn hit(hash: u32, mask: u32) -> bool {
    hash & mask == 0
}

/// The mask that asks for a boundary about every span bytes, rounded up to a power of two and
/// held between the two.
pub fn mask(span: usize) -> u32 {
    let span = span.clamp(MIN_SPAN, MAX_SPAN);
    let mut bits = 0;

    while (1usize << bits) < span {
        bits += 1;
    }
    (1u32 << bits) - 1
}

// The hash runs as four chains at once. A 32 bit Gear hash is the sum of the table entries for
// the last 32 bytes, each shifted by its distance, so a position's hash depends on the 32 bytes
// before it and nothing earlier. One chain is bound by its own latency, so a tile of input is
// hashed as four stretches in lockstep, each after the first warmed up on the 32 bytes before
// it. Tiles keep the work close to what one chain stopping at the first hit does.
const LANES: usize = 4;
const STRETCH: usize = 1024;

/// The first position in [first, buf.len()) whose hash hits mask, or buf.len() when none does.
/// hash carries the chain in and comes out as the hash at the position returned, or at the last
/// byte hashed. Since the hash forgets a byte after 32 shifts, hashing starts 31 bytes before
/// first, and the chain carried in matters only when first is closer to the start than that.
pub fn find(hash: &mut u32, mask: u32, buf: &[u8], first: usize) -> usize {
    let len = buf.len();
    let mut k = first.saturating_sub(31);
    let mut chain = *hash;

    if k >= len {
        return len;
    }
    while k < first && k < len {
        chain = add(chain, buf[k]);
        k += 1;
    }
    while len - k >= LANES * STRETCH {
        let tile = &buf[k..k + LANES * STRETCH];
        let mut h = [0u32; LANES];
        let mut at = [STRETCH; LANES];
        let mut at_hash = [0u32; LANES];

        h[0] = chain;
        for i in 1..LANES {
            for &byte in &tile[i * STRETCH - 32..i * STRETCH] {
                h[i] = add(h[i], byte);
            }
        }
        let (l0, rest) = tile.split_at(STRETCH);
        let (l1, rest) = rest.split_at(STRETCH);
        let (l2, l3) = rest.split_at(STRETCH);
        for j in 0..STRETCH {
            h[0] = add(h[0], l0[j]);
            h[1] = add(h[1], l1[j]);
            h[2] = add(h[2], l2[j]);
            h[3] = add(h[3], l3[j]);
            // One rare branch for all four, so the loop stays at the hash and its loads.
            if hit(h[0], mask) || hit(h[1], mask) || hit(h[2], mask) || hit(h[3], mask) {
                for i in 0..LANES {
                    if hit(h[i], mask) && at[i] == STRETCH {
                        at[i] = j;
                        at_hash[i] = h[i];
                    }
                }
            }
        }
        // The earliest stretch with a hit holds the earliest hit.
        for i in 0..LANES {
            if at[i] != STRETCH {
                *hash = at_hash[i];
                return k + i * STRETCH + at[i];
            }
        }
        chain = h[LANES - 1];
        k += LANES * STRETCH;
    }
    while k < len {
        chain = add(chain, buf[k]);
        if hit(chain, mask) {
            *hash = chain;
            return k;
        }
        k += 1;
    }
    *hash = chain;
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One chain from the start of the buffer, the definition the lanes have to agree with.
    fn find_plain(hash: &mut u32, mask: u32, buf: &[u8], first: usize) -> usize {
        let mut chain = *hash;

        if first.saturating_sub(31) >= buf.len() {
            return buf.len();
        }
        for (k, &byte) in buf.iter().enumerate().skip(first.saturating_sub(31)) {
            chain = add(chain, byte);
            if k >= first && hit(chain, mask) {
                *hash = chain;
                return k;
            }
        }
        *hash = chain;
        buf.len()
    }

    #[test]
    fn lanes_agree_with_one_chain() {
        let mut s = 0x1234_5678u32;
        let data: Vec<u8> = (0..300_000)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (s >> 24) as u8
            })
            .collect();

        for span in [MIN_SPAN, 1 << 15, 1 << 17] {
            let m = mask(span);
            let (mut pos, mut lanes, mut plain) = (0, 0, 0);
            while pos < data.len() {
                let got = find(&mut lanes, m, &data[pos..], 0);
                assert_eq!(
                    got,
                    find_plain(&mut plain, m, &data[pos..], 0),
                    "span {span} from {pos}"
                );
                assert_eq!(lanes, plain, "span {span} from {pos}");
                pos += got + 1;
            }
        }
    }

    #[test]
    fn mask_rounds_up_and_stays_in_range() {
        assert_eq!(MIN_SPAN as u32 - 1, mask(0));
        assert_eq!(4095, mask(4096));
        assert_eq!(8191, mask(4097));
        assert_eq!(MAX_SPAN as u32 - 1, mask(usize::MAX));
    }
}
