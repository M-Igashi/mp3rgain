use anyhow::Result;
use colored::*;
use mp3rgain::{steps_to_db, Channel};
use std::fmt::Write as _;
use std::path::PathBuf;

use crate::cli::options::{Options, OutputFormat};
use crate::commands::utils::{create_json_summary, finish_with_summary, for_each_file};
use crate::json_output::JsonOutput;
use crate::processors::apply::{process_apply, process_apply_channel};
use crate::util::get_filename;

pub fn cmd_apply(files: &[PathBuf], steps: i32, opts: &Options) -> Result<()> {
    if steps == 0 {
        if opts.output_format == OutputFormat::Json {
            let output = JsonOutput {
                files: Some(vec![]),
                album: None,
                summary: Some(create_json_summary(files.len(), 0, 0, opts.dry_run)),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !opts.quiet {
            println!("{}: gain is 0, nothing to do", "info".cyan());
        }
        return Ok(());
    }

    let db_value = steps_to_db(steps);
    let dry_run_prefix = opts.dry_run_prefix();

    if opts.output_format == OutputFormat::Tsv {
        println!("File\tMP3 gain\tdB gain\tMax Amplitude\tMax global_gain\tMin global_gain");
    }

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{}{} {} {} step(s) ({:+.1} dB) to {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            if opts.dry_run {
                "Would apply"
            } else {
                "Applying"
            },
            steps,
            db_value,
            files.len()
        );
        if opts.wrap_gain {
            println!("  {} Wrap mode enabled", "!".yellow());
        }
        println!();
    }

    let (json_results, successful, failed) = for_each_file(files, opts, |file| {
        let (result, mut text) = process_apply(file, steps, opts)?;
        if opts.output_format == OutputFormat::Tsv {
            // Max Amplitude is unknown without a full decode; emit "-"
            // instead of fabricating a value (issue #231).
            writeln!(
                text,
                "{}\t{}\t{:.6}\t-\t{}\t{}",
                get_filename(file),
                steps,
                db_value,
                result.max_gain.unwrap_or(255),
                result.min_gain.unwrap_or(0)
            )?;
        }
        Ok((Some(result), text))
    })?;

    finish_with_summary(files.len(), json_results, successful, failed, opts)
}

pub fn cmd_apply_channel(
    files: &[PathBuf],
    channel: Channel,
    steps: i32,
    opts: &Options,
) -> Result<()> {
    if steps == 0 {
        if opts.output_format == OutputFormat::Json {
            let output = JsonOutput {
                files: Some(vec![]),
                album: None,
                summary: Some(create_json_summary(files.len(), 0, 0, opts.dry_run)),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !opts.quiet {
            println!("{}: gain is 0, nothing to do", "info".cyan());
        }
        return Ok(());
    }

    let db_value = steps_to_db(steps);
    let dry_run_prefix = opts.dry_run_prefix();
    let channel_name = channel.name();

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{}{} {} {} step(s) ({:+.1} dB) to {} channel of {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            if opts.dry_run {
                "Would apply"
            } else {
                "Applying"
            },
            steps,
            db_value,
            channel_name,
            files.len()
        );
        println!();
    }

    let (json_results, successful, failed) = for_each_file(files, opts, |file| {
        let (result, text) = process_apply_channel(file, channel, steps, opts)?;
        Ok((Some(result), text))
    })?;

    finish_with_summary(files.len(), json_results, successful, failed, opts)
}
