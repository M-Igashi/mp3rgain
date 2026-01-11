//! # mp3rgain
//!
//! Lossless MP3 volume adjustment library - a modern mp3gain replacement.
//!
//! This library provides lossless MP3 volume adjustment by modifying
//! the `global_gain` field in each frame's side information.
//!
//! ## Features
//!
//! - **Lossless**: No re-encoding, preserves audio quality
//! - **Fast**: Direct binary manipulation, no audio decoding
//! - **Compatible**: Works with all MP3 files (MPEG1/2/2.5 Layer III)
//! - **Reversible**: Changes can be undone by applying negative gain
//!
//! ## Example
//!
//! ```no_run
//! use mp3rgain::{apply_gain, apply_gain_db, analyze};
//! use std::path::Path;
//!
//! // Apply +2 gain steps (+3.0 dB)
//! let frames = apply_gain(Path::new("song.mp3"), 2).unwrap();
//! println!("Modified {} frames", frames);
//!
//! // Or specify gain in dB directly
//! let frames = apply_gain_db(Path::new("song.mp3"), 4.5).unwrap();
//! ```
//!
//! ## Technical Details
//!
//! Each gain step equals 1.5 dB (fixed by MP3 specification).
//! The global_gain field is 8 bits, allowing values 0-255.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

/// MP3 gain step size in dB (fixed by format specification)
pub const GAIN_STEP_DB: f64 = 1.5;

/// Maximum global_gain value
pub const MAX_GAIN: u8 = 255;

/// Minimum global_gain value
pub const MIN_GAIN: u8 = 0;

/// Result of MP3 file analysis
#[derive(Debug, Clone)]
pub struct Mp3Analysis {
    /// Number of audio frames in the file
    pub frame_count: usize,
    /// MPEG version detected (1, 2, or 2.5)
    pub mpeg_version: String,
    /// Channel mode (Stereo, Joint Stereo, Dual Channel, Mono)
    pub channel_mode: String,
    /// Minimum global_gain value found across all granules
    pub min_gain: u8,
    /// Maximum global_gain value found across all granules
    pub max_gain: u8,
    /// Average global_gain value
    pub avg_gain: f64,
    /// Maximum safe positive adjustment in steps (before clipping)
    pub headroom_steps: i32,
    /// Maximum safe positive adjustment in dB
    pub headroom_db: f64,
}

/// MPEG version
#[derive(Debug, Clone, Copy, PartialEq)]
enum MpegVersion {
    Mpeg1,
    Mpeg2,
    Mpeg25,
}

impl MpegVersion {
    fn as_str(&self) -> &'static str {
        match self {
            MpegVersion::Mpeg1 => "MPEG1",
            MpegVersion::Mpeg2 => "MPEG2",
            MpegVersion::Mpeg25 => "MPEG2.5",
        }
    }
}

/// Channel mode
#[derive(Debug, Clone, Copy, PartialEq)]
enum ChannelMode {
    Stereo,
    JointStereo,
    DualChannel,
    Mono,
}

impl ChannelMode {
    fn channel_count(&self) -> usize {
        match self {
            ChannelMode::Mono => 1,
            _ => 2,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            ChannelMode::Stereo => "Stereo",
            ChannelMode::JointStereo => "Joint Stereo",
            ChannelMode::DualChannel => "Dual Channel",
            ChannelMode::Mono => "Mono",
        }
    }
}

/// Parsed MP3 frame header
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct FrameHeader {
    version: MpegVersion,
    has_crc: bool,
    bitrate_kbps: u32,
    sample_rate: u32,
    padding: bool,
    channel_mode: ChannelMode,
    frame_size: usize,
}

impl FrameHeader {
    fn granule_count(&self) -> usize {
        match self.version {
            MpegVersion::Mpeg1 => 2,
            _ => 1,
        }
    }

    fn side_info_offset(&self) -> usize {
        if self.has_crc {
            6
        } else {
            4
        }
    }
}

/// Bitrate table for MPEG1 Layer III
const BITRATE_TABLE_MPEG1_L3: [u32; 15] = [
    0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
];

/// Bitrate table for MPEG2/2.5 Layer III
const BITRATE_TABLE_MPEG2_L3: [u32; 15] =
    [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

/// Sample rate table
const SAMPLE_RATE_TABLE: [[u32; 3]; 3] = [
    [44100, 48000, 32000], // MPEG1
    [22050, 24000, 16000], // MPEG2
    [11025, 12000, 8000],  // MPEG2.5
];

/// Parse a 4-byte frame header
fn parse_header(header: &[u8]) -> Option<FrameHeader> {
    if header.len() < 4 {
        return None;
    }

    // Check sync word (11 bits: 0xFF + upper 3 bits of second byte)
    if header[0] != 0xFF || (header[1] & 0xE0) != 0xE0 {
        return None;
    }

    // MPEG version (bits 4-3 of byte 1)
    let version_bits = (header[1] >> 3) & 0x03;
    let version = match version_bits {
        0b00 => MpegVersion::Mpeg25,
        0b10 => MpegVersion::Mpeg2,
        0b11 => MpegVersion::Mpeg1,
        _ => return None,
    };

    // Layer (bits 2-1 of byte 1) - only Layer III supported
    let layer_bits = (header[1] >> 1) & 0x03;
    if layer_bits != 0b01 {
        return None;
    }

    // Protection bit (bit 0 of byte 1) - 0 means CRC present
    let has_crc = (header[1] & 0x01) == 0;

    // Bitrate index (bits 7-4 of byte 2)
    let bitrate_index = (header[2] >> 4) & 0x0F;
    if bitrate_index == 0 || bitrate_index == 15 {
        return None;
    }

    let bitrate_kbps = match version {
        MpegVersion::Mpeg1 => BITRATE_TABLE_MPEG1_L3[bitrate_index as usize],
        _ => BITRATE_TABLE_MPEG2_L3[bitrate_index as usize],
    };

    // Sample rate index (bits 3-2 of byte 2)
    let sr_index = ((header[2] >> 2) & 0x03) as usize;
    if sr_index == 3 {
        return None;
    }

    let version_index = match version {
        MpegVersion::Mpeg1 => 0,
        MpegVersion::Mpeg2 => 1,
        MpegVersion::Mpeg25 => 2,
    };
    let sample_rate = SAMPLE_RATE_TABLE[version_index][sr_index];

    // Padding (bit 1 of byte 2)
    let padding = (header[2] & 0x02) != 0;

    // Channel mode (bits 7-6 of byte 3)
    let channel_bits = (header[3] >> 6) & 0x03;
    let channel_mode = match channel_bits {
        0b00 => ChannelMode::Stereo,
        0b01 => ChannelMode::JointStereo,
        0b10 => ChannelMode::DualChannel,
        0b11 => ChannelMode::Mono,
        _ => unreachable!(),
    };

    // Calculate frame size
    let samples_per_frame = match version {
        MpegVersion::Mpeg1 => 1152,
        _ => 576,
    };
    let padding_size = if padding { 1 } else { 0 };
    let frame_size =
        (samples_per_frame * bitrate_kbps as usize * 125) / sample_rate as usize + padding_size;

    Some(FrameHeader {
        version,
        has_crc,
        bitrate_kbps,
        sample_rate,
        padding,
        channel_mode,
        frame_size,
    })
}

/// Location of a global_gain field within the file
#[derive(Debug, Clone)]
struct GainLocation {
    byte_offset: usize,
    bit_offset: u8,
}

/// Calculate global_gain locations within a frame's side information
fn calculate_gain_locations(frame_offset: usize, header: &FrameHeader) -> Vec<GainLocation> {
    let mut locations = Vec::new();
    let side_info_start = frame_offset + header.side_info_offset();

    let num_channels = header.channel_mode.channel_count();
    let num_granules = header.granule_count();

    let bits_before_granules = match (header.version, num_channels) {
        (MpegVersion::Mpeg1, 1) => 18,
        (MpegVersion::Mpeg1, _) => 20,
        (_, 1) => 9,
        (_, _) => 10,
    };

    let bits_per_granule_channel = match header.version {
        MpegVersion::Mpeg1 => 59,
        _ => 63,
    };

    for gr in 0..num_granules {
        for ch in 0..num_channels {
            let granule_start_bit =
                bits_before_granules + (gr * num_channels + ch) * bits_per_granule_channel;
            let global_gain_bit = granule_start_bit + 21;

            let byte_offset = side_info_start + global_gain_bit / 8;
            let bit_offset = (global_gain_bit % 8) as u8;

            locations.push(GainLocation {
                byte_offset,
                bit_offset,
            });
        }
    }

    locations
}

/// Read 8-bit value at bit-unaligned position
fn read_gain_at(data: &[u8], loc: &GainLocation) -> u8 {
    let idx = loc.byte_offset;
    if idx >= data.len() {
        return 0;
    }

    if loc.bit_offset == 0 {
        data[idx]
    } else if idx + 1 < data.len() {
        let shift = loc.bit_offset;
        let high = (data[idx] << shift) as u8;
        let low = data[idx + 1] >> (8 - shift);
        high | low
    } else {
        data[idx] << loc.bit_offset
    }
}

/// Write 8-bit value at bit-unaligned position
fn write_gain_at(data: &mut [u8], loc: &GainLocation, value: u8) {
    let idx = loc.byte_offset;
    if idx >= data.len() {
        return;
    }

    if loc.bit_offset == 0 {
        data[idx] = value;
    } else if idx + 1 < data.len() {
        let shift = loc.bit_offset;
        let mask_high = 0xFFu8 << (8 - shift);
        let mask_low = 0xFFu8 >> shift;

        data[idx] = (data[idx] & mask_high) | (value >> shift);
        data[idx + 1] = (data[idx + 1] & mask_low) | (value << (8 - shift));
    } else {
        let shift = loc.bit_offset;
        let mask_high = 0xFFu8 << (8 - shift);
        data[idx] = (data[idx] & mask_high) | (value >> shift);
    }
}

/// Skip ID3v2 tag at beginning of data
fn skip_id3v2(data: &[u8]) -> usize {
    if data.len() < 10 || &data[0..3] != b"ID3" {
        return 0;
    }

    let size = ((data[6] as usize & 0x7F) << 21)
        | ((data[7] as usize & 0x7F) << 14)
        | ((data[8] as usize & 0x7F) << 7)
        | (data[9] as usize & 0x7F);

    10 + size
}

/// Internal function to iterate over frames
fn iterate_frames<F>(data: &[u8], mut callback: F) -> Result<usize>
where
    F: FnMut(usize, &FrameHeader, &[GainLocation]),
{
    let file_size = data.len();
    let mut pos = skip_id3v2(data);
    let mut frame_count = 0;

    while pos + 4 <= file_size {
        let header = match parse_header(&data[pos..]) {
            Some(h) => h,
            None => {
                pos += 1;
                continue;
            }
        };

        let next_pos = pos + header.frame_size;
        let valid_frame = if next_pos + 2 <= file_size {
            data[next_pos] == 0xFF && (data[next_pos + 1] & 0xE0) == 0xE0
        } else {
            next_pos <= file_size
        };

        if !valid_frame {
            pos += 1;
            continue;
        }

        let locations = calculate_gain_locations(pos, &header);
        callback(pos, &header, &locations);

        frame_count += 1;
        pos = next_pos;
    }

    Ok(frame_count)
}

/// Analyze an MP3 file and return gain statistics
///
/// # Arguments
/// * `file_path` - Path to MP3 file
///
/// # Returns
/// * Analysis results including frame count, gain range, and headroom
pub fn analyze(file_path: &Path) -> Result<Mp3Analysis> {
    let data =
        fs::read(file_path).with_context(|| format!("Failed to read: {}", file_path.display()))?;

    let mut min_gain = 255u8;
    let mut max_gain = 0u8;
    let mut total_gain: u64 = 0;
    let mut gain_count: u64 = 0;
    let mut first_version = None;
    let mut first_channel_mode = None;

    let frame_count = iterate_frames(&data, |_pos, header, locations| {
        if first_version.is_none() {
            first_version = Some(header.version);
            first_channel_mode = Some(header.channel_mode);
        }

        for loc in locations {
            let gain = read_gain_at(&data, loc);
            min_gain = min_gain.min(gain);
            max_gain = max_gain.max(gain);
            total_gain += gain as u64;
            gain_count += 1;
        }
    })?;

    if frame_count == 0 {
        anyhow::bail!("No valid MP3 frames found");
    }

    let avg_gain = total_gain as f64 / gain_count as f64;
    let headroom_steps = (MAX_GAIN - max_gain) as i32;
    let headroom_db = headroom_steps as f64 * GAIN_STEP_DB;

    Ok(Mp3Analysis {
        frame_count,
        mpeg_version: first_version.unwrap().as_str().to_string(),
        channel_mode: first_channel_mode.unwrap().as_str().to_string(),
        min_gain,
        max_gain,
        avg_gain,
        headroom_steps,
        headroom_db,
    })
}

/// Apply gain adjustment to MP3 file (lossless)
///
/// # Arguments
/// * `file_path` - Path to MP3 file
/// * `gain_steps` - Number of 1.5dB steps to apply (positive = louder)
///
/// # Returns
/// * Number of frames modified
pub fn apply_gain(file_path: &Path, gain_steps: i32) -> Result<usize> {
    if gain_steps == 0 {
        return Ok(0);
    }

    let mut data =
        fs::read(file_path).with_context(|| format!("Failed to read: {}", file_path.display()))?;

    let mut modified_frames = 0;
    let file_size = data.len();
    let mut pos = skip_id3v2(&data);

    while pos + 4 <= file_size {
        let header = match parse_header(&data[pos..]) {
            Some(h) => h,
            None => {
                pos += 1;
                continue;
            }
        };

        let next_pos = pos + header.frame_size;
        let valid_frame = if next_pos + 2 <= file_size {
            data[next_pos] == 0xFF && (data[next_pos + 1] & 0xE0) == 0xE0
        } else {
            next_pos <= file_size
        };

        if !valid_frame {
            pos += 1;
            continue;
        }

        let locations = calculate_gain_locations(pos, &header);

        for loc in &locations {
            let current_gain = read_gain_at(&data, loc);
            let new_gain = if gain_steps > 0 {
                current_gain.saturating_add(gain_steps.min(255) as u8)
            } else {
                current_gain.saturating_sub((-gain_steps).min(255) as u8)
            };
            write_gain_at(&mut data, loc, new_gain);
        }

        modified_frames += 1;
        pos = next_pos;
    }

    fs::write(file_path, &data)
        .with_context(|| format!("Failed to write: {}", file_path.display()))?;

    Ok(modified_frames)
}

/// Apply gain adjustment in dB (converted to nearest step)
///
/// # Arguments
/// * `file_path` - Path to MP3 file
/// * `gain_db` - Gain in decibels (positive = louder)
///
/// # Returns
/// * Number of frames modified
pub fn apply_gain_db(file_path: &Path, gain_db: f64) -> Result<usize> {
    let steps = db_to_steps(gain_db);
    apply_gain(file_path, steps)
}

/// Convert dB gain to MP3 gain steps
pub fn db_to_steps(db: f64) -> i32 {
    (db / GAIN_STEP_DB).round() as i32
}

/// Convert MP3 gain steps to dB
pub fn steps_to_db(steps: i32) -> f64 {
    steps as f64 * GAIN_STEP_DB
}

// =============================================================================
// APEv2 Tag Support
// =============================================================================

/// APEv2 tag preamble
const APE_PREAMBLE: &[u8; 8] = b"APETAGEX";

/// APEv2 tag version
const APE_VERSION: u32 = 2000;

/// APEv2 tag flags
const APE_FLAG_HEADER_PRESENT: u32 = 1 << 31;
const APE_FLAG_IS_HEADER: u32 = 1 << 29;

/// MP3Gain specific tag keys
pub const TAG_MP3GAIN_UNDO: &str = "MP3GAIN_UNDO";
pub const TAG_MP3GAIN_MINMAX: &str = "MP3GAIN_MINMAX";

/// APEv2 tag item
#[derive(Debug, Clone)]
pub struct ApeItem {
    pub key: String,
    pub value: String,
}

/// APEv2 tag collection
#[derive(Debug, Clone, Default)]
pub struct ApeTag {
    items: Vec<ApeItem>,
}

impl ApeTag {
    /// Create a new empty APE tag
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Get a tag value by key (case-insensitive)
    pub fn get(&self, key: &str) -> Option<&str> {
        let key_upper = key.to_uppercase();
        self.items
            .iter()
            .find(|item| item.key.to_uppercase() == key_upper)
            .map(|item| item.value.as_str())
    }

    /// Set a tag value (replaces existing if present)
    pub fn set(&mut self, key: &str, value: &str) {
        let key_upper = key.to_uppercase();
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.key.to_uppercase() == key_upper)
        {
            item.value = value.to_string();
        } else {
            self.items.push(ApeItem {
                key: key_upper,
                value: value.to_string(),
            });
        }
    }

    /// Remove a tag by key
    pub fn remove(&mut self, key: &str) {
        let key_upper = key.to_uppercase();
        self.items
            .retain(|item| item.key.to_uppercase() != key_upper);
    }

    /// Check if tag is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get MP3GAIN_UNDO value as gain steps
    pub fn get_undo_gain(&self) -> Option<i32> {
        self.get(TAG_MP3GAIN_UNDO).and_then(|v| {
            // Format: "+002,+002,N" or similar
            // First field is the left channel adjustment, second is right
            let parts: Vec<&str> = v.split(',').collect();
            if !parts.is_empty() {
                parts[0].trim().parse::<i32>().ok()
            } else {
                None
            }
        })
    }

    /// Set MP3GAIN_UNDO value
    pub fn set_undo_gain(&mut self, left_gain: i32, right_gain: i32, wrap: bool) {
        let wrap_flag = if wrap { "W" } else { "N" };
        let value = format!("{:+04},{:+04},{}", left_gain, right_gain, wrap_flag);
        self.set(TAG_MP3GAIN_UNDO, &value);
    }

    /// Set MP3GAIN_MINMAX value
    pub fn set_minmax(&mut self, min: u8, max: u8) {
        let value = format!("{},{}", min, max);
        self.set(TAG_MP3GAIN_MINMAX, &value);
    }
}

/// Find APEv2 tag footer position in file data
fn find_ape_footer(data: &[u8]) -> Option<usize> {
    if data.len() < 32 {
        return None;
    }

    // Check for APE tag at end of file
    let footer_start = data.len() - 32;
    if &data[footer_start..footer_start + 8] == APE_PREAMBLE {
        return Some(footer_start);
    }

    // Check if there's an ID3v1 tag (128 bytes) before APE footer
    if data.len() >= 160 {
        let footer_start = data.len() - 32 - 128;
        if &data[footer_start..footer_start + 8] == APE_PREAMBLE
            && &data[data.len() - 128..data.len() - 125] == b"TAG"
        {
            return Some(footer_start);
        }
    }

    None
}

/// Read u32 little-endian from slice
fn read_u32_le(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Read APEv2 tag from file data
pub fn read_ape_tag(data: &[u8]) -> Option<ApeTag> {
    let footer_start = find_ape_footer(data)?;

    // Parse footer
    let version = read_u32_le(&data[footer_start + 8..]);
    if version != APE_VERSION {
        return None;
    }

    let tag_size = read_u32_le(&data[footer_start + 12..]) as usize;
    let item_count = read_u32_le(&data[footer_start + 16..]) as usize;

    // Calculate items start (tag_size includes items + footer, not header)
    if footer_start + 32 < tag_size {
        return None;
    }
    let items_start = footer_start + 32 - tag_size;

    // Parse items
    let mut tag = ApeTag::new();
    let mut pos = items_start;

    for _ in 0..item_count {
        if pos + 8 > footer_start {
            break;
        }

        let value_size = read_u32_le(&data[pos..]) as usize;
        pos += 8; // skip value_size + flags

        // Find null-terminated key
        let key_start = pos;
        while pos < footer_start && data[pos] != 0 {
            pos += 1;
        }
        if pos >= footer_start {
            break;
        }

        let key = String::from_utf8_lossy(&data[key_start..pos]).to_string();
        pos += 1; // skip null terminator

        // Read value
        if pos + value_size > footer_start {
            break;
        }
        let value = String::from_utf8_lossy(&data[pos..pos + value_size]).to_string();
        pos += value_size;

        tag.items.push(ApeItem { key, value });
    }

    Some(tag)
}

/// Read APEv2 tag from file
pub fn read_ape_tag_from_file(file_path: &Path) -> Result<Option<ApeTag>> {
    let data =
        fs::read(file_path).with_context(|| format!("Failed to read: {}", file_path.display()))?;
    Ok(read_ape_tag(&data))
}

/// Serialize APE tag to bytes
fn serialize_ape_tag(tag: &ApeTag) -> Vec<u8> {
    if tag.is_empty() {
        return Vec::new();
    }

    let mut items_data = Vec::new();

    // Serialize items
    for item in &tag.items {
        let value_bytes = item.value.as_bytes();
        let key_bytes = item.key.as_bytes();

        // Value size (4 bytes)
        items_data.extend_from_slice(&(value_bytes.len() as u32).to_le_bytes());
        // Item flags (4 bytes) - 0 for UTF-8 text
        items_data.extend_from_slice(&0u32.to_le_bytes());
        // Key (null-terminated)
        items_data.extend_from_slice(key_bytes);
        items_data.push(0);
        // Value
        items_data.extend_from_slice(value_bytes);
    }

    let tag_size = items_data.len() + 32; // items + footer
    let item_count = tag.items.len() as u32;

    let mut result = Vec::new();

    // Header
    result.extend_from_slice(APE_PREAMBLE);
    result.extend_from_slice(&APE_VERSION.to_le_bytes());
    result.extend_from_slice(&(tag_size as u32).to_le_bytes());
    result.extend_from_slice(&item_count.to_le_bytes());
    result.extend_from_slice(&(APE_FLAG_HEADER_PRESENT | APE_FLAG_IS_HEADER).to_le_bytes());
    result.extend_from_slice(&[0u8; 8]); // reserved

    // Items
    result.extend_from_slice(&items_data);

    // Footer
    result.extend_from_slice(APE_PREAMBLE);
    result.extend_from_slice(&APE_VERSION.to_le_bytes());
    result.extend_from_slice(&(tag_size as u32).to_le_bytes());
    result.extend_from_slice(&item_count.to_le_bytes());
    result.extend_from_slice(&APE_FLAG_HEADER_PRESENT.to_le_bytes());
    result.extend_from_slice(&[0u8; 8]); // reserved

    result
}

/// Remove existing APE tag from file data, returning the audio data portion
fn remove_ape_tag(data: &[u8]) -> Vec<u8> {
    let footer_start = match find_ape_footer(data) {
        Some(pos) => pos,
        None => return data.to_vec(),
    };

    // Get tag size from footer
    let tag_size = read_u32_le(&data[footer_start + 12..]) as usize;
    let flags = read_u32_le(&data[footer_start + 20..]);
    let has_header = (flags & APE_FLAG_HEADER_PRESENT) != 0;
    let header_size = if has_header { 32 } else { 0 };

    // Calculate where audio ends
    let audio_end = if footer_start + 32 >= tag_size + header_size {
        footer_start + 32 - tag_size - header_size
    } else {
        0
    };

    // Check for ID3v1 after APE
    let id3v1_start = footer_start + 32;
    let has_id3v1 = data.len() > id3v1_start + 3 && &data[id3v1_start..id3v1_start + 3] == b"TAG";

    if has_id3v1 {
        // Keep audio + ID3v1
        let mut result = data[..audio_end].to_vec();
        result.extend_from_slice(&data[id3v1_start..]);
        result
    } else {
        data[..audio_end].to_vec()
    }
}

/// Write APEv2 tag to file
pub fn write_ape_tag(file_path: &Path, tag: &ApeTag) -> Result<()> {
    let data =
        fs::read(file_path).with_context(|| format!("Failed to read: {}", file_path.display()))?;

    // Remove existing APE tag
    let mut audio_data = remove_ape_tag(&data);

    // Check for ID3v1 at end
    let has_id3v1 = audio_data.len() >= 128
        && &audio_data[audio_data.len() - 128..audio_data.len() - 125] == b"TAG";

    // Serialize new tag
    let tag_data = serialize_ape_tag(tag);

    // Reconstruct file: audio + APE tag + ID3v1 (if present)
    if has_id3v1 {
        let id3v1 = audio_data[audio_data.len() - 128..].to_vec();
        audio_data.truncate(audio_data.len() - 128);
        audio_data.extend_from_slice(&tag_data);
        audio_data.extend_from_slice(&id3v1);
    } else {
        audio_data.extend_from_slice(&tag_data);
    }

    fs::write(file_path, &audio_data)
        .with_context(|| format!("Failed to write: {}", file_path.display()))?;

    Ok(())
}

/// Delete APEv2 tag from file
pub fn delete_ape_tag(file_path: &Path) -> Result<()> {
    let data =
        fs::read(file_path).with_context(|| format!("Failed to read: {}", file_path.display()))?;

    let audio_data = remove_ape_tag(&data);

    fs::write(file_path, &audio_data)
        .with_context(|| format!("Failed to write: {}", file_path.display()))?;

    Ok(())
}

/// Apply gain and store undo information in APEv2 tag
pub fn apply_gain_with_undo(file_path: &Path, gain_steps: i32) -> Result<usize> {
    if gain_steps == 0 {
        return Ok(0);
    }

    // First, get current min/max before modification
    let analysis = analyze(file_path)?;

    // Read existing APE tag or create new one
    let mut tag = read_ape_tag_from_file(file_path)?.unwrap_or_else(ApeTag::new);

    // Store or update undo information
    let existing_undo = tag.get_undo_gain().unwrap_or(0);
    let new_undo = existing_undo + gain_steps;
    tag.set_undo_gain(new_undo, new_undo, false);

    // Store original min/max if not already stored
    if tag.get(TAG_MP3GAIN_MINMAX).is_none() {
        tag.set_minmax(analysis.min_gain, analysis.max_gain);
    }

    // Apply the gain
    let frames = apply_gain(file_path, gain_steps)?;

    // Write APE tag
    write_ape_tag(file_path, &tag)?;

    Ok(frames)
}

/// Undo gain changes based on APEv2 tag information
pub fn undo_gain(file_path: &Path) -> Result<usize> {
    let tag = read_ape_tag_from_file(file_path)?
        .ok_or_else(|| anyhow::anyhow!("No APE tag found - cannot undo"))?;

    let undo_gain = tag
        .get_undo_gain()
        .ok_or_else(|| anyhow::anyhow!("No MP3GAIN_UNDO tag found - cannot undo"))?;

    if undo_gain == 0 {
        return Ok(0);
    }

    // Apply inverse gain
    let frames = apply_gain(file_path, -undo_gain)?;

    // Update or remove undo tag
    let mut new_tag = tag.clone();
    new_tag.remove(TAG_MP3GAIN_UNDO);
    new_tag.remove(TAG_MP3GAIN_MINMAX);

    if new_tag.is_empty() {
        delete_ape_tag(file_path)?;
    } else {
        write_ape_tag(file_path, &new_tag)?;
    }

    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_to_steps() {
        assert_eq!(db_to_steps(0.0), 0);
        assert_eq!(db_to_steps(1.5), 1);
        assert_eq!(db_to_steps(3.0), 2);
        assert_eq!(db_to_steps(-1.5), -1);
        assert_eq!(db_to_steps(2.25), 2);
    }

    #[test]
    fn test_steps_to_db() {
        assert_eq!(steps_to_db(0), 0.0);
        assert_eq!(steps_to_db(1), 1.5);
        assert_eq!(steps_to_db(-2), -3.0);
    }

    #[test]
    fn test_parse_valid_header() {
        let header = [0xFF, 0xFB, 0x90, 0x00];
        let parsed = parse_header(&header);
        assert!(parsed.is_some());
        let h = parsed.unwrap();
        assert_eq!(h.version, MpegVersion::Mpeg1);
        assert_eq!(h.bitrate_kbps, 128);
        assert_eq!(h.sample_rate, 44100);
    }

    #[test]
    fn test_parse_invalid_header() {
        assert!(parse_header(&[0x00, 0x00, 0x00, 0x00]).is_none());
        assert!(parse_header(&[0xFF, 0xFF, 0x90, 0x00]).is_none());
    }

    #[test]
    fn test_bit_operations() {
        let mut data = vec![0xAB, 0xCD, 0xEF, 0x12, 0x34];

        let loc_aligned = GainLocation {
            byte_offset: 1,
            bit_offset: 0,
        };
        assert_eq!(read_gain_at(&data, &loc_aligned), 0xCD);

        let loc_unaligned = GainLocation {
            byte_offset: 1,
            bit_offset: 4,
        };
        assert_eq!(read_gain_at(&data, &loc_unaligned), 0xDE);

        write_gain_at(&mut data, &loc_aligned, 0x42);
        assert_eq!(data[1], 0x42);

        data = vec![0xAB, 0xCD, 0xEF, 0x12, 0x34];
        write_gain_at(&mut data, &loc_unaligned, 0x99);
        assert_eq!(data[1], 0xC9);
        assert_eq!(data[2], 0x9F);
    }

    #[test]
    fn test_skip_id3v2() {
        let data_no_tag = vec![0xFF, 0xFB, 0x90, 0x00];
        assert_eq!(skip_id3v2(&data_no_tag), 0);

        let data_with_tag = vec![b'I', b'D', b'3', 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(skip_id3v2(&data_with_tag), 10);
    }
}
