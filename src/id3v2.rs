//! ID3v2 TXXX frame storage for ReplayGain and undo tags.
//!
//! Original write-path issue (#115) reported a case where mp3gain values
//! were not persisted to the file; the fix landed in #116 and was later
//! subsumed by a broader rewrite of this module.

use crate::ape::{
    parse_undo_values, parse_undo_wrap, range_wraps_out, shift_stored_value, MP3GAIN_VALUE_KEYS,
    TAG_MP3GAIN_ALBUM_MINMAX, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO, TAG_REPLAYGAIN_ALBUM_GAIN,
    TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_ALGORITHM, TAG_REPLAYGAIN_TRACK_GAIN,
    TAG_REPLAYGAIN_TRACK_PEAK, UNDONE_KEYS,
};
use crate::error::{Error, Result};
use crate::gain::apply_undo_to_data;
use id3::TagLike;

use std::path::Path;

const TARGET_VERSION: id3::Version = id3::Version::Id3v24;

/// ReplayGain and undo data stored in ID3v2 TXXX frames
#[derive(Debug, Clone, Default)]
pub struct Id3v2ReplayGain {
    /// `REPLAYGAIN_TRACK_GAIN`, e.g. `"-3.500000 dB"`. `None` leaves any
    /// existing item alone.
    pub track_gain: Option<String>,
    /// `REPLAYGAIN_TRACK_PEAK`, e.g. `"0.987650"`.
    pub track_peak: Option<String>,
    /// `REPLAYGAIN_ALBUM_GAIN`. Only set when an album was analyzed.
    pub album_gain: Option<String>,
    /// `REPLAYGAIN_ALBUM_PEAK`. Only set when an album was analyzed.
    pub album_peak: Option<String>,
    /// `REPLAYGAIN_ALGORITHM`; `None` in the mp3gain-compatible RG1 mode.
    pub algorithm: Option<String>,
    /// `MP3GAIN_UNDO`, present in this container only under `-s i` and in a
    /// raw ADTS stream.
    pub undo: Option<String>,
    /// `MP3GAIN_MINMAX`, the file's `global_gain` range, `min,max`.
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

/// Padding left after a tag that has to grow, or is new, so the next write,
/// which usually changes a few short values, fits in place (issue #467).
const GROWTH_PADDING: usize = 2048;

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
///
/// The `id3` crate writes in place only when the encoded tag is exactly as
/// long as the region it replaces, padding included, and otherwise moves the
/// whole audio stream after it. So the tag is padded to fill that region when
/// it fits, and given [`GROWTH_PADDING`] when it has to grow (issue #467). The
/// crate still locates the region and does the write; a wrong estimate here
/// only costs the move it used to make every time.
fn write_tag_direct(path: &Path, tag: &mut id3::Tag) -> Result<()> {
    let len = encoded_len(tag).or_else(|| {
        tag.frames_vec_mut().retain(is_frame_encodable);
        encoded_len(tag)
    });
    let padding = match (len, tag_region_len(path)) {
        (Some(len), Some(region)) if len <= region => region - len,
        _ => GROWTH_PADDING,
    };
    id3::Encoder::new()
        .version(TARGET_VERSION)
        .padding(padding)
        .write_to_path(tag, path)
        .map_err(|e| Error::Id3v2Error {
            message: e.to_string(),
        })
}

/// Length of `tag` encoded as [`TARGET_VERSION`] without padding, `None` when
/// it does not encode as a whole.
fn encoded_len(tag: &id3::Tag) -> Option<usize> {
    let mut buf: Vec<u8> = Vec::new();
    id3::Encoder::new()
        .version(TARGET_VERSION)
        .encode(tag, &mut buf)
        .ok()
        .map(|()| buf.len())
}

/// The region the `id3` crate rewrites at the start of `path`: the ID3v2 tag
/// by its declared size and the zero bytes after it, which the crate counts as
/// padding too. `None` without a tag, or when the tag claims the whole file,
/// which the crate then replaces entirely.
fn tag_region_len(path: &Path) -> Option<usize> {
    use std::io::{BufReader, Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let mut head = [0u8; 10];
    file.read_exact(&mut head).ok()?;
    let tag_len = crate::frame::skip_id3v2(&head);
    if tag_len == 0 || tag_len as u64 >= file_len {
        return None;
    }
    file.seek(SeekFrom::Start(tag_len as u64)).ok()?;
    let zeros = BufReader::new(file)
        .bytes()
        .take_while(|b| matches!(b, Ok(0)))
        .count();
    Some(tag_len + zeros)
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

/// Add each `(description, value)` as a TXXX frame unless `tag` already has
/// one, so an ID3v2 value wins over the APEv2 one it is merged with, as in
/// mp3gain's `-s i`, which reads APEv2 first and ID3v2 over it (issue #449).
pub(crate) fn add_missing_txxx(tag: &mut id3::Tag, values: &[(String, String)]) {
    for (description, value) in values {
        if get_txxx(tag, description).is_none() {
            tag.add_frame(id3::frame::ExtendedText {
                description: description.clone(),
                value: value.clone(),
            });
        }
    }
}

/// Replace every TXXX frame `description` names, whatever its case, with one
/// holding `value`.
fn set_txxx(tag: &mut id3::Tag, description: &str, value: String) {
    remove_txxx_ci(tag, description);
    tag.add_frame(id3::frame::ExtendedText {
        description: description.to_string(),
        value,
    });
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
    read_id3v2_stored(path).map(|(rg, _)| rg)
}

/// [`read_id3v2_replaygain`] plus the `MP3GAIN_ALBUM_MINMAX` frame, which
/// `Id3v2ReplayGain` has no field for (issue #450), from one parse.
pub(crate) fn read_id3v2_stored(path: &Path) -> Result<(Id3v2ReplayGain, Option<String>)> {
    let tag = read_tag(path)?;
    let rg = Id3v2ReplayGain {
        track_gain: get_txxx(&tag, TAG_REPLAYGAIN_TRACK_GAIN),
        track_peak: get_txxx(&tag, TAG_REPLAYGAIN_TRACK_PEAK),
        album_gain: get_txxx(&tag, TAG_REPLAYGAIN_ALBUM_GAIN),
        album_peak: get_txxx(&tag, TAG_REPLAYGAIN_ALBUM_PEAK),
        algorithm: get_txxx(&tag, TAG_REPLAYGAIN_ALGORITHM),
        undo: get_txxx(&tag, TAG_MP3GAIN_UNDO),
        minmax: get_txxx(&tag, TAG_MP3GAIN_MINMAX),
    };
    Ok((rg, get_txxx(&tag, TAG_MP3GAIN_ALBUM_MINMAX)))
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
            set_txxx(tag, desc, v.clone());
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

/// Delete all ReplayGain and undo TXXX frames from ID3v2 tag. A file without
/// any is left untouched rather than rewritten with a re-encoded (or, when it
/// had no tag at all, a new empty) ID3v2 tag.
pub fn delete_id3v2_replaygain(path: &Path) -> Result<()> {
    let mut tag = read_tag(path)?;
    if !strip_txxx(&mut tag, &MP3GAIN_VALUE_KEYS) {
        return Ok(());
    }
    write_tag(path, &mut tag)
}

/// [`delete_id3v2_replaygain`] without the temp+rename dance, for callers
/// already writing onto a not-yet-visible temp file (issue #232).
pub(crate) fn delete_id3v2_replaygain_direct(path: &Path) -> Result<()> {
    let mut tag = read_tag(path)?;
    if !strip_txxx(&mut tag, &MP3GAIN_VALUE_KEYS) {
        return Ok(());
    }
    write_tag_direct(path, &mut tag)
}

/// Remove every TXXX frame whose description matches one of `descriptions`,
/// case-insensitively. `true` if any frame was removed.
fn strip_txxx(tag: &mut id3::Tag, descriptions: &[&str]) -> bool {
    let present = tag.extended_texts().any(|t| {
        descriptions
            .iter()
            .any(|d| t.description.eq_ignore_ascii_case(d))
    });
    if present {
        for desc in descriptions {
            remove_txxx_ci(tag, desc);
        }
    }
    present
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
    let frames = apply_undo_to_data(&mut data, left, right, parse_undo_wrap(undo_str))?;

    // Revert the audio and strip the undo/minmax frames in one visible write
    // (temp + rename) so a failed tag rewrite can't leave the delta applied
    // with the undo tag still present (issue #227).
    crate::apply::with_temp_file(path, |original, temp| {
        std::fs::write(temp, &data).map_err(|e| Error::io_write(original, e))?;
        // Issue #306: everything mp3rgain stored (undo, minmax, and the
        // REPLAYGAIN_* residuals) described the gained audio, so strip it
        // all in the same write.
        delete_id3v2_replaygain_direct(temp)?;
        // The APEv2 copies (a stale `REPLAYGAIN_*` set from mp3gain or an
        // earlier `-s a` run, and the album range) described the gained
        // audio too. A tail rewrite on the temp folds their removal into the
        // same rename instead of a second visible write after it.
        crate::ape::remove_ape_undone_gain_values(temp)
    })?;
    Ok(frames)
}

/// Post-undo cleanup for cross-container layouts (issue #306): drop the
/// [`UNDONE_KEYS`] frames, leaving any `MP3GAIN_UNDO` / `MP3GAIN_MINMAX`
/// alone. Files without such frames are left untouched rather than
/// rewritten.
///
/// Writes in place, without the temp+rename dance, because the only caller
/// (the APEv2 undo path) hands it a not-yet-visible temp file it is about to
/// rename over the original — so the ID3v2 cleanup rides along instead of
/// costing a second full-file copy.
pub(crate) fn remove_id3v2_rg_values_direct(path: &Path) -> Result<()> {
    let mut tag = read_tag(path)?;
    if !strip_txxx(&mut tag, &UNDONE_KEYS) {
        return Ok(());
    }
    write_tag_direct(path, &mut tag)
}

/// Add (or replace) the `MP3GAIN_ALBUM_MINMAX` TXXX frame, preserving the
/// rest of the tag, as mp3gain's `-a -s i` stores the album range (issue
/// #450). The ID3v2 tag sits at the front, so this rewrites the file through
/// a temp file.
pub(crate) fn write_id3v2_album_minmax(path: &Path, min: u8, max: u8) -> Result<()> {
    let mut tag = read_tag(path)?;
    set_txxx(
        &mut tag,
        TAG_MP3GAIN_ALBUM_MINMAX,
        crate::ape::format_minmax(min, max),
    );
    write_tag(path, &mut tag)
}

/// Move every TXXX value [`shift_stored_value`] knows by `steps` of applied
/// gain (issue #377), dropping a range that wraps out under `-w`
/// ([`range_wraps_out`], issue #451). Frames are replaced where they stand, so
/// each keeps its description as written (mp3gain's `-s i` uses lowercase)
/// and its place in the tag. `true` if any frame changed.
pub(crate) fn shift_stored_gain(tag: &mut id3::Tag, steps: i32, wrap: bool) -> bool {
    let mut shifted = false;
    if wrap {
        let dropped: Vec<String> = tag
            .extended_texts()
            .filter(|t| range_wraps_out(&t.description, &t.value, steps))
            .map(|t| t.description.clone())
            .collect();
        for description in &dropped {
            tag.remove_extended_text(Some(description), None);
        }
        shifted = !dropped.is_empty();
    }
    for frame in tag.frames_vec_mut() {
        let replacement = match frame.content() {
            id3::Content::ExtendedText(t) => shift_stored_value(&t.description, &t.value, steps)
                .map(|value| id3::frame::ExtendedText {
                    description: t.description.clone(),
                    value,
                }),
            _ => None,
        };
        if let Some(t) = replacement {
            *frame = t.into();
            shifted = true;
        }
    }
    shifted
}

/// [`shift_stored_gain`] on `path`'s tag, written without the temp+rename
/// dance for callers already on a not-yet-visible temp file. Files with
/// nothing to shift are left untouched, and so is a tag the `id3` crate
/// cannot parse: it holds nothing that could be shifted, and the layouts that
/// call this never needed to read ID3v2 before, so it must not fail the apply.
pub(crate) fn shift_id3v2_stored_gain_direct(path: &Path, steps: i32, wrap: bool) -> Result<()> {
    let Ok(mut tag) = read_tag(path) else {
        return Ok(());
    };
    if !shift_stored_gain(&mut tag, steps, wrap) {
        return Ok(());
    }
    write_tag_direct(path, &mut tag)
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

    /// Issue #377: the shift replaces each frame where it stands, keeping the
    /// lowercase description mp3gain's `-s i` writes, and leaves frames that do
    /// not describe the audio's level alone.
    #[test]
    fn shift_keeps_description_and_order() {
        let mut tag = id3::Tag::new();
        for (description, value) in [
            ("replaygain_reference_loudness", "89.0 dB"),
            ("replaygain_track_gain", "-0.380000 dB"),
            ("MP3GAIN_ALBUM_MINMAX", "128,228"),
            ("MP3GAIN_UNDO", "-018,-018,N"),
        ] {
            tag.add_frame(ExtendedText {
                description: description.to_string(),
                value: value.to_string(),
            });
        }

        assert!(shift_stored_gain(&mut tag, 2, false));
        let frames: Vec<_> = tag
            .extended_texts()
            .map(|t| (t.description.as_str(), t.value.as_str()))
            .collect();
        assert_eq!(
            frames,
            [
                ("replaygain_reference_loudness", "89.0 dB"),
                ("replaygain_track_gain", "-3.390300 dB"),
                ("MP3GAIN_ALBUM_MINMAX", "130,230"),
                ("MP3GAIN_UNDO", "-018,-018,N"),
            ]
        );
    }

    /// Issue #467: a tag that fits the region it replaces, padding included,
    /// is written in place, so the audio after it stays where it was. A tag
    /// that has to grow gets room for the next write.
    #[test]
    fn writes_fill_the_old_region_and_leave_room_to_grow() {
        let dir = std::env::temp_dir().join("mp3rgain_id3v2_padding_tests");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("padded.mp3");
        let audio = [0xFFu8, 0xFB, 0x90, 0x00, 1, 2, 3, 4];
        let region_len = |path: &Path| tag_region_len(path).unwrap();
        let rg = |gain: &str| Id3v2ReplayGain {
            track_gain: Some(gain.to_string()),
            ..Default::default()
        };

        // A tag with 1000 bytes of padding, and 200 zero bytes after it that
        // the id3 crate counts as padding too.
        let mut tag = id3::Tag::new();
        tag.set_title("Title");
        let mut file = Vec::new();
        id3::Encoder::new()
            .version(TARGET_VERSION)
            .padding(1000)
            .encode(&tag, &mut file)
            .unwrap();
        file.extend([0u8; 200]);
        file.extend(audio);
        std::fs::write(&path, &file).unwrap();
        let region = region_len(&path);
        assert_eq!(region, file.len() - audio.len());

        for gain in ["-3.390300 dB", "+10.123400 dB", "-0.010000 dB"] {
            write_id3v2_replaygain_direct(&path, &rg(gain)).unwrap();
            let written = std::fs::read(&path).unwrap();
            assert_eq!(written.len(), file.len(), "{gain}");
            assert_eq!(written[region..], audio, "{gain}");
            let stored = read_id3v2_replaygain(&path).unwrap();
            assert_eq!(stored.track_gain.as_deref(), Some(gain));
        }
        assert_eq!(read_tag(&path).unwrap().title(), Some("Title"));

        // No tag yet: the new one is followed by GROWTH_PADDING, and the next
        // write fits in it.
        std::fs::write(&path, audio).unwrap();
        write_id3v2_replaygain_direct(&path, &rg("-1.000000 dB")).unwrap();
        let grown = std::fs::read(&path).unwrap();
        let region = region_len(&path);
        assert!(region >= GROWTH_PADDING, "{region}");
        assert_eq!(grown[region..], audio);
        write_id3v2_replaygain_direct(&path, &rg("+2.000000 dB")).unwrap();
        assert_eq!(std::fs::read(&path).unwrap().len(), grown.len());

        std::fs::remove_file(&path).unwrap();
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
