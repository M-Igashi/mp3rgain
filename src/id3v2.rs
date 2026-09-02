//! ID3v2 TXXX frame storage for ReplayGain and undo tags.
//!
//! Original write-path issue (#115) reported a case where mp3gain values
//! were not persisted to the file; the fix landed in #116 and was later
//! subsumed by a broader rewrite of this module.

use crate::ape::{
    parse_undo_values, parse_undo_wrap, REPLAYGAIN_KEYS, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO,
    TAG_REPLAYGAIN_ALBUM_GAIN, TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_ALGORITHM,
    TAG_REPLAYGAIN_TRACK_GAIN, TAG_REPLAYGAIN_TRACK_PEAK,
};
use crate::error::{Error, Result};
use crate::gain::apply_undo_to_data;
use id3::TagLike;

use std::path::Path;

const TARGET_VERSION: id3::Version = id3::Version::Id3v24;

/// The `REPLAYGAIN_*` value descriptions only (no undo/minmax). Same set as
/// the APEv2 keys — the two containers store identical names.
const RG_VALUE_DESCRIPTIONS: &[&str] = &REPLAYGAIN_KEYS;

/// All known mp3gain/ReplayGain TXXX descriptions.
const ALL_RG_DESCRIPTIONS: &[&str] = &[
    TAG_MP3GAIN_UNDO,
    TAG_MP3GAIN_MINMAX,
    TAG_REPLAYGAIN_TRACK_GAIN,
    TAG_REPLAYGAIN_TRACK_PEAK,
    TAG_REPLAYGAIN_ALBUM_GAIN,
    TAG_REPLAYGAIN_ALBUM_PEAK,
    TAG_REPLAYGAIN_ALGORITHM,
];

/// ReplayGain and undo data stored in ID3v2 TXXX frames
#[derive(Debug, Clone, Default)]
pub struct Id3v2ReplayGain {
    pub track_gain: Option<String>,
    pub track_peak: Option<String>,
    pub album_gain: Option<String>,
    pub album_peak: Option<String>,
    /// `REPLAYGAIN_ALGORITHM`; `None` in the mp3gain-compatible RG1 mode.
    pub algorithm: Option<String>,
    pub undo: Option<String>,
    pub minmax: Option<String>,
}

pub(crate) fn read_tag(path: &Path) -> Result<id3::Tag> {
    match id3::Tag::read_from_path(path) {
        Ok(tag) => Ok(tag),
        Err(id3::Error {
            kind: id3::ErrorKind::NoTag,
            ..
        }) => Ok(id3::Tag::new()),
        Err(e) => Err(Error::Id3v2Error {
            message: e.to_string(),
        }),
    }
}

/// Drop any frame that the `id3` crate refuses to encode in `TARGET_VERSION`.
///
/// An existing v2.2/v2.3 tag can carry frames that cannot be round-tripped into
/// v2.4 without failing the whole write and losing the ReplayGain TXXX frames.
/// Seen in the wild so far:
/// - `ID::Invalid` 3-byte IDs with no v2.3/v2.4 equivalent (e.g. `GP1`).
/// - Frames whose content type mismatches the declared ID and trips
///   `Frame::validate` (e.g. IPLS read as `InvolvedPeopleList` then targeted
///   as a plain text frame via ID remap).
/// - Frames whose content encoding fails in the underlying crate for other
///   reasons.
///
/// Test-encoding each frame in isolation catches all three: if the single-frame
/// tag round-trips, the frame is safe to keep; otherwise we drop it before
/// attempting the full write.
fn is_frame_encodable(frame: &id3::Frame) -> bool {
    if frame.id_for_version(TARGET_VERSION).is_none() {
        return false;
    }
    let mut probe = id3::Tag::new();
    probe.add_frame(frame.clone());
    let mut buf: Vec<u8> = Vec::new();
    id3::Encoder::new()
        .version(TARGET_VERSION)
        .encode(&probe, &mut buf)
        .is_ok()
}

/// Write `tag` to `path`, dropping the frames [`is_frame_encodable`] rejects.
///
/// Takes `&mut` so the unencodable frames can be filtered in place: cloning the
/// tag first would copy every frame, including multi-megabyte embedded art.
///
/// The whole tag is probed once first, and the per-frame filter only runs when
/// that fails. `is_frame_encodable` clones each frame it inspects, so screening
/// every frame up front made a tag with embedded cover art clone and encode
/// those megabytes on every write — for the rare malformed frame the filter
/// actually exists to catch.
fn write_tag_direct(path: &Path, tag: &mut id3::Tag) -> Result<()> {
    if !is_tag_encodable(tag) {
        tag.frames_vec_mut().retain(is_frame_encodable);
    }
    tag.write_to_path(path, TARGET_VERSION)
        .map_err(|e| Error::Id3v2Error {
            message: e.to_string(),
        })
}

/// Whether `tag` encodes cleanly as [`TARGET_VERSION`] as a whole.
fn is_tag_encodable(tag: &id3::Tag) -> bool {
    let mut buf: Vec<u8> = Vec::new();
    id3::Encoder::new()
        .version(TARGET_VERSION)
        .encode(tag, &mut buf)
        .is_ok()
}

/// Rewrite the tag atomically: copy the file to a sibling temp, write the tag
/// there, then fsync + rename over the original (issue #227).
fn write_tag(path: &Path, tag: &mut id3::Tag) -> Result<()> {
    crate::apply::with_temp_file(path, |original, temp| {
        std::fs::copy(original, temp).map_err(|e| Error::io_write(original, e))?;
        write_tag_direct(temp, tag)
    })
}

pub(crate) fn get_txxx(tag: &id3::Tag, description: &str) -> Option<String> {
    let desc_upper = description.to_uppercase();
    tag.extended_texts()
        .find(|t| t.description.to_uppercase() == desc_upper)
        .map(|t| t.value.clone())
}

/// Remove all TXXX frames matching `description` case-insensitively.
///
/// `Tag::add_frame` and `Tag::remove_extended_text` only match the exact
/// description, but other taggers write these descriptions in lowercase —
/// without this, writing our uppercase frame would leave a stale lowercase
/// duplicate behind.
fn remove_txxx_ci(tag: &mut id3::Tag, description: &str) {
    let variants: Vec<String> = tag
        .extended_texts()
        .filter(|t| t.description.eq_ignore_ascii_case(description))
        .map(|t| t.description.clone())
        .collect();
    for desc in variants {
        tag.remove_extended_text(Some(&desc), None);
    }
}

/// Read ReplayGain and undo data from ID3v2 TXXX frames
pub fn read_id3v2_replaygain(path: &Path) -> Result<Id3v2ReplayGain> {
    let tag = read_tag(path)?;
    Ok(Id3v2ReplayGain {
        track_gain: get_txxx(&tag, TAG_REPLAYGAIN_TRACK_GAIN),
        track_peak: get_txxx(&tag, TAG_REPLAYGAIN_TRACK_PEAK),
        album_gain: get_txxx(&tag, TAG_REPLAYGAIN_ALBUM_GAIN),
        album_peak: get_txxx(&tag, TAG_REPLAYGAIN_ALBUM_PEAK),
        algorithm: get_txxx(&tag, TAG_REPLAYGAIN_ALGORITHM),
        undo: get_txxx(&tag, TAG_MP3GAIN_UNDO),
        minmax: get_txxx(&tag, TAG_MP3GAIN_MINMAX),
    })
}

fn add_rg_frames(tag: &mut id3::Tag, rg: &Id3v2ReplayGain) {
    let fields: &[(&str, &Option<String>)] = &[
        (TAG_MP3GAIN_UNDO, &rg.undo),
        (TAG_MP3GAIN_MINMAX, &rg.minmax),
        (TAG_REPLAYGAIN_TRACK_GAIN, &rg.track_gain),
        (TAG_REPLAYGAIN_TRACK_PEAK, &rg.track_peak),
        (TAG_REPLAYGAIN_ALBUM_GAIN, &rg.album_gain),
        (TAG_REPLAYGAIN_ALBUM_PEAK, &rg.album_peak),
        (TAG_REPLAYGAIN_ALGORITHM, &rg.algorithm),
    ];

    for &(desc, value) in fields {
        if let Some(v) = value {
            remove_txxx_ci(tag, desc);
            tag.add_frame(id3::frame::ExtendedText {
                description: desc.to_string(),
                value: v.clone(),
            });
        }
    }
}

/// Write ReplayGain and undo data to ID3v2 TXXX frames (preserves existing ID3v2 data)
pub fn write_id3v2_replaygain(path: &Path, rg: &Id3v2ReplayGain) -> Result<()> {
    let mut tag = read_tag(path)?;
    add_rg_frames(&mut tag, rg);
    write_tag(path, &mut tag)
}

/// [`write_id3v2_replaygain`] without the temp+rename dance, for callers that
/// are already writing onto a not-yet-visible temp file (issue #232).
pub(crate) fn write_id3v2_replaygain_direct(path: &Path, rg: &Id3v2ReplayGain) -> Result<()> {
    let mut tag = read_tag(path)?;
    write_rg_frames_direct(path, &mut tag, rg)
}

/// [`write_id3v2_replaygain_direct`] on a tag the caller already parsed with
/// [`read_tag`], so a caller that needs to inspect the existing frames first
/// (the `-s i` apply reads the prior `MP3GAIN_UNDO`) parses the tag once.
pub(crate) fn write_rg_frames_direct(
    path: &Path,
    tag: &mut id3::Tag,
    rg: &Id3v2ReplayGain,
) -> Result<()> {
    add_rg_frames(tag, rg);
    write_tag_direct(path, tag)
}

/// Delete all ReplayGain and undo TXXX frames from ID3v2 tag
pub fn delete_id3v2_replaygain(path: &Path) -> Result<()> {
    let mut tag = read_tag(path)?;

    for desc in ALL_RG_DESCRIPTIONS {
        remove_txxx_ci(&mut tag, desc);
    }

    write_tag(path, &mut tag)
}

/// Undo gain changes based on ID3v2 undo tag information
pub fn undo_gain_id3v2(path: &Path) -> Result<usize> {
    let rg = read_id3v2_replaygain(path)?;

    let undo_str = rg.undo.as_deref();
    if undo_str.is_none() {
        return Err(Error::NoId3v2UndoTag);
    }
    let (left, right) = parse_undo_values(undo_str);

    if left == 0 && right == 0 {
        return Ok(0);
    }

    let mut data = std::fs::read(path).map_err(|e| Error::io_read(path, e))?;
    let frames = apply_undo_to_data(&mut data, left, right, parse_undo_wrap(undo_str));

    // Revert the audio and strip the undo/minmax frames in one visible write
    // (temp + rename) so a failed tag rewrite can't leave the delta applied
    // with the undo tag still present (issue #227).
    crate::apply::with_temp_file(path, |original, temp| {
        std::fs::write(temp, &data).map_err(|e| Error::io_write(original, e))?;
        let mut tag = read_tag(temp)?;
        // Issue #306: everything mp3rgain stored (undo, minmax, and the
        // REPLAYGAIN_* residuals) described the gained audio, so strip it
        // all in the same write.
        for desc in ALL_RG_DESCRIPTIONS {
            remove_txxx_ci(&mut tag, desc);
        }
        write_tag_direct(temp, &mut tag)?;
        // The APEv2 copies (a stale `REPLAYGAIN_*` set from mp3gain or an
        // earlier `-s a` run, and the album range) described the gained
        // audio too. A tail rewrite on the temp folds their removal into the
        // same rename instead of a second visible write after it.
        crate::ape::remove_ape_undone_gain_values(temp)
    })?;
    Ok(frames)
}

/// Post-undo cleanup for cross-container layouts (issue #306): drop only the
/// `REPLAYGAIN_*` TXXX frames, leaving any `MP3GAIN_UNDO` / `MP3GAIN_MINMAX`
/// alone. Files without ReplayGain frames are left untouched rather than
/// rewritten.
#[allow(dead_code)]
pub(crate) fn remove_id3v2_rg_values(path: &Path) -> Result<()> {
    let mut tag = read_tag(path)?;
    if !strip_rg_values(&mut tag) {
        return Ok(());
    }
    write_tag(path, &mut tag)
}

/// [`remove_id3v2_rg_values`] for a not-yet-visible temp file the caller is
/// about to rename into place (the APEv2 undo path), so the ID3v2 cleanup
/// rides along instead of costing a second full-file copy.
pub(crate) fn remove_id3v2_rg_values_direct(path: &Path) -> Result<()> {
    let mut tag = read_tag(path)?;
    if !strip_rg_values(&mut tag) {
        return Ok(());
    }
    write_tag_direct(path, &mut tag)
}

/// Remove the `REPLAYGAIN_*` frames from `tag`; `false` if there were none.
fn strip_rg_values(tag: &mut id3::Tag) -> bool {
    let has_rg = tag.extended_texts().any(|t| {
        RG_VALUE_DESCRIPTIONS
            .iter()
            .any(|d| t.description.eq_ignore_ascii_case(d))
    });
    if has_rg {
        for desc in RG_VALUE_DESCRIPTIONS {
            remove_txxx_ci(tag, desc);
        }
    }
    has_rg
}

#[cfg(test)]
mod tests {
    use super::*;
    use id3::frame::{Content, ExtendedText};
    use id3::Frame;

    #[test]
    fn write_replaces_lowercase_duplicate_descriptions() {
        // Other taggers write ReplayGain TXXX descriptions in lowercase; the
        // id3 crate's add_frame only replaces exact-case matches, so without
        // remove_txxx_ci a stale lowercase frame would survive next to ours.
        let mut tag = id3::Tag::new();
        tag.add_frame(ExtendedText {
            description: "replaygain_track_gain".to_string(),
            value: "+1.00 dB".to_string(),
        });

        remove_txxx_ci(&mut tag, TAG_REPLAYGAIN_TRACK_GAIN);
        tag.add_frame(ExtendedText {
            description: TAG_REPLAYGAIN_TRACK_GAIN.to_string(),
            value: "+2.00 dB".to_string(),
        });

        let frames: Vec<_> = tag.extended_texts().collect();
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].description, TAG_REPLAYGAIN_TRACK_GAIN);
        assert_eq!(frames[0].value, "+2.00 dB");
    }

    #[test]
    fn extended_text_frame_is_encodable() {
        let frame: Frame = ExtendedText {
            description: "REPLAYGAIN_TRACK_GAIN".to_string(),
            value: "+1.50 dB".to_string(),
        }
        .into();
        assert!(is_frame_encodable(&frame));
    }

    #[test]
    fn invalid_three_byte_id_is_dropped() {
        // "GP1" is an ID3v2.2-only ID that has no v2.3/v2.4 equivalent, so the
        // id3 crate stores it as ID::Invalid and refuses to encode as v2.4.
        let frame = Frame::with_content("GP1", Content::Text("grouping".to_string()));
        assert!(!is_frame_encodable(&frame));
    }

    #[test]
    fn text_content_in_tipl_frame_is_dropped() {
        // TIPL must hold an InvolvedPeopleList; giving it Content::Text trips
        // Frame::validate and blocks the whole tag encode. Our per-frame probe
        // catches this so the RG TXXX frames still get written.
        let frame = Frame::with_content("TIPL", Content::Text("producer".to_string()));
        assert!(!is_frame_encodable(&frame));
    }
}
