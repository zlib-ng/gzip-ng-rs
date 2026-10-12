#!/bin/sh
# Compare gzip-ng against other gzip implementations on this machine.
# Usage: benches/compare.sh [size-MB] [threads]
# Uses hyperfine when installed, otherwise a built-in best-of-3 timer.
set -e

SIZE_MB=${1:-256}
THREADS=${2:-$(getconf _NPROCESSORS_ONLN)}
GZNG=${GZNG:-target/release/gzip-ng}
[ -x "$GZNG" ] || { echo "gzip-ng binary not found at $GZNG, set GZNG or build first" >&2; exit 1; }
GZNG=$(cd "$(dirname "$GZNG")" && pwd)/$(basename "$GZNG")

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
DATA=$WORK/data

python3 - "$DATA" "$SIZE_MB" <<'PY'
import sys
path, mb = sys.argv[1], int(sys.argv[2])
chunk = (b"the quick brown fox jumps over the lazy dog 0123456789 " * 1200)[:65536]
with open(path, "wb") as f:
    for i in range(mb * 16):
        f.write(chunk)
PY

run_set() {
    kind=$1; shift
    echo ""
    echo "== $kind, ${SIZE_MB} MB, $THREADS threads"
    if command -v hyperfine >/dev/null 2>&1; then
        hyperfine --warmup 1 --runs 5 "$@"
    else
        echo "(hyperfine not installed, brew install hyperfine for better statistics)"
        for cmd in "$@"; do
            python3 - "$cmd" <<'PY'
import subprocess, sys, time
cmd = sys.argv[1]
best = min(
    (lambda t: (subprocess.run(cmd, shell=True, check=True), time.time() - t)[1])(time.time())
    for _ in range(3))
print(f"  {best:7.2f}s  {cmd}")
PY
        done
    fi
}

# Each tool compresses the data its own way, then decompresses its own output.
COMPRESS="'$GZNG' -b 128K -p $THREADS -k -c <'$DATA' >'$DATA.gzng.gz'"
DECOMPRESS="'$GZNG' -d -p $THREADS -k -c <'$DATA.gzng.gz' >/dev/null"
CROSS=""
if command -v gzip >/dev/null 2>&1; then
    COMPRESS_GZ="gzip -6 -c <'$DATA' >'$DATA.gz.gz'"
    DECOMPRESS_GZ="gzip -d -c <'$DATA.gz.gz' >/dev/null"
fi
if command -v pigz >/dev/null 2>&1; then
    COMPRESS_PIGZ="pigz -p $THREADS -i -c <'$DATA' >'$DATA.pigz.gz'"
    DECOMPRESS_PIGZ="pigz -d -c <'$DATA.pigz.gz' >/dev/null"
    CROSS="'$GZNG' -d -p $THREADS -c <'$DATA.pigz.gz' >/dev/null"
fi

run_set "compress" "$COMPRESS" ${COMPRESS_GZ:+"$COMPRESS_GZ"} ${COMPRESS_PIGZ:+"$COMPRESS_PIGZ"}
run_set "decompress" "$DECOMPRESS" ${DECOMPRESS_GZ:+"$DECOMPRESS_GZ"} ${DECOMPRESS_PIGZ:+"$DECOMPRESS_PIGZ"}
if [ -n "$CROSS" ]; then
    run_set "decompress pigz -i output with gzip-ng" "$CROSS"
fi
