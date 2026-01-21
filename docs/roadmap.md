# mp3rgain Roadmap

## Current Status: v1.5.0 (Production Ready)

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

## Upcoming Goals

### v1.6.0 - Distribution Expansion

- [ ] Official Debian repository (ITP submission)
- [ ] Homebrew core inclusion (currently in tap)
- [ ] Fedora/RPM package
- [ ] Flatpak package

### Future Enhancements

- [ ] GUI application (cross-platform)
- [ ] FLAC support
- [ ] Ogg Vorbis support
- [ ] Library API stabilization
- [ ] Integration with music players/taggers

## Community Goals

- [ ] Reach 100 GitHub stars
- [ ] 5+ contributors
- [ ] Grow Windows user base
- [x] Package availability in major package managers

---

## How to Contribute

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details on how to get involved.
