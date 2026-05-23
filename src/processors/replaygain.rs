use anyhow::Result;
use colored::*;
use indicatif::ProgressBar;
use mp3rgain::apply::{apply_with_options, ApplyOptions, ClippingDetection};
use mp3rgain::replaygain::{self, AudioFileType, ReplayGainResult};
use mp3rgain::{apply_gain_to_peak, mp4meta, steps_to_db, AacAlbumInfo};
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::progress::update_analysis_progress;
use crate::util::get_filename;

use super::utils::{restore_timestamp, save_original_mtime, warn_aac_multi_track};

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
    let is_aac = result.file_type() == AudioFileType::Aac;

    // Dry run: don't actually modify. Clipping prevention still needs to
    // be reflected in the "would apply N steps" message, so mirror the
    // ReplayGain-peak cap here without touching the file.
    if opts.dry_run {
        let (actual_steps, warning_msg) =
            dry_run_clipping_summary(steps, result, opts, dry_run_prefix, filename);
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

    // AAC keeps a fail-soft tag-writing path: bitstream gain failures
    // print a warning but still let us record ReplayGain tags. Branch
    // before calling into the unified pipeline so we can apply that
    // policy.
    if is_aac {
        return apply_replaygain_aac_with_album_into(
            file,
            steps,
            result,
            opts,
            album_info,
            dry_run_prefix,
            out,
        );
    }

    // MP3 path: hand the whole pipeline to apply_with_options.
    let mut apply_opts = ApplyOptions::new(steps);
    apply_opts.track_result = Some(result.clone());
    apply_opts.album_info = album_info.copied();
    apply_opts.prevent_clipping = opts.prevent_clipping;
    apply_opts.wrap = opts.wrap_gain;
    apply_opts.preserve_timestamp = opts.preserve_timestamp;
    apply_opts.use_temp_file = opts.use_temp_file;
    // RG path writes undo unless -s s.
    apply_opts.write_undo = opts.stored_tag_mode != StoredTagMode::Skip;
    apply_opts.write_replaygain_tags = opts.use_id3v2;
    apply_opts.use_id3v2 = opts.use_id3v2;

    match apply_with_options(file, &apply_opts) {
        Ok(report) => {
            let warning_msg =
                emit_clipping_warning_peak(steps, result, &report, opts, dry_run_prefix, filename);

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                writeln!(
                    out,
                    "  {} {} ({} frames, {:+.1} dB)",
                    "v".green(),
                    filename,
                    report.modified,
                    steps_to_db(report.actual_steps)
                )?;
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Success),
                frames: Some(report.modified),
                loudness_db: Some(result.loudness_db()),
                peak: Some(result.peak()),
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

/// Recompute the ReplayGain-peak clipping cap purely for the dry-run
/// branch (no file writes). Mirrors [`mp3rgain::apply::apply_with_options`]
/// for the same inputs so the displayed "would apply N steps" matches what
/// a real apply would do.
fn dry_run_clipping_summary(
    steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    dry_run_prefix: &str,
    filename: &str,
) -> (i32, Option<String>) {
    if steps <= 0 || opts.wrap_gain {
        return (steps, None);
    }
    let new_peak = apply_gain_to_peak(result.peak(), steps_to_db(steps));
    if new_peak <= 1.0 {
        return (steps, None);
    }
    if opts.prevent_clipping {
        let max_safe_db = mp3rgain::peak_to_headroom_db(result.peak()).unwrap_or(0.0);
        // Floor (not round) to match the real apply path in
        // `mp3rgain::apply::check_clipping` — round() can push the cap above
        // true headroom and re-introduce clipping.
        let actual = ((max_safe_db / mp3rgain::GAIN_STEP_DB).floor() as i32).max(0);
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            eprintln!(
                "  {} {}{} - gain reduced from {} to {} steps to prevent clipping (peak: {:.4})",
                "!".yellow(),
                dry_run_prefix,
                filename,
                steps,
                actual,
                result.peak()
            );
        }
        return (
            actual,
            Some(format!(
                "gain reduced from {} to {} steps to prevent clipping (peak: {:.4})",
                steps,
                actual,
                result.peak()
            )),
        );
    }
    if !opts.ignore_clipping && !opts.quiet {
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
        return (
            steps,
            Some(format!(
                "clipping warning: peak would be {:.2} (>1.00)",
                new_peak
            )),
        );
    }
    (steps, None)
}

/// Render the user-visible clipping warning after a real apply, using the
/// ReplayGain-peak diagnostic from [`ApplyReport`].
fn emit_clipping_warning_peak(
    requested_steps: i32,
    result: &ReplayGainResult,
    report: &mp3rgain::ApplyReport,
    opts: &Options,
    dry_run_prefix: &str,
    filename: &str,
) -> Option<String> {
    let Some(ClippingDetection::Peak(new_peak)) = report.clipping_detected else {
        return None;
    };
    if report.clipping_prevented {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            eprintln!(
                "  {} {}{} - gain reduced from {} to {} steps to prevent clipping (peak: {:.4})",
                "!".yellow(),
                dry_run_prefix,
                filename,
                requested_steps,
                report.actual_steps,
                result.peak()
            );
        }
        return Some(format!(
            "gain reduced from {} to {} steps to prevent clipping (peak: {:.4})",
            requested_steps,
            report.actual_steps,
            result.peak()
        ));
    }
    if opts.ignore_clipping || opts.quiet {
        return None;
    }
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
    Some(format!(
        "clipping warning: peak would be {:.2} (>1.00)",
        new_peak
    ))
}

/// Apply ReplayGain to AAC/M4A files with optional album info.
///
/// Differs from the MP3 path in two ways the unified API can't model
/// directly:
///   - bitstream gain failures are logged and swallowed so we still
///     write the ReplayGain tags (matches pre-issue-#153 behavior),
///   - tag writing always runs (independent of `--stored-tag-mode`).
///
/// We therefore drive the apply step with `write_replaygain_tags=false`
/// and call `mp4meta::write_replaygain_tags` afterwards directly.
fn apply_replaygain_aac_with_album_into(
    file: &Path,
    requested_steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    album_info: Option<&AacAlbumInfo>,
    dry_run_prefix: &str,
    out: &mut String,
) -> Result<JsonFileResult> {
    let filename = get_filename(file);

    warn_aac_multi_track(file, filename, opts, "");

    // mtime must be restored AFTER the ReplayGain tag write, not after the
    // apply step alone — otherwise `mp4meta::write_replaygain_tags` below
    // bumps the timestamp again. So we keep mtime handling on this side
    // and tell apply_with_options to leave it alone.
    let original_mtime = save_original_mtime(file, opts);

    let mut apply_opts = ApplyOptions::new(requested_steps);
    apply_opts.track_result = Some(result.clone());
    apply_opts.album_info = album_info.copied();
    apply_opts.prevent_clipping = opts.prevent_clipping;
    apply_opts.wrap = opts.wrap_gain;
    apply_opts.preserve_timestamp = false;
    apply_opts.use_temp_file = opts.use_temp_file;
    // AAC tag writing is fail-soft, so we drive it ourselves below.
    apply_opts.write_replaygain_tags = false;

    let mut actual_steps = requested_steps;
    let mut warning_msg: Option<String> = None;
    let mut gain_modified: usize = 0;

    // apply_with_options handles temp file, mtime restore, and the
    // clipping check. We only swallow bitstream errors so we can still
    // record the ReplayGain tags afterwards.
    match apply_with_options(file, &apply_opts) {
        Ok(report) => {
            actual_steps = report.actual_steps;
            gain_modified = report.modified;
            warning_msg = emit_clipping_warning_peak(
                requested_steps,
                result,
                &report,
                opts,
                dry_run_prefix,
                filename,
            );
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!(
                    "  {} {} - bitstream gain failed: {} (tags still written)",
                    "!".yellow(),
                    filename,
                    e
                );
            }
        }
    }

    let mut tags = mp4meta::ReplayGainTags::default();
    tags.set_track(result.gain_db(), result.peak());
    if let Some(album) = album_info {
        tags.set_album(album.album_gain_db, album.album_peak);
    }

    match mp4meta::write_replaygain_tags(file, &tags) {
        Ok(()) => {
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
