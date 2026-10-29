// rolling.rs -- content-defined boundaries for rsyncable output
// For conditions of distribution and use, see LICENSE.md

/// One random word per byte value, fixed here rather than generated so that boundaries do not
/// depend on the build.
#[rustfmt::skip]
pub static GEAR: gearhash::Table = [
    0x161922c645ce50e8, 0xad760cafa1697b60, 0x3501ff44902ca50d, 0x417cb9a826d831df,
    0x99af6f9b0c4476b6, 0x5d51f5f75b762c59, 0x66239e8c309a282b, 0x53e01f580916c5cb,
    0xaa941016a4c2958b, 0x279993774594e137, 0x20e9a7a844bdacc0, 0x90ec693596cc8ab0,
    0x4d7760d307367afa, 0x4315096655b77a33, 0x0e907aa9d946b562, 0x1947cecfc10e24f3,
    0x8a27bdf7c4b88166, 0x3989c8272f2ae095, 0xb7dc9a7f27f0b595, 0xa0f6c1d2ed13c145,
    0xc54ad38a1e595bce, 0xd87e930b7f41a756, 0x87ead6b5c67ec06b, 0xa4353faba48b2382,
    0x19a42fc02250ff9d, 0x5baeac52832826b1, 0x862b3e793173997b, 0x60ba89bb02987253,
    0xd51b395c4f12bd9a, 0x0bc7804037d52ade, 0x42252510d604c41f, 0x29f45920a9f57c95,
    0xa93b6ea467675dbc, 0x15c3aaabd5956aec, 0xa5daabf7c364c8e5, 0xd094cf38e10d9faa,
    0xad06e37401370752, 0xcdb61e7bd233a525, 0x0a4ba189d018c8d3, 0x50b327159db36439,
    0x82a6283919ae345e, 0xcbe4fec009a705bc, 0x00140bc367f632b3, 0xc01390dfaf502656,
    0xe4a211a9598495bf, 0x2de60a74ac7442e6, 0x7c80a5d8393d87dc, 0x0042f9e8ad284fd5,
    0x1e86ae8dae777e7b, 0x056b110d49d7a50e, 0x0cb3ea3f164075ae, 0x810c2241d09be6d9,
    0x8c3e2645b1f287d0, 0xd1e311a47f9cd5f8, 0xce8d06c14b42138d, 0xf655d4c61563800d,
    0x2b83b4facee21349, 0xff5070d67c85f362, 0xfff81fe0b509fd83, 0x26584fd1187d611c,
    0xa339def8905cc9b6, 0x062d2657944baf3c, 0x53395a748d962c4b, 0xadfc499f2a938342,
    0x7ea69ed006af8bd7, 0x8a2d3e828f6d3ae5, 0x32fb0973d630265d, 0x4051fe43c4b522ae,
    0x082c3a7ac6f2b2da, 0x0c3a17d99df22145, 0xf6445251c28d637b, 0x9975c19cf44affdb,
    0xb35f858bd5a4c400, 0x698f51eb4b966aa9, 0x825a83fad5f42f53, 0xb1a1c87a8e370a11,
    0xdd78e2d4f2beffbc, 0xde74c9244ae698f4, 0x853315df4f1b7c7a, 0x5953cf89da9626e9,
    0x7ef1aff252b419a7, 0x0d7c263366fa669e, 0x8576aac3174e2232, 0x9c20825cd0a0e128,
    0x922a277c96f9a79e, 0x66fe071aa89214d5, 0x28e26d7561f3016d, 0x08bb2d9d88ba3be2,
    0xb1b00e7b7dd5f20c, 0x5c5b6b824c2705ae, 0x9f6535d60528fb6c, 0x50ab140e38a246c6,
    0x993b4bf586e84635, 0x44dfc222af3ef96d, 0xaab7732237af2bca, 0xde089459f29e2aaf,
    0xeb399ec3f5faa893, 0x86bc73b51214aefb, 0x3235a8d4e6b2b330, 0x6c98d4263aa01342,
    0xeba2c848fbf2f151, 0xf0617b36bdef52f8, 0x7359334c5cc1d837, 0xca488d0a3e805164,
    0x557edcf42586aa06, 0x831a3dbf422ebdb6, 0x0b7183f2af6defc7, 0x3ca78d39e1a1a93d,
    0x7d96c744610c034e, 0xaf43c1f572b365d4, 0xa0a90b7e6688faaa, 0x1dd7168c3a6b4c74,
    0x08426523307a1662, 0xebe9adef78634e13, 0x7da4310ddc823b8b, 0xda579bf86fae8b5a,
    0xf653a134a4c747dc, 0xbc5486addab05206, 0x91d48852d77f8c1c, 0xffdc36128b720421,
    0x696576be9bd2f14c, 0x36c0ffbedd4bdf79, 0x0d80d05b8e4fdf8f, 0x8be7b9e56060c921,
    0xfc5eaa037b74faa7, 0xb6a9c94f46d601ad, 0x203f082946b4a0f6, 0x8e059f98e9c6069b,
    0xd5b54bd28a19acb8, 0xb343dd5a78f8b450, 0x36079f11691ee4bb, 0xc49f5fbdc6610839,
    0x31338b7fde79ca2e, 0x22668f106ff6bff1, 0x717be48a0921e6a4, 0xd3005c7d06b347a7,
    0x88adcba352c0aa12, 0x0d727f23d654948c, 0x8da856c2fa827fe8, 0x7826fc59ddbbc97f,
    0x25557d00e33333dd, 0x6033aff71ebbe4ec, 0x1c1c81bb063415a8, 0x2ba93ba66ce2f230,
    0x33b8ba7d7c707a7c, 0x7fafa11db8782f26, 0x24223fa0d0736b12, 0xa90e63b82c2f481e,
    0x5a6b12258c9920b5, 0xff2304eede1531e4, 0x84fe097fde1d8469, 0xc8992dce1397403b,
    0x4846e5ee33ac3fb2, 0x8404322637000bbc, 0x09d6006a1a5525d6, 0xd605db240dd49e26,
    0xcf13d9c29bc3e6c6, 0xdc5339ee61466f5e, 0x76de1c04fbd26e72, 0xd285febfe53ee592,
    0xed8852011245ba89, 0xa34dae9383e4fed1, 0x3ce937eddc675df6, 0x6c0eced66a6f703f,
    0xb99df75e3eb2de36, 0x482b5a5739286e35, 0x12471e12223f1d69, 0x9a195b06398c4375,
    0x601b91de3551443f, 0xe207c680ddfca9d8, 0xbdde1dd799d22472, 0x1365ae8c8e0463e3,
    0xbbbf5c35a8301ca6, 0xddbfa7323a79e77a, 0x975795d03753999b, 0xb42d170f98a37694,
    0x873cca3f004fa35f, 0x6426be49467ad445, 0x82f3f34340c65372, 0xeaac60cf55373f10,
    0x7d8bc4a13793ef8f, 0x36be91bdba01424a, 0xe224abb895d92ef4, 0x24a827201fffecaa,
    0xc60f8957d003e7e3, 0xa2dce8feed8ef8d3, 0x02d8a2c1da0325a3, 0xa3d3a8c5fccee46a,
    0x47d0d7c1880bd7f0, 0xaa24c34dfd59d363, 0xb47a9cb39d5b1e88, 0xd043e700aaddc81e,
    0xf4382b6a43edb55e, 0x371b1d53c01b8623, 0x42ee771782290d54, 0xfe8adc45ee9674e1,
    0x275ebd3de2960fae, 0x6f5393514f0c4205, 0x18de42fbf438dddb, 0x15ee1b0bac1032ed,
    0xfbc48a0e9a8bfaf0, 0x6cd2c9b8b2ddbfdc, 0x1fe0843e20a62ed4, 0xeebbdfc0d8e95ede,
    0xce56a65bba2c8fe1, 0xa9c362010c4b727b, 0xb960d31d45608cd6, 0x129f546f0bb74d08,
    0x386b7bbc401d5186, 0x962f45d44eadbbd4, 0x15b43f281c01563d, 0x0ae2346188f2806e,
    0x819c7fd6e1ad7369, 0x17493bd4a5004bf7, 0x210d8aad5939712b, 0x4870b197d4236315,
    0x68a0f7011736adbf, 0x503f2b65d8b2f13b, 0x8094a466dd35c927, 0xc3808a841a80f20a,
    0x7aa622d21fdebd73, 0xebe6e4092686b39e, 0xe7d85f2a14eaa9c9, 0x07d7e8260a482653,
    0x53fa24e731fbcfb6, 0x60f18718978e354f, 0xeece5a82bb599ec9, 0x1212a7bcae5e3015,
    0x13a65fe41102c51e, 0x3db1b71be310c0e3, 0x79d8e260590be224, 0x17b100a3ac6bd71a,
    0x7d6fa19714baae33, 0x4fb5fae13cc57bcf, 0x49d56da2b2fac5c6, 0x774d14c98e1b7c2b,
    0xd58c4556d4526aea, 0xaad2d192b58b0134, 0x9679886e33440fc4, 0x3cec22a3cb9a95ee,
    0x4ca0258ec42ad0ed, 0x1d0ae54accd4b9c6, 0xdb41a92694e74a2f, 0x3a1d372b6859db2f,
    0x5d99f4609bcb4e69, 0xccf1403b250cf1bc, 0xcefb33a79bc86423, 0xf115f56dd10738b8,
    0x22525c63b311797a, 0xdb064656f83e2935, 0x2c83e48c640c0037, 0x9b354b795e8858c1,
    0x44bfb35f5c988406, 0x5191422a8dafb040, 0x71854a3c39c71ee8, 0xea2be3a8adbd94da,
];

/// Closest and furthest apart boundaries are ever asked to fall. Below the minimum a sync point
/// costs more than the sharing it buys, which is the spacing gzip and pigz settled on for
/// rsyncable output, and the maximum caps the mask at 24 bits.
pub const MIN_SPAN: usize = 4096;
pub const MAX_SPAN: usize = 16 << 20;

/// Gear, the hash FastCDC uses, one shift and one table lookup per byte. The table spreads each
/// byte across the word, landing boundaries far closer to their intended spacing than a plain
/// shift and exclusive or, and old bytes age out of the top so no window is tracked. A mask of
/// the top n bits asks for a boundary every 2^n bytes on average. Bit k of the hash sums the last
/// k + 1 bytes, so the top bits weigh the whole 64 byte window, where the low n bits see only the
/// n newest, too few distinct contexts on repetitive input to hit the mask.
#[cfg(test)]
#[inline(always)]
fn add(hash: u64, byte: u8) -> u64 {
    (hash << 1).wrapping_add(GEAR[byte as usize])
}

#[cfg(test)]
#[inline(always)]
fn hit(hash: u64, mask: u64) -> bool {
    hash & mask == 0
}

/// The mask that asks for a boundary about every span bytes, rounded up to a power of two and
/// held between the two.
pub fn mask(span: usize) -> u64 {
    let span = span.clamp(MIN_SPAN, MAX_SPAN);
    let mut bits = 0;

    while (1usize << bits) < span {
        bits += 1;
    }
    !0u64 << (64 - bits)
}

/// The first position in [first, buf.len()) whose hash hits mask, or buf.len() when none does.
/// hash carries the chain in and comes out as the hash at the position returned, or at the last
/// byte hashed. Since the hash forgets a byte after 64 shifts, hashing starts 63 bytes before
/// first, and the chain carried in matters only when first is closer to the start than that.
pub fn find(hash: &mut u64, mask: u64, buf: &[u8], first: usize) -> usize {
    let warm = first.saturating_sub(63).min(buf.len());
    let first = first.min(buf.len());
    let mut h = gearhash::Hasher::new(&GEAR);

    h.set_hash(*hash);
    h.update(&buf[warm..first]);
    let at = match h.next_match(&buf[first..], mask) {
        Some(n) => first + n - 1,
        None => buf.len(),
    };
    *hash = h.get_hash();
    at
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One chain from the start of the buffer, the definition the lanes have to agree with.
    fn find_plain(hash: &mut u64, mask: u64, buf: &[u8], first: usize) -> usize {
        let mut chain = *hash;

        if first.saturating_sub(63) >= buf.len() {
            return buf.len();
        }
        for (k, &byte) in buf.iter().enumerate().skip(first.saturating_sub(63)) {
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
        assert_eq!(!0u64 << 52, mask(0));
        assert_eq!(!0u64 << 52, mask(4096));
        assert_eq!(!0u64 << 51, mask(4097));
        assert_eq!(!0u64 << 40, mask(usize::MAX));
    }
}
