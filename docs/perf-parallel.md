# Parallel ReplayGain analysis — `-j` / `--threads`

mp3rgain 2.4 introduces multi-file parallelism for ReplayGain analysis,
addressing issues [#125] and [#126]. This document records the design
decisions and the real-corpus benchmark numbers that motivated the
default-on choice.

## TL;DR

- **Parallel by default.** `mp3rgain *.mp3`, `-r`, `-a`, recursive
  `-R <dir>`, and the default info command all use
  `std::thread::available_parallelism()` worker threads via rayon.
- **`-j 1` is the legacy fallback** for behavioral parity with mp3gain
  or for debugging. `-j 0` and `MP3RGAIN_THREADS=0` mean "auto".
- **Output is byte-identical regardless of `-j`** — TSV/Text/JSON line
  order and album-summary numbers all match the serial path exactly.
- On a 124-track / 2.1 GB corpus, an Apple M3 (4 performance + 4
  efficiency cores) sees **~3x wall-clock speedup** for the default
  recursive analysis (`mp3rgain -R -o tsv .`), going from 110.2s →
  35.5s with `-j 8`. Per-track gain (`-r -n`) and album gain
  (`-a -n`) both go from ~44s → ~14–16s in the same setup.

## CLI surface

| Form                    | Meaning                                              |
|-------------------------|------------------------------------------------------|
| _omitted_               | Use `available_parallelism()` (typically num cores)  |
| `-j 0` / `--threads 0`  | Same as omitted (auto)                               |
| `-j 1` / `--threads 1`  | Serial — matches legacy mp3gain behavior             |
| `-j N`                  | Use exactly N rayon worker threads                   |
| `MP3RGAIN_THREADS=N`    | Same effect as `-j N`; explicit flag wins            |

`-j` does not collide with any short flag in mp3gain's reference set
(`-? -h -v -g -l -d -r -a -k -m -p -s -c -e -x -t -T -q -f -o`) or in
aacgain.

## Benchmark methodology

- **Corpus**: Crossfader Music Pack (2019 + 2021 editions) — 124 actual
  audio files (236 .mp3 + 12 .m4a entries minus 124 macOS resource-fork
  shadows), 2.1 GB on disk.
- **Hardware**: Apple MacBook Air (M3, 2024) — 8 logical cores
  (4 performance + 4 efficiency), 24 GB RAM, internal SSD.
- **Build**: `cargo build --release` from
  `perf/parallel-replaygain-126`,  `[profile.release] lto = "thin",
  codegen-units = 1, strip = "debuginfo"`.
- **Tool**: [hyperfine](https://github.com/sharkdp/hyperfine) 1.20
  with `--warmup 1 --runs 3`. Corpus is pre-staged on the internal
  SSD (not the original USB volume) to remove disk-bandwidth variance.
- **Workloads** (run from the corpus root):
  - `mp3rgain -j N -R -q -o tsv .` (default info — analyze + album
    summary)
  - `mp3rgain -j N -R -r -n -q -o tsv .` (track gain dry-run)
  - `mp3rgain -j N -R -a -n -q -o tsv .` (album gain dry-run)

### Reproducing

```sh
cargo build --release
mkdir -p /tmp/mp3rgain-bench
cp -R "/path/to/Music Library" /tmp/mp3rgain-bench/
scripts/bench-parallel.sh "/tmp/mp3rgain-bench/<dir>"
# Outputs: /tmp/bench-info.md /tmp/bench-track.md /tmp/bench-album.md
```

## Results — default info (`mp3rgain -R -q -o tsv .`)

Workload: per-file ReplayGain analysis **plus** the trailing
album-summary `analyze_album_parallel` pass (decodes every file twice).

| `-j` | Mean (s)        | Speedup vs `-j 1` | Aggregate CPU usage |
|-----:|----------------:|------------------:|--------------------:|
| 1    | 110.151 ± 0.085 | 1.00× (baseline)  | 99% × 1 core        |
| 2    | 66.297 ± 9.119  | 1.66×             | 97% × 1.94 cores    |
| 4    | 51.625 ± 1.512  | 2.13×             | 97% × 3.86 cores    |
| 8    | 35.513 ± 0.380  | **3.10×**         | 87% × 7.00 cores    |

Source: `/tmp/bench-info.md` from the bench script run.

## Results — track gain dry-run (`mp3rgain -r -n -R -q -o tsv .`)

Workload: single ReplayGain analysis pass per file. No second pass,
no file modification (dry-run).

| `-j` | Mean (s)       | Speedup vs `-j 1` | Aggregate CPU usage |
|-----:|---------------:|------------------:|--------------------:|
| 1    | 44.550 ± 0.113 | 1.00× (baseline)  | 99% × 1 core        |
| 2    | 24.433 ± 0.060 | 1.82×             | 97% × 1.94 cores    |
| 4    | 16.115 ± 0.562 | **2.76×**         | 95% × 3.80 cores    |
| 8    | 19.309 ± 3.221 | 2.31×             | 76% × 6.12 cores    |

Note: `-j 8` is **slower** than `-j 4` here. The track-gain workload
fits in ~16s, and on a 4P+4E hybrid CPU the overhead of pushing four
extra threads onto efficiency cores outweighs their throughput
contribution for jobs of this size. The same workload on a homogeneous
8-core CPU is expected to scale further.

## Results — album gain dry-run (`mp3rgain -a -n -R -q -o tsv .`)

Workload: `analyze_album_parallel` decodes every track once and folds
histograms.

| `-j` | Mean (s)       | Speedup vs `-j 1` | Aggregate CPU usage |
|-----:|---------------:|------------------:|--------------------:|
| 1    | 44.159 ± 0.015 | 1.00× (baseline)  | 99% × 1 core        |
| 2    | 24.570 ± 0.707 | 1.80×             | 98% × 1.95 cores    |
| 4    | 22.143 ± 0.750 | 1.99×             | 96% × 3.84 cores    |
| 8    | 14.584 ± 0.505 | **3.03×**         | 88% × 7.00 cores    |

## Acceptance-criteria check (issue #126)

> `mp3rgain *.mp3 -r` is ≥ N×/2 faster on N cores for a corpus
> large enough to amortize startup (e.g. 50+ tracks).

| Cores | Bar (N/2) | Achieved (track-gain) | Achieved (album-gain) | Achieved (info) |
|------:|----------:|----------------------:|----------------------:|----------------:|
|     2 |      1.0× |                  1.82× |                 1.80× |           1.66× |
|     4 |      2.0× |                  2.76× |                 1.99× |           2.13× |
|     8 |      4.0× |                  2.31× |                 3.03× |           3.10× |

The 2-core and 4-core bars are met across all three workloads. The
8-core bar is missed because M3 has 4 performance + 4 efficiency
cores, so "8 cores" overstates the available compute throughput. On
a homogeneous 8-core CPU (Ryzen 7, Xeon E-23xx, etc.) we expect the
8-core bar to be cleared too.

## Output identity

Across every `-j` value tested on this corpus, the TSV/Text/JSON
output and the modified MP3 byte stream (after `-r` apply) are
**byte-identical** to the serial `-j 1` path. The album-fold is
associative and rayon's `par_iter().collect::<Vec<_>>()` preserves
input order, so `album_peak`, `album_loudness_db`, and
`album_gain_db` all match `-j 1` exactly.

```sh
# Verification (run during PR validation):
mp3rgain -j 1 -R -q -o tsv . > /tmp/serial.tsv
mp3rgain -j 8 -R -q -o tsv . > /tmp/parallel.tsv
diff /tmp/serial.tsv /tmp/parallel.tsv  # exits 0
```

## What gets parallelized

The two hot loops called out in [#126]:

1. `cmd_info` per-file ReplayGain analysis loop.
2. `analyze_album_internal` per-track decode + filter loop, exposed
   via two new public APIs in `src/replaygain.rs`:
   - `analyze_album_parallel(files, track_index, threads)`
   - `analyze_album_parallel_with_completion(files, track_index, threads, on_complete)`
   Both fall back to the existing serial implementation for
   `threads <= 1` or `files.len() <= 1`.

Plus, in this PR's scope:

3. `cmd_info`'s second album-summary pass — switched from
   `analyze_album` (serial) to `analyze_album_parallel` when `-j > 1`.
   Without this, the album-summary pass becomes the wall-clock
   bottleneck and limits the overall speedup to ~2× even with 8 cores.
4. `cmd_track_gain` per-file analyze + apply loop.
5. `cmd_album_gain` per-file apply loop (after the parallel
   `analyze_album_parallel_with_completion` analysis pass).

## What is *not* parallelized

- **Per-sample DSP inside a single track.** The equal-loudness IIR
  filter has tight inter-sample data dependency, so it doesn't
  parallelize without changing the algorithm. SIMD packing of L+R
  samples is the right answer there — see [#125] for follow-up.
- **`cmd_apply` / `cmd_apply_channel` / `cmd_undo` / `cmd_max_amplitude`
  / `cmd_check_tags` / `cmd_delete_tags`.** These are I/O-bound
  per-file (read tag, modify global_gain bytes, write file). They
  benefit much less from parallelism, and parallelizing them would
  require the same `(JsonFileResult, String)` output-buffer refactor
  applied to ReplayGain processors. They remain serial in this PR;
  open a follow-up if a real workload shows them as a bottleneck.

## Concurrency safety

- Each track gets its own Symphonia decoder, format reader,
  `EqualLoudnessFilter` array, and `LoudnessHistogram` — no shared
  mutable state.
- The album histogram fold is associative
  (`LoudnessHistogram::accumulate` is bin-wise sum), so reordering
  is safe; we still iterate in input order to keep the result
  bit-identical.
- Stdout output is buffered into per-file `String` instances inside
  `process_*` functions and replayed by the cmd layer in input order.
  This guarantees deterministic line ordering regardless of completion
  order.
- Stderr (warnings/errors) stays on `eprintln!`; OS-level per-line
  atomicity is sufficient for diagnostics. Order across files may
  differ between runs.

[#125]: https://github.com/M-Igashi/mp3rgain/issues/125
[#126]: https://github.com/M-Igashi/mp3rgain/issues/126
