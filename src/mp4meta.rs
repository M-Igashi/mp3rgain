//! MP4/M4A metadata handling for ReplayGain tags
//!
//! This module provides reading and writing of iTunes-style freeform metadata
//! in MP4/M4A files, specifically for ReplayGain tags.
//!
//! MP4 file structure:
//! ```text
//! ftyp (file type)
//! moov (movie/metadata container)
//!   ├── mvhd (movie header)
//!   ├── trak (track)
//!   └── udta (user data)
//!       └── meta (metadata)
//!           ├── hdlr (handler)
//!           └── ilst (iTunes metadata list)
//!               └── ---- (freeform tags for ReplayGain)
//! mdat (media data)
//! ```
//!
//! Related: #118 (m4a write-path round-trip — ffmpeg decode errors and
//! atom-rewriter pitfalls noted in the discussion thread).

use crate::error::{Error, Result};
use std::fs;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

// Per-process counter for atomic_write temp filenames. Without this, parallel
// callers writing to MP4 files in the same parent directory would collide on
// `.mp3rgain_temp_{pid}.m4a`.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// ReplayGain tag keys (iTunes freeform format)
pub const RG_TRACK_GAIN: &str = "replaygain_track_gain";
pub const RG_TRACK_PEAK: &str = "replaygain_track_peak";
pub const RG_ALBUM_GAIN: &str = "replaygain_album_gain";
pub const RG_ALBUM_PEAK: &str = "replaygain_album_peak";

/// Undo tag keys (iTunes freeform format, same namespace)
pub const UNDO_TAG: &str = "mp3rgain_undo";
pub const MINMAX_TAG: &str = "mp3rgain_minmax";

/// iTunes namespace for freeform tags
const ITUNES_NAMESPACE: &str = "com.apple.iTunes";

/// MP4 box/atom types
#[allow(dead_code)]
const FTYP: u32 = u32::from_be_bytes(*b"ftyp");
pub(crate) const MOOV: u32 = u32::from_be_bytes(*b"moov");
const UDTA: u32 = u32::from_be_bytes(*b"udta");
const META: u32 = u32::from_be_bytes(*b"meta");
const ILST: u32 = u32::from_be_bytes(*b"ilst");
#[allow(dead_code)]
const FREE: u32 = u32::from_be_bytes(*b"free");
#[allow(dead_code)]
pub(crate) const MDAT: u32 = u32::from_be_bytes(*b"mdat");
#[allow(dead_code)]
const HDLR: u32 = u32::from_be_bytes(*b"hdlr");
const FREEFORM: u32 = u32::from_be_bytes(*b"----");
const MEAN: u32 = u32::from_be_bytes(*b"mean");
const NAME: u32 = u32::from_be_bytes(*b"name");
const DATA: u32 = u32::from_be_bytes(*b"data");

/// MP4 box header
#[derive(Debug, Clone)]
pub(crate) struct BoxHeader {
    pub(crate) size: u64,
    pub(crate) box_type: u32,
    pub(crate) header_size: u8, // 8 for normal, 16 for extended size
}

impl BoxHeader {
    pub(crate) fn read<R: Read>(reader: &mut R) -> std::io::Result<Option<Self>> {
        let mut buf = [0u8; 8];
        match reader.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }

        let size = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let box_type = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]);

        let (size, header_size) = if size == 1 {
            // Extended size
            let mut ext_buf = [0u8; 8];
            reader.read_exact(&mut ext_buf)?;
            (u64::from_be_bytes(ext_buf), 16)
        } else if size == 0 {
            // Box extends to end of file - we'll handle this specially
            (0, 8)
        } else {
            (size as u64, 8)
        };

        // A box smaller than its own header is malformed; stop parsing here
        // rather than letting callers compute negative content sizes or
        // inverted slice ranges.
        if size != 0 && size < header_size as u64 {
            return Ok(None);
        }

        Ok(Some(BoxHeader {
            size,
            box_type,
            header_size,
        }))
    }

    pub(crate) fn content_size(&self) -> u64 {
        if self.size == 0 {
            0 // Unknown/extends to EOF
        } else {
            self.size - self.header_size as u64
        }
    }

    #[allow(dead_code)]
    fn type_str(&self) -> String {
        String::from_utf8_lossy(&self.box_type.to_be_bytes()).to_string()
    }
}

/// Freeform tag (---- box) for ReplayGain
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FreeformTag {
    namespace: String,
    name: String,
    value: String,
}

impl FreeformTag {
    pub(crate) fn new(namespace: String, name: String, value: String) -> Self {
        Self {
            namespace,
            name,
            value,
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn value(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Display for FreeformTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}={}", self.namespace(), self.name(), self.value())
    }
}

/// Collection of ReplayGain tags
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReplayGainTags {
    track_gain: Option<String>,
    track_peak: Option<String>,
    album_gain: Option<String>,
    album_peak: Option<String>,
}

impl ReplayGainTags {
    pub fn track_gain(&self) -> Option<&str> {
        self.track_gain.as_deref()
    }
    pub fn track_peak(&self) -> Option<&str> {
        self.track_peak.as_deref()
    }
    pub fn album_gain(&self) -> Option<&str> {
        self.album_gain.as_deref()
    }
    pub fn album_peak(&self) -> Option<&str> {
        self.album_peak.as_deref()
    }

    pub fn set_track(&mut self, gain_db: f64, peak: f64) {
        self.track_gain = Some(format!("{:+.2} dB", gain_db));
        self.track_peak = Some(format!("{:.6}", peak));
    }

    pub fn set_album(&mut self, gain_db: f64, peak: f64) {
        self.album_gain = Some(format!("{:+.2} dB", gain_db));
        self.album_peak = Some(format!("{:.6}", peak));
    }

    pub fn is_empty(&self) -> bool {
        self.track_gain.is_none()
            && self.track_peak.is_none()
            && self.album_gain.is_none()
            && self.album_peak.is_none()
    }
}

impl std::fmt::Display for ReplayGainTags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut parts = Vec::new();
        if let Some(ref g) = self.track_gain {
            parts.push(format!("Track: {}", g));
        }
        if let Some(ref p) = self.track_peak {
            parts.push(format!("Peak: {}", p));
        }
        if let Some(ref g) = self.album_gain {
            parts.push(format!("Album: {}", g));
        }
        if let Some(ref p) = self.album_peak {
            parts.push(format!("Album Peak: {}", p));
        }
        if parts.is_empty() {
            f.write_str("(no tags)")
        } else {
            write!(f, "{}", parts.join(", "))
        }
    }
}

impl ReplayGainTags {
    fn to_freeform_tags(&self) -> Vec<FreeformTag> {
        let entries: [(&str, &Option<String>); 4] = [
            (RG_TRACK_GAIN, &self.track_gain),
            (RG_TRACK_PEAK, &self.track_peak),
            (RG_ALBUM_GAIN, &self.album_gain),
            (RG_ALBUM_PEAK, &self.album_peak),
        ];

        entries
            .into_iter()
            .filter_map(|(name, value)| {
                value.as_ref().map(|v| {
                    FreeformTag::new(ITUNES_NAMESPACE.to_string(), name.to_string(), v.clone())
                })
            })
            .collect()
    }
}

/// Undo information stored as iTunes freeform tags
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct UndoTags {
    /// Cumulative gain adjustment, format: "+003,+003,N" (left,right,wrap_flag)
    undo: Option<String>,
    /// Original min/max global_gain before any modification, format: "80,120"
    minmax: Option<String>,
}

impl UndoTags {
    #[allow(dead_code)]
    pub(crate) fn new(undo: Option<String>, minmax: Option<String>) -> Self {
        Self { undo, minmax }
    }

    pub fn undo(&self) -> Option<&str> {
        self.undo.as_deref()
    }
    pub fn minmax(&self) -> Option<&str> {
        self.minmax.as_deref()
    }

    pub fn is_empty(&self) -> bool {
        self.undo.is_none() && self.minmax.is_none()
    }

    fn to_freeform_tags(&self) -> Vec<FreeformTag> {
        let entries: [(&str, &Option<String>); 2] =
            [(UNDO_TAG, &self.undo), (MINMAX_TAG, &self.minmax)];

        entries
            .into_iter()
            .filter_map(|(name, value)| {
                value.as_ref().map(|v| {
                    FreeformTag::new(ITUNES_NAMESPACE.to_string(), name.to_string(), v.clone())
                })
            })
            .collect()
    }
}

/// Find box position in data
pub(crate) fn find_box(data: &[u8], box_type: u32) -> Option<(usize, BoxHeader)> {
    let mut cursor = Cursor::new(data);

    while let Ok(Some(header)) = BoxHeader::read(&mut cursor) {
        let pos = cursor.position() as usize - header.header_size as usize;

        if header.box_type == box_type {
            return Some((pos, header));
        }

        // Skip to next box
        if header.size == 0 {
            break; // Extends to EOF
        }

        let next_pos = pos as u64 + header.size;
        if next_pos >= data.len() as u64 {
            break;
        }
        cursor.set_position(next_pos);
    }

    None
}

/// Find box within a container (searches inside the container's content)
pub(crate) fn find_box_in_container(
    data: &[u8],
    container_start: usize,
    container_size: usize,
    box_type: u32,
) -> Option<(usize, BoxHeader)> {
    // Clamp to the actual data length: box sizes come from the file and a
    // malformed/truncated MP4 can declare a container that overruns the
    // buffer, which would panic on the slice below.
    let container_end = container_start
        .saturating_add(container_size)
        .min(data.len());
    let mut pos = container_start;

    while pos + 8 <= container_end {
        let mut cursor = Cursor::new(&data[pos..]);
        if let Ok(Some(header)) = BoxHeader::read(&mut cursor) {
            if header.box_type == box_type {
                return Some((pos, header));
            }

            if header.size == 0 {
                break;
            }

            pos = pos.saturating_add(header.size as usize);
        } else {
            break;
        }
    }

    None
}

/// Parse freeform tag from data
fn parse_freeform_tag(data: &[u8]) -> Option<FreeformTag> {
    let mut cursor = Cursor::new(data);
    let mut namespace = None;
    let mut name = None;
    let mut value = None;

    while let Ok(Some(header)) = BoxHeader::read(&mut cursor) {
        let content_start = cursor.position() as usize;
        let content_size = header.content_size() as usize;

        // Bounds check: ensure we don't read past the end of data
        let content_end = match content_start.checked_add(content_size) {
            Some(end) if end <= data.len() => end,
            _ => break,
        };

        match header.box_type {
            MEAN => {
                // Skip 4-byte version/flags
                let string_start = content_start.saturating_add(4);
                if string_start < content_end {
                    namespace =
                        Some(String::from_utf8_lossy(&data[string_start..content_end]).to_string());
                }
            }
            NAME => {
                // Skip 4-byte version/flags
                let string_start = content_start.saturating_add(4);
                if string_start < content_end {
                    name =
                        Some(String::from_utf8_lossy(&data[string_start..content_end]).to_string());
                }
            }
            DATA => {
                // Skip 8-byte version/flags + type indicator
                let string_start = content_start.saturating_add(8);
                if string_start < content_end {
                    value =
                        Some(String::from_utf8_lossy(&data[string_start..content_end]).to_string());
                }
            }
            _ => {}
        }

        cursor.set_position(content_end as u64);
    }

    match (namespace, name, value) {
        (Some(ns), Some(n), Some(v)) => Some(FreeformTag::new(ns, n, v)),
        _ => None,
    }
}

/// Serialize freeform tag to bytes
fn serialize_freeform_tag(tag: &FreeformTag) -> Vec<u8> {
    let mut result = Vec::new();

    // mean box
    let mean_data = tag.namespace().as_bytes();
    let mean_size = 12 + mean_data.len() as u32; // 8 header + 4 version/flags + data
    result.extend_from_slice(&mean_size.to_be_bytes());
    result.extend_from_slice(b"mean");
    result.extend_from_slice(&[0u8; 4]); // version/flags
    result.extend_from_slice(mean_data);

    // name box
    let name_data = tag.name().as_bytes();
    let name_size = 12 + name_data.len() as u32;
    result.extend_from_slice(&name_size.to_be_bytes());
    result.extend_from_slice(b"name");
    result.extend_from_slice(&[0u8; 4]); // version/flags
    result.extend_from_slice(name_data);

    // data box
    let value_data = tag.value().as_bytes();
    let data_size = 16 + value_data.len() as u32; // 8 header + 4 version/flags + 4 type + data
    result.extend_from_slice(&data_size.to_be_bytes());
    result.extend_from_slice(b"data");
    result.extend_from_slice(&[0u8; 4]); // version/flags
    result.extend_from_slice(&1u32.to_be_bytes()); // type = 1 (UTF-8 text)
    result.extend_from_slice(value_data);

    // Wrap in ---- box
    let freeform_size = 8 + result.len() as u32;
    let mut freeform = Vec::with_capacity(freeform_size as usize);
    freeform.extend_from_slice(&freeform_size.to_be_bytes());
    freeform.extend_from_slice(b"----");
    freeform.extend_from_slice(&result);

    freeform
}

/// Read all iTunes freeform tags from an MP4/M4A file.
/// Navigates moov -> udta -> meta -> ilst and collects tags with the iTunes namespace.
fn read_itunes_freeform_tags(file_path: &Path) -> Result<Vec<FreeformTag>> {
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    Ok(read_itunes_freeform_tags_from_data(&data))
}

/// Slice-based variant of `read_itunes_freeform_tags`.
/// Returns an empty vec for any non-fatal parse failure (missing moov / udta / meta / ilst).
pub(crate) fn read_itunes_freeform_tags_from_data(data: &[u8]) -> Vec<FreeformTag> {
    let (moov_pos, moov_header) = match find_box(data, MOOV) {
        Some(x) => x,
        None => return Vec::new(),
    };

    let moov_content_start = moov_pos + moov_header.header_size as usize;
    let moov_content_size = moov_header.content_size() as usize;

    let (udta_pos, udta_header) =
        match find_box_in_container(data, moov_content_start, moov_content_size, UDTA) {
            Some(x) => x,
            None => return Vec::new(),
        };

    let udta_content_start = udta_pos + udta_header.header_size as usize;
    let udta_content_size = udta_header.content_size() as usize;

    let (meta_pos, meta_header) =
        match find_box_in_container(data, udta_content_start, udta_content_size, META) {
            Some(x) => x,
            None => return Vec::new(),
        };

    let meta_content_start = meta_pos + meta_header.header_size as usize + 4;
    let meta_content_size = (meta_header.content_size() as usize).saturating_sub(4);

    let (ilst_pos, ilst_header) =
        match find_box_in_container(data, meta_content_start, meta_content_size, ILST) {
            Some(x) => x,
            None => return Vec::new(),
        };

    let ilst_content_start = ilst_pos + ilst_header.header_size as usize;
    let ilst_content_size = ilst_header.content_size() as usize;

    let mut tags = Vec::new();
    // Clamp to the buffer: a malformed ilst can declare a size past EOF.
    let ilst_end = ilst_content_start
        .saturating_add(ilst_content_size)
        .min(data.len());
    let mut pos = ilst_content_start;
    while pos + 8 <= ilst_end {
        let mut cursor = Cursor::new(&data[pos..]);
        if let Ok(Some(header)) = BoxHeader::read(&mut cursor) {
            if header.size == 0 || pos + header.size as usize > ilst_end {
                break;
            }

            if header.box_type == FREEFORM {
                let tag_data = &data[pos + header.header_size as usize..pos + header.size as usize];
                if let Some(tag) = parse_freeform_tag(tag_data) {
                    if tag.namespace() == ITUNES_NAMESPACE {
                        tags.push(tag);
                    }
                }
            }

            pos += header.size as usize;
        } else {
            break;
        }
    }

    tags
}

/// Read ReplayGain tags from MP4/M4A file
pub fn read_replaygain_tags(file_path: &Path) -> Result<ReplayGainTags> {
    let freeform_tags = read_itunes_freeform_tags(file_path)?;
    let mut tags = ReplayGainTags::default();

    for tag in &freeform_tags {
        match tag.name() {
            x if x.eq_ignore_ascii_case(RG_TRACK_GAIN) => {
                tags.track_gain = Some(tag.value().to_string());
            }
            x if x.eq_ignore_ascii_case(RG_TRACK_PEAK) => {
                tags.track_peak = Some(tag.value().to_string());
            }
            x if x.eq_ignore_ascii_case(RG_ALBUM_GAIN) => {
                tags.album_gain = Some(tag.value().to_string());
            }
            x if x.eq_ignore_ascii_case(RG_ALBUM_PEAK) => {
                tags.album_peak = Some(tag.value().to_string());
            }
            _ => {}
        }
    }

    Ok(tags)
}

/// Read undo tags from MP4/M4A file
pub fn read_undo_tags(file_path: &Path) -> Result<UndoTags> {
    let freeform_tags = read_itunes_freeform_tags(file_path)?;
    Ok(undo_tags_from_freeform(&freeform_tags))
}

/// Slice-based variant of `read_undo_tags`.
pub(crate) fn read_undo_tags_from_data(data: &[u8]) -> UndoTags {
    let freeform_tags = read_itunes_freeform_tags_from_data(data);
    undo_tags_from_freeform(&freeform_tags)
}

fn undo_tags_from_freeform(freeform_tags: &[FreeformTag]) -> UndoTags {
    let mut tags = UndoTags::default();
    for tag in freeform_tags {
        match tag.name() {
            x if x.eq_ignore_ascii_case(UNDO_TAG) => {
                tags.undo = Some(tag.value().to_string());
            }
            x if x.eq_ignore_ascii_case(MINMAX_TAG) => {
                tags.minmax = Some(tag.value().to_string());
            }
            _ => {}
        }
    }
    tags
}

/// Write undo tags to MP4/M4A file.
/// Uses atomic write (temp file + rename) to prevent corruption on interruption.
pub fn write_undo_tags(file_path: &Path, tags: &UndoTags) -> Result<()> {
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    let new_data = update_mp4_undo_metadata(&data, tags)?;
    atomic_write(file_path, &new_data)
}

/// Delete undo tags from MP4/M4A file
pub fn delete_undo_tags(file_path: &Path) -> Result<()> {
    write_undo_tags(file_path, &UndoTags::default())
}

/// Write ReplayGain tags to MP4/M4A file.
/// Uses atomic write (temp file + rename) to prevent corruption on interruption.
pub fn write_replaygain_tags(file_path: &Path, tags: &ReplayGainTags) -> Result<()> {
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    let new_data = update_mp4_metadata(&data, tags)?;
    atomic_write(file_path, &new_data)
}

/// Atomic write: write to a temp file then rename over the original.
/// Falls back to direct write if rename fails (e.g., cross-filesystem).
pub(crate) fn atomic_write(file_path: &Path, data: &[u8]) -> Result<()> {
    let parent = file_path.parent().unwrap_or(Path::new("."));
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path = parent.join(format!(
        ".mp3rgain_temp_{}_{}.m4a",
        std::process::id(),
        counter
    ));

    if let Err(e) = fs::write(&temp_path, data) {
        let _ = fs::remove_file(&temp_path);
        return Err(Error::io_write(&temp_path, e));
    }

    if let Err(_rename_err) = fs::rename(&temp_path, file_path) {
        let _ = fs::remove_file(&temp_path);
        fs::write(file_path, data).map_err(|e| Error::io_write(file_path, e))?;
    }

    Ok(())
}

/// Update MP4 metadata with new ReplayGain tags
fn update_mp4_metadata(data: &[u8], tags: &ReplayGainTags) -> Result<Vec<u8>> {
    let make_ilst = |existing: &[u8]| create_ilst_box(tags, existing);
    rebuild_mp4_with_ilst(data, make_ilst)
}

/// Update MP4 metadata with new undo tags
pub(crate) fn update_mp4_undo_metadata(data: &[u8], tags: &UndoTags) -> Result<Vec<u8>> {
    let make_ilst = |existing: &[u8]| create_ilst_box_undo(tags, existing);
    rebuild_mp4_with_ilst(data, make_ilst)
}

/// Common logic for rebuilding MP4 file with updated ilst content.
/// `make_ilst` takes existing ilst content (or empty) and returns the new ilst box.
fn rebuild_mp4_with_ilst(data: &[u8], make_ilst: impl Fn(&[u8]) -> Vec<u8>) -> Result<Vec<u8>> {
    // Find moov box
    let (moov_pos, moov_header) = find_box(data, MOOV).ok_or(Error::NoMoovBox)?;

    let moov_content_start = moov_pos + moov_header.header_size as usize;
    let moov_content_size = moov_header.content_size() as usize;
    let moov_end = moov_pos + moov_header.size as usize;

    // Try to find existing ilst or create new metadata structure
    let (new_ilst, ilst_info) =
        find_ilst_location(data, moov_content_start, moov_content_size, &make_ilst)?;

    // Rebuild the file
    let mut result = Vec::with_capacity(data.len() + 1024);

    match ilst_info {
        IlstLocation::Existing {
            ilst_pos,
            ilst_size,
            meta_pos,
            udta_pos,
        } => {
            let new_ilst_is_empty = new_ilst.len() <= 8; // header-only = no tags

            if new_ilst_is_empty {
                // Determine what to remove: ilst, or meta, or udta
                let meta_size = read_box_size(data, meta_pos);
                let udta_size = read_box_size(data, udta_pos);

                // meta content = version/flags(4) + hdlr + ilst; if removing ilst
                // leaves only hdlr, the meta is effectively empty for our purposes.
                let meta_content_without_ilst = meta_size - ilst_size;
                let hdlr_size = create_hdlr_box().len();
                let meta_only_has_hdlr_and_ilst = meta_content_without_ilst == 8 + 4 + hdlr_size; // header + ver/flags + hdlr

                if meta_only_has_hdlr_and_ilst && udta_size == meta_size + 8 {
                    // udta contains only meta, and meta contains only hdlr+ilst
                    // Remove entire udta
                    let size_diff = -(udta_size as i64);
                    result.extend_from_slice(&data[..udta_pos]);
                    result.extend_from_slice(&data[udta_pos + udta_size..]);
                    update_box_size(&mut result, moov_pos, size_diff);
                } else if meta_only_has_hdlr_and_ilst {
                    // meta contains only hdlr+ilst but udta has other boxes too
                    // Remove entire meta
                    let size_diff = -(meta_size as i64);
                    result.extend_from_slice(&data[..meta_pos]);
                    result.extend_from_slice(&data[meta_pos + meta_size..]);
                    update_box_size(&mut result, moov_pos, size_diff);
                    update_box_size(&mut result, udta_pos, size_diff);
                } else {
                    // meta has other boxes besides hdlr+ilst; just remove ilst
                    let size_diff = -(ilst_size as i64);
                    result.extend_from_slice(&data[..ilst_pos]);
                    result.extend_from_slice(&data[ilst_pos + ilst_size..]);
                    update_box_size(&mut result, moov_pos, size_diff);
                    update_box_size(&mut result, udta_pos, size_diff);
                    update_box_size(&mut result, meta_pos, size_diff);
                }
            } else {
                // Normal case: replace ilst with new content
                let old_ilst_size = ilst_size;
                let new_ilst_size = new_ilst.len();
                let size_diff = new_ilst_size as i64 - old_ilst_size as i64;

                result.extend_from_slice(&data[..ilst_pos]);
                result.extend_from_slice(&new_ilst);
                result.extend_from_slice(&data[ilst_pos + old_ilst_size..]);

                update_box_size(&mut result, moov_pos, size_diff);
                update_box_size(&mut result, udta_pos, size_diff);
                update_box_size(&mut result, meta_pos, size_diff);
            }
        }
        IlstLocation::NeedsIlst {
            meta_pos,
            meta_size,
            udta_pos,
        } => {
            // meta exists but has no ilst — append ilst at end of existing meta
            let meta_end = meta_pos + meta_size;
            let size_diff = new_ilst.len() as i64;

            result.extend_from_slice(&data[..meta_end]);
            result.extend_from_slice(&new_ilst);
            result.extend_from_slice(&data[meta_end..]);

            update_box_size(&mut result, moov_pos, size_diff);
            update_box_size(&mut result, udta_pos, size_diff);
            update_box_size(&mut result, meta_pos, size_diff);
        }
        IlstLocation::NeedsMeta {
            udta_pos,
            udta_size,
        } => {
            // Need to create meta + ilst inside udta
            let meta_box = create_meta_box(&new_ilst);
            let size_diff = meta_box.len() as i64;

            let udta_end = udta_pos + udta_size;

            // Write data before udta end
            result.extend_from_slice(&data[..udta_end]);

            // Insert meta box at end of udta
            result.extend_from_slice(&meta_box);

            // Write data after udta
            result.extend_from_slice(&data[udta_end..]);

            // Update sizes
            update_box_size(&mut result, moov_pos, size_diff);
            update_box_size(&mut result, udta_pos, size_diff);
        }
        IlstLocation::NeedsUdta => {
            // Need to create udta + meta + ilst at end of moov
            let meta_box = create_meta_box(&new_ilst);
            let udta_box = create_udta_box(&meta_box);
            let size_diff = udta_box.len() as i64;

            // Write data before moov end
            result.extend_from_slice(&data[..moov_end]);

            // Insert udta box at end of moov
            result.extend_from_slice(&udta_box);

            // Write data after moov
            result.extend_from_slice(&data[moov_end..]);

            // Update moov size
            update_box_size(&mut result, moov_pos, size_diff);
        }
    }

    // Update stco/co64 chunk offsets if the file size changed.
    // Any chunk offset pointing beyond the original moov end needs adjustment,
    // regardless of moov/mdat ordering or multiple mdat boxes.
    let size_diff = result.len() as i64 - data.len() as i64;
    if size_diff != 0 {
        update_chunk_offsets(&mut result, moov_pos, moov_end, size_diff)?;
    }

    Ok(result)
}

#[derive(Debug)]
enum IlstLocation {
    Existing {
        ilst_pos: usize,
        ilst_size: usize,
        meta_pos: usize,
        udta_pos: usize,
    },
    NeedsIlst {
        meta_pos: usize,
        meta_size: usize,
        udta_pos: usize,
    },
    NeedsMeta {
        udta_pos: usize,
        udta_size: usize,
    },
    NeedsUdta,
}

fn find_ilst_location(
    data: &[u8],
    moov_content_start: usize,
    moov_content_size: usize,
    make_ilst: &impl Fn(&[u8]) -> Vec<u8>,
) -> Result<(Vec<u8>, IlstLocation)> {
    // Find udta
    let (udta_pos, udta_header) =
        match find_box_in_container(data, moov_content_start, moov_content_size, UDTA) {
            Some(x) => x,
            None => {
                let ilst = make_ilst(&[]);
                return Ok((ilst, IlstLocation::NeedsUdta));
            }
        };

    let udta_content_start = udta_pos + udta_header.header_size as usize;
    let udta_content_size = udta_header.content_size() as usize;

    // Find meta
    let (meta_pos, meta_header) =
        match find_box_in_container(data, udta_content_start, udta_content_size, META) {
            Some(x) => x,
            None => {
                let ilst = make_ilst(&[]);
                return Ok((
                    ilst,
                    IlstLocation::NeedsMeta {
                        udta_pos,
                        udta_size: udta_header.size as usize,
                    },
                ));
            }
        };

    let meta_content_start = meta_pos + meta_header.header_size as usize + 4; // +4 for version/flags
    let meta_content_size = (meta_header.content_size() as usize).saturating_sub(4);

    // Find ilst
    let (ilst_pos, ilst_header) =
        match find_box_in_container(data, meta_content_start, meta_content_size, ILST) {
            Some(x) => x,
            None => {
                let ilst = make_ilst(&[]);
                return Ok((
                    ilst,
                    IlstLocation::NeedsIlst {
                        meta_pos,
                        meta_size: meta_header.size as usize,
                        udta_pos,
                    },
                ));
            }
        };

    // Parse existing ilst and merge with new tags. Clamp to the buffer so a
    // malformed ilst size cannot slice past EOF.
    let ilst_content_start = (ilst_pos + ilst_header.header_size as usize).min(data.len());
    let ilst_content_end = ilst_content_start
        .saturating_add(ilst_header.content_size() as usize)
        .min(data.len());
    let existing_content = &data[ilst_content_start..ilst_content_end];

    let new_ilst = make_ilst(existing_content);

    Ok((
        new_ilst,
        IlstLocation::Existing {
            ilst_pos,
            ilst_size: ilst_header.size as usize,
            meta_pos,
            udta_pos,
        },
    ))
}

/// Check if a box at the given position is a ReplayGain freeform tag
fn is_replaygain_freeform(data: &[u8], pos: usize, header: &BoxHeader) -> bool {
    if header.box_type != FREEFORM {
        return false;
    }
    let inner_data = &data[pos + header.header_size as usize..pos + header.size as usize];
    parse_freeform_tag(inner_data).is_some_and(|tag| {
        tag.namespace() == ITUNES_NAMESPACE
            && (tag.name().eq_ignore_ascii_case(RG_TRACK_GAIN)
                || tag.name().eq_ignore_ascii_case(RG_TRACK_PEAK)
                || tag.name().eq_ignore_ascii_case(RG_ALBUM_GAIN)
                || tag.name().eq_ignore_ascii_case(RG_ALBUM_PEAK))
    })
}

/// Check if a box at the given position is an undo freeform tag
fn is_undo_freeform(data: &[u8], pos: usize, header: &BoxHeader) -> bool {
    if header.box_type != FREEFORM {
        return false;
    }
    let inner_data = &data[pos + header.header_size as usize..pos + header.size as usize];
    parse_freeform_tag(inner_data).is_some_and(|tag| {
        tag.namespace() == ITUNES_NAMESPACE
            && (tag.name().eq_ignore_ascii_case(UNDO_TAG)
                || tag.name().eq_ignore_ascii_case(MINMAX_TAG))
    })
}

fn create_ilst_box_filtered(
    new_tags: &[FreeformTag],
    existing_content: &[u8],
    should_replace: impl Fn(&[u8], usize, &BoxHeader) -> bool,
) -> Vec<u8> {
    let mut content = Vec::new();

    // Copy existing tags that don't match the filter
    let mut pos = 0;
    while pos + 8 <= existing_content.len() {
        let mut cursor = Cursor::new(&existing_content[pos..]);
        if let Ok(Some(header)) = BoxHeader::read(&mut cursor) {
            if header.size == 0 || pos + header.size as usize > existing_content.len() {
                break;
            }

            let tag_data = &existing_content[pos..pos + header.size as usize];

            if !should_replace(existing_content, pos, &header) {
                content.extend_from_slice(tag_data);
            }

            pos += header.size as usize;
        } else {
            break;
        }
    }

    // Add new tags
    for tag in new_tags {
        content.extend_from_slice(&serialize_freeform_tag(tag));
    }

    // Wrap in ilst box
    let ilst_size = 8 + content.len() as u32;
    let mut ilst = Vec::with_capacity(ilst_size as usize);
    ilst.extend_from_slice(&ilst_size.to_be_bytes());
    ilst.extend_from_slice(b"ilst");
    ilst.extend_from_slice(&content);

    ilst
}

fn create_ilst_box(tags: &ReplayGainTags, existing_content: &[u8]) -> Vec<u8> {
    create_ilst_box_filtered(
        &tags.to_freeform_tags(),
        existing_content,
        is_replaygain_freeform,
    )
}

fn create_ilst_box_undo(tags: &UndoTags, existing_content: &[u8]) -> Vec<u8> {
    create_ilst_box_filtered(&tags.to_freeform_tags(), existing_content, is_undo_freeform)
}

fn create_meta_box(ilst: &[u8]) -> Vec<u8> {
    // meta box structure:
    // - 8 byte header
    // - 4 byte version/flags (0)
    // - hdlr box
    // - ilst box

    let hdlr = create_hdlr_box();
    let content_size = 4 + hdlr.len() + ilst.len();
    let meta_size = 8 + content_size;

    let mut meta = Vec::with_capacity(meta_size);
    meta.extend_from_slice(&(meta_size as u32).to_be_bytes());
    meta.extend_from_slice(b"meta");
    meta.extend_from_slice(&[0u8; 4]); // version/flags
    meta.extend_from_slice(&hdlr);
    meta.extend_from_slice(ilst);

    meta
}

fn create_hdlr_box() -> Vec<u8> {
    // hdlr box for metadata
    let mut hdlr = Vec::new();
    hdlr.extend_from_slice(&[0u8; 4]); // version/flags
    hdlr.extend_from_slice(&[0u8; 4]); // pre_defined
    hdlr.extend_from_slice(b"mdir"); // handler_type
    hdlr.extend_from_slice(b"appl"); // manufacturer
    hdlr.extend_from_slice(&[0u8; 4]); // reserved
    hdlr.extend_from_slice(&[0u8; 4]); // reserved
    hdlr.extend_from_slice(&[0u8]); // name (empty string)

    let hdlr_size = 8 + hdlr.len() as u32;
    let mut result = Vec::with_capacity(hdlr_size as usize);
    result.extend_from_slice(&hdlr_size.to_be_bytes());
    result.extend_from_slice(b"hdlr");
    result.extend_from_slice(&hdlr);

    result
}

fn create_udta_box(content: &[u8]) -> Vec<u8> {
    let udta_size = 8 + content.len() as u32;
    let mut udta = Vec::with_capacity(udta_size as usize);
    udta.extend_from_slice(&udta_size.to_be_bytes());
    udta.extend_from_slice(b"udta");
    udta.extend_from_slice(content);

    udta
}

fn read_box_size(data: &[u8], box_pos: usize) -> usize {
    u32::from_be_bytes([
        data[box_pos],
        data[box_pos + 1],
        data[box_pos + 2],
        data[box_pos + 3],
    ]) as usize
}

fn update_box_size(data: &mut [u8], box_pos: usize, size_diff: i64) {
    if box_pos + 4 > data.len() {
        return;
    }

    let current_size = u32::from_be_bytes([
        data[box_pos],
        data[box_pos + 1],
        data[box_pos + 2],
        data[box_pos + 3],
    ]);

    // Don't update if it's an extended size box (size == 1) or extends to EOF (size == 0)
    if current_size <= 1 {
        return;
    }

    let new_size = (current_size as i64 + size_diff) as u32;
    data[box_pos..box_pos + 4].copy_from_slice(&new_size.to_be_bytes());
}

/// Update stco/co64 chunk offsets after modifying moov size.
/// `original_moov_end` is the end of moov in the original (unmodified) data.
/// Only offsets pointing at or beyond `original_moov_end` are adjusted, so
/// data before moov (e.g., an earlier mdat) is left untouched.
fn update_chunk_offsets(
    data: &mut [u8],
    moov_pos: usize,
    original_moov_end: usize,
    size_diff: i64,
) -> Result<()> {
    // Find moov box again in the modified data
    let (_, moov_header) = match find_box(data, MOOV) {
        Some(x) => x,
        None => return Ok(()),
    };

    let moov_end = moov_pos + moov_header.size as usize;

    // Recursively find and update stco/co64 boxes within moov
    update_offsets_recursive(data, moov_pos + 8, moov_end, size_diff, original_moov_end)?;

    Ok(())
}

pub(crate) const STCO: u32 = u32::from_be_bytes(*b"stco");
pub(crate) const CO64: u32 = u32::from_be_bytes(*b"co64");
pub(crate) const TRAK: u32 = u32::from_be_bytes(*b"trak");
pub(crate) const MDIA: u32 = u32::from_be_bytes(*b"mdia");
pub(crate) const MINF: u32 = u32::from_be_bytes(*b"minf");
pub(crate) const STBL: u32 = u32::from_be_bytes(*b"stbl");

fn update_offsets_recursive(
    data: &mut [u8],
    start: usize,
    end: usize,
    size_diff: i64,
    threshold: usize,
) -> Result<()> {
    let mut pos = start;

    while pos + 8 <= end {
        let size = u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]);
        let box_type =
            u32::from_be_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]]);

        if size == 0 || pos + size as usize > end {
            break;
        }

        match box_type {
            STCO => {
                // Update 32-bit chunk offsets that point beyond the insertion point
                let version_flags_pos = pos + 8;
                let entry_count_pos = version_flags_pos + 4;
                if entry_count_pos + 4 <= data.len() {
                    let entry_count = u32::from_be_bytes([
                        data[entry_count_pos],
                        data[entry_count_pos + 1],
                        data[entry_count_pos + 2],
                        data[entry_count_pos + 3],
                    ]);

                    let mut offset_pos = entry_count_pos + 4;
                    for _ in 0..entry_count {
                        if offset_pos + 4 > data.len() {
                            break;
                        }
                        let offset = u32::from_be_bytes([
                            data[offset_pos],
                            data[offset_pos + 1],
                            data[offset_pos + 2],
                            data[offset_pos + 3],
                        ]);
                        if (offset as usize) >= threshold {
                            let new_offset = (offset as i64 + size_diff) as u32;
                            data[offset_pos..offset_pos + 4]
                                .copy_from_slice(&new_offset.to_be_bytes());
                        }
                        offset_pos += 4;
                    }
                }
            }
            CO64 => {
                // Update 64-bit chunk offsets that point beyond the insertion point
                let version_flags_pos = pos + 8;
                let entry_count_pos = version_flags_pos + 4;
                if entry_count_pos + 4 <= data.len() {
                    let entry_count = u32::from_be_bytes([
                        data[entry_count_pos],
                        data[entry_count_pos + 1],
                        data[entry_count_pos + 2],
                        data[entry_count_pos + 3],
                    ]);

                    let mut offset_pos = entry_count_pos + 4;
                    for _ in 0..entry_count {
                        if offset_pos + 8 > data.len() {
                            break;
                        }
                        let offset = u64::from_be_bytes([
                            data[offset_pos],
                            data[offset_pos + 1],
                            data[offset_pos + 2],
                            data[offset_pos + 3],
                            data[offset_pos + 4],
                            data[offset_pos + 5],
                            data[offset_pos + 6],
                            data[offset_pos + 7],
                        ]);
                        if (offset as usize) >= threshold {
                            let new_offset = (offset as i64 + size_diff) as u64;
                            data[offset_pos..offset_pos + 8]
                                .copy_from_slice(&new_offset.to_be_bytes());
                        }
                        offset_pos += 8;
                    }
                }
            }
            TRAK | MDIA | MINF | STBL | MOOV | UDTA => {
                // Container boxes - recurse into them
                update_offsets_recursive(data, pos + 8, pos + size as usize, size_diff, threshold)?;
            }
            _ => {}
        }

        pos += size as usize;
    }

    Ok(())
}

/// Delete ReplayGain tags from MP4/M4A file
pub fn delete_replaygain_tags(file_path: &Path) -> Result<()> {
    write_replaygain_tags(file_path, &ReplayGainTags::default())
}

/// Check if a 4-byte brand is a recognized MP4/M4A audio brand.
/// Note: M4P (DRM-protected) is intentionally excluded.
fn is_accepted_brand(brand: &[u8]) -> bool {
    matches!(
        brand,
        b"M4A " | b"M4B " | b"M4V " | b"mp41" | b"mp42" | b"isom" | b"iso2"
    )
}

/// Check if file is an MP4/M4A file by reading only the ftyp header.
/// Checks both the major brand and the compatible brands list.
pub fn is_mp4_file(file_path: &Path) -> bool {
    let mut file = match fs::File::open(file_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    // 128 bytes is enough for a typical ftyp box (major brand + ~28 compatible brands)
    let mut buf = [0u8; 128];
    let bytes_read = match file.read(&mut buf) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if bytes_read < 12 {
        return false;
    }
    let size = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if &buf[4..8] != b"ftyp" || size < 12 {
        return false;
    }
    let check_end = size.min(bytes_read);
    // Check major brand at offset 8, then compatible brands at offset 16, 20, 24, ...
    // (offset 12 is the minor_version field, not a brand)
    let mut offset = 8;
    while offset + 4 <= check_end {
        if is_accepted_brand(&buf[offset..offset + 4]) {
            return true;
        }
        offset = if offset == 8 { 16 } else { offset + 4 };
    }
    false
}

pub(crate) const MP4A: u32 = u32::from_be_bytes(*b"mp4a");
const ALAC: u32 = u32::from_be_bytes(*b"alac");
pub(crate) const STSD: u32 = u32::from_be_bytes(*b"stsd");

/// Audio codec detected in an MP4 file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Mp4AudioCodec {
    Aac,
    Alac,
    Unknown,
}

impl std::fmt::Display for Mp4AudioCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mp4AudioCodec::Aac => f.write_str("AAC"),
            Mp4AudioCodec::Alac => f.write_str("ALAC"),
            Mp4AudioCodec::Unknown => f.write_str("Unknown"),
        }
    }
}

/// Read the content of the top-level `moov` box without loading the rest of
/// the file — the audio payload (`mdat`) is usually orders of magnitude
/// larger than the metadata, so codec/track inspection shouldn't pay for a
/// full-file read (issue #188).
fn read_moov_content(file_path: &Path) -> Option<Vec<u8>> {
    let mut file = fs::File::open(file_path).ok()?;
    let file_len = file.metadata().ok()?.len();
    let mut pos = 0u64;

    while pos + 8 <= file_len {
        file.seek(SeekFrom::Start(pos)).ok()?;
        let header = BoxHeader::read(&mut file).ok()??;
        if header.box_type == MOOV {
            // Clamp to the bytes actually present: a malformed size must not
            // drive the allocation or read past EOF.
            let content_start = pos + header.header_size as u64;
            let len = header
                .content_size()
                .min(file_len.saturating_sub(content_start));
            let mut buf = vec![0u8; len as usize];
            file.read_exact(&mut buf).ok()?;
            return Some(buf);
        }
        // size == 0 means "extends to EOF"; advancing by 0 would loop forever.
        if header.size == 0 {
            break;
        }
        pos = pos.saturating_add(header.size);
    }

    None
}

/// Detect the audio codec in an MP4 file by inspecting the stsd box.
/// Navigates moov → trak → mdia → minf → stbl → stsd to find the codec.
pub fn detect_mp4_audio_codec(file_path: &Path) -> Option<Mp4AudioCodec> {
    let moov = read_moov_content(file_path)?;

    // Search through all trak boxes for an audio track
    let mut trak_search_pos = 0;

    while trak_search_pos < moov.len() {
        let (trak_pos, trak_header) =
            find_box_in_container(&moov, trak_search_pos, moov.len() - trak_search_pos, TRAK)?;
        let trak_start = trak_pos + trak_header.header_size as usize;
        let trak_size = trak_header.content_size() as usize;

        if let Some(codec) = detect_codec_in_trak(&moov, trak_start, trak_size) {
            return Some(codec);
        }

        // size == 0 means "extends to EOF"; advancing by 0 would loop forever.
        if trak_header.size == 0 {
            break;
        }
        trak_search_pos = trak_pos + trak_header.size as usize;
    }

    None
}

fn detect_codec_in_trak(data: &[u8], trak_start: usize, trak_size: usize) -> Option<Mp4AudioCodec> {
    let (mdia_pos, mdia_header) = find_box_in_container(data, trak_start, trak_size, MDIA)?;
    let mdia_start = mdia_pos + mdia_header.header_size as usize;
    let mdia_size = mdia_header.content_size() as usize;

    let (minf_pos, minf_header) = find_box_in_container(data, mdia_start, mdia_size, MINF)?;
    let minf_start = minf_pos + minf_header.header_size as usize;
    let minf_size = minf_header.content_size() as usize;

    let (stbl_pos, stbl_header) = find_box_in_container(data, minf_start, minf_size, STBL)?;
    let stbl_start = stbl_pos + stbl_header.header_size as usize;
    let stbl_size = stbl_header.content_size() as usize;

    let (stsd_pos, stsd_header) = find_box_in_container(data, stbl_start, stbl_size, STSD)?;
    // stsd has 4-byte version/flags + 4-byte entry count before entries.
    // Clamp to the buffer: the declared stsd size may overrun a truncated file.
    let entries_start = stsd_pos + stsd_header.header_size as usize + 8;
    let stsd_end = (stsd_pos + stsd_header.size as usize).min(data.len());

    if entries_start + 8 > stsd_end {
        return None;
    }

    // Read the first sample entry's box type (the codec identifier)
    let entry_type = u32::from_be_bytes([
        data[entries_start + 4],
        data[entries_start + 5],
        data[entries_start + 6],
        data[entries_start + 7],
    ]);

    match entry_type {
        MP4A => Some(Mp4AudioCodec::Aac),
        ALAC => Some(Mp4AudioCodec::Alac),
        _ => Some(Mp4AudioCodec::Unknown),
    }
}

/// Check if file is an MP4/M4A file containing AAC audio.
/// Returns false for ALAC, DRM-protected, and non-MP4 files.
pub fn is_aac_file(file_path: &Path) -> bool {
    if !is_mp4_file(file_path) {
        return false;
    }
    matches!(
        detect_mp4_audio_codec(file_path),
        Some(Mp4AudioCodec::Aac) | None
    )
}

/// Count the number of audio tracks in an MP4 file.
/// Returns 0 if the file cannot be read or has no moov box.
pub fn count_audio_tracks(file_path: &Path) -> usize {
    let moov = match read_moov_content(file_path) {
        Some(m) => m,
        None => return 0,
    };

    let mut count = 0;
    let mut search_pos = 0;

    while search_pos < moov.len() {
        let (trak_pos, trak_header) =
            match find_box_in_container(&moov, search_pos, moov.len() - search_pos, TRAK) {
                Some(x) => x,
                None => break,
            };

        // Check if this trak has an audio codec (mp4a or alac)
        let trak_start = trak_pos + trak_header.header_size as usize;
        let trak_size = trak_header.content_size() as usize;
        if detect_codec_in_trak(&moov, trak_start, trak_size).is_some() {
            count += 1;
        }

        // size == 0 means "extends to EOF"; advancing by 0 would loop forever.
        if trak_header.size == 0 {
            break;
        }
        search_pos = trak_pos + trak_header.size as usize;
    }

    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freeform_tag_serialization() {
        let tag = FreeformTag::new(
            "com.apple.iTunes".to_string(),
            "replaygain_track_gain".to_string(),
            "+3.50 dB".to_string(),
        );

        let serialized = serialize_freeform_tag(&tag);

        // Should start with ---- box header
        assert_eq!(&serialized[4..8], b"----");

        // Parse it back
        let parsed = parse_freeform_tag(&serialized[8..]).unwrap();
        assert_eq!(parsed.namespace(), tag.namespace());
        assert_eq!(parsed.name(), tag.name());
        assert_eq!(parsed.value(), tag.value());
    }

    #[test]
    fn test_replaygain_tags() {
        let mut tags = ReplayGainTags::default();
        tags.set_track(3.5, 0.98765);
        tags.set_album(2.0, 0.99999);

        assert_eq!(tags.track_gain(), Some("+3.50 dB"));
        assert_eq!(tags.track_peak(), Some("0.987650"));
        assert_eq!(tags.album_gain(), Some("+2.00 dB"));
        assert_eq!(tags.album_peak(), Some("0.999990"));

        let freeform_tags = tags.to_freeform_tags();
        assert_eq!(freeform_tags.len(), 4);
    }

    #[test]
    fn test_undo_tags() {
        let tags = UndoTags::default();
        assert!(tags.is_empty());

        let tags = UndoTags::new(Some("+003,+003,N".to_string()), Some("80,120".to_string()));
        assert!(!tags.is_empty());

        let freeform_tags = tags.to_freeform_tags();
        assert_eq!(freeform_tags.len(), 2);
        assert_eq!(freeform_tags[0].name(), UNDO_TAG);
        assert_eq!(freeform_tags[0].value(), "+003,+003,N");
        assert_eq!(freeform_tags[1].name(), MINMAX_TAG);
        assert_eq!(freeform_tags[1].value(), "80,120");
    }

    #[test]
    fn test_undo_tag_serialization_roundtrip() {
        let tag = FreeformTag::new(
            "com.apple.iTunes".to_string(),
            UNDO_TAG.to_string(),
            "+005,+005,N".to_string(),
        );

        let serialized = serialize_freeform_tag(&tag);
        let parsed = parse_freeform_tag(&serialized[8..]).unwrap();
        assert_eq!(parsed.namespace(), tag.namespace());
        assert_eq!(parsed.name(), tag.name());
        assert_eq!(parsed.value(), tag.value());
    }

    #[test]
    fn test_ilst_box_filtered_preserves_other_tags() {
        // Create an ilst with one RG tag and one undo tag
        let rg_tag = FreeformTag::new(
            ITUNES_NAMESPACE.to_string(),
            RG_TRACK_GAIN.to_string(),
            "+3.50 dB".to_string(),
        );
        let undo_tag = FreeformTag::new(
            ITUNES_NAMESPACE.to_string(),
            UNDO_TAG.to_string(),
            "+002,+002,N".to_string(),
        );

        let mut existing = Vec::new();
        existing.extend_from_slice(&serialize_freeform_tag(&rg_tag));
        existing.extend_from_slice(&serialize_freeform_tag(&undo_tag));

        // Writing new RG tags should preserve the undo tag
        let mut new_rg = ReplayGainTags::default();
        new_rg.set_track(5.0, 0.0);
        let result = create_ilst_box(&new_rg, &existing);
        // Result should contain the new RG tag AND the preserved undo tag
        assert!(result.len() > 8); // more than just the ilst header

        // Writing new undo tags should preserve the RG tag
        let new_undo = UndoTags::new(Some("+005,+005,N".to_string()), None);
        let result = create_ilst_box_undo(&new_undo, &existing);
        assert!(result.len() > 8);
    }

    /// Codec detection and track counting must work without a full-file
    /// read, including the non-faststart layout where moov follows mdat
    /// (issue #188).
    #[test]
    fn test_detect_codec_and_count_tracks_reads_moov_only() {
        use std::io::Write;

        fn mp4_box(typ: &[u8; 4], content: &[u8]) -> Vec<u8> {
            let mut v = Vec::with_capacity(8 + content.len());
            v.extend_from_slice(&((content.len() + 8) as u32).to_be_bytes());
            v.extend_from_slice(typ);
            v.extend_from_slice(content);
            v
        }

        fn audio_trak(codec: &[u8; 4]) -> Vec<u8> {
            let entry = mp4_box(codec, &[]);
            let mut stsd_content = vec![0u8; 4]; // version + flags
            stsd_content.extend_from_slice(&1u32.to_be_bytes()); // entry count
            stsd_content.extend_from_slice(&entry);
            let stsd = mp4_box(b"stsd", &stsd_content);
            let stbl = mp4_box(b"stbl", &stsd);
            let minf = mp4_box(b"minf", &stbl);
            let mdia = mp4_box(b"mdia", &minf);
            mp4_box(b"trak", &mdia)
        }

        let ftyp = mp4_box(b"ftyp", b"M4A \x00\x00\x00\x00M4A ");
        let mdat = mp4_box(b"mdat", &[0u8; 4096]);
        let mut moov_content = audio_trak(b"mp4a");
        moov_content.extend_from_slice(&audio_trak(b"alac"));
        let moov = mp4_box(b"moov", &moov_content);

        let dir = std::env::temp_dir().join("mp3rgain_test_moov_only_read");
        let _ = std::fs::create_dir_all(&dir);

        for (name, layout) in [
            ("faststart.m4a", [&ftyp, &moov, &mdat]),
            ("trailing_moov.m4a", [&ftyp, &mdat, &moov]),
        ] {
            let path = dir.join(name);
            let mut f = std::fs::File::create(&path).unwrap();
            for part in layout {
                f.write_all(part).unwrap();
            }

            assert_eq!(
                detect_mp4_audio_codec(&path),
                Some(Mp4AudioCodec::Aac),
                "{name}"
            );
            assert_eq!(count_audio_tracks(&path), 2, "{name}");
            assert!(is_aac_file(&path), "{name}");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_is_mp4_detection() {
        use std::io::Write;

        let dir = std::env::temp_dir().join("mp3rgain_test_mp4_detection");
        let _ = std::fs::create_dir_all(&dir);

        // M4A major brand -> accepted
        let path = dir.join("test_m4a.m4a");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[
            0x00, 0x00, 0x00, 0x14, // size = 20
            b'f', b't', b'y', b'p', // type = ftyp
            b'M', b'4', b'A', b' ', // major brand = M4A
            0x00, 0x00, 0x00, 0x00, // minor version
            b'M', b'4', b'A', b' ', // compatible brand
        ])
        .unwrap();
        drop(f);
        assert!(is_mp4_file(&path));

        // M4P (DRM) major brand -> rejected
        let path = dir.join("test_m4p.m4a");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[
            0x00, 0x00, 0x00, 0x14, // size = 20
            b'f', b't', b'y', b'p', // type = ftyp
            b'M', b'4', b'P', b' ', // major brand = M4P (DRM)
            0x00, 0x00, 0x00, 0x00, // minor version
            b'M', b'4', b'P', b' ', // compatible brand
        ])
        .unwrap();
        drop(f);
        assert!(!is_mp4_file(&path));

        // isom major brand with M4A in compatible brands -> accepted
        let path = dir.join("test_compat.m4a");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[
            0x00, 0x00, 0x00, 0x1c, // size = 28
            b'f', b't', b'y', b'p', // type = ftyp
            b'd', b'a', b's', b'h', // major brand = dash (not accepted)
            0x00, 0x00, 0x00, 0x00, // minor version
            b'i', b's', b'o', b'6', // compatible brand = iso6 (not accepted)
            b'M', b'4', b'A', b' ', // compatible brand = M4A (accepted!)
        ])
        .unwrap();
        drop(f);
        assert!(is_mp4_file(&path));

        // Non-MP4 file -> rejected
        let path = dir.join("test_mp3.mp3");
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(b"ID3\x04\x00\x00\x00\x00\x00\x00").unwrap();
        drop(f);
        assert!(!is_mp4_file(&path));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
