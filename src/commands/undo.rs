use anyhow::Result;
use colored::*;
use std::path::PathBuf;

use crate::cli::options::{Options, OutputFormat};
use crate::commands::utils::{finish_with_summary, for_each_file};
use crate::processors::undo::process_undo;

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

    let (json_results, successful, failed) = for_each_file(files, opts, |file| {
        let (result, text) = process_undo(file, opts)?;
        Ok((Some(result), text))
    })?;

    finish_with_summary(files.len(), json_results, successful, failed, opts)
}
