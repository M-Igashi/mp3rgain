use colored::*;

use crate::cli::options::{Options, OutputFormat};
use crate::json_output::{FileStatus, JsonFileResult, JsonSummary};

pub fn update_counters(result: &JsonFileResult, successful: &mut usize, failed: &mut usize) {
    match result.status {
        Some(FileStatus::Success) => *successful += 1,
        Some(FileStatus::Error) => *failed += 1,
        Some(FileStatus::Skipped)
        | Some(FileStatus::DryRun)
        | Some(FileStatus::Info)
        | Some(FileStatus::NoTag)
        | None => {}
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
