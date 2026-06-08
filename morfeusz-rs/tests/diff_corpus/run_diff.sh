#!/usr/bin/env bash
# Differential parity harness: runs the C++ reference and the Rust CLI on the
# same dictionary + input corpus + option set, and diffs their stdout.
#
# Usage: run_diff.sh <analyzer|generator> <dict-name> [extra CLI flags...]
# Reads the input corpus from stdin.
set -uo pipefail

REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
CPP_BIN="$REPO/build-cpp-ref"
RUST_BIN="$REPO/target/debug"
FIX="$REPO/rust/morfeusz-rs/tests/fixtures/binary"

mode="$1"; dict="$2"; shift 2
flags=("$@")

case "$mode" in
  analyzer) cpp="$CPP_BIN/morfeusz_analyzer"; rust="$RUST_BIN/morfeusz_analyzer" ;;
  generator) cpp="$CPP_BIN/morfeusz_generator"; rust="$RUST_BIN/morfeusz_generator" ;;
  *) echo "bad mode: $mode" >&2; exit 2 ;;
esac

corpus="$(cat)"

cpp_out="$(printf '%s' "$corpus" | "$cpp" --dict-dir "$FIX" --dict "$dict" "${flags[@]}" 2>/dev/null)"
rust_out="$(printf '%s' "$corpus" | "$rust" --dict-dir "$FIX" --dict "$dict" "${flags[@]}" 2>/dev/null)"

if [ "$cpp_out" = "$rust_out" ]; then
  echo "MATCH: $mode $dict ${flags[*]}"
  exit 0
else
  echo "DIFF:  $mode $dict ${flags[*]}"
  diff <(printf '%s' "$cpp_out") <(printf '%s' "$rust_out") | head -40
  exit 1
fi
