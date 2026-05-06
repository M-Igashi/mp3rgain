use anyhow::Result;
use colored::*;
use mp3rgain::{
    delete_ape_tag, id3v2, mp4meta, read_ape_tag_from_file, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO,
    TAG_REPLAYGAIN_ALBUM_GAIN, TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_TRACK_GAIN,
    TAG_REPLAYGAIN_TRACK_PEAK,
};
use rayon::prelude::*;
use std::fmt::Write as _;
use std::io::{self, Write as IoWrite};
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::threading::effective_threads;
use crate::commands::utils::{create_json_summary, print_dry_run_notice};
use crate::json_output::{FileStatus, JsonFileResult, JsonOutput};
use crate::processors::utils::{restore_timestamp, save_original_mtime};
use crate::progress::{create_progress_bar, progress_finish, progress_inc, progress_set_message};
use crate::util::get_filename;

/// Tag values and labels for display in cmd_check_tags
struct CheckTagInfo<'a> {
    undo: Option<&'a str>,
    minmax: Option<&'a str>,
    track_gain: Option<&'a str>,
    track_peak: Option<&'a str>,
    album_gain: Option<&'a str>,
    album_peak: Option<&'a str>,
    undo_label: &'a str,
    minmax_label: &'a str,
    no_tag_msg: &'a str,
}

impl CheckTagInfo<'_> {
    fn has_any(&self) -> bool {
        self.undo.is_some()
            || self.minmax.is_some()
            || self.track_gain.is_some()
            || self.track_peak.is_some()
            || self.album_gain.is_some()
            || self.album_peak.is_some()
    }

    fn render(
        &self,
        filename: &str,
        file_path: &Path,
        format: OutputFormat,
        out: &mut String,
    ) -> Option<JsonFileResult> {
        match format {
            OutputFormat::Text => {
                writeln!(out, "{}", filename.cyan().bold()).ok();
                if let Some(v) = self.undo {
                    writeln!(out, "  {:<25}{}", format!("{}:", self.undo_label), v).ok();
                }
                if let Some(v) = self.minmax {
                    writeln!(out, "  {:<25}{}", format!("{}:", self.minmax_label), v).ok();
                }
                if let Some(v) = self.track_gain {
                    writeln!(out, "  REPLAYGAIN_TRACK_GAIN: {}", v).ok();
                }
                if let Some(v) = self.track_peak {
                    writeln!(out, "  REPLAYGAIN_TRACK_PEAK: {}", v).ok();
                }
                if let Some(v) = self.album_gain {
                    writeln!(out, "  REPLAYGAIN_ALBUM_GAIN: {}", v).ok();
                }
                if let Some(v) = self.album_peak {
                    writeln!(out, "  REPLAYGAIN_ALBUM_PEAK: {}", v).ok();
                }
                if !self.has_any() {
                    writeln!(out, "  ({})", self.no_tag_msg).ok();
                }
                writeln!(out).ok();
                None
            }
            OutputFormat::Tsv => {
                writeln!(
                    out,
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    filename,
                    self.undo.unwrap_or("-"),
                    self.minmax.unwrap_or("-"),
                    self.track_gain.unwrap_or("-"),
                    self.track_peak.unwrap_or("-"),
                    self.album_gain.unwrap_or("-"),
                    self.album_peak.unwrap_or("-")
                )
                .ok();
                None
            }
            OutputFormat::Json => Some(JsonFileResult {
                file: file_path.display().to_string(),
                status: Some(if self.has_any() {
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
        println!(
            "{}{} {} ReplayGain tags from {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            if opts.dry_run {
                "Would delete"
            } else {
                "Deleting"
            },
            files.len()
        );
        println!();
    }

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::with_capacity(files.len());
    let mut successful = 0;
    let mut failed = 0;

    let parallel = effective_threads(opts) > 1 && files.len() > 1;

    if parallel {
        let pb_ref = pb.as_ref();
        let collected: Vec<(JsonFileResult, String)> = files
            .par_iter()
            .map(|file| -> Result<(JsonFileResult, String)> {
                let r = process_delete_tags(file, opts)?;
                if let Some(pb) = pb_ref {
                    pb.set_message(get_filename(file).to_string());
                    pb.inc(1);
                }
                Ok(r)
            })
            .collect::<Result<Vec<_>>>()?;

        let stdout = io::stdout();
        let mut handle = stdout.lock();
        for (_, text) in &collected {
            if !text.is_empty() {
                handle.write_all(text.as_bytes())?;
            }
        }
        drop(handle);

        for (result, _) in collected {
            match result.status {
                Some(FileStatus::Success) => successful += 1,
                Some(FileStatus::Error) => failed += 1,
                _ => {}
            }
            json_results.push(result);
        }
    } else {
        for file in files {
            let filename = get_filename(file);
            progress_set_message(&pb, filename);

            let (result, text) = process_delete_tags(file, opts)?;
            if !text.is_empty() {
                print!("{}", text);
            }
            match result.status {
                Some(FileStatus::Success) => successful += 1,
                Some(FileStatus::Error) => failed += 1,
                _ => {}
            }
            json_results.push(result);

            progress_inc(&pb);
        }
    }

    progress_finish(pb);

    if opts.output_format == OutputFormat::Json {
        let output = JsonOutput {
            files: Some(json_results),
            album: None,
            summary: Some(create_json_summary(
                files.len(),
                successful,
                failed,
                opts.dry_run,
            )),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_dry_run_notice(opts);
    }

    Ok(())
}

fn process_delete_tags(file: &Path, opts: &Options) -> Result<(JsonFileResult, String)> {
    let filename = get_filename(file);
    let mut out = String::new();

    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            writeln!(
                out,
                "  {} [DRY RUN] {} (would delete tags)",
                "~".cyan(),
                filename
            )?;
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

    let delete_result = if mp4meta::is_aac_file(file) {
        // AAC: delete both ReplayGain and undo freeform tags
        mp4meta::delete_replaygain_tags(file).and_then(|()| mp4meta::delete_undo_tags(file))
    } else if opts.use_id3v2 {
        id3v2::delete_id3v2_replaygain(file)
    } else {
        delete_ape_tag(file)
    };

    match delete_result {
        Ok(()) => {
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                writeln!(out, "  {} {} (tags deleted)", "v".green(), filename)?;
            }
            Ok((
                JsonFileResult {
                    file: file.display().to_string(),
                    status: Some(FileStatus::Success),
                    ..Default::default()
                },
                out,
            ))
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!("  {} {} - {}", "x".red(), filename, e);
            }
            Ok((
                JsonFileResult {
                    file: file.display().to_string(),
                    status: Some(FileStatus::Error),
                    error: Some(e.to_string()),
                    ..Default::default()
                },
                out,
            ))
        }
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

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();

    let parallel = effective_threads(opts) > 1 && files.len() > 1;

    if parallel {
        let pb_ref = pb.as_ref();
        let collected: Vec<(Option<JsonFileResult>, String)> = files
            .par_iter()
            .map(|file| -> Result<(Option<JsonFileResult>, String)> {
                let r = process_check_tags(file, opts);
                if let Some(pb) = pb_ref {
                    pb.set_message(get_filename(file).to_string());
                    pb.inc(1);
                }
                Ok(r)
            })
            .collect::<Result<Vec<_>>>()?;

        let stdout = io::stdout();
        let mut handle = stdout.lock();
        for (_, text) in &collected {
            if !text.is_empty() {
                handle.write_all(text.as_bytes())?;
            }
        }
        drop(handle);

        for (result, _) in collected {
            if let Some(r) = result {
                json_results.push(r);
            }
        }
    } else {
        for file in files {
            let filename = get_filename(file);
            progress_set_message(&pb, filename);

            let (result, text) = process_check_tags(file, opts);
            if !text.is_empty() {
                print!("{}", text);
            }
            if let Some(r) = result {
                json_results.push(r);
            }

            progress_inc(&pb);
        }
    }

    progress_finish(pb);

    if opts.output_format == OutputFormat::Json {
        let output = JsonOutput {
            files: Some(json_results),
            album: None,
            summary: None,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}

fn process_check_tags(file: &Path, opts: &Options) -> (Option<JsonFileResult>, String) {
    let filename = get_filename(file);
    let mut out = String::new();

    let is_aac = mp4meta::is_aac_file(file);

    if is_aac {
        let undo_tags = mp4meta::read_undo_tags(file).unwrap_or_default();
        let rg_tags = mp4meta::read_replaygain_tags(file).unwrap_or_default();

        let info = CheckTagInfo {
            undo: undo_tags.undo(),
            minmax: undo_tags.minmax(),
            track_gain: rg_tags.track_gain(),
            track_peak: rg_tags.track_peak(),
            album_gain: rg_tags.album_gain(),
            album_peak: rg_tags.album_peak(),
            undo_label: "MP3RGAIN_UNDO",
            minmax_label: "MP3RGAIN_MINMAX",
            no_tag_msg: "no tags found",
        };
        let json = info.render(filename, file, opts.output_format, &mut out);
        (json, out)
    } else if opts.use_id3v2 {
        match id3v2::read_id3v2_replaygain(file) {
            Ok(rg) => {
                let info = CheckTagInfo {
                    undo: rg.undo.as_deref(),
                    minmax: rg.minmax.as_deref(),
                    track_gain: rg.track_gain.as_deref(),
                    track_peak: rg.track_peak.as_deref(),
                    album_gain: rg.album_gain.as_deref(),
                    album_peak: rg.album_peak.as_deref(),
                    undo_label: "MP3GAIN_UNDO",
                    minmax_label: "MP3GAIN_MINMAX",
                    no_tag_msg: "no ID3v2 ReplayGain tags found",
                };
                let json = info.render(filename, file, opts.output_format, &mut out);
                (json, out)
            }
            Err(e) => {
                if opts.output_format != OutputFormat::Json {
                    eprintln!("{} - {}", filename.red(), e);
                    (None, out)
                } else {
                    (
                        Some(JsonFileResult {
                            file: file.display().to_string(),
                            status: Some(FileStatus::Error),
                            error: Some(e.to_string()),
                            ..Default::default()
                        }),
                        out,
                    )
                }
            }
        }
    } else {
        match read_ape_tag_from_file(file) {
            Ok(Some(tag)) => {
                let info = CheckTagInfo {
                    undo: tag.get(TAG_MP3GAIN_UNDO),
                    minmax: tag.get(TAG_MP3GAIN_MINMAX),
                    track_gain: tag.get(TAG_REPLAYGAIN_TRACK_GAIN),
                    track_peak: tag.get(TAG_REPLAYGAIN_TRACK_PEAK),
                    album_gain: tag.get(TAG_REPLAYGAIN_ALBUM_GAIN),
                    album_peak: tag.get(TAG_REPLAYGAIN_ALBUM_PEAK),
                    undo_label: "MP3GAIN_UNDO",
                    minmax_label: "MP3GAIN_MINMAX",
                    no_tag_msg: "no mp3gain tags found",
                };
                let json = info.render(filename, file, opts.output_format, &mut out);
                (json, out)
            }
            Ok(None) => {
                let info = CheckTagInfo {
                    undo: None,
                    minmax: None,
                    track_gain: None,
                    track_peak: None,
                    album_gain: None,
                    album_peak: None,
                    undo_label: "MP3GAIN_UNDO",
                    minmax_label: "MP3GAIN_MINMAX",
                    no_tag_msg: "no APE tag found",
                };
                let json = info.render(filename, file, opts.output_format, &mut out);
                (json, out)
            }
            Err(e) => {
                if opts.output_format != OutputFormat::Json {
                    eprintln!("{} - {}", filename.red(), e);
                    (None, out)
                } else {
                    (
                        Some(JsonFileResult {
                            file: file.display().to_string(),
                            status: Some(FileStatus::Error),
                            error: Some(e.to_string()),
                            ..Default::default()
                        }),
                        out,
                    )
                }
            }
        }
    }
}
