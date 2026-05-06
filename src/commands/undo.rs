use anyhow::Result;
use colored::*;
use rayon::prelude::*;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::cli::options::{Options, OutputFormat};
use crate::commands::threading::effective_threads;
use crate::commands::utils::{create_json_summary, print_dry_run_notice, update_counters};
use crate::json_output::{JsonFileResult, JsonOutput};
use crate::processors::undo::process_undo;
use crate::progress::{create_progress_bar, progress_finish, progress_inc, progress_set_message};
use crate::util::get_filename;

pub fn cmd_undo(files: &[PathBuf], opts: &Options) -> Result<()> {
    let dry_run_prefix = opts.dry_run_prefix();

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{}{} {} gain changes on {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            if opts.dry_run {
                "Would undo"
            } else {
                "Undoing"
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
                let r = process_undo(file, opts)?;
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
            update_counters(&result, &mut successful, &mut failed);
            if opts.output_format == OutputFormat::Json {
                json_results.push(result);
            }
        }
    } else {
        for file in files {
            let filename = get_filename(file);
            progress_set_message(&pb, filename);

            let (result, text) = process_undo(file, opts)?;
            if !text.is_empty() {
                print!("{}", text);
            }
            update_counters(&result, &mut successful, &mut failed);

            if opts.output_format == OutputFormat::Json {
                json_results.push(result);
            }

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
