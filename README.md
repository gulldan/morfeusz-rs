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

Absolute timings are machine-specific. The current tables were measured on:

| component | value |
|-----------|-------|
| CPU | AMD Ryzen 9 7950X, 16 cores / 32 threads |
| RAM | 124 GiB |
| C++ CLI build | upstream C++ source, `-O3 -DNDEBUG` |
| Rust CLI build | `--release`, thin-LTO, PGO |
| Rust Python build | `maturin develop --release` |
| allocator | mimalloc in the CLI/Python crates |

C++ reference comparisons can be reproduced locally when the upstream C++
binaries are available under `build-cpp-ref-O2/`.

## Results

All correctness rows are **byte-identical** to the checked reference output. The
current Rust build includes the bounded positive word-template cache for
repeated word analyses; it changes performance only, not API shape or results.

### CLI — C++ reference vs current Rust

README-style full-corpus run with the original C++ implementation and the
current Rust implementation on the same machine. The Rust parallel rows use
`--threads 0`, which resolves to 32 worker threads on this host.

| corpus (lines)   | stack              | time   | lines/s | vs C++ | peak RSS | output |
|------------------|--------------------|--------|---------|--------|----------|--------|
| msmarco (300k)   | C++ `-O3`          | 34.37s | 8,729   | 1.00x  | 34.3 MB  | reference |
|                  | Rust, 1 thread     | 10.78s | 27,832  | 3.19x  | 44.9 MB  | identical |
|                  | Rust, 32 threads   | 2.16s  | 139,192 | 15.95x | 418.8 MB | identical |
| c4pl (400k)      | C++ `-O3`          | 23.04s | 17,361  | 1.00x  | 79.2 MB  | reference |
|                  | Rust, 1 thread     | 8.71s  | 45,928  | 2.65x  | 74.9 MB  | identical |
|                  | Rust, 32 threads   | 1.92s  | 208,058 | 11.98x | 452.8 MB | identical |
| wiki_pl (1.5M)   | C++ `-O3`          | 54.84s | 27,352  | 1.00x  | 35.3 MB  | reference |
|                  | Rust, 1 thread     | 20.30s | 73,898  | 2.70x  | 52.9 MB  | identical |
|                  | Rust, 32 threads   | 7.47s  | 200,754 | 7.34x  | 354.8 MB | identical |

The timing rows redirect analyzer output to `/dev/null`. Byte identity was
checked separately with streaming SHA-256 over full stdout; Rust matches the C++
reference on all three full corpora above, in both serial and 32-thread modes.
The 32-thread RSS is the opt-in cost of per-worker analyzer state and caches;
serial/default mode stays much smaller and is the best mode for embedded or
memory-sensitive use.

### Python bindings — current PyO3 throughput

The Python extension uses the same Rust core as the CLI. Its throughput is lower
than the CLI because Python still has to allocate lists/tuples and format Python
objects.

README-style dump mode: high-level `Morfeusz(...).analyse()` tuples serialized
to stdout on 200k-line subsets.

| corpus  | time   | lines/s | peak RSS | output |
|---------|--------|---------|----------|--------|
| msmarco | 28.21s | 7,090   | 207.0 MB | identical |
| c4pl    | 15.15s | 13,198  | 162.7 MB | identical |
| wiki_pl | 13.74s | 14,554  | 137.9 MB | identical |

Pure `analyse()` loop mode: Python tuples are still built, but no giant dump is
written.

| corpus  | time   | lines/s |
|---------|--------|---------|
| msmarco | 13.06s | 15,314  |
| c4pl    | 7.66s  | 26,096  |
| wiki_pl | 6.90s  | 28,979  |

The Rust `morfeusz2_rs` module remains a **drop-in API replacement** (same
`Morfeusz(...).analyse()` tuples, generator, DAG/tag expansion, low-level
`_Morfeusz`, `MorphInterpretation`, `IdResolver`, `ResultsIterator`), builds an
abi3 wheel via `maturin`, supports free-threaded (no-GIL) CPython 3.14, and
keeps identical Python-level output.

### Drop-in replacement for the official `morfeusz2`

To avoid clobbering the upstream SGJP binding, this package installs under a
**different name** and therefore coexists with it — installing one does not
overwrite the other:

| | official | this project |
|---|---|---|
| PyPI / wheel name | `morfeusz2` | **`morfeusz2-rs`** |
| import name | `morfeusz2` | **`morfeusz2_rs`** |

The public API is identical, so you swap it in by **aliasing the import** and
leaving the rest of your code untouched:

```python
import morfeusz2_rs as morfeusz2          # the one line you change

m = morfeusz2.Morfeusz(dict_name="sgjp", dict_path="/path/to/sgjp-dict-dir")
print(m.analyse("Ala ma kota"))           # everything below is unchanged
```

To force it project-wide without touching each file, alias it once at startup
(e.g. `import sys, morfeusz2_rs; sys.modules["morfeusz2"] = morfeusz2_rs`) so that
existing `import morfeusz2` statements resolve to the Rust module.

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
/tmp/morf-builder-venv/bin/python \
       morfeusz-rs/tests/diff_corpus/py_parallel_check.py   # analyse_many() correctness + throughput
```

## Python wheel

Prebuilt wheels for Linux (x86_64/aarch64), macOS (Intel/Apple Silicon) and
Windows are attached to every
[GitHub Release](https://github.com/gulldan/morfeusz-rs/releases) — download the
one for your platform and `pip install` it. To build it yourself:

The `morfeusz2_rs` extension (crate `python/`) is built with
[maturin](https://www.maturin.rs/). One forward-compatible **abi3** wheel covers
CPython 3.9+; free-threaded interpreters get their own version-specific wheel
automatically.

```sh
pip install maturin

# Build a release wheel  ->  target/wheels/morfeusz2_rs-*.whl
maturin build --release -m python/Cargo.toml
pip install target/wheels/morfeusz2_rs-*.whl

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
import morfeusz2_rs as morfeusz2
m = morfeusz2.Morfeusz(dict_name="sgjp", dict_path="/path/to/sgjp-dict-dir")
print(m.analyse("Ala ma kota"))
```

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
