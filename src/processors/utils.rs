use colored::*;
use indicatif::ProgressBar;
use mp3rgain::apply::ClippingDetection;
use mp3rgain::mp4meta;
use mp3rgain::replaygain::{self, AudioFileType, ReplayGainResult};
use std::path::Path;
use std::time::SystemTime;

pub use mp3rgain::apply::restore_timestamp;

use crate::cli::options::{Options, OutputFormat};
use crate::json_output::{FileStatus, JsonFileResult};

/// One yellow `!` warning line for `filename` in text mode (never under
/// `-q`), with an optional indented hint beneath it. Every per-file warning
/// (clipping, saturation, tags-only cap, multi-track AAC) goes through here
/// so the stderr shape stays uniform instead of each emitter re-spelling
/// the format string.
pub fn emit_file_warning(opts: &Options, filename: &str, msg: &str, hint: Option<&str>) {
    if opts.output_format != OutputFormat::Text || opts.quiet {
        return;
    }
    eprintln!(
        "  {} {}{} - {}",
        "!".yellow(),
        opts.dry_run_prefix(),
        filename,
        msg
    );
    if let Some(hint) = hint {
        eprintln!("      {}", hint);
    }
}

/// Shared error arm for the per-file processors: red stderr line in text and
/// TSV mode, plus the JSON error record. TSV rows go to stdout, so the stderr
/// line cannot corrupt the stream, and staying silent left a failing file with
/// no explanation at all.
///
/// A file whose *format* mp3rgain cannot process (ALAC, DRM-protected M4P) is
/// reported as a skip instead, in yellow, and does not set the exit code
/// (issue #330): it is not a genuine failure, and letting one such file in a
/// library make `-R -r` exit non-zero tells a script the whole run failed.
pub fn report_file_error(
    file: &Path,
    filename: &str,
    e: mp3rgain::Error,
    opts: &Options,
) -> JsonFileResult {
    if e.is_unsupported_format() {
        return report_unsupported_format(file, filename, &e.to_string(), opts);
    }
    if opts.output_format != OutputFormat::Json && !opts.quiet {
        eprintln!("  {} {} - {}", "x".red(), filename, e);
    }
    JsonFileResult::error(file, e)
}

/// The skipped-file record and warning line for an unsupported format. Split
/// out so the album path, whose analysis failures arrive as strings rather
/// than [`mp3rgain::Error`] values, reports them identically.
pub fn report_unsupported_format(
    file: &Path,
    filename: &str,
    reason: &str,
    opts: &Options,
) -> JsonFileResult {
    emit_file_warning(opts, filename, &format!("{reason} - skipped"), None);
    JsonFileResult {
        file: file.display().to_string(),
        status: Some(FileStatus::Skipped),
        warning: Some(reason.to_string()),
        ..Default::default()
    }
}

/// Analyze one track with the selected analysis mode, driving the byte-level
/// analysis bar when present.
pub fn analyze_track(
    file: &Path,
    opts: &Options,
    analysis_pb: Option<&ProgressBar>,
) -> mp3rgain::Result<ReplayGainResult> {
    let on_progress = analysis_pb.map(|pb| {
        move |bytes, total| {
            pb.set_length(total);
            pb.set_position(bytes);
        }
    });
    replaygain::analyze_track_with_options(
        file,
        &replaygain::TrackAnalysisOptions {
            track_index: opts.track_index,
            mode: opts.analysis_mode,
            true_peak: opts.true_peak,
            chunk: opts.chunk_tracks,
            on_progress: on_progress
                .as_ref()
                .map(|cb| cb as &(dyn Fn(u64, u64) + Sync)),
        },
    )
}

/// `-s R` (issue #298): build an analysis result from the file's stored
/// `REPLAYGAIN_TRACK_*` tags, or `None` when the tags are absent, malformed,
/// unreadable, or untrusted (see [`Options::stored_tags_usable`]) — the
/// caller then falls back to a real analysis. A `REPLAYGAIN_ALGORITHM` tag
/// marks BS.1770 values, which don't match the RG1 target this path trusts.
pub fn stored_track_result(file: &Path, opts: &Options) -> Option<ReplayGainResult> {
    if !opts.stored_tags_usable() {
        return None;
    }
    let tags = mp3rgain::read_gain_tags_auto(file, opts.tag_layout).ok()?;
    let (gain_db, peak) = tags.rg1_track_values()?;
    Some(ReplayGainResult::from_stored_tags(
        gain_db,
        peak,
        AudioFileType::from_path(file),
        opts.analysis_mode,
    ))
}

pub fn save_original_mtime(file: &Path, opts: &Options) -> Option<SystemTime> {
    mp3rgain::apply::read_mtime_if(file, opts.preserve_timestamp && !opts.dry_run)
}

/// Render the user-visible clipping warning after a real or predicted apply.
///
/// Handles both diagnostics from [`mp3rgain::ApplyReport`]: headroom-based
/// (`-g`) and ReplayGain-peak based (`-r`/`-a`, which pass the track's
/// analysis peak as `track_peak` for the prevented-clipping detail).
pub fn emit_clipping_warning(
    requested_steps: i32,
    report: &mp3rgain::ApplyReport,
    opts: &Options,
    filename: &str,
    track_peak: Option<f64>,
) -> Option<String> {
    let (prevented_detail, warn_msg) = match report.clipping_detected {
        Some(ClippingDetection::Headroom(headroom_steps)) => (
            String::new(),
            format!(
                "clipping warning: requested {} steps but only {} headroom",
                requested_steps, headroom_steps
            ),
        ),
        Some(ClippingDetection::Peak(new_peak)) => (
            track_peak
                .map(|p| format!(" (peak: {:.4})", p))
                .unwrap_or_default(),
            format!("clipping warning: peak would be {:.2} (>1.00)", new_peak),
        ),
        None => return None,
    };

    if report.clipping_prevented {
        let msg = format!(
            "gain reduced from {} to {} steps to prevent clipping{}",
            requested_steps, report.actual_steps, prevented_detail
        );
        emit_file_warning(opts, filename, &msg, None);
        return Some(msg);
    }
    if opts.ignore_clipping || opts.quiet {
        return None;
    }
    emit_file_warning(
        opts,
        filename,
        &warn_msg,
        Some("Use -c to ignore clipping warnings or -k to prevent clipping"),
    );
    Some(warn_msg)
}

pub fn warn_aac_multi_track(file: &Path, filename: &str, opts: &Options) {
    if opts.output_format != OutputFormat::Text || opts.quiet {
        return;
    }
    let track_count = mp4meta::count_audio_tracks(file);
    if track_count > 1 {
        emit_file_warning(
            opts,
            filename,
            &format!(
                "{} audio tracks detected, processing first track only",
                track_count
            ),
            None,
        );
    }
}
