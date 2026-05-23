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
//! ## Optional Features
//!
//! - **replaygain**: Enable ReplayGain analysis (requires symphonia)
//!   - Track gain calculation (`-r` flag)
//!   - Album gain calculation (`-a` flag)
//!
//! ## Example
//!
//! ```no_run
//! use mp3rgain::{apply_gain, apply_gain_db, analyze, GainOptions, Channel};
//! use std::path::Path;
//!
//! // Simple gain adjustment: +2 steps (+3.0 dB)
//! let frames = apply_gain(Path::new("song.mp3"), 2).unwrap();
//! println!("Modified {} frames", frames);
//!
//! // Or specify gain in dB directly
//! let frames = apply_gain_db(Path::new("song.mp3"), 4.5).unwrap();
//!
//! // Builder pattern for advanced options
//! GainOptions::new(5)
//!     .wrap(true)
//!     .undo(true)
//!     .apply(Path::new("song.mp3")).unwrap();
//!
//! // Channel-specific gain with undo support
//! GainOptions::new(3)
//!     .channel(Channel::Left)
//!     .undo(true)
//!     .apply(Path::new("song.mp3")).unwrap();
//! ```
//!
//! ## Modules
//!
//! - [`analysis`] - MP3 file analysis and amplitude detection
//! - [`gain`] - Gain adjustment operations and the [`GainOptions`] builder
//! - [`ape`] - APEv2 tag reading, writing, and management
//! - [`replaygain`] - ReplayGain loudness analysis
//! - [`mp4meta`] - MP4/M4A metadata handling
//! - [`aac`] - AAC bitstream parsing (feature-gated)
//!
//! ## Technical Details
//!
//! Each gain step equals 1.5 dB (fixed by MP3 specification).
//! The global_gain field is 8 bits, allowing values 0-255.

#[cfg(feature = "aac")]
pub mod aac;
#[cfg(feature = "aac")]
mod aac_codebooks;

pub mod analysis;
pub mod ape;
pub mod apply;
pub mod error;
mod frame;
pub mod gain;
pub mod id3v2;
pub mod mp4meta;
pub mod replaygain;

pub use analysis::{
    analyze, find_max_amplitude, is_mono, ChannelMode, MaxAmplitudeResult, Mp3Analysis, MpegVersion,
};
pub use ape::{
    delete_ape_tag, read_ape_tag, read_ape_tag_from_file, write_ape_tag, ApeItem, ApeTag,
    TAG_MP3GAIN_ALBUM_MINMAX, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO, TAG_REPLAYGAIN_ALBUM_GAIN,
    TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_TRACK_GAIN, TAG_REPLAYGAIN_TRACK_PEAK,
};
pub use apply::{
    apply_with_options, predict_apply, AacAlbumInfo, ApplyOptions, ApplyReport, ClippingDetection,
};
pub use error::{Error, Result};
pub use gain::{
    apply_gain, apply_gain_db, apply_gain_to_peak, db_to_linear, db_to_steps, peak_to_headroom_db,
    peak_to_pcm_sample, steps_to_db, undo_gain, would_clip, Channel, GainOptions, GAIN_STEP_DB,
    MAX_GAIN, MIN_GAIN,
};
pub use id3v2::{
    delete_id3v2_replaygain, read_id3v2_replaygain, undo_gain_id3v2, write_id3v2_replaygain,
    write_id3v2_undo, Id3v2ReplayGain,
};

use std::path::{Path, PathBuf};

/// File extensions mp3rgain can process.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "m4a", "aac", "mp4"];

/// Returns true if `path` is a regular audio file mp3rgain can process.
/// Filters out macOS resource fork files (`._*`) and unsupported extensions.
pub fn is_supported_audio_path(path: &Path) -> bool {
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("._"))
    {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|s| ext.eq_ignore_ascii_case(s))
        })
}

/// Collect supported audio file paths from a directory.
///
/// When `recursive` is true, descends into subdirectories. Files are filtered
/// by [`is_supported_audio_path`]. The returned paths are not sorted.
pub fn collect_audio_files(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    collect_audio_files_into(dir, recursive, &mut result)?;
    Ok(result)
}

/// Apply gain in dB, auto-dispatching by file format.
///
/// Detects MP4/AAC files via [`mp4meta::is_aac_file`] and routes them through
/// the AAC pipeline (which rewrites only the AAC `global_gain` bitfields inside
/// `mdat`). All other files fall back to the MP3 pipeline.
///
/// Calling [`gain::apply_gain_db`] directly on an M4A file would scan the raw
/// bytes for MP3 sync words and overwrite the byte following any match,
/// corrupting the MP4 container — see issue #149.
pub fn apply_gain_db_auto(file_path: &Path, gain_db: f64) -> Result<usize> {
    #[cfg(feature = "aac")]
    {
        if mp4meta::is_aac_file(file_path) {
            return aac::apply_aac_gain_to_path(file_path, file_path, gain::db_to_steps(gain_db));
        }
    }
    gain::apply_gain_db(file_path, gain_db)
}

/// Undo previously-applied gain, auto-dispatching by file format and tag mode.
///
/// AAC files go through the AAC undo path. For MP3, `use_id3v2 = true` routes
/// to ID3v2; otherwise the default APE undo is used.
pub fn undo_gain_auto(file_path: &Path, use_id3v2: bool) -> Result<usize> {
    #[cfg(feature = "aac")]
    {
        if mp4meta::is_aac_file(file_path) {
            return aac::undo_aac_gain(file_path);
        }
    }
    if use_id3v2 {
        id3v2::undo_gain_id3v2(file_path)
    } else {
        gain::undo_gain(file_path)
    }
}

/// Delete ReplayGain / undo tags, auto-dispatching by file format and tag mode.
///
/// For AAC, deletes both the ReplayGain and undo freeform tags. For MP3,
/// `use_id3v2 = true` removes the ID3v2 frames; otherwise the APE tag is
/// removed.
pub fn delete_gain_tags_auto(file_path: &Path, use_id3v2: bool) -> Result<()> {
    #[cfg(feature = "aac")]
    {
        if mp4meta::is_aac_file(file_path) {
            mp4meta::delete_replaygain_tags(file_path)?;
            return mp4meta::delete_undo_tags(file_path);
        }
    }
    if use_id3v2 {
        id3v2::delete_id3v2_replaygain(file_path)
    } else {
        ape::delete_ape_tag(file_path)
    }
}

/// Read the left-channel undo step count without modifying the file.
///
/// Mirrors [`undo_gain_auto`]'s dispatch so the returned value matches what
/// `undo_gain_auto` would roll back. Returns `None` if the tag is absent or
/// unreadable.
pub fn read_undo_steps(file_path: &Path, use_id3v2: bool) -> Option<i32> {
    #[cfg(feature = "aac")]
    {
        if mp4meta::is_aac_file(file_path) {
            let undo_tags = mp4meta::read_undo_tags(file_path).ok()?;
            return Some(ape::parse_undo_values(undo_tags.undo()).0);
        }
    }
    if use_id3v2 {
        let rg = id3v2::read_id3v2_replaygain(file_path).ok()?;
        return Some(ape::parse_undo_values(rg.undo.as_deref()).0);
    }
    let tag = ape::read_ape_tag_from_file(file_path).ok()??;
    tag.get_undo_gain()
}

fn collect_audio_files_into(dir: &Path, recursive: bool, result: &mut Vec<PathBuf>) -> Result<()> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io_read(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io_read(dir, e))?;
        let file_type = entry.file_type().map_err(|e| Error::io_read(dir, e))?;
        let path = entry.path();
        if file_type.is_dir() {
            if recursive {
                collect_audio_files_into(&path, recursive, result)?;
            }
        } else if is_supported_audio_path(&path) {
            result.push(path);
        }
    }
    Ok(())
}

#[cfg(all(test, feature = "aac"))]
mod auto_dispatch_tests {
    use super::*;
    use std::io::Write;

    /// Regression for issue #149: applying gain to an MP4 file via the
    /// auto-dispatcher must NOT run the MP3 sync-word scanner, which would
    /// overwrite bytes inside MP4 atoms whenever they happen to look like a
    /// valid MPEG L3 frame header and corrupt the container.
    ///
    /// The crafted MP4 below embeds a 72-byte MPEG2.5 L3 8kbps frame header
    /// immediately after the ftyp box. The buggy MP3 path would treat byte 27
    /// (the `global_gain` location inside the side info) as a writable gain
    /// slot and rewrite it. The dispatch must hand the file to the AAC path
    /// (which rejects it cleanly because there's no `mdat`) and leave the
    /// bytes untouched.
    #[test]
    fn auto_dispatch_does_not_corrupt_mp4_when_payload_mimics_mp3_frame() {
        let dir = std::env::temp_dir().join("mp3rgain_issue_149");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("fake.m4a");

        // ftyp box (20 bytes) + two back-to-back MPEG2.5 L3 8kbps/11025Hz stereo
        // "frames" (52 bytes each). The MP3 scanner validates a frame by looking
        // for another sync word at next_pos or by next_pos == audio_end — so two
        // chained frames make the first one parse as valid.
        let mut bytes = vec![
            0x00, 0x00, 0x00, 0x14, b'f', b't', b'y', b'p', b'M', b'4', b'A', b' ', 0x00, 0x00,
            0x00, 0x00, b'M', b'4', b'A', b' ', // ftyp box (20 bytes, accepted brand)
        ];
        let frame_header = [0xFFu8, 0xE3, 0x10, 0x00];
        bytes.extend_from_slice(&frame_header);
        bytes.resize(20 + 52, 0x55); // pad first frame to 52 bytes
        bytes.extend_from_slice(&frame_header);
        bytes.resize(20 + 52 + 52, 0x55); // pad second frame to 52 bytes
        let original = bytes.clone();

        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();

        let _ = apply_gain_db_auto(&path, 3.0);

        let after = std::fs::read(&path).unwrap();
        assert_eq!(
            after, original,
            "MP4 bytes must be untouched by auto dispatch"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
