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

**Text and TSV rows come out in completion order; JSON records do not.** `CompletionFlush` in `src/commands/utils.rs` writes each unit's block the moment that unit finishes, under one lock so the block cannot be split. It deliberately does *not* gate on input order. The ordered version it replaced made every finished album wait behind the slowest earlier one, which on a 16-album run held fourteen of them until near the end, and the whole point is that a program consuming `-o tsv` can start parsing early. The JSON records are still collected in input order, so `-o json` is byte-identical at every `-j`, and `-j 1` still emits in input order. Issue #348.

**The Xing/Info marker offset ignores the CRC; the `global_gain` offsets do not.** `src/frame.rs::is_xing_frame` looks at 4 + side info length while `calculate_gain_locations` starts the side info at `side_info_offset()`, which is 6 on a CRC-protected frame. Both are right: LAME writes the marker at 4 + side info length whether or not the frame has a CRC, and mpg123, ffmpeg and symphonia look for it there. Using `side_info_offset()` for the marker too, as the code did until 3.9.2, missed the Info frame of every `lame -p` file and gained it as audio. The CRC of every frame a gain write touches is recomputed from scratch, as mp3gain's `crcWriteHeader` does, so a frame whose CRC was already wrong comes out with a valid one, byte-identical to mp3gain's output. Issue #374.

**`db_to_steps` rounds, the clipping cap floors.** `src/apply.rs::check_clipping` computes `(max_safe_db / GAIN_STEP_DB).floor()` rather than calling `db_to_steps`. Rounding would turn 0.8 dB of headroom into one 1.5 dB step and re-introduce the clipping it is preventing. Issues #162 and #173.

**The collision scan keys on artist and album strings, not `release_key`.** `src/commands/albumgroup.rs::split_release_warnings` asks which folders share a *title*; keying on `MUSICBRAINZ_ALBUMID` would put two discs of one release in different buckets and find nothing to report. The id is consulted afterwards by `looks_like_one_release`, to decide what a shared title means. Commit `836deee`.

## On-disk formats that cannot be unified

**The undo tag's sign convention differs by container.** MP3 (APEv2 / ID3v2 `MP3GAIN_UNDO`) stores the *undo delta*, the value to re-apply to restore the original, which is mp3gain's convention. AAC (MP4 freeform) stores the cumulative *applied* gain and negates it on undo. Both are load-bearing on-disk formats: changing either corrupts round-trips on files tagged by older versions or by mp3gain itself. The note on `src/ape.rs::format_undo_value` is the authority. Issue #210.

**The MP4 freeform atom names are lowercase**, unlike the uppercase APEv2 / ID3v2 keys: `mp3rgain_undo`, `mp3rgain_minmax`, `replaygain_track_gain`, in the `com.apple.iTunes` namespace. `src/mp4meta.rs` holds the constants. This has drifted twice, once in `-s c` output and once in `docs/migrating-from-mp3gain.md`; both were fixed in `31265df` by using the constants instead of literals.

**The freeform reader ignores the `data` box's type indicator and locale.** Until #363 mp3rgain wrote the two swapped (type 0, locale 1), which Mp3tag reads as a non-text item and hides. Writes now use type 1 (UTF-8) and locale 0, and every ilst rebuild re-serializes mp3rgain's own items still in the swapped form. The reader must keep accepting both, or files tagged by older versions lose their undo information. `src/mp4meta.rs::repair_swapped_data_header`. Issue #363.

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

**winget ships the portable zip, not the Inno installer**, even though the release carries both. `InstallerType: zip` + `NestedInstallerType: portable` is what existing users installed through, winget tracks portable installs in its own link directory, and the transition was never tested on a real machine. Switching the GUI to the Inno installer would not be a clean upgrade for those existing portable installs, so the installer stays a direct-download release asset only. Continuity beats the Start Menu entry.

**Two editions of one album in sibling folders are left alone** in `--album-by=dir`. They are already grouped correctly by directory, and telling that user to switch to `--album-by=tag` would merge them wrongly, which is what the tag-mode collision report exists to warn about. PR #339.

**The peak tag reports the decoder's raw output, not a clamped one**, which is why mp3rgain reads higher than rsgain and foobar2000 on AAC that carries decoder overshoot. This is not a defect and was investigated to the bottom in issue #350. AAC decoders disagree about whether to clamp: symphonia and ffmpeg's native decoder pass samples above 1.0 through, Apple's AudioToolbox decoder hard-limits at exactly 1.0. On the file the reporter supplied, mp3rgain measures a true peak of 2.287768 where rsgain measures 1.307024, and 1.3215 is exactly what an ideal band-limited interpolation of the *clamped* signal gives. Both tools are right about their own input. Clamping would make the tag describe one decoder rather than the file, under-report the headroom a float playback path actually needs, and silently disagree with every peak mp3rgain has already written. Issue #350 carries the full evidence and the argument.

**A write splits hard links, and two hard-link names are two files.** Every audio write goes to a temp file renamed over the original (#227), which replaces the directory entry: the name given gets the new file and every other hard link keeps the old audio. That is the price of a write that never leaves a half-written file, and mp3gain 1.6.2's default pays it too. (The APEv2 tail rewrite of #252 works in place and so reaches every name, but it never touches audio.) Symlinks are different: `src/apply.rs::rename_target` resolves a link to its target, so the link stays a link. It resolves only a link, not every path, so an ordinary file keeps the exact rename it always had rather than a canonical form that on Windows is a `\\?\` or UNC path no SMB share (#358) has been tested with. `src/cli/parse_args.rs::dedup_files` processes a file once however often it is listed, unlike mp3gain, which applies it once per listing; in parallel the two jobs raced and one change was lost. It compares canonical paths, so a symlink and its target are one file, but two hard-link names are different paths and each ends up with its own adjusted copy. Deduplicating by inode would process one name and leave the other with the old audio, which is no better, and has no stable equivalent in `std` on Windows. Issue #370.

**`--album-depth` is not exposed in the GUI.** It exists because a CLI user cannot glob a library on Windows and cannot exceed `ARG_MAX` anywhere, and neither constraint reaches a GUI. Selecting rows and choosing **Single album** covers the case it would serve. PR #343.

## Where the reasoning lives

Commit messages in this repo carry the *why*, at length, and so do PR bodies. `836deee` and `56d2763` are whole-codebase review passes that each end with a list of what was examined and deliberately left alone. When something here looks arbitrary, `git log -S` on the line is usually faster than reasoning about it from scratch.
