# ReplayGain analysis cross-check (issue #201)

This directory isolates mp3rgain's ReplayGain **analysis** from its decoder and
checks it against the original `gain_analysis.c` — the same lineage mp3gain
uses (David Robinson / Glen Sawyer, LGPL-2.1).

## Why

mp3rgain is not a port of mp3gain's decoder. The only thing carried over from
the mp3gain lineage is the loudness analysis; the PCM feeding it comes from an
independent decoder (symphonia). End-to-end, mp3rgain and mp3gain agree on the
quantized gain step on every fixture, but the raw dB differs by ~0.05 dB on
broadband signals (pink noise). The hypothesis was that this gap is a
*decoder* difference, not an *analysis* bug.

To prove it, hold the PCM constant and vary only the analysis: feed the
identical samples into mp3rgain's analyzer and into the reference C, and compare.

## What

- `gain_analysis.c` / `gain_analysis.h` — vendored from
  <https://github.com/cpuimage/ReplayGainAnalysis> (Glen Sawyer's
  `gain_analysis.c` lineage). **One change:** `Float_t` is `double` instead of
  the upstream `float`, to match mp3gain's original precision and mp3rgain's
  f64. LGPL-2.1; used here for local verification only.
- `main.c` — reads the PCM dump and prints `GetTitleGain()`.
- `run.sh` — regenerates the dump from Rust, compiles the harness, prints the
  gain.

This is **not** built by Cargo and **not** shipped in the crate (`tests/` is
excluded from the package, and no C toolchain is added to CI). It exists to
reproduce the golden value frozen in `src/replaygain.rs`.

## Reproduce

```sh
./tests/reference/run.sh
```

The Rust test `analysis_matches_reference_c_to_float_precision`
(in `src/replaygain.rs`) feeds the identical `golden_pcm()` through mp3rgain's
production analysis path and asserts it matches the printed value.

Result: mp3rgain reproduces the reference `GetTitleGain()` to the last ULP
(Δ ≈ 4e-17 dB), i.e. the analysis is bit-faithful and the ~0.05 dB end-to-end
gap is entirely decoder-side.

The PCM is integer-deterministic (fixed-seed LCG, no transcendentals), so the
golden value is platform-independent.
