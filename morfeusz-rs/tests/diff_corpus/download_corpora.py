#!/usr/bin/env python3
"""Download diverse huge Polish corpora from HuggingFace for differential testing.

Pulls real Polish text across several genres so the differential tests exercise
the analyzer on far more than dictionary forms:
  - wiki    : Polish Wikipedia       (wikimedia/wikipedia 20231101.pl) — encyclopedic
  - nkjp    : National Corpus (sample) (ipipan/nkjp1m)                 — balanced/reference
  - msmarco : MS MARCO pl passages    (clarin-knext/msmarco-pl)        — QA/web passages
  - c4pl    : C4 Polish               (allenai/c4 multilingual pl)     — raw web text

Writes /tmp/bench/<name>.txt (one text unit per line). Requires `pip install
huggingface_hub pyarrow`. Use with corpora_diff.sh / compare_all.py.
"""
import gzip
import io
import json
import sys
from pathlib import Path

OUT = Path("/tmp/bench")


def save(name, lines, limit):
    OUT.mkdir(parents=True, exist_ok=True)
    buf, n = io.StringIO(), 0
    for line in lines:
        line = line.strip()
        if line:
            buf.write(line)
            buf.write("\n")
            n += 1
            if n >= limit:
                break
    (OUT / f"{name}.txt").write_text(buf.getvalue(), encoding="utf-8")
    print(f"  {name}.txt: {n:,} lines")


def get_wiki(limit=1_500_000):
    from huggingface_hub import hf_hub_download
    import pyarrow.parquet as pq
    p = hf_hub_download("wikimedia/wikipedia",
                        "20231101.pl/train-00000-of-00006.parquet", repo_type="dataset")
    texts = pq.read_table(p, columns=["text"]).column("text").to_pylist()
    return (ln for t in texts for ln in t.split("\n"))


def get_nkjp(limit=1_000_000):
    from huggingface_hub import hf_hub_download
    out = []
    for shard in ("nkjp1m_train.jsonl.gz", "nkjp1m_test.jsonl.gz", "nkjp1m_dev.jsonl.gz"):
        try:
            p = hf_hub_download("ipipan/nkjp1m", shard, repo_type="dataset")
        except Exception:
            continue
        with gzip.open(p, "rt", encoding="utf-8") as f:
            for line in f:
                o = json.loads(line)
                t = o.get("text") or o.get("sentence")
                if not t and isinstance(o.get("tokens"), list) and o["tokens"]:
                    t = " ".join(tok.get("orth", "") if isinstance(tok, dict) else str(tok)
                                 for tok in o["tokens"])
                if t:
                    out.append(t)
    return out


def get_msmarco(limit=300_000):
    from huggingface_hub import hf_hub_download
    p = hf_hub_download("clarin-knext/msmarco-pl", "corpus.jsonl.gz", repo_type="dataset")
    out = []
    with gzip.open(p, "rt", encoding="utf-8") as f:
        for i, line in enumerate(f):
            if i >= limit:
                break
            o = json.loads(line)
            t = o.get("text") or o.get("contents")
            if t:
                out.append(t)
    return out


def get_c4pl(limit=400_000):
    from huggingface_hub import hf_hub_download, HfApi
    files = HfApi().list_repo_files("allenai/c4", repo_type="dataset")
    pl = sorted(f for f in files if "/c4-pl" in f)
    p = hf_hub_download("allenai/c4", pl[0], repo_type="dataset")
    out = []
    with gzip.open(p, "rt", encoding="utf-8") as f:
        for i, line in enumerate(f):
            if len(out) >= limit:
                break
            for para in json.loads(line).get("text", "").split("\n"):
                if para.strip():
                    out.append(para)
    return out


CORPORA = {
    "wiki": (get_wiki, 1_500_000),
    "nkjp": (get_nkjp, 1_000_000),
    "msmarco": (get_msmarco, 300_000),
    "c4pl": (get_c4pl, 400_000),
}


def main():
    wanted = sys.argv[1:] or list(CORPORA)
    for name in wanted:
        fn, limit = CORPORA[name]
        print(f"{name} ...")
        try:
            save(name, fn(limit), limit)
        except Exception as e:
            print(f"  {name} FAILED: {repr(e)[:160]}")


if __name__ == "__main__":
    main()
