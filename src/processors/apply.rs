use anyhow::Result;
use colored::*;
use mp3rgain::{aac, analyze, mp4meta, steps_to_db, Channel, GainOptions, Mp3Analysis};
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::util::get_filename;

use super::utils::{
    apply_with_temp_file, restore_timestamp, save_original_mtime, warn_aac_multi_track,
    write_id3v2_undo_after_apply,
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
    let dry_run_prefix = opts.dry_run_prefix();

    let original_mtime = save_original_mtime(file, opts);

    let is_aac = mp4meta::is_aac_file(file);

    if is_aac {
        warn_aac_multi_track(file, filename, opts, dry_run_prefix);
    }

    // Check for clipping and possibly prevent it
    let mut actual_steps = steps;
    let mut warning_msg: Option<String> = None;

    // For MP3, hold onto the pre-apply analysis so the ID3v2 undo write below
    // can reuse it instead of triggering a second `analyze(file)` on the same
    // bytes (issue #135).
    let mut mp3_analysis: Option<Mp3Analysis> = None;

    if steps > 0 && !opts.wrap_gain {
        let headroom = if is_aac {
            aac::analyze_aac_gains(file)
                .ok()
                .map(|a| (255u8.saturating_sub(a.max_gain())) as i32)
        } else {
            let info = analyze(file).ok();
            let headroom = info.as_ref().map(|i| i.headroom_steps());
            mp3_analysis = info;
            headroom
        };

        if let Some(headroom_steps) = headroom {
            if steps > headroom_steps {
                if opts.prevent_clipping {
                    let original_steps = steps;
                    actual_steps = headroom_steps;
                    if opts.output_format == OutputFormat::Text && !opts.quiet {
                        eprintln!(
                            "  {} {}{} - gain reduced from {} to {} steps to prevent clipping",
                            "!".yellow(),
                            dry_run_prefix,
                            filename,
                            original_steps,
                            actual_steps
                        );
                    }
                    warning_msg = Some(format!(
                        "gain reduced from {} to {} steps to prevent clipping",
                        original_steps, actual_steps
                    ));
                } else if !opts.ignore_clipping && !opts.quiet {
                    if opts.output_format == OutputFormat::Text {
                        eprintln!(
                            "  {} {}{} - clipping warning: requested {} steps but only {} headroom",
                            "!".yellow(),
                            dry_run_prefix,
                            filename,
                            steps,
                            headroom_steps
                        );
                        eprintln!(
                            "      Use -c to ignore clipping warnings or -k to prevent clipping"
                        );
                    }
                    warning_msg = Some(format!(
                        "clipping warning: requested {} steps but only {} headroom",
                        steps, headroom_steps
                    ));
                }
            }
        }
    }

    // Dry run: don't actually modify
    if opts.dry_run {
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

    // Apply gain
    let apply_result = if is_aac {
        if opts.stored_tag_mode == StoredTagMode::Skip {
            apply_with_temp_file(file, |f| Ok(aac::apply_aac_gain(f, actual_steps)?), opts)
        } else {
            apply_with_temp_file(
                file,
                |f| Ok(aac::apply_aac_gain_with_undo(f, actual_steps)?),
                opts,
            )
        }
    } else if opts.use_id3v2 {
        // MP3 with -s i: apply gain without APE undo, write undo to ID3v2
        let skip_undo = opts.stored_tag_mode == StoredTagMode::Skip;
        // Materialize analysis lazily so unwrap-mode skip path stays free.
        if !skip_undo && mp3_analysis.is_none() {
            mp3_analysis = analyze(file).ok();
        }
        let result = apply_with_temp_file(
            file,
            |f| {
                Ok(GainOptions::new(actual_steps)
                    .wrap(opts.wrap_gain)
                    .undo(false)
                    .apply(f)?)
            },
            opts,
        );
        if !skip_undo && result.is_ok() {
            write_id3v2_undo_after_apply(
                file,
                actual_steps,
                actual_steps,
                opts.wrap_gain,
                mp3_analysis.as_ref(),
            )?;
        }
        result
    } else {
        let use_undo = opts.stored_tag_mode != StoredTagMode::Skip;
        apply_with_temp_file(
            file,
            |f| {
                Ok(GainOptions::new(actual_steps)
                    .wrap(opts.wrap_gain)
                    .undo(use_undo)
                    .apply(f)?)
            },
            opts,
        )
    };

    match apply_result {
        Ok(modified) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                if is_aac {
                    writeln!(
                        out,
                        "  {} {} ({} gains modified)",
                        "v".green(),
                        filename,
                        modified
                    )?;
                } else {
                    writeln!(out, "  {} {} ({} frames)", "v".green(), filename, modified)?;
                }
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Success),
                frames: Some(modified),
                gain_applied_steps: Some(actual_steps),
                gain_applied_db: Some(steps_to_db(actual_steps)),
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
    let channel_name = match channel {
        Channel::Left => "left",
        Channel::Right => "right",
        _ => unreachable!(),
    };

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

    let original_mtime = save_original_mtime(file, opts);

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

    let apply_result = if opts.use_id3v2 {
        // Apply gain without APE undo, then write undo to ID3v2.
        // Capture the pre-apply analysis so the undo write can reuse min/max
        // without a second file scan (issue #135).
        let pre_analysis = analyze(file).ok();
        let result = GainOptions::new(steps)
            .channel(channel)
            .undo(false)
            .apply(file);
        if result.is_ok() {
            let (delta_left, delta_right) = match channel {
                Channel::Left => (steps, 0),
                Channel::Right => (0, steps),
                _ => unreachable!(),
            };
            write_id3v2_undo_after_apply(
                file,
                delta_left,
                delta_right,
                false,
                pre_analysis.as_ref(),
            )?;
        }
        result
    } else {
        GainOptions::new(steps)
            .channel(channel)
            .undo(true)
            .apply(file)
    };

    match apply_result {
        Ok(frames) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                writeln!(
                    out,
                    "  {} {} ({} frames, {} channel)",
                    "v".green(),
                    filename,
                    frames,
                    channel_name
                )?;
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Success),
                frames: Some(frames),
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
