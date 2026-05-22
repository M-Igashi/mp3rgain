use anyhow::Result;
use colored::*;
use mp3rgain::apply::{apply_with_options, ApplyOptions, ClippingDetection};
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
    // still needs to be reflected in the "would apply N steps" line.
    if opts.dry_run {
        let (actual_steps, warning_msg) =
            dry_run_clipping_summary(file, steps, opts, is_aac, dry_run_prefix, filename);
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
            let warning_msg =
                emit_clipping_warning_headroom(steps, &report, opts, dry_run_prefix, filename);

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

/// Recompute the headroom-based clipping cap purely for the dry-run
/// branch. Mirrors [`mp3rgain::apply::apply_with_options`]'s check.
fn dry_run_clipping_summary(
    file: &Path,
    steps: i32,
    opts: &Options,
    is_aac: bool,
    dry_run_prefix: &str,
    filename: &str,
) -> (i32, Option<String>) {
    if steps <= 0 || opts.wrap_gain {
        return (steps, None);
    }

    let headroom = if is_aac {
        #[cfg(feature = "aac")]
        {
            mp3rgain::aac::analyze_aac_gains(file)
                .ok()
                .map(|a| 255u8.saturating_sub(a.max_gain()) as i32)
        }
        #[cfg(not(feature = "aac"))]
        {
            let _ = file;
            None
        }
    } else {
        analyze(file).ok().map(|i| i.headroom_steps())
    };

    let Some(headroom_steps) = headroom else {
        return (steps, None);
    };
    if steps <= headroom_steps {
        return (steps, None);
    }
    if opts.prevent_clipping {
        let actual = headroom_steps;
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            eprintln!(
                "  {} {}{} - gain reduced from {} to {} steps to prevent clipping",
                "!".yellow(),
                dry_run_prefix,
                filename,
                steps,
                actual
            );
        }
        return (
            actual,
            Some(format!(
                "gain reduced from {} to {} steps to prevent clipping",
                steps, actual
            )),
        );
    }
    if !opts.ignore_clipping && !opts.quiet {
        if opts.output_format == OutputFormat::Text {
            eprintln!(
                "  {} {}{} - clipping warning: requested {} steps but only {} headroom",
                "!".yellow(),
                dry_run_prefix,
                filename,
                steps,
                headroom_steps
            );
            eprintln!("      Use -c to ignore clipping warnings or -k to prevent clipping");
        }
        return (
            steps,
            Some(format!(
                "clipping warning: requested {} steps but only {} headroom",
                steps, headroom_steps
            )),
        );
    }
    (steps, None)
}

/// Render the user-visible clipping warning after a real apply, using the
/// headroom diagnostic from [`ApplyReport`].
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
