#!/usr/bin/env bash
# Profile-Guided Optimization build of the Rust CLI. Branchy code like the FSA /
# segmentation traversal benefits from PGO with no source change. Instruments,
# profiles on a representative (diverse) corpus, then rebuilds optimized.
#
# Usage: pgo_build.sh [profile-corpus] [dict-dir]
set -euo pipefail

RUSTDIR="$(cd "$(dirname "$0")/../../.." && pwd)"
CORPUS="${1:-/tmp/bench/pgo_train.txt}"
DICTDIR="${2:-/tmp/bench}"
PGO="/tmp/pgo"
PROFDATA="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's/host: //p')/bin/llvm-profdata"

cd "$RUSTDIR"
rm -rf "$PGO"; mkdir -p "$PGO"

echo "[1/4] building instrumented CLI..."
RUSTFLAGS="-Cprofile-generate=$PGO" cargo build --release -q -p morfeusz-cli

echo "[2/4] gathering profile on $(wc -l < "$CORPUS" | tr -d ' ') lines..."
./target/release/morfeusz_analyzer --dict sgjp --dict-dir "$DICTDIR" < "$CORPUS" >/dev/null 2>/dev/null
./target/release/morfeusz_generator --dict sgjp --dict-dir "$DICTDIR" < "$DICTDIR/lemmas.txt" >/dev/null 2>/dev/null || true

echo "[3/4] merging profile data..."
"$PROFDATA" merge -o "$PGO/merged.profdata" "$PGO"/*.profraw

echo "[4/4] rebuilding optimized with profile..."
RUSTFLAGS="-Cprofile-use=$PGO/merged.profdata -Cllvm-args=-pgo-warn-missing-function" \
  cargo build --release -q -p morfeusz-cli

echo "Done. PGO-optimized binaries in target/release/."
