# Deliberate decisions that look like defects

Some of this codebase looks wrong until you know why it is that way. Duplicated functions that measurement says to keep, two gates that differ by one condition on purpose, a `round()` next to a `floor()`, tag values whose sign flips between containers.

This file is the index of those. Every entry names where the reasoning actually lives, because the reasoning is the part that matters and it is too long to restate here.

It exists because a whole-codebase review finds these reliably and proposes "fixing" all of them. That happened during the 3.8.0 review and one of the changes was reverted after reading the history. If you are here from such a review, check this list before acting on a finding.

## Duplication that is measured, not accidental

| What looks duplicated | Why it stays | Where |
|---|---|---|
| `append_interleaved` and `process_audio_buffer_bs1770` (`src/replaygain.rs`) apply the same conversions to the same buffer types | Routing the serial path through the interleaving one costs 5% on `-j 1`, the path kept for mp3gain parity. `pipelined_analysis_matches_the_single_threaded_path` enforces that the two agree | commit `836deee` |
| The album grouping loops in `src/commands/albumgroup.rs` and `mp3rgui/src/app.rs::group_albums` | What one album *is* (`ReleaseKey`, `AlbumLabel`, `repeated_position`) lives in `src/albummeta.rs` and is shared. The loops around it differ in shape: the CLI returns warning strings, the GUI returns row indices and a `merged_releases` flag. Only the semantics were worth centralising | PR #343, PR #345 |
| `src/aac.rs` and `src/adts.rs` both walk AAC frames | ADTS shares the `global_gain` parser, the bit-level writer, `AacAnalysis` and the sample-rate table. Only the framing differs, and that genuinely differs | commit `797f069` (#330) |

## Two things that differ by one condition, on purpose

**The two "is there spare parallelism" gates** in `src/replaygain.rs`. The decode pipeline turns on whenever the rayon pool has more than one thread; chunking a single track additionally requires the run to have fewer files than threads. The pipeline adds one thread per file being analyzed and a saturated run tolerates that; chunking adds a task per piece and a saturated run does not. Dividing an already-saturated pool measured 23% slower. See commit `836deee` and PR #342.

**`db_to_steps` rounds, the clipping cap floors.** `src/apply.rs::check_clipping` computes `(max_safe_db / GAIN_STEP_DB).floor()` rather than calling `db_to_steps`. Rounding would turn 0.8 dB of headroom into one 1.5 dB step and re-introduce the clipping it is preventing. Issues #162 and #173.

**The collision scan keys on artist and album strings, not `release_key`.** `src/commands/albumgroup.rs::split_release_warnings` asks which folders share a *title*; keying on `MUSICBRAINZ_ALBUMID` would put two discs of one release in different buckets and find nothing to report. The id is consulted afterwards by `looks_like_one_release`, to decide what a shared title means. Commit `836deee`.

## On-disk formats that cannot be unified

**The undo tag's sign convention differs by container.** MP3 (APEv2 / ID3v2 `MP3GAIN_UNDO`) stores the *undo delta*, the value to re-apply to restore the original, which is mp3gain's convention. AAC (MP4 freeform) stores the cumulative *applied* gain and negates it on undo. Both are load-bearing on-disk formats: changing either corrupts round-trips on files tagged by older versions or by mp3gain itself. The note on `src/ape.rs::format_undo_value` is the authority. Issue #210.

**The MP4 freeform atom names are lowercase**, unlike the uppercase APEv2 / ID3v2 keys: `mp3rgain_undo`, `mp3rgain_minmax`, `replaygain_track_gain`, in the `com.apple.iTunes` namespace. `src/mp4meta.rs` holds the constants. This has drifted twice, once in `-s c` output and once in `docs/migrating-from-mp3gain.md`; both were fixed in `31265df` by using the constants instead of literals.

## Bit-exactness constraints

RG1 is the mp3gain-compatible path and its values have to match bit for bit. That is why:

- RG1 is **never** divided across workers, whatever `-j` says. Its equal-loudness filter is a 10th-order Yule-Walker IIR that settles far more slowly than K-weighting's two biquads, and it is already decode-bound. A test asserts RG1 output is byte-identical between `-j 1` and `-j 8`. PR #342.
- **AAC is never divided either**, whatever the mode and whatever `-j` says, and the check in `plan_chunks` keys on the codec rather than the container so ALAC in an M4A is still divided. Chunking assumes a seeked decode reproduces the samples a linear decode would have produced there. AAC breaks that assumption: perceptual noise substitution synthesises a band's coefficients from a generator seeded once per decoder instance, so starting at a different packet substitutes a different realisation of the same band energy, indefinitely and not merely past the warm-up. The energy is preserved, so loudness moves by 1e-6 dB and the peak by 0.06. Every AAC decoder behaves this way, ffmpeg's included. Issue #349.
- The true-peak test asserts **exact** `f64` equality against a transcription of the previous loop, not a tolerance. A difference there means the filter itself moved, which is a separate decision needing its own discussion. PR #340.
- The analysis is golden-tested against the reference C `gain_analysis.c` on deterministic PCM, in `src/replaygain.rs` with the harness in `tests/reference/`. Issues #201 and #217.

Do not propose a change to these paths on the grounds that it is equivalent. If it were equivalent it would produce the same bytes, and the tests will tell you.

## Shapes that exist for memory, not for style

`run_album` in `src/commands/replaygain.rs` is long, and its nested `par_iter` over albums with an inner `par_iter` over files looks like something to flatten. The flat form was rejected: it holds one `LoudnessHistogram` (48 KB) or `BlockEnergies` per file for the whole run, which is 480 MB on a 10,000 file library. Nested, each album drops its per-track state the moment it folds, so live state tracks the albums in flight rather than the size of the library. PR #335.

## Product decisions that keep being re-proposed

**winget ships the portable zip, not the Inno installer**, even though the release carries both. `InstallerType: zip` + `NestedInstallerType: portable` is what existing users installed through, winget tracks portable installs in its own link directory, and the transition was never tested on a real machine. Continuity beats the Start Menu entry. See `.claude/rules/winget.md`.

**Two editions of one album in sibling folders are left alone** in `--album-by=dir`. They are already grouped correctly by directory, and telling that user to switch to `--album-by=tag` would merge them wrongly, which is what the tag-mode collision report exists to warn about. PR #339.

**`--album-depth` is not exposed in the GUI.** It exists because a CLI user cannot glob a library on Windows and cannot exceed `ARG_MAX` anywhere, and neither constraint reaches a GUI. Selecting rows and choosing **Single album** covers the case it would serve. PR #343.

## Where the reasoning lives

Commit messages in this repo carry the *why*, at length, and so do PR bodies. `836deee` and `56d2763` are whole-codebase review passes that each end with a list of what was examined and deliberately left alone. When something here looks arbitrary, `git log -S` on the line is usually faster than reasoning about it from scratch.
