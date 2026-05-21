use anyhow::Result;
use colored::*;
use indicatif::ProgressBar;
use mp3rgain::replaygain::{self, AudioFileType, ReplayGainResult};
use mp3rgain::{aac, db_to_steps, id3v2, mp4meta, peak_to_headroom_db, steps_to_db, GainOptions};
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{AacAlbumInfo, Options, OutputFormat};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::progress::update_analysis_progress;
use crate::util::get_filename;

use super::utils::{
    apply_with_temp_file, restore_timestamp, save_original_mtime, warn_aac_multi_track,
    write_id3v2_undo_after_apply,
};

pub fn process_track_gain(
    file: &Path,
    opts: &Options,
    analysis_pb: Option<&ProgressBar>,
) -> Result<(JsonFileResult, String)> {
    let mut out = String::new();
    let result = process_track_gain_into(file, opts, analysis_pb, &mut out)?;
    Ok((result, out))
}

fn process_track_gain_into(
    file: &Path,
    opts: &Options,
    analysis_pb: Option<&ProgressBar>,
    out: &mut String,
) -> Result<JsonFileResult> {
    let filename = get_filename(file);
    let dry_run_prefix = opts.dry_run_prefix();

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        writeln!(
            out,
            "  {} {}Analyzing {}...",
            "->".cyan(),
            dry_run_prefix,
            filename
        )?;
    }

    let rg_result = if let Some(pb) = analysis_pb {
        replaygain::analyze_track_with_progress(file, opts.track_index, &|bytes, total| {
            update_analysis_progress(&Some(pb.clone()), bytes, total);
        })
    } else {
        replaygain::analyze_track_with_index(file, opts.track_index)
    };

    match rg_result {
        Ok(result) => {
            // Apply gain modifier (-m steps + -d dB, combined into steps)
            let base_steps = result.gain_steps();
            let modifier_steps = opts.gain_modifier_steps();
            let modified_steps = base_steps + modifier_steps;

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                writeln!(
                    out,
                    "      Loudness: {:.1} dB, Gain: {:+.1} dB ({} steps{}), Peak: {:.4}",
                    result.loudness_db(),
                    result.gain_db(),
                    base_steps,
                    if modifier_steps != 0 {
                        format!(" + {} = {}", modifier_steps, modified_steps)
                    } else {
                        String::new()
                    },
                    result.peak()
                )?;
            }

            if modified_steps == 0 {
                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    writeln!(out, "  {} {} (no adjustment needed)", ".".cyan(), filename)?;
                }
                return Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some(FileStatus::Skipped),
                    loudness_db: Some(result.loudness_db()),
                    peak: Some(result.peak()),
                    gain_applied_steps: Some(0),
                    gain_applied_db: Some(0.0),
                    ..Default::default()
                });
            }

            apply_replaygain_with_album_into(file, modified_steps, &result, opts, None, out)
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

pub fn process_apply_replaygain_with_album(
    file: &Path,
    steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    album_info: Option<&AacAlbumInfo>,
) -> Result<(JsonFileResult, String)> {
    let mut out = String::new();
    let r = apply_replaygain_with_album_into(file, steps, result, opts, album_info, &mut out)?;
    Ok((r, out))
}

fn apply_replaygain_with_album_into(
    file: &Path,
    steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    album_info: Option<&AacAlbumInfo>,
    out: &mut String,
) -> Result<JsonFileResult> {
    let filename = get_filename(file);
    let dry_run_prefix = opts.dry_run_prefix();

    let original_mtime = save_original_mtime(file, opts);

    let mut actual_steps = steps;
    let mut warning_msg: Option<String> = None;

    if steps > 0 && !opts.wrap_gain {
        // Check if applying this gain would cause clipping
        let gain_linear = 10.0_f64.powf(result.gain_db() / 20.0);
        let new_peak = result.peak() * gain_linear;
        if new_peak > 1.0 {
            if opts.prevent_clipping {
                // Calculate the maximum safe gain. The outer `new_peak > 1.0`
                // guard implies peak > 0, so headroom is always defined here.
                let max_safe_db = peak_to_headroom_db(result.peak()).unwrap_or(0.0);
                let max_safe_steps = db_to_steps(max_safe_db);
                actual_steps = max_safe_steps.max(0);

                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    eprintln!(
                        "  {} {}{} - gain reduced from {} to {} steps to prevent clipping (peak: {:.4})",
                        "!".yellow(),
                        dry_run_prefix,
                        filename,
                        steps,
                        actual_steps,
                        result.peak()
                    );
                }
                warning_msg = Some(format!(
                    "gain reduced from {} to {} steps to prevent clipping (peak: {:.4})",
                    steps,
                    actual_steps,
                    result.peak()
                ));
            } else if !opts.ignore_clipping && !opts.quiet {
                if opts.output_format == OutputFormat::Text {
                    eprintln!(
                        "  {} {}{} - clipping warning: peak would be {:.2} (>{:.2})",
                        "!".yellow(),
                        dry_run_prefix,
                        filename,
                        new_peak,
                        1.0
                    );
                    eprintln!("      Use -c to ignore clipping warnings or -k to prevent clipping");
                }
                warning_msg = Some(format!(
                    "clipping warning: peak would be {:.2} (>1.00)",
                    new_peak
                ));
            }
        }
    }

    // Dry run: don't actually modify
    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            writeln!(
                out,
                "  {} [DRY RUN] {} (would apply {:+.1} dB, {} steps)",
                "~".cyan(),
                filename,
                steps_to_db(actual_steps),
                actual_steps,
            )?;
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some(FileStatus::DryRun),
            loudness_db: Some(result.loudness_db()),
            peak: Some(result.peak()),
            gain_applied_steps: Some(actual_steps),
            gain_applied_db: Some(steps_to_db(actual_steps)),
            warning: warning_msg,
            dry_run: Some(true),
            ..Default::default()
        });
    }

    // Handle AAC/M4A files differently - only write ReplayGain tags
    if result.file_type() == AudioFileType::Aac {
        let ctx = AacApplyContext {
            warning_msg,
            original_mtime,
            album_info,
        };
        return apply_replaygain_aac_with_album_into(file, actual_steps, result, opts, ctx, out);
    }

    // MP3: Apply gain to audio frames
    let use_id3v2_undo = opts.use_id3v2;
    // Pre-apply analysis is reused for the ID3v2 undo write below so we don't
    // re-read the file just to fetch min/max (issue #135).
    let pre_analysis = if use_id3v2_undo {
        mp3rgain::analyze(file).ok()
    } else {
        None
    };
    let apply_result = apply_with_temp_file(
        file,
        |r, w| {
            Ok(GainOptions::new(actual_steps)
                .wrap(opts.wrap_gain)
                .undo(!use_id3v2_undo) // APE undo only when not using ID3v2
                .apply_to_path(r, w)?)
        },
        opts,
    );

    match apply_result {
        Ok(frames) => {
            // Write ID3v2 tags if -s i mode
            if opts.use_id3v2 {
                write_id3v2_undo_after_apply(
                    file,
                    actual_steps,
                    actual_steps,
                    opts.wrap_gain,
                    pre_analysis.as_ref(),
                )?;

                // Write ReplayGain metadata tags
                let rg = mp3rgain::Id3v2ReplayGain {
                    track_gain: Some(format!("{:+.2} dB", result.gain_db())),
                    track_peak: Some(format!("{:.6}", result.peak())),
                    album_gain: album_info.map(|a| format!("{:+.2} dB", a.album_gain_db)),
                    album_peak: album_info.map(|a| format!("{:.6}", a.album_peak)),
                    ..Default::default()
                };
                id3v2::write_id3v2_replaygain(file, &rg)?;
            }

            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                writeln!(
                    out,
                    "  {} {} ({} frames, {:+.1} dB)",
                    "v".green(),
                    filename,
                    frames,
                    steps_to_db(actual_steps)
                )?;
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Success),
                frames: Some(frames),
                loudness_db: Some(result.loudness_db()),
                peak: Some(result.peak()),
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

struct AacApplyContext<'a> {
    warning_msg: Option<String>,
    original_mtime: Option<std::time::SystemTime>,
    album_info: Option<&'a AacAlbumInfo>,
}

/// Apply ReplayGain to AAC/M4A files with optional album info
fn apply_replaygain_aac_with_album_into(
    file: &Path,
    actual_steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    ctx: AacApplyContext<'_>,
    out: &mut String,
) -> Result<JsonFileResult> {
    let AacApplyContext {
        warning_msg,
        original_mtime,
        album_info,
    } = ctx;
    let filename = get_filename(file);

    warn_aac_multi_track(file, filename, opts, "");

    let gain_modified = if actual_steps != 0 {
        match aac::apply_aac_gain_with_undo(file, actual_steps) {
            Ok(n) => n,
            Err(e) => {
                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    eprintln!(
                        "  {} {} - bitstream gain failed: {} (tags still written)",
                        "!".yellow(),
                        filename,
                        e
                    );
                }
                0
            }
        }
    } else {
        0
    };

    // Create ReplayGain tags for AAC
    let mut tags = mp4meta::ReplayGainTags::default();
    tags.set_track(result.gain_db(), result.peak());

    // Add album tags if available
    if let Some(album) = album_info {
        tags.set_album(album.album_gain_db, album.album_peak);
    }

    // Write tags to file
    match mp4meta::write_replaygain_tags(file, &tags) {
        Ok(()) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            let tag_type = if album_info.is_some() {
                "track+album tags"
            } else {
                "tags"
            };

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                if gain_modified > 0 {
                    writeln!(
                        out,
                        "  {} {} ({} gains modified + {} written, {:+.1} dB)",
                        "v".green(),
                        filename,
                        gain_modified,
                        tag_type,
                        result.gain_db()
                    )?;
                } else {
                    writeln!(
                        out,
                        "  {} {} ({} written, {:+.1} dB)",
                        "v".green(),
                        filename,
                        tag_type,
                        result.gain_db()
                    )?;
                }
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Success),
                loudness_db: Some(result.loudness_db()),
                peak: Some(result.peak()),
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
