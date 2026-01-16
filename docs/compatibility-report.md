# mp3rgain Compatibility Report

[![mp3gain compatible](https://img.shields.io/badge/mp3gain-compatible-brightgreen.svg)](#verification-results)

This document verifies that mp3rgain produces **identical output** to the original mp3gain tool.

## Summary

mp3rgain is a drop-in replacement for the original mp3gain. Both tools modify the `global_gain` field in MP3 frame headers identically, producing bit-for-bit identical output files when given the same input and parameters.

## Verification Method

### Binary Exact Match Testing

We verify compatibility by applying identical operations to the same MP3 files using both tools and comparing SHA-256 hashes of the output:

```bash
# Prepare identical copies
cp original.mp3 test_mp3gain.mp3
cp original.mp3 test_mp3rgain.mp3

# Apply same operation with each tool
mp3gain -g 2 test_mp3gain.mp3
mp3rgain -g 2 test_mp3rgain.mp3

# Compare SHA-256 hashes
sha256sum test_mp3gain.mp3 test_mp3rgain.mp3
```

If the hashes match, the output files are byte-for-byte identical.

### Automated Testing

Run the compatibility test suite:

```bash
# From project root
./scripts/compatibility-test.sh

# With verbose output
VERBOSE=1 ./scripts/compatibility-test.sh

# Specify custom binary paths
MP3GAIN_BIN=/usr/bin/mp3gain MP3RGAIN_BIN=./target/release/mp3rgain ./scripts/compatibility-test.sh
```

### CI/CD Integration

Compatibility tests run automatically on every pull request in GitHub Actions. See the [CI workflow](../.github/workflows/ci.yml) for details.

## Test Cases

### Phase 1: Gain Step Operations (`-g`)

| Test | Command | Status |
|------|---------|--------|
| Positive gain +1 | `-g 1` | Verified |
| Positive gain +2 | `-g 2` | Verified |
| Positive gain +3 | `-g 3` | Verified |
| Positive gain +5 | `-g 5` | Verified |
| Negative gain -1 | `-g -1` | Verified |
| Negative gain -3 | `-g -3` | Verified |
| Negative gain -5 | `-g -5` | Verified |

### Phase 2: Clipping Prevention (`-k`)

| Test | Command | Status |
|------|---------|--------|
| Clip prevention high gain | `-k -g 10` | Verified |

### Phase 3: Channel-Specific Gain (`-l`)

| Test | Command | Status |
|------|---------|--------|
| Left channel +2 | `-l 0 2` | Verified |
| Right channel -2 | `-l 1 -2` | Verified |

## MP3 Format Coverage

Tests are performed on the following MP3 formats:

| Format | File | Status |
|--------|------|--------|
| Stereo CBR | `test_stereo.mp3` | Verified |
| Mono CBR | `test_mono.mp3` | Verified |
| Joint Stereo | `test_joint_stereo.mp3` | Verified |
| VBR | `test_vbr.mp3` | Verified |

### MPEG Version Coverage

| MPEG Version | Status |
|--------------|--------|
| MPEG1 Layer III | Verified |
| MPEG2 Layer III | Verified |
| MPEG2.5 Layer III | Verified |

## Test Environment

### Reference mp3gain Version

- **Version**: mp3gain 1.6.2 (or latest available via package manager)
- **Source**: `apt install mp3gain` on Ubuntu/Debian
- **Platform**: Linux (x86_64)

### mp3rgain Version

- **Version**: Current release
- **Build**: `cargo build --release`
- **Platform**: Cross-platform (macOS, Linux, Windows)

## Technical Details

### How Gain Adjustment Works

Both mp3gain and mp3rgain adjust volume by modifying the `global_gain` field in each MP3 frame's side information:

1. Parse MP3 frame headers
2. Locate `global_gain` field (8 bits, values 0-255)
3. Add/subtract the specified gain steps
4. Write modified frame back to file

Each gain step equals **1.5 dB** (defined by the MP3 specification).

### Why Binary Compatibility Matters

Binary compatibility ensures:

1. **Predictable behavior**: Same input produces identical output
2. **Reversibility**: Undo information stored by one tool works with the other
3. **Trust**: Users can verify claims independently
4. **Migration**: Easy transition from mp3gain to mp3rgain

### Known Differences

| Feature | mp3gain | mp3rgain |
|---------|---------|----------|
| `-d` option | Modifies suggested gain | Identical (v1.2.1+) |
| `-o` option | TSV output (no argument) | Identical (v1.2.1+) |
| Undo tag cleanup | Keeps empty APE tags after undo | Removes APE tags completely after undo |
| ReplayGain algorithm | Uses LAME routines | Uses Symphonia + native Rust |
| ReplayGain results | May differ slightly | May differ slightly |
| Gain adjustment (`-g`) | Identical | Identical |

**Notes**:
- As of v1.2.1, the `-d` and `-o` options are fully mp3gain-compatible. The `-d` option modifies the suggested ReplayGain value, and `-o` without an argument outputs TSV format.
- After undo, mp3gain leaves empty APE tags in the file while mp3rgain removes them completely. The audio data is identical in both cases.
- ReplayGain analysis results may have minor differences due to different audio decoding libraries, but the gain *application* mechanism is identical.

## Reproducing Tests

### Prerequisites

```bash
# Install original mp3gain
# Ubuntu/Debian
sudo apt-get install mp3gain

# Generate test fixtures (if not present)
mkdir -p tests/fixtures
ffmpeg -y -f lavfi -i "sine=frequency=440:duration=1" -ac 2 -ar 44100 -b:a 128k tests/fixtures/test_stereo.mp3
ffmpeg -y -f lavfi -i "sine=frequency=440:duration=1" -ac 1 -ar 44100 -b:a 64k tests/fixtures/test_mono.mp3
ffmpeg -y -f lavfi -i "sine=frequency=440:duration=1" -ac 2 -ar 44100 -b:a 128k -joint_stereo 1 tests/fixtures/test_joint_stereo.mp3
ffmpeg -y -f lavfi -i "sine=frequency=440:duration=1" -ac 2 -ar 44100 -q:a 2 tests/fixtures/test_vbr.mp3
```

### Running Tests

```bash
# Clone and build
git clone https://github.com/M-Igashi/mp3rgain.git
cd mp3rgain
cargo build --release

# Run compatibility tests
./scripts/compatibility-test.sh
```

### Manual Verification

```bash
# Single file test
cp tests/fixtures/test_stereo.mp3 /tmp/test_mp3gain.mp3
cp tests/fixtures/test_stereo.mp3 /tmp/test_mp3rgain.mp3

mp3gain -g 2 /tmp/test_mp3gain.mp3
./target/release/mp3rgain -g 2 /tmp/test_mp3rgain.mp3

# Compare (should show identical hashes)
sha256sum /tmp/test_mp3gain.mp3 /tmp/test_mp3rgain.mp3
```

## Verification Results

### Latest Test Run

Tests are run automatically in CI on every pull request. See the latest workflow run for current results:

[![CI Status](https://github.com/M-Igashi/mp3rgain/actions/workflows/ci.yml/badge.svg)](https://github.com/M-Igashi/mp3rgain/actions/workflows/ci.yml)

### Historical Results

| Date | mp3gain Version | mp3rgain Version | Result |
|------|-----------------|------------------|--------|
| 2026-01 | 1.6.2 | 1.4.0 | All tests passed |
| 2026-01 | 1.6.2 | 1.3.0 | All tests passed |

## FAQ

### Q: Why might ReplayGain values differ slightly?

ReplayGain analysis requires decoding the MP3 to PCM audio. mp3gain uses LAME's internal routines, while mp3rgain uses the Symphonia library. Minor floating-point differences in audio decoding can result in slightly different loudness measurements (typically <0.1 dB).

**The gain adjustment mechanism itself is identical** - only the analysis phase may differ.

**Note**: Prior to v1.2.6, mp3rgain had a bug where filter coefficients for 44.1 kHz and 48 kHz were swapped, causing significant loudness calculation errors. This has been fixed in v1.2.6+.

### Q: Can I use APEv2 tags created by mp3gain with mp3rgain?

Yes. Both tools use the same APEv2 tag format for storing undo information and ReplayGain data.

### Q: Is mp3rgain compatible with mp3gain on all platforms?

Yes. mp3rgain produces identical output on macOS, Linux, and Windows.

## Third-Party Integration

### beets

mp3rgain is compatible with the [beets](https://beets.io/) replaygain plugin as a drop-in replacement for mp3gain:

```yaml
# ~/.config/beets/config.yaml
replaygain:
  backend: command
  command: mp3rgain
```

The following beets command syntax is fully supported (as of v1.2.1):

```bash
mp3rgain -o -s s -k -d 0 file.mp3
```

- `-o` (without argument): TSV output format
- `-s s`: Skip stored tag info
- `-k`: Prevent clipping
- `-d 0`: Target 89 dB (ReplayGain reference level)

## Related Resources

- [Original mp3gain](http://mp3gain.sourceforge.net/)
- [ReplayGain Specification](https://wiki.hydrogenaud.io/index.php?title=ReplayGain_specification)
- [MP3 Frame Header Format](http://www.mp3-tech.org/programmer/frame_header.html)
- [beets replaygain plugin](https://beets.readthedocs.io/en/stable/plugins/replaygain.html)

## Contributing

Found a compatibility issue? Please [open an issue](https://github.com/M-Igashi/mp3rgain/issues) with:

1. The specific command that produces different output
2. SHA-256 hashes of both output files
3. The input MP3 file (or a minimal reproduction case)
4. Version information for both tools
