use crate::error::{Error, Result};
use crate::frame::{read_u32_le, APE_FLAG_HEADER_PRESENT, APE_PREAMBLE};

use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// APEv2 tag version
const APE_VERSION: u32 = 2000;

/// APEv2 tag flag: is header
const APE_FLAG_IS_HEADER: u32 = 1 << 29;

/// MP3Gain specific tag keys
pub const TAG_MP3GAIN_UNDO: &str = "MP3GAIN_UNDO";
pub const TAG_MP3GAIN_MINMAX: &str = "MP3GAIN_MINMAX";
pub const TAG_MP3GAIN_ALBUM_MINMAX: &str = "MP3GAIN_ALBUM_MINMAX";

/// ReplayGain tag keys
pub const TAG_REPLAYGAIN_TRACK_GAIN: &str = "REPLAYGAIN_TRACK_GAIN";
pub const TAG_REPLAYGAIN_TRACK_PEAK: &str = "REPLAYGAIN_TRACK_PEAK";
pub const TAG_REPLAYGAIN_ALBUM_GAIN: &str = "REPLAYGAIN_ALBUM_GAIN";
pub const TAG_REPLAYGAIN_ALBUM_PEAK: &str = "REPLAYGAIN_ALBUM_PEAK";

/// APEv2 tag item
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ApeItem {
    key: String,
    value: String,
}

impl ApeItem {
    pub(crate) fn new(key: String, value: String) -> Self {
        Self { key, value }
    }

    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Display for ApeItem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}={}", self.key, self.value)
    }
}

/// APEv2 tag collection
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ApeTag {
    pub(crate) items: Vec<ApeItem>,
}

impl ApeTag {
    /// Create a new empty APE tag
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Get a tag value by key (case-insensitive)
    pub fn get(&self, key: &str) -> Option<&str> {
        self.items
            .iter()
            .find(|item| item.key.eq_ignore_ascii_case(key))
            .map(|item| item.value.as_str())
    }

    /// Set a tag value (replaces existing if present)
    pub fn set(&mut self, key: &str, value: &str) {
        if let Some(item) = self
            .items
            .iter_mut()
            .find(|item| item.key.eq_ignore_ascii_case(key))
        {
            item.value = value.to_string();
        } else {
            self.items
                .push(ApeItem::new(key.to_uppercase(), value.to_string()));
        }
    }

    /// Remove a tag by key
    pub fn remove(&mut self, key: &str) {
        self.items
            .retain(|item| !item.key.eq_ignore_ascii_case(key));
    }

    /// Check if tag is empty
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Get the number of items in this tag
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Iterate over all items in this tag
    pub fn iter(&self) -> impl Iterator<Item = &ApeItem> {
        self.items.iter()
    }

    /// Get MP3GAIN_UNDO value as gain steps
    pub fn get_undo_gain(&self) -> Option<i32> {
        self.get(TAG_MP3GAIN_UNDO)
            .and_then(|v| v.split(',').next()?.trim().parse::<i32>().ok())
    }

    /// Set MP3GAIN_UNDO value
    pub fn set_undo_gain(&mut self, left_gain: i32, right_gain: i32, wrap: bool) {
        let value = format_undo_value(left_gain, right_gain, wrap);
        self.set(TAG_MP3GAIN_UNDO, &value);
    }

    /// Set MP3GAIN_MINMAX value
    pub fn set_minmax(&mut self, min: u8, max: u8) {
        let value = format!("{},{}", min, max);
        self.set(TAG_MP3GAIN_MINMAX, &value);
    }
}

impl std::fmt::Display for ApeTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ApeTag({} items)", self.items.len())
    }
}

/// Find APEv2 tag footer position in file data
pub(crate) fn find_ape_footer(data: &[u8]) -> Option<usize> {
    if data.len() < 32 {
        return None;
    }

    let footer_start = data.len() - 32;
    if &data[footer_start..footer_start + 8] == APE_PREAMBLE {
        return Some(footer_start);
    }

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

/// Read APEv2 tag from file data
pub fn read_ape_tag(data: &[u8]) -> Option<ApeTag> {
    let footer_start = find_ape_footer(data)?;

    let version = read_u32_le(&data[footer_start + 8..]);
    if version != APE_VERSION {
        return None;
    }

    let tag_size = read_u32_le(&data[footer_start + 12..]) as usize;
    let item_count = read_u32_le(&data[footer_start + 16..]) as usize;

    if footer_start + 32 < tag_size {
        return None;
    }
    let items_start = footer_start + 32 - tag_size;

    let mut tag = ApeTag::new();
    let mut pos = items_start;

    for _ in 0..item_count {
        if pos + 8 > footer_start {
            break;
        }

        let value_size = read_u32_le(&data[pos..]) as usize;
        pos += 8;

        let key_start = pos;
        while pos < footer_start && data[pos] != 0 {
            pos += 1;
        }
        if pos >= footer_start {
            break;
        }

        let key = String::from_utf8_lossy(&data[key_start..pos]).to_string();
        pos += 1;

        if pos + value_size > footer_start {
            break;
        }
        let value = String::from_utf8_lossy(&data[pos..pos + value_size]).to_string();
        pos += value_size;

        tag.items.push(ApeItem::new(key, value));
    }

    Some(tag)
}

/// Read APEv2 tag from file.
///
/// Reads only the file tail: the footer lives in the last 32 bytes
/// (optionally followed by a 128-byte ID3v1 tag), and the tag items in the
/// `tag_size` bytes before it — so a full-file read is never needed.
pub fn read_ape_tag_from_file(file_path: &Path) -> Result<Option<ApeTag>> {
    let mut file = fs::File::open(file_path).map_err(|e| Error::io_read(file_path, e))?;
    let file_len = file
        .metadata()
        .map_err(|e| Error::io_read(file_path, e))?
        .len() as usize;

    // Footer is at EOF-32, or EOF-160 when an ID3v1 tag follows it.
    let probe = read_tail(&mut file, file_path, file_len, file_len.min(160))?;
    let Some(footer_start) = find_ape_footer(&probe) else {
        return Ok(None);
    };

    // `tag_size` covers items + footer; add 32 for an optional header so the
    // tail slice is a superset of what `read_ape_tag` inspects.
    let tag_size = read_u32_le(&probe[footer_start + 12..]) as usize;
    let suffix = probe.len() - footer_start;
    let tail_len = file_len.min(tag_size.saturating_add(32 + suffix));

    if tail_len <= probe.len() {
        return Ok(read_ape_tag(&probe));
    }
    let tail = read_tail(&mut file, file_path, file_len, tail_len)?;
    Ok(read_ape_tag(&tail))
}

fn read_tail(
    file: &mut fs::File,
    file_path: &Path,
    file_len: usize,
    tail_len: usize,
) -> Result<Vec<u8>> {
    file.seek(SeekFrom::Start((file_len - tail_len) as u64))
        .map_err(|e| Error::io_read(file_path, e))?;
    let mut buf = vec![0u8; tail_len];
    file.read_exact(&mut buf)
        .map_err(|e| Error::io_read(file_path, e))?;
    Ok(buf)
}

/// Serialize APE tag to bytes
fn serialize_ape_tag(tag: &ApeTag) -> Vec<u8> {
    if tag.is_empty() {
        return Vec::new();
    }

    let mut items_data = Vec::new();

    for item in &tag.items {
        let value_bytes = item.value.as_bytes();
        let key_bytes = item.key.as_bytes();

        items_data.extend_from_slice(&(value_bytes.len() as u32).to_le_bytes());
        items_data.extend_from_slice(&0u32.to_le_bytes());
        items_data.extend_from_slice(key_bytes);
        items_data.push(0);
        items_data.extend_from_slice(value_bytes);
    }

    let tag_size = items_data.len() + 32;
    let item_count = tag.items.len() as u32;

    let mut result = Vec::new();

    // Header
    result.extend_from_slice(APE_PREAMBLE);
    result.extend_from_slice(&APE_VERSION.to_le_bytes());
    result.extend_from_slice(&(tag_size as u32).to_le_bytes());
    result.extend_from_slice(&item_count.to_le_bytes());
    result.extend_from_slice(&(APE_FLAG_HEADER_PRESENT | APE_FLAG_IS_HEADER).to_le_bytes());
    result.extend_from_slice(&[0u8; 8]);

    // Items
    result.extend_from_slice(&items_data);

    // Footer
    result.extend_from_slice(APE_PREAMBLE);
    result.extend_from_slice(&APE_VERSION.to_le_bytes());
    result.extend_from_slice(&(tag_size as u32).to_le_bytes());
    result.extend_from_slice(&item_count.to_le_bytes());
    result.extend_from_slice(&APE_FLAG_HEADER_PRESENT.to_le_bytes());
    result.extend_from_slice(&[0u8; 8]);

    result
}

/// Remove existing APE tag from file data, returning the audio data portion
fn remove_ape_tag(data: &[u8]) -> Vec<u8> {
    let footer_start = match find_ape_footer(data) {
        Some(pos) => pos,
        None => return data.to_vec(),
    };

    let tag_size = read_u32_le(&data[footer_start + 12..]) as usize;
    let flags = read_u32_le(&data[footer_start + 20..]);
    let has_header = (flags & APE_FLAG_HEADER_PRESENT) != 0;
    let header_size = if has_header { 32 } else { 0 };

    let audio_end = if footer_start + 32 >= tag_size + header_size {
        footer_start + 32 - tag_size - header_size
    } else {
        0
    };

    let id3v1_start = footer_start + 32;
    let has_id3v1 = data.len() > id3v1_start + 3 && &data[id3v1_start..id3v1_start + 3] == b"TAG";

    if has_id3v1 {
        let mut result = data[..audio_end].to_vec();
        result.extend_from_slice(&data[id3v1_start..]);
        result
    } else {
        data[..audio_end].to_vec()
    }
}

/// Replace (or remove, when `tag` is empty) the APEv2 tag in file data,
/// keeping a trailing ID3v1 tag after the APE tag.
pub(crate) fn replace_ape_tag(data: &[u8], tag: &ApeTag) -> Vec<u8> {
    let mut audio_data = remove_ape_tag(data);

    let has_id3v1 = audio_data.len() >= 128
        && &audio_data[audio_data.len() - 128..audio_data.len() - 125] == b"TAG";

    let tag_data = serialize_ape_tag(tag);

    if has_id3v1 {
        let id3v1 = audio_data[audio_data.len() - 128..].to_vec();
        audio_data.truncate(audio_data.len() - 128);
        audio_data.extend_from_slice(&tag_data);
        audio_data.extend_from_slice(&id3v1);
    } else {
        audio_data.extend_from_slice(&tag_data);
    }

    audio_data
}

/// Write APEv2 tag to file
pub fn write_ape_tag(file_path: &Path, tag: &ApeTag) -> Result<()> {
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    let new_data = replace_ape_tag(&data, tag);
    fs::write(file_path, &new_data).map_err(|e| Error::io_write(file_path, e))?;
    Ok(())
}

/// ReplayGain analysis values written into an APEv2 tag (issue #204).
///
/// The mp3gain-compatible default mode records these alongside the
/// `MP3GAIN_UNDO` / `MP3GAIN_MINMAX` items the apply step already writes,
/// so APEv2 files carry the same `REPLAYGAIN_*` metadata as `mp3gain` and
/// as mp3rgain's ID3v2 (`-s i`) and AAC paths.
#[derive(Debug, Clone, Default)]
pub struct ApeReplayGain {
    pub track_gain: Option<String>,
    pub track_peak: Option<String>,
    pub album_gain: Option<String>,
    pub album_peak: Option<String>,
}

/// Add (or replace) the `REPLAYGAIN_*` items in `file_path`'s APEv2 tag.
///
/// Existing items written by the gain-apply step (`MP3GAIN_UNDO`,
/// `MP3GAIN_MINMAX`) are preserved — only the four ReplayGain keys present
/// in `rg` are set.
pub fn write_ape_replaygain(file_path: &Path, rg: &ApeReplayGain) -> Result<()> {
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    let mut tag = read_ape_tag(&data).unwrap_or_default();

    let fields: [(&str, &Option<String>); 4] = [
        (TAG_REPLAYGAIN_TRACK_GAIN, &rg.track_gain),
        (TAG_REPLAYGAIN_TRACK_PEAK, &rg.track_peak),
        (TAG_REPLAYGAIN_ALBUM_GAIN, &rg.album_gain),
        (TAG_REPLAYGAIN_ALBUM_PEAK, &rg.album_peak),
    ];
    for (key, value) in fields {
        if let Some(v) = value {
            tag.set(key, v);
        }
    }

    let new_data = replace_ape_tag(&data, &tag);
    fs::write(file_path, &new_data).map_err(|e| Error::io_write(file_path, e))?;
    Ok(())
}

/// Delete APEv2 tag from file
pub fn delete_ape_tag(file_path: &Path) -> Result<()> {
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;

    let audio_data = remove_ape_tag(&data);

    fs::write(file_path, &audio_data).map_err(|e| Error::io_write(file_path, e))?;

    Ok(())
}

/// Format MP3GAIN_UNDO tag value: `+LLL,+RRR,W|N`.
pub fn format_undo_value(left_gain: i32, right_gain: i32, wrap: bool) -> String {
    let wrap_flag = if wrap { "W" } else { "N" };
    format!("{:+04},{:+04},{}", left_gain, right_gain, wrap_flag)
}

/// Parse the wrap flag (third field, `W`/`N`) of an MP3GAIN_UNDO tag value.
pub fn parse_undo_wrap(undo_str: Option<&str>) -> bool {
    undo_str
        .and_then(|v| v.split(',').nth(2))
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("W"))
}

/// Parse MP3GAIN_UNDO tag value into (left_gain, right_gain)
pub fn parse_undo_values(undo_str: Option<&str>) -> (i32, i32) {
    match undo_str {
        Some(v) => {
            let parts: Vec<&str> = v.split(',').collect();
            let left = parts
                .first()
                .and_then(|s| s.trim().parse::<i32>().ok())
                .unwrap_or(0);
            let right = parts
                .get(1)
                .and_then(|s| s.trim().parse::<i32>().ok())
                .unwrap_or(left);
            (left, right)
        }
        None => (0, 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp(name: &str, data: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("mp3rgain_ape_tail_tests");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(name);
        fs::File::create(&path).unwrap().write_all(data).unwrap();
        path
    }

    fn sample_tag(value: &str) -> ApeTag {
        let mut tag = ApeTag::new();
        tag.set(TAG_MP3GAIN_UNDO, "+002,+002,N");
        tag.set("COMMENT", value);
        tag
    }

    /// The tail read must see exactly what the old full-file read saw,
    /// for a tag at EOF on a file larger than the 160-byte probe.
    #[test]
    fn tail_read_matches_full_read() {
        let tag = sample_tag("hello");
        let data = replace_ape_tag(&vec![0u8; 100_000], &tag);
        let path = write_temp("plain.mp3", &data);

        assert_eq!(read_ape_tag(&data), Some(tag.clone()));
        assert_eq!(read_ape_tag_from_file(&path).unwrap(), Some(tag));
    }

    /// APE footer at EOF-160 with a trailing ID3v1 tag.
    #[test]
    fn tail_read_with_trailing_id3v1() {
        let tag = sample_tag("id3v1 case");
        let mut audio = vec![0u8; 50_000];
        audio.extend_from_slice(b"TAG");
        audio.extend_from_slice(&[0u8; 125]);
        let data = replace_ape_tag(&audio, &tag);
        let path = write_temp("id3v1.mp3", &data);

        assert_eq!(read_ape_tag_from_file(&path).unwrap(), Some(tag));
    }

    /// A tag larger than the initial 160-byte probe must trigger the second,
    /// wider tail read.
    #[test]
    fn tail_read_tag_larger_than_probe() {
        let tag = sample_tag(&"x".repeat(4096));
        let data = replace_ape_tag(&vec![0u8; 100_000], &tag);
        let path = write_temp("large.mp3", &data);

        assert_eq!(read_ape_tag_from_file(&path).unwrap(), Some(tag));
    }

    #[test]
    fn tail_read_no_tag_returns_none() {
        let path = write_temp("untagged.mp3", &vec![0u8; 10_000]);
        assert_eq!(read_ape_tag_from_file(&path).unwrap(), None);

        let tiny = write_temp("tiny.mp3", &[0u8; 10]);
        assert_eq!(read_ape_tag_from_file(&tiny).unwrap(), None);
    }

    /// Issue #204: write_ape_replaygain must add the REPLAYGAIN_* items while
    /// preserving the MP3GAIN_UNDO/MINMAX items the apply step already wrote.
    #[test]
    fn write_ape_replaygain_preserves_existing_items() {
        let mut tag = ApeTag::new();
        tag.set_undo_gain(2, 2, false);
        tag.set_minmax(100, 200);
        let data = replace_ape_tag(&vec![0u8; 20_000], &tag);
        let path = write_temp("rg_preserve.mp3", &data);

        let rg = ApeReplayGain {
            track_gain: Some("+1.50 dB".to_string()),
            track_peak: Some("0.250000".to_string()),
            album_gain: None,
            album_peak: None,
        };
        write_ape_replaygain(&path, &rg).unwrap();

        let out = read_ape_tag_from_file(&path).unwrap().unwrap();
        assert_eq!(out.get(TAG_MP3GAIN_UNDO), Some("+002,+002,N"));
        assert_eq!(out.get(TAG_MP3GAIN_MINMAX), Some("100,200"));
        assert_eq!(out.get(TAG_REPLAYGAIN_TRACK_GAIN), Some("+1.50 dB"));
        assert_eq!(out.get(TAG_REPLAYGAIN_TRACK_PEAK), Some("0.250000"));
        // Album fields were None, so they must not be written.
        assert_eq!(out.get(TAG_REPLAYGAIN_ALBUM_GAIN), None);
    }
}
