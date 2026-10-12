// scanner.rs -- marker scanner throughput
// For conditions of distribution and use, see LICENSE.md

// The production scan against the scalar memchr loop it replaces. The haystack is real block
// writer output, so zero density and marker spacing match what the reader scans, and each
// checks its hit count against the other's so a faster scan that misses markers cannot hide.
//
// Unrolling the vector scan loses, because the filter branch was never the cost. Compressed
// bytes hold about one zero per 256, so the branch is predicted not taken about 94 percent of
// the time, while a combined minimum adds work to every iteration and doubles or quadruples the
// chance of paying for the slow path.

use std::hint::black_box;
use std::sync::LazyLock;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use gzng::Strategy;
use gzng::gzblock::Writer;
use gzng::scanner::scan_marker;

static HAYSTACK: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut s = 0x2545_f491u32;
    let data: Vec<u8> = (0..64 << 20)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            0x20 + ((s >> 24) & 0x3f) as u8
        })
        .collect();
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, 6, Strategy::Default, 128 * 1024, 0).unwrap();
    w.write(&data).unwrap();
    w.finish().unwrap();
    drop(w);
    out
});

/// The scalar loop the vector scan falls back to, memchr for the zero and a byte check.
fn scan_scalar(buf: &[u8], mut start: usize, end: usize) -> Option<usize> {
    while start < end {
        start += memchr::memchr(0, &buf[start..end])?;
        if buf[start + 1] == 0 && buf[start + 2] == 0xff && buf[start + 3] == 0xff {
            return Some(start);
        }
        start += 1;
    }
    None
}

fn count_hits(scan: fn(&[u8], usize, usize) -> Option<usize>) -> usize {
    let end = HAYSTACK.len() - 3;
    let (mut hits, mut pos) = (0, 0);

    while let Some(hit) = scan(&HAYSTACK, pos, end) {
        hits += 1;
        pos = hit + 1;
    }
    hits
}

fn scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("scan");

    assert_eq!(count_hits(scan_scalar), count_hits(scan_marker));
    group.throughput(Throughput::Bytes(HAYSTACK.len() as u64));
    group.bench_function("marker", |b| b.iter(|| black_box(count_hits(scan_marker))));
    group.bench_function("scalar", |b| b.iter(|| black_box(count_hits(scan_scalar))));
    group.finish();
}

criterion_group!(benches, scan);
criterion_main!(benches);
