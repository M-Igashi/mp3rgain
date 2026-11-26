# Audio hash

`mp3rgain --audio-hash` prints a hash of each file's encoded audio that tags, the container, and a lossless gain change all leave alone. Two files with the same hash hold the same encode, whatever they were tagged with and however their volume was adjusted by mp3gain, aacgain, or mp3rgain. It was requested in [#491](https://github.com/M-Igashi/mp3rgain/issues/491) for finding duplicates, and is meant to be called by deduplicating software or linked into it through the crate.

```bash
mp3rgain --audio-hash *.mp3 *.m4a        # one "<hash>  <path>" line per file, like sha256sum
mp3rgain --audio-hash -R /music          # a whole library, in parallel
mp3rgain --audio-hash -o tsv -R /music   # File<TAB>Audio hash, with a header row
mp3rgain --audio-hash -o json song.mp3   # an "audio_hash" field per file
mp3rgain --audio-hash=v1 song.mp3        # a specific version of the definition
```

```text
mp3rgain-v1:7c151524fe1b7596530e6436d483e1e2ef7bf7b81c9d567511212ce66c555368  song.mp3
```

Hashing only reads: nothing is written to the file. A file whose format has no hash (ALAC, another MP4 codec, or DRM-protected M4P) is reported as skipped and does not set the exit status; one that cannot be read or holds no audio does.

From Rust: `mp3rgain::audio_hash(path, AudioHashVersion::LATEST)`, or `audio_hash_data(bytes, version)` for a file already in memory. Both return an `AudioHash`, whose `Display` is the text form above and whose `digest()` is the 32 bytes. They belong to the crate's `audio-hash` feature, which is on by default and brings the `sha2` dependency; with `default-features = false`, name it to keep them.

## What a match means

The hash is the same when two files differ only in:

- tags: ID3v1, ID3v2, APEv2, and MP4 metadata, the ReplayGain and undo tags included;
- the Xing, Info (LAME), or VBRI frame, which tools regenerate, and with it the gapless information;
- the container of AAC: an M4A, the raw ADTS stream extracted from it, and the same stream remuxed or with `moov` moved to the front;
- a gain change by mp3gain, aacgain, or mp3rgain in their default mode, by any number of steps in either direction, and its undo;
- the private, copyright, and original bits of the MP3 frame headers, and the CRCs that cover them;
- the `global_gain` of a granule or channel that codes no spectral data, which decodes to silence whatever the value.

It differs when:

- the encodes differ. Two encodes of one recording never match; finding those is the job of an acoustic fingerprint such as Chromaprint;
- a gain change could not be applied evenly: a value clamped at 0 or 255, a change to one channel only (`-l`), or a wrapped change (`-w`) that wrapped a value around or moved a 0 in a granule that codes spectral data. A wrapped change that did neither decodes to the same audio as the default mode, and matches;
- the frames differ in any other way, such as a fade made by editing the gain values.

## Limitations

- A frame directly followed by something that is neither another frame header nor the end of the audio, such as a Lyrics3 tag, an ID3v2 tag appended at the end or left between joined files, or junk, is not adjusted by a gain change, and the hash leaves it out as well. Removing those bytes brings the frame back into both, so the hash of the file changes.
- MP4 files are hashed from the first `mp4a` track only.
- An AAC stream with channel configuration 0 keeps its program config element in the MP4 `esds` box, while an ADTS stream carries it in its first frame, so the two do not match.
- Files adjusted by mp3rgain 2.8.0 to 3.11.1 whose AAC encoder wrote 14-byte fill elements (FDK-AAC, issue #402) had some frames left unadjusted, and do not match their original until undone with the version that made the change.

## Versions

A version of the definition is frozen once it is released: the same file gives the same `mp3rgain-v1:` hash in every later release, on every platform. If the definition has to change, for example because a later release adjusts frames that this one leaves alone, the new one becomes `v2`, `--audio-hash` without a version moves to it, and `--audio-hash=v1` keeps giving the old values. The library's `AudioHashVersion` enum lists every version, and `AudioHashVersion::LATEST` is the newest. The test suite holds the v1 hash of every fixture, and checks it before and after a gain change and an undo.

## Definition of v1

This section specifies v1 completely, so that it can be implemented without reading mp3rgain's source. Bytes are numbered from 0, and bits within a byte from the most significant one. "u32 LE" is a 32-bit unsigned integer in little-endian order, and "i16 LE" a 16-bit signed one.

### Output

The hash is the SHA-256 digest of the input below. Its text form is `mp3rgain-v1:` followed by the 32 digest bytes as 64 lowercase hexadecimal digits.

### Input

```text
input  = "mp3rgain-audio-hash-v1" 0x00 codec 0x00 unit*
codec  = "mp3" | "aac"
unit   = kind (1 byte), body length (u32 LE), body, value count (u32 LE), values (i16 LE each)
```

The units follow the order of the stream. A unit without values has a value count of 0. There are five kinds:

| Kind | Unit | Body | Values |
|---|---|---|---|
| 1 | MP3 frame a gain change adjusts | the frame, masked | one per `global_gain` field |
| 2 | Any other MP3 frame | the frame, masked | none |
| 3 | AAC stream configuration | object type (1 byte), sampling frequency in Hz (u32 LE), channel configuration (1 byte) | none |
| 4 | AAC raw_data_block a gain change adjusts | the block, masked | one per `global_gain` field |
| 5 | Any other AAC raw_data_block | the block as it is | none |

A file that yields no unit of kind 1, 2, 4, or 5 has no hash.

### Values

Each `global_gain` field of a kind 1 or kind 4 unit has an effective value: 0 when its channel codes no spectral data (an MP3 granule and channel whose `part2_3_length` is 0, an AAC channel whose `max_sfb` is 0), and the field's 8-bit value otherwise.

The reference `r` is the first effective value that is not 0, over all kind 1 units of an MP3 or all kind 4 units of an AAC stream, in stream order. A gain change adds the same number of steps to every value that is not 0 and leaves every 0 alone, so it moves `r` with the rest. When every effective value is 0 there is no `r`, and none is needed.

An effective value of 0 is written as -32768 (bytes `00 80`). Any other effective value `e` is written as `e - r`, which lies between -254 and 254.

### Which format

1. A file that starts with an MP4 `ftyp` box naming one of the brands `M4A `, `M4B `, `M4V `, `mp41`, `mp42`, `isom`, or `iso2`, as its major brand or among the compatible brands within its first 128 bytes, is an MP4. When the first track whose `hdlr` is `soun` (or that has no `hdlr`) has an `mp4a` sample entry, or no audio codec can be found, it is hashed as AAC in MP4. When that track has another codec (`alac`, `drms`, or anything else), there is no hash.
2. A file whose `ftyp` box names none of those brands and has `M4P ` as its major brand (DRM-protected) has no hash.
3. Otherwise, a file whose stream (after the ID3v2 tag described below) starts with a valid ADTS header is hashed as raw ADTS, provided another valid ADTS header follows `aac_frame_length` bytes later or the file has fewer than 7 bytes left at that point.
4. Anything else is hashed as MP3.

### The audio region

MP3 and ADTS are hashed between a start and an end offset.

- **start**: when the file is at least 10 bytes long and begins with `ID3`, start is 10 plus the size in bytes 6 to 9, read as four 7-bit groups (`(b6 & 0x7F) << 21 | (b7 & 0x7F) << 14 | (b8 & 0x7F) << 7 | (b9 & 0x7F)`). Otherwise start is 0. Only one tag is skipped, and its footer flag is not read. When start is not below end, the region is empty and holds no frames.
- **end**: the file length, less 128 when the file is at least 128 bytes long and its last 128 bytes begin with `TAG`. Then, when end is at least 32 and the 8 bytes at end - 32 are `APETAGEX`, an APEv2 footer is there: with `size` the u32 LE at end - 20 and `flags` the u32 LE at end - 12, and `header` 32 when bit 31 of `flags` is set and 0 otherwise, end becomes end - size - header, unless size + header is greater than end, in which case end stays as it was.

### MP3 frames

A frame header is the 4 bytes `b0 b1 b2 b3` at a position. It is valid when all of these hold:

- `b0` is `0xFF` and `b1 & 0xE0` is `0xE0`;
- the version `(b1 >> 3) & 3` is not 1 (3 is MPEG-1, 2 is MPEG-2, 0 is MPEG-2.5);
- the layer `(b1 >> 1) & 3` is 1 (Layer III);
- the bitrate index `b2 >> 4` is neither 0 nor 15;
- the sampling frequency index `(b2 >> 2) & 3` is not 3.

From a valid header:

- bitrate in kbit/s, for index 1 to 14: MPEG-1 `32 40 48 56 64 80 96 112 128 160 192 224 256 320`, MPEG-2 and 2.5 `8 16 24 32 40 48 56 64 80 96 112 128 144 160`;
- sampling frequency, for index 0 to 2: MPEG-1 `44100 48000 32000`, MPEG-2 `22050 24000 16000`, MPEG-2.5 `11025 12000 8000`;
- frame length: `floor(samples * bitrate * 125 / frequency) + ((b2 >> 1) & 1)`, with samples 1152 for MPEG-1 and 576 otherwise;
- the frame is mono when `b3 >> 6` is 3, and protected by a CRC when `b1 & 1` is 0;
- side information length: 17 bytes for MPEG-1 mono, 32 for MPEG-1 otherwise, 9 for MPEG-2 and 2.5 mono, 17 for MPEG-2 and 2.5 otherwise. It starts at byte 4 of the frame, or byte 6 when the frame is protected;
- the frame is a **Xing frame** when its 4 bytes at 4 plus the side information length (not moved by a CRC) are `Xing` or `Info`, and a **VBRI frame** when its bytes 36 to 39 are `VBRI`.

**Frames a gain change adjusts.** These are found by this walk over the audio region, which is the one mp3gain and mp3rgain use:

```text
p = start, reference = none
while p + 4 <= end:
    if the header at p is not valid: p = p + 1, continue
    n = p + frame length
    if n + 2 <= end: linked = (byte n is 0xFF and byte n+1 & 0xE0 is 0xE0)
    else:            linked = (n <= end)
    if not linked: p = p + 1, continue
    if the frame at p is a Xing frame: p = n, continue
    if reference is set and (version, sampling frequency index) differs from it: p = p + 1, continue
    if reference is none: reference = (version, sampling frequency index)
    the frame from p to n is adjusted; p = n
```

Each adjusted frame that is not a VBRI frame is a kind 1 unit. A VBRI frame gives no unit, and its values take no part in choosing `r`.

**Other frames.** Before the first adjusted frame, between two adjusted frames, and after the last one (or over the whole region when there is none), the bytes from `a` (start, or the end of an adjusted frame) to `b` (the next adjusted frame, or end) are searched for further frames. A VBRI frame found by the walk above counts as an adjusted frame here, although it gives no unit.

```text
q = a
while q + 4 <= b:
    if the header at q is valid, n = q + frame length, n <= b,
       and (n == b or (n + 4 <= b and the header at n is valid)):
        unless the frame is a Xing or VBRI frame, it is a kind 2 unit
        q = n
    else:
        q = q + 1
```

These are frames a gain change leaves alone, such as the part of a file joined from two encodes whose MPEG version or sampling frequency differs from the first frame's. Everything else in the stretch is passed over.

**Body.** The frame's bytes, with the private bit cleared (`b2 & 0x01`), the copyright and original bits cleared (`b3 & 0x0C`), every `global_gain` field of a kind 1 unit set to 0, and, when the frame is protected, bytes 4 and 5 (the CRC) removed.

**`global_gain` fields.** In the side information, counting bits from its first byte, the fields of granule `g` and channel `c` form a block that starts at bit `pre + (g * channels + c) * size`:

| | `pre`, mono | `pre`, stereo | `size` | granules |
|---|---|---|---|---|
| MPEG-1 | 18 | 20 | 59 | 2 |
| MPEG-2, 2.5 | 9 | 10 | 63 | 1 |

`part2_3_length` is the block's first 12 bits and `global_gain` the 8 bits from bit 21. The values of a kind 1 unit are in the order granule 0 channel 0, granule 0 channel 1, granule 1 channel 0, granule 1 channel 1, leaving out what the frame does not have.

### AAC in MP4

The track is the first `trak` (in `moov`) whose first sample description in `stsd` is `mp4a`. Its samples, in order, have the sizes in `stsz` and the offsets that `stsc` and `stco` (or `co64`) give. A sample that does not lie wholly inside the file is left out; every other sample is one raw_data_block.

The configuration comes from the AudioSpecificConfig in the `esds` box of the `mp4a` entry (the DecoderSpecificInfo, tag 5, inside the DecoderConfigDescriptor, tag 4, inside the ES_Descriptor, tag 3):

- the object type: 5 bits, or 32 plus the next 6 bits when those 5 bits are 31;
- the sampling frequency: a 4-bit index into `96000 88200 64000 48000 44100 32000 24000 22050 16000 12000 11025 8000 7350`, or, when the index is 15, the frequency in the next 24 bits;
- the channel configuration: 4 bits;
- when the object type is 5 or 29 (SBR or PS signalled explicitly), a second sampling frequency in the same form follows, and then the object type again, which replaces the first one.

So the object type, frequency, and channel configuration are those of the core AAC stream, which is what an ADTS header of the same stream carries. A track without an AudioSpecificConfig has no hash. One kind 3 unit precedes the first block, and every block is parsed with this sampling frequency.

### Raw ADTS

An ADTS header is the 7 bytes `h0` to `h6` at a position, valid when `h0` is `0xFF`, `h1 & 0xF6` is `0xF0`, the sampling frequency index `(h2 >> 2) & 0xF` is at most 12, and the frame length `(h3 & 3) << 11 | h4 << 3 | h5 >> 5` is greater than the header length, which is 7 when `h1 & 1` is 1 and 9 otherwise. Its configuration is the object type `(h2 >> 6) + 1`, the frequency of that index in the table above, and the channel configuration `(h2 & 1) << 2 | h3 >> 6`. It holds `(h6 & 3) + 1` raw_data_blocks. A header is only read where all 7 of its bytes lie before end.

Over the audio region:

```text
p = start
while p < end, the header at p is valid, and p + frame length <= end:
    the bytes from p + header length to p + frame length are a block,
    parsed at the header's sampling frequency when the frame holds 1 block, and not parsed otherwise
    p = p + frame length
while p < end:
    if the header at p is valid, n = p + frame length, n <= end,
       and (n == end or the header at n is valid):
        the bytes from p + header length to n are a block, not parsed
        p = n
    else:
        p = p + 1
```

A kind 3 unit precedes the first block, and every block whose header's configuration differs from that of the last kind 3 unit.

### AAC blocks

A block that is parsed, and parses, is a kind 4 unit; any other block is a kind 5 unit holding the block's bytes as they are.

Parsing follows the AAC LC raw_data_block syntax of ISO/IEC 14496-3 (subpart 4): elements are read until an END element or until fewer than 3 bits remain. Single channel and LFE elements carry one `individual_channel_stream`, a channel pair element two, after a common `ics_info` and M/S mask when `common_window` is set. Data stream, program config, and fill elements are skipped; a fill element's count is `cnt`, or `15 + esc_count - 1` when `cnt` is 15. The scalefactor band offsets are those of the standard for 96 kHz from 75132 Hz up, 64 kHz from 55426 Hz, 48 kHz from 37566 Hz, 32 kHz from 27713 Hz (whose short windows use the 48 kHz offsets), 24 kHz from 18783 Hz, 16 kHz from 9391 Hz, and 8 kHz below.

A block fails to parse when it contains a coupling channel element, when an `ics_info` sets `predictor_data_present`, when an `individual_channel_stream` sets `gain_control_data_present`, uses section codebook 12, has a section of length 0 or one running past `max_sfb`, has `max_sfb` greater than the number of scalefactor bands, sets pulse data in a short window, or has a TNS filter longer than the bands that remain, when an escape code has a prefix of 9 or more ones or a Huffman code is not in its codebook, and whenever a read runs past the end of the block.

The body of a kind 4 unit is the block with every `global_gain` field set to 0. Its values are those fields in the order they occur, and `max_sfb` is the one from the `ics_info` that applies to each channel.
