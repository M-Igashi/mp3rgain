#!/bin/bash
# mp3rgain #201 — produce the golden ReplayGain values from the reference
# gain_analysis.c, fed the exact PCM that the Rust `golden_pcm()` emits.
#
# Local, one-time use. Not run in CI (no C toolchain in CI by design).
#
#   ./tests/reference/run.sh [rate[:windows:tail]...]
#   # default: 44100 22050 11025 44100:79:1000
#
# Prints GetTitleGain() in dB per sample rate, number of full windows
# (default 80) and partial window of `tail` samples after them (issue #423) —
# paste them into GOLDEN_GAINS_DB in src/replaygain.rs.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
DUMP="${RG_PCM_DUMP:-/tmp/rg_golden_pcm.bin}"
BIN="$(mktemp -d)/rgref"
RATES="${*:-44100 22050 11025 44100:79:1000}"

echo "compiling reference harness (Float_t=double)"
cc -O2 -std=c11 -Wall -o "$BIN" "$HERE/main.c" "$HERE/gain_analysis.c" -lm

for entry in $RATES; do
    IFS=: read -r rate windows tail <<<"$entry"
    windows="${windows:-80}"
    tail="${tail:-0}"
    RG_PCM_DUMP="$DUMP" RG_PCM_RATE="$rate" RG_PCM_WINDOWS="$windows" RG_PCM_TAIL="$tail" \
        cargo test --quiet --lib --features replaygain dump_golden_pcm -- --ignored >/dev/null
    printf '%6s Hz, %2s windows + %4s: %s\n' "$rate" "$windows" "$tail" "$("$BIN" "$DUMP")"
done
