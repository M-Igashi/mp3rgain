use anyhow::Result;
use colored::*;
use mp3rgain::{aac, id3v2, mp4meta, undo_gain};
use std::path::Path;

use crate::cli::options::{Options, OutputFormat};
use crate::json_output::JsonFileResult;
use crate::util::get_filename;

use super::utils::restore_timestamp;

pub fn process_undo(file: &Path, opts: &Options) -> Result<JsonFileResult> {
    let filename = get_filename(file);
    let dry_run_prefix = opts.dry_run_prefix();

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp && !opts.dry_run {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    // Dry run: just analyze what would be done
    if opts.dry_run {
        // Try to read the undo tag to see what would happen
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            println!("  {} [DRY RUN] {} (would undo)", "~".cyan(), filename);
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some("dry_run".to_string()),
            dry_run: Some(true),
            ..Default::default()
        });
    }

    let is_aac = mp4meta::is_aac_file(file);
    let undo_result = if is_aac {
        aac::undo_aac_gain(file)
    } else if opts.use_id3v2 {
        id3v2::undo_gain_id3v2(file)
    } else {
        undo_gain(file)
    };

    match undo_result {
        Ok(frames) => {
            if frames == 0 {
                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    println!(
                        "  {} {}{} (no changes to undo)",
                        ".".cyan(),
                        dry_run_prefix,
                        filename
                    );
                }

                Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some("skipped".to_string()),
                    frames: Some(0),
                    ..Default::default()
                })
            } else {
                // Restore timestamp if needed
                if let Some(mtime) = original_mtime {
                    restore_timestamp(file, mtime);
                }

                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    println!(
                        "  {} {} ({} frames restored)",
                        "v".green(),
                        filename,
                        frames
                    );
                }

                Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some("success".to_string()),
                    frames: Some(frames),
                    ..Default::default()
                })
            }
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
