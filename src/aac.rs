//! AAC bitstream parser for locating `global_gain` fields in M4A/MP4 files.
//!
//! This module implements a read-only parser that navigates the MP4 container
//! structure and parses AAC raw_data_blocks to find the byte offset and bit
//! offset of every `global_gain` field. It does **not** modify any data.
//!
//! The parser supports AAC-LC single channel elements (SCE), channel pair
//! elements (CPE), and LFE elements. Unsupported element types (CCE, PCE)
//! cause the individual sample to be skipped with a warning count increment.
//!
//! Initial AAC support was driven by the investigation in #118 (slow path
//! and ffmpeg-decode errors on rewritten m4a), which seeded #120 / #121
//! before being superseded by the present parser implementation.

use crate::error::{Error, Result};
use std::path::Path;
use std::sync::OnceLock;

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
    sample_index: u32,
    file_offset: u64,
    sample_byte_offset: u32,
    bit_offset: u8,
    channel: u8,
    original_gain: u8,
}

impl AacGainLocation {
    pub(crate) fn new(
        sample_index: u32,
        file_offset: u64,
        sample_byte_offset: u32,
        bit_offset: u8,
        channel: u8,
        original_gain: u8,
    ) -> Self {
        Self {
            sample_index,
            file_offset,
            sample_byte_offset,
            bit_offset,
            channel,
            original_gain,
        }
    }

    pub fn sample_index(&self) -> u32 {
        self.sample_index
    }
    pub fn file_offset(&self) -> u64 {
        self.file_offset
    }
    pub fn sample_byte_offset(&self) -> u32 {
        self.sample_byte_offset
    }
    pub fn bit_offset(&self) -> u8 {
        self.bit_offset
    }
    pub fn channel(&self) -> u8 {
        self.channel
    }
    pub fn original_gain(&self) -> u8 {
        self.original_gain
    }
}

/// Result of AAC gain analysis
#[derive(Debug, Clone)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AacAnalysis {
    gain_locations: Vec<AacGainLocation>,
    sample_count: u32,
    channel_count: u8,
    min_gain: u8,
    max_gain: u8,
    sample_rate: u32,
    parse_warnings: u32,
}

impl AacAnalysis {
    pub(crate) fn new(
        gain_locations: Vec<AacGainLocation>,
        sample_count: u32,
        channel_count: u8,
        min_gain: u8,
        max_gain: u8,
        sample_rate: u32,
        parse_warnings: u32,
    ) -> Self {
        Self {
            gain_locations,
            sample_count,
            channel_count,
            min_gain,
            max_gain,
            sample_rate,
            parse_warnings,
        }
    }

    pub fn gain_locations(&self) -> &[AacGainLocation] {
        &self.gain_locations
    }
    pub fn sample_count(&self) -> u32 {
        self.sample_count
    }
    pub fn channel_count(&self) -> u8 {
        self.channel_count
    }
    pub fn min_gain(&self) -> u8 {
        self.min_gain
    }
    pub fn max_gain(&self) -> u8 {
        self.max_gain
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn parse_warnings(&self) -> u32 {
        self.parse_warnings
    }
}

// =============================================================================
// Constants
// =============================================================================

const ID_SCE: u32 = 0; // Single Channel Element
const ID_CPE: u32 = 1; // Channel Pair Element
const ID_CCE: u32 = 2; // Coupling Channel Element
const ID_LFE: u32 = 3; // LFE Channel Element
const ID_DSE: u32 = 4; // Data Stream Element
const ID_PCE: u32 = 5; // Program Config Element
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

    /// Read 1-32 bits, MSB first
    fn read_bits(&mut self, n: u8) -> Result<u32> {
        if n == 0 {
            return Ok(0);
        }
        let value = self.peek_bits(n)?;
        self.advance_bits(n as usize);
        Ok(value)
    }

    fn peek_bits(&self, n: u8) -> Result<u32> {
        let bits_to_read = n as usize;
        if bits_to_read == 0 {
            return Ok(0);
        }
        if self.bits_remaining() < bits_to_read {
            return Err(Error::AacParse {
                message: "unexpected end of bitstream".into(),
            });
        }

        let bytes_needed = (self.bit_pos as usize + bits_to_read).div_ceil(8);
        let mut window = 0u64;
        for byte in &self.data[self.byte_pos..self.byte_pos + bytes_needed] {
            window = (window << 8) | u64::from(*byte);
        }

        let window_bits = bytes_needed * 8;
        let shift = window_bits - self.bit_pos as usize - bits_to_read;
        let mask = if n == 32 {
            u64::from(u32::MAX)
        } else {
            (1u64 << bits_to_read) - 1
        };
        Ok(((window >> shift) & mask) as u32)
    }

    fn advance_bits(&mut self, bits_to_advance: usize) {
        let next_bit = self.byte_pos * 8 + self.bit_pos as usize + bits_to_advance;
        self.byte_pos = next_bit / 8;
        self.bit_pos = (next_bit % 8) as u8;
    }

    fn read_bit(&mut self) -> Result<bool> {
        if self.byte_pos >= self.data.len() {
            return Err(Error::AacParse {
                message: "unexpected end of bitstream".into(),
            });
        }
        let bit = ((self.data[self.byte_pos] >> (7 - self.bit_pos)) & 1) != 0;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Ok(bit)
    }

    fn skip_bits(&mut self, n: usize) -> Result<()> {
        let total_bits = self.byte_pos * 8 + self.bit_pos as usize + n;
        self.byte_pos = total_bits / 8;
        self.bit_pos = (total_bits % 8) as u8;
        if self.byte_pos > self.data.len() || (self.byte_pos == self.data.len() && self.bit_pos > 0)
        {
            return Err(Error::AacParse {
                message: "unexpected end of bitstream".into(),
            });
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

#[derive(Clone, Copy)]
struct HuffmanEntry {
    symbol: u16,
    len: u8,
}

struct HuffmanTable {
    lens: &'static [u8],
    codes: &'static [u32],
    max_len: u8,
    entries: Vec<HuffmanEntry>,
}

impl HuffmanTable {
    fn new(lens: &'static [u8], codes: &'static [u32], max_len: u8) -> Self {
        let size = 1usize << max_len;
        let mut entries = vec![HuffmanEntry { symbol: 0, len: 0 }; size];

        for (symbol, (&len, &code)) in lens.iter().zip(codes.iter()).enumerate() {
            if len == 0 {
                continue;
            }
            let prefix = (code as usize) << (max_len - len);
            let fill = 1usize << (max_len - len);
            for slot in entries.iter_mut().skip(prefix).take(fill) {
                *slot = HuffmanEntry {
                    symbol: symbol as u16,
                    len,
                };
            }
        }

        Self {
            lens,
            codes,
            max_len,
            entries,
        }
    }
}

static SCF_HUFFMAN_TABLE: OnceLock<HuffmanTable> = OnceLock::new();
static SPECTRUM_HUFFMAN_TABLES: OnceLock<Vec<HuffmanTable>> = OnceLock::new();

fn scf_huffman_table() -> &'static HuffmanTable {
    SCF_HUFFMAN_TABLE.get_or_init(|| {
        HuffmanTable::new(
            &aac_codebooks::SCF_CB_LENS,
            &aac_codebooks::SCF_CB_CODES,
            aac_codebooks::SCF_CB_MAX_LEN,
        )
    })
}

fn spectrum_huffman_tables() -> &'static [HuffmanTable] {
    SPECTRUM_HUFFMAN_TABLES.get_or_init(|| {
        aac_codebooks::SPECTRUM_CODEBOOKS
            .iter()
            .map(|cb| HuffmanTable::new(cb.lens, cb.codes, cb.max_len))
            .collect()
    })
}

/// Decode one Huffman symbol from the bitstream.
/// Fast path: peek `max_len` bits, look up symbol in precomputed table, advance
/// by the symbol's actual code length. Slow path falls back at end-of-stream.
fn decode_huffman(reader: &mut BitReader, table: &HuffmanTable) -> Result<usize> {
    if reader.bits_remaining() >= table.max_len as usize {
        let bits = reader.peek_bits(table.max_len)? as usize;
        let entry = table.entries[bits];
        if entry.len != 0 {
            reader.advance_bits(entry.len as usize);
            return Ok(entry.symbol as usize);
        }
    }
    decode_huffman_slow(reader, table.lens, table.codes, table.max_len)
}

fn decode_huffman_slow(
    reader: &mut BitReader,
    lens: &[u8],
    codes: &[u32],
    max_len: u8,
) -> Result<usize> {
    let mut code: u32 = 0;
    let mut bits_read: u8 = 0;

    for _ in 0..max_len {
        code = (code << 1) | u32::from(reader.read_bit()?);
        bits_read += 1;

        for (i, (&len, &cw)) in lens.iter().zip(codes.iter()).enumerate() {
            if len == bits_read && cw == code {
                return Ok(i);
            }
        }
    }

    Err(Error::AacParse {
        message: "invalid Huffman code".into(),
    })
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

/// Verify that `count` entries of `entry_size` bytes starting at `start` fit
/// inside `data`. Counts come straight from the file, so a malformed MP4 can
/// otherwise drive out-of-bounds reads or multi-GB allocations.
fn check_table_bounds(
    data: &[u8],
    start: usize,
    count: usize,
    entry_size: usize,
    what: &str,
) -> Result<()> {
    let end = count
        .checked_mul(entry_size)
        .and_then(|n| start.checked_add(n));
    match end {
        Some(end) if end <= data.len() => Ok(()),
        _ => Err(Error::AacParse {
            message: format!("{what} table extends past end of file"),
        }),
    }
}

/// Build sample table: for each AAC sample, compute (file_offset, size).
/// Returns (sample_entries, stsd_pos).
fn build_sample_table(data: &[u8]) -> Result<(Vec<SampleEntry>, usize)> {
    let (moov_pos, moov_header) = mp4meta::find_box(data, mp4meta::MOOV).ok_or(Error::NoMoovBox)?;
    let moov_start = moov_pos + moov_header.header_size as usize;
    let moov_size = moov_header.content_size() as usize;

    let (stbl_start, stbl_size, stsd_pos) = find_audio_stbl(data, moov_start, moov_size)?;

    // Parse STSZ
    let (stsz_pos, stsz_header) = mp4meta::find_box_in_container(data, stbl_start, stbl_size, STSZ)
        .ok_or_else(|| Error::AacParse {
            message: "no stsz box".into(),
        })?;
    let stsz_content = stsz_pos + stsz_header.header_size as usize;
    check_table_bounds(data, stsz_content, 3, 4, "stsz header")?;
    let _version = read_u32_be(data, stsz_content);
    let default_size = read_u32_be(data, stsz_content + 4);
    let sample_count = read_u32_be(data, stsz_content + 8) as usize;

    // Even with a default size (no per-sample table to bound the count),
    // every sample occupies at least one byte of mdat.
    if sample_count > data.len() {
        return Err(Error::AacParse {
            message: "stsz sample count exceeds file size".into(),
        });
    }

    let mut sample_sizes = Vec::with_capacity(sample_count);
    if default_size != 0 {
        sample_sizes.resize(sample_count, default_size);
    } else {
        let sizes_start = stsz_content + 12;
        check_table_bounds(data, sizes_start, sample_count, 4, "stsz")?;
        for i in 0..sample_count {
            sample_sizes.push(read_u32_be(data, sizes_start + i * 4));
        }
    }

    // Parse STSC (sample-to-chunk)
    let (stsc_pos, stsc_header) = mp4meta::find_box_in_container(data, stbl_start, stbl_size, STSC)
        .ok_or_else(|| Error::AacParse {
            message: "no stsc box".into(),
        })?;
    let stsc_content = stsc_pos + stsc_header.header_size as usize;
    check_table_bounds(data, stsc_content, 2, 4, "stsc header")?;
    let stsc_count = read_u32_be(data, stsc_content + 4) as usize;
    let stsc_entries_start = stsc_content + 8;
    check_table_bounds(data, stsc_entries_start, stsc_count, 12, "stsc")?;

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
    if stsc_entries.is_empty() {
        return Err(Error::AacParse {
            message: "empty stsc table".into(),
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

    Ok((entries, stsd_pos))
}

fn find_audio_stbl(
    data: &[u8],
    moov_start: usize,
    moov_size: usize,
) -> Result<(usize, usize, usize)> {
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

        if let Some(result) = find_aac_stbl_in_trak(data, &trak_header, trak_pos) {
            return Ok(result);
        }

        // size == 0 means "extends to EOF" — nothing can follow, and
        // advancing by 0 would loop forever on a malformed file.
        if trak_header.size == 0 {
            break;
        }
        search_pos = trak_pos + trak_header.size as usize;
    }

    Err(Error::NoAacTrack)
}

/// Navigate trak -> mdia -> minf -> stbl -> stsd and check for mp4a codec.
/// Returns (stbl_start, stbl_size, stsd_pos) if this trak contains AAC audio.
fn find_aac_stbl_in_trak(
    data: &[u8],
    trak_header: &mp4meta::BoxHeader,
    trak_pos: usize,
) -> Option<(usize, usize, usize)> {
    let trak_start = trak_pos + trak_header.header_size as usize;
    let trak_size = trak_header.content_size() as usize;

    let (mdia_pos, mdia_h) =
        mp4meta::find_box_in_container(data, trak_start, trak_size, mp4meta::MDIA)?;
    let (minf_pos, minf_h) = mp4meta::find_box_in_container(
        data,
        mdia_pos + mdia_h.header_size as usize,
        mdia_h.content_size() as usize,
        mp4meta::MINF,
    )?;
    let (stbl_pos, stbl_h) = mp4meta::find_box_in_container(
        data,
        minf_pos + minf_h.header_size as usize,
        minf_h.content_size() as usize,
        mp4meta::STBL,
    )?;

    let stbl_start = stbl_pos + stbl_h.header_size as usize;
    let stbl_size = stbl_h.content_size() as usize;

    let (stsd_pos, stsd_h) =
        mp4meta::find_box_in_container(data, stbl_start, stbl_size, mp4meta::STSD)?;

    let entries_start = stsd_pos + stsd_h.header_size as usize + 8;
    if entries_start + 8 > data.len() {
        return None;
    }

    let entry_type = read_u32_be(data, entries_start + 4);
    if entry_type == mp4meta::MP4A {
        Some((stbl_start, stbl_size, stsd_pos))
    } else {
        None
    }
}

fn parse_chunk_offsets(data: &[u8], stbl_start: usize, stbl_size: usize) -> Result<Vec<u64>> {
    // Try STCO first, then CO64
    if let Some((stco_pos, stco_h)) =
        mp4meta::find_box_in_container(data, stbl_start, stbl_size, mp4meta::STCO)
    {
        let content = stco_pos + stco_h.header_size as usize;
        check_table_bounds(data, content, 2, 4, "stco header")?;
        let count = read_u32_be(data, content + 4) as usize;
        check_table_bounds(data, content + 8, count, 4, "stco")?;
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
        check_table_bounds(data, content, 2, 4, "co64 header")?;
        let count = read_u32_be(data, content + 4) as usize;
        check_table_bounds(data, content + 8, count, 8, "co64")?;
        let mut offsets = Vec::with_capacity(count);
        for i in 0..count {
            offsets.push(read_u64_be(data, content + 8 + i * 8));
        }
        return Ok(offsets);
    }

    Err(Error::AacParse {
        message: "no stco or co64 box found".into(),
    })
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
        return Err(Error::AacParse {
            message: "stsd too short".into(),
        });
    }
    let mp4a_size = read_u32_be(data, entries_start) as usize;
    let mp4a_start = entries_start;
    let mp4a_end = mp4a_start + mp4a_size;

    // sample_rate is at byte 32 within mp4a box:
    //   size(4) + type(4) + reserved(6) + data_ref_index(2) + version(2) +
    //   revision(2) + vendor(4) + channel_count(2) + sample_size(2) +
    //   compression_id(2) + packet_size(2) = 32
    let sr_offset = mp4a_start + 32;
    if sr_offset + 4 > data.len() {
        return Err(Error::AacParse {
            message: "mp4a too short for sample rate".into(),
        });
    }
    let sr_fixed = read_u32_be(data, sr_offset);
    let sample_rate = sr_fixed >> 16; // 16.16 fixed point -> integer part

    // Child boxes (esds, etc.) start after the 36-byte mp4a fixed fields
    let esds_search_start = mp4a_start + 36;
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
            return Err(Error::AacParse {
                message: "predictor data not supported for AAC-LC".into(),
            });
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
                return Err(Error::AacParse {
                    message: "reserved codebook 12".into(),
                });
            }

            let mut sect_len = 0usize;
            loop {
                let incr = reader.read_bits(sect_bits)? as usize;
                sect_len += incr;
                if incr < sect_esc_val as usize {
                    break;
                }
            }

            if sect_len == 0 || k + sect_len > info.max_sfb {
                return Err(Error::AacParse {
                    message: "invalid AAC section length".into(),
                });
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
    let scf_table = scf_huffman_table();

    for g in 0..info.window_groups {
        for sfb in 0..info.max_sfb {
            let cb = section.sfb_cb[g][sfb];
            if cb == ZERO_HCB {
                continue;
            }
            if cb == NOISE_HCB && noise_pcm_flag {
                reader.read_bits(9)?;
                noise_pcm_flag = false;
                continue;
            }
            // INTENSITY, NOISE (after first), and regular scalefactors all
            // use the same Huffman codebook
            decode_huffman(reader, scf_table)?;
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
    let huffman_tables = spectrum_huffman_tables();

    for g in 0..info.window_groups {
        // Short-window spectral data is ordered by scalefactor band, then by
        // each window in the group; reversing that order desynchronizes CPEs.
        for sfb in 0..info.max_sfb {
            let cb_idx = section.sfb_cb[g][sfb];
            if matches!(
                cb_idx,
                ZERO_HCB | NOISE_HCB | INTENSITY_HCB | INTENSITY_HCB2
            ) {
                continue;
            }

            let start = bands[sfb];
            let end = bands[sfb + 1];
            let width = end - start;

            let cb_info = &aac_codebooks::SPECTRUM_CODEBOOKS[cb_idx as usize - 1];
            let dim = cb_info.dimension as usize;
            let num_codewords = width / dim;
            let huffman_table = &huffman_tables[cb_idx as usize - 1];

            for _w in 0..info.window_group_len[g] {
                for _ in 0..num_codewords {
                    let symbol = decode_huffman(reader, huffman_table)?;

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
            return Err(Error::AacParse {
                message: "escape sequence too long".into(),
            });
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
        let mut remaining_bands = info.max_sfb;
        let n_filt = reader.read_bits(n_filt_bits)? as usize;
        if n_filt > 0 {
            let coef_res = reader.read_bits(1)?; // coef_res flag
            for _ in 0..n_filt {
                let length = reader.read_bits(length_bits)? as usize;
                if length > remaining_bands {
                    return Err(Error::AacParse {
                        message: "invalid TNS filter length".into(),
                    });
                }
                remaining_bands -= length;
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

    let gain_loc = AacGainLocation::new(
        0, // sample_index: filled in by caller
        0, // file_offset: filled in by caller
        byte_off as u32,
        bit_off,
        channel,
        global_gain,
    );

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
        return Err(Error::AacParse {
            message: format!(
                "max_sfb {} exceeds available bands {}",
                info.max_sfb,
                bands.len() - 1
            ),
        });
    }

    let section = parse_section_data(reader, &info)?;
    parse_scale_factor_data(reader, &info, &section)?;

    // pulse_data
    if reader.read_bit()? {
        if !info.long_win {
            return Err(Error::AacParse {
                message: "pulse data in short window".into(),
            });
        }
        parse_pulse_data(reader)?;
    }

    // tns_data
    if reader.read_bit()? {
        parse_tns_data(reader, &info)?;
    }

    // gain_control_data (SSR only, should be 0 for AAC-LC)
    if reader.read_bit()? {
        return Err(Error::AacParse {
            message: "gain control data not supported for AAC-LC".into(),
        });
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
    // FIL carries extension data including SBR (HE-AAC).
    // Skipping it is safe — the base layer global_gain is in SCE/CPE elements.
    let mut count = reader.read_bits(4)? as usize;
    if count == 15 {
        // The escape count is normally >= 1; saturate so a malformed 0
        // cannot underflow.
        count += (reader.read_bits(8)? as usize).saturating_sub(1);
    }
    reader.skip_bits(count * 8)?;
    Ok(())
}

/// Skip program_config_element (ISO 14496-3 Table 4.2).
/// PCE describes channel configuration but contains no audio data or global_gain.
fn skip_pce(reader: &mut BitReader) -> Result<()> {
    let _element_instance_tag = reader.read_bits(4)?;
    let _object_type = reader.read_bits(2)?;
    let _sampling_frequency_index = reader.read_bits(4)?;
    let num_front = reader.read_bits(4)? as usize;
    let num_side = reader.read_bits(4)? as usize;
    let num_back = reader.read_bits(4)? as usize;
    let num_lfe = reader.read_bits(2)? as usize;
    let num_assoc_data = reader.read_bits(3)? as usize;
    let num_valid_cc = reader.read_bits(4)? as usize;
    let mono_mixdown_present = reader.read_bit()?;
    if mono_mixdown_present {
        reader.read_bits(4)?;
    }
    let stereo_mixdown_present = reader.read_bit()?;
    if stereo_mixdown_present {
        reader.read_bits(4)?;
    }
    let matrix_mixdown_idx_present = reader.read_bit()?;
    if matrix_mixdown_idx_present {
        reader.read_bits(3)?;
    }
    // front: is_cpe(1) + tag(4) = 5 bits each
    for _ in 0..num_front {
        reader.read_bits(5)?;
    }
    // side: is_cpe(1) + tag(4) = 5 bits each
    for _ in 0..num_side {
        reader.read_bits(5)?;
    }
    // back: is_cpe(1) + tag(4) = 5 bits each
    for _ in 0..num_back {
        reader.read_bits(5)?;
    }
    // LFE: tag(4) only
    for _ in 0..num_lfe {
        reader.read_bits(4)?;
    }
    // assoc_data: tag(4)
    for _ in 0..num_assoc_data {
        reader.read_bits(4)?;
    }
    // valid_cc: is_ind_sw(1) + tag(4) = 5 bits each
    for _ in 0..num_valid_cc {
        reader.read_bits(5)?;
    }
    reader.byte_align();
    let comment_len = reader.read_bits(8)? as usize;
    reader.skip_bits(comment_len * 8)?;
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
            ID_CCE => {
                // Coupling channel — complex interactions with other channels,
                // skip this sample to avoid unintended effects
                return Err(Error::AacParse {
                    message: "CCE element found - sample skipped".into(),
                });
            }
            ID_DSE => skip_dse(reader)?,
            ID_PCE => skip_pce(reader)?,
            ID_FIL => skip_fil(reader)?,
            ID_END => break,
            _ => {
                return Err(Error::AacParse {
                    message: format!("unsupported element type {}", id),
                });
            }
        }
    }

    Ok(locations)
}

// =============================================================================
// Gain write helpers
// =============================================================================

/// Read 8-bit value at bit-unaligned position in file data
fn read_aac_gain_at(data: &[u8], loc: &AacGainLocation) -> u8 {
    crate::frame::read_bits_u8(data, loc.file_offset as usize, loc.bit_offset)
}

/// Write 8-bit value at bit-unaligned position in file data
fn write_aac_gain_at(data: &mut [u8], loc: &AacGainLocation, value: u8) {
    crate::frame::write_bits_u8(data, loc.file_offset as usize, loc.bit_offset, value)
}

/// Adjust gain with saturating clamp to 0-255
fn adjust_aac_gain_value(current: u8, steps: i32) -> u8 {
    crate::frame::adjust_gain_value(current, steps, crate::frame::GainMode::Saturating)
}

/// Apply gain adjustment to all gain locations in a file buffer.
/// Skips locations where current gain is 0 (silence).
/// Returns the number of modified gain locations.
fn apply_aac_gain_to_data(data: &mut [u8], analysis: &AacAnalysis, gain_steps: i32) -> usize {
    let mut modified = 0usize;
    for loc in &analysis.gain_locations {
        let current = read_aac_gain_at(data, loc);
        if current == 0 {
            continue;
        }
        let new_value = adjust_aac_gain_value(current, gain_steps);
        if new_value != current {
            write_aac_gain_at(data, loc, new_value);
            modified += 1;
        }
    }
    modified
}

// =============================================================================
// Public API
// =============================================================================

/// Apply gain adjustment to AAC/M4A file (lossless, in-place modification).
///
/// Modifies `global_gain` values directly in the file without container rewriting.
/// Each gain step is approximately 1.5 dB. Values are clamped to 0-255 range.
///
/// Returns the number of modified gain locations.
pub fn apply_aac_gain(file_path: &Path, gain_steps: i32) -> Result<usize> {
    apply_aac_gain_to_path(file_path, file_path, gain_steps)
}

/// Apply gain reading from `read_from` and writing to `write_to`.
///
/// When `read_from == write_to`, behaves identically to [`apply_aac_gain`].
/// When the paths differ, reads `read_from` once and writes the modified bytes
/// directly to `write_to` — the caller (e.g. `apply_with_temp_file`) is
/// responsible for the rename to atomically swap the file (issue #135).
pub fn apply_aac_gain_to_path(read_from: &Path, write_to: &Path, gain_steps: i32) -> Result<usize> {
    if gain_steps == 0 {
        if read_from != write_to {
            std::fs::copy(read_from, write_to).map_err(|e| Error::io_write(write_to, e))?;
        }
        return Ok(0);
    }

    let mut data = std::fs::read(read_from).map_err(|e| Error::io_read(read_from, e))?;

    let analysis = analyze_aac_gains_from_data(&data)?;

    let modified = apply_aac_gain_to_data(&mut data, &analysis, gain_steps);

    std::fs::write(write_to, &data).map_err(|e| Error::io_write(write_to, e))?;

    Ok(modified)
}

/// Apply gain adjustment and store undo information in iTunes freeform tags.
///
/// Undo tags are stored cumulatively: each application adds to the existing
/// undo value, so multiple gain changes can be fully reversed with a single undo.
///
/// Returns the number of modified gain locations.
//
// Performs exactly one read and one atomic write: analysis, undo-tag parsing,
// gain application, and metadata rewrite all run against an in-memory buffer
// (issue #135).
pub fn apply_aac_gain_with_undo(file_path: &Path, gain_steps: i32) -> Result<usize> {
    apply_aac_gain_with_undo_to_path(file_path, file_path, gain_steps)
}

/// Read-from-A / write-to-B variant of [`apply_aac_gain_with_undo`].
///
/// The atomic-write step targets `write_to`, so when this is called from
/// `apply_with_temp_file` with `write_to == temp_path`, the temp file ends up
/// containing the fully-rewritten MP4 ready to be renamed over the original
/// (issue #135).
pub fn apply_aac_gain_with_undo_to_path(
    read_from: &Path,
    write_to: &Path,
    gain_steps: i32,
) -> Result<usize> {
    if gain_steps == 0 {
        if read_from != write_to {
            std::fs::copy(read_from, write_to).map_err(|e| Error::io_write(write_to, e))?;
        }
        return Ok(0);
    }

    let mut data = std::fs::read(read_from).map_err(|e| Error::io_read(read_from, e))?;

    let analysis = analyze_aac_gains_from_data(&data)?;

    let existing_undo = mp4meta::read_undo_tags_from_data(&data);
    let existing_gain = crate::ape::parse_undo_values(existing_undo.undo()).0;
    let new_undo_gain = existing_gain + gain_steps;

    let modified = apply_aac_gain_to_data(&mut data, &analysis, gain_steps);

    let minmax = existing_undo
        .minmax()
        .map(|s| s.to_string())
        .or_else(|| Some(format!("{},{}", analysis.min_gain(), analysis.max_gain())));
    let undo_tags = mp4meta::UndoTags::new(
        Some(crate::ape::format_undo_value(
            new_undo_gain,
            new_undo_gain,
            false,
        )),
        minmax,
    );

    let final_data = mp4meta::update_mp4_undo_metadata(&data, &undo_tags)?;
    mp4meta::atomic_write(write_to, &final_data)?;

    Ok(modified)
}

/// Undo gain changes on AAC/M4A file using stored undo information.
///
/// Reads the cumulative gain adjustment from undo tags, applies the inverse,
/// and removes the undo tags. Note: the result is functionally equivalent to the
/// original but may not be bit-for-bit identical due to MP4 container restructuring.
///
/// Returns the number of modified gain locations.
pub fn undo_aac_gain(file_path: &Path) -> Result<usize> {
    let undo_tags = mp4meta::read_undo_tags(file_path)?;
    let undo_str = undo_tags.undo().ok_or(Error::NoUndoTag)?;

    let undo_gain = crate::ape::parse_undo_values(Some(undo_str)).0;

    if undo_gain == 0 {
        return Ok(0);
    }

    // Apply inverse gain
    let analysis = analyze_aac_gains(file_path)?;
    let mut data = std::fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;

    let modified = apply_aac_gain_to_data(&mut data, &analysis, -undo_gain);

    std::fs::write(file_path, &data).map_err(|e| Error::io_write(file_path, e))?;

    // Remove undo tags
    mp4meta::delete_undo_tags(file_path)?;

    Ok(modified)
}

/// Analyze AAC/M4A file and locate all global_gain fields (read-only)
pub fn analyze_aac_gains(file_path: &Path) -> Result<AacAnalysis> {
    if !mp4meta::is_mp4_file(file_path) {
        return Err(Error::NotMp4File {
            path: file_path.to_path_buf(),
        });
    }

    let data = std::fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    analyze_aac_gains_from_data(&data)
}

/// Slice-based variant of `analyze_aac_gains`. The caller is responsible for
/// validating MP4 brand (e.g. via `mp4meta::is_mp4_file`) when working from a
/// path; this function assumes `data` already represents an MP4 container and
/// returns parser-level errors only.
pub(crate) fn analyze_aac_gains_from_data(data: &[u8]) -> Result<AacAnalysis> {
    let (sample_table, stsd_pos) = build_sample_table(data)?;
    let sample_rate = parse_audio_config(data, stsd_pos)?;

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
        return Err(Error::AacParseFailure {
            warnings: parse_warnings,
        });
    }

    Ok(AacAnalysis::new(
        all_locations,
        sample_count,
        max_channel + 1,
        min_gain,
        max_gain,
        sample_rate,
        parse_warnings,
    ))
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
        let table = &spectrum_huffman_tables()[0];
        let symbol = decode_huffman(&mut reader, table).unwrap();
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

    // =========================================================================
    // Gain write tests
    // =========================================================================

    fn make_loc(file_offset: u64, bit_offset: u8, original_gain: u8) -> AacGainLocation {
        AacGainLocation::new(0, file_offset, 0, bit_offset, 0, original_gain)
    }

    #[test]
    fn test_read_write_aac_gain_aligned() {
        let mut data = vec![0xAB, 0xCD, 0xEF];
        let loc = make_loc(1, 0, 0xCD);
        assert_eq!(read_aac_gain_at(&data, &loc), 0xCD);
        write_aac_gain_at(&mut data, &loc, 0x42);
        assert_eq!(data[1], 0x42);
        assert_eq!(data[0], 0xAB); // unchanged
        assert_eq!(data[2], 0xEF); // unchanged
    }

    #[test]
    fn test_read_write_aac_gain_unaligned() {
        let mut data = vec![0xAB, 0xCD, 0xEF];
        let loc = make_loc(1, 4, 0);
        // Read: high = 0xCD << 4 = 0xD0, low = 0xEF >> 4 = 0x0E -> 0xDE
        assert_eq!(read_aac_gain_at(&data, &loc), 0xDE);
        write_aac_gain_at(&mut data, &loc, 0x99);
        // data[1]: upper nibble preserved (0xC_), lower = 0x99 >> 4 = 0x09 -> 0xC9
        assert_eq!(data[1], 0xC9);
        // data[2]: lower nibble preserved (0x_F), upper = 0x99 << 4 = 0x90 -> 0x9F
        assert_eq!(data[2], 0x9F);
        assert_eq!(data[0], 0xAB); // unchanged
    }

    #[test]
    fn test_read_write_roundtrip_all_offsets() {
        for bit_off in 0..8u8 {
            let mut data = vec![0x00; 4];
            let loc = make_loc(1, bit_off, 0);
            write_aac_gain_at(&mut data, &loc, 0xA5);
            assert_eq!(
                read_aac_gain_at(&data, &loc),
                0xA5,
                "roundtrip failed at bit_offset={}",
                bit_off
            );
        }
    }

    #[test]
    fn test_adjust_aac_gain_saturating() {
        assert_eq!(adjust_aac_gain_value(100, 10), 110);
        assert_eq!(adjust_aac_gain_value(100, -10), 90);
        assert_eq!(adjust_aac_gain_value(250, 10), 255);
        assert_eq!(adjust_aac_gain_value(5, -10), 0);
        assert_eq!(adjust_aac_gain_value(0, 10), 10);
        assert_eq!(adjust_aac_gain_value(128, 0), 128);
    }

    #[test]
    fn test_apply_aac_gain_skips_silence() {
        let mut data = vec![0x00; 100];
        data[10] = 0x00; // silence
        data[20] = 80; // non-silence
        data[30] = 80; // non-silence

        let analysis = AacAnalysis::new(
            vec![make_loc(10, 0, 0), make_loc(20, 0, 80), make_loc(30, 0, 80)],
            3,
            1,
            0,
            80,
            44100,
            0,
        );

        let modified = apply_aac_gain_to_data(&mut data, &analysis, 5);
        assert_eq!(modified, 2);
        assert_eq!(data[10], 0); // silence unchanged
        assert_eq!(data[20], 85); // 80 + 5
        assert_eq!(data[30], 85); // 80 + 5
    }

    #[test]
    fn test_apply_aac_gain_zero_steps() {
        let mut data = vec![0x00; 50];
        data[10] = 80;

        let analysis = AacAnalysis::new(vec![make_loc(10, 0, 80)], 1, 1, 80, 80, 44100, 0);

        let modified = apply_aac_gain_to_data(&mut data, &analysis, 0);
        assert_eq!(modified, 0);
        assert_eq!(data[10], 80); // unchanged
    }
}
