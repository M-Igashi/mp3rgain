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

## Album-crossing parallelism for `-a --per-directory` (3.8, issue [#332])

Until 3.7.0, `-a --per-directory` walked the album groups in a sequential loop and parallelized only *within* an album. Every album boundary was a barrier: while the last and longest track of an album finished on one core, the rest of the pool sat idle, and with N albums in one invocation that tail was paid N times. skamp saw it from the outside on the Hydrogenaudio thread, without instrumentation: "with foobar2000, all CPU cores remain fully active until the very last *file* (not album, file). With mp3rgain, I see short drops of CPU as it is scanning albums one by one."

The fix nests the parallelism instead of flattening it. The outer loop over album groups became a `par_iter`, and the existing inner `par_iter` over an album's files is untouched, so rayon's work-stealing fills an album's tail with files from the next album. A flat `par_iter` over every file with grouping applied afterwards was rejected: it would hold one `LoudnessHistogram` (48 KB) or `BlockEnergies` per file for the whole run, which is 480 MB on a 10,000-file library. Nested, each album still drops its per-track state the moment it folds, so live state tracks the albums in flight rather than the size of the library.

### Benchmark

Synthesized corpus, one directory per album, 6 tracks each with deliberately ragged lengths (120/150/180/210/150/270 s) so every album has a long tail to drain; 44.1 kHz stereo MP3 at 320 kbps. MacBook Air (M3, 2024), 4 performance + 4 efficiency cores, fanless. Median of 3 runs, `-q -n`, warm cache.

| Corpus | Command | 3.7.0 | this change | one pooled album (no barrier) |
|---|---|---|---|---|
| 6 albums, 36 files, 1.8 h | `-a --per-directory --rg2 --true-peak` | 6.47 s | **5.03 s** (1.29x) | 4.78 s |
| 12 albums, 72 files, 3.6 h | `-a --per-directory --rg2 --true-peak` | 13.62 s | **11.25 s** (1.21x) | 10.92 s |
| 12 albums, 72 files, 3.6 h | `-a --per-directory` (RG1) | 4.12 s | **3.02 s** (1.36x) | 2.87 s |

The rightmost column is the floor: the same files analyzed as a single pooled album, which has no album boundary to stall on. The per-directory run now sits within 3 to 5% of it, so the barrier is gone rather than merely reduced.

`-j 1` is unaffected (18.56 s → 18.37 s on the 6-album corpus): with one worker thread there is nothing to overlap, so that path keeps the sequential loop, the per-album progress bars and the direct-to-stdout writes it always had.

### Memory

Peak RSS, same corpus, `--rg2 --true-peak`:

| Run | 6 albums / 36 files | 12 albums / 72 files |
|---|---|---|
| `-a --per-directory` (this change) | 6.5 MB | 6.4 MB |
| `-a` pooled into one album | 6.3 MB | 8.1 MB |

Per-directory stays flat as the library doubles; pooling does not. That is the property the nested form buys, and it is why the flat-scan alternative was not taken.

### Output identity

Albums finish out of order, so everything observable is re-ordered before it is shown. Album runs are collected by index, so the JSON `albums` array, the `files` array, the counters and the exit code are all built in group order after the fact. Each album's text is buffered and flushed only once every earlier album has been flushed, so a finished album still prints immediately unless an earlier one is outstanding. The album fold was already associative and folded in input order, which is what keeps the numbers bit-identical.

Verified on the 12-album corpus and on a 30-file apply, against the 3.7.0 binary:

```sh
# -o json, -o tsv, text stdout and text stderr all byte-identical
mp3rgain-3.7.0 -a --per-directory --rg2 --true-peak -n -R -o json . > base.json
mp3rgain-new   -a --per-directory --rg2 --true-peak -n -R -o json . > new.json
diff base.json new.json    # exits 0, and likewise for -j 1 vs -j 8

# a real (non dry-run) apply over 6 concurrent albums, MP3 + AAC
diff <(cd base && find . -type f | sort | xargs shasum) \
     <(cd new  && find . -type f | sort | xargs shasum)   # exits 0
```

One thing does move: per-file warnings (clipping, saturation) are emitted by the apply workers straight to stderr, as they already were inside a single album, so they are not grouped by album and their position relative to an album's buffered stderr lines can differ. The album-level lines are the ones that had to stay grouped, because "Failed to analyze album" names nothing on its own.

### Not in scope

The other half of [#332] is granularity: the smallest schedulable unit is still one whole file, so a 9-minute track cannot be split or stolen once a worker picks it up, and thread efficiency is down to 71% at 4 threads even with no album boundary anywhere. Splitting a file into chunks with overlap-warmup is a separate design problem, and [#334] (the true-peak inner loop, a measured 2.1x on 68% of the analysis cost) is a bigger and cheaper win that should land before either.

## Overlapping the decode with the analysis (3.8, issue [#337])

Every level of parallelism above was between files. The work unit was still one whole file, so a single track could never use more than one core: `mp3rgain -r --rg2 --true-peak long.mp3` took the same 0.94 s at `-j 1`, `-j 4` and `-j 8`, because there was nothing to hand to the other cores.

The bitstream decode cannot be parallelized, but the DSP behind it can be taken off the decode thread. With true peak on, the split for a 10 minute 44.1 kHz file is roughly 0.46 s of decode plus conversion and 0.59 s of analysis, so overlapping them takes the file from the sum of the two down to the larger of them. The analyzer still receives every frame exactly once and in order, so this is not an approximation: output is byte-identical.

Batching matters. Handing one 26 ms packet across the channel at a time costs more in wakeups and allocator traffic than the overlap buys back, and only reached 1.25x. Batching to roughly a second of audio, with the buffers travelling back for reuse, reaches 1.42x.

| Workload | before | after | |
|---|---|---|---|
| one 10 min file, `-r --rg2 --true-peak` | 1.05 s | **0.74 s** | 1.42x |
| 36 files / 3.6 h, `-a --rg2 --true-peak -j 4` | 6.33 s | 5.82 s | 1.09x |
| 36 files / 3.6 h, `-a --rg2 --true-peak -j 8` | 5.34 s | 5.23 s | 1.02x |
| one 10 min file, `-j 1` | 1.05 s | 1.05 s | unchanged by design |

`-j 1` is documented as the single-threaded legacy path, so the pipeline is disabled there: measured CPU time stays at 1.04 s of user time for 1.05 s of wall time, i.e. one core. Above `-j 1` the pipeline costs about 7% more CPU time for 30% less wall time on a single file.

### What this does not do

The work unit is still a whole file for the *decode*, so one file is now decode-bound rather than decode-plus-DSP bound. Going further means splitting the decode itself across workers, which needs container-level seeking: on MP3 that works (`n_frames` is reported and an accurate seek lands a known distance before the target, 1,249 samples in the case measured), but some M4A files report no frame count at all, so a correct implementation needs a fallback and a way to verify each chunk landed exactly where it expected. [#337] stays open for that.

## Dividing one file across workers (3.8, issue [#337])

Overlapping the decode with the analysis left a single file decode-bound. Going past that means splitting the decode, which needs container-level seeking.

A track is divided at 100 ms sub-block boundaries. Each piece seeks to its start minus a one-second warm-up region, decodes forward, runs the filters through the warm-up without accumulating, and then measures its own sub-block sums and peaks. The pieces' sums are concatenated in order and the 400 ms gating blocks are formed from the whole list afterwards, so a block straddling a piece boundary is neither lost nor counted twice.

| Workload | before | after | |
|---|---|---|---|
| one 10 min file, `-r --rg2 --true-peak -j 8` | 0.74 s | **0.26 s** | 2.8x |
| the same at `-j 4` | 0.73 s | **0.35 s** | 2.1x |
| the same at `-j 2` | 0.74 s | 0.58 s | 1.3x |
| the same at `-j 1` | 1.04 s | 1.06 s | unchanged by design |
| 36 files / 3.6 h, `-a --rg2 --true-peak -j 4` | 5.44 s | 5.38 s | unchanged, not divided |

Against the `-j 1` baseline of 1.04 s, a single file now runs 4.0x faster at `-j 8`, where before this change `-j` did nothing at all for one file.

### When a file is divided

Only when the run as a whole has fewer files than threads. A run with a file per thread already has every core busy, and dividing there costs the warm-up regions and the seeks for no gain: measured at 23% *slower* at `-j 4` on the 36-file corpus before this gate was added. The decision is made once for the whole run rather than per album, since six albums of six files saturate the pool just as thoroughly as one list of thirty-six.

Also only for `--rg2` and `--r128`. RG1's equal-loudness filter is a 10th-order Yule-Walker IIR that settles far more slowly than the two biquads of K-weighting, and RG1 is the mp3gain-compatible path where the values have to match bit for bit. RG1 without true peak is already decode-bound at about 880x realtime, so leaving it whole costs nothing.

### When a file is not divided

Every check that cannot be satisfied falls back to the whole-file pass, because the failure mode is a wrong loudness value written into someone's tags:

- No frame count. Some M4A reports none: a 96 kHz HE-AAC file here reports `n_frames = None`.
- A time base that is not 1/sample_rate, so there is no sample index to divide. The same HE-AAC file reports a 96 kHz base against a 48 kHz rate.
- A seek that overshoots the piece's first wanted sample, which would leave a hole in the measurement.
- A piece that produced fewer sub-blocks than the plan said it owed, which means the plan and the file disagree.

### Accuracy

Not bit-identical, unlike [#334] and [#341], and this is the one place in the analysis where that is true.

Each piece starts its filters from zero and converges during the warm-up. The K-weighting's slowest pole is the 38 Hz high-pass, radius about 0.995 at 44.1 kHz, so a second of warm-up leaves a state error around 1e-100 relative. That is far below f64 resolution, but it can still flip the last bit of the filter state, and that carries through to the reported loudness.

Measured worst case over MP3 and AAC corpora: **7.1e-15 dB**. MP3 was exact in most runs; AAC differed by one or two ulp. The tests assert agreement within 1e-9 dB, six orders of margin over the observed worst case, which would still catch a misaligned piece or a dropped block.

True peak has no tolerance to spend: the 49-tap FIR reaches back 48 samples, and the warm-up feeds the meter the real preceding samples, so its history at a piece boundary is exact. The peak is asserted byte-equal.

[#125]: https://github.com/M-Igashi/mp3rgain/issues/125
[#126]: https://github.com/M-Igashi/mp3rgain/issues/126

[#332]: https://github.com/M-Igashi/mp3rgain/issues/332
[#334]: https://github.com/M-Igashi/mp3rgain/issues/334
[#337]: https://github.com/M-Igashi/mp3rgain/issues/337
[#334]: https://github.com/M-Igashi/mp3rgain/issues/334
[#341]: https://github.com/M-Igashi/mp3rgain/pull/341
