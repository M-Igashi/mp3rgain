<p align="center">
  <img src="docs/branding/mp3rgain-banner.png" alt="mp3rgain — lossless MP3/AAC volume normalization" width="800">
</p>

# mp3rgain

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-blue.svg)](https://www.rust-lang.org)
[![crates.io](https://img.shields.io/crates/v/mp3rgain.svg)](https://crates.io/crates/mp3rgain)
[![GitHub Downloads](https://img.shields.io/github/downloads/M-Igashi/mp3rgain/total?label=downloads&color=brightgreen)](https://m-igashi.github.io/mp3rgain/)
[![mp3gain compatible](https://img.shields.io/badge/mp3gain-compatible-brightgreen.svg)](docs/compatibility-report.md)

**Lossless MP3/AAC volume adjustment - a modern mp3gain / aacgain replacement written in Rust**

🌐 **Website:** [mp3rgain.tyna.ninja](https://mp3rgain.tyna.ninja/)

mp3rgain adjusts MP3 and AAC volume without re-encoding by modifying the `global_gain` field in each frame. This preserves audio quality while achieving permanent volume changes.

> **The only actively maintained CLI for lossless AAC/M4A bitstream gain adjustment.**
> aacgain (the historic CLI) has been unmaintained since ~2009 and rarely builds on modern 64-bit systems. foobar2000 also offers re-encode-free AAC gain on MP4/MKA via its "Apply ReplayGain to file content" feature (Windows GUI only, no undo). mp3rgain is the actively maintained, cross-platform, scriptable, undoable option for headless / batch / CI use.

## Features

- **CLI lossless AAC bitstream gain**: re-encode-free `global_gain` rewrite for AAC/M4A — replacing the long-abandoned aacgain, with `-u` undo (foobar2000's GUI equivalent has no undo path)
- **Lossless & Reversible**: No re-encoding, all changes can be undone (MP3 and AAC)
- **ReplayGain**: Track and album gain analysis for MP3 and AAC/M4A
- **Zero dependencies**: Single static binary (no ffmpeg, no mp3gain, no aacgain)
- **Cross-platform**: macOS, Linux, Windows (x86_64 and ARM64)
- **mp3gain / aacgain compatible**: Drop-in replacement with identical CLI
- **GUI Application**: Native desktop app for drag-and-drop workflow

## Installation

### CLI (`mp3rgain`)

| Platform | Command |
|----------|---------|
| macOS | `brew install M-Igashi/tap/mp3rgain` |
| Windows | `winget install M-Igashi.mp3rgain` |
| Arch Linux (AUR) | `yay -S mp3rgain-bin` |
| Ubuntu 25.10 (PPA) | `sudo add-apt-repository ppa:m-igashi/mp3rgain && sudo apt install mp3rgain` (amd64/arm64) |
| Debian | `sudo apt install ./mp3rgain_*_amd64.deb` ([download](https://github.com/M-Igashi/mp3rgain/releases)) (ARM64 also available) |
| Nix/NixOS | `nix profile install github:M-Igashi/mp3rgain` |
| Docker | `docker pull ghcr.io/m-igashi/mp3rgain:latest` |
| Cargo | `cargo install mp3rgain` |

### GUI (`mp3rgui`)

| Platform | Command |
|----------|---------|
| macOS | `brew install --cask M-Igashi/tap/mp3rgui` |
| Windows | `winget install M-Igashi.mp3rgui` |
| Arch Linux (AUR) | `yay -S mp3rgui` |
| Ubuntu 25.10 (PPA) | `sudo add-apt-repository ppa:m-igashi/mp3rgui && sudo apt install mp3rgui` (amd64/arm64) |
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

## Migrating from mp3gain?

Already running `mp3gain` (or `aacgain`) in a script, Dockerfile, or CI pipeline? mp3rgain is a drop-in replacement — the CLI flags, the TSV output format, and the APEv2 `mp3gain_undo` tag are all mp3gain-compatible, so existing parsers (e.g. [beets](https://beets.io/)) keep working unchanged. For most setups, migration is a one-line substitution:

```bash
sed -i 's/\bmp3gain\b/mp3rgain/g' your_script.sh
```

See **[docs/migrating-from-mp3gain.md](docs/migrating-from-mp3gain.md)** for the full flag equivalence table, Dockerfile/CI substitution patterns, tag interop notes, and the small set of intentional behaviour differences. Bit-level verification lives in [docs/compatibility-report.md](docs/compatibility-report.md).

## GUI Application

A native GUI application (`mp3rgui`) is available for users who prefer a graphical interface.

<p align="center">
  <img src="docs/branding/mp3rgui-screenshot-compact.png" alt="mp3rgui showing track and album ReplayGain analysis for a batch of files" width="820">
</p>

**Features:**

- Drag-and-drop file / folder loading (recurses subfolders)
- Track and Album ReplayGain analysis (parallel, with Cancel)
- Apply Track / Album Gain — shares the same `apply_with_options` pipeline as the CLI
- **Options panel:** Prevent clipping (`-k`), Preserve mtime (`-p`), Wrap mode (`-w`), Use ID3v2 (`-s i`), Dry run (`-n`)
- **Modify Gain menu:** Apply Track / Album / Manual (`-g`) / Channel (`-l`) Gain, Undo (`-u`), Delete Stored Tags (`-s d`)
- **Analysis menu:** Track / Album Analysis, Find Max Amplitude (`-x`), Check Stored Tags (`-s c`)
- **Stored RG** table column shows existing ReplayGain / undo tags (APE / ID3v2 / MP4 freeform) with a per-tag breakdown on hover
- Responsive UI: all batch work runs on a worker thread with per-file progress and a Cancel button

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
| `-d <n>` | Modify suggested dB gain by n (mp3gain-compatible; applied with `-r` / `-a`) |
| `-u` | Undo gain changes |
| `-k` | Prevent clipping |
| `-R` | Process directories recursively |
| `--skip-errors` | Keep album analysis (`-a`) going past unreadable files |
| `-n` | Dry-run mode |
| `-j <n>` / `--threads <n>` | Worker threads for analysis (default: auto, 0=auto, 1=serial) |
| `-o [fmt]` | Output format: `text`, `json`, `tsv` (default: tsv if no argument) |

Run `mp3rgain -h` for the full list of options.

ReplayGain analysis runs in parallel by default
(`std::thread::available_parallelism()` worker threads). Use `-j 1` or
`MP3RGAIN_THREADS=1` for the legacy serial path. See
[docs/perf-parallel.md](docs/perf-parallel.md) for the design and
real-corpus benchmark numbers.

## Documentation

- [Migration Guide](docs/migrating-from-mp3gain.md) - Drop-in replacement for mp3gain: flag equivalence, sed/Dockerfile/CI substitution patterns, beets config
- [Parallel Performance](docs/perf-parallel.md) - `-j` / `--threads` design and real-corpus benchmark numbers
- [Roadmap](docs/roadmap.md) - Development plans and upcoming features
- [Security](docs/security.md) - Memory safety and CVE analysis
- [Compatibility Report](docs/compatibility-report.md) - Verification against original mp3gain
- [Technical Comparison](docs/COMPARISON.md) - Comparison with similar tools
- [Use Cases](docs/use-cases.md) - Integration examples (beets, headroom, etc.)
- [Download Stats](https://m-igashi.github.io/mp3rgain/) - Weekly download trends across all platforms

## Why mp3rgain?

The original [mp3gain](http://mp3gain.sourceforge.net/) has been unmaintained upstream since ~2015 (though distribution maintainers continue to apply security patches). [aacgain](http://aacgain.altosdesign.com/), its AAC counterpart, has been unmaintained since ~2009 and is effectively unbuildable on modern 64-bit systems. mp3rgain is a modern, memory-safe replacement written in Rust that covers both.

**AAC/M4A on the CLI is the differentiator.** Among CLIs, rsgain / loudgain / FFmpeg either only write ReplayGain tags (which non-compliant players ignore) or re-encode the audio. foobar2000 has a comparable "Apply ReplayGain to file content" feature for AAC in MP4/MKA, but it is Windows GUI only, has no undo, and is unsuited for batch, headless, or container workflows. mp3rgain fills the cross-platform, scriptable, reversible niche.

mp3rgain implements the **ReplayGain 1.0 algorithm** (89 dB reference level) for full compatibility with the original mp3gain / aacgain. Loudness values will differ from EBU R128/LUFS-based tools (foobar2000, loudgain, ffmpeg loudnorm).

## Use mp3rgain in Docker / CI

Official multi-arch images (`linux/amd64`, `linux/arm64`) are published to GHCR:

```
ghcr.io/m-igashi/mp3rgain:latest
ghcr.io/m-igashi/mp3rgain:v2          # latest 2.x
ghcr.io/m-igashi/mp3rgain:v2.8.0      # exact version
```

The image is built `FROM scratch` with a fully static (musl) binary — no
shell, no runtime deps, ~2 MB. Drop-in replacement for `mp3gain` in
containerized batch / cron pipelines (e.g. Plex maintenance windows):

```bash
# Normalize a music library by mounting it into the container
docker run --rm \
  -v /path/to/music:/music \
  ghcr.io/m-igashi/mp3rgain:latest -r -R /music

# Run as your own user so written files keep correct ownership
docker run --rm \
  --user "$(id -u):$(id -g)" \
  -v /path/to/music:/music \
  ghcr.io/m-igashi/mp3rgain:latest -r -R /music
```

Because the entrypoint is the binary itself, all `mp3rgain` flags work
exactly the same as the host CLI (`-r`, `-a`, `-R`, `-k`, `-u`, …).

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
