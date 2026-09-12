//! Album identity tags, used by `-a --album-by=tag` to decide which files
//! belong to the same release (issue #333).
//!
//! One symphonia metadata probe covers every input format mp3rgain accepts:
//! ID3v2 in MP3 and in raw ADTS, and the `ilst` atoms in M4A/MP4. Hand-rolling
//! a reader per container, the way [`crate::id3v2`] and [`crate::mp4meta`] do
//! for the ReplayGain tags, would mean three parsers that have to agree.

use std::path::Path;

/// The tags that decide which album a file belongs to.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AlbumTags {
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub artist: Option<String>,
    /// `MUSICBRAINZ_ALBUMID`, written by Picard and beets. The only field that
    /// separates two releases carrying the same artist and album string
    /// without guessing at how the user chose to tell them apart.
    pub musicbrainz_album_id: Option<String>,
    pub disc: Option<u64>,
    pub track: Option<u64>,
}

impl AlbumTags {
    /// The artist half of the grouping key: ALBUMARTIST, falling back to
    /// ARTIST. Without it, two unrelated "Greatest Hits" become one album.
    pub fn effective_artist(&self) -> Option<&str> {
        self.album_artist.as_deref().or(self.artist.as_deref())
    }

    /// Whether this file can be grouped by release at all.
    pub fn has_album(&self) -> bool {
        self.album.is_some()
    }
}

/// Read the album identity tags from `path`, or `None` when the file cannot be
/// probed. A file that probes but carries no tags returns an empty
/// [`AlbumTags`], which is a different thing from a file that cannot be read.
#[cfg(feature = "replaygain")]
pub fn read_album_tags(path: &Path) -> Option<AlbumTags> {
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::{MetadataOptions, StandardTag};

    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, Default::default(), MetadataOptions::default())
        .ok()?;

    // Every revision is merged, not just the latest: a file routinely carries
    // more than one (ID3v2 at the front plus ID3v1 at the end, or a second
    // ID3v2 tag prepended by a tool), and `skip_to_latest` would throw the
    // richer front tag away. The first revision to define a field wins, so the
    // modern tag beats a truncated legacy one that happens to come after it.
    let mut tags = AlbumTags::default();
    let mut metadata = format.metadata();
    loop {
        if let Some(revision) = metadata.current() {
            for tag in &revision.media.tags {
                match &tag.std {
                    Some(StandardTag::Album(v)) => fill(&mut tags.album, v),
                    Some(StandardTag::AlbumArtist(v)) => fill(&mut tags.album_artist, v),
                    Some(StandardTag::Artist(v)) => fill(&mut tags.artist, v),
                    Some(StandardTag::MusicBrainzAlbumId(v)) => {
                        fill(&mut tags.musicbrainz_album_id, v)
                    }
                    Some(StandardTag::DiscNumber(n)) => tags.disc = tags.disc.or(Some(*n)),
                    Some(StandardTag::TrackNumber(n)) => tags.track = tags.track.or(Some(*n)),
                    _ => {}
                }
            }
        }
        // `pop` always keeps the last revision, so this is what ends the walk.
        if metadata.pop().is_none() {
            break;
        }
    }
    Some(tags)
}

/// Set `slot` from `value` unless it is already set, or the value is blank.
/// Tag values are routinely present but empty, and a blank ALBUM must not
/// become a grouping key of its own: every untagged file in a library would
/// land in one album together.
#[cfg(feature = "replaygain")]
fn fill(slot: &mut Option<String>, value: &str) {
    if slot.is_some() {
        return;
    }
    let trimmed = value.trim();
    if !trimmed.is_empty() {
        *slot = Some(trimmed.to_string());
    }
}

#[cfg(not(feature = "replaygain"))]
pub fn read_album_tags(_path: &Path) -> Option<AlbumTags> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_artist_wins_over_artist() {
        let tags = AlbumTags {
            album_artist: Some("Various Artists".into()),
            artist: Some("Pink Floyd".into()),
            ..Default::default()
        };
        assert_eq!(tags.effective_artist(), Some("Various Artists"));
    }

    #[test]
    fn artist_is_the_fallback() {
        let tags = AlbumTags {
            artist: Some("Pink Floyd".into()),
            ..Default::default()
        };
        assert_eq!(tags.effective_artist(), Some("Pink Floyd"));
    }
}
