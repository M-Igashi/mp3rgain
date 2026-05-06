use anyhow::Result;
use colored::*;
use mp3rgain::mp4meta;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crate::cli::options::{Options, OutputFormat};

pub fn apply_with_temp_file<F>(file: &Path, operation: F, opts: &Options) -> Result<usize>
where
    F: FnOnce(&Path) -> Result<usize>,
{
    if opts.use_temp_file {
        let parent = file.parent().unwrap_or(Path::new("."));
        let temp_path = parent.join(format!(".mp3rgain_temp_{}.mp3", std::process::id()));

        fs::copy(file, &temp_path)?;

        match operation(&temp_path) {
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
        operation(file)
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
