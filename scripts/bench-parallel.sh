#!/usr/bin/env bash
# scripts/bench-parallel.sh
#
# Reproducible benchmark for the -j / --threads parallelism work
# (see docs/perf-parallel.md and issues #125, #126).
#
# Usage:
#   scripts/bench-parallel.sh <CORPUS_DIR> [-- <hyperfine-args>...]
#
# Example:
#   cargo build --release
#   scripts/bench-parallel.sh /tmp/mp3rgain-bench -- --runs 5
#
# Outputs:
#   /tmp/bench-info.md   — TSV markdown summary (default cmd_info)
#   /tmp/bench-track.md  — track gain dry-run summary (-r -n)
#   /tmp/bench-album.md  — album gain dry-run summary (-a -n)
#
# Prerequisites:
#   - hyperfine (brew install hyperfine)
#   - target/release/mp3rgain built from this branch
#   - corpus pre-staged on a fast local disk (avoid USB / network)

set -euo pipefail

CORPUS="${1:-}"
if [ -z "$CORPUS" ] || [ ! -d "$CORPUS" ]; then
  echo "usage: $0 <CORPUS_DIR> [-- <hyperfine-args>...]" >&2
  exit 1
fi
shift

# Allow the caller to pass extra hyperfine flags after `--`.
if [ "${1:-}" = "--" ]; then
  shift
fi
HYPERFINE_EXTRA=("$@")

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$REPO_ROOT/target/release/mp3rgain"
if [ ! -x "$BIN" ]; then
  echo "error: $BIN not found. Run 'cargo build --release' first." >&2
  exit 1
fi

if ! command -v hyperfine >/dev/null 2>&1; then
  echo "error: hyperfine not installed. Try 'brew install hyperfine'." >&2
  exit 1
fi

cd "$CORPUS"

DEFAULT_FLAGS=(--warmup 1 --runs 3)
HYPERFINE_FLAGS=("${DEFAULT_FLAGS[@]}" "${HYPERFINE_EXTRA[@]}")

echo "== cmd_info (TSV) =="
hyperfine "${HYPERFINE_FLAGS[@]}" \
  -n "info -j 1" "$BIN -j 1 -R -q -o tsv ." \
  -n "info -j 2" "$BIN -j 2 -R -q -o tsv ." \
  -n "info -j 4" "$BIN -j 4 -R -q -o tsv ." \
  -n "info -j 8" "$BIN -j 8 -R -q -o tsv ." \
  --export-markdown /tmp/bench-info.md \
  --export-json /tmp/bench-info.json

echo "== cmd_track_gain dry-run =="
hyperfine "${HYPERFINE_FLAGS[@]}" \
  -n "track -j 1" "$BIN -j 1 -R -r -n -q -o tsv ." \
  -n "track -j 2" "$BIN -j 2 -R -r -n -q -o tsv ." \
  -n "track -j 4" "$BIN -j 4 -R -r -n -q -o tsv ." \
  -n "track -j 8" "$BIN -j 8 -R -r -n -q -o tsv ." \
  --export-markdown /tmp/bench-track.md \
  --export-json /tmp/bench-track.json

echo "== cmd_album_gain dry-run =="
hyperfine "${HYPERFINE_FLAGS[@]}" \
  -n "album -j 1" "$BIN -j 1 -R -a -n -q -o tsv ." \
  -n "album -j 2" "$BIN -j 2 -R -a -n -q -o tsv ." \
  -n "album -j 4" "$BIN -j 4 -R -a -n -q -o tsv ." \
  -n "album -j 8" "$BIN -j 8 -R -a -n -q -o tsv ." \
  --export-markdown /tmp/bench-album.md \
  --export-json /tmp/bench-album.json

echo
echo "Results:"
echo "  /tmp/bench-info.md"
echo "  /tmp/bench-track.md"
echo "  /tmp/bench-album.md"
