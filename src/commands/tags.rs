use anyhow::Result;
use colored::*;
use mp3rgain::{
    delete_ape_tag, id3v2, mp4meta, read_ape_tag_from_file, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO,
    TAG_REPLAYGAIN_ALBUM_GAIN, TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_TRACK_GAIN,
    TAG_REPLAYGAIN_TRACK_PEAK,
};
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::utils::create_json_summary;
use crate::get_filename;
use crate::json_output::{JsonFileResult, JsonOutput};
use crate::processors::utils::restore_timestamp;
use crate::progress::{create_progress_bar, progress_finish, progress_inc, progress_set_message};

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

    fn display(
        &self,
        filename: &str,
        file_path: &Path,
        format: OutputFormat,
        json_results: &mut Vec<JsonFileResult>,
    ) {
        match format {
            OutputFormat::Text => {
                println!("{}", filename.cyan().bold());
                if let Some(v) = self.undo {
                    println!("  {:<25}{}", format!("{}:", self.undo_label), v);
                }
                if let Some(v) = self.minmax {
                    println!("  {:<25}{}", format!("{}:", self.minmax_label), v);
                }
                if let Some(v) = self.track_gain {
                    println!("  REPLAYGAIN_TRACK_GAIN: {}", v);
                }
                if let Some(v) = self.track_peak {
                    println!("  REPLAYGAIN_TRACK_PEAK: {}", v);
                }
                if let Some(v) = self.album_gain {
                    println!("  REPLAYGAIN_ALBUM_GAIN: {}", v);
                }
                if let Some(v) = self.album_peak {
                    println!("  REPLAYGAIN_ALBUM_PEAK: {}", v);
                }
                if !self.has_any() {
                    println!("  ({})", self.no_tag_msg);
                }
                println!();
            }
            OutputFormat::Tsv => {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    filename,
                    self.undo.unwrap_or("-"),
                    self.minmax.unwrap_or("-"),
                    self.track_gain.unwrap_or("-"),
                    self.track_peak.unwrap_or("-"),
                    self.album_gain.unwrap_or("-"),
                    self.album_peak.unwrap_or("-")
                );
            }
            OutputFormat::Json => {
                json_results.push(JsonFileResult {
                    file: file_path.display().to_string(),
                    status: Some(if self.has_any() { "success" } else { "no_tag" }.to_string()),
                    ..Default::default()
                });
            }
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
    let mut json_results: Vec<JsonFileResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        if opts.dry_run {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!(
                    "  {} [DRY RUN] {} (would delete tags)",
                    "~".cyan(),
                    filename
                );
            }
            json_results.push(JsonFileResult {
                file: file.display().to_string(),
                status: Some("dry_run".to_string()),
                dry_run: Some(true),
                ..Default::default()
            });
        } else {
            // Save original timestamp if needed
            let original_mtime = if opts.preserve_timestamp {
                std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
            } else {
                None
            };

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
                        println!("  {} {} (tags deleted)", "v".green(), filename);
                    }
                    successful += 1;
                    json_results.push(JsonFileResult {
                        file: file.display().to_string(),
                        status: Some("success".to_string()),
                        ..Default::default()
                    });
                }
                Err(e) => {
                    if opts.output_format == OutputFormat::Text && !opts.quiet {
                        eprintln!("  {} {} - {}", "x".red(), filename, e);
                    }
                    failed += 1;
                    json_results.push(JsonFileResult {
                        file: file.display().to_string(),
                        status: Some("error".to_string()),
                        error: Some(e.to_string()),
                        ..Default::default()
                    });
                }
            }
        }

        progress_inc(&pb);
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
    } else if opts.dry_run && !opts.quiet {
        println!();
        println!("{}", "No files were modified.".yellow());
    }

    Ok(())
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

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        let is_aac = mp4meta::is_aac_file(file);

        if is_aac {
            // AAC: read iTunes freeform tags
            let undo_tags = mp4meta::read_undo_tags(file).unwrap_or_default();
            let rg_tags = mp4meta::read_replaygain_tags(file).unwrap_or_default();

            let undo = undo_tags.undo();
            let minmax = undo_tags.minmax();
            let track_gain = rg_tags.track_gain();
            let track_peak = rg_tags.track_peak();
            let album_gain = rg_tags.album_gain();
            let album_peak = rg_tags.album_peak();

            CheckTagInfo {
                undo,
                minmax,
                track_gain,
                track_peak,
                album_gain,
                album_peak,
                undo_label: "MP3RGAIN_UNDO",
                minmax_label: "MP3RGAIN_MINMAX",
                no_tag_msg: "no tags found",
            }
            .display(filename, file, opts.output_format, &mut json_results);
        } else if opts.use_id3v2 {
            // MP3 with -s i: read ID3v2 TXXX frames
            match id3v2::read_id3v2_replaygain(file) {
                Ok(rg) => {
                    let undo = rg.undo.as_deref();
                    let minmax = rg.minmax.as_deref();
                    let track_gain = rg.track_gain.as_deref();
                    let track_peak = rg.track_peak.as_deref();
                    let album_gain = rg.album_gain.as_deref();
                    let album_peak = rg.album_peak.as_deref();

                    CheckTagInfo {
                        undo,
                        minmax,
                        track_gain,
                        track_peak,
                        album_gain,
                        album_peak,
                        undo_label: "MP3GAIN_UNDO",
                        minmax_label: "MP3GAIN_MINMAX",
                        no_tag_msg: "no ID3v2 ReplayGain tags found",
                    }
                    .display(
                        filename,
                        file,
                        opts.output_format,
                        &mut json_results,
                    );
                }
                Err(e) => {
                    if opts.output_format != OutputFormat::Json {
                        eprintln!("{} - {}", filename.red(), e);
                    } else {
                        json_results.push(JsonFileResult {
                            file: file.display().to_string(),
                            status: Some("error".to_string()),
                            error: Some(e.to_string()),
                            ..Default::default()
                        });
                    }
                }
            }
        } else {
            // MP3: read APEv2 tags
            match read_ape_tag_from_file(file) {
                Ok(Some(tag)) => {
                    CheckTagInfo {
                        undo: tag.get(TAG_MP3GAIN_UNDO),
                        minmax: tag.get(TAG_MP3GAIN_MINMAX),
                        track_gain: tag.get(TAG_REPLAYGAIN_TRACK_GAIN),
                        track_peak: tag.get(TAG_REPLAYGAIN_TRACK_PEAK),
                        album_gain: tag.get(TAG_REPLAYGAIN_ALBUM_GAIN),
                        album_peak: tag.get(TAG_REPLAYGAIN_ALBUM_PEAK),
                        undo_label: "MP3GAIN_UNDO",
                        minmax_label: "MP3GAIN_MINMAX",
                        no_tag_msg: "no mp3gain tags found",
                    }
                    .display(
                        filename,
                        file,
                        opts.output_format,
                        &mut json_results,
                    );
                }
                Ok(None) => {
                    CheckTagInfo {
                        undo: None,
                        minmax: None,
                        track_gain: None,
                        track_peak: None,
                        album_gain: None,
                        album_peak: None,
                        undo_label: "MP3GAIN_UNDO",
                        minmax_label: "MP3GAIN_MINMAX",
                        no_tag_msg: "no APE tag found",
                    }
                    .display(
                        filename,
                        file,
                        opts.output_format,
                        &mut json_results,
                    );
                }
                Err(e) => {
                    if opts.output_format != OutputFormat::Json {
                        eprintln!("{} - {}", filename.red(), e);
                    } else {
                        json_results.push(JsonFileResult {
                            file: file.display().to_string(),
                            status: Some("error".to_string()),
                            error: Some(e.to_string()),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        progress_inc(&pb);
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
