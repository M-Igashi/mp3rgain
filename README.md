# mp3rgain

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-blue.svg)](https://www.rust-lang.org)
[![crates.io](https://img.shields.io/crates/v/mp3rgain.svg)](https://crates.io/crates/mp3rgain)

**Lossless MP3 volume adjustment - a modern mp3gain replacement written in Rust**

mp3rgain adjusts MP3 volume without re-encoding by modifying the `global_gain` field in each frame's side information. This preserves audio quality while achieving permanent volume changes.

## Features

- **Lossless**: No re-encoding, preserves original audio quality
- **Fast**: Direct binary manipulation, no audio decoding required for gain adjustment
- **Reversible**: All changes can be undone (stored in APEv2 tags)
- **ReplayGain**: Track and album gain analysis (included by default)
- **AAC/M4A Support**: Analyze and tag M4A files with ReplayGain metadata
- **Zero runtime dependencies**: Single static binary (no ffmpeg, no mp3gain)
- **Cross-platform**: macOS, Linux, Windows (x86_64 and ARM64)
- **mp3gain compatible**: Full command-line compatibility with original mp3gain
- **Pure Rust**: Memory-safe implementation

## Installation

### Windows

**Download and extract** (recommended for most users):

1. Download the latest release from [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases):
   - `mp3rgain-*-windows-x86_64.zip` (most Windows PCs)
   - `mp3rgain-*-windows-arm64.zip` (ARM-based Windows devices)

2. Extract the ZIP file to a folder (e.g., `C:\Tools\mp3rgain`)

3. Open PowerShell or Command Prompt and navigate to the folder:
   ```powershell
   cd C:\Tools\mp3rgain
   .\mp3rgain.exe -h
   ```

4. (Optional) Add to PATH for system-wide access:
   - Open "Edit environment variables for your account" from Start menu
   - Edit the `Path` variable and add your folder (e.g., `C:\Tools\mp3rgain`)
   - Restart your terminal

> **Note:** mp3rgain is a command-line tool, not a GUI application. Double-clicking the executable will briefly open and close a terminal window. Always run it from PowerShell or Command Prompt.

**Using Cargo** (for Rust developers):
```powershell
cargo install mp3rgain
```

### macOS

**Homebrew** (recommended):
```bash
brew install M-Igashi/tap/mp3rgain
```

**Download binary:**
1. Download `mp3rgain-*-macos-universal.tar.gz` from [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases)
2. Extract: `tar -xzf mp3rgain-*-macos-universal.tar.gz`
3. Move to PATH: `sudo mv mp3rgain /usr/local/bin/`

### Linux

**Download binary:**
1. Download `mp3rgain-*-linux-x86_64.tar.gz` from [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases)
2. Extract: `tar -xzf mp3rgain-*-linux-x86_64.tar.gz`
3. Move to PATH: `sudo mv mp3rgain /usr/local/bin/`

### Cargo (all platforms)

```bash
# Standard installation (includes ReplayGain support)
cargo install mp3rgain

# Minimal installation (gain adjustment only, no audio decoding)
cargo install mp3rgain --no-default-features
```

All binaries include full ReplayGain support with zero runtime dependencies.

## Quick Start

```bash
# Normalize a single track to ReplayGain reference level (89 dB)
mp3rgain -r song.mp3

# Normalize an album (all tracks get the same adjustment)
mp3rgain -a *.mp3

# Apply manual gain adjustment (+3.0 dB)
mp3rgain -g 2 song.mp3

# Undo previous changes
mp3rgain -u song.mp3
```

## Usage

### Show file information (default)

```bash
mp3rgain song.mp3
```

Output:
```
song.mp3
  Format:      MPEG1 Layer III, Joint Stereo
  Frames:      5765
  Gain range:  89 - 217 (avg: 168.2)
  Headroom:    38 steps (+57.0 dB)
```

### Apply gain adjustment

```bash
# Apply +2 steps (+3.0 dB)
mp3rgain -g 2 song.mp3

# Apply +4.5 dB (rounds to nearest step)
mp3rgain -d 4.5 song.mp3

# Reduce volume by 3 steps (-4.5 dB)
mp3rgain -g -3 *.mp3

# Apply gain and preserve file timestamp
mp3rgain -g 2 -p song.mp3
```

### ReplayGain

```bash
# Apply track gain (normalize each file to 89 dB)
mp3rgain -r song.mp3
mp3rgain -r *.mp3

# Apply album gain (normalize album to 89 dB)
mp3rgain -a *.mp3
```

### AAC/M4A Support

```bash
# Analyze and tag M4A files with ReplayGain
mp3rgain -r song.m4a
mp3rgain -r *.m4a

# Album gain for M4A files
mp3rgain -a *.m4a

# Mix MP3 and M4A files
mp3rgain -r *.mp3 *.m4a

# Recursive directory processing includes M4A files
mp3rgain -R /path/to/music

# Process specific audio track in multi-track files (e.g., video files)
mp3rgain -i 1 movie.m4v   # Process second audio track
mp3rgain -i 0 song.m4a    # Process first track (default)
```

Note: For M4A files, mp3rgain writes ReplayGain tags (iTunes freeform format) but does not modify the audio data, as AAC doesn't have a lossless gain adjustment mechanism like MP3's `global_gain` field.

### Undo previous adjustment

```bash
# Undo gain changes (uses APEv2 tag info)
mp3rgain -u song.mp3
```

### Clipping prevention

```bash
# Automatically reduce gain if clipping would occur
mp3rgain -k -g 5 song.mp3
# Output: gain reduced from 5 to 3 steps to prevent clipping

# With ReplayGain
mp3rgain -k -r song.mp3
```

### Recursive directory processing

```bash
# Process all MP3s in a directory recursively
mp3rgain -R /path/to/music
mp3rgain -g 2 -R /path/to/music
mp3rgain -r -R /path/to/album
```

### Channel-specific gain (stereo balance)

```bash
# Apply +3 steps to left channel only
mp3rgain -l 0 3 song.mp3

# Apply -2 steps to right channel only
mp3rgain -l 1 -2 song.mp3
```

### Dry-run mode

```bash
# Preview changes without modifying files
mp3rgain -n -g 2 *.mp3
mp3rgain --dry-run -r *.mp3
```

### Find maximum amplitude

```bash
# Only show max amplitude (no gain applied)
mp3rgain -x song.mp3
```

### Modify suggested gain

```bash
# Apply track gain with +2 step offset
mp3rgain -r -m 2 *.mp3

# Apply album gain with -1 step offset
mp3rgain -a -m -1 *.mp3
```

### Tag management

```bash
# Check stored tag info
mp3rgain -s c *.mp3

# Delete stored ReplayGain tags
mp3rgain -s d *.mp3

# Skip existing tags and force recalculation
mp3rgain -s r -r *.mp3
```

### Output formats

```bash
# Output in JSON format for scripting
mp3rgain -o json song.mp3

# Tab-separated output (database-friendly)
mp3rgain -o tsv *.mp3

# Combine with other options
mp3rgain -o json -r *.mp3
```

### Safe file writing

```bash
# Use temp file for atomic writes (safer)
mp3rgain -t -g 2 song.mp3

# Apply gain with wrapping (for edge cases)
mp3rgain -w -g 10 song.mp3
```

Example JSON output:
```json
{
  "files": [
    {
      "file": "song.mp3",
      "mpeg_version": "MPEG1",
      "channel_mode": "Joint Stereo",
      "frames": 5765,
      "min_gain": 89,
      "max_gain": 217,
      "avg_gain": 168.2,
      "headroom_steps": 38,
      "headroom_db": 57.0
    }
  ]
}
```

## Command-Line Options

| Option | Description |
|--------|-------------|
| `-g <i>` | Apply gain of i steps (each step = 1.5 dB) |
| `-d <n>` | Apply gain of n dB (rounded to nearest step) |
| `-l <c> <g>` | Apply gain to left (0) or right (1) channel only |
| `-m <i>` | Modify suggested gain by integer i |
| `-r` | Apply Track gain (ReplayGain analysis) |
| `-a` | Apply Album gain (ReplayGain analysis) |
| `-e` | Skip album analysis (even with multiple files) |
| `-i <n>` | Specify which audio track to process (default: 0) |
| `-u` | Undo gain changes (restore from APEv2 tag) |
| `-x` | Only find max amplitude of file |
| `-s <mode>` | Stored tag handling: `c` (check), `d` (delete), `s` (skip), `r` (recalc), `i` (ID3v2), `a` (APEv2) |
| `-p` | Preserve original file timestamp |
| `-c` | Ignore clipping warnings |
| `-k` | Prevent clipping (automatically limit gain) |
| `-w` | Wrap gain values (instead of clamping 0-255) |
| `-t` | Use temp file for writing (safer atomic writes) |
| `-f` | Assume MPEG 2 Layer III (compatibility flag) |
| `-q` | Quiet mode (less output) |
| `-R` | Process directories recursively |
| `-n`, `--dry-run` | Dry-run mode (show what would be done) |
| `-o <fmt>` | Output format: `text` (default), `json`, or `tsv` |
| `-v` | Show version |
| `-h` | Show help |

### mp3gain Compatibility

mp3rgain is a drop-in replacement for the original mp3gain:

```bash
# These commands work identically in both mp3gain and mp3rgain
mp3gain -r *.mp3           # original mp3gain
mp3rgain -r *.mp3          # mp3rgain

mp3gain -a *.mp3           # original mp3gain  
mp3rgain -a *.mp3          # mp3rgain

mp3gain -g 2 song.mp3      # original mp3gain
mp3rgain -g 2 song.mp3     # mp3rgain
```

## Technical Details

### Gain Steps

Each gain step equals **1.5 dB** (fixed by MP3 specification). The `global_gain` field is 8 bits, allowing values 0-255.

| Steps | dB Change |
|-------|-----------|
| +1 | +1.5 dB |
| +2 | +3.0 dB |
| +4 | +6.0 dB |
| -2 | -3.0 dB |

### How It Works

MP3 files contain a `global_gain` field in each frame's side information that controls playback volume. mp3rgain directly modifies these values without touching the audio data, making the adjustment completely lossless and reversible.

### ReplayGain Analysis

mp3rgain uses the [symphonia](https://github.com/pdrat/symphonia) crate for audio decoding and implements the ReplayGain 1.0 algorithm:

1. Decode MP3/AAC to PCM audio
2. Apply equal-loudness filter (Yule-Walker + Butterworth)
3. Calculate RMS loudness in 50ms windows
4. Use 95th percentile for loudness measurement
5. Calculate gain to reach 89 dB reference level

### AAC/M4A Support

For AAC/M4A files, mp3rgain:
- Analyzes audio loudness using the same ReplayGain algorithm as MP3
- Writes ReplayGain tags in iTunes freeform format (`com.apple.iTunes:replaygain_*`)
- Does NOT modify audio data (AAC lacks a lossless gain mechanism)

Players that support ReplayGain tags will automatically apply volume normalization during playback.

### Compatibility

- MPEG1 Layer III (MP3)
- MPEG2 Layer III
- MPEG2.5 Layer III
- AAC/M4A (ReplayGain tags only)
- Mono, Stereo, Joint Stereo, Dual Channel
- ID3v2 tags (preserved)
- APEv2 tags (for undo support)
- iTunes metadata (for M4A files)
- VBR and CBR files

## Why mp3rgain?

The original [mp3gain](http://mp3gain.sourceforge.net/) has been unmaintained since ~2015 and has compatibility issues with modern systems (including Windows 11). mp3rgain is a modern replacement that:

- Works on Windows 11, macOS, and Linux
- Has no runtime dependencies
- Is written in memory-safe Rust
- Uses the same command-line interface
- Includes a library API for integration

## Library Usage

```rust
use mp3rgain::{apply_gain, apply_gain_db, analyze};
use std::path::Path;

// Apply +2 gain steps (+3.0 dB)
let frames = apply_gain(Path::new("song.mp3"), 2)?;
println!("Modified {} frames", frames);

// Analyze file
let info = analyze(Path::new("song.mp3"))?;
println!("Headroom: {} steps", info.headroom_steps);
```

## Acknowledgments

- [symphonia](https://github.com/pdeljanov/Symphonia) - Pure Rust audio decoding library (used for ReplayGain analysis)
- [Original mp3gain](http://mp3gain.sourceforge.net/) - The original C implementation that inspired this project
- [aacgain](https://github.com/dgilman/aacgain) - AAC/MP4 ReplayGain implementation

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

We especially welcome:
- Windows testing and compatibility reports
- Bug reports and feature requests

## License

MIT License - see [LICENSE](LICENSE) for details.

## See Also

- [Original mp3gain](http://mp3gain.sourceforge.net/) - The original C implementation
- [headroom](https://github.com/M-Igashi/headroom) - DJ audio loudness optimizer (uses mp3rgain internally)
