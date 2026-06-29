#!/bin/bash
# mp3rgain #201 — produce the golden ReplayGain value from the reference
# gain_analysis.c, fed the exact PCM that the Rust `golden_pcm()` emits.
#
# Local, one-time use. Not run in CI (no C toolchain in CI by design).
#
#   ./tests/reference/run.sh
#
# Prints GetTitleGain() in dB — paste it into GOLDEN_GAIN_DB in
# src/replaygain.rs (analysis_matches_reference_c_to_float_precision).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
DUMP="${RG_PCM_DUMP:-/tmp/rg_golden_pcm.bin}"
BIN="$(mktemp -d)/rgref"

echo "1/3 regenerating PCM dump -> $DUMP"
RG_PCM_DUMP="$DUMP" cargo test --quiet --lib --features replaygain \
    dump_golden_pcm -- --ignored --nocapture

echo "2/3 compiling reference harness (Float_t=double)"
cc -O2 -std=c11 -Wall -o "$BIN" "$HERE/main.c" "$HERE/gain_analysis.c" -lm

echo "3/3 reference GetTitleGain():"
"$BIN" "$DUMP"
