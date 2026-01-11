# mp3rgain

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-blue.svg)](https://www.rust-lang.org)
[![crates.io](https://img.shields.io/crates/v/mp3rgain.svg)](https://crates.io/crates/mp3rgain)

**Lossless MP3 volume adjustment - a modern mp3gain replacement written in Rust**

mp3rgain adjusts MP3 volume without re-encoding by modifying the `global_gain` field in each frame's side information. This preserves audio quality while achieving permanent volume changes.

## Features

- **Lossless**: No re-encoding, preserves original audio quality
- **Fast**: Direct binary manipulation, no audio decoding required
- **Reversible**: All changes can be undone
- **Zero dependencies**: Single static binary (no ffmpeg, no mp3gain)
- **Cross-platform**: macOS, Linux, Windows (x86_64 and ARM64)
- **mp3gain compatible**: Same command-line interface as the original mp3gain
- **Pure Rust**: Memory-safe implementation

## Installation

### Homebrew (macOS)

```bash
brew install M-Igashi/tap/mp3rgain
```

### Cargo (all platforms)

```bash
cargo install mp3rgain
```

### Download binary

Download the latest release from [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases):
- macOS (Universal): `mp3rgain-*-macos-universal.tar.gz`
- Linux (x86_64): `mp3rgain-*-linux-x86_64.tar.gz`
- Windows (x86_64): `mp3rgain-*-windows-x86_64.zip`
- Windows (ARM64): `mp3rgain-*-windows-arm64.zip`

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

### Undo previous adjustment

To undo a previous gain change, apply the inverse:

```bash
# Undo a +2 step adjustment
mp3rgain -g -2 song.mp3
```

## Command-Line Options

| Option | Description |
|--------|-------------|
| `-g <i>` | Apply gain of i steps (each step = 1.5 dB) |
| `-d <n>` | Apply gain of n dB (rounded to nearest step) |
| `-s c` | Check/show file info (analysis only) |
| `-p` | Preserve original file timestamp |
| `-c` | Ignore clipping warnings |
| `-q` | Quiet mode (less output) |
| `-v` | Show version |
| `-h` | Show help |

### mp3gain Compatibility

mp3rgain uses the same command-line syntax as the original mp3gain:

```bash
# These commands work the same way in both mp3gain and mp3rgain
mp3gain -g 2 song.mp3      # original mp3gain
mp3rgain -g 2 song.mp3     # mp3rgain (drop-in replacement)
```

**Not yet implemented:**
- `-r` (Track gain) - requires ReplayGain analysis
- `-a` (Album gain) - requires ReplayGain analysis  
- `-u` (Undo from tags) - requires APEv2 tag support

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

### Compatibility

- MPEG1 Layer III (MP3)
- MPEG2 Layer III
- MPEG2.5 Layer III
- Mono, Stereo, Joint Stereo, Dual Channel
- ID3v2 tags (preserved)
- VBR and CBR files

## Why mp3rgain?

The original [mp3gain](http://mp3gain.sourceforge.net/) has been unmaintained since ~2015 and has compatibility issues with modern systems (including Windows 11). mp3rgain is a modern replacement that:

- Works on Windows 11, macOS, and Linux
- Has no external dependencies
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

## Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

We especially welcome:
- Windows testing and compatibility reports
- ReplayGain analysis implementation
- Bug reports and feature requests

## License

MIT License - see [LICENSE](LICENSE) for details.

## See Also

- [Original mp3gain](http://mp3gain.sourceforge.net/) - The original C implementation
- [headroom](https://github.com/M-Igashi/headroom) - DJ audio loudness optimizer (uses mp3rgain internally)
