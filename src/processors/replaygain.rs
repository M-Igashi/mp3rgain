use anyhow::Result;
use colored::*;
use indicatif::ProgressBar;
use mp3rgain::apply::{apply_with_options, predict_apply, ApplyOptions};
use mp3rgain::replaygain::{AudioFileType, ReplayGainResult};
use mp3rgain::{apply_gain_to_peak, mp4meta, steps_to_db, AacAlbumInfo};
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::json_output::{analysis_mode_str, FileStatus, JsonFileResult};
use crate::util::get_filename;

/// Unit label for a result's loudness value: LUFS for the BS.1770 modes,
/// dB for RG1.
fn loudness_unit(result: &ReplayGainResult) -> &'static str {
    if result.loudness_lufs().is_some() {
        "LUFS"
    } else {
        "dB"
    }
}

use super::utils::{
    analyze_track, emit_clipping_warning, report_file_error, restore_timestamp,
    save_original_mtime, warn_aac_multi_track,
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

    match analyze_track(file, opts, analysis_pb) {
        Ok(result) => {
            // Apply gain modifier (-m steps + -d dB, combined into steps)
            let base_steps = result.gain_steps();
            let modifier_steps = opts.gain_modifier_steps();
            let modified_steps = base_steps + modifier_steps;

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                writeln!(
                    out,
                    "      Loudness: {:.1} {}, Gain: {:+.1} dB ({} steps{}), Peak: {:.4}",
                    result.loudness_db(),
                    loudness_unit(&result),
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

            // A net 0-step adjustment still has work to do when (issue #206):
            //   - ReplayGain analysis tags would be written (AAC always; MP3
            //     in `-s i` mode), so an already-on-target track gets its
            //     REPLAYGAIN_* tags (re)written rather than skipped, and/or
            //   - `-k` must attenuate a track that is already at the
            //     reference loudness yet already clips (peak > 1.0).
            // Only the common already-normalized, non-clipping, no-tags case
            // keeps the cheap "no adjustment needed" skip.
            // RG analysis tags are written for AAC always, and for MP3 in any
            // mode that keeps tags (APE default or `-s i` ID3v2) — only `-s s`
            // skips them (issue #204).
            let writes_rg_tags = result.file_type() == AudioFileType::Aac
                || opts.stored_tag_mode != StoredTagMode::Skip;
            let clip_prevention_applies = opts.prevent_clipping && result.peak() > 1.0;
            if modified_steps == 0 && !writes_rg_tags && !clip_prevention_applies {
                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    writeln!(out, "  {} {} (no adjustment needed)", ".".cyan(), filename)?;
                }
                return Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some(FileStatus::Skipped),
                    loudness_db: Some(result.loudness_db()),
                    loudness_lufs: result.loudness_lufs(),
                    analysis_mode: Some(analysis_mode_str(result.analysis_mode())),
                    peak: Some(result.peak()),
                    gain_applied_steps: Some(0),
                    gain_applied_db: Some(0.0),
                    ..Default::default()
                });
            }

            apply_replaygain_with_album_into(file, modified_steps, &result, opts, None, out)
                .map(|(r, _)| r)
        }
        Err(e) => Ok(report_file_error(file, filename, e, opts)),
    }
}

/// Per-file result, buffered text output, and the post-apply `(max, min)`
/// global_gain range from [`mp3rgain::ApplyReport::gain_range`] so album mode
/// can build `MP3GAIN_ALBUM_MINMAX` without re-analyzing every file
/// (issue #232).
pub type ApplyWithAlbumOutcome = (JsonFileResult, String, Option<(u8, u8)>);

pub fn process_apply_replaygain_with_album(
    file: &Path,
    steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    album_info: Option<&AacAlbumInfo>,
) -> Result<ApplyWithAlbumOutcome> {
    let mut out = String::new();
    let (r, range) =
        apply_replaygain_with_album_into(file, steps, result, opts, album_info, &mut out)?;
    Ok((r, out, range))
}

fn apply_replaygain_with_album_into(
    file: &Path,
    steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    album_info: Option<&AacAlbumInfo>,
    out: &mut String,
) -> Result<(JsonFileResult, Option<(u8, u8)>)> {
    let filename = get_filename(file);
    let is_aac = result.file_type() == AudioFileType::Aac;

    // Dry run: don't actually modify. Clipping prevention still needs to
    // be reflected in the "would apply N steps" message, so drive the same
    // ReplayGain-peak cap through predict_apply without touching the file.
    if opts.dry_run {
        let mut apply_opts = ApplyOptions::new(steps);
        apply_opts.track_result = Some(result.clone());
        apply_opts.prevent_clipping = opts.prevent_clipping;
        apply_opts.wrap = opts.wrap_gain;
        let report = predict_apply(file, &apply_opts)?;
        let warning_msg =
            emit_clipping_warning(steps, &report, opts, filename, Some(result.peak()));
        let actual_steps = report.actual_steps;
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
        return Ok((
            JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::DryRun),
                loudness_db: Some(result.loudness_db()),
                loudness_lufs: result.loudness_lufs(),
                analysis_mode: Some(analysis_mode_str(result.analysis_mode())),
                peak: Some(result.peak()),
                gain_applied_steps: Some(actual_steps),
                gain_applied_db: Some(steps_to_db(actual_steps)),
                warning: warning_msg,
                dry_run: Some(true),
                ..Default::default()
            },
            None,
        ));
    }

    // AAC keeps a fail-soft tag-writing path: bitstream gain failures
    // print a warning but still let us record ReplayGain tags. Branch
    // before calling into the unified pipeline so we can apply that
    // policy.
    if is_aac {
        return apply_replaygain_aac_with_album_into(file, steps, result, opts, album_info, out)
            .map(|r| (r, None));
    }

    // MP3 path: hand the whole pipeline to apply_with_options.
    let mut apply_opts = ApplyOptions::new(steps);
    apply_opts.track_result = Some(result.clone());
    apply_opts.album_info = album_info.copied();
    apply_opts.prevent_clipping = opts.prevent_clipping;
    apply_opts.wrap = opts.wrap_gain;
    apply_opts.preserve_timestamp = opts.preserve_timestamp;
    apply_opts.use_temp_file = opts.use_temp_file;
    // RG path writes undo and ReplayGain tags unless -s s. Default APE,
    // `-s i` ID3v2, and AAC all write the REPLAYGAIN_* tags (issue #204).
    apply_opts.write_undo = opts.stored_tag_mode != StoredTagMode::Skip;
    apply_opts.write_replaygain_tags = opts.stored_tag_mode != StoredTagMode::Skip;
    apply_opts.use_id3v2 = opts.use_id3v2;

    match apply_with_options(file, &apply_opts) {
        Ok(report) => {
            let warning_msg =
                emit_clipping_warning(steps, &report, opts, filename, Some(result.peak()));

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

            Ok((
                JsonFileResult {
                    file: file.display().to_string(),
                    status: Some(FileStatus::Success),
                    frames: Some(report.modified),
                    loudness_db: Some(result.loudness_db()),
                    loudness_lufs: result.loudness_lufs(),
                    analysis_mode: Some(analysis_mode_str(result.analysis_mode())),
                    peak: Some(result.peak()),
                    gain_applied_steps: Some(report.actual_steps),
                    gain_applied_db: Some(steps_to_db(report.actual_steps)),
                    warning: warning_msg,
                    ..Default::default()
                },
                report.gain_range,
            ))
        }
        Err(e) => Ok((report_file_error(file, filename, e, opts), None)),
    }
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

    let mut actual_steps = 0;
    let mut warning_msg: Option<String> = None;
    let mut gain_modified: usize = 0;

    // apply_with_options handles temp file, mtime restore, and the
    // clipping check. We only swallow bitstream errors so we can still
    // record the ReplayGain tags afterwards.
    match apply_with_options(file, &apply_opts) {
        Ok(report) => {
            actual_steps = report.actual_steps;
            gain_modified = report.modified;
            warning_msg = emit_clipping_warning(
                requested_steps,
                &report,
                opts,
                filename,
                Some(result.peak()),
            );
        }
        Err(e) => {
            // No gain was baked into the bitstream, so actual_steps stays 0
            // — otherwise the residual tags written below would claim a
            // loudness shift that never happened.
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

    // Post-apply residual (issue #210), mirroring the MP3 path in
    // apply_with_options. AAC clamps internally with no saturation tally, so
    // the loudness shift is the arithmetic applied gain.
    let applied_db = steps_to_db(actual_steps);
    let mut tags = mp4meta::ReplayGainTags::default();
    tags.set_track(
        result.gain_db() - applied_db,
        apply_gain_to_peak(result.peak(), applied_db),
    );
    if let Some(album) = album_info {
        tags.set_album(
            album.album_gain_db - applied_db,
            apply_gain_to_peak(album.album_peak, applied_db),
        );
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
                loudness_lufs: result.loudness_lufs(),
                analysis_mode: Some(analysis_mode_str(result.analysis_mode())),
                peak: Some(result.peak()),
                gain_applied_steps: Some(actual_steps),
                gain_applied_db: Some(steps_to_db(actual_steps)),
                warning: warning_msg,
                ..Default::default()
            })
        }
        Err(e) => Ok(report_file_error(file, filename, e, opts)),
    }
}
