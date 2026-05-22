use colored::*;
use mp3rgain::mp4meta;
use std::path::Path;
use std::time::SystemTime;

pub use mp3rgain::apply::restore_timestamp;

use crate::cli::options::{Options, OutputFormat};

pub fn save_original_mtime(file: &Path, opts: &Options) -> Option<SystemTime> {
    if opts.preserve_timestamp && !opts.dry_run {
        mp3rgain::apply::read_mtime(file)
    } else {
        None
    }
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
