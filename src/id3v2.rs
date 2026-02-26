//! ID3v2 TXXX frame storage for ReplayGain and undo tags.

use crate::ape::{
    parse_undo_values, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO, TAG_REPLAYGAIN_ALBUM_GAIN,
    TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_TRACK_GAIN, TAG_REPLAYGAIN_TRACK_PEAK,
};
use crate::error::{Error, Result};
use crate::gain::apply_gain;
use id3::TagLike;

use std::path::Path;

/// All known mp3gain/ReplayGain TXXX descriptions
const ALL_RG_DESCRIPTIONS: &[&str] = &[
    TAG_MP3GAIN_UNDO,
    TAG_MP3GAIN_MINMAX,
    TAG_REPLAYGAIN_TRACK_GAIN,
    TAG_REPLAYGAIN_TRACK_PEAK,
    TAG_REPLAYGAIN_ALBUM_GAIN,
    TAG_REPLAYGAIN_ALBUM_PEAK,
];

/// ReplayGain and undo data stored in ID3v2 TXXX frames
#[derive(Debug, Clone, Default)]
pub struct Id3v2ReplayGain {
    pub track_gain: Option<String>,
    pub track_peak: Option<String>,
    pub album_gain: Option<String>,
    pub album_peak: Option<String>,
    pub undo: Option<String>,
    pub minmax: Option<String>,
}

fn read_tag(path: &Path) -> Result<id3::Tag> {
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

fn write_tag(path: &Path, tag: &id3::Tag) -> Result<()> {
    tag.write_to_path(path, id3::Version::Id3v24)
        .map_err(|e| Error::Id3v2Error {
            message: e.to_string(),
        })
}

fn get_txxx(tag: &id3::Tag, description: &str) -> Option<String> {
    let desc_upper = description.to_uppercase();
    tag.extended_texts()
        .find(|t| t.description.to_uppercase() == desc_upper)
        .map(|t| t.value.clone())
}

/// Read ReplayGain and undo data from ID3v2 TXXX frames
pub fn read_id3v2_replaygain(path: &Path) -> Result<Id3v2ReplayGain> {
    let tag = read_tag(path)?;
    Ok(Id3v2ReplayGain {
        track_gain: get_txxx(&tag, TAG_REPLAYGAIN_TRACK_GAIN),
        track_peak: get_txxx(&tag, TAG_REPLAYGAIN_TRACK_PEAK),
        album_gain: get_txxx(&tag, TAG_REPLAYGAIN_ALBUM_GAIN),
        album_peak: get_txxx(&tag, TAG_REPLAYGAIN_ALBUM_PEAK),
        undo: get_txxx(&tag, TAG_MP3GAIN_UNDO),
        minmax: get_txxx(&tag, TAG_MP3GAIN_MINMAX),
    })
}

/// Write ReplayGain and undo data to ID3v2 TXXX frames (preserves existing ID3v2 data)
pub fn write_id3v2_replaygain(path: &Path, rg: &Id3v2ReplayGain) -> Result<()> {
    let mut tag = read_tag(path)?;

    let fields: &[(&str, &Option<String>)] = &[
        (TAG_REPLAYGAIN_TRACK_GAIN, &rg.track_gain),
        (TAG_REPLAYGAIN_TRACK_PEAK, &rg.track_peak),
        (TAG_REPLAYGAIN_ALBUM_GAIN, &rg.album_gain),
        (TAG_REPLAYGAIN_ALBUM_PEAK, &rg.album_peak),
        (TAG_MP3GAIN_UNDO, &rg.undo),
        (TAG_MP3GAIN_MINMAX, &rg.minmax),
    ];

    for &(desc, value) in fields {
        if let Some(v) = value {
            tag.add_frame(id3::frame::ExtendedText {
                description: desc.to_string(),
                value: v.clone(),
            });
        }
    }

    write_tag(path, &tag)
}

/// Delete all ReplayGain and undo TXXX frames from ID3v2 tag
pub fn delete_id3v2_replaygain(path: &Path) -> Result<()> {
    let mut tag = read_tag(path)?;

    for desc in ALL_RG_DESCRIPTIONS {
        tag.remove_extended_text(Some(desc), None);
    }

    write_tag(path, &tag)
}

/// Write undo information to ID3v2 TXXX frames
pub fn write_id3v2_undo(
    path: &Path,
    left_gain: i32,
    right_gain: i32,
    wrap: bool,
    min: u8,
    max: u8,
) -> Result<()> {
    let mut tag = read_tag(path)?;

    let wrap_flag = if wrap { "W" } else { "N" };
    let undo_value = format!("{:+04},{:+04},{}", left_gain, right_gain, wrap_flag);
    let minmax_value = format!("{},{}", min, max);

    tag.add_frame(id3::frame::ExtendedText {
        description: TAG_MP3GAIN_UNDO.to_string(),
        value: undo_value,
    });
    tag.add_frame(id3::frame::ExtendedText {
        description: TAG_MP3GAIN_MINMAX.to_string(),
        value: minmax_value,
    });

    write_tag(path, &tag)
}

/// Undo gain changes based on ID3v2 undo tag information
pub fn undo_gain_id3v2(path: &Path) -> Result<usize> {
    let rg = read_id3v2_replaygain(path)?;

    let undo_str = rg.undo.as_deref();
    let (left, _right) = parse_undo_values(undo_str);

    if left == 0 {
        if undo_str.is_none() {
            return Err(Error::NoId3v2UndoTag);
        }
        return Ok(0);
    }

    let frames = apply_gain(path, -left)?;

    // Remove undo and minmax tags
    let mut tag = read_tag(path)?;
    tag.remove_extended_text(Some(TAG_MP3GAIN_UNDO), None);
    tag.remove_extended_text(Some(TAG_MP3GAIN_MINMAX), None);
    write_tag(path, &tag)?;

    Ok(frames)
}
