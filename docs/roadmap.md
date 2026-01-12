# mp3rgain Roadmap

## Current Status: v1.0.0 (Production Ready)

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

## Upcoming Goals

### v1.2.0 - Package Manager Expansion

- [x] Scoop package (Windows)
- [x] winget package (Windows Package Manager)
- [ ] Homebrew core inclusion
- [ ] AUR package (Arch Linux)
- [ ] Nix package

### Future

- [ ] GUI wrapper (optional)
- [ ] FLAC support (optional)
- [ ] Ogg Vorbis support (optional)
- [ ] Library API stabilization
- [ ] Integration with music players/taggers

## Community Goals

- [ ] Reach 100 GitHub stars
- [ ] 5+ contributors
- [ ] Grow Windows user base
- [ ] Package availability in major package managers

---

## How to Contribute

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details on how to get involved.
