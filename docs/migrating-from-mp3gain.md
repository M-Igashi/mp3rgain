# Migrating from mp3gain to mp3rgain

This guide is for users who already run **mp3gain** in a script, CI job, Dockerfile or media-server pipeline and want to switch to **mp3rgain** without rewriting everything.

For most setups the change is the binary name: `mp3gain` → `mp3rgain`. mp3rgain accepts mp3gain's options, prints the same tab-separated output and reads and writes the same APEv2 undo tag, so the two tools can be mixed on one file. A few behaviours differ, though, and they matter most in unattended scripts: read [Behaviour differences](#behaviour-differences) before switching.

> **Upgrading from mp3rgain 2.8.x or earlier?** Those releases stored `MP3GAIN_UNDO` with the opposite sign from mp3gain. This was fixed in 2.9.0 ([#210](https://github.com/M-Igashi/mp3rgain/issues/210)). Undo a file adjusted by one of those releases with that same release, or re-analyze it: undoing it with 2.9.0 or later doubles the gain instead. Files written by mp3gain, or by mp3rgain 2.9.0 and later, undo correctly with either tool.

## TL;DR

```bash
# Before
mp3gain -r -k -d 0 -s s -o file.mp3

# After
mp3rgain -r -k -d 0 -s s -o file.mp3
```

For interactive use, an alias works too:

```bash
# ~/.bashrc / ~/.zshrc
alias mp3gain=mp3rgain
```

## Options shared with mp3gain

Every mp3gain 1.6.2 option is accepted except `-T`. Where mp3rgain's behaviour differs, the table says so.

| Option | Meaning in mp3rgain |
|--------|---------------------|
| `-r` | Analyze and apply track gain |
| `-a` | Analyze and apply album gain. Every file given is one album, the same rule as mp3gain |
| `-e` | Skip album analysis. **Differs:** on its own, `-e` applies track gain (see below) |
| `-g <i>` | Apply `i` gain steps of 1.5 dB each, without analysis |
| `-l <c> <g>` | Apply `g` steps to one channel only: `0` = left, `1` = right. MP3 only |
| `-d <n>` | Shift the target by `n` dB, rounded to whole 1.5 dB steps. Without `-r`/`-a` it shifts the recommendation the plain analysis prints |
| `-m <i>` | Shift the suggested gain by `i` steps (adds to `-d`) |
| `-u` | Undo from the stored `MP3GAIN_UNDO` tag. Either tool can undo the other's changes |
| `-x` | Print the maximum amplitude only |
| `-k` | Lower the gain to avoid clipping. With `-r`/`-a` the limit comes from the decoded peak; with `-g` it only stops any frame's `global_gain` from exceeding 255 |
| `-c` | Silence clipping warnings |
| `-p` | Keep the file's modification time |
| `-w` | Wrap `global_gain` around 0-255 instead of clamping |
| `-q` | Quiet mode |
| `-f` | Accepted, no effect (prints a note) |
| `-t` | Accepted, no effect: every write already goes to a temp file that replaces the original (since 2.9.6). mp3gain's `-T` (modify in place) is not supported: it is reported as an unknown option and ignored |
| `-s c` / `-s d` | Show / delete stored tags. `-s d -u` undoes the gain first, then deletes the tags |
| `-s s` | Write no undo or ReplayGain tags (MP3, M4A and raw ADTS alike), so the change cannot be undone afterwards |
| `-s r` | Accepted; re-analysis is already mp3rgain's default (see `-s R` below) |
| `-s i` / `-s a` | Put every tag in ID3v2 / every tag in APEv2. `-s a` is mp3gain's layout (see [Tag compatibility](#tag-compatibility)) |
| `-o` | Tab-separated output with mp3gain's header (see [Output format](#output-format)) |
| `-v` / `-h` | Version / help. `-v` is version, not verbose |

As in mp3gain, an unknown short option such as `-Z` prints a warning and is ignored, so the run continues and exits 0. An unknown long option is an error.

## mp3rgain extensions

These options do not exist in mp3gain. A migrated script does not need them, but they are worth knowing:

| Option | Meaning |
|--------|---------|
| `-R` | Recurse into directory arguments, picking up `.mp3`, `.m4a`, `.aac` and `.mp4` files |
| `-n`, `--dry-run` | Show what would be done without writing anything |
| `-o text` / `-o json` / `-o tsv` | Choose the output format explicitly |
| `-i <n>` | Which audio track of a multi-track MP4 to analyze (default `0`). Gain, undo and ReplayGain tags always apply to the first audio track, so a value other than `0` works only for analysis and is an error with `-r`, `-a`, `-e`, `-g`, `-l`, `-u`, `-s d` or `--tags-only` ([#375](https://github.com/M-Igashi/mp3rgain/issues/375)). `-x` and the `global_gain` range columns always describe the first track |
| `-s R` | Reuse stored ReplayGain tags with `-r`/`-a` and rescan only files without them, which is mp3gain's default behaviour ([#298](https://github.com/M-Igashi/mp3rgain/issues/298)). In album mode, one file with missing or disagreeing album tags rescans the whole album. Ignored with `-s r`, `-d`/`-m`, `--rg2`/`--r128`, or when a tag was written by a BS.1770 analysis |
| `-j <n>`, `--threads <n>` | Worker threads for every per-file command (default and `0`: one per CPU; `1`: serial, like mp3gain). `MP3RGAIN_THREADS` sets the default. See [perf-parallel.md](perf-parallel.md) |
| `--skip-errors` | With `-a`, leave files that fail to decode out of the album instead of aborting |
| `--album-by dir\|tag` | With `-a`, split the run into several albums instead of one: `dir` is one album per directory, `tag` is one album per release (from `MUSICBRAINZ_ALBUMID`, or ALBUMARTIST/ARTIST plus ALBUM), so discs in subfolders share one album gain |
| `--per-directory` | Alias for `--album-by=dir` |
| `--album-depth <n>` | With `-a -R`, one album per directory `n` levels below each directory argument, for untagged libraries (`2` suits an `Artist/Album` tree). Cannot be combined with `--album-by` |
| `--rg2` / `--r128` | Measure loudness with ITU-R BS.1770 (ReplayGain 2.0 at -18 LUFS, or EBU R128 at -23 LUFS) instead of the default ReplayGain 1.0 |
| `--true-peak` | Write the BS.1770-4 true peak to `REPLAYGAIN_*_PEAK` instead of the sample peak. Requires `--rg2` or `--r128` |
| `--tags-only` | Write `REPLAYGAIN_*` tags and leave the audio untouched, the way loudgain and rsgain work ([#308](https://github.com/M-Igashi/mp3rgain/issues/308)). The tag holds the full gain rather than mp3gain's residual, and no undo tag is written. Requires `-r`, `-a` or `-e`; `-d`/`-m` shift the written value exactly (no step rounding) and `-k` caps it at the file's headroom |

Run `mp3rgain --help` for the full list.

## Output format

`-o` with no format word after it prints mp3gain's tab-separated format with the same header:

```
File	MP3 gain	dB gain	Max Amplitude	Max global_gain	Min global_gain
Albums/Foo/01.mp3	0	0.0	17234	148	100
```

- `File` is the path exactly as given on the command line, as mp3gain prints it (since 3.6.0; earlier versions printed the bare file name).
- `Max Amplitude` is on mp3gain's 16-bit scale (peak × 32768) in the default ReplayGain 1.0 mode. Under `--rg2`/`--r128` it is the float peak instead, the value written to `REPLAYGAIN_*_PEAK`.
- `Max global_gain` / `Min global_gain` cover MP3, AAC in MP4/M4A and raw ADTS `.aac`, matching what `-x` prints. A file whose gain fields cannot be scanned shows `-` (since 3.7.0, [#329](https://github.com/M-Igashi/mp3rgain/issues/329)).
- Every command prints rows, not just the plain analysis: `-r`, `-a` and `-e` print the recommended change for each file (plus the `"Album"` row under `-a`) before rewriting it, so `mp3rgain -o tsv -a */*.mp3` reports the same numbers as `mp3rgain -o tsv */*.mp3`.

Parsers written for mp3gain's output therefore keep working, including the command backend of the [beets](https://beets.io/) replaygain plugin:

```yaml
# ~/.config/beets/config.yaml
replaygain:
  backend: command
  command: mp3rgain
```

For new integrations, `-o json` is easier to parse. Combine it with an action (`-r -n -o json` for a preview): `-o json` on its own reports frame statistics only, without the loudness analysis.

## Tag compatibility

| Tag | mp3gain | mp3rgain |
|-----|---------|----------|
| APEv2 `MP3GAIN_UNDO`, `MP3GAIN_MINMAX` (MP3) | Written | Written. Either tool can undo the other's changes |
| APEv2 `MP3GAIN_ALBUM_MINMAX` (MP3, `-a`) | Written | Written, but only to APEv2: under `-s i` it is not written at all |
| APEv2 `REPLAYGAIN_*` (MP3) | Written | Written with `-s a` only. Since 3.2.0 the default puts `REPLAYGAIN_*` in ID3v2 instead and removes stale APEv2 copies, so the two cannot disagree |
| ID3v2 `TXXX` `REPLAYGAIN_*` (MP3) | Written with `-s i` | Written by default since 3.2.0, because that is where most players look (ffmpeg, for one, does not read APEv2 on MP3) |
| MP4 freeform atoms (AAC in MP4/M4A) | n/a | `mp3rgain_undo`, `mp3rgain_minmax`, `replaygain_track_gain`, `replaygain_track_peak`, `replaygain_album_gain`, `replaygain_album_peak` (plus `replaygain_algorithm` under `--rg2`/`--r128`) in the `com.apple.iTunes` namespace |
| ID3v2 (raw ADTS `.aac`) | n/a | Undo and `REPLAYGAIN_*` both go into an ID3v2 tag, since a raw stream has no container for freeform atoms; `-s a`/`-s i` do not apply (since 3.7.0, [#330](https://github.com/M-Igashi/mp3rgain/issues/330)) |

For the choice between rewriting `global_gain` and writing ReplayGain *tags*, see [COMPARISON.md](COMPARISON.md).

## Behaviour differences

| Area | mp3gain | mp3rgain |
|------|---------|----------|
| Clipping | Stops and asks before applying a gain that may clip, unless `-c` is given | Never prompts: applies the gain and prints a warning. Use `-k` to cap the gain, `-c` to silence the warning |
| `-e` on its own | Analyzes and stores the result; the audio is not changed | Analyzes and applies track gain, like `-r` |
| Plain analysis (`mp3gain file.mp3`) | Stores the analysis in an APEv2 tag | Read-only: nothing is written, so a later `-s R` has nothing to reuse |
| Stored analysis | Reused unless `-s r` | Re-analyzed unless `-s R` |
| Tag layout | Everything in APEv2 | Since 3.2.0, `REPLAYGAIN_*` in ID3v2 and `MP3GAIN_*` in APEv2. `-s a` restores mp3gain's layout |
| `-g` and stored ReplayGain tags | Shifts stored `REPLAYGAIN_*` values by the applied gain | Leaves existing `REPLAYGAIN_*` tags as they were (`-l` too), so they no longer match the audio. Re-run `-r`/`-a` (or `-r --tags-only`) afterwards, or use `-s d` to remove them (which also removes the undo tag) |
| Tags written by `-g` | `MP3GAIN_UNDO` only | `MP3GAIN_UNDO` and `MP3GAIN_MINMAX`. The audio frames are byte-identical to mp3gain's ([compatibility-report.md](compatibility-report.md)); the tag block is not |
| Undo (`-u`) | Keeps its APEv2 tag, with the undo value reset to zero and the `REPLAYGAIN_*` values adjusted | Removes the undo tag and the `REPLAYGAIN_*` values in both containers |
| ReplayGain analysis | Decodes with mpglib | Decodes with symphonia. Values normally match mp3gain's to the printed precision; CI checks that they agree within one gain step |
| Formats | MP3 only | MP3, AAC in MP4/M4A (including the audio track of a video `.mp4`) and raw ADTS `.aac` |
| A file listed more than once | Processed once per listing: `-g 2 a.mp3 a.mp3` moves the audio by 4 steps but records 2 in the undo tag, so `-u` restores only half | Processed once, whether it is listed twice, named through a symlink alongside its target, or found under overlapping `-R` directories; a note says how many paths were skipped ([#370](https://github.com/M-Igashi/mp3rgain/issues/370)) |
| Symbolic links | Replaced by a modified regular file; the target is left unchanged | Written through: the target changes and the link stays a link. Hard links are split by both tools, since both write a new file in place of the old one ([#370](https://github.com/M-Igashi/mp3rgain/issues/370)) |

Undo restores every audio frame exactly, but the file is not always byte-identical to the original: when the default layout wrote an ID3v2 tag, `-u` leaves an empty ID3v2.4 header on a file that had none, and an existing ID3v2 tag stays rewritten as ID3v2.4. Undo is byte-identical when no ID3v2 tag was involved, for example after `-g` or `-s a -r` on a file without ID3v2 ReplayGain tags. Frames whose `global_gain` was clamped at 0 or 255 cannot be restored either way; mp3rgain prints a warning when that happens.

If you find a case where mp3rgain's audio output differs from mp3gain's for the same operation, please [open an issue](https://github.com/M-Igashi/mp3rgain/issues).

## Migrating common pipelines

### Shell scripts and cron jobs

Usually a literal substitution:

```bash
sed -i 's/\bmp3gain\b/mp3rgain/g' /path/to/your/script.sh
```

(On macOS use `gsed`, or drop `-i` and inspect the output first.)

### Dockerfiles

Use the image on GHCR instead of installing mp3gain:

```dockerfile
# Before
RUN apt-get update && apt-get install -y mp3gain && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["mp3gain"]

# After: the image is FROM scratch with mp3rgain as its entrypoint
FROM ghcr.io/m-igashi/mp3rgain:latest
```

To add mp3rgain to an existing image instead:

```dockerfile
COPY --from=ghcr.io/m-igashi/mp3rgain:latest /usr/local/bin/mp3rgain /usr/local/bin/mp3rgain
```

The image is published for `linux/amd64` and `linux/arm64` with the tags `latest`, `vX.Y.Z` and `vX`. It contains a single static musl binary and nothing else: no libc, no shell.

### CI workflows (GitHub Actions / GitLab CI)

```yaml
# Before
- run: |
    sudo apt-get install -y mp3gain
    mp3gain -r -k music/*.mp3

# After, using the Docker image
- run: |
    docker run --rm -v "$PWD/music:/music" \
      ghcr.io/m-igashi/mp3rgain:latest -r -k -R /music
```

Or download a release binary. Asset names carry the version, so pin one:

```yaml
- run: |
    curl -fsSL https://github.com/M-Igashi/mp3rgain/releases/download/v3.9.2/mp3rgain-v3.9.2-linux-x86_64.tar.gz \
      | sudo tar -xz -C /usr/local/bin mp3rgain
    mp3rgain -r -k music/*.mp3
```

The Linux release binaries are linked against glibc 2.34 or newer (Ubuntu 22.04, Debian 12 and later). For anything older, use the Docker image.

### Installing

| Platform | Before | After |
|----------|--------|-------|
| Ubuntu 26.04 LTS | `apt install mp3gain` | `sudo add-apt-repository ppa:m-igashi/mp3rgain && sudo apt install mp3rgain` |
| Debian / other Ubuntu releases | `apt install mp3gain` | `.deb` from the [releases page](https://github.com/M-Igashi/mp3rgain/releases) (amd64 and arm64) |
| Arch Linux | `yay -S mp3gain` (AUR) | `yay -S mp3rgain-bin` (community-maintained AUR package) |
| macOS | `brew install mp3gain` | `brew install M-Igashi/tap/mp3rgain` or `sudo port install mp3rgain` |
| Windows | SourceForge download | `winget install M-Igashi.mp3rgain` |
| Any platform with Rust | n/a | `cargo install mp3rgain` |

Users of MP3Gain's Windows GUI can install the desktop app instead: `winget install M-Igashi.mp3rgui`. Its defaults differ from the CLI's: clipping prevention and timestamp preservation are on, and album gain treats each folder as one album.

### beets

Change `command: mp3gain` to `command: mp3rgain` in `~/.config/beets/config.yaml` (see [Output format](#output-format)). Nothing else is needed since beets/beets#6289.

### Migrating from aacgain (AAC/M4A users)

[aacgain](https://github.com/dgilman/aacgain) has had no commits since 2022. mp3rgain performs the same lossless `global_gain` rewrite on AAC, with the same command line:

```bash
# Before
aacgain -r -k *.m4a

# After
mp3rgain -r -k *.m4a
```

mp3rgain keeps its AAC undo data in its own `mp3rgain_undo` atom and does not read aacgain's undo data, so undo files adjusted by aacgain with aacgain before switching. Raw ADTS `.aac` streams (from `ffmpeg -f adts`, DVB/HLS captures and some rippers) are handled since 3.7.0, with their tags in an ID3v2 tag. See [COMPARISON.md](COMPARISON.md) for a feature matrix.

## When *not* to migrate

mp3rgain is not the right tool if you need:

- **An exact loudness level in the audio.** `--rg2` and `--r128` measure BS.1770 loudness, but a lossless change moves in 1.5 dB steps, so the audio lands within about 0.75 dB of the target. `--tags-only` writes the exact value as a tag; to bake an exact level into the audio, use ffmpeg `loudnorm`, which re-encodes.
- **FLAC, Ogg, Opus, WAV or ALAC.** mp3rgain handles MP3 and AAC only; loudgain and rsgain cover the other formats. Under `-R` such files are ignored. An ALAC or DRM-protected M4P file is reported as skipped without failing the run ([#330](https://github.com/M-Igashi/mp3rgain/issues/330)); FLAC, Opus or WAV files passed by name fail with an error.

## Reporting migration problems

If a command that worked under mp3gain behaves unexpectedly under mp3rgain, please [open an issue](https://github.com/M-Igashi/mp3rgain/issues) with:

1. The exact command line for both tools
2. SHA-256 of the input and of both output files
3. The mp3gain version (`mp3gain -v`) and the mp3rgain version (`mp3rgain -v`)
4. A minimal file that reproduces it, if possible

## See also

- [compatibility-report.md](compatibility-report.md): bit-level verification against mp3gain
- [COMPARISON.md](COMPARISON.md): feature comparison with aacgain and mp3gain
- [use-cases.md](use-cases.md): integrations (beets, Bake'n Deck, scripts, Docker)
- [Original mp3gain](http://mp3gain.sourceforge.net/)
- [ReplayGain specification](https://wiki.hydrogenaud.io/index.php?title=ReplayGain_specification)
