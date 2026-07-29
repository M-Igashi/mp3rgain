//! Progress reporting helpers used during ReplayGain analysis.
//!
//! Per-file analysis progress was originally requested by @Sappharad in
//! #106 to mirror the byte-level progress output of the legacy mp3gain CLI
//! ("`54% of 119337493 bytes analyzed`"). The feature shipped in v2.2.0
//! once the Symphonia v0.6 decoder migration landed.

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::cell::Cell;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat};

pub const PROGRESS_THRESHOLD: usize = 5;

/// Minimum file size (1 MB) to show per-file analysis progress bar
pub const ANALYSIS_PROGRESS_MIN_SIZE: u64 = 1_000_000;

/// Standard file-count progress bar template used across commands.
const FILE_COUNT_TEMPLATE: &str = "{spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} {msg}";

/// Build a bar style from a template, using the shared progress characters.
fn bar_style(template: &str) -> ProgressStyle {
    ProgressStyle::default_bar()
        .template(template)
        .unwrap()
        .progress_chars("=>-")
}

/// Whether a file-count progress bar should be shown for `total` files.
fn file_count_enabled(total: usize, opts: &Options) -> bool {
    !opts.quiet && opts.output_format == OutputFormat::Text && total >= PROGRESS_THRESHOLD
}

pub fn create_progress_bar(total: usize, opts: &Options) -> Option<ProgressBar> {
    if !file_count_enabled(total, opts) {
        return None;
    }
    let pb = ProgressBar::new(total as u64);
    pb.set_style(bar_style(FILE_COUNT_TEMPLATE));
    Some(pb)
}

/// Same as [`create_progress_bar`], but attaches the bar to an existing
/// `MultiProgress` so it can compose with byte-level analysis bars.
pub fn create_file_count_pb_in(
    mp: &MultiProgress,
    total: usize,
    opts: &Options,
) -> Option<ProgressBar> {
    if !file_count_enabled(total, opts) {
        return None;
    }
    let pb = mp.add(ProgressBar::new(total as u64));
    pb.set_style(bar_style(FILE_COUNT_TEMPLATE));
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
    pb.set_style(bar_style(template));
    pb
}

/// `on_progress` / `on_complete` closure pair driving an optional shared
/// album-analysis progress bar (see [`create_album_progress_pb_in`]).
///
/// Serial analysis paths call `on_progress` with byte-level progress;
/// parallel paths call `on_complete` with a completed-file count. A new
/// message string is only allocated when the current file changes — the
/// progress callback runs once per decoded packet (~9k calls per minute
/// of audio).
pub fn album_progress_callbacks<'a>(
    pb: &Option<ProgressBar>,
    file_names: Vec<&'a str>,
) -> (
    impl Fn(usize, u64, u64) + 'a,
    impl Fn(usize, &Path) + Sync + 'a,
) {
    let total = file_names.len();
    let pb_for_progress = pb.clone();
    let last_message_idx: Cell<Option<usize>> = Cell::new(None);
    let on_progress = move |file_idx: usize, bytes: u64, total_bytes: u64| {
        if let Some(pb) = &pb_for_progress {
            pb.set_length(total_bytes);
            pb.set_position(bytes);
            if last_message_idx.get() != Some(file_idx) {
                pb.set_message(format!(
                    "({}/{}) {}",
                    file_idx + 1,
                    total,
                    file_names[file_idx]
                ));
                last_message_idx.set(Some(file_idx));
            }
        }
    };
    let pb_for_complete = pb.clone();
    let on_complete = move |completed_idx: usize, _path: &Path| {
        if let Some(pb) = &pb_for_complete {
            pb.set_position((completed_idx + 1) as u64);
        }
    };
    (on_progress, on_complete)
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
    pb.set_style(bar_style(
        "      [{bar:30.cyan/blue}] {bytes}/{total_bytes}",
    ));
    Some(pb)
}
