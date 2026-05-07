use anyhow::Result;
use colored::*;
use mp3rgain::{analyze, ape, id3v2, mp4meta, Mp3Analysis};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crate::cli::options::{Options, OutputFormat};

/// Run a gain operation, optionally going through a sibling temp file.
///
/// The closure receives `(read_from, write_to)`. When `-t` is in effect they
/// differ (read original, write temp file, rename) — when it isn't, both are
/// the original path and the operation works in place.
///
/// Compared with the old "copy original to temp, then operate on temp" flow,
/// the split-path variant saves one full-file pass on the `-t` apply path
/// (issue #135).
pub fn apply_with_temp_file<F>(file: &Path, operation: F, opts: &Options) -> Result<usize>
where
    F: FnOnce(&Path, &Path) -> Result<usize>,
{
    if opts.use_temp_file {
        let parent = file.parent().unwrap_or(Path::new("."));
        let temp_path = parent.join(format!(".mp3rgain_temp_{}.mp3", std::process::id()));

        match operation(file, &temp_path) {
            Ok(frames) => {
                fs::rename(&temp_path, file)?;
                Ok(frames)
            }
            Err(e) => {
                let _ = fs::remove_file(&temp_path);
                Err(e)
            }
        }
    } else {
        operation(file, file)
    }
}

pub fn restore_timestamp(file: &Path, mtime: SystemTime) {
    let _ = std::fs::File::options()
        .write(true)
        .open(file)
        .and_then(|f| f.set_times(std::fs::FileTimes::new().set_modified(mtime)));
}

pub fn save_original_mtime(file: &Path, opts: &Options) -> Option<SystemTime> {
    if opts.preserve_timestamp && !opts.dry_run {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    }
}

/// Write ID3v2 undo metadata after a gain has been applied.
///
/// Reads the existing undo values, accumulates the deltas, and writes the
/// combined left/right values back as a TXXX:MP3GAIN_UNDO frame along with
/// the supplied min/max global_gain (`analysis` is taken pre-apply so callers
/// can reuse the result they already have, avoiding a redundant file read).
pub fn write_id3v2_undo_after_apply(
    file: &Path,
    delta_left: i32,
    delta_right: i32,
    wrap: bool,
    analysis: Option<&Mp3Analysis>,
) -> Result<()> {
    let existing_rg = id3v2::read_id3v2_replaygain(file).unwrap_or_default();
    let (existing_left, existing_right) = ape::parse_undo_values(existing_rg.undo.as_deref());

    let owned;
    let (min, max) = match analysis {
        Some(a) => (a.min_gain(), a.max_gain()),
        None => {
            owned = analyze(file)?;
            (owned.min_gain(), owned.max_gain())
        }
    };

    id3v2::write_id3v2_undo(
        file,
        existing_left + delta_left,
        existing_right + delta_right,
        wrap,
        min,
        max,
    )?;
    Ok(())
}

pub fn warn_aac_multi_track(file: &Path, filename: &str, opts: &Options, dry_run_prefix: &str) {
    if opts.output_format != OutputFormat::Text || opts.quiet {
        return;
    }
    let track_count = mp4meta::count_audio_tracks(file);
    if track_count > 1 {
        eprintln!(
            "  {} {}{} - {} audio tracks detected, processing first track only",
            "!".yellow(),
            dry_run_prefix,
            filename,
            track_count
        );
    }
}
