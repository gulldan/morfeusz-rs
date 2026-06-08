#!/usr/bin/env bash
# Differential byte-identity check of the Rust analyzer against the C++ reference
# across many huge, diverse Polish corpora (run download_corpora.py first).
#
# For each /tmp/bench/<name>.txt it feeds the corpus through both the C++ -O3 CLI
# and the Rust --release CLI with the real SGJP dictionary and reports whether the
# outputs are byte-for-byte identical, plus per-corpus line/interp counts and a
# grand total. This is the broad "does it actually work on real, diverse text"
# test — far beyond dictionary forms.
#
# Usage: corpora_diff.sh [dict-dir] [corpus names...]
set -uo pipefail

REPO="$(cd "$(dirname "$0")/../../.." && pwd)"
CPP="$REPO/build-cpp-ref-O2/morfeusz_analyzer"
RUST="$REPO/target/release/morfeusz_analyzer"
# RUST_THREADS controls the Rust analyzer's parallel mode (1 = serial, default;
# 0 or "auto" = all cores). Output must stay byte-identical to C++ at any value.
RUST_THREADS="${RUST_THREADS:-1}"
DICTDIR="${1:-/tmp/bench}"
shift || true
NAMES=("$@")
if [ ${#NAMES[@]} -eq 0 ]; then
  NAMES=()
  for f in "$DICTDIR"/wiki.txt "$DICTDIR"/nkjp.txt "$DICTDIR"/msmarco.txt "$DICTDIR"/c4pl.txt "$DICTDIR"/wiki_pl.txt; do
    [ -f "$f" ] && NAMES+=("$(basename "$f" .txt)")
  done
fi

printf "%-12s %14s %16s  %s\n" "corpus" "lines" "interps(C++)" "result"
fail=0; total_lines=0; total_interps=0
for name in "${NAMES[@]}"; do
  corpus="$DICTDIR/$name.txt"
  [ -f "$corpus" ] || { printf "%-12s %14s\n" "$name" "MISSING"; continue; }
  lines=$(wc -l < "$corpus" | tr -d ' ')
  "$CPP"  --dict sgjp --dict-dir "$DICTDIR" < "$corpus" 2>/dev/null > "/tmp/cmp_${name}_cpp.out"
  "$RUST" --dict sgjp --dict-dir "$DICTDIR" --threads "$RUST_THREADS" < "$corpus" 2>/dev/null > "/tmp/cmp_${name}_rust.out"
  interps=$(grep -c ',' "/tmp/cmp_${name}_cpp.out")
  if cmp -s "/tmp/cmp_${name}_cpp.out" "/tmp/cmp_${name}_rust.out"; then
    res="IDENTICAL"
  else
    res="DIFFERS ($(diff "/tmp/cmp_${name}_cpp.out" "/tmp/cmp_${name}_rust.out" | grep -c '^<') lines)"
    fail=1
  fi
  printf "%-12s %14s %16s  %s\n" "$name" "$lines" "$interps" "$res"
  total_lines=$((total_lines + lines)); total_interps=$((total_interps + interps))
  rm -f "/tmp/cmp_${name}_cpp.out" "/tmp/cmp_${name}_rust.out"
done
echo "------------------------------------------------------------"
printf "%-12s %14s %16s\n" "TOTAL" "$total_lines" "$total_interps"
[ $fail -eq 0 ] && echo "ALL CORPORA BYTE-IDENTICAL" || echo "SOME CORPORA DIFFER"
exit $fail
