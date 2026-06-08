#!/usr/bin/env python3
"""Generate committed golden parity fixtures from the C++ reference.

Writes, under rust/morfeusz-rs/tests/parity/:
  <dict>/input.txt            deterministic adversarial corpus for the dictionary
  <dict>/<flagkey>.expected   C++ analyzer output for that corpus + option set
  manifest.tsv                dict_name <tab> dict_file <tab> flagkey per case

The Rust test `cpp_golden.rs` replays each corpus through the Rust analyzer and
asserts byte-equality with the committed `.expected` file, locking in behavioral
parity without needing the C++ build at test time. Re-run this script (after
`bash build-cpp-ref.sh`) only when the C++ reference is the intended source of
truth for new cases.
"""
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
CPP = REPO / "build-cpp-ref"
FIX = REPO / "morfeusz-rs" / "tests" / "fixtures" / "binary"
TESTS = REPO / "tests"
OUT = REPO / "morfeusz-rs" / "tests" / "parity"

# dict-name, source-fixture-dir, dict-file (in fixtures/binary)
DICTS = [
    ("test-segtypes", "analyzer/test_segtypes", "test-segtypes-a.dict"),
    ("test-segtypes-homonyms", "analyzer/test_segtypes_with_homonyms", "test-segtypes-homonyms-a.dict"),
    ("test-names", "analyzer/test_names", "test-names-a.dict"),
    ("test-qualifiers", "analyzer/test_qualifiers", "test-qualifiers-a.dict"),
    ("test-mixed-case", "analyzer/test_mixed_case", "test-mixed-case-a.dict"),
    ("test-digits-roman", "analyzer/test_digits_roman", "test-digits-roman-a.dict"),
    ("test-inflection-graph-numbers", "analyzer/test_inflection_graph_numbers", "test-inflection-graph-numbers-a.dict"),
    # dict_name MUST be a C++-loadable <name>-a.dict basename in fixtures/binary.
    ("test-prefixes-uppercase-beginning", "analyzer/test_prefixes_with_uppercase_at_the_beginning", "test-prefixes-uppercase-beginning-a.dict"),
    ("test-prefixes-uppercase-middle", "analyzer/test_prefixes_with_uppercase_in_the_middle", "test-prefixes-uppercase-middle-a.dict"),
    ("test-multisegments", "analyzer/test_multisegments", "test-multisegments-a.dict"),
    ("test-digits", "analyzer/test_digits", "test-digits-a.dict"),
    ("test-whitespace-append", "analyzer/test_whitespace_handling_append", "test-whitespace-append-a.dict"),
]

SEPARATORS = [".", ",", ";", "-", ":", "?", "!", "/", "(", ")"]
WS = [" ", "  ", "\t"]

# flagkey -> CLI flags
FLAGS = {
    "default": [],
    "keep": ["--whitespace-handling", "KEEP_WHITESPACES"],
    "append": ["--whitespace-handling", "APPEND_WHITESPACES"],
    "strict": ["--case-handling", "STRICTLY_CASE_SENSITIVE"],
    "ignore": ["--case-handling", "IGNORE_CASE"],
    "continuous": ["--token-numbering", "CONTINUOUS_NUMBERING"],
}


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


def corpus(fixture_dir):
    words = vocab(fixture_dir)
    lines = []
    fin = TESTS / fixture_dir / "input.txt"
    if fin.exists():
        lines += fin.read_text(encoding="utf-8").splitlines()
    sample = words[:8]
    unknowns = ["xyzzy", "Qmpx", "łabędź", "ZAŻÓŁĆ"]
    for w in sample:
        lines += [w, w.upper(), w.lower(), w.capitalize(), w.swapcase()]
    for w in sample[:4]:
        lines += [w + "123", "123" + w, w + "x", "x" + w, w + w]
    joiners = SEPARATORS + WS + ["-", ""]
    pool = (sample[:5] or ["a"]) + unknowns[:2] + ["123", "VII"]
    for i, a in enumerate(pool):
        for b in pool[i:]:
            for j in joiners:
                lines.append(f"{a}{j}{b}")
    for w in sample[:3]:
        lines += [f".{w}.", f"({w})", f"{w}...", f"-{w}-", f"{w},{w};{w}",
                  f"  {w}  ", f"\t{w}\t"]
    lines += [".", "..", "...", ",", ";", "-", "--", ".,;", "?!", "()",
              "", " ", "   ", "a.b.c.d", "1,2,3", "x-y-z", "co?", "n.p.",
              "p.n.e.", "tzn.", "O.K.", "3.14", "1+1"]
    return "\n".join(lines) + "\n"


def run_cpp(dict_name, flags, corpus_text):
    proc = subprocess.run(
        [str(CPP / "morfeusz_analyzer"), "--dict-dir", str(FIX), "--dict", dict_name, *flags],
        input=corpus_text.encode("utf-8"),
        stdout=subprocess.PIPE, stderr=subprocess.PIPE,
    )
    out = proc.stdout.decode("utf-8", "replace")
    # The reference exits non-zero on dictionary load failure (and can segfault on
    # malformed dictionaries). Refuse to commit a degenerate golden silently.
    if proc.returncode != 0 or out.strip() in ("", "[]"):
        raise SystemExit(
            f"C++ reference produced no usable output for {dict_name} {flags} "
            f"(exit {proc.returncode}): {proc.stderr.decode('utf-8', 'replace').strip()}"
        )
    return out


def main():
    if not (CPP / "morfeusz_analyzer").exists():
        print("C++ reference missing; run `bash build-cpp-ref.sh` first", file=sys.stderr)
        return 2
    OUT.mkdir(parents=True, exist_ok=True)
    manifest = []
    for dict_name, fixture_dir, dict_file in DICTS:
        d = OUT / dict_name
        d.mkdir(exist_ok=True)
        text = corpus(fixture_dir)
        (d / "input.txt").write_text(text, encoding="utf-8")
        for flagkey, flags in FLAGS.items():
            expected = run_cpp(dict_name, flags, text)
            (d / f"{flagkey}.expected").write_text(expected, encoding="utf-8")
            manifest.append(f"{dict_name}\t{dict_file}\t{flagkey}")
    (OUT / "manifest.tsv").write_text("\n".join(manifest) + "\n", encoding="utf-8")
    print(f"Wrote {len(manifest)} golden cases across {len(DICTS)} dictionaries to {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
