//! Raw ADTS AAC stream support (issue #330).
//!
//! `.aac` files produced by `ffmpeg -f adts`, DVB/HLS captures and some
//! rippers are raw ADTS: a bare sequence of frames, each a 7-byte header
//! (9 with CRC) followed by one AAC raw_data_block. There is no `moov`, so the
//! MP4 sample table [`crate::aac`] walks does not exist, but the bitstream
//! inside a frame *is* a raw_data_block, so the `global_gain` parser, the
//! bit-level gain writer and [`crate::aac::AacAnalysis`] are all shared
//! verbatim. Only the framing and the sample rate lookup differ.
//!
//! The one behavioural difference that reaches the caller is metadata: with no
//! `moov` to hold the iTunes freeform atoms the M4A path writes, the undo and
//! `REPLAYGAIN_*` values go into an ID3v2 tag at the front of the stream,
//! which is the convention taggers and players already use for `.aac`. The
//! `-s a` / `-s i` layout choice selects a container for *MP3* tags and does
//! not apply here.

use std::fs;
use std::path::Path;

use crate::aac::{apply_aac_gain_to_data, AacAnalysis, AacGainLocation, BitReader};
use crate::error::{Error, Result};
use crate::frame::{find_audio_end, skip_id3v2};

/// Fixed ADTS header length, without the optional 16-bit CRC.
const HEADER_LEN: usize = 7;

/// `sampling_frequency_index` table. Indices 13-15 are reserved.
const SAMPLE_RATES: [u32; 13] = [
    96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350,
];

struct AdtsHeader {
    /// Bytes from the start of the frame to its first raw_data_block.
    header_len: usize,
    /// `aac_frame_length`: the whole frame including the header.
    frame_len: usize,
    sample_rate: u32,
    channel_config: u8,
    /// `number_of_raw_data_blocks_in_frame` (already un-biased).
    blocks: u8,
}

/// Parse the ADTS header at the start of `data`, or `None` if there isn't one.
fn parse_adts_header(data: &[u8]) -> Option<AdtsHeader> {
    let h = data.get(..HEADER_LEN)?;

    // 12-bit syncword plus `layer == 00`. The layer bits are what separate
    // ADTS from an MPEG audio frame, whose 11-bit sync word is otherwise a
    // prefix of the same `0xFFF` pattern — layer 00 is reserved (and rejected
    // by `frame::parse_header`) in MPEG audio.
    if h[0] != 0xFF || h[1] & 0xF6 != 0xF0 {
        return None;
    }

    let sample_rate = *SAMPLE_RATES.get(((h[2] >> 2) & 0x0F) as usize)?;
    let channel_config = ((h[2] & 0x01) << 2) | (h[3] >> 6);
    let frame_len = ((h[3] as usize & 0x03) << 11) | ((h[4] as usize) << 3) | (h[5] as usize >> 5);
    // protection_absent == 0 means a 16-bit crc_check follows the header.
    let header_len = if h[1] & 0x01 != 0 {
        HEADER_LEN
    } else {
        HEADER_LEN + 2
    };

    if frame_len <= header_len {
        return None;
    }

    Some(AdtsHeader {
        header_len,
        frame_len,
        sample_rate,
        channel_config,
        blocks: (h[6] & 0x03) + 1,
    })
}

/// Whether `data` starts on an ADTS frame, corroborated by a second syncword
/// exactly `aac_frame_length` bytes later. A single header is a weak signal —
/// the pattern occurs in arbitrary payload bytes — so the follow-up check is
/// what makes this usable as container detection. A stream whose first frame
/// reaches the end of `data` has no second frame to check and is accepted on
/// the first header alone.
fn has_adts_frames(data: &[u8]) -> bool {
    let Some(first) = parse_adts_header(data) else {
        return false;
    };
    match data.get(first.frame_len..) {
        Some(rest) if rest.len() >= HEADER_LEN => parse_adts_header(rest).is_some(),
        _ => true,
    }
}

/// Whether `file_path` is a raw ADTS AAC stream.
///
/// Reads the head only: a leading ID3v2 tag is skipped by its declared size
/// (cover art can run to megabytes) and a small window read after it, which
/// holds the two frame headers [`has_adts_frames`] needs.
pub fn is_adts_file(file_path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    // The largest ADTS frame (`aac_frame_length` is 13 bits, so up to 8191
    // bytes) plus the header after it, so the second-syncword check always has
    // something to look at.
    const WINDOW: usize = 8 * 1024 + HEADER_LEN;

    let Ok(mut file) = fs::File::open(file_path) else {
        return false;
    };
    let mut head = [0u8; 10];
    let Ok(n) = file.read(&mut head) else {
        return false;
    };
    let start = skip_id3v2(&head[..n]) as u64;
    if file.seek(SeekFrom::Start(start)).is_err() {
        return false;
    }

    let mut window = [0u8; WINDOW];
    let mut filled = 0;
    while filled < window.len() {
        match file.read(&mut window[filled..]) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(_) => return false,
        }
        // Every file mp3rgain touches is classified through here, so stop as
        // soon as the first header rules ADTS out: an MP3 costs one short read
        // instead of the whole window.
        if filled >= HEADER_LEN && parse_adts_header(&window[..filled]).is_none() {
            return false;
        }
    }
    has_adts_frames(&window[..filled])
}

/// [`is_adts_file`] on a whole file already in memory.
pub fn is_adts_data(data: &[u8]) -> bool {
    let start = skip_id3v2(data);
    data.get(start..).is_some_and(has_adts_frames)
}

/// Locate every `global_gain` field in a raw ADTS stream (read-only).
pub fn analyze_adts_gains(file_path: &Path) -> Result<AacAnalysis> {
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    analyze_adts_gains_from_data(&data)
}

/// Slice-based [`analyze_adts_gains`]. Assumes `data` is a raw ADTS stream
/// (see [`is_adts_data`]) and returns parser-level errors only.
pub fn analyze_adts_gains_from_data(data: &[u8]) -> Result<AacAnalysis> {
    let start = skip_id3v2(data);
    let end = find_audio_end(data).max(start);

    let mut all_locations = Vec::new();
    let mut block_locations = Vec::with_capacity(8);
    let mut parse_warnings = 0u32;
    let mut min_gain = 255u8;
    let mut max_gain = 0u8;
    let mut channel_count = 1u8;
    let mut sample_rate = 0u32;
    let mut frame_count = 0u32;
    let mut pos = start;

    while pos < end {
        let Some(header) = parse_adts_header(&data[pos..end]) else {
            // Not a resync point: mp3rgain only ever writes into existing
            // fields, so a stream that stops looking like ADTS mid-way is
            // truncated or spliced, and guessing where it resumes risks
            // writing into the wrong bits.
            break;
        };
        if pos + header.frame_len > end {
            break;
        }
        if frame_count == 0 {
            sample_rate = header.sample_rate;
            channel_count = header.channel_config.max(1);
        }
        let frame_index = frame_count;
        frame_count += 1;
        let next = pos + header.frame_len;

        // `number_of_raw_data_blocks_in_frame > 1` puts a position table and
        // per-block CRCs ahead of the blocks. No encoder in practice emits it,
        // so count the frame as unparsed rather than guess at the layout.
        if header.blocks != 1 {
            parse_warnings += 1;
            pos = next;
            continue;
        }

        let block_start = pos + header.header_len;
        let mut reader = BitReader::new(&data[block_start..next]);
        block_locations.clear();
        // Per-frame rate, not the stream's: a spliced stream that switches
        // rate mid-way still parses its later frames correctly, and in a
        // normal stream every frame carries the same value anyway.
        match crate::aac::parse_raw_data_block(
            &mut reader,
            header.sample_rate,
            &mut block_locations,
        ) {
            Ok(()) => {
                for loc in block_locations.drain(..) {
                    min_gain = min_gain.min(loc.original_gain());
                    max_gain = max_gain.max(loc.original_gain());
                    channel_count = channel_count.max(loc.channel() + 1);
                    all_locations.push(AacGainLocation::new(
                        frame_index,
                        (block_start + loc.sample_byte_offset() as usize) as u64,
                        loc.sample_byte_offset(),
                        loc.bit_offset(),
                        loc.channel(),
                        loc.original_gain(),
                    ));
                }
            }
            Err(_) => parse_warnings += 1,
        }
        pos = next;
    }

    if frame_count == 0 {
        return Err(Error::AacParse {
            message: "no ADTS frames found".into(),
        });
    }
    if all_locations.is_empty() {
        return Err(Error::AacParseFailure {
            warnings: parse_warnings.max(1),
        });
    }

    Ok(AacAnalysis::new(
        all_locations,
        frame_count,
        channel_count,
        min_gain,
        max_gain,
        sample_rate,
        parse_warnings,
    ))
}

/// Outcome of [`apply_adts_gain`].
pub struct AdtsApplyOutcome {
    /// `global_gain` fields actually changed.
    pub modified: usize,
    /// Post-apply `(max, min)` `global_gain` range, `None` for a zero-step
    /// apply that never scanned the stream.
    pub gain_range: Option<(u8, u8)>,
}

/// Apply `gain_steps` to every `global_gain` in a raw ADTS stream, reading
/// `read_from` and writing `write_to` (in place when they are the same path).
///
/// Each step is ~1.5 dB and values saturate at 0-255, matching the M4A path.
/// Metadata is the caller's job: with no container box for it, undo and
/// `REPLAYGAIN_*` go into ID3v2 (see [`crate::apply::apply_with_options`]).
pub fn apply_adts_gain(
    read_from: &Path,
    write_to: &Path,
    gain_steps: i32,
    analysis: Option<AacAnalysis>,
) -> Result<AdtsApplyOutcome> {
    if gain_steps == 0 {
        if read_from != write_to {
            fs::copy(read_from, write_to).map_err(|e| Error::io_write(write_to, e))?;
        }
        return Ok(AdtsApplyOutcome {
            modified: 0,
            gain_range: None,
        });
    }

    let mut data = fs::read(read_from).map_err(|e| Error::io_read(read_from, e))?;
    let analysis = match analysis {
        Some(a) => a,
        None => analyze_adts_gains_from_data(&data)?,
    };
    let modified = apply_aac_gain_to_data(&mut data, &analysis, gain_steps);

    // Post-apply range, from the analysis rather than a second full parse of
    // the written stream: the analysis describes exactly the bytes just
    // adjusted, and the adjustment is the same saturating step count applied
    // to every non-silent field.
    let gain_range = post_apply_range(&analysis, gain_steps);

    crate::aac::write_container(read_from, write_to, &data)?;

    Ok(AdtsApplyOutcome {
        modified,
        gain_range,
    })
}

/// `(max, min)` `global_gain` after a saturating `gain_steps` apply. Silent
/// fields (`global_gain == 0`) are left alone by the apply, so they carry
/// through unchanged.
fn post_apply_range(analysis: &AacAnalysis, gain_steps: i32) -> Option<(u8, u8)> {
    let mut min = u8::MAX;
    let mut max = u8::MIN;
    for loc in analysis.gain_locations() {
        let value = if loc.original_gain() == 0 {
            0
        } else {
            crate::frame::adjust_gain_value(
                loc.original_gain(),
                gain_steps,
                crate::frame::GainMode::Saturating,
            )
        };
        min = min.min(value);
        max = max.max(value);
    }
    (max >= min).then_some((max, min))
}

/// Roll back the gain recorded in the stream's ID3v2 `MP3GAIN_UNDO` frame and
/// strip every gain tag mp3rgain wrote.
///
/// The tag follows the MP3 convention rather than the M4A one: it holds the
/// already-negated *undo* delta and is applied as-is (see the note on
/// [`crate::ape::format_undo_value`]).
pub fn undo_adts_gain(file_path: &Path) -> Result<usize> {
    let stored = crate::id3v2::read_id3v2_replaygain(file_path)?;
    let undo_str = stored.undo.as_deref().ok_or(Error::NoId3v2UndoTag)?;
    // ADTS gain is applied uniformly to every channel element, so the two
    // halves of the value are always equal; take either.
    let (undo_steps, _) = crate::ape::parse_undo_values(Some(undo_str));
    if undo_steps == 0 {
        return Ok(0);
    }

    let mut data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    let analysis = analyze_adts_gains_from_data(&data)?;
    let modified = apply_aac_gain_to_data(&mut data, &analysis, undo_steps);

    // One visible write: a failed tag rewrite can't leave the audio rolled
    // back with the undo tag still claiming a pending delta (issue #227).
    crate::apply::with_temp_file(file_path, |original, temp| {
        fs::write(temp, &data).map_err(|e| Error::io_write(original, e))?;
        // Issue #306: the REPLAYGAIN_* residuals described the gained audio,
        // so they go in the same write as the undo/minmax frames.
        crate::id3v2::delete_id3v2_replaygain_direct(temp)
    })?;

    Ok(modified)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/test_adts.aac"
        ))
        .unwrap()
    }

    #[test]
    fn detects_the_adts_fixture() {
        assert!(is_adts_data(&fixture()));
    }

    #[test]
    fn rejects_mp3_and_mp4() {
        let mp3 = fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/test_stereo.mp3"
        ))
        .unwrap();
        assert!(!is_adts_data(&mp3), "MP3 sync words are not ADTS");

        let m4a = fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/test_aac.m4a"
        ))
        .unwrap();
        assert!(!is_adts_data(&m4a), "AAC in MP4 is not raw ADTS");
    }

    #[test]
    fn analyzes_every_frame_of_the_fixture() {
        let analysis = analyze_adts_gains_from_data(&fixture()).unwrap();
        assert!(analysis.sample_count() > 0);
        assert_eq!(analysis.parse_warnings(), 0);
        assert!(!analysis.gain_locations().is_empty());
        assert_eq!(analysis.sample_rate(), 44100);
        assert!(analysis.min_gain() <= analysis.max_gain());
    }

    #[test]
    fn gain_locations_stay_inside_the_stream() {
        let data = fixture();
        let analysis = analyze_adts_gains_from_data(&data).unwrap();
        for loc in analysis.gain_locations() {
            assert!(
                (loc.file_offset() as usize) < data.len(),
                "offset {} past end of {} byte stream",
                loc.file_offset(),
                data.len()
            );
        }
    }

    #[test]
    fn apply_then_undo_restores_every_gain_field() {
        let data = fixture();
        let analysis = analyze_adts_gains_from_data(&data).unwrap();
        let original: Vec<u8> = analysis
            .gain_locations()
            .iter()
            .map(|l| l.original_gain())
            .collect();

        let mut adjusted = data.clone();
        apply_aac_gain_to_data(&mut adjusted, &analysis, 3);
        assert_ne!(adjusted, data, "apply should change the stream");
        assert_eq!(adjusted.len(), data.len(), "apply is size-preserving");

        let after = analyze_adts_gains_from_data(&adjusted).unwrap();
        for (before, now) in original.iter().zip(after.gain_locations()) {
            let expected = if *before == 0 {
                0
            } else {
                before.wrapping_add(3)
            };
            assert_eq!(now.original_gain(), expected);
        }

        let mut restored = adjusted;
        apply_aac_gain_to_data(&mut restored, &after, -3);
        assert_eq!(restored, data, "undo should be byte-exact");
    }

    #[test]
    fn post_apply_range_tracks_the_applied_steps() {
        let data = fixture();
        let analysis = analyze_adts_gains_from_data(&data).unwrap();
        let (max, min) = post_apply_range(&analysis, 2).unwrap();
        assert_eq!(max, analysis.max_gain().saturating_add(2));
        assert!(min >= analysis.min_gain().min(min));
    }

    #[test]
    fn header_rejects_a_reserved_sample_rate_index() {
        // syncword + layer 00 + protection_absent, then sampling_frequency
        // index 13 (reserved).
        let data = [0xFF, 0xF1, 0b0011_0100, 0x40, 0x00, 0x20, 0x00];
        assert!(parse_adts_header(&data).is_none());
    }

    #[test]
    fn header_rejects_an_mpeg_audio_frame() {
        // 0xFFFB is MPEG-1 Layer III: same sync word, layer bits 01.
        let data = [0xFF, 0xFB, 0x90, 0x00, 0x00, 0x00, 0x00];
        assert!(parse_adts_header(&data).is_none());
    }
}
