use crate::analysis::{ChannelMode, MpegVersion};
use crate::error::{Error, Result};

/// APEv2 tag preamble (needed for find_audio_end)
pub(crate) const APE_PREAMBLE: &[u8; 8] = b"APETAGEX";

/// APEv2 flag: header present
pub(crate) const APE_FLAG_HEADER_PRESENT: u32 = 1 << 31;

/// Parsed MP3 frame header
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) struct FrameHeader {
    pub version: MpegVersion,
    pub has_crc: bool,
    pub bitrate_kbps: u32,
    pub sample_rate: u32,
    pub padding: bool,
    pub channel_mode: ChannelMode,
    pub frame_size: usize,
}

impl FrameHeader {
    pub fn granule_count(&self) -> usize {
        match self.version {
            MpegVersion::Mpeg1 => 2,
            _ => 1,
        }
    }

    pub fn side_info_offset(&self) -> usize {
        if self.has_crc { 6 } else { 4 }
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
pub(crate) fn parse_header(header: &[u8]) -> Option<FrameHeader> {
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
pub(crate) struct GainLocation {
    pub byte_offset: usize,
    pub bit_offset: u8,
}

/// Calculate global_gain locations within a frame's side information
pub(crate) fn calculate_gain_locations(
    frame_offset: usize,
    header: &FrameHeader,
) -> Vec<GainLocation> {
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
pub(crate) fn read_gain_at(data: &[u8], loc: &GainLocation) -> u8 {
    let idx = loc.byte_offset;
    if idx >= data.len() {
        return 0;
    }

    if loc.bit_offset == 0 {
        data[idx]
    } else if idx + 1 < data.len() {
        let shift = loc.bit_offset;
        let high = data[idx] << shift;
        let low = data[idx + 1] >> (8 - shift);
        high | low
    } else {
        data[idx] << loc.bit_offset
    }
}

/// Write 8-bit value at bit-unaligned position
pub(crate) fn write_gain_at(data: &mut [u8], loc: &GainLocation, value: u8) {
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
pub(crate) fn skip_id3v2(data: &[u8]) -> usize {
    if data.len() < 10 || &data[0..3] != b"ID3" {
        return 0;
    }

    let size = ((data[6] as usize & 0x7F) << 21)
        | ((data[7] as usize & 0x7F) << 14)
        | ((data[8] as usize & 0x7F) << 7)
        | (data[9] as usize & 0x7F);

    10 + size
}

/// Read u32 little-endian from slice
pub(crate) fn read_u32_le(data: &[u8]) -> u32 {
    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
}

/// Find the end of audio data (before trailing tags)
pub(crate) fn find_audio_end(data: &[u8]) -> usize {
    let mut audio_end = data.len();

    // Check for ID3v1 tag at end (128 bytes, starts with "TAG")
    if audio_end >= 128 && &data[audio_end - 128..audio_end - 125] == b"TAG" {
        audio_end -= 128;
    }

    // Check for APE tag before ID3v1 (or at end if no ID3v1)
    if audio_end >= 32 && &data[audio_end - 32..audio_end - 24] == APE_PREAMBLE {
        let footer_start = audio_end - 32;
        let tag_size = read_u32_le(&data[footer_start + 12..]) as usize;
        let flags = read_u32_le(&data[footer_start + 20..]);
        let has_header = (flags & APE_FLAG_HEADER_PRESENT) != 0;
        let header_size = if has_header { 32 } else { 0 };

        if footer_start + 32 >= tag_size + header_size {
            audio_end = footer_start + 32 - tag_size - header_size;
        }
    }

    audio_end
}

/// Check if a frame contains a Xing or Info VBR header
pub(crate) fn is_xing_frame(data: &[u8], frame_offset: usize, header: &FrameHeader) -> bool {
    let side_info_len = match (header.version, header.channel_mode) {
        (MpegVersion::Mpeg1, ChannelMode::Mono) => 17,
        (MpegVersion::Mpeg1, _) => 32,
        (_, ChannelMode::Mono) => 9,
        (_, _) => 17,
    };

    let xing_offset = frame_offset + header.side_info_offset() + side_info_len;

    if xing_offset + 4 > data.len() {
        return false;
    }

    let marker = &data[xing_offset..xing_offset + 4];
    marker == b"Xing" || marker == b"Info"
}

/// Internal function to iterate over frames
/// Skips Xing/Info VBR header frames to match mp3gain behavior
pub(crate) fn iterate_frames<F>(data: &[u8], mut callback: F) -> Result<usize>
where
    F: FnMut(usize, &FrameHeader, &[GainLocation]),
{
    let audio_end = find_audio_end(data);
    let mut pos = skip_id3v2(data);
    let mut frame_count = 0;

    while pos + 4 <= audio_end {
        let header = match parse_header(&data[pos..]) {
            Some(h) => h,
            None => {
                pos += 1;
                continue;
            }
        };

        let next_pos = pos + header.frame_size;

        let valid_frame = if next_pos + 2 <= audio_end {
            data[next_pos] == 0xFF && (data[next_pos + 1] & 0xE0) == 0xE0
        } else {
            next_pos <= audio_end
        };

        if !valid_frame {
            pos += 1;
            continue;
        }

        if is_xing_frame(data, pos, &header) {
            pos = next_pos;
            continue;
        }

        let locations = calculate_gain_locations(pos, &header);
        callback(pos, &header, &locations);

        frame_count += 1;
        pos = next_pos;
    }

    Ok(frame_count)
}

/// Gain adjustment mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum GainMode {
    Saturating,
    Wrapping,
}

/// Apply the gain adjustment to a single gain location
pub(crate) fn adjust_gain_value(current: u8, steps: i32, mode: GainMode) -> u8 {
    match mode {
        GainMode::Saturating => {
            if steps > 0 {
                current.saturating_add(steps.min(255) as u8)
            } else {
                current.saturating_sub((-steps).min(255) as u8)
            }
        }
        GainMode::Wrapping => {
            let new_gain = (current as i32 + steps) % 256;
            ((new_gain + 256) % 256) as u8
        }
    }
}

/// Internal function to apply gain to all frames in data
pub(crate) fn apply_gain_to_data(data: &mut [u8], gain_steps: i32, mode: GainMode) -> usize {
    let audio_end = find_audio_end(data);
    let mut pos = skip_id3v2(data);
    let mut modified_frames = 0;

    while pos + 4 <= audio_end {
        let header = match parse_header(&data[pos..]) {
            Some(h) => h,
            None => {
                pos += 1;
                continue;
            }
        };

        let next_pos = pos + header.frame_size;

        let valid_frame = if next_pos + 2 <= audio_end {
            data[next_pos] == 0xFF && (data[next_pos + 1] & 0xE0) == 0xE0
        } else {
            next_pos <= audio_end
        };

        if !valid_frame {
            pos += 1;
            continue;
        }

        if is_xing_frame(data, pos, &header) {
            pos = next_pos;
            continue;
        }

        let locations = calculate_gain_locations(pos, &header);

        for loc in &locations {
            let current_gain = read_gain_at(data, loc);
            let new_gain = adjust_gain_value(current_gain, gain_steps, mode);
            write_gain_at(data, loc, new_gain);
        }

        modified_frames += 1;
        pos = next_pos;
    }

    modified_frames
}

/// Internal function to apply gain to a specific channel in data
pub(crate) fn apply_gain_to_channel_data(
    data: &mut [u8],
    channel_index: usize,
    gain_steps: i32,
) -> usize {
    let audio_end = find_audio_end(data);
    let mut pos = skip_id3v2(data);
    let mut modified_frames = 0;

    while pos + 4 <= audio_end {
        let header = match parse_header(&data[pos..]) {
            Some(h) => h,
            None => {
                pos += 1;
                continue;
            }
        };

        let next_pos = pos + header.frame_size;

        let valid_frame = if next_pos + 2 <= audio_end {
            data[next_pos] == 0xFF && (data[next_pos + 1] & 0xE0) == 0xE0
        } else {
            next_pos <= audio_end
        };

        if !valid_frame {
            pos += 1;
            continue;
        }

        if is_xing_frame(data, pos, &header) {
            pos = next_pos;
            continue;
        }

        let locations = calculate_gain_locations(pos, &header);
        let num_channels = header.channel_mode.channel_count();
        let num_granules = header.granule_count();

        for gr in 0..num_granules {
            let loc_index = gr * num_channels + channel_index;
            if loc_index < locations.len() {
                let loc = &locations[loc_index];
                let current_gain = read_gain_at(data, loc);
                let new_gain = adjust_gain_value(current_gain, gain_steps, GainMode::Saturating);
                write_gain_at(data, loc, new_gain);
            }
        }

        modified_frames += 1;
        pos = next_pos;
    }

    modified_frames
}

/// Scan gain range (min/max global_gain) across all frames in file data.
pub(crate) fn scan_gain_range(data: &[u8]) -> Result<(u8, u8)> {
    let mut min_gain = 255u8;
    let mut max_gain = 0u8;

    let frame_count = iterate_frames(data, |_pos, _header, locations| {
        for loc in locations {
            let gain = read_gain_at(data, loc);
            min_gain = min_gain.min(gain);
            max_gain = max_gain.max(gain);
        }
    })?;

    if frame_count == 0 {
        return Err(Error::NoMp3Frames);
    }

    Ok((min_gain, max_gain))
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_is_xing_frame() {
        let mut data = vec![0u8; 100];
        data[0] = 0xFF;
        data[1] = 0xFB;
        data[2] = 0x90;
        data[3] = 0x00;

        data[36] = b'X';
        data[37] = b'i';
        data[38] = b'n';
        data[39] = b'g';

        let header = parse_header(&data).unwrap();
        assert!(is_xing_frame(&data, 0, &header));

        data[36] = b'I';
        data[37] = b'n';
        data[38] = b'f';
        data[39] = b'o';
        assert!(is_xing_frame(&data, 0, &header));

        data[36] = 0x00;
        data[37] = 0x00;
        data[38] = 0x00;
        data[39] = 0x00;
        assert!(!is_xing_frame(&data, 0, &header));
    }
}
