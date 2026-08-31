# mp3rgain vs aacgain/mp3gain: Detailed Comparison

This document provides a detailed comparison between mp3rgain and the original aacgain/mp3gain tools.

> **Headline:** mp3rgain is the **only actively maintained CLI** that performs lossless `global_gain` rewrite on AAC/M4A files. aacgain has been effectively abandoned since ~2009 and is unbuildable on most modern 64-bit systems; among other CLIs, rsgain / loudgain stop at ReplayGain tags and FFmpeg re-encodes. foobar2000 has a comparable "Apply ReplayGain to file content" feature for AAC in MP4/MKA, but it is Windows GUI only with no undo and is not built for batch / headless / CI use. If you need re-encode-free AAC volume adjustment from a script, container, or non-Windows host in 2026, mp3rgain is the only practical choice — **on a Windows desktop with a GUI, foobar2000 is the one to reach for.**

> **A framing note before the tables:** every tool discussed here is a ReplayGain tool, mp3rgain
> included. The mp3gain lineage (mp3gain, aacgain, mp3rgain) runs a ReplayGain analysis, writes the
> standard `REPLAYGAIN_*` tags, *and* bakes the correction into the bitstream; foobar2000 can do
> both as well. "Tagger vs gain tool" is the wrong axis — the axis that matters is whether a tool
> can touch the bitstream at all, and whether it can put it back.

## Overview

| | mp3rgain | aacgain | mp3gain |
|---|----------|---------|---------|
| **Language** | Rust | C | C |
| **Last Update** | Active (2026) | 2022 | 2018 |
| **License** | MIT | LGPL | LGPL |
| **Version** | 3.1.0 | 1.8.2 | 1.6.2 |
| **Repository** | [M-Igashi/mp3rgain](https://github.com/M-Igashi/mp3rgain) | [dgilman/aacgain](https://github.com/dgilman/aacgain) | SourceForge |

## Feature Comparison

### Supported Formats

| Format | mp3rgain | aacgain | mp3gain |
|--------|----------|---------|---------|
| MP3 (MPEG1 Layer III) | Yes | Yes | Yes |
| MP3 (MPEG2 Layer III) | Yes | Yes | Yes |
| MP3 (MPEG2.5 Layer III) | Yes | Yes | Yes |
| AAC (M4A/MP4) | Yes (lossless) | Yes (lossless) | No |
| AAC (raw .aac) | No | No | No |
| HE-AAC/SBR | Yes (base layer) | No | No |
| Apple Lossless | No | No | No |

Note: As of v2.0.0, mp3rgain supports lossless AAC bitstream gain adjustment (modifying `global_gain` fields), matching aacgain's approach. Both tools also store undo information in iTunes freeform metadata tags. mp3rgain additionally supports HE-AAC/SBR files (base layer gain adjustment). ALAC and DRM-protected M4P files are detected and rejected with clear error messages.

### Command-Line Options

All options from the original mp3gain are fully implemented in mp3rgain:

| Option | Description | mp3rgain | aacgain | mp3gain |
|--------|-------------|----------|---------|---------|
| `-g <i>` | Apply gain of i steps | Yes | Yes | Yes |
| `-d <n>` | Modify suggested dB gain by n | Yes | Yes | Yes |
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
| `-s i` | Use ID3v2 tags | Yes | Yes | Yes |
| `-s a` | Use APEv2 tags | Yes | Yes | Yes |
| `-v` | Show version | Yes | Yes | Yes |
| `-h` | Show help | Yes | Yes | Yes |

### mp3rgain Extensions (Not in aacgain/mp3gain)

| Option | Description |
|--------|-------------|
| `-R` | Recursive directory processing |
| `-s R` | Reuse stored ReplayGain tags with `-r`/`-a`, rescanning only when tags are missing (mp3gain's *default* behavior, opt-in here) |
| `-n` / `--dry-run` | Dry-run mode (preview changes without modifying files) |
| `-o json` | JSON output format (for scripting and automation) |
| `-o tsv` | Tab-separated output (database-friendly) |
| Progress bar | Visual progress for batch operations |

## Technical Comparison

### ReplayGain Implementation

| Aspect | mp3rgain | aacgain/mp3gain |
|--------|----------|-----------------|
| Algorithm | ReplayGain 1.0 (default); BS.1770 opt-in via `--rg2` / `--r128` (v3.0+) | ReplayGain 1.0 |
| Reference level | 89 dB (RG1) / −18 LUFS (`--rg2`) / −23 LUFS (`--r128`) | 89 dB |
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
| ID3v2 | Yes (`-s i`) | Yes | Yes |
| iTunes freeform (M4A) | Yes | Yes | - |

### Undo Information

For MP3 files, both tools store undo data in APEv2 tags:
- `MP3GAIN_MINMAX` - Original min/max gain values
- `MP3GAIN_UNDO` - Gain adjustment applied

For AAC/M4A files, both mp3rgain and aacgain store undo data in iTunes freeform metadata tags.

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

# Windows (winget)
winget install M-Igashi.mp3rgain

# Arch Linux (AUR)
yay -S mp3rgain-bin

# Debian/Ubuntu (amd64 and arm64 .deb available)
sudo apt install ./mp3rgain_*_amd64.deb

# Any platform (Cargo) - includes ReplayGain by default
cargo install mp3rgain

# Binary download (macOS, Linux x86_64/arm64, Windows x86_64/arm64)
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

For AAC/M4A files, mp3rgain v2.0.0+ provides the same lossless bitstream gain adjustment as aacgain:
```bash
# Analyze and apply gain to M4A files
mp3rgain -r *.m4a
mp3rgain -a *.m4a

# Undo AAC gain changes
mp3rgain -u *.m4a
```

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

### AAC Volume Adjustment: Tool Landscape

For AAC/M4A files specifically, the choice of tool matters more than for MP3, because almost no other modern tool can avoid re-encoding:

| Tool | AAC approach | Writes RG tags? | Lossless? | Player-agnostic? | Maintained? | CLI / scriptable? |
|------|--------------|-----------------|-----------|------------------|-------------|-------------------|
| **mp3rgain** | `global_gain` rewrite | **Yes (RG1 / RG2 / R128)** | **Yes** | **Yes** | **Yes (active)** | **Yes** |
| aacgain | `global_gain` rewrite | Yes (RG1) | Yes | Yes | No (~2009) | Yes |
| foobar2000 "Apply ReplayGain to file content" | scalefactor rewrite (MP4/MKA AAC) | Yes (RG2 — reference implementation) | Yes (one-shot, no undo) | Yes | Yes | No (Windows GUI only) |
| rsgain | ReplayGain 2.0 tags | Yes (RG2 / R128) | Yes (file untouched) | No (player must read tags) | Yes | Yes |
| loudgain | ReplayGain 2.0 tags | Yes (RG2 / R128) | Yes (file untouched) | No (player must read tags) | Yes | Yes |
| FFmpeg `volume` | Re-encode | No | No (lossy) | Yes | Yes | Yes |
| FFmpeg `loudnorm` | Re-encode | No | No (lossy) | Yes | Yes | Yes |
| beets ReplayGain plugin | Tags via backend | Yes (backend-dependent) | Yes (file untouched) | No (player must read tags) | Yes | Yes |

**The "lossless + player-agnostic + maintained + CLI/scriptable + reversible" intersection contains exactly one tool: mp3rgain.** This matters for DJ equipment, car audio, smart speakers, batch / Docker / CI pipelines, and any environment where the playback device ignores ReplayGain tags or where a desktop GUI is not an option.

That is a statement about the intersection, not a ranking. [foobar2000](https://www.foobar2000.org/) covers the lossless / player-agnostic / maintained cells and does so well; what it does not offer is a command line, a non-Windows host, or an undo path. **If you are on Windows, working in a GUI, and want the most standards-faithful ReplayGain, foobar2000 is the tool to recommend** — it is the closest thing ReplayGain 2.0 has to a reference implementation, and it tags far more formats than mp3rgain does. The two interoperate by design: mp3rgain writes the standard `REPLAYGAIN_*` tags foobar2000 reads, and `--rg2` is built to reproduce its measurement rather than to offer a rival one.

### When to Use global_gain vs ReplayGain Tags

| Use Case | Recommended Approach |
|----------|---------------------|
| DJ equipment (CDJs, controllers) | `global_gain` (mp3rgain) |
| Car stereos, portable players | `global_gain` (mp3rgain) |
| Smart speakers, Chromecast | `global_gain` (mp3rgain) |
| Desktop players (foobar2000, etc.) | ReplayGain tags (rsgain, or foobar2000's own scanner) |
| Streaming to phone apps | ReplayGain tags (rsgain) |
| Maximum flexibility | ReplayGain tags (rsgain) |

For most modern listening setups, **ReplayGain tags are the cleaner solution**. Use `global_gain` adjustment when your playback device doesn't support ReplayGain tags.

Note that this is not an either/or for mp3rgain users: applying gain also writes the standard
`REPLAYGAIN_TRACK_GAIN` / `REPLAYGAIN_ALBUM_GAIN` tags (residual values, per mp3gain's convention),
so a tag-aware player and a tag-blind one converge on the same loudness. `-s s` skips the tags if
you want the bitstream change alone.

The other end of that trade-off is available too: `--tags-only` runs the same analysis
but writes full `REPLAYGAIN_*` values without modifying a single frame, matching what `loudgain` and
`rsgain` produce. Use it when the listener should keep the choice of switching ReplayGain off in
their player; use the default apply when the playback device ignores tags entirely.

```bash
mp3rgain -a --tags-only *.mp3   # tags only, audio byte-identical
mp3rgain -a *.mp3               # gain baked into global_gain, plus residual tags
```

## Security

See [Security Documentation](security.md) for detailed CVE analysis.

| Tool | Security Status |
|------|-----------------|
| mp3rgain | Memory-safe (Rust), not affected by mp3gain/aacgain CVEs |
| mp3gain 1.6.2 | All known CVEs fixed (CVE-2023-49356 patched in Debian 1.6.2-2) |
| aacgain 2.0.0 | **Still bundles vulnerable mpglibDBL** - CVE-2021-34085 and others unpatched |

## Known Limitations

### mp3rgain
- ID3v2 tag storage supported via `-s i` option

### aacgain
- **Security**: Bundles vulnerable mpglibDBL (CVE-2021-34085 unpatched)
- Limited Windows 11 compatibility
- Requires C build environment on some platforms
- faad2 dependency for AAC

### mp3gain
- Upstream unmaintained (security patches applied by distribution maintainers)
- Limited modern OS support
- No AAC support

## Why Choose mp3rgain?

1. **Only viable AAC bitstream gain CLI today**: aacgain is unmaintained and unbuildable on modern 64-bit systems; rsgain/loudgain only write tags; FFmpeg only re-encodes; foobar2000 is Windows GUI only with no undo. mp3rgain is the only path to lossless, reversible, player-agnostic AAC volume adjustment from a script, container, or non-Windows host.
2. **Modern platform support**: Works on Windows 11, macOS (including Apple Silicon), and Linux
3. **No dependencies**: Single static binary, no ffmpeg or other libraries required
4. **Memory safety**: Written in Rust with strong safety guarantees
5. **Active development**: Regularly updated and maintained
6. **Extended features**: Recursive processing, dry-run mode, JSON output
7. **Drop-in replacement**: 100% command-line compatible with original mp3gain / aacgain
