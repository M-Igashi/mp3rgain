# mp3rgain Use Cases

Real-world uses for mp3rgain, from library management to game assets and in-store audio.

## Lossless AAC volume adjustment from the command line

As far as we know, mp3rgain is the only actively maintained command-line tool that runs a ReplayGain analysis on AAC/M4A files and then applies the gain losslessly by rewriting `global_gain`, the way aacgain used to: tags *and* baked-in gain. ([foobar2000](https://www.foobar2000.org/), the reference-grade ReplayGain suite, has a comparable "Apply ReplayGain to file content" for AAC in MP4/MKA. If you work in a desktop GUI on Windows, use it. It has no command line and no undo.)

Where a scriptable, cross-platform option helps:

- **iTunes / Apple Music libraries on macOS or Linux.** Most of these libraries are AAC/M4A.
- **DJ workflows with AAC.** DJ hardware (CDJs, XDJs, controllers) ignores ReplayGain tags, so the gain has to be in the audio. Without a lossless option, a purchased M4A has to be re-encoded to change its level.
- **Smart speakers and car audio.** The same problem: these devices do not read ReplayGain tags.
- **Archiving.** Volume changes are reversible with `-u`.
- **Headless, batch, CI and Docker pipelines**, where a desktop GUI is not an option.

For MP3, mp3rgain is one of several mp3gain replacements.

## Projects using mp3rgain

### beets

[beets](https://beets.io/) is a command-line music library manager. Its ReplayGain plugin's command backend supports mp3rgain as a replacement for mp3gain/aacgain ([PR #6289](https://github.com/beetbox/beets/pull/6289)).

Install mp3rgain, then enable the plugin in `~/.config/beets/config.yaml`:

```yaml
plugins: replaygain

replaygain:
    backend: command
    command: mp3rgain
```

```bash
beet import /path/to/music/              # import with ReplayGain analysis
beet replaygain                          # update the whole library
beet replaygain album:"Album Name"       # update one album
```

What mp3rgain brings to beets:

- **mp3gain's command line and output**, so the backend needs no changes
- **MP3 and AAC** in one tool, as aacgain offered
- **Current platforms**: Windows 11, macOS on Apple Silicon, Linux x86_64 and arm64
- **Rust instead of C**: none of the C code behind mp3gain's and aacgain's CVEs is used (see [security.md](security.md))

See the [beets ReplayGain plugin documentation](https://beets.readthedocs.io/en/latest/plugins/replaygain.html) for details.

### Bake'n Deck (baken): rekordbox → CDJ prep toolkit

[Bake'n Deck](https://baken.ravers.workers.dev) ([GitHub](https://github.com/M-Igashi/baken), called `headroom` before v3.0.0) writes rekordbox prep into the audio files so it survives the USB export to CDJs. rekordbox Auto Gain is never written into exported files, so a CDJ never sees it; the loudness tool, `baken headroom`, instead brings each track to a uniform true peak ceiling (-0.5 dBTP by default) with gain only and no limiter. The toolkit also sorts rekordbox playlists by key and BPM (`baken rbsort`) and builds a CDJ-safe MP3 backup (`baken cdjsafe`).

Bake'n Deck embeds mp3rgain as a library:

- **Native lossless gain, up or down**: MP3 and AAC/M4A files move in whole 1.5 dB `global_gain` steps; nothing is re-encoded
- **AAC bitstream gain** through the same library call as MP3
- **In-process measurement**: since baken 3.6.0, loudness and true peak come from mp3rgain's BS.1770-4 analyzer instead of ffmpeg `loudnorm`, about 15x faster per file

```
# baken headroom's processing approach:
1. Native lossless (mp3rgain)  - MP3 / AAC, whole 1.5 dB steps up or down
2. Exact gain (ffmpeg)         - FLAC / WAV / AIFF / ALAC, source bit depth kept
3. Skip                        - MP3 / AAC already within one step of the ceiling
```

```bash
brew install M-Igashi/tap/baken    # macOS
winget install M-Igashi.baken      # Windows
yay -S baken-bin                   # Arch Linux
cargo install baken                # All platforms (ffmpeg required)
```

Bake'n Deck for Mac, the native app edition, is on the [Mac App Store](https://apps.apple.com/app/baken-deck/id6808813823).

## Typical workflows

### Game development

Game developers often use mp3gain to prepare audio assets: BGM and sound effects collected from different free asset sites vary a lot in level, and players notice the jumps between tracks. mp3rgain batch-normalizes them to one ReplayGain level (89 dB by default) without re-encoding.

Engines whose projects benefit from pre-normalized audio include RPG Maker (MV, MZ), WOLF RPG Editor, Unity, Godot, Ren'Py and TyranoBuilder.

```bash
# Show each file's level and recommended change (read-only)
mp3rgain ./assets/bgm/*.mp3

# Normalize BGM and sound effects before importing them into the engine
mp3rgain -r -R ./assets/bgm/
mp3rgain -r -R ./assets/se/
```

Why it fits: the change is lossless and reversible (`-u`), it handles large asset folders in one run, and it runs on Windows, macOS and Linux.

### Podcast production

Segments from different contributors arrive at different levels. Track gain brings each one to the same level:

```bash
# Preview the gain each segment would get
mp3rgain -r -n -o json episode_01_*.mp3 | jq -r '.files[] | "\(.file)\t\(.gain_applied_db)"'

# Apply it
mp3rgain -r episode_01_*.mp3
```

### Music library management

```bash
# Track gain for a whole library
mp3rgain -r -R ~/Music/

# Album gain for a whole library, one album per release (from tags)
mp3rgain -a --album-by=tag -R ~/Music/

# Album gain for one album
mp3rgain -a ~/Music/Artist/Album/*.mp3
```

Plain `-a` treats every file given as one album, so use `--album-by` or `--album-depth` when scanning more than one album at a time.

### DJ preparation

DJ hardware ignores ReplayGain tags, so gain has to be baked into the file to reach the deck. For a DJ library played from CDJs, [Bake'n Deck](https://baken.ravers.workers.dev) (above) is the purpose-built option: a true peak ceiling instead of a ReplayGain target, FLAC/AIFF/WAV support, and rekordbox cues kept linked. For a quick MP3/AAC-only pass, mp3rgain alone works:

```bash
# Preview, then apply track gain without letting any track clip
mp3rgain -r -k -n -R ~/Music/DJ-Sets/
mp3rgain -r -k -R ~/Music/DJ-Sets/
```

### Audio archiving

```bash
# Preview the changes
mp3rgain -r -R -n /archive/audio/

# Apply them and keep a JSON log
mp3rgain -r -R -o json /archive/audio/ > normalization_log.json
```

### Commercial audio and digital signage

Retail stores, restaurants, hotels and signage systems need a consistent level, and their players rarely help:

1. **Playback devices ignore ReplayGain tags.** Embedded systems, simple media players and PA systems do not apply metadata-based volume adjustment.
2. **Jarring volume changes disrupt the customer experience.**
3. **Chains need the same audio at every site.**
4. **Nobody configures ReplayGain on hundreds of devices.**

From an [r/DataHoarder discussion](https://www.reddit.com/r/DataHoarder/):

> "I setup PAs for a national store years ago with raspberry pies in front of the amp. We normalized all mp3s first. In a store you'll notice the shopping 'volume' is always the same."

```bash
# On the central server: normalize all store music
mp3rgain -r -R /nas/store-music/

# Deploy to the Raspberry Pi players
for store in store-{001..100}; do
    rsync -av /nas/store-music/ pi@${store}:/music/
done
```

Why it fits:

- Works with any playback device or software, since the level is in the audio
- Runs on a Raspberry Pi 4/5 with a 64-bit OS: Linux arm64 release binaries, `.deb` packages and a Docker image are provided
- Changes are reversible if needed

## Integration examples

### Shell scripts

```bash
#!/bin/bash
# normalize_new_audio.sh - process audio files added in the last day

MUSIC_DIR="$HOME/Music"
LOG_FILE="$HOME/.mp3rgain_log"

find "$MUSIC_DIR" -name "*.mp3" -mtime -1 -print0 | \
    xargs -0 mp3rgain -r -o json >> "$LOG_FILE"
```

### CI/CD pipelines

```yaml
# GitHub Actions example (mp3rgain installed on the runner)
- name: Normalize audio assets
  run: mp3rgain -r -R -o json ./assets/audio/ > audio_report.json
```

### Containerized batch normalization (Plex / cron)

A common mp3gain setup is a Docker container triggered by cron during a media-server maintenance window, adjusting newly added tracks automatically. mp3rgain's official image on GHCR drops into the same setup. It is published for `linux/amd64` and `linux/arm64` and contains a single static musl binary (no shell, no runtime dependencies):

```bash
docker pull ghcr.io/m-igashi/mp3rgain:latest
```

Host crontab entry, running at 03:00 daily during Plex maintenance (crontab entries must stay on one line):

```
0 3 * * * docker run --rm --user 1000:1000 -v /srv/plex/music:/music ghcr.io/m-igashi/mp3rgain:v3 -r -R /music
```

The image is tagged `latest`, `vX` and `vX.Y.Z`. Pin the major tag (`:v3`) to receive minor and patch releases without breaking changes, or an exact version (`:v3.9.2`) for fully reproducible runs. `--user` makes the rewritten files belong to that UID instead of root.

The image's entrypoint is `mp3rgain` itself, so every flag works as on the host: `-r`, `-a`, `-R`, `-k`, `-u`, `-o json` and so on.

### Rust projects

The library behind the CLI is on [crates.io](https://crates.io/crates/mp3rgain). `apply::apply_with_options` is the pipeline the CLI and GUI share: an atomic write plus the undo tag, for MP3 and AAC alike.

```rust
use mp3rgain::apply::{apply_with_options, ApplyOptions};
use std::path::Path;

/// Lower every supported file in `dir` by `steps` (1 step = 1.5 dB), with undo info.
fn lower_all(dir: &Path, steps: i32) -> Result<(), Box<dyn std::error::Error>> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if mp3rgain::is_supported_audio_path(&path) {
            let report = apply_with_options(&path, &ApplyOptions::new(-steps))?;
            println!("{}: {} step(s) applied", path.display(), report.actual_steps);
        }
    }
    Ok(())
}
```

API documentation: [docs.rs/mp3rgain](https://docs.rs/mp3rgain).

## References

- [Qiita: Adjusting Volume of Multiple Audio Files (MP3Gain)](https://qiita.com/qesulive/items/1e71886e891f6aaa3912): game development use case
- [MP3Gain for Unifying Audio Volume](https://www.stmn.tech/entry/2019/10/06/011544): app development workflow
- [About Volume Adjustment](https://studio-sunny-side.hatenablog.com/entry/20130122/1358821078): industry standards discussion
