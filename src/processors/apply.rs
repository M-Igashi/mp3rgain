use anyhow::Result;
use colored::*;
use mp3rgain::apply::{apply_with_options, predict_apply, ApplyOptions, ClippingDetection};
use mp3rgain::{analyze, mp4meta, steps_to_db, Channel};
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::util::get_filename;

use super::utils::warn_aac_multi_track;

pub fn process_apply(file: &Path, steps: i32, opts: &Options) -> Result<(JsonFileResult, String)> {
    let mut out = String::new();
    let result = process_apply_into(file, steps, opts, &mut out)?;
    Ok((result, out))
}

fn process_apply_into(
    file: &Path,
    steps: i32,
    opts: &Options,
    out: &mut String,
) -> Result<JsonFileResult> {
    let filename = get_filename(file);
    let dry_run_prefix = opts.dry_run_prefix();

    let is_aac = mp4meta::is_aac_file(file);
    if is_aac {
        warn_aac_multi_track(file, filename, opts, dry_run_prefix);
    }

    // Dry run: don't touch the file. Headroom-based clipping prevention
    // still needs to be reflected in the "would apply N steps" line, so
    // drive the same clipping check through predict_apply.
    //
    // Without -k the steps are never capped, and the clipping warning is
    // only emitted/recorded when neither -c (ignore) nor -q (quiet) is set.
    // In that case the prediction feeds nothing the caller can observe, so
    // skip it — a `--dry-run -c`/`-q` sweep then avoids a full read per file.
    if opts.dry_run {
        let (actual_steps, warning_msg) =
            if !opts.prevent_clipping && (opts.ignore_clipping || opts.quiet) {
                (steps, None)
            } else {
                let mut apply_opts = ApplyOptions::new(steps);
                apply_opts.prevent_clipping = opts.prevent_clipping;
                apply_opts.wrap = opts.wrap_gain;
                let report = predict_apply(file, &apply_opts)?;
                let warning =
                    emit_clipping_warning_headroom(steps, &report, opts, dry_run_prefix, filename);
                (report.actual_steps, warning)
            };
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            writeln!(
                out,
                "  {} [DRY RUN] {} (would apply {} steps)",
                "~".cyan(),
                filename,
                actual_steps
            )?;
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some(FileStatus::DryRun),
            gain_applied_steps: Some(actual_steps),
            gain_applied_db: Some(steps_to_db(actual_steps)),
            warning: warning_msg,
            dry_run: Some(true),
            ..Default::default()
        });
    }

    let mut apply_opts = ApplyOptions::new(steps);
    apply_opts.prevent_clipping = opts.prevent_clipping;
    apply_opts.wrap = opts.wrap_gain;
    apply_opts.preserve_timestamp = opts.preserve_timestamp;
    apply_opts.use_temp_file = opts.use_temp_file;
    apply_opts.write_undo = opts.stored_tag_mode != StoredTagMode::Skip;
    apply_opts.use_id3v2 = opts.use_id3v2;

    match apply_with_options(file, &apply_opts) {
        Ok(report) => {
            let clip_warn =
                emit_clipping_warning_headroom(steps, &report, opts, dry_run_prefix, filename);
            let sat_warn = emit_saturation_warning(&report, opts, filename);
            let warning_msg = combine_warnings(clip_warn, sat_warn);

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                if is_aac {
                    writeln!(
                        out,
                        "  {} {} ({} gains modified)",
                        "v".green(),
                        filename,
                        report.modified
                    )?;
                } else {
                    writeln!(
                        out,
                        "  {} {} ({} frames)",
                        "v".green(),
                        filename,
                        report.modified
                    )?;
                }
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Success),
                frames: Some(report.modified),
                gain_applied_steps: Some(report.actual_steps),
                gain_applied_db: Some(steps_to_db(report.actual_steps)),
                warning: warning_msg,
                ..Default::default()
            })
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!("  {} {} - {}", "x".red(), filename, e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Error),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}

/// Render the user-visible clipping warning after a real or predicted
/// apply, using the headroom diagnostic from [`ApplyReport`].
fn emit_clipping_warning_headroom(
    requested_steps: i32,
    report: &mp3rgain::ApplyReport,
    opts: &Options,
    dry_run_prefix: &str,
    filename: &str,
) -> Option<String> {
    let Some(ClippingDetection::Headroom(headroom_steps)) = report.clipping_detected else {
        return None;
    };
    if report.clipping_prevented {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            eprintln!(
                "  {} {}{} - gain reduced from {} to {} steps to prevent clipping",
                "!".yellow(),
                dry_run_prefix,
                filename,
                requested_steps,
                report.actual_steps
            );
        }
        return Some(format!(
            "gain reduced from {} to {} steps to prevent clipping",
            requested_steps, report.actual_steps
        ));
    }
    if opts.ignore_clipping || opts.quiet {
        return None;
    }
    if opts.output_format == OutputFormat::Text {
        eprintln!(
            "  {} {}{} - clipping warning: requested {} steps but only {} headroom",
            "!".yellow(),
            dry_run_prefix,
            filename,
            requested_steps,
            headroom_steps
        );
        eprintln!("      Use -c to ignore clipping warnings or -k to prevent clipping");
    }
    Some(format!(
        "clipping warning: requested {} steps but only {} headroom",
        requested_steps, headroom_steps
    ))
}

/// Warn when a saturating manual-gain apply clamped global_gain values at
/// the [0, 255] boundary (issue #207). Saturation is lossy — the clamped
/// frames can no longer be byte-restored by undo, so the lossless guarantee
/// silently doesn't hold there. ReplayGain adjustments never get near this
/// range, so the warning only fires for extreme manual `-g` / `-l` values.
fn emit_saturation_warning(
    report: &mp3rgain::ApplyReport,
    opts: &Options,
    filename: &str,
) -> Option<String> {
    let (low, high) = (report.saturated_low, report.saturated_high);
    if low == 0 && high == 0 {
        return None;
    }
    let detail = match (low, high) {
        (l, 0) => format!("{l} gain value(s) clamped at 0 (silence)"),
        (0, h) => format!("{h} gain value(s) clamped at 255 (distortion)"),
        (l, h) => format!("{l} gain value(s) clamped at 0 (silence), {h} at 255 (distortion)"),
    };
    let msg = format!("{detail} - saturated gain is not losslessly reversible");
    if opts.output_format == OutputFormat::Text && !opts.quiet {
        eprintln!("  {} {} - {}", "!".yellow(), filename, msg);
    }
    Some(msg)
}

/// Join optional clipping and saturation warnings into the single
/// [`JsonFileResult::warning`] field.
fn combine_warnings(clip: Option<String>, saturation: Option<String>) -> Option<String> {
    match (clip, saturation) {
        (Some(a), Some(b)) => Some(format!("{a}; {b}")),
        (a, b) => a.or(b),
    }
}

pub fn process_apply_channel(
    file: &Path,
    channel: Channel,
    steps: i32,
    opts: &Options,
) -> Result<(JsonFileResult, String)> {
    let mut out = String::new();
    let result = process_apply_channel_into(file, channel, steps, opts, &mut out)?;
    Ok((result, out))
}

fn process_apply_channel_into(
    file: &Path,
    channel: Channel,
    steps: i32,
    opts: &Options,
    out: &mut String,
) -> Result<JsonFileResult> {
    let filename = get_filename(file);
    let channel_name = channel.name();

    // Warn if file is Joint Stereo (mp3gain only supports Stereo for -l)
    if let Ok(info) = analyze(file) {
        if info.channel_mode() == mp3rgain::ChannelMode::JointStereo
            && opts.output_format == OutputFormat::Text
            && !opts.quiet
        {
            eprintln!(
                "  {} {} - Joint Stereo file: channel-specific gain may not work as expected",
                "!".yellow(),
                filename
            );
        }
    }

    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            writeln!(
                out,
                "  {} [DRY RUN] {} (would apply {} steps to {} channel)",
                "~".cyan(),
                filename,
                steps,
                channel_name
            )?;
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some(FileStatus::DryRun),
            gain_applied_steps: Some(steps),
            gain_applied_db: Some(steps_to_db(steps)),
            dry_run: Some(true),
            ..Default::default()
        });
    }

    let mut apply_opts = ApplyOptions::new(steps);
    apply_opts.channel = Some(channel);
    apply_opts.preserve_timestamp = opts.preserve_timestamp;
    apply_opts.use_temp_file = opts.use_temp_file;
    apply_opts.write_undo = opts.stored_tag_mode != StoredTagMode::Skip;
    apply_opts.use_id3v2 = opts.use_id3v2;

    match apply_with_options(file, &apply_opts) {
        Ok(report) => {
            let warning_msg = emit_saturation_warning(&report, opts, filename);

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                writeln!(
                    out,
                    "  {} {} ({} frames, {} channel)",
                    "v".green(),
                    filename,
                    report.modified,
                    channel_name
                )?;
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Success),
                frames: Some(report.modified),
                gain_applied_steps: Some(steps),
                gain_applied_db: Some(steps_to_db(steps)),
                warning: warning_msg,
                ..Default::default()
            })
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!("  {} {} - {}", "x".red(), filename, e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Error),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}
