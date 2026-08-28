# Migrating from mp3gain to mp3rgain

This guide is for users who already have **mp3gain** in a script, CI job,
Dockerfile, or media-server pipeline and want to switch to **mp3rgain**
without rewriting everything.

For most setups, migration is a one-line change — `mp3gain` → `mp3rgain`.

> **Looking for binary-level proof?** See
> [compatibility-report.md](compatibility-report.md) for SHA-256 verification
> that `mp3rgain -g N file.mp3` produces a byte-identical file to
> `mp3gain -g N file.mp3`.

## TL;DR

```bash
# Before
mp3gain -r -k -d 0 -s s -o file.mp3

# After
mp3rgain -r -k -d 0 -s s -o file.mp3
```

The CLI surface, the TSV output format, and the APEv2 undo tag are all
mp3gain-compatible. You can mix the two tools on the same file — gain written
by mp3gain can be undone by mp3rgain and vice versa.

> **Upgrading from mp3rgain ≤ 2.8.x?** Releases up to 2.8.x stored the
> `mp3gain_undo` value with the *opposite* sign from mp3gain. That is fixed in
> this version ([#210](https://github.com/M-Igashi/mp3rgain/issues/210)), so a
> file gain-adjusted by an **older mp3rgain** must be undone by that older
> version (or re-analyzed) — undoing it with this version would double the gain.
> Files written by mp3gain, or by this version onward, undo correctly with
> either tool.

## Drop-in alias

For interactive use, an alias is the fastest migration:

```bash
# ~/.bashrc / ~/.zshrc
alias mp3gain=mp3rgain
```

Caveats — see [Behaviour differences](#behaviour-differences) below before
relying on this in scripts.

## Command and flag equivalence

The flag list comes from `mp3rgain --help` (v2.8.0). All classic mp3gain
flags are accepted with the same semantics; mp3rgain adds a few extensions.

### Identical behaviour

| Flag | Meaning | Notes |
|------|---------|-------|
| `-r` | Apply track gain (ReplayGain) | |
| `-a` | Apply album gain (ReplayGain) | |
| `-e` | Skip album analysis even with multiple files | |
| `-g <i>` | Apply gain of `i` steps (1 step = 1.5 dB) | Bit-identical output (see compatibility report) |
| `-d <n>` | Modify suggested gain by `n` dB (applied with `-r` / `-a`) | mp3gain-compatible since v1.2.1 |
| `-m <i>` | Modify suggested gain by `i` steps | |
| `-u` | Undo gain changes (reads APEv2 `mp3gain_undo`) | Reads tags written by either tool ([#210](https://github.com/M-Igashi/mp3rgain/issues/210)); see the note on upgrading from mp3rgain ≤ 2.8.x |
| `-x` | Print max amplitude only | |
| `-k` | Prevent clipping (auto-limit gain) | |
| `-c` | Ignore clipping warnings | |
| `-p` | Preserve original file timestamp | |
| `-t` | Use temp file for safer writes | |
| `-f` | Assume MPEG2 Layer III | Accepted; no effect (mp3rgain auto-detects) |
| `-q` | Quiet mode | |
| `-R` | Process directories recursively | |
| `-s c` / `-s d` / `-s s` / `-s r` / `-s a` | Stored-tag handling: check / delete / skip / recalc / all-APEv2 | Same modes as mp3gain. `-s a` pins every tag to APEv2, which is mp3gain's layout and was mp3rgain's default before 3.2.0. Note the default is inverted: mp3gain reuses stored tags unless you pass `-s r`, while mp3rgain re-analyzes unless you pass `-s R` (see below) |
| `-o` (no argument) | TSV output | Default header matches mp3gain exactly (see [Output format](#output-format)) |

### mp3rgain extensions

These flags do not exist in mp3gain. They will not break a migration but
are worth knowing:

| Flag | Meaning |
|------|---------|
| `-l <c> <g>` | Apply gain to left (`0`) or right (`1`) channel only |
| `-i <n>` | Specify which audio track to process (M4A multi-track) |
| `-w` | Wrap gain values instead of clamping |
| `-n`, `--dry-run` | Preview changes without writing |
| `-o text` / `-o json` / `-o tsv` | Explicit output format selection |
| `-s R` | Reuse stored ReplayGain tags with `-r`/`-a`, rescanning only files whose tags are missing. This restores mp3gain's default behavior as an opt-in ([#298](https://github.com/M-Igashi/mp3rgain/issues/298)). Album mode requires a consistent set of album tags on every file, otherwise the whole album is rescanned. Ignored with `-s r`, `-d`/`-m`, or `--rg2`/`--r128` |
| `-s i` | Put *every* tag in ID3v2 `TXXX`, undo included. Since 3.2.0 the default already writes `REPLAYGAIN_*` to ID3v2, so this is only needed when you want `MP3GAIN_UNDO` there too |
| `-j <n>` / `--threads <n>` | Worker threads for ReplayGain analysis (default: auto). `MP3RGAIN_THREADS` env var also honored. `-j 1` reproduces mp3gain's serial behavior. See [docs/perf-parallel.md](perf-parallel.md). |
| `--skip-errors` | Keep album analysis (`-a`) going past unreadable files; failed files are reported and excluded from the album gain |

For the full list run `mp3rgain --help`.

## Output format

mp3rgain's `-o` (with no argument) emits the exact mp3gain tab-delimited
format with the canonical header:

```
File	MP3 gain	dB gain	Max Amplitude	Max global_gain	Min global_gain
song.mp3	0	0.0	17234	148	100
```

This means existing parsers that consume mp3gain output — most notably the
[beets](https://beets.io/) replaygain plugin's command backend — work
unchanged. A typical beets configuration:

```yaml
# ~/.config/beets/config.yaml
replaygain:
  backend: command
  command: mp3rgain
```

For new integrations, prefer `-o json`, which is structured and stable.

## Tag compatibility

| Tag location | mp3gain | mp3rgain | Interop |
|--------------|---------|----------|---------|
| APEv2 `mp3gain_undo` (MP3) | Written | Written | Bidirectional — either tool can undo the other's changes ([#210](https://github.com/M-Igashi/mp3rgain/issues/210); files written by mp3rgain ≤ 2.8.x used the opposite sign) |
| APEv2 `mp3gain_minmax` (MP3) | Written | Written | Same |
| APEv2 ReplayGain (`mp3gain_album_*`, `replaygain_*`) | Written | Written with `-s a` | Since 3.2.0 the default puts `REPLAYGAIN_*` in ID3v2 instead, and clears stale APEv2 copies so the two cannot disagree. `-s a` restores mp3gain's layout exactly |
| ID3v2 TXXX ReplayGain | Not written | Written by default since 3.2.0 | Where standard ReplayGain readers look. ffmpeg does not read APEv2 on MP3 at all, and Rockbox only handles APE tags for WavPack/Musepack |
| MP4 freeform metadata (AAC/M4A) | N/A | Written | mp3gain has no AAC support; mp3rgain stores AAC undo and ReplayGain in `com.apple.metadata.mdta:ReplayGain_*` and a `mp3gain_undo` freeform atom |

For more on the choice between bitstream `global_gain` rewriting and
ReplayGain *tags*, see [docs/COMPARISON.md](COMPARISON.md).

## Behaviour differences

These are the only behaviour differences worth knowing about:

| Area | Difference | Impact |
|------|------------|--------|
| Undo of files from mp3rgain ≤ 2.8.x | Those releases stored `mp3gain_undo` with the opposite sign; fixed in this version ([#210](https://github.com/M-Igashi/mp3rgain/issues/210)) | Undo a file gain-adjusted by an older mp3rgain with that **older** version (or re-analyze). Undoing it with this version would double the gain. mp3gain-written files and files from this version onward are unaffected |
| Undo cleanup | mp3gain leaves empty APEv2 tags after `-u`; mp3rgain removes them | Audio data is identical; only the tag block differs |
| ReplayGain analysis | mp3gain uses LAME; mp3rgain uses Symphonia + native Rust | Track/album gain values may differ by <0.1 dB. The *applied* gain is bit-identical for any given step value |
| Format coverage | mp3gain handles MP3 only | mp3rgain also handles AAC/M4A/.mp4 (lossless `global_gain` rewrite, the same idea aacgain used) |
| Tag placement | mp3gain puts everything in APEv2 | Since 3.2.0 mp3rgain splits: `REPLAYGAIN_*` to ID3v2 where players look, `MP3GAIN_UNDO`/`MINMAX` to APEv2 where mp3gain looks. `-s a` restores the all-APEv2 layout; `-s i` puts everything in ID3v2. `MP3GAIN_ALBUM_MINMAX` is APEv2-only in every mode |

If you discover a case where mp3rgain produces non-identical output for an
operation listed under [Identical behaviour](#identical-behaviour), please
[open an issue](https://github.com/M-Igashi/mp3rgain/issues).

## Migrating common pipelines

### Shell scripts and cron jobs

Usually a literal substitution:

```bash
sed -i 's/\bmp3gain\b/mp3rgain/g' /path/to/your/script.sh
```

(Use `gsed` on macOS, or drop `-i` and inspect first.)

### Dockerfiles

Replace the apt install with a static binary pull from GHCR:

```dockerfile
# Before
RUN apt-get update && apt-get install -y mp3gain && rm -rf /var/lib/apt/lists/*
ENTRYPOINT ["mp3gain"]

# After (multi-stage, ~2 MB final image)
FROM ghcr.io/m-igashi/mp3rgain:latest
# That's it — the image is FROM scratch with mp3rgain as the entrypoint.
```

Or, if you need mp3rgain alongside other tooling in an existing image:

```dockerfile
COPY --from=ghcr.io/m-igashi/mp3rgain:latest /mp3rgain /usr/local/bin/mp3rgain
```

The image publishes `linux/amd64` and `linux/arm64` and is statically
linked against musl — no glibc, no shell, no other runtime deps.

### CI workflows (GitHub Actions / GitLab CI)

```yaml
# Before
- run: |
    sudo apt-get install -y mp3gain
    mp3gain -r -k music/*.mp3

# After — use the official Docker image
- run: |
    docker run --rm -v "$PWD/music:/music" \
      ghcr.io/m-igashi/mp3rgain:latest -r -k -R /music
```

Or install the static binary directly from a release artifact:

```yaml
- run: |
    curl -fsSL https://github.com/M-Igashi/mp3rgain/releases/latest/download/mp3rgain-linux-x86_64.tar.gz \
      | tar -xz -C /usr/local/bin mp3rgain
    mp3rgain -r -k music/*.mp3
```

### Linux distros (apt / dnf / pacman)

| Distribution | Before | After |
|--------------|--------|-------|
| Ubuntu 25.10 | `apt install mp3gain` | `add-apt-repository ppa:m-igashi/mp3rgain && apt install mp3rgain` |
| Debian / older Ubuntu | `apt install mp3gain` | Download `.deb` from [releases](https://github.com/M-Igashi/mp3rgain/releases) |
| Arch Linux | `pacman -S mp3gain` (AUR) | `yay -S mp3rgain-bin` |
| macOS (Homebrew) | `brew install mp3gain` | `brew install M-Igashi/tap/mp3rgain` |
| Windows | (no official package) | `winget install M-Igashi.mp3rgain` |
| Cargo | n/a | `cargo install mp3rgain` |

### beets

Already covered above — change `command: mp3gain` to `command: mp3rgain`
in `~/.config/beets/config.yaml`. No other changes required as of
beets/beets#6289.

### Migrating from aacgain (AAC/M4A users)

[aacgain](http://aacgain.altosdesign.com/) has been unmaintained since
~2009 and rarely builds on modern 64-bit systems. mp3rgain is the only
actively maintained replacement that performs lossless `global_gain`
rewriting on AAC. Migration is the same one-line rename:

```bash
# Before
aacgain -r -k *.m4a

# After
mp3rgain -r -k *.m4a
```

Undo information is stored in MP4 freeform metadata rather than APEv2
(AAC files do not natively support APEv2). See
[docs/COMPARISON.md](COMPARISON.md) for a detailed feature matrix.

## When *not* to migrate

mp3rgain is not the right tool if you need any of the following:

- **EBU R128 / LUFS** loudness — mp3rgain implements ReplayGain 1.0 (89 dB
  reference). For R128 use loudgain, ffmpeg `loudnorm`, or rsgain.
- **FLAC / OGG / Opus / WAV** — mp3rgain only handles MP3 and AAC.
  loudgain or rsgain cover lossless containers.
- **Tag-only ReplayGain on AAC** — if all your players honour ReplayGain
  tags, rsgain is lighter-weight. mp3rgain's value is the lossless
  bitstream rewrite for players that ignore tags (DJ hardware, smart
  speakers, car audio).

See [docs/COMPARISON.md](COMPARISON.md) for the full decision matrix.

## Reporting migration problems

If a command that worked under mp3gain produces unexpected output under
mp3rgain, please [open an issue](https://github.com/M-Igashi/mp3rgain/issues)
with:

1. The exact command line for both tools
2. SHA-256 of input and both output files
3. mp3gain version (`mp3gain -v`) and mp3rgain version (`mp3rgain -v`)
4. A minimal reproducer file if possible

## See also

- [compatibility-report.md](compatibility-report.md) — bit-level verification methodology and results
- [COMPARISON.md](COMPARISON.md) — detailed feature comparison vs aacgain / mp3gain / rsgain / loudgain
- [use-cases.md](use-cases.md) — real-world integrations (beets, headroom, DJ workflows)
- [Original mp3gain](http://mp3gain.sourceforge.net/)
- [ReplayGain specification](https://wiki.hydrogenaud.io/index.php?title=ReplayGain_specification)
