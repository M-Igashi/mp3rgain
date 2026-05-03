use colored::*;

use crate::cli::options::{Options, OutputFormat};
use crate::json_output::{JsonFileResult, JsonSummary};

pub fn update_counters(result: &JsonFileResult, successful: &mut usize, failed: &mut usize) {
    match result.status.as_deref() {
        Some("success") => *successful += 1,
        Some("error") => *failed += 1,
        _ => {}
    }
}

pub fn create_json_summary(
    total_files: usize,
    successful: usize,
    failed: usize,
    dry_run: bool,
) -> JsonSummary {
    JsonSummary {
        total_files,
        successful,
        failed,
        dry_run: if dry_run { Some(true) } else { None },
    }
}

pub fn print_dry_run_notice(opts: &Options) {
    if opts.dry_run && !opts.quiet && opts.output_format == OutputFormat::Text {
        println!();
        println!("{}", "No files were modified.".yellow());
    }
}
