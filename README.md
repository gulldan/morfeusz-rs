# Morfeusz Rust Port

> **A Rust rewrite of [Morfeusz](https://morfeusz.sgjp.pl/), the morphological
> analyzer and generator for Polish** by SGJP (*Zespół Słownika gramatycznego
> języka polskiego*) / IPI PAN — original C++ source at
> <https://github.com/sgjp/morfeusz> (mirror of
> <http://git.nlp.ipipan.waw.pl/SGJP/Morfeusz>).
>
> This is an independent, clean reimplementation in Rust that reads the **same
> official SGJP binary dictionaries** and is **byte-for-byte identical** to the
> reference C++ analyzer and generator (verified on 113M+ interpretations across
> four Polish corpora). It is **not** affiliated with or endorsed by SGJP; all
> credit for Morfeusz, its algorithm, tagset, and dictionaries belongs to the
> original authors. Dictionaries remain under their own SGJP licenses.

This workspace is the Rust rewrite of the existing C++ Morfeusz
implementation. The layout follows the same separation used by projects such
as Polars: a Rust core crate owns the data model and algorithms, while language
bindings stay in dedicated crates.

## Status — parity-verified rewrite

The analyzer **and** generator are fully ported to Rust as an independent
library/service/bindings complex next to the untouched C++ tree: native binary
`*.dict` reading, the VLength1/VLength2 FSA traversal, the segmentation-rules
runtime, orthographic case handling, the `InflexionGraph` (node minimization /
topological numbering), `ign` separator splitting, the legacy C ABI
(`libmorfeusz2` + `morfeusz2.h`), the PyO3 `morfeusz2` extension, the CLI
binaries, and the JSONL service. The Rust core crate is **zero-dependency**
(std only); allocators and the thread pool live only in the binary crates.

**Behavioral parity is byte-for-byte, not approximate.** The Rust output is
identical to the C++ reference on **113,290,407 interpretations across four
diverse corpora** (`tests/diff_corpus/corpora_diff.sh`), in serial and at every
thread count, plus **230 passing workspace tests** and the shared
`tests/analyzer` / `tests/generator` fixture suite. Five real bugs that only
surfaced on the real dictionary were found and fixed along the way (default
`aggl`, conditional-case weak-path pruning, edge dedup by group identity, the
faithful `InflexionGraph` port, and Morfeusz's own 1:1 case tables — Turkish
`İ` etc. — which differ from Unicode casing).

### Test data

All parity and performance numbers use the real **SGJP** binary dictionary
(`morfeusz2-dictionary-sgjp`, ~7M forms; `sgjp-a.dict` analyzer + `sgjp-s.dict`
generator) and these public Polish corpora:

| corpus  | lines     | source                                                |
|---------|-----------|-------------------------------------------------------|
| nkjp    | 8,964     | National Corpus of Polish sample                      |
| msmarco | 300,000   | MS MARCO (Polish)                                     |
| c4pl    | 400,000   | C4 / Common Crawl (Polish)                            |
| wiki_pl | 1,500,000 | Polish Wikipedia (HF `wikimedia/wikipedia` 20231101.pl) |

`tests/diff_corpus/download_corpora.py` fetches them into `/tmp/bench`.

### Hardware

Numbers below were measured on an **Apple Silicon** laptop (12 performance + 4
efficiency cores, macOS / Darwin), C++ built `-O3`, Rust built `--release` with
thin-LTO + PGO + mimalloc. **Absolute timings are machine-specific** — on a
typical Linux x86-64 server they differ, but the per-core speedup and the
near-linear multi-core scaling hold the same way (the design is portable; see
*Portability* below).

## Results

All rows are **byte-identical** to the C++ reference.

### CLI — full corpus (C++ `-O3` vs Rust)

| corpus (lines)   | stack              | time    | lines/s | peak RSS |
|------------------|--------------------|---------|---------|----------|
| msmarco (300k)   | C++ `-O3`          | 39.82s  | 7,535   | 28.6 MB  |
|                  | Rust, 1 thread     | 22.54s  | 13,311  | 25.3 MB  |
|                  | Rust, all cores    | 2.04s   | 147,073 | 256 MB   |
| c4pl (400k)      | C++ `-O3`          | 27.11s  | 14,755  | 109.6 MB |
|                  | Rust, 1 thread     | 14.49s  | 27,604  | 45.2 MB  |
|                  | Rust, all cores    | 1.44s   | 277,979 | 261 MB   |
| wiki_pl (1.5M)   | C++ `-O3`          | 65.86s  | 22,775  | 29.6 MB  |
|                  | Rust, 1 thread     | 34.99s  | 42,865  | 26.4 MB  |
|                  | Rust, all cores    | 3.64s   | 412,531 | 207 MB   |

Per core Rust is **1.77–1.88× faster** and usually leaner in RAM; with
`--threads 0` (all cores) it is **18–19.5× faster than single-threaded C++**.
The all-cores RSS (~200–260 MB) is the opt-in cost of a private decode cache per
worker; serial/default mode stays ~25–45 MB. The generator is **~2.8× faster**
per core and also leaner.

### Python bindings — 200k-line subset (C++/SWIG vs Rust/PyO3)

| corpus  | stack               | time   | lines/s | peak RSS |
|---------|---------------------|--------|---------|----------|
| msmarco | C++ `_morfeusz2` SWIG | 71.63s | 2,792 | 276.5 MB |
|         | Rust `morfeusz2` PyO3 | 35.60s | 5,619 | 201.4 MB |
| c4pl    | C++ SWIG            | 35.79s | 5,589   | 213.8 MB |
|         | Rust PyO3           | 17.54s | 11,403  | 162.0 MB |
| wiki_pl | C++ SWIG            | 32.45s | 6,164   | 160.0 MB |
|         | Rust PyO3           | 16.34s | 12,242  | 122.5 MB |

The Rust `morfeusz2` module is a **drop-in API replacement** (same
`Morfeusz(...).analyse()` tuples, generator, DAG/tag expansion, low-level
`_Morfeusz`, `MorphInterpretation`, `IdResolver`, `ResultsIterator`), builds an
abi3 wheel via `maturin`, supports free-threaded (no-GIL) CPython 3.14, and runs
**~2× faster at ~0.73–0.77× the memory** of the C++/SWIG binding — identical
output.

`analyse()` releases the GIL during the (pure-Rust) analysis, so multiple Python
threads — each with its own `Morfeusz` — run concurrently. For batch work,
**`analyse_many(texts)`** fans the analysis across a work-stealing pool with the
GIL released (each worker forks its own analyzer: shared dictionary, private
decode cache) and returns one analysis list per text, in input order,
byte-identical to a serial `analyse()` loop. On a **GIL** interpreter it is
**~2.3–3.1× faster** (the returned Python objects are still built serially under
the GIL, whereas the CLI writes plain text). On a **free-threaded** interpreter
(CPython 3.14t) the result objects are built on the workers too, so object
construction parallelizes and it reaches **~6.5×** (c4pl 50k: serial loop 3.0s →
0.46s). Verified by `tests/diff_corpus/py_parallel_check.py` on both builds.

### Reproduce

The differential checks compare against the **original C++ Morfeusz**, which is
not part of this Rust-only repo. Build it separately from the upstream source
and place (or symlink) its `morfeusz_analyzer` / `morfeusz_generator` binaries
under `build-cpp-ref-O2/` at this repo's root (the diff tooling looks there);
`cargo build`/`cargo test` and the per-call benchmarks do not need it.

```sh
python3 morfeusz-rs/tests/diff_corpus/download_corpora.py   # corpora -> /tmp/bench
bash   morfeusz-rs/tests/diff_corpus/pgo_build.sh           # PGO+mimalloc CLI
bash   morfeusz-rs/tests/diff_corpus/corpora_diff.sh        # byte-identity vs C++ (RUST_THREADS=0 for all cores)
python3 morfeusz-rs/tests/diff_corpus/full_compare.py       # the tables above (CLI + Python bindings)
/tmp/morf-builder-venv/bin/python \
       morfeusz-rs/tests/diff_corpus/py_parallel_check.py   # analyse_many() correctness + speedup
```

## Python wheel

The `morfeusz2` extension (crate `python/`) is built with
[maturin](https://www.maturin.rs/). One forward-compatible **abi3** wheel covers
CPython 3.9+; free-threaded interpreters get their own version-specific wheel
automatically.

```sh
pip install maturin

# Build a release wheel  ->  target/wheels/morfeusz2-*.whl
maturin build --release -m python/Cargo.toml
pip install target/wheels/morfeusz2-*.whl

# ...or develop-install (editable) into the ACTIVE virtualenv
cd python && maturin develop --release
```

Since `python/pyproject.toml` declares maturin as the build backend, plain
PEP 517 works too: `pip install ./python` (or `pip wheel ./python`).

**Free-threaded (no-GIL) CPython 3.14t** — build against the free-threaded
interpreter and maturin emits a version-specific `cp314t` wheel (abi3 is
auto-disabled there), which enables the parallel object-building path in
`analyse_many`:

```sh
python3.14t -m venv .venv-ft && . .venv-ft/bin/activate
pip install maturin
maturin build --release -m python/Cargo.toml   # -> ...-cp314-cp314t-*.whl
```

Smoke test the installed module:

```python
import morfeusz2
m = morfeusz2.Morfeusz(dict_name="sgjp", dict_path="/path/to/sgjp-dict-dir")
print(m.analyse("Ala ma kota"))
```

## Further optimization ideas

Per *single-call* analysis the code is near its algorithmic ceiling (~50% of
runtime is the inherent FSA + segmentation DFS). The highest-value Python-binding
wins — GIL release and the batch `analyse_many` — are already implemented (see
above); the remaining ideas are in deployment and incremental per-core/memory
work. Ordered roughly by value-to-effort; all must keep byte-identical output.

**Per-core speed / memory**
- **Share output strings via `Arc<str>`**: orth is currently cloned per
  interpretation (~226M `String` allocs on the 113M-interp set); sharing one
  orth across a chunk's interpretations, and reusing it for identity-form lemmas,
  cuts allocations and peak RAM (~3-5%). Trade-off: changes
  `MorphInterpretation`'s field types (internal Rust API only; CLI/Python/C
  consumers read `&str`).
- **`mmap` the dictionary** (binary-only `memmap2`) instead of reading into
  `Arc<[u8]>`: faster cold start, lazy paging, and pages shared across processes
  and threads — lower RSS, especially for many-process / many-thread deployments.
- **SIMD** the FSA label scan and ASCII case-folding (portable-simd). Speculative
  — profile first; the DFS is branch-bound, not obviously vectorizable.
- **Inline storage** for the common single-segment path buffers (a tiny
  hand-rolled inline vector keeps the core dependency-free). Low ROI now that
  mimalloc absorbs most of the per-word allocation churn.

**Throughput / parallel**
- **Parallelize `CONTINUOUS` numbering** (currently serial): analyze each line
  from node 0 in parallel, then prefix-sum the per-line node counts to offset —
  preserves identity.
- **Lower the all-cores RSS** (~200-260 MB = N private 32K-group decode caches):
  a smaller per-thread cap, or a shared read-mostly cache (immutable snapshot
  read lock-free + per-thread write-through) to recover cross-thread reuse
  without the lock contention that makes naive sharing scale *negatively*.
- **Reader-thread pipelining** (only the writer overlaps today) — helps when
  input is a slow pipe rather than a page-cached file.

**Build / deployment**
- **PGO on the target platform**: gather the profile in Linux CI on a production
  corpus instead of reusing a macOS profile — PGO helps most when the profile
  matches production.
- **`-C target-cpu=native`** (or a chosen feature baseline) for self-built
  deployments, on top of PGO. Caveat: the binary is then not portable across CPU
  generations.
- **LLVM BOLT** post-link optimization on Linux for a few extra percent on the
  branchy FSA code, layered on PGO + LTO.

## Layout

- `morfeusz-rs`: Rust core API and implementation.
- `capi`: C ABI compatibility crate exposing the legacy `libmorfeusz2`
  library name plus `include/morfeusz2_c.h`; it also ships
  `include/morfeusz2.h`, a C++11 source-compatibility wrapper over the C ABI
  for legacy enums, `MorphInterpretation`, instance creation, vector and
  iterator analysis, generation, id resolver lookups, dictionary metadata, and
  the core option setters/getters. The wrapper also exposes the legacy
  `Morfeusz::dictionarySearchPaths` list, initializes it to `.`, and syncs it
  into Rust dictionary lookup before instance creation and dictionary switches.
- `cli`: Rust `morfeusz_analyzer` and `morfeusz_generator` binaries.
- `python`: PyO3 extension module named `morfeusz2`.
- `service`: JSONL stdin/stdout service adapter over the Rust `Engine`.
