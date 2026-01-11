# mp3rgain Roadmap

## Current Status: v0.1.0

Core functionality complete:
- [x] MP3 frame parsing (MPEG 1/2/2.5 Layer III)
- [x] Global gain modification
- [x] ID3v2 tag preservation
- [x] VBR/CBR support
- [x] CLI interface (apply/info/undo commands)

## Short-term Goals

### v0.2.0 - Windows & Stability

- [ ] Windows 11 compatibility verification
- [ ] Windows ARM64 support
- [ ] Comprehensive test suite with real MP3 files
- [ ] Error handling improvements
- [ ] Homebrew official formula submission

### v0.3.0 - mp3gain Feature Parity

- [ ] ReplayGain analysis (track gain calculation)
- [ ] Album gain support
- [ ] `-r` (apply Track gain) flag compatibility
- [ ] `-a` (apply Album gain) flag compatibility
- [ ] `-c` (ignore clipping) flag
- [ ] `-p` (preserve original timestamp) flag

## Medium-term Goals

### v0.4.0 - Extended Features

- [ ] Batch processing improvements
- [ ] Progress bar for large files
- [ ] JSON output format
- [ ] Dry-run mode
- [ ] Undo log file (for exact restoration)

### v0.5.0 - Additional Formats (Optional)

- [ ] AAC/M4A support (like aacgain)
- [ ] FLAC support (optional)
- [ ] Ogg Vorbis support (optional)

## Long-term Goals

- [ ] GUI wrapper (optional)
- [ ] Library API stabilization
- [ ] Integration with music players/taggers
- [ ] Homebrew core inclusion

## Community Goals

- [ ] Reach 100 GitHub stars
- [ ] 5+ contributors
- [ ] Windows user base
- [ ] Package manager availability (beyond Homebrew)
  - [ ] Scoop (Windows)
  - [ ] winget (Windows)
  - [ ] AUR (Arch Linux)
  - [ ] Nix

---

## How to Contribute

See [CONTRIBUTING.md](../CONTRIBUTING.md) for details on how to get involved.
