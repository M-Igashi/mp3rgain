# mp3rgain

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-blue.svg)](https://www.rust-lang.org)
[![crates.io](https://img.shields.io/crates/v/mp3rgain.svg)](https://crates.io/crates/mp3rgain)
[![GitHub Downloads](https://img.shields.io/github/downloads/M-Igashi/mp3rgain/total?label=downloads&color=brightgreen)](https://m-igashi.github.io/mp3rgain/)
[![mp3gain compatible](https://img.shields.io/badge/mp3gain-compatible-brightgreen.svg)](docs/compatibility-report.md)

**Lossless MP3/AAC volume adjustment - a modern mp3gain replacement written in Rust**

mp3rgain adjusts MP3 and AAC volume without re-encoding by modifying the `global_gain` field in each frame. This preserves audio quality while achieving permanent volume changes.

## Features

- **Lossless & Reversible**: No re-encoding, all changes can be undone (MP3 and AAC)
- **ReplayGain**: Track and album gain analysis for MP3 and AAC/M4A
- **Zero dependencies**: Single static binary (no ffmpeg, no mp3gain)
- **Cross-platform**: macOS, Linux, Windows (x86_64 and ARM64)
- **mp3gain compatible**: Drop-in replacement with identical CLI
- **GUI Application**: Native desktop app for drag-and-drop workflow

## Installation

### CLI (`mp3rgain`)

| Platform | Command |
|----------|---------|
| macOS | `brew install M-Igashi/tap/mp3rgain` |
| Windows | `winget install M-Igashi.mp3rgain` |
| Arch Linux (AUR) | `yay -S mp3rgain-bin` |
| Ubuntu (PPA) | `sudo add-apt-repository ppa:m-igashi/mp3rgain && sudo apt install mp3rgain` |
| Debian | `sudo apt install ./mp3rgain_*_amd64.deb` ([download](https://github.com/M-Igashi/mp3rgain/releases)) (ARM64 also available) |
| Nix/NixOS | `nix profile install github:M-Igashi/mp3rgain` |
| Cargo | `cargo install mp3rgain` |

### GUI (`mp3rgui`)

| Platform | Command |
|----------|---------|
| macOS | `brew install --cask M-Igashi/tap/mp3rgui` |
| Windows | `winget install M-Igashi.mp3rgui` |
| Arch Linux (AUR) | `yay -S mp3rgui` |
| Ubuntu (PPA) | `sudo add-apt-repository ppa:m-igashi/mp3rgain && sudo apt install mp3rgui` |
| Debian/Ubuntu | `sudo apt install ./mp3rgui_*_amd64.deb` ([download](https://github.com/M-Igashi/mp3rgain/releases)) (ARM64 also available, requires Ubuntu 24.04+ / Debian trixie+) |

Binaries for all platforms are also available from [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases).

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

**Install:** See [Installation](#installation) above for Homebrew, Winget, and AUR options. Binaries are also available from [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases):
- `mp3rgui-*-macos-universal.dmg` (macOS)
- `mp3rgui-*-linux-x86_64.tar.gz` / `mp3rgui-*-linux-arm64.tar.gz` (Linux)
- `mp3rgui-*-windows-x86_64.zip` / `mp3rgui-*-windows-arm64.zip` (Windows)
- `mp3rgui_*_amd64.deb` / `mp3rgui_*_arm64.deb` (Debian/Ubuntu)

> **macOS manual download:** If you see "mp3rgui cannot be opened" warning, run:
> ```bash
> xattr -cr /path/to/mp3rgui.app
> ```
> This is not needed when installing via Homebrew.

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

## Documentation

- [Roadmap](docs/roadmap.md) - Development plans and upcoming features
- [Security](docs/security.md) - Memory safety and CVE analysis
- [Compatibility Report](docs/compatibility-report.md) - Verification against original mp3gain
- [Technical Comparison](docs/COMPARISON.md) - Comparison with similar tools
- [Use Cases](docs/use-cases.md) - Integration examples (beets, headroom, etc.)
- [Download Stats](https://m-igashi.github.io/mp3rgain/) - Weekly download trends across all platforms

## Why mp3rgain?

The original [mp3gain](http://mp3gain.sourceforge.net/) has been unmaintained upstream since ~2015 (though distribution maintainers continue to apply security patches). mp3rgain is a modern, memory-safe replacement written in Rust.

mp3rgain implements the **ReplayGain 1.0 algorithm** (89 dB reference level) for full compatibility with the original mp3gain. Loudness values will differ from EBU R128/LUFS-based tools (foobar2000, loudgain, ffmpeg loudnorm).

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
