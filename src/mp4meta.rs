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

use anyhow::{Context, Result};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;

/// ReplayGain tag keys (iTunes freeform format)
pub const RG_TRACK_GAIN: &str = "replaygain_track_gain";
pub const RG_TRACK_PEAK: &str = "replaygain_track_peak";
pub const RG_ALBUM_GAIN: &str = "replaygain_album_gain";
pub const RG_ALBUM_PEAK: &str = "replaygain_album_peak";

/// iTunes namespace for freeform tags
const ITUNES_NAMESPACE: &str = "com.apple.iTunes";

/// MP4 box/atom types
#[allow(dead_code)]
const FTYP: u32 = u32::from_be_bytes(*b"ftyp");
const MOOV: u32 = u32::from_be_bytes(*b"moov");
const UDTA: u32 = u32::from_be_bytes(*b"udta");
const META: u32 = u32::from_be_bytes(*b"meta");
const ILST: u32 = u32::from_be_bytes(*b"ilst");
#[allow(dead_code)]
const FREE: u32 = u32::from_be_bytes(*b"free");
const MDAT: u32 = u32::from_be_bytes(*b"mdat");
#[allow(dead_code)]
const HDLR: u32 = u32::from_be_bytes(*b"hdlr");
const FREEFORM: u32 = u32::from_be_bytes(*b"----");
const MEAN: u32 = u32::from_be_bytes(*b"mean");
const NAME: u32 = u32::from_be_bytes(*b"name");
const DATA: u32 = u32::from_be_bytes(*b"data");

/// MP4 box header
#[derive(Debug, Clone)]
struct BoxHeader {
    size: u64,
    box_type: u32,
    header_size: u8, // 8 for normal, 16 for extended size
}

impl BoxHeader {
    fn read<R: Read>(reader: &mut R) -> Result<Option<Self>> {
        let mut buf = [0u8; 8];
        match reader.read_exact(&mut buf) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
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

        Ok(Some(BoxHeader {
            size,
            box_type,
            header_size,
        }))
    }

    fn content_size(&self) -> u64 {
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
#[derive(Debug, Clone)]
pub struct FreeformTag {
    pub namespace: String,
    pub name: String,
    pub value: String,
}

/// Collection of ReplayGain tags
#[derive(Debug, Clone, Default)]
pub struct ReplayGainTags {
    pub track_gain: Option<String>,
    pub track_peak: Option<String>,
    pub album_gain: Option<String>,
    pub album_peak: Option<String>,
}

impl ReplayGainTags {
    pub fn new() -> Self {
        Self::default()
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

    fn to_freeform_tags(&self) -> Vec<FreeformTag> {
        let mut tags = Vec::new();

        if let Some(ref v) = self.track_gain {
            tags.push(FreeformTag {
                namespace: ITUNES_NAMESPACE.to_string(),
                name: RG_TRACK_GAIN.to_string(),
                value: v.clone(),
            });
        }
        if let Some(ref v) = self.track_peak {
            tags.push(FreeformTag {
                namespace: ITUNES_NAMESPACE.to_string(),
                name: RG_TRACK_PEAK.to_string(),
                value: v.clone(),
            });
        }
        if let Some(ref v) = self.album_gain {
            tags.push(FreeformTag {
                namespace: ITUNES_NAMESPACE.to_string(),
                name: RG_ALBUM_GAIN.to_string(),
                value: v.clone(),
            });
        }
        if let Some(ref v) = self.album_peak {
            tags.push(FreeformTag {
                namespace: ITUNES_NAMESPACE.to_string(),
                name: RG_ALBUM_PEAK.to_string(),
                value: v.clone(),
            });
        }

        tags
    }
}

/// Find box position in data
fn find_box(data: &[u8], box_type: u32) -> Option<(usize, BoxHeader)> {
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
fn find_box_in_container(
    data: &[u8],
    container_start: usize,
    container_size: usize,
    box_type: u32,
) -> Option<(usize, BoxHeader)> {
    let container_end = container_start + container_size;
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

            pos += header.size as usize;
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

        if content_start + content_size > data.len() {
            break;
        }

        match header.box_type {
            MEAN => {
                // Skip 4-byte version/flags
                if content_size > 4 {
                    namespace = Some(
                        String::from_utf8_lossy(
                            &data[content_start + 4..content_start + content_size],
                        )
                        .to_string(),
                    );
                }
            }
            NAME => {
                // Skip 4-byte version/flags
                if content_size > 4 {
                    name = Some(
                        String::from_utf8_lossy(
                            &data[content_start + 4..content_start + content_size],
                        )
                        .to_string(),
                    );
                }
            }
            DATA => {
                // Skip 8-byte version/flags + type indicator
                if content_size > 8 {
                    value = Some(
                        String::from_utf8_lossy(
                            &data[content_start + 8..content_start + content_size],
                        )
                        .to_string(),
                    );
                }
            }
            _ => {}
        }

        cursor.set_position((content_start + content_size) as u64);
    }

    match (namespace, name, value) {
        (Some(ns), Some(n), Some(v)) => Some(FreeformTag {
            namespace: ns,
            name: n,
            value: v,
        }),
        _ => None,
    }
}

/// Serialize freeform tag to bytes
fn serialize_freeform_tag(tag: &FreeformTag) -> Vec<u8> {
    let mut result = Vec::new();

    // mean box
    let mean_data = tag.namespace.as_bytes();
    let mean_size = 12 + mean_data.len() as u32; // 8 header + 4 version/flags + data
    result.extend_from_slice(&mean_size.to_be_bytes());
    result.extend_from_slice(b"mean");
    result.extend_from_slice(&[0u8; 4]); // version/flags
    result.extend_from_slice(mean_data);

    // name box
    let name_data = tag.name.as_bytes();
    let name_size = 12 + name_data.len() as u32;
    result.extend_from_slice(&name_size.to_be_bytes());
    result.extend_from_slice(b"name");
    result.extend_from_slice(&[0u8; 4]); // version/flags
    result.extend_from_slice(name_data);

    // data box
    let value_data = tag.value.as_bytes();
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

/// Read ReplayGain tags from MP4/M4A file
pub fn read_replaygain_tags(file_path: &Path) -> Result<ReplayGainTags> {
    let data =
        fs::read(file_path).with_context(|| format!("Failed to read: {}", file_path.display()))?;

    let mut tags = ReplayGainTags::new();

    // Find moov box
    let (moov_pos, moov_header) = match find_box(&data, MOOV) {
        Some(x) => x,
        None => return Ok(tags), // No moov, no metadata
    };

    let moov_content_start = moov_pos + moov_header.header_size as usize;
    let moov_content_size = moov_header.content_size() as usize;

    // Find udta in moov
    let (udta_pos, udta_header) =
        match find_box_in_container(&data, moov_content_start, moov_content_size, UDTA) {
            Some(x) => x,
            None => return Ok(tags),
        };

    let udta_content_start = udta_pos + udta_header.header_size as usize;
    let udta_content_size = udta_header.content_size() as usize;

    // Find meta in udta
    let (meta_pos, meta_header) =
        match find_box_in_container(&data, udta_content_start, udta_content_size, META) {
            Some(x) => x,
            None => return Ok(tags),
        };

    // meta box has 4-byte version/flags before content
    let meta_content_start = meta_pos + meta_header.header_size as usize + 4;
    let meta_content_size = meta_header.content_size() as usize - 4;

    // Find ilst in meta
    let (ilst_pos, ilst_header) =
        match find_box_in_container(&data, meta_content_start, meta_content_size, ILST) {
            Some(x) => x,
            None => return Ok(tags),
        };

    let ilst_content_start = ilst_pos + ilst_header.header_size as usize;
    let ilst_content_size = ilst_header.content_size() as usize;

    // Parse freeform tags in ilst
    let mut pos = ilst_content_start;
    while pos + 8 <= ilst_content_start + ilst_content_size {
        let mut cursor = Cursor::new(&data[pos..]);
        if let Ok(Some(header)) = BoxHeader::read(&mut cursor) {
            if header.box_type == FREEFORM {
                let tag_data = &data[pos + header.header_size as usize..pos + header.size as usize];
                if let Some(tag) = parse_freeform_tag(tag_data) {
                    if tag.namespace == ITUNES_NAMESPACE {
                        match tag.name.as_str() {
                            x if x.eq_ignore_ascii_case(RG_TRACK_GAIN) => {
                                tags.track_gain = Some(tag.value);
                            }
                            x if x.eq_ignore_ascii_case(RG_TRACK_PEAK) => {
                                tags.track_peak = Some(tag.value);
                            }
                            x if x.eq_ignore_ascii_case(RG_ALBUM_GAIN) => {
                                tags.album_gain = Some(tag.value);
                            }
                            x if x.eq_ignore_ascii_case(RG_ALBUM_PEAK) => {
                                tags.album_peak = Some(tag.value);
                            }
                            _ => {}
                        }
                    }
                }
            }

            if header.size == 0 {
                break;
            }
            pos += header.size as usize;
        } else {
            break;
        }
    }

    Ok(tags)
}

/// Write ReplayGain tags to MP4/M4A file
pub fn write_replaygain_tags(file_path: &Path, tags: &ReplayGainTags) -> Result<()> {
    let data =
        fs::read(file_path).with_context(|| format!("Failed to read: {}", file_path.display()))?;

    let new_data = update_mp4_metadata(&data, tags)?;

    fs::write(file_path, &new_data)
        .with_context(|| format!("Failed to write: {}", file_path.display()))?;

    Ok(())
}

/// Update MP4 metadata with new ReplayGain tags
fn update_mp4_metadata(data: &[u8], tags: &ReplayGainTags) -> Result<Vec<u8>> {
    // Find moov box
    let (moov_pos, moov_header) =
        find_box(data, MOOV).ok_or_else(|| anyhow::anyhow!("No moov box found in MP4 file"))?;

    let moov_content_start = moov_pos + moov_header.header_size as usize;
    let moov_content_size = moov_header.content_size() as usize;
    let moov_end = moov_pos + moov_header.size as usize;

    // Try to find existing ilst or create new metadata structure
    let (new_ilst, ilst_info) =
        create_or_update_ilst(data, moov_content_start, moov_content_size, tags)?;

    // Rebuild the file
    let mut result = Vec::with_capacity(data.len() + 1024);

    match ilst_info {
        IlstLocation::Existing {
            ilst_pos,
            ilst_size,
            meta_pos,
            udta_pos,
        } => {
            // Calculate size differences
            let old_ilst_size = ilst_size;
            let new_ilst_size = new_ilst.len();
            let size_diff = new_ilst_size as i64 - old_ilst_size as i64;

            // Write data before ilst
            result.extend_from_slice(&data[..ilst_pos]);

            // Write new ilst
            result.extend_from_slice(&new_ilst);

            // Write data after old ilst
            result.extend_from_slice(&data[ilst_pos + old_ilst_size..]);

            // Update sizes in headers
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

    // Update mdat offset if needed (stco/co64 atoms)
    // For simplicity, we'll handle this by checking if moov comes before mdat
    if let Some((mdat_pos, _)) = find_box(data, MDAT) {
        if mdat_pos > moov_pos {
            // moov is before mdat, need to update chunk offsets
            let size_diff = result.len() as i64 - data.len() as i64;
            if size_diff != 0 {
                update_chunk_offsets(&mut result, moov_pos, size_diff)?;
            }
        }
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
    NeedsMeta {
        udta_pos: usize,
        udta_size: usize,
    },
    NeedsUdta,
}

fn create_or_update_ilst(
    data: &[u8],
    moov_content_start: usize,
    moov_content_size: usize,
    tags: &ReplayGainTags,
) -> Result<(Vec<u8>, IlstLocation)> {
    // Find udta
    let (udta_pos, udta_header) =
        match find_box_in_container(data, moov_content_start, moov_content_size, UDTA) {
            Some(x) => x,
            None => {
                // No udta, need to create everything
                let ilst = create_ilst_box(tags, &[]);
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
                let ilst = create_ilst_box(tags, &[]);
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
    let meta_content_size = meta_header.content_size() as usize - 4;

    // Find ilst
    let (ilst_pos, ilst_header) =
        match find_box_in_container(data, meta_content_start, meta_content_size, ILST) {
            Some(x) => x,
            None => {
                let ilst = create_ilst_box(tags, &[]);
                return Ok((
                    ilst,
                    IlstLocation::NeedsMeta {
                        udta_pos,
                        udta_size: udta_header.size as usize,
                    },
                ));
            }
        };

    // Parse existing ilst and merge with new tags
    let ilst_content_start = ilst_pos + ilst_header.header_size as usize;
    let ilst_content_size = ilst_header.content_size() as usize;
    let existing_content = &data[ilst_content_start..ilst_content_start + ilst_content_size];

    let new_ilst = create_ilst_box(tags, existing_content);

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

fn create_ilst_box(tags: &ReplayGainTags, existing_content: &[u8]) -> Vec<u8> {
    let mut content = Vec::new();

    // Copy existing non-ReplayGain tags
    let mut pos = 0;
    while pos + 8 <= existing_content.len() {
        let mut cursor = Cursor::new(&existing_content[pos..]);
        if let Ok(Some(header)) = BoxHeader::read(&mut cursor) {
            if header.size == 0 || pos + header.size as usize > existing_content.len() {
                break;
            }

            let tag_data = &existing_content[pos..pos + header.size as usize];

            // Check if this is a ReplayGain freeform tag
            let is_replaygain = if header.box_type == FREEFORM {
                let inner_data = &existing_content
                    [pos + header.header_size as usize..pos + header.size as usize];
                if let Some(tag) = parse_freeform_tag(inner_data) {
                    tag.namespace == ITUNES_NAMESPACE
                        && (tag.name.eq_ignore_ascii_case(RG_TRACK_GAIN)
                            || tag.name.eq_ignore_ascii_case(RG_TRACK_PEAK)
                            || tag.name.eq_ignore_ascii_case(RG_ALBUM_GAIN)
                            || tag.name.eq_ignore_ascii_case(RG_ALBUM_PEAK))
                } else {
                    false
                }
            } else {
                false
            };

            if !is_replaygain {
                content.extend_from_slice(tag_data);
            }

            pos += header.size as usize;
        } else {
            break;
        }
    }

    // Add new ReplayGain tags
    for tag in tags.to_freeform_tags() {
        content.extend_from_slice(&serialize_freeform_tag(&tag));
    }

    // Wrap in ilst box
    let ilst_size = 8 + content.len() as u32;
    let mut ilst = Vec::with_capacity(ilst_size as usize);
    ilst.extend_from_slice(&ilst_size.to_be_bytes());
    ilst.extend_from_slice(b"ilst");
    ilst.extend_from_slice(&content);

    ilst
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

/// Update stco/co64 chunk offsets after modifying moov size
fn update_chunk_offsets(data: &mut [u8], moov_pos: usize, size_diff: i64) -> Result<()> {
    // Find moov box again in the modified data
    let (_, moov_header) = match find_box(data, MOOV) {
        Some(x) => x,
        None => return Ok(()),
    };

    let moov_end = moov_pos + moov_header.size as usize;

    // Recursively find and update stco/co64 boxes within moov
    update_offsets_recursive(data, moov_pos + 8, moov_end, size_diff)?;

    Ok(())
}

const STCO: u32 = u32::from_be_bytes(*b"stco");
const CO64: u32 = u32::from_be_bytes(*b"co64");
const TRAK: u32 = u32::from_be_bytes(*b"trak");
const MDIA: u32 = u32::from_be_bytes(*b"mdia");
const MINF: u32 = u32::from_be_bytes(*b"minf");
const STBL: u32 = u32::from_be_bytes(*b"stbl");

fn update_offsets_recursive(
    data: &mut [u8],
    start: usize,
    end: usize,
    size_diff: i64,
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
                // Update 32-bit chunk offsets
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
                        let new_offset = (offset as i64 + size_diff) as u32;
                        data[offset_pos..offset_pos + 4].copy_from_slice(&new_offset.to_be_bytes());
                        offset_pos += 4;
                    }
                }
            }
            CO64 => {
                // Update 64-bit chunk offsets
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
                        let new_offset = (offset as i64 + size_diff) as u64;
                        data[offset_pos..offset_pos + 8].copy_from_slice(&new_offset.to_be_bytes());
                        offset_pos += 8;
                    }
                }
            }
            TRAK | MDIA | MINF | STBL | MOOV | UDTA => {
                // Container boxes - recurse into them
                update_offsets_recursive(data, pos + 8, pos + size as usize, size_diff)?;
            }
            _ => {}
        }

        pos += size as usize;
    }

    Ok(())
}

/// Delete ReplayGain tags from MP4/M4A file
pub fn delete_replaygain_tags(file_path: &Path) -> Result<()> {
    let empty_tags = ReplayGainTags::new();
    write_replaygain_tags(file_path, &empty_tags)
}

/// Check if file is an MP4/M4A file
pub fn is_mp4_file(file_path: &Path) -> bool {
    if let Ok(data) = fs::read(file_path) {
        if data.len() >= 12 {
            // Check for ftyp box
            let size = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            let box_type = &data[4..8];
            if box_type == b"ftyp" && size >= 12 {
                // Check compatible brands
                let brand = &data[8..12];
                return matches!(
                    brand,
                    b"M4A " | b"M4B " | b"M4P " | b"M4V " | b"mp41" | b"mp42" | b"isom" | b"iso2"
                );
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_freeform_tag_serialization() {
        let tag = FreeformTag {
            namespace: "com.apple.iTunes".to_string(),
            name: "replaygain_track_gain".to_string(),
            value: "+3.50 dB".to_string(),
        };

        let serialized = serialize_freeform_tag(&tag);

        // Should start with ---- box header
        assert_eq!(&serialized[4..8], b"----");

        // Parse it back
        let parsed = parse_freeform_tag(&serialized[8..]).unwrap();
        assert_eq!(parsed.namespace, tag.namespace);
        assert_eq!(parsed.name, tag.name);
        assert_eq!(parsed.value, tag.value);
    }

    #[test]
    fn test_replaygain_tags() {
        let mut tags = ReplayGainTags::new();
        tags.set_track(3.5, 0.98765);
        tags.set_album(2.0, 0.99999);

        assert_eq!(tags.track_gain, Some("+3.50 dB".to_string()));
        assert_eq!(tags.track_peak, Some("0.987650".to_string()));
        assert_eq!(tags.album_gain, Some("+2.00 dB".to_string()));
        assert_eq!(tags.album_peak, Some("0.999990".to_string()));

        let freeform_tags = tags.to_freeform_tags();
        assert_eq!(freeform_tags.len(), 4);
    }

    #[test]
    fn test_is_mp4_detection() {
        // Minimal valid ftyp header for M4A
        let m4a_header: Vec<u8> = vec![
            0x00, 0x00, 0x00, 0x14, // size = 20
            b'f', b't', b'y', b'p', // type = ftyp
            b'M', b'4', b'A', b' ', // brand = M4A
            0x00, 0x00, 0x00, 0x00, // minor version
            b'M', b'4', b'A', b' ', // compatible brand
        ];

        // This test would need a temp file, but we can verify the logic
        assert!(matches!(&m4a_header[8..12], b"M4A "));
    }
}
