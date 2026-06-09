# morfeusz2-rs

A **drop-in Rust/PyO3 replacement** for the Morfeusz 2 Python bindings. It keeps
the legacy `morfeusz2` API shape (the `Morfeusz` analyzer/generator, low-level
`_Morfeusz`, `MorphInterpretation`, `IdResolver`, `ResultsIterator`, DAG/tag
expansion) while delegating analysis and generation to the Rust core library —
~2× faster at lower memory, with free-threaded (no-GIL) CPython support.

## Install

```sh
pip install morfeusz2-rs
```

Prebuilt abi3 wheels (CPython 3.9+) for Linux, macOS and Windows are attached to
each [GitHub Release](https://github.com/gulldan/morfeusz-rs/releases); a source
distribution is included for everything else (and for free-threaded 3.14t).

## Usage — drop-in, without overwriting the official package

To avoid clobbering the upstream SGJP binding, this package installs under a
different name and therefore coexists with it:

| | official | this project |
|---|---|---|
| distribution name | `morfeusz2` | **`morfeusz2-rs`** |
| import name | `morfeusz2` | **`morfeusz2_rs`** |

The public API is identical, so you swap it in by **aliasing the import** and
leaving the rest of your code unchanged:

```python
import morfeusz2_rs as morfeusz2          # the one line you change

m = morfeusz2.Morfeusz(dict_name="sgjp", dict_path="/path/to/sgjp-dict-dir")
print(m.analyse("Ala ma kota"))
```

To force it project-wide without editing each import, alias it once at startup:

```python
import sys, morfeusz2_rs
sys.modules["morfeusz2"] = morfeusz2_rs   # existing `import morfeusz2` now uses Rust
```

The Rust workspace also provides the core library, C ABI compatibility layer,
C++ source-compatibility header, CLI binaries, and JSONL service adapter.
