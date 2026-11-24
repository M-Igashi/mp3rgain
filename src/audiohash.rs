//! A hash of a file's encoded audio that tags, the container, and a lossless
//! gain change all leave alone, for finding copies of one encode (issue #491).
//!
//! Every `global_gain` value is hashed as its distance from the stream's
//! first non-zero one, and a 0 as a marker of its own: a gain change adds the
//! same number of steps to every non-zero value and leaves the zeros where
//! they are, which is exactly what this form cannot see. Clamped values, a
//! wrapped change (`-w`), and a change to one channel (`-l`) still show.
//!
//! A channel that codes no spectral data decodes to silence whatever its
//! `global_gain` says, and its value counts as 0. Encoders write such frames
//! for digital silence, and tools that moved them along with the rest (as
//! mp3rgain did before 3.13.0) left a value that a later gain change can take
//! to exactly 0, which mp3gain and mp3rgain then never move again.
//!
//! The definition is versioned, and a version never changes once released:
//! [`AudioHashVersion::V1`] is specified byte for byte in
//! `docs/audio-hash.md`, so that it can be implemented without this code.

#[cfg(feature = "aac")]
use crate::aac::{AacGainLocation, AudioSpecificConfig};
use crate::error::{Error, Result};
use crate::frame::{self, FrameHeader, GainLocation, MAX_GAIN_LOCATIONS};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::Path;

/// A definition of the audio hash. Each one is frozen when it is released, so
/// a stored hash can always be recomputed; a later definition is a new
/// variant next to the old ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum AudioHashVersion {
    /// The first definition (issue #491).
    V1,
}

impl AudioHashVersion {
    /// The newest definition, which the CLI's bare `--audio-hash` uses.
    pub const LATEST: Self = Self::V1;

    /// Every definition, oldest first.
    pub const ALL: &'static [Self] = &[Self::V1];

    /// The version number: 1 for [`Self::V1`].
    pub fn number(self) -> u16 {
        match self {
            Self::V1 => 1,
        }
    }

    /// The definition numbered `number`, if there is one.
    pub fn from_number(number: u16) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.number() == number)
    }

    /// Parse `1` or `v1` (as [`Display`](fmt::Display) prints it).
    pub fn parse(s: &str) -> Option<Self> {
        let digits = s.strip_prefix(['v', 'V']).unwrap_or(s);
        Self::from_number(digits.parse().ok()?)
    }
}

impl Default for AudioHashVersion {
    fn default() -> Self {
        Self::LATEST
    }
}

impl fmt::Display for AudioHashVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "v{}", self.number())
    }
}

/// The audio hash of one file: a SHA-256 digest and the definition it
/// follows. Displays as `mp3rgain-v1:` and 64 lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AudioHash {
    version: AudioHashVersion,
    digest: [u8; 32],
}

impl AudioHash {
    /// The definition this hash follows.
    pub fn version(&self) -> AudioHashVersion {
        self.version
    }

    /// The SHA-256 digest.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

impl fmt::Display for AudioHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "mp3rgain-{}:", self.version)?;
        self.digest.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

/// The audio hash of the file at `file_path`, by the definition `version`
/// ([`AudioHashVersion::LATEST`] for the newest). Reads the file and never
/// writes to it.
///
/// MP3 and AAC (in MP4/M4A or raw ADTS, HE-AAC included) have a hash. ALAC,
/// another MP4 audio codec, and DRM-protected M4P are
/// [`Error::UnsupportedFormat`].
pub fn audio_hash(file_path: &Path, version: AudioHashVersion) -> Result<AudioHash> {
    let data = std::fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    audio_hash_data(&data, version).map_err(|e| e.refine_format(file_path))
}

/// [`audio_hash`] of a whole file already in memory.
pub fn audio_hash_data(data: &[u8], version: AudioHashVersion) -> Result<AudioHash> {
    let digest = match version {
        AudioHashVersion::V1 => v1(data)?,
    };
    Ok(AudioHash { version, digest })
}

// =============================================================================
// Definition v1
// =============================================================================

/// The kind byte that opens every unit.
const MP3_FRAME: u8 = 1;
const MP3_OTHER_FRAME: u8 = 2;
const AAC_CONFIG: u8 = 3;
#[cfg(feature = "aac")]
const AAC_BLOCK: u8 = 4;
#[cfg(feature = "aac")]
const AAC_OTHER_BLOCK: u8 = 5;

/// What a `global_gain` of 0 is hashed as, and one whose channel codes no
/// spectral data: outside the range of a difference between two values that
/// are not 0 (-254..=254).
const ZERO_GAIN: i16 = i16::MIN;

fn v1(data: &[u8]) -> Result<[u8; 32]> {
    if crate::mp4meta::is_aac_data(data) {
        #[cfg(feature = "aac")]
        return aac_mp4(data);
        #[cfg(not(feature = "aac"))]
        return Err(Error::aac_unavailable());
    }
    // Scanning ALAC for MP3 frames would hash whatever happened to look like
    // one.
    if let Some(format) = crate::mp4meta::unsupported_mp4_format(data) {
        return Err(Error::UnsupportedFormat { format });
    }
    #[cfg(feature = "aac")]
    if crate::adts::is_adts_data(data) {
        return aac_adts(data);
    }
    mp3(data)
}

/// The SHA-256 input: the prefix, then one unit after another.
struct Units {
    sha: Sha256,
    /// The first `global_gain` that is not 0, or 0 when there is none.
    reference: u8,
    /// Units other than [`AAC_CONFIG`], so an empty stream can be refused.
    audio_units: usize,
    gains: Vec<u8>,
}

impl Units {
    fn new(codec: &str, reference: u8) -> Self {
        let mut sha = Sha256::new();
        sha.update(b"mp3rgain-audio-hash-v1\0");
        sha.update(codec.as_bytes());
        sha.update([0]);
        Self {
            sha,
            reference,
            audio_units: 0,
            gains: Vec::new(),
        }
    }

    /// One unit: its kind, the length and bytes of its body, and the count
    /// and canonical form of its `global_gain` values.
    fn push(&mut self, kind: u8, body: &[u8], gains: impl IntoIterator<Item = u8>) {
        self.gains.clear();
        for gain in gains {
            let canonical = if gain == 0 {
                ZERO_GAIN
            } else {
                i16::from(gain) - i16::from(self.reference)
            };
            self.gains.extend_from_slice(&canonical.to_le_bytes());
        }
        self.sha.update([kind]);
        self.sha.update((body.len() as u32).to_le_bytes());
        self.sha.update(body);
        self.sha
            .update(((self.gains.len() / 2) as u32).to_le_bytes());
        self.sha.update(&self.gains);
        if kind != AAC_CONFIG {
            self.audio_units += 1;
        }
    }

    fn finish(self, empty: impl FnOnce() -> Error) -> Result<[u8; 32]> {
        if self.audio_units == 0 {
            return Err(empty());
        }
        Ok(self.sha.finalize().into())
    }
}

// =============================================================================
// MP3
// =============================================================================

/// The `global_gain` at `loc` as hashed: 0 when its granule and channel code
/// no spectral data. `part2_3_length`, the 12 bits that start 21 bits before
/// `global_gain`, is the length of the scale factors and Huffman data, and
/// with none the granule decodes to silence whatever its gain.
fn mp3_gain(frame: &[u8], loc: &GainLocation) -> u8 {
    let start = loc.byte_offset * 8 + loc.bit_offset as usize - 21;
    let coded = (start..start + 12).any(|bit| frame[bit / 8] >> (7 - bit % 8) & 1 == 1);
    if coded {
        frame::read_gain_at(frame, loc)
    } else {
        0
    }
}

/// A Fraunhofer VBRI header sits 32 bytes after the 4-byte frame header,
/// whatever the MPEG version, channel mode, or CRC.
fn is_vbri_frame(frame: &[u8]) -> bool {
    frame.get(36..40) == Some(b"VBRI".as_slice())
}

/// `frame` as hashed: the private, copyright, and original bits cleared, the
/// `global_gain` fields at `gains` set to 0, and the CRC left out.
fn mp3_body(body: &mut Vec<u8>, frame: &[u8], header: &FrameHeader, gains: &[GainLocation]) {
    body.clear();
    body.extend_from_slice(frame);
    body[2] &= !0x01;
    body[3] &= !0x0C;
    for loc in gains {
        frame::write_gain_at(body, loc, 0);
    }
    if header.has_crc {
        body.drain(4..6);
    }
}

fn mp3(data: &[u8]) -> Result<[u8; 32]> {
    let start = frame::skip_id3v2(data);
    let audio_end = frame::find_audio_end(data);

    // The frames a gain change adjusts, found by the walk it uses.
    let mut frames = Vec::new();
    let mut pos = start;
    let mut reference = None;
    while let Some((frame_pos, header, next_pos)) =
        frame::next_frame(data, pos, audio_end, reference)
    {
        reference.get_or_insert((header.version, header.sample_rate));
        frames.push((frame_pos, header));
        pos = next_pos;
    }

    let mut locations = [GainLocation {
        byte_offset: 0,
        bit_offset: 0,
    }; MAX_GAIN_LOCATIONS];
    let first_nonzero = frames.iter().find_map(|(frame_pos, header)| {
        let frame = &data[*frame_pos..frame_pos + header.frame_size];
        if is_vbri_frame(frame) {
            return None;
        }
        let len = frame::calculate_gain_locations(0, header, &mut locations);
        locations[..len]
            .iter()
            .map(|loc| mp3_gain(frame, loc))
            .find(|&gain| gain != 0)
    });

    let mut units = Units::new("mp3", first_nonzero.unwrap_or(0));
    let mut body = Vec::new();
    let mut gap_start = start;
    for (frame_pos, header) in &frames {
        mp3_other_frames(&mut units, &mut body, data, gap_start, *frame_pos);
        let frame = &data[*frame_pos..frame_pos + header.frame_size];
        if !is_vbri_frame(frame) {
            let len = frame::calculate_gain_locations(0, header, &mut locations);
            let gains = &locations[..len];
            mp3_body(&mut body, frame, header, gains);
            let values = gains.iter().map(|loc| mp3_gain(frame, loc));
            units.push(MP3_FRAME, &body, values);
        }
        gap_start = frame_pos + header.frame_size;
    }
    mp3_other_frames(&mut units, &mut body, data, gap_start, audio_end);

    units.finish(|| Error::NoMp3Frames)
}

/// Layer III frames in `data[start..end]`, a stretch the gain walk passed
/// over: the frames of a part with another sample rate or MPEG version, say,
/// which a gain change leaves as they are. A frame counts when it fits and is
/// followed by another frame header or by `end`, so tags and junk in the
/// stretch are passed over byte by byte.
fn mp3_other_frames(units: &mut Units, body: &mut Vec<u8>, data: &[u8], start: usize, end: usize) {
    let mut pos = start;
    while pos + 4 <= end {
        if let Some(header) = frame::parse_header(&data[pos..]) {
            let next = pos + header.frame_size;
            let chained = next <= end
                && (next == end
                    || (next + 4 <= end && frame::parse_header(&data[next..]).is_some()));
            if chained {
                let frame = &data[pos..next];
                if !frame::is_xing_frame(data, pos, &header) && !is_vbri_frame(frame) {
                    mp3_body(body, frame, &header, &[]);
                    units.push(MP3_OTHER_FRAME, body, []);
                }
                pos = next;
                continue;
            }
        }
        pos += 1;
    }
}

// =============================================================================
// AAC
// =============================================================================

/// One raw_data_block in stream order, before the reference value is known.
#[cfg(feature = "aac")]
struct AacBlock<'a> {
    config: AudioSpecificConfig,
    bytes: &'a [u8],
    /// The block's `global_gain` fields in [`AacStream::gains`], or `None`
    /// when a gain change does not parse it.
    gains: Option<std::ops::Range<usize>>,
}

#[cfg(feature = "aac")]
#[derive(Default)]
struct AacStream<'a> {
    blocks: Vec<AacBlock<'a>>,
    gains: Vec<AacGainLocation>,
}

#[cfg(feature = "aac")]
impl<'a> AacStream<'a> {
    /// Add a block, parsing it at `parse_rate` when a gain change would.
    fn push(&mut self, config: AudioSpecificConfig, bytes: &'a [u8], parse_rate: Option<u32>) {
        let gains = parse_rate.and_then(|rate| {
            let start = self.gains.len();
            let mut reader = crate::aac::BitReader::new(bytes);
            match crate::aac::parse_raw_data_block(&mut reader, rate, &mut self.gains) {
                Ok(_) => Some(start..self.gains.len()),
                Err(_) => {
                    self.gains.truncate(start);
                    None
                }
            }
        });
        self.blocks.push(AacBlock {
            config,
            bytes,
            gains,
        });
    }

    fn finish(self, empty: &str) -> Result<[u8; 32]> {
        let first_nonzero = self.gains.iter().map(aac_gain).find(|&gain| gain != 0);
        let mut units = Units::new("aac", first_nonzero.unwrap_or(0));
        let mut config = None;
        let mut body = Vec::new();
        for block in &self.blocks {
            if config != Some(block.config) {
                let c = block.config;
                let mut bytes = [0u8; 6];
                bytes[0] = c.object_type;
                bytes[1..5].copy_from_slice(&c.sample_rate.to_le_bytes());
                bytes[5] = c.channel_config;
                units.push(AAC_CONFIG, &bytes, []);
                config = Some(c);
            }
            match &block.gains {
                Some(range) => {
                    let gains = &self.gains[range.clone()];
                    body.clear();
                    body.extend_from_slice(block.bytes);
                    for loc in gains {
                        let offset = loc.sample_byte_offset() as usize;
                        frame::write_bits_u8(&mut body, offset, loc.bit_offset(), 0);
                    }
                    units.push(AAC_BLOCK, &body, gains.iter().map(aac_gain));
                }
                None => units.push(AAC_OTHER_BLOCK, block.bytes, []),
            }
        }
        units.finish(|| Error::AacParse {
            message: empty.into(),
        })
    }
}

/// The `global_gain` at `loc` as hashed: 0 when the channel codes no scale
/// factor band (`max_sfb` 0), which leaves its spectrum empty.
#[cfg(feature = "aac")]
fn aac_gain(loc: &AacGainLocation) -> u8 {
    if loc.max_sfb() == 0 {
        0
    } else {
        loc.original_gain()
    }
}

/// The samples of the first AAC track of an MP4, parsed as
/// `aac::analyze_aac_gains_from_data` parses them for a gain change.
#[cfg(feature = "aac")]
fn aac_mp4(data: &[u8]) -> Result<[u8; 32]> {
    let (samples, stsd_pos) = crate::aac::build_sample_table(data)?;
    let parse_rate = crate::aac::parse_audio_config(data, stsd_pos)?;
    let config = crate::aac::mp4a_audio_config(data, stsd_pos).ok_or_else(|| Error::AacParse {
        message: "no AudioSpecificConfig".into(),
    })?;

    let mut stream = AacStream::default();
    for entry in &samples {
        let start = entry.file_offset as usize;
        // A sample past the end of a truncated file is left out, as a cut-off
        // MP3 frame is.
        let Some(end) = start
            .checked_add(entry.size as usize)
            .filter(|&end| end <= data.len())
        else {
            continue;
        };
        stream.push(config, &data[start..end], Some(parse_rate));
    }
    stream.finish("no AAC samples")
}

/// The frames of a raw ADTS stream: first those
/// `adts::analyze_adts_gains_from_data` walks for a gain change, then any
/// further ADTS frames after the point where that walk stops.
#[cfg(feature = "aac")]
fn aac_adts(data: &[u8]) -> Result<[u8; 32]> {
    use crate::adts::{parse_adts_header, AdtsHeader};

    let config = |header: &AdtsHeader| AudioSpecificConfig {
        object_type: header.object_type,
        sample_rate: header.sample_rate,
        channel_config: header.channel_config,
    };
    let start = frame::skip_id3v2(data);
    let end = frame::find_audio_end(data).max(start);
    let mut stream = AacStream::default();

    let mut pos = start;
    while pos < end {
        let Some(header) = parse_adts_header(&data[pos..end]) else {
            break;
        };
        let next = pos + header.frame_len;
        if next > end {
            break;
        }
        // A frame of several raw_data_blocks is not parsed by the gain change.
        let parse_rate = (header.blocks == 1).then_some(header.sample_rate);
        stream.push(
            config(&header),
            &data[pos + header.header_len..next],
            parse_rate,
        );
        pos = next;
    }

    while pos < end {
        if let Some(header) = parse_adts_header(&data[pos..end]) {
            let next = pos + header.frame_len;
            let chained =
                next <= end && (next == end || parse_adts_header(&data[next..end]).is_some());
            if chained {
                stream.push(config(&header), &data[pos + header.header_len..next], None);
                pos = next;
                continue;
            }
        }
        pos += 1;
    }

    stream.finish("no ADTS frames found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::GainMode;

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(Path::new("tests/fixtures").join(name)).unwrap()
    }

    fn hash(data: &[u8]) -> AudioHash {
        audio_hash_data(data, AudioHashVersion::V1).unwrap()
    }

    const MP3_FIXTURES: [&str; 6] = [
        "test_joint_stereo.mp3",
        "test_lame_gapless.mp3",
        "test_mono.mp3",
        "test_simple_stereo.mp3",
        "test_stereo.mp3",
        "test_vbr.mp3",
    ];

    /// The v1 value of every fixture. A change here means v1 changed, which
    /// it must never do once released: a different definition is a new
    /// version.
    #[test]
    fn v1_values_are_frozen() {
        let expected = [
            (
                "test_aac.m4a",
                "mp3rgain-v1:2fbf04994777e13f6ec53d3715e40663805e12f8ad82327577f5022c7c1b0c33",
            ),
            (
                "test_adts.aac",
                "mp3rgain-v1:2fbf04994777e13f6ec53d3715e40663805e12f8ad82327577f5022c7c1b0c33",
            ),
            (
                "test_he_aac.aac",
                "mp3rgain-v1:48ead0467c6137f8512c108def2182cd04d3f3b9ebcd3e7f6c49627ca040a844",
            ),
            (
                "test_he_aac.m4a",
                "mp3rgain-v1:48ead0467c6137f8512c108def2182cd04d3f3b9ebcd3e7f6c49627ca040a844",
            ),
            (
                "test_joint_stereo.mp3",
                "mp3rgain-v1:2909a53defb4860e84844d9ce4fe3f47ff9c0ab2b1fba510435ec7ee1037dddb",
            ),
            (
                "test_lame_gapless.mp3",
                "mp3rgain-v1:f5a559939bb258afcb4436ae4b770635a41b45938ce96ed2cfa3fae90352b309",
            ),
            (
                "test_mono.mp3",
                "mp3rgain-v1:7c151524fe1b7596530e6436d483e1e2ef7bf7b81c9d567511212ce66c555368",
            ),
            (
                "test_simple_stereo.mp3",
                "mp3rgain-v1:672ae0ea1554b38afe0e4c4b4e1fc58f626b4141b689fee7b2341a035955f9f3",
            ),
            (
                "test_stereo.mp3",
                "mp3rgain-v1:ea944a209fbcd4c63b788dbf5b5bb69a627040239866c4c688c857d86ac4e9e6",
            ),
            (
                "test_vbr.mp3",
                "mp3rgain-v1:23d6fd641cc85fde16f176eb6695611685229a8dc8bc8a0db59d75b3ef14b567",
            ),
        ];
        for (name, value) in expected {
            if cfg!(not(feature = "aac")) && !name.ends_with(".mp3") {
                continue;
            }
            assert_eq!(hash(&fixture(name)).to_string(), value, "{name}");
        }
    }

    #[test]
    fn version_parses_and_displays() {
        assert_eq!(AudioHashVersion::parse("1"), Some(AudioHashVersion::V1));
        assert_eq!(AudioHashVersion::parse("v1"), Some(AudioHashVersion::V1));
        assert_eq!(AudioHashVersion::parse("v2"), None);
        assert_eq!(AudioHashVersion::parse(""), None);
        assert_eq!(AudioHashVersion::V1.to_string(), "v1");
        let shown = hash(&fixture("test_mono.mp3")).to_string();
        assert!(shown.starts_with("mp3rgain-v1:"));
        assert_eq!(shown.len(), "mp3rgain-v1:".len() + 64);
    }

    /// The point of the hash: a gain change, in either direction, and its
    /// undo leave it as it was.
    #[test]
    fn mp3_gain_change_and_undo_keep_the_hash() {
        for name in MP3_FIXTURES {
            let original = fixture(name);
            let before = hash(&original);
            // test_stereo.mp3 sits at 255 throughout, so only a cut moves it.
            let (min, max) = frame::scan_gain_range(&original).unwrap();
            let fits = |steps: i32| {
                (1..=255).contains(&(i32::from(min) + steps))
                    && (1..=255).contains(&(i32::from(max) + steps))
            };
            for steps in [3, -2, 10].into_iter().filter(|&s| fits(s)) {
                let mut data = original.clone();
                frame::apply_gain_to_data(&mut data, steps, GainMode::Saturating, None);
                assert_ne!(data, original, "{name}: {steps} changed nothing");
                assert_eq!(hash(&data), before, "{name}: {steps} steps");
                frame::apply_gain_to_data(&mut data, -steps, GainMode::Saturating, None);
                assert_eq!(hash(&data), before, "{name}: undo of {steps} steps");
            }
        }
    }

    /// A change to one channel is a different mix, so a different hash.
    #[test]
    fn mp3_one_channel_change_changes_the_hash() {
        let original = fixture("test_simple_stereo.mp3");
        let mut data = original.clone();
        frame::apply_gain_to_data(&mut data, 2, GainMode::Saturating, Some(1));
        assert_ne!(data, original);
        assert_ne!(hash(&data), hash(&original));
    }

    /// Tags at either end, a different ID3v2 tag, and the Xing/Info frame are
    /// not part of the audio.
    #[test]
    fn mp3_tags_and_info_frame_are_left_out() {
        let original = fixture("test_lame_gapless.mp3");
        let before = hash(&original);
        let start = frame::skip_id3v2(&original);
        let audio = &original[start..frame::find_audio_end(&original)];

        let mut tagged = b"ID3\x04\x00\x00\x00\x00\x00\x0a".to_vec();
        tagged.extend_from_slice(b"0123456789");
        tagged.extend_from_slice(audio);
        let mut ape = crate::ape::ApeTag::new();
        ape.set_undo_gain(-2, -2, false);
        let mut tagged = crate::ape::replace_ape_tag(&tagged, &ape);
        tagged.extend_from_slice(b"TAG");
        tagged.resize(tagged.len() + 125, b' ');
        assert_eq!(hash(&tagged), before);

        let header = frame::parse_header(audio).unwrap();
        assert!(
            frame::is_xing_frame(audio, 0, &header),
            "fixture has no Info frame"
        );
        assert_eq!(hash(&audio[header.frame_size..]), before);
    }

    /// The private, copyright, and original bits say nothing about the audio,
    /// and a CRC covers them, so neither is hashed.
    #[test]
    fn mp3_informational_header_bits_are_left_out() {
        let original = fixture("test_simple_stereo.mp3");
        let mut data = original.clone();
        let mut pos = frame::skip_id3v2(&data);
        let end = frame::find_audio_end(&data);
        let mut flipped = 0;
        while let Some((frame_pos, _, next)) = frame::next_frame(&data, pos, end, None) {
            data[frame_pos + 2] ^= 0x01;
            data[frame_pos + 3] ^= 0x0C;
            flipped += 1;
            pos = next;
        }
        assert!(flipped > 1);
        assert_eq!(hash(&data), hash(&original));

        // Emphasis tells the decoder how to reproduce the signal, so it stays.
        let (first, _, _) = frame::next_frame(&data, frame::skip_id3v2(&data), end, None).unwrap();
        data[first + 3] ^= 0x01;
        assert_ne!(hash(&data), hash(&original));
    }

    /// One MPEG-1 Layer III frame, 128 kbps, no CRC, with `gain` in every
    /// `global_gain` field and `fill` in the main data. Each granule claims
    /// one bit of Huffman data, so that its gain counts.
    fn make_frame(sr_index: u8, channel_bits: u8, gain: u8, fill: u8) -> Vec<u8> {
        let mut frame = make_silent_frame(sr_index, channel_bits, gain, fill);
        let header = frame::parse_header(&frame).unwrap();
        let mut locations = [GainLocation {
            byte_offset: 0,
            bit_offset: 0,
        }; MAX_GAIN_LOCATIONS];
        let len = frame::calculate_gain_locations(0, &header, &mut locations);
        for loc in &locations[..len] {
            // The last bit of part2_3_length, 21 bits before global_gain.
            let bit = loc.byte_offset * 8 + loc.bit_offset as usize - 10;
            frame[bit / 8] |= 0x80 >> (bit % 8);
        }
        frame
    }

    /// [`make_frame`] with no coded spectral data, as encoders write
    /// digital silence.
    fn make_silent_frame(sr_index: u8, channel_bits: u8, gain: u8, fill: u8) -> Vec<u8> {
        let mut frame = vec![fill; 4 + 32];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, (9 << 4) | (sr_index << 2), channel_bits << 6]);
        let header = frame::parse_header(&frame).unwrap();
        frame.resize(header.frame_size, fill);
        frame[4..4 + header.side_info_len()].fill(0);
        let mut locations = [GainLocation {
            byte_offset: 0,
            bit_offset: 0,
        }; MAX_GAIN_LOCATIONS];
        let len = frame::calculate_gain_locations(0, &header, &mut locations);
        for loc in &locations[..len] {
            frame::write_gain_at(&mut frame, loc, gain);
        }
        frame
    }

    fn stream(frames: &[Vec<u8>]) -> Vec<u8> {
        frames.concat()
    }

    /// A gain of 0 stays 0 through a gain change, so it is hashed as a marker
    /// and the other values as their distance from the first one that is not
    /// 0. A wrapped change moves the 0 too, and is a different hash.
    #[test]
    fn mp3_zero_gain_keeps_the_hash_unless_wrapped() {
        let original = stream(&[
            make_frame(0, 0, 0, 0x11),
            make_frame(0, 0, 150, 0x22),
            make_frame(0, 0, 0, 0x33),
            make_frame(0, 0, 160, 0x44),
        ]);
        let before = hash(&original);

        let mut saturating = original.clone();
        frame::apply_gain_to_data(&mut saturating, 5, GainMode::Saturating, None);
        assert_eq!(hash(&saturating), before);

        let mut wrapped = original.clone();
        frame::apply_gain_to_data(&mut wrapped, 5, GainMode::Wrapping, None);
        assert_ne!(hash(&wrapped), before);

        // Clamping at 255 loses the distance between two values.
        let mut clamped = original.clone();
        frame::apply_gain_to_data(&mut clamped, 100, GainMode::Saturating, None);
        assert_ne!(hash(&clamped), before);

        // A fade made with the gain values is a different signal.
        let faded = stream(&[
            make_frame(0, 0, 0, 0x11),
            make_frame(0, 0, 150, 0x22),
            make_frame(0, 0, 0, 0x33),
            make_frame(0, 0, 155, 0x44),
        ]);
        assert_ne!(hash(&faded), before);
    }

    /// A granule with no coded spectral data is silent whatever its gain, so
    /// the gain is not hashed. Older tools moved it from 0 with the rest, and
    /// a later gain change could take it back to exactly 0.
    #[test]
    fn mp3_gain_of_a_silent_granule_is_left_out() {
        let audio = [make_frame(0, 0, 150, 0x22), make_frame(0, 0, 160, 0x23)];
        let original = stream(&[vec![make_silent_frame(0, 0, 0, 0)], audio.to_vec()].concat());
        let moved = stream(&[vec![make_silent_frame(0, 0, 7, 0)], audio.to_vec()].concat());
        assert_eq!(hash(&moved), hash(&original));

        let mut adjusted = moved.clone();
        frame::apply_gain_to_data(&mut adjusted, -7, GainMode::Saturating, None);
        assert_eq!(hash(&adjusted), hash(&original));
    }

    /// Frames with another sample rate are left alone by a gain change, so
    /// they are hashed as they are, and a file joined from two parts does not
    /// hash like its first part alone.
    #[test]
    fn mp3_frames_the_gain_walk_skips_are_hashed_as_they_are() {
        let first: Vec<Vec<u8>> = (0..4).map(|_| make_frame(0, 0, 150, 0x55)).collect();
        let second: Vec<Vec<u8>> = (0..3).map(|_| make_frame(1, 3, 140, 0x66)).collect();
        let first_part = stream(&first);
        let joined = stream(&[first.clone(), second.clone()].concat());
        let other_second: Vec<Vec<u8>> = (0..3).map(|_| make_frame(1, 3, 140, 0x67)).collect();
        let other_joined = stream(&[first, other_second].concat());

        let before = hash(&joined);
        assert_ne!(before, hash(&first_part));
        assert_ne!(before, hash(&other_joined));

        let mut adjusted = joined.clone();
        let stats = frame::apply_gain_to_data(&mut adjusted, 4, GainMode::Saturating, None);
        assert_eq!(stats.frames, 4, "the second part is not adjusted");
        assert_eq!(hash(&adjusted), before);
    }

    /// Bytes that are not a frame are passed over, and so is a frame cut off
    /// by the end of the file, as an incomplete download leaves it.
    #[test]
    fn mp3_junk_and_cut_off_frames_are_left_out() {
        let frames: Vec<Vec<u8>> = (0..4).map(|i| make_frame(0, 0, 150 + i, 0x55)).collect();
        let audio = stream(&frames);
        let before = hash(&audio);

        let mut junk_first = b"\0\0LYRICSBEGIN junk".to_vec();
        junk_first.extend_from_slice(&audio);
        assert_eq!(hash(&junk_first), before);

        let mut cut_off = audio.clone();
        cut_off.extend_from_slice(&make_frame(0, 0, 170, 0x56)[..200]);
        assert_eq!(hash(&cut_off), before);
    }

    /// A VBRI header frame can be rewritten or replaced by a Xing frame.
    #[test]
    fn mp3_vbri_frame_is_left_out() {
        let audio = stream(&[make_frame(0, 0, 150, 0x22), make_frame(0, 0, 151, 0x23)]);
        let mut vbri = make_frame(0, 0, 0, 0);
        vbri[36..40].copy_from_slice(b"VBRI");
        vbri[40..44].copy_from_slice(&[0, 1, 0, 0x4B]);
        let mut with_vbri = vbri;
        with_vbri.extend_from_slice(&audio);
        assert_eq!(hash(&with_vbri), hash(&audio));
    }

    #[cfg(feature = "aac")]
    #[test]
    fn aac_gain_change_keeps_the_hash() {
        for name in ["test_aac.m4a", "test_he_aac.m4a"] {
            let original = fixture(name);
            let before = hash(&original);
            let analysis = crate::aac::analyze_aac_gains_from_data(&original).unwrap();
            for steps in [3, -4] {
                let mut data = original.clone();
                crate::aac::apply_aac_gain_to_data(&mut data, &analysis, steps);
                assert_ne!(data, original);
                assert_eq!(hash(&data), before, "{name}: {steps} steps");
            }
        }
        for name in ["test_adts.aac", "test_he_aac.aac"] {
            let original = fixture(name);
            let before = hash(&original);
            let analysis = crate::adts::analyze_adts_gains_from_data(&original).unwrap();
            let mut data = original.clone();
            crate::aac::apply_aac_gain_to_data(&mut data, &analysis, 3);
            assert_ne!(data, original);
            assert_eq!(hash(&data), before, "{name}");
        }
    }

    /// The raw ADTS stream extracted from an M4A, as `ffmpeg -c copy -f adts`
    /// writes it, has the same hash: neither container is hashed.
    #[cfg(feature = "aac")]
    #[test]
    fn m4a_and_its_adts_stream_hash_alike() {
        for name in ["test_aac.m4a", "test_he_aac.m4a"] {
            let m4a = fixture(name);
            let (samples, stsd_pos) = crate::aac::build_sample_table(&m4a).unwrap();
            let asc = crate::aac::mp4a_audio_config(&m4a, stsd_pos).unwrap();
            let index = crate::aac::SAMPLE_RATE_TABLE
                .iter()
                .position(|&rate| rate == asc.sample_rate)
                .unwrap() as u8;
            let mut adts = Vec::new();
            for entry in &samples {
                let start = entry.file_offset as usize;
                let block = &m4a[start..start + entry.size as usize];
                let len = block.len() + 7;
                adts.extend_from_slice(&[
                    0xFF,
                    0xF1,
                    ((asc.object_type - 1) << 6) | (index << 2) | (asc.channel_config >> 2),
                    ((asc.channel_config & 3) << 6) | (len >> 11) as u8,
                    (len >> 3) as u8,
                    ((len & 7) << 5) as u8 | 0x1F,
                    0xFC,
                ]);
                adts.extend_from_slice(block);
            }
            assert!(crate::adts::is_adts_data(&adts));
            assert_eq!(hash(&adts), hash(&m4a), "{name}");
        }
    }

    #[test]
    fn alac_has_no_hash() {
        let err = audio_hash_data(&fixture("test_alac.m4a"), AudioHashVersion::V1).unwrap_err();
        assert!(err.is_unsupported_format(), "{err}");
        let err = audio_hash(
            Path::new("tests/fixtures/test_alac.m4a"),
            AudioHashVersion::V1,
        )
        .unwrap_err();
        assert!(err.is_unsupported_format(), "{err}");
    }

    #[test]
    fn data_without_audio_has_no_hash() {
        assert!(matches!(
            audio_hash_data(&[0u8; 4096], AudioHashVersion::V1),
            Err(Error::NoMp3Frames)
        ));
    }
}
