use anyhow::Result;
use colored::*;
use mp3rgain::undo_gain_auto;
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::util::get_filename;

use super::utils::{restore_timestamp, save_original_mtime};

pub fn process_undo(file: &Path, opts: &Options) -> Result<(JsonFileResult, String)> {
    let mut out = String::new();
    let result = process_undo_into(file, opts, &mut out)?;
    Ok((result, out))
}

fn process_undo_into(file: &Path, opts: &Options, out: &mut String) -> Result<JsonFileResult> {
    let filename = get_filename(file);
    let dry_run_prefix = opts.dry_run_prefix();

    let original_mtime = save_original_mtime(file, opts);

    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            writeln!(out, "  {} [DRY RUN] {} (would undo)", "~".cyan(), filename)?;
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some(FileStatus::DryRun),
            dry_run: Some(true),
            ..Default::default()
        });
    }

    let undo_result = undo_gain_auto(file, opts.use_id3v2);

    match undo_result {
        Ok(frames) => {
            if frames == 0 {
                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    writeln!(
                        out,
                        "  {} {}{} (no changes to undo)",
                        ".".cyan(),
                        dry_run_prefix,
                        filename
                    )?;
                }

                Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some(FileStatus::Skipped),
                    frames: Some(0),
                    ..Default::default()
                })
            } else {
                // Restore timestamp if needed
                if let Some(mtime) = original_mtime {
                    restore_timestamp(file, mtime);
                }

                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    writeln!(
                        out,
                        "  {} {} ({} frames restored)",
                        "v".green(),
                        filename,
                        frames
                    )?;
                }

                Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some(FileStatus::Success),
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
                status: Some(FileStatus::Error),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}
