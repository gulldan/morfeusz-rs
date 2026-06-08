#!/usr/bin/env python3
"""Per-input-line divergence analyzer using sentinel delimiting.

A multi-token input line emits several `[...]` edge groups, so the output has no
per-line boundary. We interleave a unique sentinel token before every real line;
its `[0,1,<sentinel>,...,ign,_,_]` block reliably delimits each real line's
output (SEPARATE_NUMBERING resets nodes per line, so the sentinel does not
perturb neighbours). For each diverging real line we report whether C++ and Rust
produced the same multiset reordered (REORDER) or a different set (SETDIFF).
"""
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
CPP = REPO / "build-cpp-ref-O2" / "morfeusz_analyzer"
RUST = REPO / "target" / "release" / "morfeusz_analyzer"
SENT = "Zqxwbsentinel"


def run(binary, dict_name, dict_dir, data):
    return subprocess.run(
        [str(binary), "--dict-dir", str(dict_dir), "--dict", dict_name],
        input=data.encode("utf-8"), stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
    ).stdout.decode("utf-8", "replace")


def split_on_sentinel(text):
    """Return list of per-real-line interp-row lists, split on the sentinel block."""
    rows = [ln for ln in text.split("\n")]
    groups, cur = [], None
    for ln in rows:
        s = ln.lstrip("[").strip().rstrip("]").lstrip()
        if s.startswith(f"0,1,{SENT},"):
            if cur is not None:
                groups.append(cur)
            cur = []
        elif cur is not None:
            v = ln.strip()
            if v:
                cur.append(v.lstrip("[").rstrip("]").strip())
    if cur is not None:
        groups.append(cur)
    return groups


def main():
    dict_name, dict_dir, corpus = sys.argv[1], sys.argv[2], sys.argv[3]
    lines = [l for l in Path(corpus).read_text(encoding="utf-8").split("\n") if l != ""]
    interleaved = "".join(f"{SENT}\n{l}\n" for l in lines)
    cpp = split_on_sentinel(run(CPP, dict_name, dict_dir, interleaved))
    rust = split_on_sentinel(run(RUST, dict_name, dict_dir, interleaved))
    n = min(len(cpp), len(rust), len(lines))

    reorder = setdiff = 0
    shown = 0
    for i in range(n):
        if cpp[i] == rust[i]:
            continue
        c, r = cpp[i], rust[i]
        if sorted(c) == sorted(r):
            reorder += 1
            kind, detail = "REORDER", ""
        else:
            setdiff += 1
            only_c = [x for x in c if x not in r]
            only_r = [x for x in r if x not in c]
            kind = "SETDIFF"
            detail = f"\n      only C++ : {only_c}\n      only Rust: {only_r}"
        if shown < 20:
            print(f"  [{kind}] input={lines[i]!r}{detail}")
            shown += 1
    print(f"\nlines compared: {n}  diverging: {reorder+setdiff}  |  REORDER {reorder}  SETDIFF {setdiff}")


if __name__ == "__main__":
    main()
