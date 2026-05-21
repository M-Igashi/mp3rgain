use anyhow::Result;
use colored::*;
use mp3rgain::{analyze, ape, id3v2, mp4meta, Mp3Analysis};
use std::path::Path;
use std::time::SystemTime;

use crate::cli::options::{Options, OutputFormat};

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
