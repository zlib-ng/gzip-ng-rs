// gzblock.rs -- engine throughput benchmarks
// For conditions of distribution and use, see LICENSE.md

// The pool does the work on other threads, so wall clock is the meaningful axis, which is what
// Criterion measures.

use std::hint::black_box;
use std::io::{self, BufRead, Write};
use std::sync::LazyLock;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use gzng::Compress;
use gzng::gzblock::{Reader, Writer};

const DATA_LEN: usize = 64 << 20;
const BLOCK: u32 = 128 * 1024;

static SAMPLE: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let text = b"the quick brown fox jumps over the lazy dog ";
    (0..DATA_LEN).map(|i| text[i % 44] + ((i / 8191) % 7) as u8).collect()
});

/// Settings for the block writer, on nthreads workers.
fn block_params(nthreads: usize) -> Compress<'static> {
    Compress {
        level: 6,
        block_size: BLOCK,
        threads: nthreads,
        ..Compress::default()
    }
}

static BLOCK_PACKED: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let mut out = Vec::new();
    let mut w = Writer::new(&mut out, &block_params(0)).unwrap();
    w.write_all(&SAMPLE).unwrap();
    w.finish().unwrap();
    drop(w);
    out
});

static PLAIN_PACKED: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let params = Compress {
        level: 6,
        ..Compress::default()
    };
    let mut out = Vec::new();
    gzng::compress_stream(&SAMPLE[..], &mut out, &params).unwrap();
    out
});

fn read_all(packed: &[u8], nthreads: usize) {
    let mut r = Reader::new(packed, &[], nthreads).unwrap();
    loop {
        match r.fill_buf() {
            Ok(piece) if !piece.is_empty() => {
                let n = piece.len();
                black_box(piece);
                r.consume(n);
            }
            _ => break,
        }
    }
}

fn block_compress(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_compress");

    group.throughput(Throughput::Bytes(DATA_LEN as u64)).sample_size(10);
    for nthreads in [1, 2, 4, 8] {
        group.bench_with_input(BenchmarkId::from_parameter(nthreads), &nthreads, |b, &nthreads| {
            b.iter(|| {
                let mut w = Writer::new(io::sink(), &block_params(nthreads)).unwrap();
                w.write_all(&SAMPLE).unwrap();
                w.finish().unwrap();
            });
        });
    }
    group.finish();
}

fn block_decompress(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_decompress");

    group.throughput(Throughput::Bytes(DATA_LEN as u64)).sample_size(10);
    for nthreads in [1, 2, 4, 8] {
        group.bench_with_input(BenchmarkId::from_parameter(nthreads), &nthreads, |b, &nthreads| {
            b.iter(|| read_all(&BLOCK_PACKED, nthreads));
        });
    }
    group.finish();
}

fn plain_decompress(c: &mut Criterion) {
    let mut group = c.benchmark_group("plain_decompress");

    group.throughput(Throughput::Bytes(DATA_LEN as u64)).sample_size(10);
    group.bench_function("1", |b| b.iter(|| read_all(&PLAIN_PACKED, 1)));
    group.finish();
}

criterion_group!(benches, block_compress, block_decompress, plain_decompress);
criterion_main!(benches);
