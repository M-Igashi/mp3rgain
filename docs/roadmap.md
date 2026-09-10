# mp3rgain Roadmap

## Current Status: v3.1.0 (Production Ready)

**Positioning:** mp3rgain is a ReplayGain tool in the mp3gain lineage — it analyses, writes standard `REPLAYGAIN_*` tags, and bakes the correction into the bitstream — and it is the only actively maintained CLI doing that for AAC/M4A (aacgain has been abandoned since ~2009; foobar2000 is the reference-grade ReplayGain suite and its "Apply ReplayGain to file content" offers a comparable scalefactor-based AAC rewrite, but it is Windows GUI only with no undo). For MP3 it's a modern drop-in replacement for mp3gain; for AAC on the command line it has no equivalent.

All core functionality complete:
- [x] MP3 frame parsing (MPEG 1/2/2.5 Layer III)
- [x] Global gain modification (MP3 and AAC)
- [x] ID3v2 tag preservation
- [x] VBR/CBR support
- [x] CLI interface (apply/info/undo commands)
- [x] ReplayGain analysis (track and album gain)
- [x] AAC/M4A lossless bitstream gain adjustment and undo
- [x] Full mp3gain command-line compatibility
- [x] Cross-platform support (Windows, macOS, Linux)
- [x] Stabilized library API with custom error types, builder pattern, submodules

## Completed Milestones

### v0.2.0 - Windows & Stability

- [x] Windows 11 compatibility verification
- [x] Windows ARM64 support
- [x] Comprehensive test suite with real MP3 files
- [x] Error handling improvements
- [x] Homebrew tap formula

### v0.3.0 - mp3gain Feature Parity

- [x] ReplayGain analysis (track gain calculation)
- [x] Album gain support
- [x] `-r` (apply Track gain) flag compatibility
- [x] `-a` (apply Album gain) flag compatibility
- [x] `-c` (ignore clipping) flag
- [x] `-p` (preserve original timestamp) flag

### v0.4.0 - Extended Features

- [x] Batch processing with recursive directory support
- [x] Progress bar for large files
- [x] JSON output format
- [x] Dry-run mode
- [x] TSV output format

### v1.0.0 - AAC/M4A Support

- [x] AAC/M4A ReplayGain analysis
- [x] iTunes freeform tag writing
- [x] Multi-track audio support (`-i` option)
- [x] Production-ready release

### v1.1.0 - Package Manager Expansion

- [x] Scoop package (Windows)
- [x] winget package (Windows Package Manager)
- [x] AUR package (Arch Linux)
- [x] Nix package
- [x] Debian/Ubuntu package (.deb)

### v1.2.0 - GUI Application & Bug Fixes

- [x] Native GUI application (mp3rgui) for macOS, Linux, Windows
- [x] Fix ReplayGain filter coefficients (v1.2.6)
- [x] Improved Debian package build

### v1.3.0 - Code Quality & Stability

- [x] Code refactoring for better maintainability
- [x] Documentation updates

### v1.4.0 - Bug Fixes & mp3gain Compatibility

- [x] Improved max amplitude detection (#51)
- [x] Fixed global_gain range handling (#52)
- [x] Handle last frame before APE/ID3 tags (#54)
- [x] Fixed M4A info display (#55)
- [x] Improved ReplayGain analysis accuracy (#48)
- [x] Corrected ReplayGain calculation ~90dB offset (#50)

### v1.5.0 - Debian Packaging

- [x] Man page (docs/man/mp3rgain.1)
- [x] cargo-deb configuration
- [x] .deb package build in release workflow
- [x] .deb package test workflow (Debian 12/13, Ubuntu 22.04/24.04)

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

### v2.1.0 - Linux ARM64 & GUI Debian Package

- [x] Linux ARM64 build targets (CLI and GUI) using `ubuntu-24.04-arm` runner
- [x] mp3rgui .deb package (amd64 and arm64)
- [x] mp3rgain .deb package ARM64 support
- [x] .deb test workflow for mp3rgui

### Distribution: Container Images

- [x] Official multi-arch Docker image on GHCR (Issue #123)
  - `ghcr.io/m-igashi/mp3rgain:{latest, vX.Y.Z, vX}`
  - `linux/amd64` + `linux/arm64`, native build per arch
  - `FROM scratch` + musl static binary, ~2 MB
  - Drop-in replacement for `mp3gain` containers in cron / Plex pipelines

### v2.4.0 - Parallel ReplayGain Analysis (Issues #125, #126)

- [x] `-j` / `--threads` flag + `MP3RGAIN_THREADS` env var
- [x] Parallel album analysis via `analyze_album_lenient_parallel_with_completion`
- [x] Auto-tune via `std::thread::available_parallelism`; `-j 1` reproduces the legacy serial path
- [x] CLI structure refactor (`processors/` per-file work, `commands/` dispatchers)

### v2.5.0 - Performance & Lean Library Build

- [x] Faster parallel pipelines: skip duplicate analyze passes; AAC apply and the `-t` pipeline avoid redundant file I/O (#135)
- [x] Fix temp-file collisions when multiple workers apply gain in the same directory
- [x] Default-features gate for the binary so library-only consumers no longer compile `clap`

### v2.6.x - Robustness, Decoder Upgrade & Bug Fixes

- [x] v2.6.0: `--skip-errors` keeps album analysis (`-a`) going past unreadable files (#145)
- [x] v2.6.1: upgrade Symphonia 0.6.0-alpha.2 → 0.6.0 stable (SIMD decode, redesigned audio primitives) (#146)
- [x] v2.6.2: apply the `-d` dB modifier during `-a` / `-r` gain application (#147 / #148)
- [x] v2.6.3: fix GUI corrupting M4A files when applying gain — the dispatcher now routes MP4 files to the AAC path instead of the MP3 sync-word scanner (#149 / #150)

### v2.7.0 - Apply Pipeline Refactor & GUI Feature Parity (Issue #153, closes #152)

- [x] `mp3rgain::apply::apply_with_options` — unified pipeline shared by CLI and GUI (Step 1-2, PR #154)
- [x] GUI worker threads + mpsc progress channel + Cancel button (Step 3, closes #152, PR #154)
- [x] GUI Options panel: Prevent clipping / Preserve mtime / Wrap / Use ID3v2 (Step 4, PR #154)
- [x] GUI Step 5 menus (PR #155):
  - Undo, Check Stored Tags (+ Stored RG table column), Delete Stored Tags (with confirm modal)
  - Find Max Amplitude, Dry Run toggle
  - Apply Manual Gain, Apply Channel Gain
- [x] `mp3rgain::apply::predict_apply` — dry-run companion to `apply_with_options`
- [x] Channel gain routed through the unified pipeline; CLI `processors/utils::write_id3v2_undo_after_apply` removed

### v2.7.1 - GUI Parallelization & Table UX

- [x] Parallelize GUI Track Analysis and Apply Gain via a worker pool (#158, #165, #169)
- [x] Treat each folder as a separate album in Album Analysis (#159, #166)
- [x] Refresh table values after Apply Gain without a rescan (#160, #164)
- [x] Floor the prevent-clipping cap so it never overshoots headroom (#162, #163)
- [x] Sortable table columns, selection-scoped actions, reveal in file manager (#161, #167, #168, #170)

### v2.7.2 - GUI AAC & Refresh Fixes

- [x] AAC Find Max Amplitude and negative prevent-clipping cap (#173 / #174)
- [x] Refresh cached peak after Apply Gain so sequential applies keep preventing clipping (#172 / #175)
- [x] Refresh row values after Undo (#171 / #176)

### v2.7.3 - Packaging

- [x] Move the Nix flake to the repo root and drop hash maintenance (#177 / #178, co-authored by @alinnow)

### v2.8.0 - Latent-Bug Fixes & Performance

- [x] Lossless undo for channel-specific (`-l`) and wrap-mode (`-w`) gain (10d04a6)
- [x] Harden MP4/AAC parsing against malformed/truncated files — validate box sizes and `stsz`/`stsc`/`stco`/`co64`; zero-size `trak` no longer stalls the scan (9471354, #195)
- [x] Default command analyzes each file once instead of twice (~2x faster) (7d90443)
- [x] APE tag reads touch only the file tail; AAC applies walk the bitstream once; MP4 codec detection reads only the `moov` box (#192, #193)
- [x] MP3 apply/undo on a single in-memory buffer — one read and one write per file (10d04a6)
- [x] GUI: parallel Find Max Amplitude / Undo / Delete Tags and a virtualized file table for large libraries (#194)

### v3.0.0 - BS.1770 Loudness Modes & API Cleanup (Issue #269)

- [x] ITU-R BS.1770-4 gated loudness engine, no new dependencies (#270, PR #273)
- [x] Opt-in `--rg2` (ReplayGain 2.0, -18 LUFS) and `--r128` (EBU R128, -23 LUFS) CLI flags;
      RG1 stays the default with mp3gain-identical values (#271, PR #274)
- [x] GUI analysis mode selector with LUFS display (#272, PR #275)
- [x] Removed unused public API items (semver-major) (#266, PR #276)
- [x] Post-review simplification pass over the v3.0 changes (PR #277)

### v3.1.0 - Algorithm Tagging & GUI Diagnostics

- [x] Write `REPLAYGAIN_ALGORITHM` (ID3v2/APEv2/MP4) when analyzing in `--rg2` or `--r128` mode (#287)
- [x] GUI explains startup failures (missing GPU/display drivers) instead of dumping the raw error (#285)
- [x] Packaging metadata and documentation updates (#283, #284, #286)

### v3.1.1 - Windows Wildcard Expansion

- [x] Expand `*` and `?` in the final path component on Windows, where cmd.exe and PowerShell hand patterns through unexpanded (#288)

### v3.2.0 - Split Tag Layout (ReplayGain to ID3v2)

- [x] `REPLAYGAIN_*` goes to ID3v2 `TXXX` by default, where players actually read it; `MP3GAIN_UNDO` / `MP3GAIN_MINMAX` stay in APEv2 for the mp3gain lineage
- [x] `-s a` (everything in APEv2, byte-for-byte mp3gain) and `-s i` (everything in ID3v2) override the split
- [x] Album gain at 0 steps no longer discards the per-track ReplayGain tags it just measured

### v3.3.0 - True Peak, Windows Installer & wgpu

- [x] `--true-peak` for `--rg2` / `--r128`, using a polyphase FIR meter (49-tap Hann-windowed sinc)
- [x] Windows GUI renders through wgpu (D3D12 / Vulkan / WARP), so it starts without an OpenGL driver; `MP3RGUI_RENDERER=glow` forces the old backend
- [x] Windows Inno installer for mp3rgui: per-user, Start Menu entry, uninstaller, one file for x86_64 and ARM64
- [x] Exact gain step constant `20*log10(2)/4`, so repeated runs no longer drift the ReplayGain tags (#291)

### v3.4.0 - Apply From Stored Tags

- [x] `-s R`: reuse stored `REPLAYGAIN_*` values and rescan only the files that lack them (#298, #300)
- [x] GUI installs a log-first panic hook writing `panic.log`, instead of the window vanishing silently (#297, #301)

### v3.5.0 - Tags-Only Mode & Tag-Writing Fixes

- [x] `--tags-only`: write the absolute `REPLAYGAIN_*` values and leave every audio frame untouched, the way loudgain / rsgain work (#308, #313)
- [x] Undo runs before `-s d` deletes the tags, and strips the stale `REPLAYGAIN_*` residuals it leaves behind (#305, #306, #311, #312)
- [x] `-s i` on an `.m4a` refreshes the mp4 ReplayGain tags; the split layout writes before deleting; the GUI's undo shifts the columns the right way (#315)
- [x] `MP3GAIN_ALBUM_MINMAX` skips AAC album members instead of scanning MP4 bytes for MP3 sync words (#307, #310)
- [x] Temp-file operations retry on Windows sharing violations (#303, #304)
- [x] mp3rgui: "Use stored tags", the GUI counterpart of `-s R` (#302, #309)

### v3.6.0 - TSV Everywhere & GUI Column Derivation

- [x] `-o tsv` works with every command, and the `File` column carries the path as given (#318)
- [x] Read-only commands exit 1 on an unreadable file (#318)
- [x] mp3rgui derives its table columns from one applied-gain offset instead of shifting nine fields by hand (#317, #320)

### v3.6.1 - Per-Directory Albums & CJK File Names

- [x] `-a --per-directory` computes one album gain per folder, with an `albums` array in JSON output (#324, #326)
- [x] `-o tsv` prints the ReplayGain float peak under `--rg2` / `--r128` (#323, #325)
- [x] mp3rgui loads a system CJK font so Japanese, Chinese and Korean file names render (#321, #322)

### v3.7.0 - Raw ADTS Support & Unadjustable-Format Skips

- [x] Detect AAC audio in video MP4 files: codec detection skipped non-`soun` traks, so a video MP4 was processed as an MP3 and had bytes overwritten inside its H.264 payload (#327, #328)
- [x] Raw ADTS `.aac` streams get full lossless bitstream gain adjustment and undo, with the tags in ID3v2 since a raw stream has no container for freeform atoms (#330)
- [x] ALAC and DRM-protected M4P are reported as skipped rather than failed, so one such file no longer sets the exit code of a library scan (#330)
- [x] The `global_gain` range in info / `-o tsv` is scanned per container, and prints `-` when it cannot be scanned instead of the (255, 0) accumulator seed (#329)

## Upcoming Goals

### Future Enhancements

- [ ] Official Debian repository (ITP submission)
- [ ] Homebrew core inclusion (currently in tap)
- [ ] Fedora/RPM package
- [ ] Flatpak package
- [ ] FLAC support
- [ ] Ogg Vorbis support
- [ ] Integration with music players/taggers

## Community Goals

- [ ] Reach 100 GitHub stars
- [ ] 5+ contributors
- [ ] Grow Windows user base
- [x] Package availability in major package managers

---

## How to Contribute

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details on how to get involved.
