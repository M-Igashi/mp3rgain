//! Progress reporting helpers used during ReplayGain analysis.
//!
//! Per-file analysis progress was originally requested by @Sappharad in
//! #106 to mirror the byte-level progress output of the legacy mp3gain CLI
//! ("`54% of 119337493 bytes analyzed`"). The feature shipped in v2.2.0
//! once the Symphonia v0.6 decoder migration landed.

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::Path;

use crate::cli::options::{Options, OutputFormat};

pub const PROGRESS_THRESHOLD: usize = 5;

/// Minimum file size (1 MB) to show per-file analysis progress bar
pub const ANALYSIS_PROGRESS_MIN_SIZE: u64 = 1_000_000;

/// Standard file-count progress bar template used across commands.
const FILE_COUNT_TEMPLATE: &str = "{spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} {msg}";

fn file_count_style() -> ProgressStyle {
    ProgressStyle::default_bar()
        .template(FILE_COUNT_TEMPLATE)
        .unwrap()
        .progress_chars("=>-")
}

pub fn create_progress_bar(total: usize, opts: &Options) -> Option<ProgressBar> {
    if opts.quiet || opts.output_format != OutputFormat::Text || total < PROGRESS_THRESHOLD {
        return None;
    }

    let pb = ProgressBar::new(total as u64);
    pb.set_style(file_count_style());
    Some(pb)
}

/// Same as [`create_progress_bar`], but attaches the bar to an existing
/// `MultiProgress` so it can compose with byte-level analysis bars.
pub fn create_file_count_pb_in(
    mp: &MultiProgress,
    total: usize,
    opts: &Options,
) -> Option<ProgressBar> {
    if opts.quiet || opts.output_format != OutputFormat::Text || total < PROGRESS_THRESHOLD {
        return None;
    }
    let pb = mp.add(ProgressBar::new(total as u64));
    pb.set_style(file_count_style());
    Some(pb)
}

/// Album analysis progress bar attached to a `MultiProgress`.
///
/// In `parallel` mode the bar is file-count driven (`{pos}/{len}`); in
/// sequential mode it tracks bytes within a single file (`{bytes}/{total_bytes}`).
pub fn create_album_progress_pb_in(
    mp: &MultiProgress,
    total: usize,
    parallel: bool,
) -> ProgressBar {
    let initial_len = if parallel { total as u64 } else { 0 };
    let template = if parallel {
        "      [{bar:30.cyan/blue}] {pos}/{len} {msg}"
    } else {
        "      [{bar:30.cyan/blue}] {bytes}/{total_bytes} {msg}"
    };
    let pb = mp.add(ProgressBar::new(initial_len));
    pb.set_style(
        ProgressStyle::default_bar()
            .template(template)
            .unwrap()
            .progress_chars("=>-"),
    );
    pb
}

pub fn progress_set_message(pb: &Option<ProgressBar>, msg: &str) {
    if let Some(ref pb) = pb {
        pb.set_message(msg.to_string());
    }
}

pub fn progress_inc(pb: &Option<ProgressBar>) {
    if let Some(ref pb) = pb {
        pb.inc(1);
    }
}

pub fn progress_finish(pb: Option<ProgressBar>) {
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
}

pub fn create_analysis_progress_bar(
    mp: &MultiProgress,
    file: &Path,
    opts: &Options,
) -> Option<ProgressBar> {
    if opts.quiet || opts.output_format != OutputFormat::Text {
        return None;
    }
    let file_size = std::fs::metadata(file).map(|m| m.len()).unwrap_or(0);
    if file_size < ANALYSIS_PROGRESS_MIN_SIZE {
        return None;
    }
    let pb = mp.add(ProgressBar::new(file_size));
    pb.set_style(
        ProgressStyle::default_bar()
            .template("      [{bar:30.cyan/blue}] {bytes}/{total_bytes}")
            .unwrap()
            .progress_chars("=>-"),
    );
    Some(pb)
}

pub fn update_analysis_progress(pb: &Option<ProgressBar>, bytes_read: u64, total_bytes: u64) {
    if let Some(ref pb) = pb {
        pb.set_length(total_bytes);
        pb.set_position(bytes_read);
    }
}

pub fn finish_analysis_progress(pb: Option<ProgressBar>) {
    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
}
