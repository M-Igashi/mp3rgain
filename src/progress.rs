use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::path::Path;

use crate::cli::options::{Options, OutputFormat};

pub const PROGRESS_THRESHOLD: usize = 5;

/// Minimum file size (1 MB) to show per-file analysis progress bar
pub const ANALYSIS_PROGRESS_MIN_SIZE: u64 = 1_000_000;

pub fn create_progress_bar(total: usize, opts: &Options) -> Option<ProgressBar> {
    if opts.quiet || opts.output_format != OutputFormat::Text || total < PROGRESS_THRESHOLD {
        return None;
    }

    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    Some(pb)
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
