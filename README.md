# mp3rgain

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-blue.svg)](https://www.rust-lang.org)
[![crates.io](https://img.shields.io/crates/v/mp3rgain.svg)](https://crates.io/crates/mp3rgain)
[![mp3gain compatible](https://img.shields.io/badge/mp3gain-compatible-brightgreen.svg)](docs/compatibility-report.md)

**Lossless MP3 volume adjustment - a modern mp3gain replacement written in Rust**

mp3rgain adjusts MP3 volume without re-encoding by modifying the `global_gain` field in each frame's side information. This preserves audio quality while achieving permanent volume changes.

## Features

- **Lossless & Reversible**: No re-encoding, all changes can be undone
- **ReplayGain**: Track and album gain analysis with AAC/M4A support
- **Zero dependencies**: Single static binary (no ffmpeg, no mp3gain)
- **Cross-platform**: macOS, Linux, Windows (x86_64 and ARM64)
- **mp3gain compatible**: Drop-in replacement with identical CLI
- **GUI Application**: Native desktop app for drag-and-drop workflow

## Installation

### macOS

```bash
brew install M-Igashi/tap/mp3rgain
```

### Windows (recommended)

```powershell
winget install M-Igashi.mp3rgain
```

### Windows (alternative)

```powershell
scoop bucket add mp3rgain https://github.com/M-Igashi/scoop-bucket
scoop install mp3rgain
```

### Linux

```bash
# Debian/Ubuntu (.deb package from GitHub Releases)
# Download from: https://github.com/M-Igashi/mp3rgain/releases
sudo apt install ./mp3rgain_*_amd64.deb

# Arch Linux (AUR)
yay -S mp3rgain

# Nix/NixOS
nix profile install github:M-Igashi/mp3rgain
```

### Cargo

```bash
cargo install mp3rgain
```

### Manual Download

Download binaries from [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases).

## Quick Start

```bash
# Normalize a single track (ReplayGain)
mp3rgain -r song.mp3

# Normalize an album
mp3rgain -a *.mp3

# Manual gain adjustment (+3.0 dB)
mp3rgain -g 2 song.mp3

# Undo changes
mp3rgain -u song.mp3

# Show file info
mp3rgain song.mp3
```

## GUI Application

A native GUI application (`mp3rgui`) is available for users who prefer a graphical interface.

**Features:** Drag-and-drop, track/album analysis, one-click gain application, clipping warnings, progress indicators.

**Download:** [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases)
- `mp3rgui-macos-universal.tar.gz` (macOS)
- `mp3rgui-linux-x86_64.tar.gz` (Linux)
- `mp3rgui-windows-x86_64.zip` (Windows)

> **macOS users:** If you see "mp3rgui cannot be opened" warning, run:
> ```bash
> xattr -cr /path/to/mp3rgui
> ```

## Command-Line Options

| Option | Description |
|--------|-------------|
| `-r` | Apply Track gain (ReplayGain) |
| `-a` | Apply Album gain (ReplayGain) |
| `-g <i>` | Apply gain of i steps (1 step = 1.5 dB) |
| `-d <n>` | Modify target dB level (use with analysis) |
| `-u` | Undo gain changes |
| `-k` | Prevent clipping |
| `-R` | Process directories recursively |
| `-n` | Dry-run mode |
| `-o [fmt]` | Output format: `text`, `json`, `tsv` (default: tsv if no argument) |

Run `mp3rgain -h` for the full list of options.

## Integration

### beets

mp3rgain works as a drop-in replacement for mp3gain in the [beets](https://beets.io/) replaygain plugin:

```yaml
# config.yaml
replaygain:
  backend: command
  command: mp3rgain
```

## Documentation

- [Security](docs/security.md) - Security improvements over original mp3gain (CVE-2021-34085, CVE-2019-18359)
- [Compatibility Report](docs/compatibility-report.md) - Verification against original mp3gain
- [Technical Comparison](docs/COMPARISON.md) - Comparison with similar tools

## Why mp3rgain?

The original [mp3gain](http://mp3gain.sourceforge.net/) has been unmaintained since ~2015. mp3rgain is a modern, memory-safe replacement that works on current systems including Windows 11, macOS, and Linux.

## ReplayGain Algorithm

mp3rgain implements the **original ReplayGain 1.0 algorithm**, the same as the classic mp3gain/aacgain:

- Equal-loudness filter (Yule-Walker + Butterworth high-pass)
- RMS calculation in 50ms windows
- 95th percentile statistical analysis
- **89 dB reference level**

This is a deliberate choice to maintain full compatibility with the original mp3gain. Loudness values will differ from tools using EBU R128/LUFS-based analysis (such as foobar2000's ReplayGain scanner, loudgain, or ffmpeg's loudnorm filter), which use a -23 LUFS reference level.

## Library Usage

```rust
use mp3rgain::{apply_gain, analyze};
use std::path::Path;

let frames = apply_gain(Path::new("song.mp3"), 2)?;  // +3.0 dB
let info = analyze(Path::new("song.mp3"))?;
```

## Contributing

Contributions welcome! See [CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT License - see [LICENSE](LICENSE).

## See Also

- [Original mp3gain](http://mp3gain.sourceforge.net/)
- [headroom](https://github.com/M-Igashi/headroom) - DJ audio loudness optimizer
