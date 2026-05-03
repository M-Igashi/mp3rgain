use anyhow::Result;
use colored::*;
use mp3rgain::{analyze, steps_to_db, Channel};
use std::path::PathBuf;

use crate::cli::options::{Options, OutputFormat};
use crate::commands::utils::{create_json_summary, print_dry_run_notice, update_counters};
use crate::json_output::{JsonFileResult, JsonOutput};
use crate::processors::apply::{process_apply, process_apply_channel};
use crate::progress::{create_progress_bar, progress_finish, progress_inc, progress_set_message};
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

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        let result = process_apply(file, steps, opts)?;
        update_counters(&result, &mut successful, &mut failed);

        if opts.output_format == OutputFormat::Tsv {
            if let Ok(info) = analyze(file) {
                println!(
                    "{}\t{}\t{:.1}\t{:.6}\t{}\t{}",
                    filename,
                    steps,
                    db_value,
                    1.0,
                    info.max_gain(),
                    info.min_gain()
                );
            }
        }

        if opts.output_format == OutputFormat::Json {
            json_results.push(result);
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
    } else {
        print_dry_run_notice(opts);
    }

    Ok(())
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
    let channel_name = match channel {
        Channel::Left => "left",
        Channel::Right => "right",
        _ => unreachable!(),
    };

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

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        let result = process_apply_channel(file, channel, steps, opts)?;
        update_counters(&result, &mut successful, &mut failed);

        if opts.output_format == OutputFormat::Json {
            json_results.push(result);
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
    } else {
        print_dry_run_notice(opts);
    }

    Ok(())
}
