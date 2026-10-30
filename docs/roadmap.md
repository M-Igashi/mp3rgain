# mp3rgain Roadmap

## Current status: v3.15.0 (2026-10-30)

mp3rgain is a ReplayGain tool in the mp3gain lineage. It analyses loudness, writes standard `REPLAYGAIN_*` tags, and can bake the correction into the bitstream losslessly. For MP3 it is a modern drop-in replacement for mp3gain. For AAC it is the only actively maintained command-line tool that does this: aacgain has been abandoned since about 2009, and foobar2000, the reference-grade ReplayGain suite, offers a comparable scalefactor-based AAC rewrite ("Apply ReplayGain to file content") but only as a Windows GUI with no undo.

What ships today:

- Lossless gain via `global_gain` for MP3 (MPEG-1/2/2.5 Layer III) and AAC-LC (M4A/MP4 and raw ADTS), with undo; HE-AAC is skipped
- ReplayGain 1.0 analysis by default (mp3gain-identical values); ReplayGain 2.0 (`--rg2`), EBU R128 (`--r128`), and true peak (`--true-peak`) as options
- mp3gain command-line compatibility, down to the stored tags; the differences are listed in [migrating-from-mp3gain.md](migrating-from-mp3gain.md)
- Tags-only mode, album grouping by directory, release tags, or depth, reuse of stored tags (`-s R`)
- Parallel processing across and within files, JSON and TSV output
- GUI (mp3rgui) for macOS, Windows, and Linux, in English, Brazilian Portuguese, German, Indonesian, Italian, Japanese, and Turkish
- A documented library API on crates.io and docs.rs
- Distribution through crates.io, Homebrew, winget, .deb with an apt repository, Ubuntu PPA (deprecated, updated until 2026-12-31), AUR (GUI), MacPorts, pkgx, Termux (TUR), a Nix flake, and a GHCR Docker image

## Open

- [ ] Renew the Developer ID certificate before 2027-02-01 (#355)
- [ ] Retire the Ubuntu PPA after 2026-12-31, and decide whether to delete the two Launchpad PPAs
- [ ] Upgrade mp3rgui's egui/eframe stack, which retires the quick-xml audit ignores in `ci.yml` and would let right-to-left and complex scripts be translated
- [ ] More mp3rgui translations, one `.po` file per language (#440); `pt_PT` is offered
- [ ] Homebrew core formula (#8); today it is in the `M-Igashi/tap` tap
- [ ] nixpkgs package (#314); the flake in this repo already works, see [packages/nix/README.md](../packages/nix/README.md)
- [ ] Official Debian archive package (#394, ITP [#1126519](https://bugs.debian.org/1126519)): `id3` needs packaging in `debcargo-conf`, `indicatif` 0.18 and `console` 0.16 are only in experimental, and the upload needs a sponsor
- [ ] Fedora/RPM package
- [ ] Flatpak package
- [ ] FLAC support
- [ ] Ogg Vorbis support
- [ ] Integration with music players and taggers

### Community goals

- [x] Reach 100 GitHub stars (130 on 2026-10-27)
- [x] 5+ contributors: @valipod, @cfgnunes, @dannyward630, @0anes0, and @Privato72 by v3.14.0
- [ ] Grow the Windows user base
- [x] Package availability in major package managers

## Unreleased

Nothing yet.

## Release history

One line per change. The [release notes](https://github.com/M-Igashi/mp3rgain/releases) and the linked issues have the details.

### v3.15.0 - mp3rgui in German & Indonesian, Lighter Tag Writes

The analysis and the applied gains are unchanged; an ID3v2 tag now keeps its padding, and a few outputs and mp3rgui rows that were wrong are fixed.

- mp3rgui translations: German (@M-Igashi and Claude Code, reviewed by @Rosenheim23) and Indonesian (@saimskywalker); the Japanese and Italian ones refined (#440, #460, #461, #462, #480)
- mp3rgui: an Apply that first rescans the files without stored tags applies to the rows selected at the click, not to whatever is selected when the rescan ends
- mp3rgui: Apply Channel Gain, and an Undo that rolls the channels back by different amounts, clear the row's numbers instead of shifting them as if both channels moved, which also fed the next Apply Track Gain a wrong gain; library: `read_undo_channel_steps` (#471)
- `-s d` leaves an untagged M4A alone, including one whose metadata ends in an empty `ilst`, and `-u` no longer leaves an empty `udta` behind
- `-s d` and mp3rgui's Delete stored tags find out from `moov` alone that an M4A has no tags to delete, instead of reading and rebuilding the whole file (#468)
- `-s c` shows an album range that `-a -s i` stored in ID3v2 under the default layout too
- `-g -o tsv` prints `-` for a `global_gain` range it did not scan, and the steps `-k` actually applied
- The M4A line of `-r` and `-a` shows the gain applied, the album gain under `-a`
- An ID3v2 write keeps the tag's padding and rewrites the tag in place when it fits, instead of moving the audio after it; a tag that grows gets 2 KB of padding (#467)
- A crafted `MP3GAIN_UNDO` can no longer overflow the undo arithmetic
- Library: without the `aac` feature, the `*_auto` functions refuse AAC instead of running the MP3 path over an M4A (#149)
- Library: `write_album_minmax_with_options` keeps each file's mtime itself, so the CLI and mp3rgui no longer wrap the call to restore it (#463)
- A ReplayGain re-run that needs no change syncs each file once, and the album range reuses the apply's scan
- mp3rgui: a capped dry run below zero reads "-1 steps (capped)" rather than "+-1", and manual and channel gain say so while they run (#466)
- mp3rgui: the CJK fallback font takes half the memory, and grouping albums by tags reads the files in parallel (#470)
- An MP3 apply or undo that writes APEv2 holds one copy of the file instead of two, and an M4A apply or undo two instead of three (#469)
- Cleanup from a whole-codebase review, with no other change in behaviour
- CI: the GitHub Actions bumped (#479)

### v3.14.0 - mp3rgui in Five Languages & mp3gain Tag Parity

mp3rgui is translated. On the command line the analysis and the applied gains are unchanged, the stored mp3gain values are written as mp3gain 1.6.2 writes them, and `-a -r` applies track gain.

- mp3rgui translations: one gettext `.po` file per language compiled into the binary, and a Language menu. Mechanism by @cfgnunes; Brazilian Portuguese (@cfgnunes), Italian (@Privato72), Japanese (@M-Igashi), and Turkish (@0anes0) (#422, #439, #443, #445, #448, #457)
- `MP3GAIN_MINMAX` and `MP3GAIN_ALBUM_MINMAX` are zero-padded to three digits, the form mp3gain reads (#432)
- `-l` leaves `MP3GAIN_MINMAX` alone (#433)
- `-l -w` wraps and records `W`, and `-u` honours it; fixed by @dannyward630 (#434, #438)
- The later of `-a` and `-r` selects the mode, so `-a -r` applies track gain (#435)
- `-a -p` keeps the modification time after the album range is written (#436)
- `-r` and `-a` record `MP3GAIN_MINMAX` on a file that needs no change (#437)
- `-s i` moves mp3gain values out of APEv2 into ID3v2, so an undo after mixing tag layouts restores the original (#449)
- `-a -s i` writes the album range to ID3v2; library: `write_album_minmax_with_layout` (#450)
- `-g` shifts a stored `MP3GAIN_MINMAX` and adds none (#451)
- mp3rgui's CJK file-name font follows the system language under a non-CJK UI language (#452)
- CI: the apt installs time out after 10 minutes (#453)

### v3.13.0 - mp3gain Parity: ReplayGain 1.0 Analysis, -d, -k & Silent Frames

ReplayGain 1.0 values match mp3gain 1.6.2 on 118 real MP3s, so re-analyzing can move a few tracks by a step. AAC and the `--rg2` / `--r128` modes are unchanged.

- MP3 is decoded untrimmed for RG1 and `-x`, keeping the LAME encoder delay and padding, as mp3gain does (#430)
- RG1 at the MPEG-2 and MPEG-2.5 rates: a swapped 16 kHz filter coefficient and a short RMS window, found and fixed by @cfgnunes (#412, #413)
- RG1 scales stereo MP3 by 32767 before the filters (#415), drops the partial last window, and refuses a track that fills none (#414, #423)
- `-d` is rounded together with the measured gain (#416)
- `-k` caps track gain at the 16-bit limit (#418) and album gain once for the whole album (#419)
- A `global_gain` of 0 is left as it is without `-w` (#417)
- The crates.io package includes the test fixtures and the man page, for distributions that run the tests (#394)

### v3.12.1 - Bitrate & CBR/VBR in mp3rgui, Termux Install

No CLI behaviour change.

- mp3rgui shows each file's bitrate and CBR/VBR; library: `read_bitrate` (#408)
- mp3rgain is in the Termux User Repository: `pkg install tur-repo`, then `pkg install mp3rgain` (#399)
- The README warns that crackling AAC files have to be undone with the version that gained them (#402)

### v3.12.0 - FDK-AAC Crackling Fix & HE-AAC Skipped

AAC encoded by FDK-AAC no longer crackles after a gain change, and HE-AAC is skipped. Files gained by v2.8.0 to v3.11.1 that crackle have to be undone with the version that gained them, before upgrading.

- AAC fill elements with an escape count of 0 parse, so FDK-AAC frames are no longer left at their original gain (#402)
- Unparsed AAC frames are reported: a warning in the CLI, "Done (N frames unparsed)" in mp3rgui (#402)
- HE-AAC (v1 and v2) is skipped like ALAC, since `global_gain` reaches only the AAC core (#403)
- mp3rgui shows why a file failed on hover over "Error" (#403)

### v3.11.1 - Static Linux Binaries & Version-free Download Names

No behaviour change.

- The Linux CLI binaries are static musl builds, also under version-free names for nautilus-scripts (cfgnunes/nautilus-scripts#150)
- mp3rgain is in pkgx (#395)
- The Ubuntu PPA is deprecated in favour of the apt repository and receives uploads until 2026-12-31
- apt.yml tests Ubuntu 26.04 and the mp3rgui install; the download stats leave out apt index traffic

### v3.11.0 - mp3gain Parity: Joint-stereo -l, Read-only Files & an apt Repository

`-l` on a mono or joint-stereo MP3 is an error, `-s d` leaves a file without gain tags alone, and a read-only file is refused.

- `-l` refuses mono and joint-stereo MP3s, and `-u` their channel undo records, as mp3gain does (#393)
- `-s d` leaves a file with no gain tags byte for byte as it was
- A read-only file is refused whenever a command would change it; library: `Error::ReadOnly` (#398)
- Every release is a signed apt repository for amd64 and arm64 (#394)
- Tests: a simple-stereo fixture carries the `-l` comparison with mp3gain
- mp3rgui's Stored RG tooltip names an M4A's atoms as they are written
- Cleanup from a whole-codebase review, with no other change in behaviour

### v3.10.0 - mp3gain Parity: -e, -g Tags & CRC Frames

`-e` alone no longer modifies files, `-g` updates stored ReplayGain tags, `-i <n>` other than 0 is analysis-only, and RG1 loudness is reported 24.18 dB higher (gains and tags unchanged).

- `-e` alone only analyzes, as in mp3gain (#378)
- `-g` shifts the stored `REPLAYGAIN_*` values and `MP3GAIN_ALBUM_MINMAX` (#377)
- CRC-protected MP3s keep valid CRCs, and their LAME Info frame is not gained (#374)
- `-s s` writes no tags on M4A either (#376)
- `-i <n>` other than 0 is analysis-only (#375)
- A file listed twice is processed once, and symlinks are written through; found by @cfgnunes (#370)
- RG1 loudness is reported on the 89 dB scale everywhere (#379)
- The PPA is built from the tagged commit, one PPA per package, and the GUI tarballs get `.sha256` files (#380, #381, #382)
- crates.io Trusted Publishing; secrets in GitHub environments; Dependabot and `cargo audit` cover `mp3rgui/`
- mp3rgui security updates (RUSTSEC-2026-0257, -0194, -0195) and 32 fewer crates without `all_loaders`

### v3.9.2 - M4A Tags Visible in Mp3tag & GUI Target in LUFS Modes

- M4A ReplayGain and undo tags are visible in Mp3tag (#363)
- mp3rgui shows its version in Help > About mp3rgui (#367)
- mp3rgui's Target is adjustable in RG 2.0 and R128 (#364)

### v3.9.1 - Rename Retry on Network Shares

- Applying gain on a Windows network share retries the rename on "Access is denied" (#358)
- `docs/use-cases.md` points DJs at Bake'n Deck

### v3.9.0 - Streaming Output & Notarized macOS GUI

- The AAC peak no longer depends on `-j` (#349)
- `-o tsv` and `-o text` write each unit as it finishes (#348)
- `docs/COMPARISON.md` explains why the AAC peak is higher than rsgain's and foobar2000's (#350)
- mp3rgui for macOS is signed and notarized (#354)

### v3.8.1 - Documented Public API

Documentation only, no behaviour change: a published version's docs.rs pages can only be corrected by a new release.

- Every public library item is documented, `#![warn(missing_docs)]` keeps it so, and each `Error` variant says when it occurs (#346)
- `cargo doc` has no warnings, and docs.rs builds with `all-features`
- `docs/design-decisions.md` records the shapes reviews keep re-proposing as defects
- The migration guide names the real AAC tag atoms

### v3.8.0 - Album Grouping and Parallel Analysis

- `--album-by=tag` groups albums by ALBUM / ALBUMARTIST (preferring `MUSICBRAINZ_ALBUMID`), so a release whose discs live in subfolders gets one album gain (#333, #336)
- `--album-depth N` groups by directory depth for untagged libraries, and `dir` mode reports a release split across sibling folders (#331, #339)
- mp3rgui exposes the same three album units as the CLI, and names on hover which album each row was grouped into (#338, #343, #344, #345)
- `-a --album-by=...` analyzes albums concurrently instead of draining the thread pool at every album boundary (#332, #335)
- The true-peak polyphase filter was restructured for a measured 2.1x, and a long track is now divided across workers so `-j` helps a single file (#334, #337, #340, #341, #342)

### v3.7.0 - Raw ADTS Support & Unadjustable-Format Skips

- Detect AAC audio in video MP4 files: codec detection skipped non-`soun` traks, so a video MP4 was processed as an MP3 and had bytes overwritten inside its H.264 payload (#327, #328)
- Raw ADTS `.aac` streams get full lossless bitstream gain adjustment and undo, with the tags in ID3v2 since a raw stream has no container for freeform atoms (#330)
- ALAC and DRM-protected M4P are reported as skipped rather than failed, so one such file no longer sets the exit code of a library scan (#330)
- The `global_gain` range in info / `-o tsv` is scanned per container, and prints `-` when it cannot be scanned instead of the (255, 0) accumulator seed (#329)

### v3.6.1 - Per-Directory Albums & CJK File Names

- `-a --per-directory` computes one album gain per folder, with an `albums` array in JSON output (#324, #326). Since v3.8.0 it is an alias for `--album-by=dir`
- `-o tsv` prints the ReplayGain float peak under `--rg2` / `--r128` (#323, #325)
- mp3rgui loads a system CJK font so Japanese, Chinese, and Korean file names render (#321, #322)

### v3.6.0 - TSV Everywhere & GUI Column Derivation

- `-o tsv` works with every command, and the `File` column carries the path as given (#318)
- Read-only commands exit 1 on an unreadable file (#318)
- mp3rgui derives its table columns from one applied-gain offset instead of shifting nine fields by hand (#317, #320)

### v3.5.0 - Tags-Only Mode & Tag-Writing Fixes

- `--tags-only`: write the absolute `REPLAYGAIN_*` values and leave every audio frame untouched, the way loudgain / rsgain work (#308, #313)
- Undo runs before `-s d` deletes the tags, and strips the stale `REPLAYGAIN_*` residuals it leaves behind (#305, #306, #311, #312)
- `-s i` on an `.m4a` refreshes the mp4 ReplayGain tags; the split layout writes before deleting; the GUI's undo shifts the columns the right way (#315)
- `MP3GAIN_ALBUM_MINMAX` skips AAC album members instead of scanning MP4 bytes for MP3 sync words (#307, #310)
- Temp-file operations retry on Windows sharing violations (#303, #304)
- mp3rgui: "Use stored tags", the GUI counterpart of `-s R` (#302, #309)

### v3.4.0 - Apply From Stored Tags

- `-s R`: reuse stored `REPLAYGAIN_*` values and rescan only the files that lack them (#298, #300)
- GUI installs a log-first panic hook writing `panic.log`, instead of the window vanishing silently (#297, #301)
- The Ubuntu PPA targets resolute (26.04 LTS) instead of questing (25.10)

### v3.3.0 - True Peak, Windows Installer & wgpu

- `--true-peak` for `--rg2` / `--r128`, using a polyphase FIR meter (49-tap Hann-windowed sinc)
- Windows GUI renders through wgpu (D3D12 / Vulkan / WARP), so it starts without an OpenGL driver; `MP3RGUI_RENDERER=glow` forces the old backend
- Windows Inno installer for mp3rgui: per-user, Start Menu entry, uninstaller, one file for x86_64 and ARM64
- Exact gain step constant `20*log10(2)/4`, so repeated runs no longer drift the ReplayGain tags (#291)

### v3.2.0 - Split Tag Layout (ReplayGain to ID3v2)

- `REPLAYGAIN_*` goes to ID3v2 `TXXX` by default, where players actually read it; `MP3GAIN_UNDO` / `MP3GAIN_MINMAX` stay in APEv2 for the mp3gain lineage
- `-s a` (everything in APEv2, mp3gain's layout) and `-s i` (everything in ID3v2) override the split
- Album gain at 0 steps no longer discards the per-track ReplayGain tags it just measured

### v3.1.1 - Windows Wildcard Expansion

- Expand `*` and `?` in the final path component on Windows, where cmd.exe and PowerShell hand patterns through unexpanded (#288)

### v3.1.0 - Algorithm Tagging & GUI Diagnostics

- Write `REPLAYGAIN_ALGORITHM` (ID3v2/APEv2/MP4) when analyzing in `--rg2` or `--r128` mode (#287)
- GUI explains startup failures (missing GPU/display drivers) instead of dumping the raw error (#285)
- Packaging metadata and documentation updates (#283, #284, #286)

### v3.0.0 - BS.1770 Loudness Modes & API Cleanup (Issue #269)

- ITU-R BS.1770-4 gated loudness engine, no new dependencies (#270, PR #273)
- Opt-in `--rg2` (ReplayGain 2.0, -18 LUFS) and `--r128` (EBU R128, -23 LUFS) CLI flags; RG1 stays the default with mp3gain-identical values (#271, PR #274)
- GUI analysis mode selector with LUFS display (#272, PR #275)
- Removed unused public API items (semver-major) (#266, PR #276)
- Post-review simplification pass over the v3.0 changes (PR #277)

### v2.8.1 - v2.11.0 - mp3gain Tag Parity & Atomic Writes

- v2.8.1: `rust-version = "1.85"` declared; mp3rgui is built and linted in CI on every platform (#199)
- v2.9.0: `MP3GAIN_UNDO` uses mp3gain's sign, so either tool can undo the other's gain; album mode writes `MP3GAIN_ALBUM_MINMAX`; `REPLAYGAIN_*` holds the residual after an apply (#210). CI cross-checks the ReplayGain analysis against mp3gain, and mp3rgui persists its settings
- v2.9.2: silent windows count in the ReplayGain histogram, and the analysis is golden-tested against the reference `gain_analysis.c` (#201, #217)
- v2.9.6: tag writes go through the same temp file and rename as gain writes (#246), so `-t` is always on; any per-file failure sets exit code 1 (#239)
- v2.10.0: the `analyze_album*` functions are consolidated into `analyze_album_with_options` (#258); `-s r` is a compatibility no-op with a notice (#257)

### v2.8.0 - Latent-Bug Fixes & Performance

- Lossless undo for channel-specific (`-l`) and wrap-mode (`-w`) gain (10d04a6)
- Harden MP4/AAC parsing against malformed/truncated files: validate box sizes and `stsz`/`stsc`/`stco`/`co64`; a zero-size `trak` no longer stalls the scan (9471354, #195)
- Default command analyzes each file once instead of twice (~2x faster) (7d90443)
- APE tag reads touch only the file tail; AAC applies walk the bitstream once; MP4 codec detection reads only the `moov` box (#192, #193)
- MP3 apply/undo on a single in-memory buffer: one read and one write per file (10d04a6)
- GUI: parallel Find Max Amplitude / Undo / Delete Tags and a virtualized file table for large libraries (#194)

### v2.7.3 - Packaging

- Move the Nix flake to the repo root and drop hash maintenance (#177 / #178, co-authored by @alinnow)

### v2.7.2 - GUI AAC & Refresh Fixes

- AAC Find Max Amplitude and negative prevent-clipping cap (#173 / #174)
- Refresh cached peak after Apply Gain so sequential applies keep preventing clipping (#172 / #175)
- Refresh row values after Undo (#171 / #176)

### v2.7.1 - GUI Parallelization & Table UX

- Parallelize GUI Track Analysis and Apply Gain via a worker pool (#158, #165, #169)
- Treat each folder as a separate album in Album Analysis (#159, #166)
- Refresh table values after Apply Gain without a rescan (#160, #164)
- Floor the prevent-clipping cap so it never overshoots headroom (#162, #163)
- Sortable table columns, selection-scoped actions, reveal in file manager (#161, #167, #168, #170)

### v2.7.0 - Apply Pipeline Refactor & GUI Feature Parity (Issue #153, closes #152)

- `mp3rgain::apply::apply_with_options`: one pipeline shared by CLI and GUI (Step 1-2, PR #154)
- GUI worker threads + mpsc progress channel + Cancel button (Step 3, closes #152, PR #154)
- GUI Options panel: Prevent clipping / Preserve mtime / Wrap / Use ID3v2 (Step 4, PR #154)
- GUI Step 5 menus (PR #155): Undo, Check Stored Tags (+ Stored RG table column), Delete Stored Tags (with confirm modal), Find Max Amplitude, Dry Run toggle, Apply Manual Gain, Apply Channel Gain
- `mp3rgain::apply::predict_apply`, the dry-run companion to `apply_with_options`
- Channel gain routed through the unified pipeline; CLI `processors/utils::write_id3v2_undo_after_apply` removed

### v2.6.x - Robustness, Decoder Upgrade & Bug Fixes

- v2.6.0: `--skip-errors` keeps album analysis (`-a`) going past unreadable files (#145)
- v2.6.1: upgrade Symphonia 0.6.0-alpha.2 → 0.6.0 stable (SIMD decode, redesigned audio primitives) (#146)
- v2.6.2: apply the `-d` dB modifier during `-a` / `-r` gain application (#147 / #148)
- v2.6.3: fix GUI corrupting M4A files when applying gain: the dispatcher now routes MP4 files to the AAC path instead of the MP3 sync-word scanner (#149 / #150)

### v2.5.0 - Performance & Lean Library Build

- Every per-file command (apply, channel apply, undo, check/delete tags, max amplitude) runs in parallel, not only the analysis (#134)
- Faster parallel pipelines: skip duplicate analyze passes; AAC apply and the `-t` pipeline avoid redundant file I/O (#135)
- Fix temp-file collisions when multiple workers apply gain in the same directory
- The binary requires the default features, so `cargo build --no-default-features` builds the library alone instead of failing

### v2.4.0 - Parallel ReplayGain Analysis (Issues #125, #126)

- `-j` / `--threads` flag + `MP3RGAIN_THREADS` env var
- Parallel album analysis (`analyze_album_parallel`, consolidated into `analyze_album_with_options` in v2.10.0)
- Auto-tune via `std::thread::available_parallelism`; `-j 1` reproduces the legacy serial path
- CLI structure refactor (`processors/` per-file work, `commands/` dispatchers)

### v2.2.0 - v2.3.1 - Ubuntu PPA, Docker & MacPorts

- v2.2.0: Ubuntu PPA, uploaded automatically after each release; weekly download stats workflow (#110)
- v2.2.2: separate PPAs for the CLI (`ppa:m-igashi/mp3rgain`) and the GUI (`ppa:m-igashi/mp3rgui`)
- v2.3.0: AAC parser fixes, bounds checks, and faster Huffman decoding (#120, #121)
- v2.3.1: official multi-arch Docker image on GHCR (#123): `ghcr.io/m-igashi/mp3rgain:{latest, vX.Y.Z, vX}`, `linux/amd64` + `linux/arm64` built natively per arch, `FROM scratch` with a static musl binary of about 2 MB
- v2.3.1: MacPorts Portfile; the port has been upstream since 2026-04-29 (#122)

### v2.1.0 - Linux ARM64 & GUI Debian Package

- Linux ARM64 build targets (CLI and GUI) using `ubuntu-24.04-arm` runner
- mp3rgui .deb package (amd64 and arm64)
- mp3rgain .deb package ARM64 support
- .deb test workflow for mp3rgui (removed with the CLI one in v2.2.1)

### v2.0.0 - AAC Lossless Gain & Breaking API Changes (Issues #64, #68)

- AAC lossless bitstream gain adjustment (`global_gain` modification)
- AAC undo support (iTunes freeform metadata tags)
- HE-AAC/SBR support (base layer gain adjustment)
- Multi-track detection and warnings
- Custom error types (`thiserror`) replacing `anyhow::Result`
- `MpegVersion` / `ChannelMode` changed from `String` to enum
- Private struct fields with accessor methods
- `GainOptions` builder pattern
- Submodule organization (`analysis`, `gain`, `ape`, `frame`)

### v1.7.0 - Library API Stabilization & AAC Parser (Issues #68, #64)

- Add `#[non_exhaustive]` to all public enums and structs
- Add missing standard trait implementations (`PartialEq`, `Eq`, `Hash`)
- Add `Display` trait implementations to all public types
- Add `Serialize` / `Deserialize` behind a `serde` feature flag
- Add `ApeTag::iter()` and `ApeTag::len()` methods
- Make `MpegVersion` / `ChannelMode` enums public with typed accessors on `Mp3Analysis`
- Add `MaxAmplitudeResult` struct and `find_max_amplitude_detailed()` function
- Add `Channel::other()` convenience method
- Remove unnecessary `pub` from `filter_coeffs` internal constants
- AAC bitstream parser for locating `global_gain` fields (Issue #64 Phase 1)
- AUR package for mp3rgui (GUI application)
- Automated AUR package publishing in release workflow

### v1.6.0 - MP4/M4A Hardening & GUI Fixes

- Hardened MP4/M4A metadata handling and file detection (#67)
- Atomic write (temp file + rename) for M4A tag updates
- ALAC file detection and proper rejection
- DRM-protected M4P file rejection
- Compatible brands list checking in ftyp box
- Improved ilst box management (empty container cleanup, NeedsIlst)
- Fixed chunk offset (stco/co64) updates with threshold-based adjustment
- Fixed GUI volume display to use 89 dB ReplayGain reference (#62)
- Code quality improvements (clippy, iterator patterns, helper extraction)

### v1.5.0 - Debian Packaging

- Man page (`docs/man/mp3rgain.1`)
- Improved .deb build in the release workflow (#60)
- .deb install test workflow on Debian 12/13 and Ubuntu 22.04/24.04 (#61; removed in v2.2.1)
- cargo-deb configuration (never used by a workflow; removed after v3.9.2)

### v1.4.0 - Bug Fixes & mp3gain Compatibility

- Improved max amplitude detection (#51)
- Fixed global_gain range handling (#52)
- Handle last frame before APE/ID3 tags (#54)
- Fixed M4A info display (#55)
- Improved ReplayGain analysis accuracy (#48)
- Corrected ReplayGain calculation ~90dB offset (#50)

### v0.1.0 - v1.3.0 - Foundations (January 2026)

- v0.1.0: MP3 `global_gain` adjustment, macOS universal binary
- v0.2.0: Windows x86_64 and ARM64 builds, Homebrew tap
- v0.3.0: mp3gain-style flags replace the original subcommands (`-p`, `-c`, and friends)
- v0.4.0: APEv2 undo tags
- v0.5.0: ReplayGain track and album analysis (`-r`, `-a`)
- v0.6.0: clipping prevention (`-k`), recursive directories (`-R`), JSON output, dry run, progress bar for batches
- v0.7.0: channel gain (`-l`)
- v0.8.0: the remaining mp3gain options (`-m`, `-e`, `-x`, `-w`, `-t`, `-f`, `-s`) and TSV output
- v0.9.0: AAC/M4A ReplayGain analysis and iTunes freeform tags
- v1.1.0: audio track selection in multi-track MP4 (`-i`)
- v1.1.1: winget and Scoop packages (Scoop dropped after v2.0.0), static CRT on Windows, mp3gain compatibility test in CI
- v1.2.0: GUI application (mp3rgui) and Linux packaging (AUR, Nix, Debian/Ubuntu .deb)
- v1.2.6: correct ReplayGain filter coefficients for 44.1 and 48 kHz
- v1.3.0: refactoring for maintainability

---

## How to Contribute

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details on how to get involved.
