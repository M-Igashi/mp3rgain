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
//! - [`bs1770`] - ITU-R BS.1770 loudness engine for the RG2/R128 modes (feature-gated)
//! - [`mp4meta`] - MP4/M4A metadata handling
//! - [`aac`] - AAC bitstream parsing (feature-gated)
//!
//! ## Technical Details
//!
//! Each gain step scales amplitude by 2^(1/4) ≈ 1.505 dB (fixed by MP3 specification).
//! The global_gain field is 8 bits, allowing values 0-255.

#[cfg(feature = "aac")]
pub mod aac;
#[cfg(feature = "aac")]
mod aac_codebooks;

pub mod analysis;
pub mod ape;
pub mod apply;
#[cfg(feature = "replaygain")]
pub mod bs1770;
pub mod error;
mod frame;
pub mod gain;
pub mod id3v2;
pub mod mp4meta;
pub mod replaygain;

pub use analysis::{
    analyze, analyze_data, find_max_amplitude, is_mono, ChannelMode, MaxAmplitudeResult,
    Mp3Analysis, MpegVersion,
};
pub use ape::{
    delete_ape_tag, read_ape_tag, read_ape_tag_from_file, write_ape_album_minmax, write_ape_tag,
    ApeItem, ApeTag, TAG_MP3GAIN_ALBUM_MINMAX, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO,
    TAG_REPLAYGAIN_ALBUM_GAIN, TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_ALGORITHM,
    TAG_REPLAYGAIN_TRACK_GAIN, TAG_REPLAYGAIN_TRACK_PEAK,
};
pub use apply::{
    apply_with_options, predict_apply, write_album_minmax, AacAlbumInfo, ApplyOptions, ApplyReport,
    ClippingDetection,
};
pub use error::{Error, Result};
pub use gain::{
    apply_gain, apply_gain_db, apply_gain_to_peak, db_to_linear, db_to_steps, peak_to_headroom_db,
    peak_to_pcm_sample, steps_to_db, undo_gain, would_clip, Channel, GainOptions, GAIN_STEP_DB,
    MAX_GAIN,
};
pub use id3v2::{
    delete_id3v2_replaygain, read_id3v2_replaygain, undo_gain_id3v2, write_id3v2_replaygain,
    Id3v2ReplayGain,
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

/// Which container MP3 tags are written to.
///
/// The two tag families have different audiences. `REPLAYGAIN_*` is read by
/// players, and they look in ID3v2 — ffmpeg (and everything built on it) does
/// not read APEv2 on MP3 at all, and Rockbox only handles APE tags for WavPack
/// and Musepack. `MP3GAIN_UNDO` / `MP3GAIN_MINMAX` are read by nothing but the
/// mp3gain lineage, which looks in APEv2. [`TagLayout::Split`] therefore sends
/// each family where its readers are, and is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TagLayout {
    /// `REPLAYGAIN_*` in ID3v2 TXXX, `MP3GAIN_*` in APEv2. Default.
    #[default]
    Split,
    /// Everything in APEv2 — byte-for-byte mp3gain behaviour (`-s a`).
    Ape,
    /// Everything in ID3v2 TXXX (`-s i`).
    Id3v2,
}

impl TagLayout {
    /// Whether `REPLAYGAIN_*` goes to ID3v2 TXXX.
    pub fn replaygain_in_id3v2(self) -> bool {
        matches!(self, TagLayout::Split | TagLayout::Id3v2)
    }

    /// Whether `MP3GAIN_UNDO` / `MP3GAIN_MINMAX` go to ID3v2 TXXX.
    pub fn mp3gain_in_id3v2(self) -> bool {
        matches!(self, TagLayout::Id3v2)
    }
}

/// Undo previously-applied gain, auto-dispatching by file format and tag mode.
///
/// AAC files go through the AAC undo path. For MP3 the undo tag is read from
/// wherever `layout` puts it, falling back to the other container so a file
/// tagged under a different layout still rolls back.
pub fn undo_gain_auto(file_path: &Path, layout: TagLayout) -> Result<usize> {
    #[cfg(feature = "aac")]
    {
        if mp4meta::is_aac_file(file_path) {
            return aac::undo_aac_gain(file_path);
        }
    }
    let ape_has_undo = || {
        ape::read_ape_tag_from_file(file_path)
            .ok()
            .flatten()
            .is_some_and(|t| t.get(TAG_MP3GAIN_UNDO).is_some())
    };
    let id3v2_has_undo = || {
        id3v2::read_id3v2_replaygain(file_path)
            .ok()
            .is_some_and(|rg| rg.undo.is_some())
    };

    if layout.mp3gain_in_id3v2() {
        if id3v2_has_undo() || !ape_has_undo() {
            return id3v2::undo_gain_id3v2(file_path);
        }
        return gain::undo_gain(file_path);
    }
    if ape_has_undo() || !id3v2_has_undo() {
        return gain::undo_gain(file_path);
    }
    id3v2::undo_gain_id3v2(file_path)
}

/// Delete ReplayGain / undo tags, auto-dispatching by file format and tag mode.
///
/// For AAC, deletes both the ReplayGain and undo freeform tags. For MP3, both
/// containers are cleared under [`TagLayout::Split`] — the point of `-s d` is
/// to leave no gain tags behind, and a split-tagged file has them in two
/// places.
pub fn delete_gain_tags_auto(file_path: &Path, layout: TagLayout) -> Result<()> {
    #[cfg(feature = "aac")]
    {
        if mp4meta::is_aac_file(file_path) {
            mp4meta::delete_replaygain_tags(file_path)?;
            return mp4meta::delete_undo_tags(file_path);
        }
    }
    match layout {
        TagLayout::Id3v2 => id3v2::delete_id3v2_replaygain(file_path),
        TagLayout::Ape => ape::delete_ape_tag(file_path),
        TagLayout::Split => {
            id3v2::delete_id3v2_replaygain(file_path)?;
            ape::delete_ape_tag(file_path)
        }
    }
}

/// Tag store a [`StoredGainTags`] snapshot was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GainTagSource {
    /// MP4/AAC iTunes freeform tags.
    Aac,
    /// ID3v2 TXXX frames.
    Id3v2,
    /// APEv2 tag. `tag_present` distinguishes a file with no APE tag at all
    /// from one whose APE tag simply carries no mp3gain items.
    Ape { tag_present: bool },
    /// Both containers were read and merged ([`TagLayout::Split`]).
    Split,
}

/// Owned snapshot of the gain tags stored in one file, as returned by
/// [`read_gain_tags_auto`]. `None` means the tag is absent (not an error).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGainTags {
    pub source: GainTagSource,
    pub track_gain: Option<String>,
    pub track_peak: Option<String>,
    pub album_gain: Option<String>,
    pub album_peak: Option<String>,
    /// `REPLAYGAIN_ALGORITHM`; only written by the `--rg2` / `--r128` modes,
    /// so `None` on anything measured with mp3gain-compatible ReplayGain 1.0.
    pub algorithm: Option<String>,
    pub undo: Option<String>,
    pub minmax: Option<String>,
    /// APE-only `MP3GAIN_ALBUM_MINMAX`; always `None` for AAC and ID3v2.
    pub album_minmax: Option<String>,
}

impl StoredGainTags {
    fn empty(source: GainTagSource) -> Self {
        Self {
            source,
            track_gain: None,
            track_peak: None,
            album_gain: None,
            album_peak: None,
            algorithm: None,
            undo: None,
            minmax: None,
            album_minmax: None,
        }
    }

    /// True if at least one gain tag is present.
    pub fn has_any(&self) -> bool {
        self.track_gain.is_some()
            || self.track_peak.is_some()
            || self.album_gain.is_some()
            || self.album_peak.is_some()
            || self.algorithm.is_some()
            || self.undo.is_some()
            || self.minmax.is_some()
            || self.album_minmax.is_some()
    }
}

/// Read stored gain tags (ReplayGain plus mp3gain undo/minmax) without
/// modifying the file, auto-dispatching by file format and tag mode.
///
/// Mirrors [`delete_gain_tags_auto`]'s dispatch: AAC files read the MP4
/// freeform tags, MP3 files read whichever container(s) `layout` uses. Under
/// [`TagLayout::Split`] both are read and merged — each field prefers the
/// container it is written to, then falls back to the other, so tags left by
/// mp3gain or by an earlier `-s i` run still show up. The AAC branch is
/// fail-soft (unreadable tags come back empty); ID3v2/APE read errors are
/// propagated.
pub fn read_gain_tags_auto(file_path: &Path, layout: TagLayout) -> Result<StoredGainTags> {
    #[cfg(feature = "aac")]
    {
        if mp4meta::is_aac_file(file_path) {
            let (undo_tags, rg_tags) = mp4meta::read_gain_tags(file_path).unwrap_or_default();
            return Ok(StoredGainTags {
                source: GainTagSource::Aac,
                track_gain: rg_tags.track_gain().map(str::to_string),
                track_peak: rg_tags.track_peak().map(str::to_string),
                album_gain: rg_tags.album_gain().map(str::to_string),
                album_peak: rg_tags.album_peak().map(str::to_string),
                algorithm: rg_tags.algorithm().map(str::to_string),
                undo: undo_tags.undo().map(str::to_string),
                minmax: undo_tags.minmax().map(str::to_string),
                album_minmax: None,
            });
        }
    }
    if layout.mp3gain_in_id3v2() {
        let rg = id3v2::read_id3v2_replaygain(file_path)?;
        return Ok(StoredGainTags {
            source: GainTagSource::Id3v2,
            track_gain: rg.track_gain,
            track_peak: rg.track_peak,
            album_gain: rg.album_gain,
            album_peak: rg.album_peak,
            algorithm: rg.algorithm,
            undo: rg.undo,
            minmax: rg.minmax,
            album_minmax: None,
        });
    }
    if layout == TagLayout::Split {
        let id3 = id3v2::read_id3v2_replaygain(file_path)?;
        let ape_tag = ape::read_ape_tag_from_file(file_path)?;
        let ape_get = |key: &str| {
            ape_tag
                .as_ref()
                .and_then(|t| t.get(key))
                .map(str::to_string)
        };
        return Ok(StoredGainTags {
            source: GainTagSource::Split,
            track_gain: id3
                .track_gain
                .or_else(|| ape_get(TAG_REPLAYGAIN_TRACK_GAIN)),
            track_peak: id3
                .track_peak
                .or_else(|| ape_get(TAG_REPLAYGAIN_TRACK_PEAK)),
            album_gain: id3
                .album_gain
                .or_else(|| ape_get(TAG_REPLAYGAIN_ALBUM_GAIN)),
            album_peak: id3
                .album_peak
                .or_else(|| ape_get(TAG_REPLAYGAIN_ALBUM_PEAK)),
            algorithm: id3.algorithm.or_else(|| ape_get(TAG_REPLAYGAIN_ALGORITHM)),
            undo: ape_get(TAG_MP3GAIN_UNDO).or(id3.undo),
            minmax: ape_get(TAG_MP3GAIN_MINMAX).or(id3.minmax),
            album_minmax: ape_get(TAG_MP3GAIN_ALBUM_MINMAX),
        });
    }
    match ape::read_ape_tag_from_file(file_path)? {
        Some(tag) => Ok(StoredGainTags {
            source: GainTagSource::Ape { tag_present: true },
            track_gain: tag.get(TAG_REPLAYGAIN_TRACK_GAIN).map(str::to_string),
            track_peak: tag.get(TAG_REPLAYGAIN_TRACK_PEAK).map(str::to_string),
            album_gain: tag.get(TAG_REPLAYGAIN_ALBUM_GAIN).map(str::to_string),
            album_peak: tag.get(TAG_REPLAYGAIN_ALBUM_PEAK).map(str::to_string),
            algorithm: tag.get(TAG_REPLAYGAIN_ALGORITHM).map(str::to_string),
            undo: tag.get(TAG_MP3GAIN_UNDO).map(str::to_string),
            minmax: tag.get(TAG_MP3GAIN_MINMAX).map(str::to_string),
            album_minmax: tag.get(TAG_MP3GAIN_ALBUM_MINMAX).map(str::to_string),
        }),
        None => Ok(StoredGainTags::empty(GainTagSource::Ape {
            tag_present: false,
        })),
    }
}

/// Read the left-channel undo step count without modifying the file.
///
/// Mirrors [`undo_gain_auto`]'s dispatch so the returned value matches what
/// `undo_gain_auto` would roll back. Returns `None` if the tag is absent or
/// unreadable.
pub fn read_undo_steps(file_path: &Path, layout: TagLayout) -> Option<i32> {
    #[cfg(feature = "aac")]
    {
        if mp4meta::is_aac_file(file_path) {
            let undo_tags = mp4meta::read_undo_tags(file_path).ok()?;
            return Some(ape::parse_undo_values(undo_tags.undo()).0);
        }
    }
    let from_ape = || {
        ape::read_ape_tag_from_file(file_path)
            .ok()
            .flatten()
            .and_then(|t| t.get_undo_gain())
    };
    let from_id3v2 = || {
        let rg = id3v2::read_id3v2_replaygain(file_path).ok()?;
        rg.undo
            .as_deref()
            .map(|u| ape::parse_undo_values(Some(u)).0)
    };
    // Same fallback order as undo_gain_auto, so the reported value matches
    // what an undo would actually roll back.
    if layout.mp3gain_in_id3v2() {
        from_id3v2().or_else(from_ape)
    } else {
        from_ape().or_else(from_id3v2)
    }
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
