# mp3rgain Roadmap

## Current status: v3.10.0 (2026-10-08)

mp3rgain is a ReplayGain tool in the mp3gain lineage. It analyses loudness, writes standard `REPLAYGAIN_*` tags, and can bake the correction into the bitstream losslessly. For MP3 it is a modern drop-in replacement for mp3gain. For AAC it is the only actively maintained command-line tool that does this: aacgain has been abandoned since about 2009, and foobar2000, the reference-grade ReplayGain suite, offers a comparable scalefactor-based AAC rewrite ("Apply ReplayGain to file content") but only as a Windows GUI with no undo.

What ships today:

- Lossless gain via `global_gain` for MP3 (MPEG-1/2/2.5 Layer III) and AAC (M4A/MP4 and raw ADTS), with undo
- ReplayGain 1.0 analysis by default (mp3gain-identical values); ReplayGain 2.0 (`--rg2`), EBU R128 (`--r128`) and true peak (`--true-peak`) as options
- mp3gain command-line compatibility; the differences are listed in [migrating-from-mp3gain.md](migrating-from-mp3gain.md)
- Tags-only mode, album grouping by directory, release tags or depth, reuse of stored tags (`-s R`)
- Parallel processing across and within files, JSON and TSV output
- GUI (mp3rgui) for macOS, Windows and Linux
- A documented library API on crates.io and docs.rs
- Distribution through crates.io, Homebrew, winget, .deb, Ubuntu PPA, AUR (GUI), MacPorts, a Nix flake and a GHCR Docker image

## Open

- [ ] Renew the Developer ID certificate before 2027-02-01 (#355)
- [ ] Upgrade mp3rgui's egui/eframe stack, which also retires the quick-xml audit ignores in `ci.yml`
- [ ] Homebrew core formula (#8); today it is in the `M-Igashi/tap` tap
- [ ] nixpkgs package (#314); the flake in this repo already works, see [packages/nix/README.md](../packages/nix/README.md)
- [ ] Official Debian repository (ITP submission) (#394)
- [ ] pkgx package (#395)
- [ ] Fedora/RPM package
- [ ] Flatpak package
- [ ] FLAC support
- [ ] Ogg Vorbis support
- [ ] Integration with music players and taggers

### Community goals

- [x] Reach 100 GitHub stars (121 on 2026-10-08)
- [ ] 5+ contributors
- [ ] Grow the Windows user base
- [x] Package availability in major package managers

## Unreleased

Behaviour changes: `-l` on a mono or joint-stereo MP3 is an error, and so is `-u` with different left and right undo values on such a file. `-s d` no longer rewrites a file that has no gain tags to delete. A read-only file is refused rather than replaced.

- [x] `-l` refuses joint-stereo MP3s with an error and leaves the file untouched, as mp3gain does, in the CLI and the GUI. A joint-stereo frame may code mid and side rather than left and right, so the change reached both output channels while the undo tag recorded one; mp3rgain only warned, and only in text output without `-q`. Every frame is checked, so a stream that switches modes partway is refused as a whole. A dry run (`-n`, and the GUI's Dry run) now reports the refusal instead of "would apply". `-u` refuses a channel undo record on a mono or joint-stereo file, which mp3gain writes when it refuses `-l` there without changing the audio. A joint-stereo file adjusted with `-l` by 3.10.0 or earlier cannot be undone either, since the tag cannot tell the two apart (#393)
- [x] Tests: `test_stereo.mp3` turned out to be joint stereo in every audio frame, so mp3gain refused `-l` on it and the compatibility script had been skipping its `-l` comparison on every fixture. A simple-stereo fixture, `test_simple_stereo.mp3`, now carries the channel tests and the `-l` comparison with mp3gain
- [x] `-s d` leaves a file with no gain tags byte for byte as it was, as mp3gain does, in the CLI and the GUI. It used to rewrite every file it was given: the ID3v2 tag of an MP3 or raw ADTS stream was re-encoded as ID3v2.4, which drops ID3v2.2 frames that have no v2.4 form and adds an empty header to a stream that had no tag, and an M4A was replaced by an identical copy. `-g` followed by `-u -s d` now restores the original bytes on a file without ID3v2 gain tags, as the migration guide says undo does
- [x] A read-only file is refused with an error and left as it was whenever a command would change it, as mp3gain does, in the CLI and the GUI. Gain applies (`-r`, `-a`, `-g`, `-u`) used to replace it through the temp file and keep its read-only mode, while tag-only writes (`-s d`, `--tags-only`) failed with a raw permission error on MP3 and raw ADTS and succeeded on M4A. The file's mode decides, so running as root changes nothing, and a command with nothing to write still succeeds. In album mode a member whose apply failed no longer takes part in `MP3GAIN_ALBUM_MINMAX`, and a failed `MP3GAIN_ALBUM_MINMAX` write is reported as an error on that file instead of being dropped with a zero exit status. Library: new `Error::ReadOnly`, and `write_album_minmax` returns the writes that failed (#398)
- [x] mp3rgui: the Stored RG tooltip names an M4A's undo and minmax atoms `mp3rgain_undo` and `mp3rgain_minmax`, as they are written and as `-s c` prints them. It showed `MP3GAIN_UNDO`, the MP3 key, whose value has the opposite sign
- [x] Cleanup from a whole-codebase review, with no other change in behaviour: shared helpers for the TSV album row, skip records, the JSON epilogue, MP4 ReplayGain tags and the APEv2 tag span; the GUI groups albums once per Apply Album Gain, removes rows in one pass and passes path-only jobs as `(row, path)`; an APEv2 tag replace allocates once instead of holding about three copies of the file

## Release history

### v3.10.0 - mp3gain Parity: -e, -g Tags & CRC Frames

Behaviour changes: `-e` on its own no longer modifies files, `-g` now updates stored ReplayGain tags, `-i <n>` other than 0 is analysis-only, and RG1 loudness is reported 24.18 dB higher than before (gains and tags are unchanged).

- [x] `-e` on its own only analyzes and prints no album summary, as in mp3gain. It used to apply track gain like `-r`. `-r -e`, `-a -e` and `-e --tags-only` keep their meaning, and `-e -g` / `-e -l` now apply the `-g` / `-l` gain (#378)
- [x] `-g` shifts the `REPLAYGAIN_*` values and `MP3GAIN_ALBUM_MINMAX` already in the file by the gain it applied, in every container, as mp3gain does. They used to be left describing the old audio, so a tag-aware player landed off by the `-g` amount. `-l` still leaves them alone, as mp3gain does (#377)
- [x] CRC-protected MP3s (`lame -p`) keep valid frame CRCs after a gain change, and their LAME Info frame is no longer gained as audio, which broke gapless playback in mpg123. Output is byte-identical to mp3gain's (#374)
- [x] `-s s` writes no undo or ReplayGain tags on M4A under `-r`/`-a`, as it already did on MP3 and raw ADTS (#376)
- [x] `-i <n>` other than 0 is analysis-only. On a multi-track MP4 it measured track N but applied the gain to track 0, which could push that track 30 dB into clipping without a warning. Combining it with an option that writes is now an error (#375)
- [x] A file listed more than once in one run is processed once, whatever `-j` says; in parallel one of the two changes was lost. Symlinks are written through to their target instead of being replaced by a modified copy, in the CLI and the GUI. Found by @cfgnunes (#370)
- [x] ReplayGain 1.0 loudness is reported on the 89 dB scale everywhere (loudness + gain = target), so a fresh analysis and a `-s R` run print the same value. Fresh RG1 values in the text output and `-o json` are 24.18 dB higher than before; for library users `loudness_db()` changes meaning in RG1 (#379)
- [x] The PPA is built from the tagged commit rather than whatever `master` held when the PPA run started (#380), `scripts/build-ppa.sh` uploads each package to its own PPA (#381), and the mp3rgui Linux tarballs get `.sha256` files like every other asset (#382)
- [x] crates.io publishing uses Trusted Publishing instead of a long-lived token
- [x] mp3rgui updates webbrowser to 1.2.4 (RUSTSEC-2026-0257) and wayland-scanner to 0.31.11, which brings quick-xml 0.41 (RUSTSEC-2026-0194 / 0195). A quick-xml 0.30 copy reached only through eframe 0.31's accessibility stack stays until the egui upgrade, with an audit ignore in `ci.yml`
- [x] mp3rgui no longer enables `egui_extras`' `all_loaders` feature, which dropped 32 crates (ureq and rustls among them) from its lockfile
- [x] CI: release and PPA secrets are scoped to GitHub environments, checkouts that do not push drop their credentials, the AUR host key is pinned, and Dependabot and `cargo audit` cover `mp3rgui/` as well. The download stats archive is split into one file per month
- [x] Removed the one-off `ppa-ftp-probe.yml` workflow and the `[package.metadata.deb]` cargo-deb section that no workflow used
- [x] Documentation corrected against the code

### v3.9.2 - M4A Tags Visible in Mp3tag & GUI Target in LUFS Modes

- [x] The ReplayGain and undo tags mp3rgain writes to M4A files are now visible in Mp3tag. Their `data` atom had the type indicator and locale swapped, so readers that check the type, Mp3tag among them, treated the values as binary and skipped them. Files tagged by older versions still read and undo normally, and processing them again rewrites the tags in the corrected form (#363)
- [x] mp3rgui shows its version under Help > About mp3rgui. Until now the version appeared only in the crash message, so a GUI user filing a bug could not say which release they were running. The bug report template now points GUI users there (#367)
- [x] mp3rgui's Target is adjustable in the RG 2.0 and R128 modes, which used to lock it at -18 / -23 LUFS. It is one setting shared by all modes as an offset from the reference, the same shift the CLI's `-d` gives with `--rg2` / `--r128`, so 95 dB in RG 1.0 shows as -12 LUFS in RG 2.0 (#364)

### v3.9.1 - Rename Retry on Network Shares

- [x] Applying gain on a Windows network share no longer fails at random with "Access is denied". Replacing the original with the finished temp file is refused while another process such as Windows Defender has the original open, and on a share Defender cannot take the oplock that makes it step aside locally. The rename now retries that error with the same short backoff as the sharing violations from #303. The original file was never damaged by the failure (#358)
- [x] The DJ section of `docs/use-cases.md` points at Bake'n Deck, formerly headroom, whose old install commands no longer worked

### v3.9.0 - Streaming Output & Notarized macOS GUI

- [x] The peak measured for an AAC file no longer depends on `-j`. Chunked analysis seeked each piece to its own start, and an AAC decode that starts at a different packet substitutes a different realisation of every noise-substituted band, so `REPLAYGAIN_TRACK_PEAK` moved by up to 0.06 with the thread count. AAC is now analyzed whole; MP3 chunking is unchanged (#349)
- [x] `-o tsv` and `-o text` write each unit as it finishes instead of holding everything until the run ends, so a program consuming the output can start parsing immediately. `-r` previously emitted nothing at all before the last file was done, and `-a --per-directory` stalled every finished album behind the slowest earlier one. Rows now appear in completion order; `-o json` and `-j 1` are unchanged (#348)
- [x] Documented why mp3rgain reports a higher AAC peak than rsgain and foobar2000. AAC decoders disagree about whether to clamp samples above full scale, mp3rgain reports the decoded signal as it is, and both tools are right about their own input. `docs/COMPARISON.md` carries the measurements (#350)
- [x] mp3rgui for macOS is signed with a Developer ID certificate and notarized, so it opens without a Gatekeeper warning from the DMG and from the Homebrew cask. Homebrew stopped installing casks that fail Gatekeeper on 2026-09-01 (#354)

### v3.8.1 - Documented Public API

Documentation only. No behaviour change: every command produces output identical to 3.8.0, and MP3, M4A and raw ADTS applies are byte-identical. It exists as a release because a published version's documentation on docs.rs is immutable, so the only way to correct 3.8.0's is to publish 3.8.1.

- [x] Every public library item is documented, and `#![warn(missing_docs)]` keeps it that way. 151 items reached docs.rs with nothing on them, on a crate whose README points at docs.rs as the API reference (#346)
- [x] The `Error` enum, 46 of those 151, now says *when* each variant is produced rather than restating its name, which is what a consumer matching on a `#[non_exhaustive]` enum needs (#346)
- [x] The crate-level module list had drifted four modules behind, `albummeta` shipped seven undocumented public fields with #345, and rustdoc emitted eight warnings including one genuinely broken link. `cargo doc` is now at zero warnings
- [x] docs.rs builds with `all-features`, so the `serde` impls on the public result types are documented
- [x] `docs/design-decisions.md` records the deliberate shapes a whole-codebase review keeps re-proposing as defects, each pointing at where it was decided
- [x] The AAC tag row in the migration guide named an atom and a namespace that were both wrong; it now lists the actual `com.apple.iTunes` atom names

### v3.8.0 - Album Grouping and Parallel Analysis

- [x] `--album-by=tag` groups albums by ALBUM / ALBUMARTIST (preferring `MUSICBRAINZ_ALBUMID`), so a release whose discs live in subfolders gets one album gain (#333, #336)
- [x] `--album-depth N` groups by directory depth for untagged libraries, and `dir` mode reports a release split across sibling folders (#331, #339)
- [x] mp3rgui exposes the same three album units as the CLI, and names on hover which album each row was grouped into (#338, #343, #344, #345)
- [x] `-a --album-by=...` analyzes albums concurrently instead of draining the thread pool at every album boundary (#332, #335)
- [x] The true-peak polyphase filter was restructured for a measured 2.1x, and a long track is now divided across workers so `-j` helps a single file (#334, #337, #340, #341, #342)

### v3.7.0 - Raw ADTS Support & Unadjustable-Format Skips

- [x] Detect AAC audio in video MP4 files: codec detection skipped non-`soun` traks, so a video MP4 was processed as an MP3 and had bytes overwritten inside its H.264 payload (#327, #328)
- [x] Raw ADTS `.aac` streams get full lossless bitstream gain adjustment and undo, with the tags in ID3v2 since a raw stream has no container for freeform atoms (#330)
- [x] ALAC and DRM-protected M4P are reported as skipped rather than failed, so one such file no longer sets the exit code of a library scan (#330)
- [x] The `global_gain` range in info / `-o tsv` is scanned per container, and prints `-` when it cannot be scanned instead of the (255, 0) accumulator seed (#329)

### v3.6.1 - Per-Directory Albums & CJK File Names

- [x] `-a --per-directory` computes one album gain per folder, with an `albums` array in JSON output (#324, #326). Since v3.8.0 it is an alias for `--album-by=dir`
- [x] `-o tsv` prints the ReplayGain float peak under `--rg2` / `--r128` (#323, #325)
- [x] mp3rgui loads a system CJK font so Japanese, Chinese and Korean file names render (#321, #322)

### v3.6.0 - TSV Everywhere & GUI Column Derivation

- [x] `-o tsv` works with every command, and the `File` column carries the path as given (#318)
- [x] Read-only commands exit 1 on an unreadable file (#318)
- [x] mp3rgui derives its table columns from one applied-gain offset instead of shifting nine fields by hand (#317, #320)

### v3.5.0 - Tags-Only Mode & Tag-Writing Fixes

- [x] `--tags-only`: write the absolute `REPLAYGAIN_*` values and leave every audio frame untouched, the way loudgain / rsgain work (#308, #313)
- [x] Undo runs before `-s d` deletes the tags, and strips the stale `REPLAYGAIN_*` residuals it leaves behind (#305, #306, #311, #312)
- [x] `-s i` on an `.m4a` refreshes the mp4 ReplayGain tags; the split layout writes before deleting; the GUI's undo shifts the columns the right way (#315)
- [x] `MP3GAIN_ALBUM_MINMAX` skips AAC album members instead of scanning MP4 bytes for MP3 sync words (#307, #310)
- [x] Temp-file operations retry on Windows sharing violations (#303, #304)
- [x] mp3rgui: "Use stored tags", the GUI counterpart of `-s R` (#302, #309)

### v3.4.0 - Apply From Stored Tags

- [x] `-s R`: reuse stored `REPLAYGAIN_*` values and rescan only the files that lack them (#298, #300)
- [x] GUI installs a log-first panic hook writing `panic.log`, instead of the window vanishing silently (#297, #301)
- [x] The Ubuntu PPA targets resolute (26.04 LTS) instead of questing (25.10)

### v3.3.0 - True Peak, Windows Installer & wgpu

- [x] `--true-peak` for `--rg2` / `--r128`, using a polyphase FIR meter (49-tap Hann-windowed sinc)
- [x] Windows GUI renders through wgpu (D3D12 / Vulkan / WARP), so it starts without an OpenGL driver; `MP3RGUI_RENDERER=glow` forces the old backend
- [x] Windows Inno installer for mp3rgui: per-user, Start Menu entry, uninstaller, one file for x86_64 and ARM64
- [x] Exact gain step constant `20*log10(2)/4`, so repeated runs no longer drift the ReplayGain tags (#291)

### v3.2.0 - Split Tag Layout (ReplayGain to ID3v2)

- [x] `REPLAYGAIN_*` goes to ID3v2 `TXXX` by default, where players actually read it; `MP3GAIN_UNDO` / `MP3GAIN_MINMAX` stay in APEv2 for the mp3gain lineage
- [x] `-s a` (everything in APEv2, mp3gain's layout) and `-s i` (everything in ID3v2) override the split
- [x] Album gain at 0 steps no longer discards the per-track ReplayGain tags it just measured

### v3.1.1 - Windows Wildcard Expansion

- [x] Expand `*` and `?` in the final path component on Windows, where cmd.exe and PowerShell hand patterns through unexpanded (#288)

### v3.1.0 - Algorithm Tagging & GUI Diagnostics

- [x] Write `REPLAYGAIN_ALGORITHM` (ID3v2/APEv2/MP4) when analyzing in `--rg2` or `--r128` mode (#287)
- [x] GUI explains startup failures (missing GPU/display drivers) instead of dumping the raw error (#285)
- [x] Packaging metadata and documentation updates (#283, #284, #286)

### v3.0.0 - BS.1770 Loudness Modes & API Cleanup (Issue #269)

- [x] ITU-R BS.1770-4 gated loudness engine, no new dependencies (#270, PR #273)
- [x] Opt-in `--rg2` (ReplayGain 2.0, -18 LUFS) and `--r128` (EBU R128, -23 LUFS) CLI flags; RG1 stays the default with mp3gain-identical values (#271, PR #274)
- [x] GUI analysis mode selector with LUFS display (#272, PR #275)
- [x] Removed unused public API items (semver-major) (#266, PR #276)
- [x] Post-review simplification pass over the v3.0 changes (PR #277)

### v2.8.1 - v2.11.0 - mp3gain Tag Parity & Atomic Writes

- [x] v2.8.1: `rust-version = "1.85"` declared; mp3rgui is built and linted in CI on every platform (#199)
- [x] v2.9.0: `MP3GAIN_UNDO` uses mp3gain's sign, so either tool can undo the other's gain; album mode writes `MP3GAIN_ALBUM_MINMAX`; `REPLAYGAIN_*` holds the residual after an apply (#210). CI cross-checks the ReplayGain analysis against mp3gain, and mp3rgui persists its settings
- [x] v2.9.2: silent windows count in the ReplayGain histogram, and the analysis is golden-tested against the reference `gain_analysis.c` (#201, #217)
- [x] v2.9.6: tag writes go through the same temp file and rename as gain writes (#246), so `-t` is always on; any per-file failure sets exit code 1 (#239)
- [x] v2.10.0: the `analyze_album*` functions are consolidated into `analyze_album_with_options` (#258); `-s r` is a compatibility no-op with a notice (#257)

### v2.8.0 - Latent-Bug Fixes & Performance

- [x] Lossless undo for channel-specific (`-l`) and wrap-mode (`-w`) gain (10d04a6)
- [x] Harden MP4/AAC parsing against malformed/truncated files: validate box sizes and `stsz`/`stsc`/`stco`/`co64`; a zero-size `trak` no longer stalls the scan (9471354, #195)
- [x] Default command analyzes each file once instead of twice (~2x faster) (7d90443)
- [x] APE tag reads touch only the file tail; AAC applies walk the bitstream once; MP4 codec detection reads only the `moov` box (#192, #193)
- [x] MP3 apply/undo on a single in-memory buffer: one read and one write per file (10d04a6)
- [x] GUI: parallel Find Max Amplitude / Undo / Delete Tags and a virtualized file table for large libraries (#194)

### v2.7.3 - Packaging

- [x] Move the Nix flake to the repo root and drop hash maintenance (#177 / #178, co-authored by @alinnow)

### v2.7.2 - GUI AAC & Refresh Fixes

- [x] AAC Find Max Amplitude and negative prevent-clipping cap (#173 / #174)
- [x] Refresh cached peak after Apply Gain so sequential applies keep preventing clipping (#172 / #175)
- [x] Refresh row values after Undo (#171 / #176)

### v2.7.1 - GUI Parallelization & Table UX

- [x] Parallelize GUI Track Analysis and Apply Gain via a worker pool (#158, #165, #169)
- [x] Treat each folder as a separate album in Album Analysis (#159, #166)
- [x] Refresh table values after Apply Gain without a rescan (#160, #164)
- [x] Floor the prevent-clipping cap so it never overshoots headroom (#162, #163)
- [x] Sortable table columns, selection-scoped actions, reveal in file manager (#161, #167, #168, #170)

### v2.7.0 - Apply Pipeline Refactor & GUI Feature Parity (Issue #153, closes #152)

- [x] `mp3rgain::apply::apply_with_options`: one pipeline shared by CLI and GUI (Step 1-2, PR #154)
- [x] GUI worker threads + mpsc progress channel + Cancel button (Step 3, closes #152, PR #154)
- [x] GUI Options panel: Prevent clipping / Preserve mtime / Wrap / Use ID3v2 (Step 4, PR #154)
- [x] GUI Step 5 menus (PR #155): Undo, Check Stored Tags (+ Stored RG table column), Delete Stored Tags (with confirm modal), Find Max Amplitude, Dry Run toggle, Apply Manual Gain, Apply Channel Gain
- [x] `mp3rgain::apply::predict_apply`, the dry-run companion to `apply_with_options`
- [x] Channel gain routed through the unified pipeline; CLI `processors/utils::write_id3v2_undo_after_apply` removed

### v2.6.x - Robustness, Decoder Upgrade & Bug Fixes

- [x] v2.6.0: `--skip-errors` keeps album analysis (`-a`) going past unreadable files (#145)
- [x] v2.6.1: upgrade Symphonia 0.6.0-alpha.2 → 0.6.0 stable (SIMD decode, redesigned audio primitives) (#146)
- [x] v2.6.2: apply the `-d` dB modifier during `-a` / `-r` gain application (#147 / #148)
- [x] v2.6.3: fix GUI corrupting M4A files when applying gain: the dispatcher now routes MP4 files to the AAC path instead of the MP3 sync-word scanner (#149 / #150)

### v2.5.0 - Performance & Lean Library Build

- [x] Every per-file command (apply, channel apply, undo, check/delete tags, max amplitude) runs in parallel, not only the analysis (#134)
- [x] Faster parallel pipelines: skip duplicate analyze passes; AAC apply and the `-t` pipeline avoid redundant file I/O (#135)
- [x] Fix temp-file collisions when multiple workers apply gain in the same directory
- [x] The binary requires the default features, so `cargo build --no-default-features` builds the library alone instead of failing

### v2.4.0 - Parallel ReplayGain Analysis (Issues #125, #126)

- [x] `-j` / `--threads` flag + `MP3RGAIN_THREADS` env var
- [x] Parallel album analysis (`analyze_album_parallel`, consolidated into `analyze_album_with_options` in v2.10.0)
- [x] Auto-tune via `std::thread::available_parallelism`; `-j 1` reproduces the legacy serial path
- [x] CLI structure refactor (`processors/` per-file work, `commands/` dispatchers)

### v2.2.0 - v2.3.1 - Ubuntu PPA, Docker & MacPorts

- [x] v2.2.0: Ubuntu PPA, uploaded automatically after each release; weekly download stats workflow (#110)
- [x] v2.2.2: separate PPAs for the CLI (`ppa:m-igashi/mp3rgain`) and the GUI (`ppa:m-igashi/mp3rgui`)
- [x] v2.3.0: AAC parser fixes, bounds checks and faster Huffman decoding (#120, #121)
- [x] v2.3.1: official multi-arch Docker image on GHCR (#123): `ghcr.io/m-igashi/mp3rgain:{latest, vX.Y.Z, vX}`, `linux/amd64` + `linux/arm64` built natively per arch, `FROM scratch` with a static musl binary of about 2 MB
- [x] v2.3.1: MacPorts Portfile; the port has been upstream since 2026-04-29 (#122)

### v2.1.0 - Linux ARM64 & GUI Debian Package

- [x] Linux ARM64 build targets (CLI and GUI) using `ubuntu-24.04-arm` runner
- [x] mp3rgui .deb package (amd64 and arm64)
- [x] mp3rgain .deb package ARM64 support
- [x] .deb test workflow for mp3rgui (removed with the CLI one in v2.2.1)

### v2.0.0 - AAC Lossless Gain & Breaking API Changes (Issues #64, #68)

- [x] AAC lossless bitstream gain adjustment (`global_gain` modification)
- [x] AAC undo support (iTunes freeform metadata tags)
- [x] HE-AAC/SBR support (base layer gain adjustment)
- [x] Multi-track detection and warnings
- [x] Custom error types (`thiserror`) replacing `anyhow::Result`
- [x] `MpegVersion` / `ChannelMode` changed from `String` to enum
- [x] Private struct fields with accessor methods
- [x] `GainOptions` builder pattern
- [x] Submodule organization (`analysis`, `gain`, `ape`, `frame`)

### v1.7.0 - Library API Stabilization & AAC Parser (Issues #68, #64)

- [x] Add `#[non_exhaustive]` to all public enums and structs
- [x] Add missing standard trait implementations (`PartialEq`, `Eq`, `Hash`)
- [x] Add `Display` trait implementations to all public types
- [x] Add `Serialize` / `Deserialize` behind a `serde` feature flag
- [x] Add `ApeTag::iter()` and `ApeTag::len()` methods
- [x] Make `MpegVersion` / `ChannelMode` enums public with typed accessors on `Mp3Analysis`
- [x] Add `MaxAmplitudeResult` struct and `find_max_amplitude_detailed()` function
- [x] Add `Channel::other()` convenience method
- [x] Remove unnecessary `pub` from `filter_coeffs` internal constants
- [x] AAC bitstream parser for locating `global_gain` fields (Issue #64 Phase 1)
- [x] AUR package for mp3rgui (GUI application)
- [x] Automated AUR package publishing in release workflow

### v1.6.0 - MP4/M4A Hardening & GUI Fixes

- [x] Hardened MP4/M4A metadata handling and file detection (#67)
- [x] Atomic write (temp file + rename) for M4A tag updates
- [x] ALAC file detection and proper rejection
- [x] DRM-protected M4P file rejection
- [x] Compatible brands list checking in ftyp box
- [x] Improved ilst box management (empty container cleanup, NeedsIlst)
- [x] Fixed chunk offset (stco/co64) updates with threshold-based adjustment
- [x] Fixed GUI volume display to use 89 dB ReplayGain reference (#62)
- [x] Code quality improvements (clippy, iterator patterns, helper extraction)

### v1.5.0 - Debian Packaging

- [x] Man page (`docs/man/mp3rgain.1`)
- [x] Improved .deb build in the release workflow (#60)
- [x] .deb install test workflow on Debian 12/13 and Ubuntu 22.04/24.04 (#61; removed in v2.2.1)
- [x] cargo-deb configuration (never used by a workflow; removed after v3.9.2)

### v1.4.0 - Bug Fixes & mp3gain Compatibility

- [x] Improved max amplitude detection (#51)
- [x] Fixed global_gain range handling (#52)
- [x] Handle last frame before APE/ID3 tags (#54)
- [x] Fixed M4A info display (#55)
- [x] Improved ReplayGain analysis accuracy (#48)
- [x] Corrected ReplayGain calculation ~90dB offset (#50)

### v0.1.0 - v1.3.0 - Foundations (January 2026)

- [x] v0.1.0: MP3 `global_gain` adjustment, macOS universal binary
- [x] v0.2.0: Windows x86_64 and ARM64 builds, Homebrew tap
- [x] v0.3.0: mp3gain-style flags replace the original subcommands (`-p`, `-c` and friends)
- [x] v0.4.0: APEv2 undo tags
- [x] v0.5.0: ReplayGain track and album analysis (`-r`, `-a`)
- [x] v0.6.0: clipping prevention (`-k`), recursive directories (`-R`), JSON output, dry run, progress bar for batches
- [x] v0.7.0: channel gain (`-l`)
- [x] v0.8.0: the remaining mp3gain options (`-m`, `-e`, `-x`, `-w`, `-t`, `-f`, `-s`) and TSV output
- [x] v0.9.0: AAC/M4A ReplayGain analysis and iTunes freeform tags
- [x] v1.1.0: audio track selection in multi-track MP4 (`-i`)
- [x] v1.1.1: winget and Scoop packages (Scoop dropped after v2.0.0), static CRT on Windows, mp3gain compatibility test in CI
- [x] v1.2.0: GUI application (mp3rgui) and Linux packaging (AUR, Nix, Debian/Ubuntu .deb)
- [x] v1.2.6: correct ReplayGain filter coefficients for 44.1 and 48 kHz
- [x] v1.3.0: refactoring for maintainability

---

## How to Contribute

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details on how to get involved.
