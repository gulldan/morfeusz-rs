#!/usr/bin/env python3
"""Full speed+memory comparison of every Morfeusz stack on the huge corpora.

Two tables:
  A) CLI         — C++ -O3 vs Rust (1 thread) vs Rust (all cores), full corpus
  B) Py bindings — C++/SWIG vs Rust/PyO3, on a 200k-line subset (bindings are
                   slower, and the binding overhead is what we want to compare)

For every run we report wall time, lines/s, peak RSS, and whether the output is
byte-identical to the C++ reference. Peak RSS comes from `/usr/bin/time -l`.
"""
import os, re, subprocess, sys, time
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
DICTDIR = "/tmp/bench"
CPP_CLI = str(REPO / "build-cpp-ref-O2/morfeusz_analyzer")
RUST_CLI = str(REPO / "target/release/morfeusz_analyzer")
PY = "/tmp/morf-builder-venv/bin/python"
HARNESS = str(Path(__file__).with_name("py_binding_harness.py"))
CPP_PYBIND = "/tmp/cppy"
COMMON = ["--dict", "sgjp", "--dict-dir", DICTDIR]
CORPORA = ["msmarco", "c4pl", "wiki_pl"]
PY_LINES = 200_000


def run(cmd, stdin_path, stdout_path, env=None):
    """Run under `/usr/bin/time -l`; return (wall_seconds, peak_rss_bytes)."""
    full = ["/usr/bin/time", "-l"] + cmd
    with open(stdin_path, "rb") as fin, open(stdout_path, "wb") as fout:
        t = time.perf_counter()
        p = subprocess.run(full, stdin=fin, stdout=fout, stderr=subprocess.PIPE,
                           env={**os.environ, **(env or {})})
        dt = time.perf_counter() - t
    m = re.search(rb"(\d+)\s+maximum resident set size", p.stderr)
    return dt, (int(m.group(1)) if m else 0)


def head(corpus, n, dest):
    with open(corpus, encoding="utf-8", errors="replace") as f:
        rows = [next(f, "") for _ in range(n)]
    Path(dest).write_text("".join(rows), encoding="utf-8")
    return sum(1 for r in rows if r)


def row(stack, n, dt, rss, ident):
    print(f"  {stack:<18} {n:>10,} {dt:>8.2f}s {n/dt:>11,.0f} {rss/1048576:>8.1f} MB  {ident}")


print("=" * 78)
print("TABLE A — CLI, full corpus  (C++ -O3 vs Rust 1-thread vs Rust all-cores)")
print("=" * 78)
print(f"  {'stack':<18} {'lines':>10} {'time':>9} {'lines/s':>11} {'peak RSS':>11}  identity")
for name in CORPORA:
    corpus = f"{DICTDIR}/{name}.txt"
    if not os.path.exists(corpus):
        continue
    n = sum(1 for _ in open(corpus, "rb"))
    print(f"-- {name} ({n:,} lines) " + "-" * (78 - len(name) - len(f'{n:,}') - 13))
    ct, cr = run([CPP_CLI, *COMMON], corpus, "/tmp/A_cpp.out")
    r1t, r1r = run([RUST_CLI, *COMMON, "--threads", "1"], corpus, "/tmp/A_r1.out")
    rNt, rNr = run([RUST_CLI, *COMMON, "--threads", "0"], corpus, "/tmp/A_rN.out")
    id1 = "IDENTICAL" if open("/tmp/A_cpp.out", "rb").read() == open("/tmp/A_r1.out", "rb").read() else "DIFFERS"
    idN = "IDENTICAL" if open("/tmp/A_cpp.out", "rb").read() == open("/tmp/A_rN.out", "rb").read() else "DIFFERS"
    row("C++ CLI (-O3)", n, ct, cr, "(reference)")
    row("Rust CLI 1-thread", n, r1t, r1r, id1)
    row("Rust CLI all-cores", n, rNt, rNr, idN)
    print(f"     -> Rust 1t {ct/r1t:.2f}x faster, {r1r/cr:.2f}x RSS; all-cores {ct/rNt:.2f}x faster vs C++")

print()
print("=" * 78)
print(f"TABLE B — Python bindings, {PY_LINES:,}-line subset  (C++/SWIG vs Rust/PyO3)")
print("=" * 78)
print(f"  {'stack':<18} {'lines':>10} {'time':>9} {'lines/s':>11} {'peak RSS':>11}  identity")
for name in CORPORA:
    corpus = f"{DICTDIR}/{name}.txt"
    if not os.path.exists(corpus):
        continue
    sub = f"/tmp/B_{name}_sub.txt"
    n = head(corpus, PY_LINES, sub)
    print(f"-- {name} ({n:,} lines) " + "-" * (78 - len(name) - len(f'{n:,}') - 13))
    cppt, cppr = run([PY, HARNESS, "dump", sub], sub, f"/tmp/B_{name}_cpp.out",
                     env={"PYTHONPATH": CPP_PYBIND})
    rt, rr = run([PY, HARNESS, "dump", sub], sub, f"/tmp/B_{name}_rust.out")
    ident = "IDENTICAL" if open(f"/tmp/B_{name}_cpp.out", "rb").read() == open(f"/tmp/B_{name}_rust.out", "rb").read() else "DIFFERS"
    row("C++ /SWIG binding", n, cppt, cppr, "(reference)")
    row("Rust /PyO3 binding", n, rt, rr, ident)
    print(f"     -> Rust {cppt/rt:.2f}x faster, {rr/cppr:.2f}x RSS vs C++/SWIG")
