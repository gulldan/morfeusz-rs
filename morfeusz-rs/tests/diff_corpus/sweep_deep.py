#!/usr/bin/env python3
"""Aggressive differential parity sweep (analyzer-focused).

Builds a large adversarial corpus per dictionary from its real vocabulary:
case perturbations, word pairs/triples joined by spaces / hyphens / each
separator char, leading/trailing/internal punctuation, mixed known+unknown
tokens, digits glued to words, and the fixture's own input. Runs the full CLI
option matrix and diffs C++ vs Rust. Reports the first diverging lines per case.
"""
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
CPP = REPO / "build-cpp-ref"
RUST = REPO / "target" / "debug"
FIX = REPO / "morfeusz-rs" / "tests" / "fixtures" / "binary"
TESTS = REPO / "tests"

ANALYZER_DICTS = [
    ("test-segtypes", "analyzer/test_segtypes"),
    ("test-segtypes-homonyms", "analyzer/test_segtypes_with_homonyms"),
    ("test-names", "analyzer/test_names"),
    ("test-qualifiers", "analyzer/test_qualifiers"),
    ("test-mixed-case", "analyzer/test_mixed_case"),
    ("test-digits-roman", "analyzer/test_digits_roman"),
    ("test-inflection-graph-numbers", "analyzer/test_inflection_graph_numbers"),
    ("test-prefixes-uppercase-beginning", "analyzer/test_prefixes_with_uppercase_at_the_beginning"),
    ("test-prefixes-uppercase-middle", "analyzer/test_prefixes_with_uppercase_in_the_middle"),
    ("test-multisegments", "analyzer/test_multisegments"),
    ("test-digits", "analyzer/test_digits"),
    ("test-whitespace-append", "analyzer/test_whitespace_handling_append"),
    # NOTE: test-dict-copyright is a --dict-copyright metadata fixture, not an
    # analysis dictionary. The C++ reference segfaults when used for analysis
    # (its FSA is not a real analyzer FSA), so it has no reference behavior to
    # diff against; Rust handles it gracefully. Excluded from analysis sweeps.
]

SEPARATORS = [".", ",", ";", "-", ":", "?", "!", "/", "(", ")", "·", "+"]
WS = [" ", "  ", "\t"]


def vocab(fixture_dir):
    tab = TESTS / fixture_dir / "dictionary.tab"
    out, seen = [], set()
    if tab.exists():
        for line in tab.read_text(encoding="utf-8").splitlines():
            parts = line.split("\t")
            if len(parts) >= 2 and parts[0] and parts[0] not in seen:
                seen.add(parts[0])
                out.append(parts[0])
    return out


def perturb_case(w):
    return [w, w.upper(), w.lower(), w.capitalize(), w.swapcase()]


def corpus(fixture_dir):
    words = vocab(fixture_dir)
    lines = []
    fin = TESTS / fixture_dir / "input.txt"
    if fin.exists():
        lines += fin.read_text(encoding="utf-8").splitlines()

    sample = words[:8] if words else []
    unknowns = ["xyzzy", "Qmpx", "łabędź", "ZAŻÓŁĆ"]

    # single words + case perturbations
    for w in sample:
        lines += perturb_case(w)
    # words with attached digits / unknown suffixes/prefixes
    for w in sample[:4]:
        lines += [w + "123", "123" + w, w + "x", "x" + w, w + w]
    # known/unknown joined by every separator and whitespace
    joiners = SEPARATORS + WS + ["-", ""]
    pool = (sample[:5] or ["a"]) + unknowns[:2] + ["123", "VII"]
    for i, a in enumerate(pool):
        for b in pool[i:]:
            for j in joiners:
                lines.append(f"{a}{j}{b}")
    # triples and punctuation wrapping
    for w in sample[:3]:
        lines += [f".{w}.", f"({w})", f"{w}...", f"-{w}-", f"{w},{w};{w}",
                  f"  {w}  ", f"\t{w}\t", f"{w}\n"]
    # pure punctuation / separators
    lines += [".", "..", "...", ",", ";", "-", "--", ".,;", "?!", "()",
              "", " ", "   ", "a.b.c.d", "1,2,3", "x-y-z", "co?", "n.p.",
              "p.n.e.", "tzn.", "O.K.", "3.14", "1+1"]
    return "\n".join(lines) + "\n"


FLAG_MATRIX = [
    [],
    ["--aggl", "permissive"],
    ["--aggl", "strict"],
    ["--whitespace-handling", "KEEP_WHITESPACES"],
    ["--whitespace-handling", "APPEND_WHITESPACES"],
    ["--case-handling", "STRICTLY_CASE_SENSITIVE"],
    ["--case-handling", "IGNORE_CASE"],
    ["--case-handling", "CONDITIONALLY_CASE_SENSITIVE"],
    ["--token-numbering", "CONTINUOUS_NUMBERING"],
]


def run(binary, dict_name, flags, corpus_text):
    return subprocess.run(
        [str(binary), "--dict-dir", str(FIX), "--dict", dict_name, *flags],
        input=corpus_text.encode("utf-8"),
        stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    ).stdout.decode("utf-8", "replace")


def main():
    total = diffs = 0
    diff_cases = []
    for dict_name, fixture_dir in ANALYZER_DICTS:
        text = corpus(fixture_dir)
        for flags in FLAG_MATRIX:
            # aggl strict is unsupported on some dicts; skip if C++ errors empty
            total += 1
            cpp = run(CPP / "morfeusz_analyzer", dict_name, flags, text)
            rust = run(RUST / "morfeusz_analyzer", dict_name, flags, text)
            if cpp != rust:
                diffs += 1
                diff_cases.append((dict_name, flags, text, cpp, rust))
    print(f"\n=== {total - diffs}/{total} cases MATCH, {diffs} DIFF ===\n")
    for dict_name, flags, text, cpp, rust in diff_cases:
        print(f"DIFF: {dict_name} {' '.join(flags)}")
        cl, rl, inputs = cpp.splitlines(), rust.splitlines(), text.splitlines()
        shown = 0
        for i in range(max(len(cl), len(rl))):
            c = cl[i] if i < len(cl) else "<none>"
            r = rl[i] if i < len(rl) else "<none>"
            if c != r:
                print(f"      C++ : {c!r}")
                print(f"      Rust: {r!r}")
                shown += 1
                if shown >= 8:
                    print("      ...")
                    break
        print()
    return 1 if diffs else 0


if __name__ == "__main__":
    sys.exit(main())
