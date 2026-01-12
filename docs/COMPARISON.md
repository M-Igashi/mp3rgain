# mp3rgain vs aacgain/mp3gain: Detailed Comparison

This document provides a detailed comparison between mp3rgain and the original aacgain/mp3gain tools.

## Overview

| | mp3rgain | aacgain | mp3gain |
|---|----------|---------|---------|
| **Language** | Rust | C | C |
| **Last Update** | Active | 2022 | ~2015 |
| **License** | MIT | LGPL | LGPL |
| **Repository** | [M-Igashi/mp3rgain](https://github.com/M-Igashi/mp3rgain) | [dgilman/aacgain](https://github.com/dgilman/aacgain) | SourceForge |

## Supported Formats

| Format | mp3rgain | aacgain | mp3gain |
|--------|----------|---------|---------|
| MP3 (MPEG1 Layer III) | Yes | Yes | Yes |
| MP3 (MPEG2 Layer III) | Yes | Yes | Yes |
| MP3 (MPEG2.5 Layer III) | Yes | Yes | Yes |
| AAC (M4A/MP4/M4V) | Planned | Yes | No |
| AAC (raw .aac) | No | No | No |
| HE-AAC/SBR | No | No | No |
| Apple Lossless | No | No | No |

## Command-Line Options

### Fully Implemented in mp3rgain

| Option | Description | mp3rgain | aacgain |
|--------|-------------|----------|---------|
| `-g <i>` | Apply gain of i steps | Yes | Yes |
| `-d <n>` | Apply gain of n dB | Yes | Yes |
| `-r` | Apply track gain (ReplayGain) | Yes | Yes |
| `-a` | Apply album gain (ReplayGain) | Yes | Yes |
| `-u` | Undo gain changes | Yes | Yes |
| `-l <c> <g>` | Channel-specific gain | Yes | Yes |
| `-k` | Prevent clipping | Yes | Yes |
| `-c` | Ignore clipping warnings | Yes | Yes |
| `-p` | Preserve file timestamp | Yes | Yes |
| `-q` | Quiet mode | Yes | Yes |
| `-s c` | Check/analyze only | Yes | Yes |
| `-v` | Show version | Yes | Yes |
| `-h` | Show help | Yes | Yes |

### mp3rgain Extensions (Not in aacgain)

| Option | Description |
|--------|-------------|
| `-R` | Recursive directory processing |
| `-n` / `--dry-run` | Dry-run mode (preview changes) |
| `-o json` | JSON output format |
| `-o text` | Text output format (default) |
| Progress bar | Visual progress for batch operations |

### Planned Features (from aacgain)

| Option | Description | Issue |
|--------|-------------|-------|
| `-m <i>` | Modify suggested gain by integer | [#18](https://github.com/M-Igashi/mp3rgain/issues/18) |
| `-e` | Skip album analysis | [#19](https://github.com/M-Igashi/mp3rgain/issues/19) |
| `-x` | Find max amplitude only | [#20](https://github.com/M-Igashi/mp3rgain/issues/20) |
| `-w` | Wrap gain values | [#21](https://github.com/M-Igashi/mp3rgain/issues/21) |
| `-t` | Use temp file for writing | [#22](https://github.com/M-Igashi/mp3rgain/issues/22) |
| `-f` | Assume MPEG 2 Layer III | [#23](https://github.com/M-Igashi/mp3rgain/issues/23) |
| `-s d` | Delete stored tag info | [#24](https://github.com/M-Igashi/mp3rgain/issues/24) |
| `-s s` | Skip (ignore) stored tag info | [#24](https://github.com/M-Igashi/mp3rgain/issues/24) |
| `-s r` | Force recalculation | [#24](https://github.com/M-Igashi/mp3rgain/issues/24) |
| `-s i` | Use ID3v2 tags | [#24](https://github.com/M-Igashi/mp3rgain/issues/24) |
| `-s a` | Use APEv2 tags (default) | [#24](https://github.com/M-Igashi/mp3rgain/issues/24) |
| `-i <i>` | Track index for multi-track | [#25](https://github.com/M-Igashi/mp3rgain/issues/25) |
| `-o` (tsv) | Tab-separated output | [#26](https://github.com/M-Igashi/mp3rgain/issues/26) |
| AAC support | M4A/MP4/M4V files | [#17](https://github.com/M-Igashi/mp3rgain/issues/17) |

## Technical Differences

### ReplayGain Implementation

| Aspect | mp3rgain | aacgain/mp3gain |
|--------|----------|-----------------|
| Algorithm | ReplayGain 1.0 | ReplayGain 1.0 |
| Reference level | 89 dB | 89 dB |
| Window size | 50ms | 50ms |
| Percentile | 95th | 95th |
| MP3 decoding | symphonia (Rust) | mpglib (C) |
| AAC decoding | Planned (symphonia) | faad2 (C) |

### Tag Storage

| Tag Type | mp3rgain | aacgain/mp3gain |
|----------|----------|-----------------|
| APEv2 (default) | Yes | Yes |
| ID3v2 | Planned | Yes (-s i) |
| MP4 atoms | Planned | Yes (AAC) |

### Undo Information

Both store undo data in APEv2 tags:
- `MP3GAIN_MINMAX` - Original min/max gain values
- `MP3GAIN_UNDO` - Gain adjustment applied

## Platform Support

| Platform | mp3rgain | aacgain | mp3gain |
|----------|----------|---------|---------|
| macOS (Intel) | Yes | Build required | Build required |
| macOS (Apple Silicon) | Yes (Universal) | Build required | Limited |
| Linux (x86_64) | Yes | Build required | Build required |
| Linux (ARM64) | Planned | Build required | Limited |
| Windows (x86_64) | Yes | Binary available | Binary available |
| Windows (ARM64) | Yes | No | No |
| Windows 11 | Yes | Compatibility issues | Compatibility issues |

## Installation Methods

### mp3rgain

```bash
# macOS (Homebrew)
brew install M-Igashi/tap/mp3rgain

# Any platform (Cargo)
cargo install mp3rgain --features replaygain

# Binary download
# https://github.com/M-Igashi/mp3rgain/releases
```

### aacgain

```bash
# macOS (Homebrew - may be outdated)
brew install aacgain

# Build from source
git clone https://github.com/dgilman/aacgain
cd aacgain
# Follow build instructions
```

### mp3gain

```bash
# Linux (package manager)
apt install mp3gain  # Debian/Ubuntu
dnf install mp3gain  # Fedora

# Windows
# Download from SourceForge
```

## Migration Guide

### From mp3gain to mp3rgain

Most commands work identically:

```bash
# Before (mp3gain)
mp3gain -r *.mp3
mp3gain -a *.mp3
mp3gain -g 2 song.mp3
mp3gain -u song.mp3

# After (mp3rgain) - same commands
mp3rgain -r *.mp3
mp3rgain -a *.mp3
mp3rgain -g 2 song.mp3
mp3rgain -u song.mp3
```

New features available:
```bash
# Recursive processing
mp3rgain -r -R /path/to/music

# Dry-run mode
mp3rgain -r -n *.mp3

# JSON output
mp3rgain -o json *.mp3
```

### From aacgain to mp3rgain

For MP3 files, commands are identical. For AAC files, wait for issue #17 to be resolved.

```bash
# MP3 files - works now
mp3rgain -r *.mp3

# AAC files - coming soon
# mp3rgain -r *.m4a  # After #17 is implemented
```

## Known Limitations

### mp3rgain
- No AAC support yet (planned)
- No ID3v2 tag storage yet (planned)
- No `-o` tab-delimited output yet (planned)

### aacgain
- Limited Windows 11 compatibility
- Requires C build environment
- faad2 dependency for AAC

### mp3gain
- Unmaintained since ~2015
- Limited modern OS support
- No AAC support
