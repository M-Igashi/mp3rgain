# mp3rgain Roadmap

## Current Status: v1.7.0 (Production Ready)

All core functionality complete:
- [x] MP3 frame parsing (MPEG 1/2/2.5 Layer III)
- [x] Global gain modification
- [x] ID3v2 tag preservation
- [x] VBR/CBR support
- [x] CLI interface (apply/info/undo commands)
- [x] ReplayGain analysis (track and album gain)
- [x] AAC/M4A support (ReplayGain tags)
- [x] Full mp3gain command-line compatibility
- [x] Cross-platform support (Windows, macOS, Linux)
- [x] Stabilized library API with `#[non_exhaustive]`, `Display`, `serde` support

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

## Upcoming Goals

### v2.0.0 - AAC Lossless Gain & Breaking API Changes (Issues #64, #68)

**Phase 1: AAC gain application — bitstream write (Issue #64 Phase 2)**
- [ ] Bit-level write operation handling byte-boundary crossing
- [ ] Locate sample file offsets via `stco`/`co64` + `stsz` atoms
- [ ] In-place `global_gain` modification within MP4 container
- [ ] Saturating clamp to 0-255 range, skip `global_gain == 0` (silence)

**Phase 2: AAC undo support (Issue #64 Phase 3)**
- [ ] Save original `global_gain` values in iTunes freeform metadata tags
- [ ] Implement undo/restore from saved values

**Phase 3: AAC essential validation (Issue #64 Phase 4 — part 1)**
- [ ] Classify parse errors as fatal vs. non-fatal (skip SBR extension data etc.)
- [ ] Apply `global_gain` to HE-AAC base layer where possible
- [ ] Detect and reject DRM-protected files / non-AAC tracks
- [ ] Report when adjustment rounds to 0 steps
- [ ] Validate against aacgain output on test files

**Phase 4: AAC extended support (Issue #64 Phase 4 — part 2)**
- [ ] Multi-channel layout support beyond stereo (5.1 etc.)
- [ ] Multiple audio track handling (default to first, warn)

**Phase 5: Breaking API changes (Issue #68)**
- [ ] Replace `anyhow::Result` with custom error types (`thiserror`)
- [ ] Change `Mp3Analysis.mpeg_version` / `channel_mode` from `String` to enum
- [ ] Remove old `find_max_amplitude` tuple return
- [ ] Make struct fields private with accessor methods

**Phase 6: API polish (Issue #68 — lower priority)**
- [ ] Consolidate `apply_gain*` function variants into builder/options pattern
- [ ] Organize flat `lib.rs` exports into submodules

### Future Enhancements

- [ ] EBU R128 loudness analysis (`--r128` option, ITU-R BS.1770 / LUFS-based)
- [ ] ReplayGain 2.0 support (`--rg2` option, R128-based with -18 LUFS reference)
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
