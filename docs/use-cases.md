# mp3rgain Use Cases

This document describes real-world use cases for mp3rgain.

## Projects Using mp3rgain

### beets - Music Library Manager

[beets](https://beets.io/) is a popular command-line music library manager with a large user base. The ReplayGain plugin's command backend now supports mp3rgain as a secure, modern alternative to mp3gain/aacgain.

- **GitHub**: [beetbox/beets](https://github.com/beetbox/beets)
- **Integration**: [PR #6289](https://github.com/beetbox/beets/pull/6289)
- **Documentation**: [ReplayGain plugin docs](https://beets.readthedocs.io/en/latest/plugins/replaygain.html)

### headroom - DJ Audio Loudness Optimizer

[headroom](https://github.com/M-Igashi/headroom) is an audio loudness analyzer and gain adjustment tool designed for mastering and DJ workflows. It simulates Rekordbox's Auto Gain feature but with a key difference: it identifies files with available headroom and applies gain adjustment without using a limiter.

**How it uses mp3rgain:**

headroom includes mp3rgain as a built-in library dependency for lossless MP3 volume adjustment. This enables:

- **Native lossless gain**: For MP3 files with sufficient headroom (≥1.5 dB), headroom uses the mp3rgain library to directly modify the `global_gain` field in MP3 frames
- **Zero external dependencies**: No need to install mp3gain separately
- **Bitrate-aware processing**: Automatically selects appropriate True Peak ceiling based on bitrate

```
# headroom's three-tier MP3 processing approach:
1. Native Lossless (mp3rgain) - For files with ≥1.5 dB headroom
2. Re-encode (ffmpeg) - For files needing precise gain <1.5 dB
3. Skip - Files already at target ceiling
```

**Installation:**
```bash
brew install M-Igashi/tap/headroom    # macOS
winget install M-Igashi.headroom      # Windows
cargo install headroom                # All platforms
```

---

## Expected Use Cases

### Game Development

Game developers frequently use mp3gain for audio asset preparation. Games typically use MP3 format for background music and sound effects due to its compression efficiency and broad compatibility.

**Common scenarios:**

1. **Normalizing free audio assets**
   - When collecting BGM and SE from multiple free asset sites, volume levels vary significantly
   - mp3rgain can batch-normalize all audio files to a consistent level (e.g., 89 dB)
   - This ensures players don't experience jarring volume differences between tracks

2. **Game engines that benefit from pre-normalized audio:**
   - RPG Maker (MV, MZ)
   - WOLF RPG Editor
   - Unity (for MP3 assets)
   - Godot
   - Ren'Py (visual novels)
   - TyranoBuilder

3. **Workflow integration**
   ```bash
   # Normalize all BGM files before importing into game engine
   mp3rgain -r -R ./assets/bgm/
   
   # Normalize sound effects
   mp3rgain -r -R ./assets/se/
   
   # Check current levels without modifying
   mp3rgain -n ./assets/bgm/*.mp3
   ```

**Why mp3rgain over other tools:**
- Lossless adjustment preserves audio quality (critical for game assets)
- Reversible changes allow experimentation
- Batch processing handles large asset libraries efficiently
- Cross-platform support matches game development workflows

### Podcast Production

Podcast producers often receive audio from multiple contributors with varying recording setups and volume levels.

```bash
# Normalize all episode segments
mp3rgain -a episode_01_*.mp3    # Album mode preserves relative dynamics

# Check levels before publishing
mp3rgain -o json *.mp3 | jq '.files[].db_gain'
```

### Music Library Management

For users with large music collections from various sources (CDs, downloads, streaming rips), volume inconsistency is a common problem.

```bash
# Normalize entire music library
mp3rgain -r -R ~/Music/

# Album-aware normalization (preserves album dynamics)
cd ~/Music/Artist/Album/
mp3rgain -a *.mp3
```

### DJ Preparation

DJs need consistent volume levels across tracks for smooth mixing. headroom (mentioned above) demonstrates this use case with additional features like True Peak analysis.

```bash
# Quick normalization for DJ sets
mp3rgain -r -R ~/Music/DJ-Sets/

# Check which files need adjustment
mp3rgain -n ~/Music/DJ-Sets/*.mp3
```

### Audio Archiving

Data hoarders and archivists often need to normalize audio files for long-term storage or distribution.

```bash
# Recursive processing with JSON output for logging
mp3rgain -r -R -o json /archive/audio/ > normalization_log.json

# Dry-run to preview changes
mp3rgain -r -R -n /archive/audio/
```

### Commercial Audio / Digital Signage

For retail stores, restaurants, hotels, and digital signage systems where consistent audio volume is critical for customer experience.

**Why pre-normalization matters in commercial settings:**

1. **Playback devices don't support ReplayGain tags** - Embedded systems, simple media players, and PA systems typically ignore metadata-based volume adjustment
2. **Consistent volume is business-critical** - Jarring volume changes disrupt the customer experience
3. **Multiple locations need identical audio** - Chain stores, franchises need consistency across sites
4. **No per-device configuration** - IT teams can't configure ReplayGain on hundreds of devices

**Real-world example** (from [r/DataHoarder discussion](https://www.reddit.com/r/DataHoarder/)):

> "I setup PAs for a national store years ago with raspberry pies in front of the amp. We normalized all mp3s first. In a store you'll notice the shopping 'volume' is always the same."

**Workflow for retail/commercial deployment:**

```bash
# On central server: normalize all store music
mp3rgain -r -R /nas/store-music/

# Deploy to Raspberry Pi players
rsync -av /nas/store-music/ pi@store-001:/music/

# Or batch deploy to multiple locations
for store in store-{001..100}; do
    rsync -av /nas/store-music/ pi@${store}:/music/
done
```

**Platforms:**

- Raspberry Pi (ARM64 supported)
- Embedded Linux systems
- Digital signage players
- Simple MP3 playback devices
- PA systems with USB/SD input

**Benefits:**

- Works with any playback software (no ReplayGain support needed)
- Single static binary runs on ARM64 (Raspberry Pi 4/5)
- No runtime dependencies
- Consistent volume across all locations
- Changes are reversible if needed

### beets - Music Library Manager

[beets](https://beets.io/) is a popular command-line music library manager that organizes and manages your music collection. Its ReplayGain plugin now supports mp3rgain as a backend for volume normalization.

**How to use mp3rgain with beets:**

1. Install mp3rgain on your system
2. Enable the ReplayGain plugin in your beets config (`~/.config/beets/config.yaml`):

```yaml
plugins: replaygain

replaygain:
    backend: command
    command: mp3rgain
```

**Benefits of using mp3rgain with beets:**

- **Security**: mp3rgain is memory-safe (Rust) and not affected by mp3gain CVEs including CVE-2023-49356
- **Modern support**: Works on Windows 11 and macOS with Apple Silicon
- **CLI compatible**: Same command-line interface as mp3gain
- **MP3 + AAC support**: Handles both formats like aacgain

**Workflow:**

```bash
# Import music with ReplayGain analysis
beet import /path/to/music/

# Update ReplayGain for existing library
beet replaygain

# Update specific album
beet replaygain album:"Album Name"
```

For more details, see the [beets ReplayGain plugin documentation](https://beets.readthedocs.io/en/latest/plugins/replaygain.html).

---

## Integration Examples

### Shell Scripts

```bash
#!/bin/bash
# normalize_new_audio.sh - Process newly added audio files

MUSIC_DIR="$HOME/Music"
LOG_FILE="$HOME/.mp3rgain_log"

find "$MUSIC_DIR" -name "*.mp3" -mtime -1 -print0 | \
    xargs -0 mp3rgain -r -o json >> "$LOG_FILE"
```

### CI/CD Pipelines

For game development or podcast production pipelines:

```yaml
# GitHub Actions example
- name: Normalize audio assets
  run: |
    mp3rgain -r -R ./assets/audio/
    mp3rgain -n -o json ./assets/audio/ > audio_report.json
```

### Rust Projects

```rust
use mp3rgain::{apply_gain, analyze};
use std::path::Path;

fn normalize_game_audio(audio_dir: &Path) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(audio_dir)? {
        let path = entry?.path();
        if path.extension().map_or(false, |e| e == "mp3") {
            let info = analyze(&path)?;
            if info.headroom_steps > 0 {
                apply_gain(&path, info.headroom_steps)?;
            }
        }
    }
    Ok(())
}
```

---

## References

- [Qiita: Adjusting Volume of Multiple Audio Files (MP3Gain)](https://qiita.com/qesulive/items/1e71886e891f6aaa3912) - Game development use case
- [MP3Gain for Unifying Audio Volume](https://www.stmn.tech/entry/2019/10/06/011544) - App development workflow
- [About Volume Adjustment](https://studio-sunny-side.hatenablog.com/entry/20130122/1358821078) - Industry standards discussion
