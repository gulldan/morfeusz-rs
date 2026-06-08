#!/usr/bin/env python3
"""Speed + peak-memory benchmark: optimized C++ (-O3) vs Rust (--release).

Both binaries process the same corpus + dictionary with stdout redirected to
/dev/null (we measure analysis throughput, not terminal I/O). Wall time and peak
RSS come from /usr/bin/time -l (macOS). Reports the best-of-N run for each.

Usage: bench.py <analyzer|generator> <dict> <dict-dir> <corpus> [runs]
"""
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]


def run_once(binary, dict_name, dict_dir, corpus):
    with open(corpus, "rb") as inp, open("/dev/null", "wb") as devnull:
        proc = subprocess.run(
            ["/usr/bin/time", "-l", str(binary),
             "--dict-dir", str(dict_dir), "--dict", dict_name],
            stdin=inp, stdout=devnull, stderr=subprocess.PIPE,
        )
    err = proc.stderr.decode("utf-8", "replace")
    real = float(re.search(r"([\d.]+)\s+real", err).group(1))
    rss = int(re.search(r"(\d+)\s+maximum resident set size", err).group(1))
    return real, rss


def measure(label, binary, dict_name, dict_dir, corpus, runs):
    best_real, peak_rss = None, 0
    for _ in range(runs):
        real, rss = run_once(binary, dict_name, dict_dir, corpus)
        best_real = real if best_real is None else min(best_real, real)
        peak_rss = max(peak_rss, rss)
    return best_real, peak_rss


def main():
    mode, dict_name, dict_dir, corpus = sys.argv[1:5]
    runs = int(sys.argv[5]) if len(sys.argv) > 5 else 5
    binname = "morfeusz_analyzer" if mode == "analyzer" else "morfeusz_generator"
    cpp = REPO / "build-cpp-ref-O2" / binname
    rust = REPO / "target" / "release" / binname
    lines = sum(1 for _ in open(corpus, "rb"))

    print(f"== {mode}  dict={dict_name}  corpus={lines} lines  runs={runs} ==")
    cr, crss = measure("C++", cpp, dict_name, dict_dir, corpus, runs)
    rr, rrss = measure("Rust", rust, dict_name, dict_dir, corpus, runs)

    for label, t, rss in (("C++ -O3", cr, crss), ("Rust --release", rr, rrss)):
        print(f"  {label:14s}  {t:6.3f}s  | {lines/t:10,.0f} lines/s  | peak RSS {rss/1048576:7.1f} MB")
    print()
    sp = cr / rr
    print(f"  SPEED:  Rust is {sp:.2f}x  ->  {'Rust faster' if sp>1 else 'C++ faster'}")
    mr = rrss / crss
    print(f"  MEMORY: Rust peak RSS {mr:.2f}x C++  ->  {'Rust leaner' if mr<1 else 'C++ leaner'}")


if __name__ == "__main__":
    main()
