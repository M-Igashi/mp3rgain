# mp3rgain Roadmap

## Current Status: v2.3.0 (Production Ready)

**Positioning:** mp3rgain is the only actively maintained tool that performs lossless `global_gain` rewrite on AAC/M4A files (aacgain has been abandoned since ~2009). For MP3 it's a modern drop-in replacement for mp3gain; for AAC it has no equivalent.

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

## Upcoming Goals

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
