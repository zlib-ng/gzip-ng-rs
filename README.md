# gzip-ng-rs

A gzip replacement built on [zlib-ng](https://github.com/zlib-ng/zlib-ng), with parallel compression into independent deflate blocks and parallel decompression of block-structured files, including `pigz -i` and `pigz --rsyncable -i` output.

This is [gzip-ng](https://github.com/nmoinvaz/gzip-ng) in Rust. The command is still `gzip-ng`, and built against the same zlib-ng it writes the same bytes.

## Goals

- Drop-in for GNU and BSD gzip, flag for flag, distro-safe defaults.
- Compatible with the tools and wrappers around gzip.
- Decompression parallel by default, output byte-identical at any thread count.
- Compression serial by default, parallel independent blocks whenever threads or a block size are used.
- zlib license.

## Build

```
git submodule update --init
cargo build --release
cargo test
```

The command lands in `target/release/gzip-ng`. zlib-ng is a submodule pinned to a commit of its develop branch, built from source along with the rest, which needs CMake and a C compiler. `git submodule update --remote zlib-ng` moves the pin to the head of develop.

## Build Options

| Feature   | Description                                     | Default |
|:----------|:------------------------------------------------|---------|
| `threads` | Compress and decompress blocks on a thread pool | on      |

`cargo build --no-default-features` builds without threads.

## Usage

```
gzip-ng [options] [files...]
```

Compresses files in place, file to file.gz, removing the input unless kept. With no files, or -, it filters stdin to stdout. Options and files mix in any order, short options cluster, and installed as gunzip, zcat, or gzcat it presets -d and -c the way gzip's aliases do.

| Option | Description |
|:-------|:------------|
| `-c` `--stdout` | Write to standard output, keep the files |
| `-d` `--decompress` | Decompress, in parallel for block or pigz -i style input |
| `-f` `--force` | Overwrite outputs, compress to a terminal |
| `-k` `--keep` | Keep input files |
| `-r` `--recursive` | Descend into directories |
| `-t` `--test` | Check integrity without writing |
| `-l` `--list` | List compressed file contents, `-v` adds method, crc, and date |
| `-n` `--no-name` | Do not save or restore the name and time |
| `-N` `--name` | Save and restore the name and time |
| `-m` `--no-time` | Do not save or restore the time, keeping the name |
| `-M` `--time` | Save and restore the time |
| `-v` `--verbose` | Report each file processed |
| `-q` `--quiet` | Suppress warnings |
| `-1` .. `-9` | Compression level, 7 by default |
| `--fast` `--best` | Level 1 and level 9 |
| `-b` `--blocksize` *size* | Average block size in KiB, or with a K, M, or G suffix |
| `-p` `--processes` *n* | Threads to use, which asks for blocks, 0 picks the number of CPUs |
| `-H` `--huffman` | Huffman only strategy |
| `-U` `--rle` | Run length strategy |
| `--filtered` | Filtered strategy |
| `--fixed` | Fixed codes strategy |
| `-T` | Store without compressing |
| `-A` | Text mode, accepted for compatibility |
| `--rsyncable` | Make a plain stream rsync friendly, block output always is |
| `--synchronous` | Write outputs to permanent storage before removing inputs |
| `-h` `--help` | Show the usage summary |
| `-V` `--version` | Show the version |
| `-L` `--license` | Show the license |

Exit status is 0, 1 on errors, 2 on warnings, as gzip behaves.

## Differences from pigz

- Blocks are always independent. pigz chains each block's dictionary into the next unless `--independent` asks otherwise, gzip-ng only writes the independent, pair-marked form. Independence costs about 1% of output size against chained pigz and buys parallel decompression, damage isolation, and cheap deltas, when the input changes and is compressed again, only the blocks around the change come out different.
- Block boundaries follow the content. A rolling hash ends each block, so an insertion in the input moves the boundaries with it, and every block past the change compresses to the same bytes as before. pigz cuts at fixed offsets unless `--rsyncable`. Here the property is always on for block output, costing about 0.02% of it, and `--rsyncable` only matters for the plain serial stream.
- Decompression is parallel. pigz inflates every file serially, its extra threads only read, write, and check. gzip-ng probes for the marker pair and inflates blocks across the pool, for its own output and for `pigz --independent` output alike, decodes sized members, BGZF and MiGz, whole and in parallel, and falls back to plain inflate for any other gzip file.
- `--blocksize` is the tradeoff knob. Independent blocks trade ratio for decode granularity and delta size, both endpoints keeping parallel decompression. Raising it recovers most of the ratio distance to chained pigz, 83% at `-b 1M` on a mixed corpus, lowering it shrinks how much of the output changes when the input does.
- The deflate, inflate, and crc engine is zlib-ng, with SIMD in the block boundary scanner as well.
- The core is a library. The block reader and writer in `gzng::gzblock` work over any `Read` and `Write`, and the command line is one caller of them.
- Not carried over from pigz: zopfli's `-11`, zlib and zip output formats, and `--suffix`.

## How it works

### Parallel compression

- With `--processes` or `--blocksize` the input is cut into blocks, 128 KiB by default, and each block is deflated by a worker thread from an empty dictionary, so no block depends on another.
- A block ends at the first byte past half the block size where the low bits of a Gear hash, the rolling hash behind FastCDC, come up zero, or at twice the block size.
- Each block ends with a sync flush and a full flush, two empty stored blocks, the nine bytes `00 00 FF FF 00 00 00 FF FF`, the same shape `pigz --independent` writes.
- The blocks are written in order as one ordinary gzip member, header, blocks back to back, and a trailer whose CRC is the blocks' CRCs combined.

```
header | block 1 … 00 00 FF FF 00 00 00 FF FF | block 2 … 00 00 FF FF 00 00 00 FF FF | … | last block | crc32 size
```

### Parallel decompression

- For each member, the reader parses the gzip header and looks at the first megabyte of compressed data for a marker pair. One found means independent blocks and a parallel decode, none means plain serial inflate, as for any gzip file. With one thread there is nothing to gain from blocks, so plain inflate takes every member.
- A header that records its member's whole size, BGZF's BC subfield or MiGz's MZ, needs no probe at all. Sized members are cut whole at their recorded lengths, grouped to about a block per slot, and inflated in parallel through the gzip wrapper with zlib checking every member's crc, so bgzip, BAM, and MiGz files decode at full parallel speed.
- A scanner finds every `00 00 FF FF`, one empty stored block, with SIMD filtering for the zero pair the way `memchr` filters for a byte. A second empty stored block behind it makes a boundary.
- Each segment between boundaries is inflated on its own by a worker into a slot the size of a block, with its CRC taken there, and the blocks are handed out in order.
- `pigz --rsyncable --independent` ends a block at every rsync point, about every 4 KiB. The reader gathers those until about a block size is in hand before handing them to a worker, since a slot per 4 KiB costs more to hand off than to inflate.
- Stored data can hold the marker pair by chance, so a boundary is trusted only when the segment inflates to a whole block. A segment that comes up short goes back to the input to be cut again past that marker, and a member with no block structure at all goes back through plain inflate.
- The trailer's CRC and length are checked against what the blocks produced.

## Benchmarks

Engine benchmarks use Criterion:

```
cargo bench
```

Comparing whole binaries is a different job, fork and exec noise belongs to tools built for it. `benches/compare.sh [size-MB] [threads]` runs gzip-ng against the system gzip and pigz where installed, through hyperfine when available and a built-in best-of-3 timer otherwise.

### Results

Whole-tool comparisons live in [gzipbench](https://github.com/nmoinvaz/gzipbench),
which runs the full matrix, level ladders 1 through 9, thread sweeps, the
cross-decode grid, and a deflate block census, all against tools built on
zlib-ng develop, and graphs each run. The graph is of the C gzip-ng:

![All tools, compress and decompress](https://raw.githubusercontent.com/nmoinvaz/gzipbench/main/results/all-tools.svg)

## Thanks

- [zlib](https://zlib.net) and its authors Jean-loup Gailly and Mark Adler, who defined the gzip format and wrote the compression everything here rests on.
- [zlib-ng](https://github.com/zlib-ng/zlib-ng), whose SIMD engine does the deflate, inflate, and crc work behind every number in this README.
- [pigz](https://zlib.net/pigz/), Mark Adler's parallel gzip, whose independent blocks and rsyncable output showed the way.
