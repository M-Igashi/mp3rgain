use anyhow::Result;
use colored::*;
use indicatif::ProgressBar;
use mp3rgain::apply::{
    apply_with_options, predict_apply, write_replaygain_tags_only, ApplyOptions, TagsOnlyOptions,
};
use mp3rgain::replaygain::{AudioFileType, ReplayGainResult};
use mp3rgain::{apply_gain_to_peak, mp4meta, peak_to_headroom_db, steps_to_db, AacAlbumInfo};
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::util::get_filename;

use super::info::{scan_gain_range_for_row, tsv_rg_row};
use super::utils::{
    analyze_track, emit_clipping_warning, emit_file_warning, report_file_error, restore_timestamp,
    save_original_mtime, stored_track_result, warn_aac_multi_track,
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

    // -s R: reuse stored tags when present and trusted; otherwise fall
    // through to a normal analysis of just this file (issue #298).
    let stored = stored_track_result(file, opts);

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        writeln!(
            out,
            "  {} {}{} {}...",
            "->".cyan(),
            dry_run_prefix,
            if stored.is_some() {
                "Using stored tags for"
            } else {
                "Analyzing"
            },
            filename
        )?;
    }

    let analysis = match stored {
        Some(result) => Ok(result),
        None => analyze_track(file, opts, analysis_pb),
    };

    match analysis {
        Ok(result) => {
            // mp3gain-compatible TSV row for the recommended change. Emitted
            // before the apply, so the global_gain range describes the file as
            // it was scanned, the same values `-o tsv` alone reports.
            if opts.output_format == OutputFormat::Tsv {
                out.push_str(&tsv_rg_row(
                    file,
                    opts,
                    &result,
                    scan_gain_range_for_row(file),
                ));
            }

            // Apply gain modifier (-m steps + -d dB, combined into steps)
            let base_steps = result.gain_steps();
            let modifier_steps = opts.gain_modifier_steps();
            let modified_steps = base_steps + modifier_steps;

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                if opts.tags_only {
                    // No frames are touched, so there are no gain steps to
                    // report, only the dB value headed for the tag.
                    let offset_db = opts.target_offset_db();
                    writeln!(
                        out,
                        "      Loudness: {:.1} {}, Tag gain: {:+.2} dB{}, Peak: {:.4}",
                        result.loudness_db(),
                        result.analysis_mode().unit(),
                        result.gain_db(),
                        if offset_db != 0.0 {
                            format!(" {:+.2} = {:+.2}", offset_db, result.gain_db() + offset_db)
                        } else {
                            String::new()
                        },
                        result.peak()
                    )?;
                } else {
                    writeln!(
                        out,
                        "      Loudness: {:.1} {}, Gain: {:+.1} dB ({} steps{}), Peak: {:.4}",
                        result.loudness_db(),
                        result.analysis_mode().unit(),
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
            }

            if apply_is_noop(
                opts,
                modified_steps,
                result.file_type() == AudioFileType::Aac,
                result.peak(),
            ) {
                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    writeln!(out, "  {} {} (no adjustment needed)", ".".cyan(), filename)?;
                }
                return Ok(JsonFileResult {
                    status: Some(FileStatus::Skipped),
                    gain_applied_steps: Some(0),
                    gain_applied_db: Some(0.0),
                    ..JsonFileResult::from_analysis(file, &result)
                });
            }

            apply_replaygain_with_album_into(file, modified_steps, &result, opts, None, out)
                .map(|(r, _)| r)
        }
        Err(e) => Ok(report_file_error(file, filename, e, opts)),
    }
}

/// Whether a ReplayGain apply that nets `steps == 0` has genuinely nothing to
/// do, so the file can take the cheap "no adjustment needed" skip. It still
/// has work when (issue #206):
///   - ReplayGain analysis tags would be written (AAC always, since mp3gain
///     has no AAC to stay compatible with; MP3 in any mode that keeps tags,
///     i.e. everything but `-s s`, issue #204), so an already-on-target
///     track gets its REPLAYGAIN_* tags (re)written rather than skipped;
///   - `-k` must attenuate a track that already sits at the reference
///     loudness yet already clips (`peak > 1.0`);
///   - `--tags-only` always has a tag to write (issue #308).
///
/// Album mode passes "any member is AAC" and the loudest member peak.
pub fn apply_is_noop(opts: &Options, steps: i32, any_aac: bool, max_peak: f64) -> bool {
    let writes_rg_tags = any_aac || opts.stored_tag_mode != StoredTagMode::Skip;
    let clip_prevention_applies = opts.prevent_clipping && max_peak > 1.0;
    !opts.tags_only && steps == 0 && !writes_rg_tags && !clip_prevention_applies
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

    // --tags-only: record the analysis and leave every frame alone
    // (issue #308). `steps` is deliberately ignored: the target shift lives
    // in the tag value instead.
    if opts.tags_only {
        return write_tags_only_into(file, result, opts, album_info, out).map(|r| (r, None));
    }

    // Dry run: don't actually modify. Clipping prevention still needs to
    // be reflected in the "would apply N steps" message, so drive the same
    // ReplayGain-peak cap through predict_apply without touching the file.
    if opts.dry_run {
        let mut apply_opts = ApplyOptions::new(steps);
        apply_opts.track_result = Some(result.clone());
        apply_opts.prevent_clipping = opts.prevent_clipping;
        apply_opts.wrap = opts.wrap_gain;
        apply_opts.file_type = Some(result.file_type());
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
                status: Some(FileStatus::DryRun),
                gain_applied_steps: Some(actual_steps),
                gain_applied_db: Some(steps_to_db(actual_steps)),
                warning: warning_msg,
                dry_run: Some(true),
                ..JsonFileResult::from_analysis(file, result)
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
    // RG path writes undo and ReplayGain tags unless -s s. Default APE,
    // `-s i` ID3v2, and AAC all write the REPLAYGAIN_* tags (issue #204).
    apply_opts.write_undo = opts.stored_tag_mode != StoredTagMode::Skip;
    apply_opts.write_replaygain_tags = opts.stored_tag_mode != StoredTagMode::Skip;
    apply_opts.tag_layout = opts.tag_layout;
    apply_opts.file_type = Some(AudioFileType::Mp3);

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
                    status: Some(FileStatus::Success),
                    frames: Some(report.modified),
                    gain_applied_steps: Some(report.actual_steps),
                    gain_applied_db: Some(steps_to_db(report.actual_steps)),
                    warning: warning_msg,
                    ..JsonFileResult::from_analysis(file, result)
                },
                report.gain_range,
            ))
        }
        Err(e) => Ok((report_file_error(file, filename, e, opts), None)),
    }
}

/// The gain a player should apply in `--tags-only` mode, capped at the file's
/// exact headroom when `-k` is on so applying the tag cannot push playback
/// past unity (issue #308). Pure, so the album summary can report the same
/// number the per-file writer stores.
pub fn capped_tag_gain(gain_db: f64, peak: f64, prevent_clipping: bool) -> f64 {
    if !prevent_clipping || apply_gain_to_peak(peak, gain_db) <= 1.0 {
        return gain_db;
    }
    // `None` only for a peak of 0 (digital silence), which cannot clip.
    peak_to_headroom_db(peak).unwrap_or(gain_db)
}

/// Clipping check for `--tags-only` (issue #308).
///
/// No gain is baked into the audio, so nothing can clip on disk; the only
/// risk is the *player* driving the file past unity when it applies the tag.
/// `-k` therefore caps the written value at the file's exact headroom, with no
/// 1.5 dB step rounding, because a tag holds a float, not a `global_gain`
/// field. Without `-k` the usual warning is emitted unless `-c` / `-q`
/// silence it.
fn cap_tag_gain(
    gain_db: f64,
    peak: f64,
    label: &str,
    filename: &str,
    opts: &Options,
) -> (f64, Option<String>) {
    let player_peak = apply_gain_to_peak(peak, gain_db);
    if player_peak <= 1.0 {
        return (gain_db, None);
    }
    let capped = capped_tag_gain(gain_db, peak, opts.prevent_clipping);
    if capped != gain_db {
        let msg = format!(
            "{} gain reduced from {:+.2} to {:+.2} dB to prevent clipping (peak: {:.4})",
            label, gain_db, capped, peak
        );
        emit_file_warning(opts, filename, &msg, None);
        return (capped, Some(msg));
    }

    if opts.ignore_clipping || opts.quiet {
        return (gain_db, None);
    }
    let msg = format!(
        "clipping warning: a player applying the {} gain of {:+.2} dB would peak at {:.2} (>1.00)",
        label, gain_db, player_peak
    );
    emit_file_warning(
        opts,
        filename,
        &msg,
        Some("Use -c to ignore clipping warnings or -k to cap the written gain"),
    );
    (gain_db, Some(msg))
}

/// `--tags-only` (issue #308): write the absolute `REPLAYGAIN_*` values and
/// leave the audio byte-identical, the way `loudgain` / `rsgain` work.
///
/// Unlike the apply path the tag is not a residual, so `-d` / `-m` shift the
/// written value rather than the gain baked into the frames, and no
/// `MP3GAIN_UNDO` / `MP3GAIN_MINMAX` is written, since there is nothing to undo.
fn write_tags_only_into(
    file: &Path,
    result: &ReplayGainResult,
    opts: &Options,
    album_info: Option<&AacAlbumInfo>,
    out: &mut String,
) -> Result<JsonFileResult> {
    let filename = get_filename(file);
    let offset_db = opts.target_offset_db();

    if result.file_type() == AudioFileType::Aac {
        warn_aac_multi_track(file, filename, opts);
    }

    let mut warnings: Vec<String> = Vec::new();
    let (track_gain_db, warning) = cap_tag_gain(
        result.gain_db() + offset_db,
        result.peak(),
        "track",
        filename,
        opts,
    );
    warnings.extend(warning);

    // The album peak is the loudest across the set, so capping against it
    // keeps the album value identical on every file, which is the whole
    // point of an album tag.
    let album = album_info.map(|a| {
        let (gain, warning) = cap_tag_gain(
            a.album_gain_db + offset_db,
            a.album_peak,
            "album",
            filename,
            opts,
        );
        warnings.extend(warning);
        (gain, a.album_peak)
    });

    let tag_type = if album.is_some() {
        "track+album tags"
    } else {
        "tags"
    };
    let values = match album {
        Some((album_gain, _)) => format!(
            "track {:+.2} dB, album {:+.2} dB",
            track_gain_db, album_gain
        ),
        None => format!("{:+.2} dB", track_gain_db),
    };
    let warning = (!warnings.is_empty()).then(|| warnings.join("; "));

    let base = JsonFileResult {
        // Nothing was applied to the audio; the gain lives in the tag.
        gain_applied_steps: Some(0),
        gain_applied_db: Some(0.0),
        tag_gain_db: Some(track_gain_db),
        warning,
        ..JsonFileResult::from_analysis(file, result)
    };

    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            writeln!(
                out,
                "  {} [DRY RUN] {} (would write {}, {})",
                "~".cyan(),
                filename,
                tag_type,
                values
            )?;
        }
        return Ok(JsonFileResult {
            status: Some(FileStatus::DryRun),
            dry_run: Some(true),
            ..base
        });
    }

    let mut tag_opts = TagsOnlyOptions::new(track_gain_db, result.peak(), result.analysis_mode());
    tag_opts.album = album;
    tag_opts.tag_layout = opts.tag_layout;
    tag_opts.preserve_timestamp = opts.preserve_timestamp;

    match write_replaygain_tags_only(file, &tag_opts) {
        Ok(()) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                writeln!(
                    out,
                    "  {} {} ({} written, {})",
                    "v".green(),
                    filename,
                    tag_type,
                    values
                )?;
            }
            Ok(JsonFileResult {
                status: Some(FileStatus::Success),
                ..base
            })
        }
        Err(e) => Ok(report_file_error(file, filename, e, opts)),
    }
}

/// Apply ReplayGain to AAC/M4A files with optional album info.
///
/// Differs from the MP3 path in two ways:
///   - the ReplayGain tags are always written (independent of
///     `--stored-tag-mode`), since mp3gain has no AAC to stay compatible
///     with;
///   - bitstream gain failures are logged and swallowed, and the tags are
///     still written, describing the unchanged audio (matches
///     pre-issue-#153 behavior).
///
/// The common case is one call: `apply_with_options` folds the ReplayGain
/// freeform tags into the same container rewrite as the gain and undo tag.
/// Only the fail-soft branch writes the tags in a separate pass, because
/// there the apply produced no file to fold them into.
fn apply_replaygain_aac_with_album_into(
    file: &Path,
    requested_steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    album_info: Option<&AacAlbumInfo>,
    out: &mut String,
) -> Result<JsonFileResult> {
    let filename = get_filename(file);

    warn_aac_multi_track(file, filename, opts);

    // mtime is restored here, after whichever write happened last, so the
    // fallback tag write cannot bump it again.
    let original_mtime = save_original_mtime(file, opts);

    let mut apply_opts = ApplyOptions::new(requested_steps);
    apply_opts.track_result = Some(result.clone());
    apply_opts.album_info = album_info.copied();
    apply_opts.prevent_clipping = opts.prevent_clipping;
    apply_opts.wrap = opts.wrap_gain;
    apply_opts.preserve_timestamp = false;
    apply_opts.write_replaygain_tags = true;
    apply_opts.file_type = Some(AudioFileType::Aac);

    let (actual_steps, gain_modified, warning_msg) = match apply_with_options(file, &apply_opts) {
        Ok(report) => (
            report.actual_steps,
            report.modified,
            emit_clipping_warning(
                requested_steps,
                &report,
                opts,
                filename,
                Some(result.peak()),
            ),
        ),
        Err(e) => {
            emit_file_warning(
                opts,
                filename,
                &format!("bitstream gain failed: {} (tags still written)", e),
                None,
            );
            // No gain was baked into the bitstream, so the tags carry the
            // full measured values rather than a residual that would claim
            // a loudness shift that never happened.
            let mut tags = mp4meta::ReplayGainTags::default();
            tags.set_track(result.gain_db(), result.peak());
            if let Some(album) = album_info {
                tags.set_album(album.album_gain_db, album.album_peak);
            }
            tags.set_algorithm(result.analysis_mode());
            if let Err(e) = mp4meta::write_replaygain_tags(file, &tags) {
                return Ok(report_file_error(file, filename, e, opts));
            }
            (0, 0, None)
        }
    };

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
        status: Some(FileStatus::Success),
        gain_applied_steps: Some(actual_steps),
        gain_applied_db: Some(steps_to_db(actual_steps)),
        warning: warning_msg,
        ..JsonFileResult::from_analysis(file, result)
    })
}
