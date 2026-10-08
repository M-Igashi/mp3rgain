# mp3rgain vs aacgain and mp3gain

How mp3rgain compares with the tools it replaces, mp3gain and aacgain, and where tag-only ReplayGain tools fit.

> **In short:** as far as we know, mp3rgain is the only actively maintained command-line tool that adjusts AAC/M4A volume losslessly by rewriting `global_gain`. aacgain did this too, but its last release is 2.0.0 and its repository has had no commits since 2022; rsgain and loudgain write ReplayGain tags only, and FFmpeg re-encodes. foobar2000's "Apply ReplayGain to file content" does a comparable lossless AAC adjustment in MP4/MKA from its desktop GUI on Windows, without an undo path. **On a Windows desktop, foobar2000 is the one to reach for**; from a script, a container or a non-Windows host, mp3rgain is.

> **A note on framing:** every tool discussed here is a ReplayGain tool, mp3rgain included. The mp3gain lineage (mp3gain, aacgain, mp3rgain) runs a ReplayGain analysis, writes the standard `REPLAYGAIN_*` tags, *and* bakes the correction into the bitstream; foobar2000 can do both as well. "Tagger vs gain tool" is the wrong axis. The axis that matters is whether a tool can touch the bitstream at all, and whether it can put it back.

## Overview

| | mp3rgain | aacgain | mp3gain |
|---|----------|---------|---------|
| **Language** | Rust | C | C |
| **Latest release** | 3.9.2 (2026) | 2.0.0 | 1.6.2 |
| **License** | MIT | LGPL | LGPL |
| **Repository** | [M-Igashi/mp3rgain](https://github.com/M-Igashi/mp3rgain) | [dgilman/aacgain](https://github.com/dgilman/aacgain) | SourceForge |

## Supported formats

| Format | mp3rgain | aacgain | mp3gain |
|--------|----------|---------|---------|
| MP3 (MPEG-1, MPEG-2, MPEG-2.5 Layer III) | Yes | Yes | Yes |
| AAC in MP4/M4A | Yes (lossless) | Yes (lossless) | No |
| AAC, raw ADTS `.aac` | Yes (lossless, since 3.7.0) | No | No |
| HE-AAC (SBR) | AAC core only, untested (see below) | No | No |
| Apple Lossless (ALAC) | No (skipped) | No | No |

Notes on mp3rgain:

- **AAC** gain has been lossless since 2.0.0: like aacgain, mp3rgain rewrites the `global_gain` field of each channel element. The audio track of a video `.mp4` is handled since 3.7.0 ([#327](https://github.com/M-Igashi/mp3rgain/issues/327)).
- **Raw ADTS** streams (from `ffmpeg -f adts`, DVB/HLS captures and some rippers) have no container for metadata, so their undo and `REPLAYGAIN_*` values go into an ID3v2 tag ([#330](https://github.com/M-Igashi/mp3rgain/issues/330)).
- **HE-AAC:** the gain is applied to the AAC core and the SBR extension data is left as it is. There is no HE-AAC test file in the test suite, so treat the result as unverified.
- **ALAC and DRM-protected M4P** files are reported as skipped and do not fail the run (since 3.7.0).
- **FLAC, Ogg, Opus and WAV** are not supported: passed by name they fail with an error, and `-R` ignores them.

## Command-line options

mp3rgain accepts every mp3gain option except `-T` (modify in place), which it ignores with a warning, so existing command lines keep working. Where the behaviour differs, the table says so; [migrating-from-mp3gain.md](migrating-from-mp3gain.md) has the full list of differences.

| Option | Description | Notes on mp3rgain |
|--------|-------------|-------------------|
| `-g <i>` | Apply gain of `i` steps | Audio frames byte-identical to mp3gain's |
| `-d <n>` | Modify suggested dB gain by `n` | Rounded to whole 1.5 dB steps |
| `-m <i>` | Modify suggested gain by `i` steps | |
| `-r` | Apply track gain | |
| `-a` | Apply album gain | Every file given is one album, as in mp3gain |
| `-e` | Skip album analysis | On its own, applies track gain (mp3gain only analyzes) |
| `-l <c> <g>` | Channel-specific gain | MP3 only |
| `-u` | Undo gain changes | Also removes the `REPLAYGAIN_*` values |
| `-x` | Find max amplitude only | |
| `-k` | Prevent clipping | With `-g`, only stops `global_gain` from exceeding 255 |
| `-c` | Ignore clipping warnings | mp3rgain never prompts; it warns and applies |
| `-p` | Preserve file timestamp | |
| `-q` | Quiet mode | |
| `-w` | Wrap gain values | |
| `-t` | Use temp file for writing | No effect: always on |
| `-f` | Assume MPEG 2 Layer III | No effect |
| `-s c` / `-s d` | Check / delete stored tag info | |
| `-s s` | Skip stored tag info | No undo or ReplayGain tags are written |
| `-s r` | Force recalculation | Already the default |
| `-s i` / `-s a` | Store tags in ID3v2 / APEv2 | Default since 3.2.0: ReplayGain in ID3v2, undo in APEv2 |
| `-v` / `-h` | Show version / help | |

### mp3rgain extensions

Options mp3gain and aacgain do not have:

- `-R`: recurse into directories
- `--album-by dir|tag`, `--per-directory`, `--album-depth <n>`: album-tag a whole library in one run, with one album per directory or per release (from tags) instead of one album for every file given
- `-s R`: reuse stored ReplayGain tags and rescan only files without them (mp3gain's *default*, opt-in here)
- `--rg2` / `--r128`, `--true-peak`: BS.1770 loudness at -18 or -23 LUFS, and true-peak measurement
- `--tags-only`: write `REPLAYGAIN_*` tags without touching the audio
- `-n` / `--dry-run`: preview without writing
- `-o json`, `-o tsv`: JSON output, and an explicit name for mp3gain's tab-separated output
- `-j <n>` / `--threads <n>`: parallel processing
- `--skip-errors`: keep an album scan going past undecodable files
- A progress bar for 5 or more files

## Technical comparison

### ReplayGain implementation

| Aspect | mp3rgain | aacgain / mp3gain |
|--------|----------|-------------------|
| Algorithm | ReplayGain 1.0 by default; ITU-R BS.1770 with `--rg2` / `--r128` (since 3.0.0) | ReplayGain 1.0 |
| Reference level | 89 dB (RG1), -18 LUFS (`--rg2`), -23 LUFS (`--r128`) | 89 dB |
| RG1 window / statistic | 50 ms RMS windows, 95th percentile | 50 ms RMS windows, 95th percentile |
| RG1 equal-loudness filter | Yule-Walker + Butterworth | Yule-Walker + Butterworth |
| MP3 decoding | symphonia (Rust) | mpglib (C) |
| AAC decoding | symphonia (Rust) | faad2 (C) |

Since 1.2.6, mp3rgain's ReplayGain 1.0 analysis uses the filter coefficients from the original specification, so its values match mp3gain's and aacgain's. The analysis is golden-tested against the reference C `gain_analysis.c`.

### Peak values on AAC, compared with rsgain and foobar2000

mp3rgain can report a noticeably higher peak than rsgain or foobar2000 for the same AAC file, while the gain values agree to 0.01 dB. This is expected: the tools are measuring different things.

Lossy AAC decodes to samples that can exceed full scale. Some encoders leave isolated bursts that decode well above 1.0, and **decoders disagree about what to do with them**: symphonia (which mp3rgain uses) and ffmpeg's native decoder pass them through, while Apple's AudioToolbox decoder hard-limits at exactly 1.0. mp3rgain reports the peak of the decoded signal as it is. A tool measuring a clamped decode necessarily reports less.

Worked example from issue #350, one track of a `ffmpeg -c:a aac -b:a 256k` encode:

| Measurement | Value | dBFS |
|---|---|---|
| mp3rgain sample peak | 1.790314 | +5.06 |
| ffmpeg's decoder, same frame | 1.790310 | +5.06 |
| mp3rgain true peak (`--true-peak`) | 2.287768 | +7.19 |
| ffmpeg `ebur128=peak=true` | (7.3 print) | +7.30 |
| ideal band-limited interpolation | 2.310848 | +7.27 |
| **ideal, after clamping to [-1, 1]** | **1.321509** | **+2.42** |
| rsgain `-t` | 1.307024 | +2.33 |

mp3rgain lands 0.09 dB under the exact unclamped answer and rsgain 0.10 dB under the exact clamped one, which is the residual a 49-tap 4x oversampling filter is expected to leave in both cases.

Why mp3rgain does not clamp: the peak describes the file rather than whichever decoder the listener happens to use, and float playback paths (CoreAudio, WASAPI shared mode, PipeWire, most software players) really do reach the DAC with those samples. Over-reporting costs a little unnecessary attenuation; under-reporting clips.

In practice it rarely changes anything. Applying each track's own gain to the unclamped peak on the album this was reported against gives a worst case of 0.84, so no player applies extra attenuation either way. It matters only on a quiet recording that carries the same kind of burst, where peak-based clipping prevention would cap the gain differently.

If you want the encoder side of this gone, re-encode with Apple's encoder (`ffmpeg -c:a aac_at`): on the same source it dropped that track's peak from 1.790314 to 1.276827.

### Tag storage

| Where | mp3rgain | aacgain | mp3gain |
|-------|----------|---------|---------|
| MP3, APEv2 | Undo and min/max by default; everything with `-s a` | Default | Default |
| MP3, ID3v2 `TXXX` | `REPLAYGAIN_*` by default (since 3.2.0); everything with `-s i` | With `-s i` | With `-s i` |
| AAC in MP4/M4A, iTunes freeform atoms | Yes | Yes | - |
| Raw ADTS, ID3v2 | Yes | - | - |

### Undo information

For MP3, mp3rgain stores undo data the way mp3gain does: `MP3GAIN_UNDO` (the adjustment to reverse) and `MP3GAIN_MINMAX` (the `global_gain` range) in APEv2, or in ID3v2 with `-s i`. Either tool can undo the other's changes.

For AAC in MP4/M4A, mp3rgain uses its own `mp3rgain_undo` and `mp3rgain_minmax` freeform atoms. It does not read aacgain's undo data.

## Platform support

| Platform | mp3rgain | aacgain | mp3gain |
|----------|----------|---------|---------|
| macOS (Intel) | Yes | Build required | Build required |
| macOS (Apple Silicon) | Yes (universal binary) | Build required | Limited |
| Linux (x86_64) | Yes | Build required | Build required |
| Linux (ARM64) | Yes | Build required | Limited |
| Windows (x86_64) | Yes | Binary available | Binary available |
| Windows (ARM64) | Yes | No | No |

## Installing mp3rgain

```bash
# macOS (Homebrew)
brew install M-Igashi/tap/mp3rgain

# Windows (winget)
winget install M-Igashi.mp3rgain

# Ubuntu 26.04 LTS (PPA)
sudo add-apt-repository ppa:m-igashi/mp3rgain && sudo apt install mp3rgain

# Debian/Ubuntu (.deb from the releases page, amd64 and arm64)
sudo apt install ./mp3rgain_*_amd64.deb

# Arch Linux (community-maintained AUR package)
yay -S mp3rgain-bin

# Docker (linux/amd64, linux/arm64)
docker run --rm -v /path/to/music:/music ghcr.io/m-igashi/mp3rgain:latest -r -R /music

# Any platform with Rust
cargo install mp3rgain
```

Prebuilt binaries for macOS (universal), Linux x86_64/arm64 and Windows x86_64/arm64 are on the [releases page](https://github.com/M-Igashi/mp3rgain/releases). Each is a single executable of a few MB with no dependencies beyond the system C library (glibc 2.34 or newer on Linux; the C runtime is linked statically on Windows, and the Docker image holds a static musl binary).

## Migrating

From mp3gain, the binary name is usually the only change; see [migrating-from-mp3gain.md](migrating-from-mp3gain.md) for the few behaviour differences.

From aacgain, the same commands work on M4A files:

```bash
mp3rgain -r *.m4a   # track gain
mp3rgain -a *.m4a   # album gain
mp3rgain -u *.m4a   # undo
```

## Performance

mp3rgain processes files in parallel by default, one worker thread per CPU (`-j` / `--threads`, or `MP3RGAIN_THREADS`). Under `--rg2`/`--r128`, a long MP3 track can also be split across workers when there are fewer files than threads. `-j 1` processes one file at a time like mp3gain. ReplayGain 1.0 results are identical at every thread count. Measurements are in [perf-parallel.md](perf-parallel.md).

## Avoiding double volume adjustment

A tag-aware player applies `REPLAYGAIN_*` on top of whatever is in the audio, so the tags have to describe the audio as it is now:

- **After `-r` or `-a`**, mp3rgain writes the residual gain left after the bitstream change (mp3gain's convention), so a tag-aware player and a tag-blind one end up at the same loudness. Tagging the file again later with another tool is also safe, since it measures the modified audio.
- **After `-g`**, mp3rgain shifts any existing `REPLAYGAIN_*` values by the gain it applied, as mp3gain does, so they keep describing the audio ([#377](https://github.com/M-Igashi/mp3rgain/issues/377)).
- **After `-l`**, existing `REPLAYGAIN_*` tags are left unchanged, as in mp3gain, so they still describe the old audio. Refresh them with `mp3rgain -r --tags-only` (which keeps the undo tag), or remove them with `-s d` (which also removes the undo tag).
- **With `-s s`**, no tags are written, so tags from an earlier scan by another tool go stale the same way.
- **To return to tags only**, undo first; `-u` also removes the `REPLAYGAIN_*` values:

```bash
mp3rgain -u *.mp3               # restore the original global_gain values
mp3rgain -r --tags-only *.mp3   # then write tags only (or use rsgain, loudgain, ...)
```

## AAC volume adjustment: tool landscape

For AAC/M4A the choice of tool matters more than for MP3, because few tools can avoid re-encoding:

| Tool | AAC approach | Writes RG tags? | Lossless? | Player-agnostic? | CLI / scriptable? |
|------|--------------|-----------------|-----------|------------------|-------------------|
| **mp3rgain** | `global_gain` rewrite, reversible | **Yes (RG1 / RG2 / R128)** | **Yes** | **Yes** | **Yes** |
| aacgain | `global_gain` rewrite | Yes (RG1) | Yes | Yes | Yes |
| foobar2000 "Apply ReplayGain to file content" | Scalefactor rewrite (MP4/MKA AAC), no undo | Yes (RG2, reference implementation) | Yes | Yes | No (Windows GUI) |
| rsgain | ReplayGain 2.0 tags | Yes (RG2 / R128) | Yes (file untouched) | No (player must read tags) | Yes |
| loudgain | ReplayGain 2.0 tags | Yes (RG2 / R128) | Yes (file untouched) | No (player must read tags) | Yes |
| FFmpeg `volume` / `loudnorm` | Re-encode | No | No (lossy) | Yes | Yes |
| beets ReplayGain plugin | Tags via backend | Yes (backend-dependent) | Yes (file untouched) | No (player must read tags) | Yes |

Lossless, player-agnostic, reversible and scriptable together is mp3rgain's niche. It matters for DJ equipment, car audio, smart speakers, batch / Docker / CI pipelines, and anywhere the playback device ignores ReplayGain tags or a desktop GUI is not an option.

That is a statement about the intersection, not a ranking. [foobar2000](https://www.foobar2000.org/) covers the lossless and player-agnostic cells and does so well; what it does not offer is a command line, a non-Windows host or an undo path. **On Windows, in a GUI, wanting the most standards-faithful ReplayGain, foobar2000 is the tool to recommend.** It is the closest thing ReplayGain 2.0 has to a reference implementation, and it tags far more formats than mp3rgain does. The two interoperate: mp3rgain writes the standard `REPLAYGAIN_*` tags foobar2000 reads, and `--rg2` is built to reproduce its measurement rather than to offer a rival one.

## When to use global_gain vs ReplayGain tags

| Use case | Recommended approach |
|----------|----------------------|
| DJ equipment (CDJs, controllers) | `global_gain` (mp3rgain) |
| Car stereos, portable players | `global_gain` (mp3rgain) |
| Smart speakers, Chromecast | `global_gain` (mp3rgain) |
| Desktop players (foobar2000, etc.) | ReplayGain tags (`mp3rgain --tags-only`, rsgain, or foobar2000's own scanner) |
| Streaming to phone apps | ReplayGain tags |
| Maximum flexibility | ReplayGain tags |

For most modern listening setups **ReplayGain tags are the cleaner solution**. Change `global_gain` when the playback device does not support ReplayGain tags.

With mp3rgain this is not either/or. Applying gain also writes the standard `REPLAYGAIN_*` tags with residual values, as mp3gain does, so tag-aware and tag-blind players converge on the same loudness. (`-s s` writes no tags, and that includes the undo tag.) `--tags-only` runs the same analysis but writes the full values without modifying a single frame, the way loudgain and rsgain work. Use it when listeners should be able to switch ReplayGain off in their player; use the default apply when the playback device ignores tags.

```bash
mp3rgain -a --tags-only *.mp3   # tags only, audio frames untouched
mp3rgain -a *.mp3               # gain baked into global_gain, plus residual tags
```

## Security

See [security.md](security.md) for the CVE details.

| Tool | Security status |
|------|-----------------|
| mp3rgain | Rust with no `unsafe` blocks, and none of the C code behind mp3gain's and aacgain's CVEs |
| mp3gain 1.6.2 | The release and the Windows binaries are unpatched; Debian/Ubuntu and some other distributions ship patched builds, and upstream master carries the APE tag fixes (2025-11) without a new release |
| aacgain 2.0.0 | Still bundles mpglibDBL and mp3gain's unpatched `apetag.c` (CVE-2021-34085, CVE-2023-49356 and others) |

## Known limitations

### mp3rgain

- Lossless gain moves in whole 1.5 dB steps, so the audio lands within about 0.75 dB of the target (`--tags-only` writes exact values).
- MP3 and AAC only: no FLAC, Ogg, Opus, WAV or ALAC.
- HE-AAC handling is untested (see [Supported formats](#supported-formats)).
- `-g` and `-l` leave existing `REPLAYGAIN_*` tags unchanged (see [Avoiding double volume adjustment](#avoiding-double-volume-adjustment)).
- Undo restores every audio frame, but after an ID3v2 tag write the file is not byte-identical to the original (the ID3v2 tag is rewritten as ID3v2.4). Frames whose `global_gain` clamped at 0 or 255 cannot be restored.
- Raw ADTS streams with CRC protection (`protection_absent = 0`) keep their original CRC after a gain change. ffmpeg and Apple's encoder do not write ADTS CRCs, so there is nothing to test the update against.

### aacgain

- Bundles vulnerable mpglibDBL (CVE-2021-34085 unpatched)
- Requires a C build environment on some platforms
- Depends on faad2 for AAC

### mp3gain

- Upstream has made no release since 1.6.2; security patches come from distribution maintainers
- No AAC support
