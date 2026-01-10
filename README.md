# mp3rgain

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-blue.svg)](https://www.rust-lang.org)

**Lossless MP3 volume adjustment - a modern mp3gain replacement written in Rust**

mp3rgain adjusts MP3 volume without re-encoding by modifying the `global_gain` field in each frame's side information. This preserves audio quality while achieving permanent volume changes.

## Features

- 🎵 **Lossless**: No re-encoding, preserves original audio quality
- ⚡ **Fast**: Direct binary manipulation, no audio decoding required
- 🔄 **Reversible**: All changes can be undone
- 📦 **Zero dependencies**: Single static binary (no ffmpeg, no mp3gain)
- 🦀 **Pure Rust**: Memory-safe, cross-platform

## Installation

### Homebrew (macOS)

```bash
brew install M-Igashi/tap/mp3rgain
```

### From source

```bash
cargo install mp3rgain
```

### Download binary

Download the latest release from [GitHub Releases](https://github.com/M-Igashi/mp3rgain/releases).

## Usage

### Apply gain adjustment

```bash
# Apply +2 steps (+3.0 dB)
mp3rgain apply -g 2 song.mp3

# Apply +4.5 dB (rounds to nearest step)
mp3rgain apply -d 4.5 song.mp3

# Reduce volume by 3 steps (-4.5 dB)
mp3rgain apply -g -3 *.mp3
```

### Show file information

```bash
mp3rgain info song.mp3
```

Output:
```
song.mp3
  Format:      MPEG1 Layer III, Joint Stereo
  Frames:      5765
  Gain range:  89 - 217 (avg: 168.2)
  Headroom:    38 steps (+57.0 dB)
```

### Undo previous adjustment

```bash
# Undo a +2 step adjustment
mp3rgain undo -g 2 song.mp3
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

### Compatibility

- MPEG1 Layer III (MP3)
- MPEG2 Layer III
- MPEG2.5 Layer III
- Mono, Stereo, Joint Stereo, Dual Channel
- ID3v2 tags (preserved)
- VBR and CBR files

## Why mp3rgain?

The original [mp3gain](http://mp3gain.sourceforge.net/) has been unmaintained since ~2015. mp3rgain is a modern replacement that:

- Is actively maintained
- Has no external dependencies
- Is written in memory-safe Rust
- Provides a clean, modern CLI
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

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please open an issue or submit a pull request.

## See Also

- [headroom](https://github.com/M-Igashi/headroom) - DJ audio loudness optimizer (uses mp3rgain internally)
