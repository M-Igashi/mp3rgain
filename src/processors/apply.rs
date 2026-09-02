use anyhow::Result;
use colored::*;
use mp3rgain::apply::{apply_with_options, predict_apply, ApplyOptions};
use mp3rgain::replaygain::AudioFileType;
use mp3rgain::{mp4meta, steps_to_db, Channel};
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::util::get_filename;

use super::utils::{
    emit_clipping_warning, emit_file_warning, report_file_error, warn_aac_multi_track,
};

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

    // Detected once here and handed to the pipeline, which used to redo the
    // MP4 probe (an open plus a `moov` parse) on its own.
    let is_aac = mp4meta::is_aac_file(file);
    let file_type = Some(if is_aac {
        AudioFileType::Aac
    } else {
        AudioFileType::Mp3
    });
    if is_aac {
        warn_aac_multi_track(file, filename, opts);
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
                apply_opts.file_type = file_type;
                match predict_apply(file, &apply_opts) {
                    Ok(report) => {
                        let warning = emit_clipping_warning(steps, &report, opts, filename, None);
                        (report.actual_steps, warning)
                    }
                    Err(e) => {
                        return Ok(JsonFileResult {
                            dry_run: Some(true),
                            ..report_file_error(file, filename, e, opts)
                        });
                    }
                }
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
    apply_opts.write_undo = opts.stored_tag_mode != StoredTagMode::Skip;
    apply_opts.tag_layout = opts.tag_layout;
    // Same reasoning as the dry-run branch above: without -k the steps are
    // never capped, and under -c/-q the clipping warning is never emitted, so
    // the headroom analyze inside check_clipping feeds nothing observable
    // (issue #232).
    apply_opts.skip_clipping_check = !opts.prevent_clipping && (opts.ignore_clipping || opts.quiet);
    apply_opts.file_type = file_type;

    match apply_with_options(file, &apply_opts) {
        Ok(report) => {
            let clip_warn = emit_clipping_warning(steps, &report, opts, filename, None);
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
                max_gain: report.gain_range.map(|(max, _)| max),
                min_gain: report.gain_range.map(|(_, min)| min),
                warning: warning_msg,
                ..Default::default()
            })
        }
        Err(e) => Ok(report_file_error(file, filename, e, opts)),
    }
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
    emit_file_warning(opts, filename, &msg, None);
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

    // Warn if file is Joint Stereo (mp3gain only supports Stereo for -l).
    // One frame header answers that; the whole-file analyze() it replaced
    // walked every frame for statistics that were then thrown away.
    if opts.output_format == OutputFormat::Text && !opts.quiet {
        if let Ok(mp3rgain::ChannelMode::JointStereo) = mp3rgain::analysis::read_channel_mode(file)
        {
            emit_file_warning(
                opts,
                filename,
                "Joint Stereo file: channel-specific gain may not work as expected",
                None,
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
    apply_opts.write_undo = opts.stored_tag_mode != StoredTagMode::Skip;
    apply_opts.tag_layout = opts.tag_layout;
    // The channel path never surfaces ApplyReport::clipping_detected and -l
    // has no clipping prevention, so the headroom analyze inside
    // check_clipping is pure waste (issue #232).
    apply_opts.skip_clipping_check = true;
    // -l is MP3-only (the pipeline rejects AAC), so skip the container probe.
    apply_opts.file_type = Some(AudioFileType::Mp3);

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
        Err(e) => Ok(report_file_error(file, filename, e, opts)),
    }
}
