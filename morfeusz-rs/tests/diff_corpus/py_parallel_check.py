#!/usr/bin/env python3
"""Verify and benchmark the Python binding's GIL-release + batch analyse_many().

  1. analyse_many(lines) must equal [analyse(l) for l in lines] exactly
     (same results, input order) — the batch API changes nothing but speed.
  2. Speed: serial analyse() loop vs analyse_many() on a large batch.
  3. Threaded analyse() (one Morfeusz per thread) scales now that the GIL is
     released during analysis.

Run with the Rust binding on sys.path:
    /tmp/morf-builder-venv/bin/python py_parallel_check.py [corpus] [n]
"""
import sys, time
from concurrent.futures import ThreadPoolExecutor

# This check exercises Rust-only features (GIL-release, analyse_many), so prefer
# this project's renamed module; fall back to anything installed as `morfeusz2`.
try:
    import morfeusz2_rs as morfeusz2
except ImportError:
    import morfeusz2

corpus = sys.argv[1] if len(sys.argv) > 1 else "/tmp/bench/msmarco.txt"
n = int(sys.argv[2]) if len(sys.argv) > 2 else 200_000
DICT = dict(dict_name="sgjp", dict_path="/tmp/bench")

with open(corpus, encoding="utf-8", errors="replace") as f:
    lines = [next(f, "").rstrip("\n") for _ in range(n)]
lines = [l for l in lines if l]
print(f"corpus={corpus.split('/')[-1]}  lines={len(lines):,}")

m = morfeusz2.Morfeusz(**DICT, analyse=True, generate=False)

# 1. Correctness: batch == serial, exactly.
k = min(20_000, len(lines))
serial = [m.analyse(l) for l in lines[:k]]
batch = m.analyse_many(lines[:k])
assert batch == serial, "analyse_many != serial analyse loop"
print(f"[1] analyse_many == serial analyse loop on {k:,} lines: IDENTICAL")

# 2. Speed: serial analyse() loop vs analyse_many().
t = time.perf_counter(); _ = [m.analyse(l) for l in lines]; ts = time.perf_counter() - t
t = time.perf_counter(); _ = m.analyse_many(lines); tb = time.perf_counter() - t
print(f"[2] serial analyse() loop : {ts:6.2f}s  ({len(lines)/ts:>9,.0f} lines/s)")
print(f"    analyse_many()        : {tb:6.2f}s  ({len(lines)/tb:>9,.0f} lines/s)  -> {ts/tb:.2f}x faster")

# 3. Threaded analyse() with one Morfeusz per thread (GIL released during work).
def worker(chunk):
    mt = morfeusz2.Morfeusz(**DICT, analyse=True, generate=False)
    return [mt.analyse(l) for l in chunk]

for nthreads in (1, 4, 8):
    chunks = [lines[i::nthreads] for i in range(nthreads)]
    t = time.perf_counter()
    with ThreadPoolExecutor(max_workers=nthreads) as ex:
        list(ex.map(worker, chunks))
    dt = time.perf_counter() - t
    print(f"[3] {nthreads} Python threads x analyse(): {dt:6.2f}s  ({len(lines)/dt:>9,.0f} lines/s)")
