use anyhow::Result;
use colored::*;
use mp3rgain::{aac, analyze, id3v2, mp4meta, steps_to_db, Channel, GainOptions};
use std::path::Path;

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::get_filename;
use crate::json_output::JsonFileResult;

use super::utils::{apply_with_temp_file, restore_timestamp};

pub fn process_apply(file: &Path, steps: i32, opts: &Options) -> Result<JsonFileResult> {
    let filename = get_filename(file);
    let dry_run_prefix = opts.dry_run_prefix();

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp && !opts.dry_run {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    let is_aac = mp4meta::is_aac_file(file);

    // Multi-track warning for AAC files
    if is_aac && opts.output_format == OutputFormat::Text && !opts.quiet {
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

    // Check for clipping and possibly prevent it
    let mut actual_steps = steps;
    let mut warning_msg: Option<String> = None;

    if steps > 0 && !opts.wrap_gain {
        let headroom = if is_aac {
            aac::analyze_aac_gains(file)
                .ok()
                .map(|a| (255u8.saturating_sub(a.max_gain())) as i32)
        } else {
            analyze(file).ok().map(|info| info.headroom_steps())
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
            println!(
                "  {} [DRY RUN] {} (would apply {} steps)",
                "~".cyan(),
                filename,
                actual_steps
            );
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some("dry_run".to_string()),
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
            let analysis = analyze(file)?;
            let existing_rg = id3v2::read_id3v2_replaygain(file).unwrap_or_default();
            let (existing_left, _) = mp3rgain::ape::parse_undo_values(existing_rg.undo.as_deref());
            let new_undo = existing_left + actual_steps;
            id3v2::write_id3v2_undo(
                file,
                new_undo,
                new_undo,
                opts.wrap_gain,
                analysis.min_gain(),
                analysis.max_gain(),
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
                    println!(
                        "  {} {} ({} gains modified)",
                        "v".green(),
                        filename,
                        modified
                    );
                } else {
                    println!("  {} {} ({} frames)", "v".green(), filename, modified);
                }
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("success".to_string()),
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
                status: Some("error".to_string()),
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

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp && !opts.dry_run {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    // Dry run: don't actually modify
    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            println!(
                "  {} [DRY RUN] {} (would apply {} steps to {} channel)",
                "~".cyan(),
                filename,
                steps,
                channel_name
            );
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some("dry_run".to_string()),
            gain_applied_steps: Some(steps),
            gain_applied_db: Some(steps_to_db(steps)),
            dry_run: Some(true),
            ..Default::default()
        });
    }

    let apply_result = if opts.use_id3v2 {
        // Apply gain without APE undo, then write undo to ID3v2
        let result = GainOptions::new(steps)
            .channel(channel)
            .undo(false)
            .apply(file);
        if result.is_ok() {
            let analysis = analyze(file)?;
            let existing_rg = id3v2::read_id3v2_replaygain(file).unwrap_or_default();
            let (existing_left, existing_right) =
                mp3rgain::ape::parse_undo_values(existing_rg.undo.as_deref());
            let (new_left, new_right) = match channel {
                Channel::Left => (existing_left + steps, existing_right),
                Channel::Right => (existing_left, existing_right + steps),
                _ => unreachable!(),
            };
            id3v2::write_id3v2_undo(
                file,
                new_left,
                new_right,
                false,
                analysis.min_gain(),
                analysis.max_gain(),
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
                println!(
                    "  {} {} ({} frames, {} channel)",
                    "v".green(),
                    filename,
                    frames,
                    channel_name
                );
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("success".to_string()),
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
                status: Some("error".to_string()),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}
