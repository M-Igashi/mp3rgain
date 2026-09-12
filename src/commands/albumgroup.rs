//! What counts as one album under `-a` (issues #324, #331, #333).
//!
//! `-a` alone pools every file, which is mp3gain's rule. The flags split the
//! run instead: `--album-by=dir` by parent directory, `--album-by=tag` by
//! release taken from the metadata, and `--album-depth N` by directory N
//! levels below each root argument.

use colored::*;
use mp3rgain::{read_album_tags, AlbumTags};
use rayon::prelude::*;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::cli::options::{AlbumGrouping, Options};

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
pub fn group_files(files: &[PathBuf], opts: &Options) -> (Vec<AlbumGroup>, Vec<String>) {
    match opts.album_by {
        AlbumGrouping::Tag => group_by_tags(files),
        AlbumGrouping::Depth(levels) => {
            let groups = group_by_depth(files, &opts.arg_roots, levels);
            let warnings = split_release_warnings(&groups);
            (groups, warnings)
        }
        // Pooled never reaches here; the caller dispatches it to `cmd_album_gain`.
        _ => {
            let groups = group_by_parent(files);
            let warnings = split_release_warnings(&groups);
            (groups, warnings)
        }
    }
}

/// Group by immediate parent directory, as given on the command line. Bare
/// file names land in `.`. Directories come out in path order.
fn group_by_parent(files: &[PathBuf]) -> Vec<AlbumGroup> {
    group_by_directory(files.iter().map(|f| (parent_of(f), f.clone())))
}

/// `--album-depth N`: the album is the directory N levels below the root
/// argument the file was found under (issue #331).
///
/// A file shallower than N levels groups at the deepest directory it actually
/// has, so a stray file at the root of the tree does not get a group key that
/// points at a directory it is not in. Descending deeper than N is what makes
/// this handle multi-disc releases: at `--album-depth 2` over `Artist/Album`,
/// `Artist/Album/disc1` and `disc2` both truncate to `Artist/Album`.
fn group_by_depth(files: &[PathBuf], roots: &[PathBuf], levels: usize) -> Vec<AlbumGroup> {
    // Deepest root first, so an overlapping pair like `-R music music/album`
    // assigns the file to `music/album` rather than to whichever came first.
    let mut roots: Vec<&Path> = roots.iter().map(|p| p.as_path()).collect();
    roots.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    group_by_directory(files.iter().map(|file| {
        let group = roots
            .iter()
            .find_map(|root| album_dir_under(file, root, levels))
            .unwrap_or_else(|| parent_of(file));
        (group, file.clone())
    }))
}

/// The album directory for `file` under `root`, or `None` when the file is not
/// under that root.
fn album_dir_under(file: &Path, root: &Path, levels: usize) -> Option<PathBuf> {
    let relative = file.strip_prefix(root).ok()?;
    // `relative` ends in the file name, which is not a level; only the
    // directories above it are counted.
    let Some(dirs) = relative.parent() else {
        // The root *is* the file: a bare file argument pools with its
        // neighbours rather than becoming a one-file album of its own.
        return Some(parent_of(file));
    };
    let mut dir = root.to_path_buf();
    dir.extend(dirs.components().take(levels));
    Some(dir)
}

/// Directory grouping cannot see that two sibling folders are one release, so
/// it says so instead of writing a per-disc album gain in silence (#331).
///
/// One tag probe per group rather than per file: ALBUM is uniform inside a
/// directory, so the first file answers for the whole group and a library
/// sweep costs one probe per directory.
fn split_release_warnings(groups: &[AlbumGroup]) -> Vec<String> {
    // Only groups under a common parent can be discs of one release.
    let mut siblings: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for (i, group) in groups.iter().enumerate() {
        let AlbumId::Directory(dir) = &group.id else {
            continue;
        };
        if let Some(parent) = dir.parent() {
            siblings.entry(parent.to_path_buf()).or_default().push(i);
        }
    }
    let candidates: Vec<usize> = siblings
        .values()
        .filter(|g| g.len() > 1)
        .flatten()
        .copied()
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let probed: HashMap<usize, AlbumTags> = candidates
        .par_iter()
        .filter_map(|&i| {
            let tags = read_album_tags(groups[i].files.first()?)?;
            tags.has_album().then_some((i, tags))
        })
        .collect();

    let mut warnings = Vec::new();
    for members in siblings.values().filter(|g| g.len() > 1) {
        let mut by_release: BTreeMap<(String, String), Vec<usize>> = BTreeMap::new();
        for &i in members {
            let Some(tags) = probed.get(&i) else { continue };
            let key = (
                tags.effective_artist().unwrap_or_default().to_string(),
                tags.album.clone().unwrap_or_default(),
            );
            by_release.entry(key).or_default().push(i);
        }
        for ((_, album), split) in by_release.iter().filter(|(_, s)| s.len() > 1) {
            let first = &probed[&split[0]];
            if !split[1..]
                .iter()
                .any(|i| looks_like_one_release(first, &probed[i]))
            {
                continue;
            }
            let mut warning = format!(
                "  {} ALBUM \"{}\" is split across {} directories and was scanned as that many albums\n",
                "!".yellow(),
                album,
                split.len()
            );
            for &i in split {
                warning.push_str(&format!("      {}\n", groups[i].id.heading()));
            }
            warning.push_str("      use --album-by=tag to treat them as one release");
            warnings.push(warning);
        }
    }
    warnings
}

/// Whether two sibling directories sharing an artist and album string are one
/// multi-disc release rather than two editions of the same album.
///
/// The distinction matters because the advice differs: discs of one release
/// want `--album-by=tag`, while two editions are already grouped correctly by
/// directory and pointing the user at `tag` would merge them wrongly. Same
/// reasoning as the tag-mode collision report, from the other direction.
fn looks_like_one_release(a: &AlbumTags, b: &AlbumTags) -> bool {
    // Distinct MusicBrainz ids are two releases by definition.
    if let (Some(x), Some(y)) = (&a.musicbrainz_album_id, &b.musicbrainz_album_id) {
        if x != y {
            return false;
        }
    }
    match (a.disc, b.disc) {
        // Different disc numbers under one album title: one release.
        (Some(x), Some(y)) => x != y,
        // No disc tags to go on, so fall back to the track numbers. The same
        // track in both directories is the signature of two editions.
        _ => a.track != b.track || a.track.is_none(),
    }
}

fn group_by_directory(entries: impl Iterator<Item = (PathBuf, PathBuf)>) -> Vec<AlbumGroup> {
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for (dir, file) in entries {
        groups.entry(dir).or_default().push(file);
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
