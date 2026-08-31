use anyhow::Result;
use colored::*;
use mp3rgain::{
    read_gain_tags_auto, GainTagSource, StoredGainTags, TAG_MP3GAIN_ALBUM_MINMAX,
    TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO, TAG_REPLAYGAIN_ALBUM_GAIN, TAG_REPLAYGAIN_ALBUM_PEAK,
    TAG_REPLAYGAIN_ALGORITHM, TAG_REPLAYGAIN_TRACK_GAIN, TAG_REPLAYGAIN_TRACK_PEAK,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::utils::{finish_with_summary, finish_without_summary, for_each_file};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::processors::utils::{report_file_error, restore_timestamp, save_original_mtime};
use crate::util::get_filename;

/// Tags plus per-container labels for display in cmd_check_tags
struct CheckTagInfo<'a> {
    tags: &'a StoredGainTags,
    undo_label: &'a str,
    minmax_label: &'a str,
    no_tag_msg: &'a str,
}

impl CheckTagInfo<'_> {
    fn render(
        &self,
        filename: &str,
        file_path: &Path,
        format: OutputFormat,
        out: &mut String,
    ) -> Option<JsonFileResult> {
        let tags = self.tags;
        match format {
            OutputFormat::Text => {
                writeln!(out, "{}", filename.cyan().bold()).ok();
                if let Some(v) = &tags.undo {
                    writeln!(out, "  {:<25}{}", format!("{}:", self.undo_label), v).ok();
                }
                if let Some(v) = &tags.minmax {
                    writeln!(out, "  {:<25}{}", format!("{}:", self.minmax_label), v).ok();
                }
                let rg_fields = [
                    (TAG_MP3GAIN_ALBUM_MINMAX, &tags.album_minmax),
                    (TAG_REPLAYGAIN_TRACK_GAIN, &tags.track_gain),
                    (TAG_REPLAYGAIN_TRACK_PEAK, &tags.track_peak),
                    (TAG_REPLAYGAIN_ALBUM_GAIN, &tags.album_gain),
                    (TAG_REPLAYGAIN_ALBUM_PEAK, &tags.album_peak),
                    (TAG_REPLAYGAIN_ALGORITHM, &tags.algorithm),
                ];
                for (label, value) in rg_fields {
                    if let Some(v) = value {
                        writeln!(out, "  {:<25}{}", format!("{}:", label), v).ok();
                    }
                }
                if !tags.has_any() {
                    writeln!(out, "  ({})", self.no_tag_msg).ok();
                }
                writeln!(out).ok();
                None
            }
            OutputFormat::Tsv => {
                writeln!(
                    out,
                    // The algorithm column is appended last so existing
                    // column indices stay stable for scripts.
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    filename,
                    tags.undo.as_deref().unwrap_or("-"),
                    tags.minmax.as_deref().unwrap_or("-"),
                    tags.track_gain.as_deref().unwrap_or("-"),
                    tags.track_peak.as_deref().unwrap_or("-"),
                    tags.album_gain.as_deref().unwrap_or("-"),
                    tags.album_peak.as_deref().unwrap_or("-"),
                    tags.album_minmax.as_deref().unwrap_or("-"),
                    tags.algorithm.as_deref().unwrap_or("-")
                )
                .ok();
                None
            }
            OutputFormat::Json => Some(JsonFileResult {
                file: file_path.display().to_string(),
                status: Some(if tags.has_any() {
                    FileStatus::Success
                } else {
                    FileStatus::NoTag
                }),
                ..Default::default()
            }),
        }
    }
}

pub fn cmd_delete_tags(files: &[PathBuf], opts: &Options) -> Result<()> {
    let dry_run_prefix = opts.dry_run_prefix();

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        let action = match (opts.dry_run, opts.undo) {
            (true, true) => "Would undo gain changes and delete",
            (true, false) => "Would delete",
            (false, true) => "Undoing gain changes and deleting",
            (false, false) => "Deleting",
        };
        println!(
            "{}{} {} ReplayGain tags from {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            action,
            files.len()
        );
        println!();
    }

    let (json_results, successful, failed) = for_each_file(files, opts, |file| {
        let (result, text) = process_delete_tags(file, opts)?;
        Ok((Some(result), text))
    })?;

    finish_with_summary(files.len(), json_results, successful, failed, opts)
}

fn process_delete_tags(file: &Path, opts: &Options) -> Result<(JsonFileResult, String)> {
    let filename = get_filename(file);
    let mut out = String::new();

    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            let action = if opts.undo {
                "would undo gain changes, then delete tags"
            } else {
                "would delete tags"
            };
            writeln!(out, "  {} [DRY RUN] {} ({})", "~".cyan(), filename, action)?;
        }
        return Ok((
            JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::DryRun),
                dry_run: Some(true),
                ..Default::default()
            },
            out,
        ));
    }

    let original_mtime = save_original_mtime(file, opts);

    // -u -s d (issue #305): undo the frame-level gain before the tags,
    // including MP3GAIN_UNDO, are destroyed. A file with nothing to undo
    // falls through to the plain delete; any other undo failure skips the
    // delete so the undo info survives for another attempt.
    let mut undone_frames: Option<usize> = None;
    if opts.undo {
        use mp3rgain::Error as GainError;
        match mp3rgain::undo_gain_auto(file, opts.tag_layout) {
            Ok(frames) => undone_frames = Some(frames),
            Err(GainError::NoApeTag | GainError::NoUndoTag | GainError::NoId3v2UndoTag) => {
                undone_frames = Some(0);
            }
            Err(e) => return Ok((report_file_error(file, filename, e, opts), out)),
        }
    }

    let delete_result = mp3rgain::delete_gain_tags_auto(file, opts.tag_layout);

    match delete_result {
        Ok(()) => {
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                let note = match undone_frames {
                    Some(frames) if frames > 0 => {
                        format!("{} frames restored, tags deleted", frames)
                    }
                    Some(_) => "no changes to undo, tags deleted".to_string(),
                    None => "tags deleted".to_string(),
                };
                writeln!(out, "  {} {} ({})", "v".green(), filename, note)?;
            }
            Ok((
                JsonFileResult {
                    file: file.display().to_string(),
                    status: Some(FileStatus::Success),
                    frames: undone_frames,
                    ..Default::default()
                },
                out,
            ))
        }
        Err(e) => Ok((report_file_error(file, filename, e, opts), out)),
    }
}

pub fn cmd_check_tags(files: &[PathBuf], opts: &Options) -> Result<()> {
    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{} Checking stored tag info for {} file(s)",
            "mp3rgain".green().bold(),
            files.len()
        );
        println!();
    }

    let (json_results, _, _) =
        for_each_file(files, opts, |file| Ok(process_check_tags(file, opts)))?;

    finish_without_summary(json_results, opts)
}

/// Shared error arm for the tag-read branches of `process_check_tags`:
/// stderr line in text/TSV mode, error record in JSON mode.
fn tag_read_error(
    file: &Path,
    filename: &str,
    e: impl std::fmt::Display,
    opts: &Options,
) -> Option<JsonFileResult> {
    if opts.output_format != OutputFormat::Json {
        eprintln!("{} - {}", filename.red(), e);
        None
    } else {
        Some(JsonFileResult::error(file, e))
    }
}

fn process_check_tags(file: &Path, opts: &Options) -> (Option<JsonFileResult>, String) {
    let filename = get_filename(file);
    let mut out = String::new();

    let tags = match read_gain_tags_auto(file, opts.tag_layout) {
        Ok(tags) => tags,
        Err(e) => return (tag_read_error(file, filename, e, opts), out),
    };

    let (undo_label, minmax_label, no_tag_msg) = match tags.source {
        GainTagSource::Aac => ("MP3RGAIN_UNDO", "MP3RGAIN_MINMAX", "no tags found"),
        GainTagSource::Id3v2 => (
            TAG_MP3GAIN_UNDO,
            TAG_MP3GAIN_MINMAX,
            "no ID3v2 ReplayGain tags found",
        ),
        GainTagSource::Ape { tag_present: true } => (
            TAG_MP3GAIN_UNDO,
            TAG_MP3GAIN_MINMAX,
            "no mp3gain tags found",
        ),
        GainTagSource::Ape { tag_present: false } => {
            (TAG_MP3GAIN_UNDO, TAG_MP3GAIN_MINMAX, "no APE tag found")
        }
        GainTagSource::Split => (TAG_MP3GAIN_UNDO, TAG_MP3GAIN_MINMAX, "no gain tags found"),
    };

    let info = CheckTagInfo {
        tags: &tags,
        undo_label,
        minmax_label,
        no_tag_msg,
    };
    let json = info.render(filename, file, opts.output_format, &mut out);
    (json, out)
}
