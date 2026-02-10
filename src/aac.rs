//! AAC bitstream parser for locating `global_gain` fields in M4A/MP4 files.
//!
//! This module implements a read-only parser that navigates the MP4 container
//! structure and parses AAC raw_data_blocks to find the byte offset and bit
//! offset of every `global_gain` field. It does **not** modify any data.
//!
//! The parser supports AAC-LC single channel elements (SCE), channel pair
//! elements (CPE), and LFE elements. Unsupported element types (CCE, PCE)
//! cause the individual sample to be skipped with a warning count increment.

use anyhow::{Context, Result};
use std::path::Path;

use crate::aac_codebooks;
use crate::mp4meta;

// =============================================================================
// Public types
// =============================================================================

/// Location of an AAC global_gain field within the MP4 file
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AacGainLocation {
    pub sample_index: u32,
    pub file_offset: u64,
    pub sample_byte_offset: u32,
    pub bit_offset: u8,
    pub channel: u8,
    pub original_gain: u8,
}

/// Result of AAC gain analysis
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AacAnalysis {
    pub gain_locations: Vec<AacGainLocation>,
    pub sample_count: u32,
    pub channel_count: u8,
    pub min_gain: u8,
    pub max_gain: u8,
    pub sample_rate: u32,
    pub parse_warnings: u32,
}

// =============================================================================
// Constants
// =============================================================================

const ID_SCE: u32 = 0; // Single Channel Element
const ID_CPE: u32 = 1; // Channel Pair Element
const ID_LFE: u32 = 3; // LFE Channel Element
const ID_DSE: u32 = 4; // Data Stream Element
const ID_FIL: u32 = 6; // Fill Element
const ID_END: u32 = 7; // End

const ZERO_HCB: u8 = 0;
const NOISE_HCB: u8 = 13;
const INTENSITY_HCB2: u8 = 14;
const INTENSITY_HCB: u8 = 15;
const ESC_HCB: u8 = 11;

#[allow(dead_code)]
const ONLY_LONG_SEQUENCE: u8 = 0;
const EIGHT_SHORT_SEQUENCE: u8 = 2;

const MAX_SFBS: usize = 64;
const MAX_WINDOWS: usize = 8;

// MP4 box types used by this module
const STSC: u32 = u32::from_be_bytes(*b"stsc");
const STSZ: u32 = u32::from_be_bytes(*b"stsz");
const ESDS: u32 = u32::from_be_bytes(*b"esds");

// =============================================================================
// BitReader
// =============================================================================

struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8, // 0-7, bits consumed in current byte (0 = no bits consumed yet)
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    /// Current position as (byte_offset, bit_offset)
    fn position(&self) -> (usize, u8) {
        (self.byte_pos, self.bit_pos)
    }

    fn bits_remaining(&self) -> usize {
        self.data.len().saturating_sub(self.byte_pos) * 8 - self.bit_pos as usize
    }

    /// Read 1-25 bits, MSB first
    fn read_bits(&mut self, n: u8) -> Result<u32> {
        let mut val = 0u32;
        for _ in 0..n {
            if self.byte_pos >= self.data.len() {
                anyhow::bail!("unexpected end of bitstream");
            }
            val = (val << 1) | ((self.data[self.byte_pos] >> (7 - self.bit_pos)) & 1) as u32;
            self.bit_pos += 1;
            if self.bit_pos == 8 {
                self.bit_pos = 0;
                self.byte_pos += 1;
            }
        }
        Ok(val)
    }

    fn read_bit(&mut self) -> Result<bool> {
        Ok(self.read_bits(1)? != 0)
    }

    fn skip_bits(&mut self, n: usize) -> Result<()> {
        let total_bits = self.byte_pos * 8 + self.bit_pos as usize + n;
        self.byte_pos = total_bits / 8;
        self.bit_pos = (total_bits % 8) as u8;
        if self.byte_pos > self.data.len() || (self.byte_pos == self.data.len() && self.bit_pos > 0)
        {
            anyhow::bail!("unexpected end of bitstream");
        }
        Ok(())
    }

    fn byte_align(&mut self) {
        if self.bit_pos > 0 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
    }
}

// =============================================================================
// Huffman decoder
// =============================================================================

/// Decode one Huffman symbol from the bitstream.
/// Returns the symbol index.
fn decode_huffman(reader: &mut BitReader, lens: &[u8], codes: &[u32]) -> Result<usize> {
    let mut code: u32 = 0;
    let mut bits_read: u8 = 0;
    let max_len = *lens.iter().max().unwrap_or(&0);

    for _ in 0..max_len {
        code = (code << 1) | reader.read_bits(1)?;
        bits_read += 1;

        for (i, (&len, &cw)) in lens.iter().zip(codes.iter()).enumerate() {
            if len == bits_read && cw == code {
                return Ok(i);
            }
        }
    }

    anyhow::bail!("invalid Huffman code");
}

// =============================================================================
// MP4 sample table parser
// =============================================================================

struct SampleEntry {
    file_offset: u64,
    size: u32,
}

/// Read a u32 big-endian from data at offset
fn read_u32_be(data: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ])
}

fn read_u64_be(data: &[u8], offset: usize) -> u64 {
    u64::from_be_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
        data[offset + 4],
        data[offset + 5],
        data[offset + 6],
        data[offset + 7],
    ])
}

/// Build sample table: for each AAC sample, compute (file_offset, size)
fn build_sample_table(data: &[u8]) -> Result<(Vec<SampleEntry>, usize, usize)> {
    // Navigate moov -> trak -> mdia -> minf -> stbl
    let (moov_pos, moov_header) =
        mp4meta::find_box(data, mp4meta::MOOV).ok_or_else(|| anyhow::anyhow!("no moov box"))?;
    let moov_start = moov_pos + moov_header.header_size as usize;
    let moov_size = moov_header.content_size() as usize;

    // Find first audio track (look for mp4a in stsd)
    let (stbl_start, stbl_size, stsd_pos) = find_audio_stbl(data, moov_start, moov_size)?;

    // Parse STSZ
    let (stsz_pos, stsz_header) = mp4meta::find_box_in_container(data, stbl_start, stbl_size, STSZ)
        .ok_or_else(|| anyhow::anyhow!("no stsz box"))?;
    let stsz_content = stsz_pos + stsz_header.header_size as usize;
    let _version = read_u32_be(data, stsz_content);
    let default_size = read_u32_be(data, stsz_content + 4);
    let sample_count = read_u32_be(data, stsz_content + 8) as usize;

    let mut sample_sizes = Vec::with_capacity(sample_count);
    if default_size != 0 {
        sample_sizes.resize(sample_count, default_size);
    } else {
        let sizes_start = stsz_content + 12;
        for i in 0..sample_count {
            sample_sizes.push(read_u32_be(data, sizes_start + i * 4));
        }
    }

    // Parse STSC (sample-to-chunk)
    let (stsc_pos, stsc_header) = mp4meta::find_box_in_container(data, stbl_start, stbl_size, STSC)
        .ok_or_else(|| anyhow::anyhow!("no stsc box"))?;
    let stsc_content = stsc_pos + stsc_header.header_size as usize;
    let stsc_count = read_u32_be(data, stsc_content + 4) as usize;
    let stsc_entries_start = stsc_content + 8;

    struct StscEntry {
        first_chunk: u32,
        samples_per_chunk: u32,
    }
    let mut stsc_entries = Vec::with_capacity(stsc_count);
    for i in 0..stsc_count {
        let off = stsc_entries_start + i * 12;
        stsc_entries.push(StscEntry {
            first_chunk: read_u32_be(data, off),
            samples_per_chunk: read_u32_be(data, off + 4),
        });
    }

    // Parse STCO or CO64
    let chunk_offsets = parse_chunk_offsets(data, stbl_start, stbl_size)?;

    // Build sample table: map each sample to its file offset
    let mut entries = Vec::with_capacity(sample_count);
    let mut sample_idx = 0usize;

    for (chunk_idx, &chunk_offset) in chunk_offsets.iter().enumerate() {
        let chunk_num = chunk_idx + 1; // 1-based

        // Find how many samples in this chunk
        let samples_in_chunk = {
            let mut spc = stsc_entries[0].samples_per_chunk;
            for entry in &stsc_entries {
                if entry.first_chunk as usize <= chunk_num {
                    spc = entry.samples_per_chunk;
                } else {
                    break;
                }
            }
            spc as usize
        };

        let mut offset_in_chunk = 0u64;
        for _ in 0..samples_in_chunk {
            if sample_idx >= sample_count {
                break;
            }
            entries.push(SampleEntry {
                file_offset: chunk_offset + offset_in_chunk,
                size: sample_sizes[sample_idx],
            });
            offset_in_chunk += sample_sizes[sample_idx] as u64;
            sample_idx += 1;
        }
    }

    Ok((entries, stsd_pos, stbl_start))
}

fn find_audio_stbl(
    data: &[u8],
    moov_start: usize,
    moov_size: usize,
) -> Result<(usize, usize, usize)> {
    // Search through trak boxes for one with mp4a in stsd
    let mut search_pos = moov_start;
    let moov_end = moov_start + moov_size;

    while search_pos < moov_end {
        let (trak_pos, trak_header) = match mp4meta::find_box_in_container(
            data,
            search_pos,
            moov_end - search_pos,
            mp4meta::TRAK,
        ) {
            Some(x) => x,
            None => break,
        };
        let trak_start = trak_pos + trak_header.header_size as usize;
        let trak_size = trak_header.content_size() as usize;

        // Navigate: trak -> mdia -> minf -> stbl -> stsd
        if let Some((mdia_pos, mdia_h)) =
            mp4meta::find_box_in_container(data, trak_start, trak_size, mp4meta::MDIA)
        {
            let mdia_start = mdia_pos + mdia_h.header_size as usize;
            let mdia_size = mdia_h.content_size() as usize;

            if let Some((minf_pos, minf_h)) =
                mp4meta::find_box_in_container(data, mdia_start, mdia_size, mp4meta::MINF)
            {
                let minf_start = minf_pos + minf_h.header_size as usize;
                let minf_size = minf_h.content_size() as usize;

                if let Some((stbl_pos, stbl_h)) =
                    mp4meta::find_box_in_container(data, minf_start, minf_size, mp4meta::STBL)
                {
                    let stbl_start = stbl_pos + stbl_h.header_size as usize;
                    let stbl_size = stbl_h.content_size() as usize;

                    if let Some((stsd_pos, stsd_h)) =
                        mp4meta::find_box_in_container(data, stbl_start, stbl_size, mp4meta::STSD)
                    {
                        // Check if this is mp4a
                        let entries_start = stsd_pos + stsd_h.header_size as usize + 8;
                        if entries_start + 8 <= data.len() {
                            let entry_type = read_u32_be(data, entries_start + 4);
                            if entry_type == mp4meta::MP4A {
                                return Ok((stbl_start, stbl_size, stsd_pos));
                            }
                        }
                    }
                }
            }
        }

        search_pos = trak_pos + trak_header.size as usize;
    }

    anyhow::bail!("no AAC audio track found");
}

fn parse_chunk_offsets(data: &[u8], stbl_start: usize, stbl_size: usize) -> Result<Vec<u64>> {
    // Try STCO first, then CO64
    if let Some((stco_pos, stco_h)) =
        mp4meta::find_box_in_container(data, stbl_start, stbl_size, mp4meta::STCO)
    {
        let content = stco_pos + stco_h.header_size as usize;
        let count = read_u32_be(data, content + 4) as usize;
        let mut offsets = Vec::with_capacity(count);
        for i in 0..count {
            offsets.push(read_u32_be(data, content + 8 + i * 4) as u64);
        }
        return Ok(offsets);
    }

    if let Some((co64_pos, co64_h)) =
        mp4meta::find_box_in_container(data, stbl_start, stbl_size, mp4meta::CO64)
    {
        let content = co64_pos + co64_h.header_size as usize;
        let count = read_u32_be(data, content + 4) as usize;
        let mut offsets = Vec::with_capacity(count);
        for i in 0..count {
            offsets.push(read_u64_be(data, content + 8 + i * 8));
        }
        return Ok(offsets);
    }

    anyhow::bail!("no stco or co64 box found");
}

// =============================================================================
// AudioSpecificConfig parser
// =============================================================================

/// Sample rate index table (ISO 14496-3)
const SAMPLE_RATE_TABLE: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

fn parse_audio_config(data: &[u8], stsd_pos: usize) -> Result<u32> {
    // stsd: header(8) + version/flags(4) + entry_count(4) + entries
    // mp4a entry: size(4) + 'mp4a'(4) + reserved(6) + data_ref_index(2)
    //   + version(2) + revision(2) + vendor(4) + channel_count(2)
    //   + sample_size(2) + compression_id(2) + packet_size(2) + sample_rate(4, 16.16 fixed)
    let stsd_header_end = stsd_pos + 8; // after box header
    let entries_start = stsd_header_end + 8; // after version/flags + entry_count

    if entries_start + 4 > data.len() {
        anyhow::bail!("stsd too short");
    }
    let mp4a_size = read_u32_be(data, entries_start) as usize;
    let mp4a_start = entries_start;
    let mp4a_end = mp4a_start + mp4a_size;

    // sample_rate at fixed offset within mp4a box
    let sr_offset = mp4a_start + 8 + 6 + 2 + 2 + 2 + 4 + 2 + 2 + 2 + 2;
    if sr_offset + 4 > data.len() {
        anyhow::bail!("mp4a too short for sample rate");
    }
    let sr_fixed = read_u32_be(data, sr_offset);
    let sample_rate = sr_fixed >> 16; // 16.16 fixed point -> integer part

    // Try to get more precise sample rate from esds AudioSpecificConfig
    let esds_search_start = mp4a_start + 8 + 28;
    if esds_search_start < mp4a_end {
        if let Some(asc_sr) = parse_esds_sample_rate(data, esds_search_start, mp4a_end) {
            return Ok(asc_sr);
        }
    }

    Ok(sample_rate)
}

fn parse_esds_sample_rate(data: &[u8], start: usize, end: usize) -> Option<u32> {
    // Find esds box
    let (esds_pos, esds_h) = mp4meta::find_box_in_container(data, start, end - start, ESDS)?;
    let esds_content = esds_pos + esds_h.header_size as usize + 4; // skip version/flags
    let esds_end = esds_pos + esds_h.size as usize;

    let asc_data = find_audio_specific_config(data, esds_content, esds_end)?;

    if asc_data.len() < 2 {
        return None;
    }

    // AudioSpecificConfig: audioObjectType(5 bits) + samplingFrequencyIndex(4 bits)
    let _aot = (asc_data[0] >> 3) & 0x1F;
    let sr_idx = ((asc_data[0] & 0x07) << 1) | (asc_data[1] >> 7);

    if sr_idx == 0x0F && asc_data.len() >= 5 {
        // 24-bit explicit frequency
        let freq = ((asc_data[1] as u32 & 0x7F) << 17)
            | ((asc_data[2] as u32) << 9)
            | ((asc_data[3] as u32) << 1)
            | ((asc_data[4] as u32) >> 7);
        return Some(freq);
    }

    if (sr_idx as usize) < SAMPLE_RATE_TABLE.len() {
        Some(SAMPLE_RATE_TABLE[sr_idx as usize])
    } else {
        None
    }
}

fn find_audio_specific_config(data: &[u8], start: usize, end: usize) -> Option<&[u8]> {
    // ESDS has nested descriptors with tag-length encoding
    // ES_Descriptor tag=3, DecoderConfigDescriptor tag=4, DecoderSpecificInfo tag=5
    let mut pos = start;

    // Skip ES_Descriptor header (tag=3)
    if pos >= end || data[pos] != 3 {
        return None;
    }
    pos += 1;
    let (_len, consumed) = read_desc_length(data, pos, end)?;
    pos += consumed;
    pos += 3; // ES_ID(2) + stream_priority(1)

    // Look for DecoderConfigDescriptor (tag=4)
    if pos >= end || data[pos] != 4 {
        return None;
    }
    pos += 1;
    let (_len, consumed) = read_desc_length(data, pos, end)?;
    pos += consumed;
    pos += 13; // objectTypeIndication(1) + streamType(1) + bufferSizeDB(3) + maxBitrate(4) + avgBitrate(4)

    // Look for DecoderSpecificInfo (tag=5)
    if pos >= end || data[pos] != 5 {
        return None;
    }
    pos += 1;
    let (len, consumed) = read_desc_length(data, pos, end)?;
    pos += consumed;

    let asc_end = (pos + len).min(end);
    Some(&data[pos..asc_end])
}

fn read_desc_length(data: &[u8], start: usize, end: usize) -> Option<(usize, usize)> {
    let mut len = 0usize;
    let mut consumed = 0usize;
    let mut pos = start;

    loop {
        if pos >= end {
            return None;
        }
        let b = data[pos];
        pos += 1;
        consumed += 1;
        len = (len << 7) | (b & 0x7F) as usize;
        if b & 0x80 == 0 {
            break;
        }
        if consumed >= 4 {
            break;
        }
    }

    Some((len, consumed))
}

// =============================================================================
// AAC bitstream parsers
// =============================================================================

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct IcsInfo {
    window_sequence: u8,
    max_sfb: usize,
    long_win: bool,
    window_groups: usize,
    window_group_len: [usize; MAX_WINDOWS],
}

fn parse_ics_info(reader: &mut BitReader) -> Result<IcsInfo> {
    let _reserved = reader.read_bits(1)?;
    let window_sequence = reader.read_bits(2)? as u8;
    let _window_shape = reader.read_bits(1)?;
    let long_win = window_sequence != EIGHT_SHORT_SEQUENCE;

    let (max_sfb, window_groups, window_group_len) = if !long_win {
        let max_sfb = reader.read_bits(4)? as usize;
        let scale_factor_grouping = reader.read_bits(7)? as u8;

        // Calculate window groups from grouping bits
        let mut groups = 1usize;
        let mut group_len = [0usize; MAX_WINDOWS];
        group_len[0] = 1;
        for i in 0..7 {
            if (scale_factor_grouping >> (6 - i)) & 1 == 0 {
                groups += 1;
                group_len[groups - 1] = 1;
            } else {
                group_len[groups - 1] += 1;
            }
        }
        (max_sfb, groups, group_len)
    } else {
        let max_sfb = reader.read_bits(6)? as usize;
        let predictor_data_present = reader.read_bit()?;
        if predictor_data_present {
            anyhow::bail!("predictor data not supported for AAC-LC");
        }
        let mut group_len = [0usize; MAX_WINDOWS];
        group_len[0] = 1;
        (max_sfb, 1, group_len)
    };

    Ok(IcsInfo {
        window_sequence,
        max_sfb,
        long_win,
        window_groups,
        window_group_len,
    })
}

// =============================================================================
// Section data
// =============================================================================

struct SectionData {
    sfb_cb: [[u8; MAX_SFBS]; MAX_WINDOWS],
}

fn parse_section_data(reader: &mut BitReader, info: &IcsInfo) -> Result<SectionData> {
    let sect_bits = if info.long_win { 5u8 } else { 3u8 };
    let sect_esc_val = (1u32 << sect_bits) - 1;
    let mut sfb_cb = [[0u8; MAX_SFBS]; MAX_WINDOWS];

    for group_cb in sfb_cb.iter_mut().take(info.window_groups) {
        let mut k = 0usize;
        while k < info.max_sfb {
            let cb = reader.read_bits(4)? as u8;
            if cb == 12 {
                anyhow::bail!("reserved codebook 12");
            }

            let mut sect_len = 0usize;
            loop {
                let incr = reader.read_bits(sect_bits)? as usize;
                sect_len += incr;
                if incr < sect_esc_val as usize {
                    break;
                }
            }

            for slot in group_cb.iter_mut().skip(k).take(sect_len) {
                *slot = cb;
            }
            k += sect_len;
        }
    }

    Ok(SectionData { sfb_cb })
}

// =============================================================================
// Scale factor data
// =============================================================================

fn parse_scale_factor_data(
    reader: &mut BitReader,
    info: &IcsInfo,
    section: &SectionData,
) -> Result<()> {
    let mut noise_pcm_flag = true;

    for g in 0..info.window_groups {
        for sfb in 0..info.max_sfb {
            let cb = section.sfb_cb[g][sfb];
            match cb {
                ZERO_HCB => {} // no bits
                INTENSITY_HCB | INTENSITY_HCB2 => {
                    decode_huffman(
                        reader,
                        &aac_codebooks::SCF_CB_LENS,
                        &aac_codebooks::SCF_CB_CODES,
                    )?;
                }
                NOISE_HCB => {
                    if noise_pcm_flag {
                        reader.read_bits(9)?; // noise PCM
                        noise_pcm_flag = false;
                    } else {
                        decode_huffman(
                            reader,
                            &aac_codebooks::SCF_CB_LENS,
                            &aac_codebooks::SCF_CB_CODES,
                        )?;
                    }
                }
                _ => {
                    decode_huffman(
                        reader,
                        &aac_codebooks::SCF_CB_LENS,
                        &aac_codebooks::SCF_CB_CODES,
                    )?;
                }
            }
        }
    }
    Ok(())
}

// =============================================================================
// Spectral data
// =============================================================================

fn parse_spectral_data(
    reader: &mut BitReader,
    info: &IcsInfo,
    section: &SectionData,
    bands: &[usize],
) -> Result<()> {
    for g in 0..info.window_groups {
        for _w in 0..info.window_group_len[g] {
            for sfb in 0..info.max_sfb {
                let cb_idx = section.sfb_cb[g][sfb];
                if cb_idx == ZERO_HCB
                    || cb_idx == NOISE_HCB
                    || cb_idx == INTENSITY_HCB
                    || cb_idx == INTENSITY_HCB2
                {
                    continue; // no spectral data
                }

                let start = bands[sfb];
                let end = bands[sfb + 1];
                let width = end - start;

                let cb_info = &aac_codebooks::SPECTRUM_CODEBOOKS[cb_idx as usize - 1];
                let dim = cb_info.dimension as usize;
                let num_codewords = width / dim;

                for _ in 0..num_codewords {
                    let symbol = decode_huffman(reader, cb_info.lens, cb_info.codes)?;

                    if cb_info.is_unsigned {
                        // For unsigned codebooks, read sign bits for nonzero values
                        if cb_info.dimension == 4 {
                            let (a, b, c, d) = aac_codebooks::AAC_QUADS[symbol];
                            if a != 0 {
                                reader.read_bits(1)?;
                            }
                            if b != 0 {
                                reader.read_bits(1)?;
                            }
                            if c != 0 {
                                reader.read_bits(1)?;
                            }
                            if d != 0 {
                                reader.read_bits(1)?;
                            }
                        } else {
                            // pairs
                            let mod_val = cb_info.mod_value as usize;
                            let x = symbol / mod_val;
                            let y = symbol % mod_val;
                            if x != 0 {
                                reader.read_bits(1)?;
                            }
                            if y != 0 {
                                reader.read_bits(1)?;
                            }

                            // Escape sequences for codebook 11
                            if cb_idx == ESC_HCB {
                                if x == 16 {
                                    read_escape(reader)?;
                                }
                                if y == 16 {
                                    read_escape(reader)?;
                                }
                            }
                        }
                    }
                    // For signed codebooks (1-2, 5-6), no extra bits needed
                }
            }
        }
    }
    Ok(())
}

fn read_escape(reader: &mut BitReader) -> Result<()> {
    // Count leading 1 bits
    let mut n = 0u8;
    while reader.read_bit()? {
        n += 1;
        if n >= 9 {
            anyhow::bail!("escape sequence too long");
        }
    }
    // Skip the N+4 data bits
    reader.skip_bits((n as usize) + 4)?;
    Ok(())
}

// =============================================================================
// Pulse data
// =============================================================================

fn parse_pulse_data(reader: &mut BitReader) -> Result<()> {
    let number_pulse = reader.read_bits(2)? as usize;
    let _pulse_start_sfb = reader.read_bits(6)?;
    for _ in 0..number_pulse + 1 {
        let _pulse_offset = reader.read_bits(5)?;
        let _pulse_amp = reader.read_bits(4)?;
    }
    Ok(())
}

// =============================================================================
// TNS data
// =============================================================================

fn parse_tns_data(reader: &mut BitReader, info: &IcsInfo) -> Result<()> {
    let n_filt_bits = if info.long_win { 2u8 } else { 1u8 };
    let length_bits = if info.long_win { 6u8 } else { 4u8 };
    let order_bits = if info.long_win { 5u8 } else { 3u8 };

    let num_windows = if info.long_win { 1 } else { 8 };

    for _ in 0..num_windows {
        let n_filt = reader.read_bits(n_filt_bits)? as usize;
        if n_filt > 0 {
            let coef_res = reader.read_bits(1)?; // coef_res flag
            for _ in 0..n_filt {
                let _length = reader.read_bits(length_bits)?;
                let order = reader.read_bits(order_bits)? as usize;
                if order > 0 {
                    let _direction = reader.read_bits(1)?;
                    let coef_compress = reader.read_bits(1)?;
                    let coef_bits = (coef_res + 3 - coef_compress) as u8;
                    for _ in 0..order {
                        reader.read_bits(coef_bits)?;
                    }
                }
            }
        }
    }
    Ok(())
}

// =============================================================================
// ICS parser (records global_gain location)
// =============================================================================

fn parse_ics(
    reader: &mut BitReader,
    channel: u8,
    common_window: bool,
    shared_info: Option<&IcsInfo>,
    sample_rate: u32,
) -> Result<(AacGainLocation, IcsInfo)> {
    // Record position BEFORE reading global_gain
    let (byte_off, bit_off) = reader.position();
    let global_gain = reader.read_bits(8)? as u8;

    let gain_loc = AacGainLocation {
        sample_index: 0, // filled in by caller
        file_offset: 0,  // filled in by caller
        sample_byte_offset: byte_off as u32,
        bit_offset: bit_off,
        channel,
        original_gain: global_gain,
    };

    let info = if common_window {
        shared_info.unwrap().clone()
    } else {
        parse_ics_info(reader)?
    };

    // Get SWB band offsets for this window type
    let (long_bands, short_bands) = aac_codebooks::swb_offsets(sample_rate);
    let bands = if info.long_win {
        long_bands
    } else {
        short_bands
    };

    // Validate max_sfb against available bands
    if info.max_sfb >= bands.len() {
        anyhow::bail!(
            "max_sfb {} exceeds available bands {}",
            info.max_sfb,
            bands.len() - 1
        );
    }

    let section = parse_section_data(reader, &info)?;
    parse_scale_factor_data(reader, &info, &section)?;

    // pulse_data
    if reader.read_bit()? {
        if !info.long_win {
            anyhow::bail!("pulse data in short window");
        }
        parse_pulse_data(reader)?;
    }

    // tns_data
    if reader.read_bit()? {
        parse_tns_data(reader, &info)?;
    }

    // gain_control_data (SSR only, should be 0 for AAC-LC)
    if reader.read_bit()? {
        anyhow::bail!("gain control data not supported for AAC-LC");
    }

    parse_spectral_data(reader, &info, &section, bands)?;

    Ok((gain_loc, info))
}

// =============================================================================
// Element parsers
// =============================================================================

fn parse_sce(reader: &mut BitReader, sample_rate: u32) -> Result<Vec<AacGainLocation>> {
    let _tag = reader.read_bits(4)?;
    let (loc, _) = parse_ics(reader, 0, false, None, sample_rate)?;
    Ok(vec![loc])
}

fn parse_cpe(reader: &mut BitReader, sample_rate: u32) -> Result<Vec<AacGainLocation>> {
    let _tag = reader.read_bits(4)?;
    let common_window = reader.read_bit()?;

    let shared_info = if common_window {
        let info = parse_ics_info(reader)?;
        let ms_mask_present = reader.read_bits(2)?;
        if ms_mask_present == 1 {
            // Read ms_used bits
            for _g in 0..info.window_groups {
                for _sfb in 0..info.max_sfb {
                    reader.read_bits(1)?;
                }
            }
        }
        Some(info)
    } else {
        None
    };

    let (loc1, _) = parse_ics(reader, 0, common_window, shared_info.as_ref(), sample_rate)?;
    let (loc2, _) = parse_ics(reader, 1, common_window, shared_info.as_ref(), sample_rate)?;

    Ok(vec![loc1, loc2])
}

fn skip_dse(reader: &mut BitReader) -> Result<()> {
    let _tag = reader.read_bits(4)?;
    let align = reader.read_bit()?;
    let mut count = reader.read_bits(8)? as usize;
    if count == 255 {
        count += reader.read_bits(8)? as usize;
    }
    if align {
        reader.byte_align();
    }
    reader.skip_bits(count * 8)?;
    Ok(())
}

fn skip_fil(reader: &mut BitReader) -> Result<()> {
    let mut count = reader.read_bits(4)? as usize;
    if count == 15 {
        count += reader.read_bits(8)? as usize - 1;
    }
    reader.skip_bits(count * 8)?;
    Ok(())
}

fn parse_raw_data_block(reader: &mut BitReader, sample_rate: u32) -> Result<Vec<AacGainLocation>> {
    let mut locations = Vec::new();

    loop {
        if reader.bits_remaining() < 3 {
            break;
        }
        let id = reader.read_bits(3)?;

        match id {
            ID_SCE | ID_LFE => {
                let locs = parse_sce(reader, sample_rate)?;
                locations.extend(locs);
            }
            ID_CPE => {
                let locs = parse_cpe(reader, sample_rate)?;
                locations.extend(locs);
            }
            ID_DSE => skip_dse(reader)?,
            ID_FIL => skip_fil(reader)?,
            ID_END => break,
            _ => {
                anyhow::bail!("unsupported element type {}", id);
            }
        }
    }

    Ok(locations)
}

// =============================================================================
// Public API
// =============================================================================

/// Analyze AAC/M4A file and locate all global_gain fields (read-only)
pub fn analyze_aac_gains(file_path: &Path) -> Result<AacAnalysis> {
    let data = std::fs::read(file_path)
        .with_context(|| format!("Failed to read: {}", file_path.display()))?;

    if !mp4meta::is_mp4_file(file_path) {
        anyhow::bail!("not an MP4 file: {}", file_path.display());
    }

    let (sample_table, stsd_pos, _stbl_start) = build_sample_table(&data)?;
    let sample_rate = parse_audio_config(&data, stsd_pos)?;

    let sample_count = sample_table.len() as u32;
    let mut all_locations = Vec::new();
    let mut parse_warnings = 0u32;
    let mut min_gain = 255u8;
    let mut max_gain = 0u8;
    let mut max_channel = 0u8;

    for (idx, entry) in sample_table.iter().enumerate() {
        let sample_start = entry.file_offset as usize;
        let sample_end = sample_start + entry.size as usize;

        if sample_end > data.len() {
            parse_warnings += 1;
            continue;
        }

        let sample_data = &data[sample_start..sample_end];
        let mut reader = BitReader::new(sample_data);

        match parse_raw_data_block(&mut reader, sample_rate) {
            Ok(locations) => {
                for mut loc in locations {
                    loc.sample_index = idx as u32;
                    loc.file_offset = entry.file_offset + loc.sample_byte_offset as u64;
                    min_gain = min_gain.min(loc.original_gain);
                    max_gain = max_gain.max(loc.original_gain);
                    max_channel = max_channel.max(loc.channel);
                    all_locations.push(loc);
                }
            }
            Err(_) => {
                parse_warnings += 1;
            }
        }
    }

    if all_locations.is_empty() && parse_warnings > 0 {
        anyhow::bail!(
            "failed to parse any AAC samples ({} errors)",
            parse_warnings
        );
    }

    Ok(AacAnalysis {
        gain_locations: all_locations,
        sample_count,
        channel_count: max_channel + 1,
        min_gain,
        max_gain,
        sample_rate,
        parse_warnings,
    })
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bit_reader_basic() {
        let data = [0b10110011, 0b01010101];
        let mut reader = BitReader::new(&data);

        assert_eq!(reader.read_bits(1).unwrap(), 1);
        assert_eq!(reader.read_bits(3).unwrap(), 0b011);
        assert_eq!(reader.read_bits(4).unwrap(), 0b0011);
        assert_eq!(reader.position(), (1, 0));

        assert_eq!(reader.read_bits(8).unwrap(), 0b01010101);
        assert_eq!(reader.bits_remaining(), 0);
    }

    #[test]
    fn test_bit_reader_cross_byte() {
        let data = [0xFF, 0x00, 0xAA];
        let mut reader = BitReader::new(&data);

        reader.skip_bits(4).unwrap();
        assert_eq!(reader.read_bits(8).unwrap(), 0xF0); // last 4 bits of 0xFF + first 4 bits of 0x00
    }

    #[test]
    fn test_bit_reader_align() {
        let data = [0xFF, 0xAA];
        let mut reader = BitReader::new(&data);

        reader.read_bits(3).unwrap();
        reader.byte_align();
        assert_eq!(reader.position(), (1, 0));
        assert_eq!(reader.read_bits(8).unwrap(), 0xAA);
    }

    #[test]
    fn test_huffman_decode_cb1() {
        // CB1 index 40 has code 0x000 with length 1 (the shortest code)
        let data = [0b00000000]; // starts with 0
        let mut reader = BitReader::new(&data);
        let symbol = decode_huffman(
            &mut reader,
            &aac_codebooks::SPECTRUM_CB1_LENS,
            &aac_codebooks::SPECTRUM_CB1_CODES,
        )
        .unwrap();
        assert_eq!(symbol, 40);
    }

    #[test]
    fn test_ics_info_long_window() {
        // reserved(0) + window_sequence(00=long) + window_shape(0) + max_sfb(001010=10) + predictor(0)
        // = 0_00_0_001010_0 = 0b0000_0010_100 padded
        let data = [0b0000_0010, 0b1000_0000];
        let mut reader = BitReader::new(&data);
        let info = parse_ics_info(&mut reader).unwrap();
        assert!(info.long_win);
        assert_eq!(info.max_sfb, 10);
        assert_eq!(info.window_groups, 1);
    }
}
