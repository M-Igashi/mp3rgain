//! What counts as one album under `-a` (issues #324, #333).
//!
//! `-a` alone pools every file, which is mp3gain's rule. `--album-by` splits
//! the run instead: `dir` by parent directory, `tag` by release taken from the
//! metadata.

use colored::*;
use mp3rgain::{read_album_tags, AlbumTags};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::cli::options::AlbumGrouping;

/// How a set of files was identified as one album.
pub enum AlbumId {
    /// By parent directory: `--album-by=dir`, and the fallback in `tag` mode
    /// for files that carry no ALBUM tag.
    Directory(PathBuf),
    /// By release, from the tags.
    Release {
        artist: Option<String>,
        album: String,
    },
}

impl AlbumId {
    /// The heading printed above the album in text output.
    pub fn heading(&self) -> String {
        match self {
            Self::Directory(dir) => dir.display().to_string(),
            Self::Release {
                artist: Some(artist),
                album,
            } => format!("{} / {}", artist, album),
            Self::Release {
                artist: None,
                album,
            } => album.clone(),
        }
    }
}

pub struct AlbumGroup {
    pub id: AlbumId,
    pub files: Vec<PathBuf>,
}

/// Split `files` into albums. Returns the groups in a deterministic order
/// alongside the warnings the caller must surface: a grouping that silently
/// merges two releases, or silently drops files into a fallback, is the one
/// outcome that must not happen.
pub fn group_files(files: &[PathBuf], mode: AlbumGrouping) -> (Vec<AlbumGroup>, Vec<String>) {
    match mode {
        AlbumGrouping::Tag => group_by_tags(files),
        // Pooled never reaches here; the caller dispatches it to `cmd_album_gain`.
        _ => (group_by_parent(files), Vec::new()),
    }
}

/// Group by immediate parent directory, as given on the command line. Bare
/// file names land in `.`. Directories come out in path order.
fn group_by_parent(files: &[PathBuf]) -> Vec<AlbumGroup> {
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for file in files {
        groups
            .entry(parent_of(file))
            .or_default()
            .push(file.clone());
    }
    groups
        .into_iter()
        .map(|(dir, files)| AlbumGroup {
            id: AlbumId::Directory(dir),
            files,
        })
        .collect()
}

fn parent_of(file: &Path) -> PathBuf {
    file.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .to_path_buf()
}

/// The grouping key in `tag` mode.
#[derive(PartialEq, Eq, Hash, Clone)]
enum TagKey {
    /// MUSICBRAINZ_ALBUMID, preferred whenever it is present: it is the one
    /// field that separates two releases of the same album without guessing
    /// at how the user chose to tell them apart.
    MusicBrainz(String),
    /// (album artist or artist, album).
    Release(String, String),
    /// No ALBUM tag: this file falls back to its directory.
    Directory(PathBuf),
}

/// Group by release. Files with no ALBUM tag fall back to their directory
/// rather than pooling together, since pooling every untagged file in a
/// library into one album is silently wrong.
fn group_by_tags(files: &[PathBuf]) -> (Vec<AlbumGroup>, Vec<String>) {
    let tags: Vec<Option<AlbumTags>> = files.par_iter().map(|f| read_album_tags(f)).collect();

    // First-seen key order. `files` arrives sorted, so groups come out ordered
    // by their first file's path, which is stable across runs and matches how
    // `dir` mode orders its output.
    let mut order: Vec<TagKey> = Vec::new();
    let mut members: HashMap<TagKey, Vec<usize>> = HashMap::new();
    let mut untagged = 0usize;

    for (i, file_tags) in tags.iter().enumerate() {
        let key = match file_tags {
            Some(t) if t.has_album() => match &t.musicbrainz_album_id {
                Some(id) => TagKey::MusicBrainz(id.clone()),
                None => TagKey::Release(
                    t.effective_artist().unwrap_or_default().to_string(),
                    t.album.clone().unwrap_or_default(),
                ),
            },
            _ => {
                untagged += 1;
                TagKey::Directory(parent_of(&files[i]))
            }
        };
        let slot = members.entry(key.clone()).or_default();
        if slot.is_empty() {
            order.push(key);
        }
        slot.push(i);
    }

    let mut warnings = Vec::new();
    if untagged > 0 {
        warnings.push(format!(
            "  {} {} file(s) carry no ALBUM tag and were grouped by directory instead",
            "!".yellow(),
            untagged
        ));
    }

    let mut groups = Vec::with_capacity(order.len());
    for key in order {
        let indices = members.remove(&key).unwrap_or_default();
        let id = match &key {
            TagKey::Directory(dir) => AlbumId::Directory(dir.clone()),
            _ => {
                let first = tags[indices[0]].as_ref();
                AlbumId::Release {
                    artist: first
                        .and_then(|t| t.effective_artist())
                        .map(|s| s.to_string()),
                    album: first
                        .and_then(|t| t.album.clone())
                        .unwrap_or_else(|| "(unknown album)".to_string()),
                }
            }
        };
        if let Some(warning) = collision_warning(&id, &indices, &tags, files) {
            warnings.push(warning);
        }
        groups.push(AlbumGroup {
            files: indices.iter().map(|&i| files[i].clone()).collect(),
            id,
        });
    }

    (groups, warnings)
}

/// Detect two releases that collapsed into one group because they share an
/// artist and album string.
///
/// skamp raised this on #333 with five releases of "The Dark Side of the Moon":
/// the ways users tell releases apart (version, release date, a suffix in the
/// directory name) are too varied to enumerate, and guessing at them is, in
/// their words, dodgy. So nothing is guessed here. A repeated (disc, track)
/// pair inside one group is structural proof that more than one release is in
/// it, whatever convention the user was following.
fn collision_warning(
    id: &AlbumId,
    indices: &[usize],
    tags: &[Option<AlbumTags>],
    files: &[PathBuf],
) -> Option<String> {
    if matches!(id, AlbumId::Directory(_)) {
        return None;
    }

    let mut seen: HashMap<(Option<u64>, u64), usize> = HashMap::new();
    for &i in indices {
        let Some(t) = tags[i].as_ref() else { continue };
        let Some(track) = t.track else { continue };
        *seen.entry((t.disc, track)).or_insert(0) += 1;
    }
    let (&(disc, track), &copies) = seen.iter().max_by_key(|(_, &n)| n)?;
    if copies < 2 {
        return None;
    }

    let mut dirs: Vec<String> = indices
        .iter()
        .map(|&i| parent_of(&files[i]).display().to_string())
        .collect();
    dirs.sort();
    dirs.dedup();

    let position = match disc {
        Some(d) => format!("disc {} track {}", d, track),
        None => format!("track {}", track),
    };
    let mut warning = format!(
        "  {} {}: {} appears {} times, so this is probably {} releases sharing one album and artist string\n",
        "!".yellow(),
        id.heading(),
        position,
        copies,
        copies
    );
    for dir in &dirs {
        warning.push_str(&format!("      {}\n", dir));
    }
    warning.push_str(
        "      use --album-by=dir to keep them apart, or tag each release with its own MUSICBRAINZ_ALBUMID",
    );
    Some(warning)
}
