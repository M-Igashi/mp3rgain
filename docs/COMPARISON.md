# mp3rgain vs aacgain/mp3gain: Detailed Comparison

This document provides a detailed comparison between mp3rgain and the original aacgain/mp3gain tools.

## Overview

| | mp3rgain | aacgain | mp3gain |
|---|----------|---------|---------|
| **Language** | Rust | C | C |
| **Last Update** | Active (2026) | 2022 | ~2015 |
| **License** | MIT | LGPL | LGPL |
| **Version** | 1.3.0 | 1.8.2 | 1.5.2.1 |
| **Repository** | [M-Igashi/mp3rgain](https://github.com/M-Igashi/mp3rgain) | [dgilman/aacgain](https://github.com/dgilman/aacgain) | SourceForge |

## Feature Comparison

### Supported Formats

| Format | mp3rgain | aacgain | mp3gain |
|--------|----------|---------|---------|
| MP3 (MPEG1 Layer III) | Yes | Yes | Yes |
| MP3 (MPEG2 Layer III) | Yes | Yes | Yes |
| MP3 (MPEG2.5 Layer III) | Yes | Yes | Yes |
| AAC (M4A/MP4) | Yes (tags only) | Yes (lossless) | No |
| AAC (raw .aac) | No | No | No |
| HE-AAC/SBR | No | No | No |
| Apple Lossless | No | No | No |

Note: For AAC files, mp3rgain writes ReplayGain metadata tags. aacgain can modify the audio data losslessly using iTunes-style Sound Check.

### Command-Line Options

All options from the original mp3gain are fully implemented in mp3rgain:

| Option | Description | mp3rgain | aacgain | mp3gain |
|--------|-------------|----------|---------|---------|
| `-g <i>` | Apply gain of i steps | Yes | Yes | Yes |
| `-d <n>` | Apply gain of n dB | Yes | Yes | Yes |
| `-r` | Apply track gain (ReplayGain) | Yes | Yes | Yes |
| `-a` | Apply album gain (ReplayGain) | Yes | Yes | Yes |
| `-u` | Undo gain changes | Yes | Yes | Yes |
| `-l <c> <g>` | Channel-specific gain | Yes | Yes | Yes |
| `-m <i>` | Modify suggested gain | Yes | Yes | Yes |
| `-e` | Skip album analysis | Yes | Yes | Yes |
| `-x` | Find max amplitude only | Yes | Yes | Yes |
| `-k` | Prevent clipping | Yes | Yes | Yes |
| `-c` | Ignore clipping warnings | Yes | Yes | Yes |
| `-p` | Preserve file timestamp | Yes | Yes | Yes |
| `-q` | Quiet mode | Yes | Yes | Yes |
| `-w` | Wrap gain values | Yes | Yes | Yes |
| `-t` | Use temp file for writing | Yes | Yes | Yes |
| `-f` | Assume MPEG 2 Layer III | Yes | Yes | Yes |
| `-s c` | Check stored tag info | Yes | Yes | Yes |
| `-s d` | Delete stored tag info | Yes | Yes | Yes |
| `-s s` | Skip stored tag info | Yes | Yes | Yes |
| `-s r` | Force recalculation | Yes | Yes | Yes |
| `-s i` | Use ID3v2 tags | Partial | Yes | Yes |
| `-s a` | Use APEv2 tags | Yes | Yes | Yes |
| `-v` | Show version | Yes | Yes | Yes |
| `-h` | Show help | Yes | Yes | Yes |

### mp3rgain Extensions (Not in aacgain/mp3gain)

| Option | Description |
|--------|-------------|
| `-R` | Recursive directory processing |
| `-n` / `--dry-run` | Dry-run mode (preview changes without modifying files) |
| `-o json` | JSON output format (for scripting and automation) |
| `-o tsv` | Tab-separated output (database-friendly) |
| Progress bar | Visual progress for batch operations |

## Technical Comparison

### ReplayGain Implementation

| Aspect | mp3rgain | aacgain/mp3gain |
|--------|----------|-----------------|
| Algorithm | ReplayGain 1.0 | ReplayGain 1.0 |
| Reference level | 89 dB | 89 dB |
| Window size | 50ms | 50ms |
| Percentile | 95th | 95th |
| Equal-loudness filter | Yule-Walker + Butterworth | Yule-Walker + Butterworth |
| MP3 decoding | symphonia (Rust) | mpglib (C) |
| AAC decoding | symphonia (Rust) | faad2 (C) |

**Note**: As of v1.2.6, mp3rgain's ReplayGain analysis uses the correct filter coefficients from the original ReplayGain specification, producing results consistent with the original mp3gain/aacgain.

### Tag Storage

| Tag Type | mp3rgain | aacgain | mp3gain |
|----------|----------|---------|---------|
| APEv2 (default for MP3) | Yes | Yes | Yes |
| ID3v2 | Planned | Yes | Yes |
| iTunes freeform (M4A) | Yes | Yes | - |

### Undo Information

Both tools store undo data in APEv2 tags:
- `MP3GAIN_MINMAX` - Original min/max gain values
- `MP3GAIN_UNDO` - Gain adjustment applied

## Platform Support

| Platform | mp3rgain | aacgain | mp3gain |
|----------|----------|---------|---------|
| macOS (Intel) | Yes | Build required | Build required |
| macOS (Apple Silicon) | Yes (Universal) | Build required | Limited |
| Linux (x86_64) | Yes | Build required | Build required |
| Linux (ARM64) | Yes | Build required | Limited |
| Windows (x86_64) | Yes | Binary available | Binary available |
| Windows (ARM64) | Yes | No | No |
| Windows 11 | Yes | Compatibility issues | Compatibility issues |

## Installation

### mp3rgain

```bash
# macOS (Homebrew)
brew install M-Igashi/tap/mp3rgain

# Any platform (Cargo) - includes ReplayGain by default
cargo install mp3rgain

# Minimal installation (no audio decoding, gain adjustment only)
cargo install mp3rgain --no-default-features

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

mp3rgain is a drop-in replacement. All commands work identically:

```bash
# These commands work the same way
mp3gain -r *.mp3
mp3rgain -r *.mp3

mp3gain -a *.mp3
mp3rgain -a *.mp3

mp3gain -g 2 song.mp3
mp3rgain -g 2 song.mp3

mp3gain -u song.mp3
mp3rgain -u song.mp3
```

Additional features in mp3rgain:
```bash
# Recursive processing (new)
mp3rgain -r -R /path/to/music

# Dry-run mode (new)
mp3rgain -r -n *.mp3

# JSON output (new)
mp3rgain -o json *.mp3
```

### From aacgain to mp3rgain

For MP3 files, commands are identical.

For AAC/M4A files, mp3rgain writes ReplayGain tags that compatible players will read:
```bash
# Analyze and tag M4A files
mp3rgain -r *.m4a
mp3rgain -a *.m4a
```

Note: aacgain can modify AAC audio data losslessly (similar to MP3 global_gain), while mp3rgain only writes metadata tags for AAC files. Most modern players support ReplayGain tags.

## Binary Size Comparison

| Tool | Approximate Size |
|------|------------------|
| mp3rgain (full) | ~1.8 MB |
| mp3rgain (minimal) | ~670 KB |
| aacgain | ~500 KB + dependencies |
| mp3gain | ~200 KB + dependencies |

mp3rgain is a single static binary with no runtime dependencies.

## Performance

Both mp3rgain and mp3gain/aacgain provide similar performance for gain analysis and application. The main differences:

- **Startup time**: mp3rgain has no dynamic library loading
- **Memory safety**: mp3rgain is written in Rust with memory-safe guarantees
- **Parallel processing**: Both process files sequentially (per-file, not per-album)

## Important Notes

### Avoiding Double Volume Adjustment

If you apply `global_gain` adjustment with mp3rgain and later add ReplayGain tags with another tool (like rsgain), you may get **double adjustment** - the player will apply ReplayGain on top of the already-modified volume.

**Recommendations:**

1. **Choose one approach**: Either use `global_gain` adjustment (mp3rgain) OR ReplayGain tags (rsgain), not both.

2. **If you need both**: Apply `global_gain` first, then delete any existing ReplayGain tags:
   ```bash
   mp3rgain -r *.mp3           # Apply gain
   mp3rgain -s d *.mp3         # Delete ReplayGain tags
   ```

3. **Check before re-tagging**: If your files have been processed with mp3rgain, undo first before applying ReplayGain tags with another tool:
   ```bash
   mp3rgain -u *.mp3           # Undo global_gain changes
   rsgain easy *.mp3           # Then apply ReplayGain tags
   ```

### When to Use global_gain vs ReplayGain Tags

| Use Case | Recommended Approach |
|----------|---------------------|
| DJ equipment (CDJs, controllers) | `global_gain` (mp3rgain) |
| Car stereos, portable players | `global_gain` (mp3rgain) |
| Smart speakers, Chromecast | `global_gain` (mp3rgain) |
| Desktop players (foobar2000, etc.) | ReplayGain tags (rsgain) |
| Streaming to phone apps | ReplayGain tags (rsgain) |
| Maximum flexibility | ReplayGain tags (rsgain) |

For most modern listening setups, **ReplayGain tags are the cleaner solution**. Use `global_gain` adjustment when your playback device doesn't support ReplayGain tags.

## Security

See [Security Documentation](security.md) for detailed CVE analysis.

| Tool | Security Status |
|------|-----------------|
| mp3rgain | Memory-safe (Rust), not affected by mp3gain/aacgain CVEs |
| mp3gain 1.6.2 | Most CVEs fixed, CVE-2023-49356 unpatched |
| aacgain 2.0.0 | **Still bundles vulnerable mpglibDBL** - CVE-2021-34085 and others unpatched |

## Known Limitations

### mp3rgain
- AAC: Writes tags only, does not modify audio data
- ID3v2 tag storage not yet implemented (uses APEv2)

### aacgain
- **Security**: Bundles vulnerable mpglibDBL (CVE-2021-34085 unpatched)
- Limited Windows 11 compatibility
- Requires C build environment on some platforms
- faad2 dependency for AAC

### mp3gain
- CVE-2023-49356 unpatched in 1.6.2
- Limited modern OS support
- No AAC support

## Why Choose mp3rgain?

1. **Modern platform support**: Works on Windows 11, macOS (including Apple Silicon), and Linux
2. **No dependencies**: Single static binary, no ffmpeg or other libraries required
3. **Memory safety**: Written in Rust with strong safety guarantees
4. **Active development**: Regularly updated and maintained
5. **Extended features**: Recursive processing, dry-run mode, JSON output
6. **Drop-in replacement**: 100% command-line compatible with original mp3gain
