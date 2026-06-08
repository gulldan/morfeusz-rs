#!/usr/bin/env python3
"""Four-way comparison: C++ vs Rust, CLI vs Python binding.

For a given corpus it runs the morphological analyzer through four stacks:
  - C++ CLI            (build-cpp-ref-O2/morfeusz_analyzer)
  - Rust CLI           (target/release/morfeusz_analyzer)
  - C++ Python binding (SWIG, via PYTHONPATH=<cpp-binding-dir>)
  - Rust Python binding (PyO3, installed in the venv)
measuring wall time and peak RSS (/usr/bin/time -l) for each, and asserting the
outputs are identical (C++CLI vs RustCLI byte-for-byte; C++Py vs RustPy on the
high-level tuple dump). Prints a comparison table.

Usage: compare_all.py <corpus> [py-corpus] [--dict NAME] [--dict-dir DIR]
                       [--cpp-bin DIR] [--cpp-pybind DIR] [--rust-bin DIR]
                       [--python PYEXE] [--harness PYHARNESS]
The py-corpus (smaller) is used for the Python-binding runs; defaults to the
first 200k lines of <corpus>.
"""
import argparse
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]


def timed_run(cmd, stdin_path, stdout_path, env=None):
    """Run `cmd` under /usr/bin/time -l; return (real_seconds, peak_rss_bytes)."""
    with open(stdin_path, "rb") as inp, open(stdout_path, "wb") as out:
        proc = subprocess.run(
            ["/usr/bin/time", "-l", *cmd],
            stdin=inp, stdout=out, stderr=subprocess.PIPE, env=env,
        )
    err = proc.stderr.decode("utf-8", "replace")
    real = float(re.search(r"([\d.]+)\s+real", err).group(1))
    rss = int(re.search(r"(\d+)\s+maximum resident set size", err).group(1))
    return real, rss


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("corpus")
    ap.add_argument("py_corpus", nargs="?")
    ap.add_argument("--dict", default="sgjp")
    ap.add_argument("--dict-dir", default="/tmp/bench")
    ap.add_argument("--cpp-bin", default=str(REPO / "build-cpp-ref-O2"))
    ap.add_argument("--cpp-pybind", default="/tmp/cppy")
    ap.add_argument("--rust-bin", default=str(REPO / "target" / "release"))
    ap.add_argument("--python", default="/tmp/morf-builder-venv/bin/python")
    ap.add_argument("--harness", default=str(Path(__file__).with_name("py_binding_harness.py")))
    args = ap.parse_args()

    tmp = Path("/tmp/bench")
    corpus = args.corpus
    py_corpus = args.py_corpus
    if py_corpus is None:
        py_corpus = str(tmp / "cmp_py_corpus.txt")
        with open(corpus, encoding="utf-8") as f:
            head = [next(f, "") for _ in range(200_000)]
        Path(py_corpus).write_text("".join(head), encoding="utf-8")

    common = ["--dict", args.dict, "--dict-dir", args.dict_dir]
    cpp_env = {"PYTHONPATH": args.cpp_pybind, "PATH": "/usr/bin:/bin"}
    out = {}
    rows = []

    # --- CLI ---
    for label, binary in (("C++ CLI", f"{args.cpp_bin}/morfeusz_analyzer"),
                          ("Rust CLI", f"{args.rust_bin}/morfeusz_analyzer")):
        path = str(tmp / f"cmp_{label.split()[0].lower()}_cli.out")
        t, rss = timed_run([binary, *common], corpus, path)
        out[label] = path
        rows.append((label, corpus, t, rss))

    # --- Python binding (high-level tuple dump) ---
    for label, env in (("C++ Py", cpp_env), ("Rust Py", None)):
        path = str(tmp / f"cmp_{label.split()[0].lower()}_py.out")
        t, rss = timed_run([args.python, args.harness, "dump", py_corpus], py_corpus, path, env=env)
        out[label] = path
        rows.append((label, py_corpus, t, rss))

    # --- identity ---
    cli_same = Path(out["C++ CLI"]).read_bytes() == Path(out["Rust CLI"]).read_bytes()
    py_same = Path(out["C++ Py"]).read_bytes() == Path(out["Rust Py"]).read_bytes()

    def lines_of(p):
        with open(p, "rb") as f:
            return sum(1 for _ in f)

    print(f"\n=== Four-way comparison on {Path(corpus).name} ===")
    print(f"{'stack':14s} {'corpus lines':>13s} {'time':>9s} {'lines/s':>12s} {'peak RSS':>11s}")
    for label, c, t, rss in rows:
        n = lines_of(c)
        print(f"{label:14s} {n:13,d} {t:8.2f}s {n/t:12,.0f} {rss/1048576:9.1f}MB")

    print()
    print(f"identity  CLI  (C++ vs Rust, byte-for-byte): {'IDENTICAL' if cli_same else 'DIFFERS'}")
    print(f"identity  Py   (C++ vs Rust, tuple dump):     {'IDENTICAL' if py_same else 'DIFFERS'}")

    # speed/memory verdicts
    def find(label):
        for l, c, t, rss in rows:
            if l == label:
                return t, rss
    ccli, rcli = find("C++ CLI"), find("Rust CLI")
    cpy, rpy = find("C++ Py"), find("Rust Py")
    print()
    print(f"CLI:    Rust is {ccli[0]/rcli[0]:.2f}x speed, {rcli[1]/ccli[1]:.2f}x RSS vs C++")
    print(f"Python: Rust is {cpy[0]/rpy[0]:.2f}x speed, {rpy[1]/cpy[1]:.2f}x RSS vs C++")
    return 0 if (cli_same and py_same) else 1


if __name__ == "__main__":
    sys.exit(main())
