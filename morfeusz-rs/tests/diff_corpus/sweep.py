#!/usr/bin/env python3
"""Differential parity sweep between the C++ reference and the Rust CLI.

For every fixture dictionary it builds an input corpus from the dictionary's own
vocabulary plus adversarial inputs (case variants, multi-token lines, separators,
hyphenation, unknown words) and runs both implementations under a matrix of CLI
option combinations, diffing stdout. Prints a summary of MATCH/DIFF per case and
shows the first diverging lines for each DIFF.
"""
import itertools
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
CPP = REPO / "build-cpp-ref"
RUST = REPO / "target" / "debug"
FIX = REPO / "morfeusz-rs" / "tests" / "fixtures" / "binary"
TESTS = REPO / "tests"

# (dict-name, source-fixture-relative-dir, kind)
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
    ("test-dict-copyright", "analyzer/test_dict_copyright"),
]
GENERATOR_DICTS = [
    ("test-digits", "generator/test_digits"),
    ("test-names", "generator/test_names"),
    ("test-qualifiers", "generator/test_qualifiers"),
    ("test-segtypes", "generator/test_segtypes"),
    ("test-additional-atomic", "generator/test_additional_atomic_segments"),
]


def vocab(fixture_dir):
    """Lemma/form column from the source dictionary.tab, if present."""
    tab = TESTS / fixture_dir / "dictionary.tab"
    words = []
    if tab.exists():
        for line in tab.read_text(encoding="utf-8").splitlines():
            parts = line.split("\t")
            if len(parts) >= 2 and parts[0]:
                words.append(parts[0])
    # dedup preserve order
    seen = set()
    out = []
    for w in words:
        if w not in seen:
            seen.add(w)
            out.append(w)
    return out


def analyzer_corpus(fixture_dir):
    words = vocab(fixture_dir)
    lines = []
    fixture_input = TESTS / fixture_dir / "input.txt"
    if fixture_input.exists():
        lines.extend(fixture_input.read_text(encoding="utf-8").splitlines())
    sample = words[:6]
    for w in sample:
        lines.append(w)
        lines.append(w.upper())
        lines.append(w.capitalize())
        lines.append(w + "x")          # unknown extension
    # multi-token lines
    if len(sample) >= 2:
        lines.append(f"{sample[0]} {sample[1]}")
        lines.append(f"  {sample[0]}   {sample[1]}  ")
        lines.append(f"{sample[0]}, {sample[1]}.")
        lines.append(f"{sample[0]}-{sample[1]}")
        lines.append(f"{sample[0]}\t{sample[1]}")
    # separators / punctuation / unknowns
    lines += ["", "   ", ".", "...", "a.b.c", "12-34", "foo", "FOO", "Łódź",
              "co?", "n.p.", "x-y-z", "a,b", "  leading", "trailing  "]
    return "\n".join(lines) + "\n"


def generator_corpus(fixture_dir):
    words = vocab(fixture_dir)
    lines = []
    fixture_input = TESTS / fixture_dir / "input.txt"
    if fixture_input.exists():
        lines.extend(fixture_input.read_text(encoding="utf-8").splitlines())
    for w in words[:10]:
        lines.append(w)
        lines.append(w.capitalize())
    lines += ["123", "foo", "nieznane"]
    return "\n".join(lines) + "\n"


def run(binary, dict_name, flags, corpus):
    proc = subprocess.run(
        [str(binary), "--dict-dir", str(FIX), "--dict", dict_name, *flags],
        input=corpus.encode("utf-8"),
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
    )
    return proc.stdout.decode("utf-8", "replace")


ANALYZER_FLAG_MATRIX = [
    [],
    ["--aggl", "permissive"],
    ["--whitespace-handling", "KEEP_WHITESPACES"],
    ["--whitespace-handling", "APPEND_WHITESPACES"],
    ["--case-handling", "STRICTLY_CASE_SENSITIVE"],
    ["--case-handling", "IGNORE_CASE"],
    ["--token-numbering", "CONTINUOUS_NUMBERING"],
]
GENERATOR_FLAG_MATRIX = [[]]


def main():
    only = sys.argv[1] if len(sys.argv) > 1 else None
    total = 0
    diffs = 0
    diff_cases = []
    for dict_name, fixture_dir in ANALYZER_DICTS:
        if only and only not in dict_name:
            continue
        corpus = analyzer_corpus(fixture_dir)
        for flags in ANALYZER_FLAG_MATRIX:
            total += 1
            cpp = run(CPP / "morfeusz_analyzer", dict_name, flags, corpus)
            rust = run(RUST / "morfeusz_analyzer", dict_name, flags, corpus)
            if cpp != rust:
                diffs += 1
                diff_cases.append(("analyzer", dict_name, flags, corpus, cpp, rust))
    for dict_name, fixture_dir in GENERATOR_DICTS:
        if only and only not in dict_name:
            continue
        corpus = generator_corpus(fixture_dir)
        for flags in GENERATOR_FLAG_MATRIX:
            total += 1
            cpp = run(CPP / "morfeusz_generator", dict_name, flags, corpus)
            rust = run(RUST / "morfeusz_generator", dict_name, flags, corpus)
            if cpp != rust:
                diffs += 1
                diff_cases.append(("generator", dict_name, flags, corpus, cpp, rust))

    print(f"\n=== {total - diffs}/{total} cases MATCH, {diffs} DIFF ===\n")
    for mode, dict_name, flags, corpus, cpp, rust in diff_cases:
        print(f"DIFF: {mode} {dict_name} {' '.join(flags)}")
        cpp_lines = cpp.splitlines()
        rust_lines = rust.splitlines()
        shown = 0
        for i in range(max(len(cpp_lines), len(rust_lines))):
            c = cpp_lines[i] if i < len(cpp_lines) else "<none>"
            r = rust_lines[i] if i < len(rust_lines) else "<none>"
            if c != r:
                print(f"    line {i}:")
                print(f"      C++ : {c!r}")
                print(f"      Rust: {r!r}")
                shown += 1
                if shown >= 6:
                    print("    ...")
                    break
        print()
    return 1 if diffs else 0


if __name__ == "__main__":
    sys.exit(main())
