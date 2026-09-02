use anyhow::Result;
use colored::*;
use indicatif::{MultiProgress, ProgressBar};
use mp3rgain::replaygain::{self, AlbumAnalysisReport};
use rayon::prelude::*;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::threading::effective_threads;
use crate::json_output::{FileStatus, JsonAlbumResult, JsonFileResult, JsonOutput, JsonSummary};
use crate::progress::{
    album_progress_callbacks, create_album_progress_pb_in, create_analysis_progress_bar,
    create_file_count_pb_in, create_progress_bar, progress_finish, progress_inc,
    progress_set_message,
};
use crate::util::get_filename;

/// Run one album analysis over `paths`, driving the shared album progress
/// bar. Serial runs get the byte-level bar via `on_progress`; parallel runs
/// tick per file via `on_complete`. Shared by `cmd_info` and `cmd_album_gain`.
/// The thread count follows `-j` (see [`effective_threads`]); only
/// `skip_errors` varies between callers.
pub fn run_album_analysis(
    paths: &[&Path],
    opts: &Options,
    skip_errors: bool,
) -> mp3rgain::error::Result<AlbumAnalysisReport> {
    let threads = effective_threads(opts);
    let parallel = threads > 1 && paths.len() > 1;
    let show_progress = !opts.quiet && opts.output_format == OutputFormat::Text;
    let mp = MultiProgress::new();
    let pb = if show_progress {
        Some(create_album_progress_pb_in(&mp, paths.len(), parallel))
    } else {
        None
    };
    let file_names: Vec<&str> = paths.iter().map(|p| get_filename(p)).collect();
    let (on_progress, on_complete) = album_progress_callbacks(&pb, file_names);

    let report = replaygain::analyze_album_with_options(
        paths,
        &replaygain::AlbumAnalysisOptions {
            track_index: opts.track_index,
            threads,
            skip_errors,
            on_progress: (!parallel).then_some(&on_progress as _),
            on_complete: parallel.then_some(&on_complete as _),
            cancel: None,
            mode: opts.analysis_mode,
            true_peak: opts.true_peak,
        },
    );

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }
    report
}

/// mp3gain-compatible TSV header shared by the info and apply commands.
pub const TSV_HEADER: &str =
    "File\tMP3 gain\tdB gain\tMax Amplitude\tMax global_gain\tMin global_gain";

/// Run `per_file` over every file — in parallel when `-j` allows it — with
/// progress reporting and ordered stdout flushing. `per_file` returns the
/// optional JSON record plus the text to print for that file; records are
/// collected in input order and counted into (successful, failed).
///
/// This is the shared fan-out driver for the per-file commands (apply,
/// channel apply, undo, delete tags, check tags, max amplitude), which all
/// repeated the progress-bar / par_iter / ordered-flush / counter loop.
pub fn for_each_file<F>(
    files: &[PathBuf],
    opts: &Options,
    per_file: F,
) -> Result<(Vec<JsonFileResult>, usize, usize)>
where
    F: Fn(&Path) -> Result<(Option<JsonFileResult>, String)> + Sync,
{
    for_each_file_impl(files, opts, false, |file, _| per_file(file))
}

/// [`for_each_file`] variant for the analysis-heavy commands (track gain,
/// basic info): the serial path additionally shows a per-file byte-level
/// analysis bar, passed to `per_file`. Parallel runs skip it (concurrent
/// byte bars would interleave); only the file-count bar runs there.
pub fn for_each_file_with_analysis_bar<F>(
    files: &[PathBuf],
    opts: &Options,
    per_file: F,
) -> Result<(Vec<JsonFileResult>, usize, usize)>
where
    F: Fn(&Path, Option<&ProgressBar>) -> Result<(Option<JsonFileResult>, String)> + Sync,
{
    for_each_file_impl(files, opts, true, per_file)
}

fn for_each_file_impl<F>(
    files: &[PathBuf],
    opts: &Options,
    analysis_bars: bool,
    per_file: F,
) -> Result<(Vec<JsonFileResult>, usize, usize)>
where
    F: Fn(&Path, Option<&ProgressBar>) -> Result<(Option<JsonFileResult>, String)> + Sync,
{
    let mp = MultiProgress::new();
    let pb = if analysis_bars {
        create_file_count_pb_in(&mp, files.len(), opts)
    } else {
        create_progress_bar(files.len(), opts)
    };
    let mut json_results: Vec<JsonFileResult> = Vec::with_capacity(files.len());
    let mut successful = 0;
    let mut failed = 0;

    let parallel = effective_threads(opts) > 1 && files.len() > 1;

    if parallel {
        let pb_ref = pb.as_ref();
        let collected: Vec<(Option<JsonFileResult>, String)> = files
            .par_iter()
            .map(|file| -> Result<(Option<JsonFileResult>, String)> {
                let r = per_file(file, None)?;
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
            if let Some(result) = result {
                update_counters(&result, &mut successful, &mut failed);
                json_results.push(result);
            }
        }
    } else {
        for file in files {
            progress_set_message(&pb, get_filename(file));

            let analysis_pb = if analysis_bars {
                create_analysis_progress_bar(&mp, file, opts)
            } else {
                None
            };
            let (result, text) = per_file(file, analysis_pb.as_ref())?;
            progress_finish(analysis_pb);

            if !text.is_empty() {
                print!("{}", text);
            }
            if let Some(result) = result {
                update_counters(&result, &mut successful, &mut failed);
                json_results.push(result);
            }

            progress_inc(&pb);
        }
    }

    progress_finish(pb);
    Ok((json_results, successful, failed))
}

/// Shared `steps == 0` early return for the apply commands: empty JSON
/// output with a zero summary, or a "nothing to do" notice.
pub fn report_zero_steps(files: &[PathBuf], opts: &Options) -> Result<()> {
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
    Ok(())
}

/// Shared command epilogue: JSON output (with per-run summary) in JSON mode,
/// otherwise the dry-run notice.
pub fn finish_with_summary(
    total_files: usize,
    json_results: Vec<JsonFileResult>,
    successful: usize,
    failed: usize,
    opts: &Options,
) -> Result<()> {
    finish_with_album_summary(total_files, json_results, None, successful, failed, opts)
}

/// [`finish_with_summary`] carrying an album block, for the `-a` paths whose
/// JSON output has one.
pub fn finish_with_album_summary(
    total_files: usize,
    json_results: Vec<JsonFileResult>,
    album: Option<JsonAlbumResult>,
    successful: usize,
    failed: usize,
    opts: &Options,
) -> Result<()> {
    if opts.output_format == OutputFormat::Json {
        let output = JsonOutput {
            files: Some(json_results),
            album,
            summary: Some(create_json_summary(
                total_files,
                successful,
                failed,
                opts.dry_run,
            )),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_dry_run_notice(opts);
    }
    exit_if_failed(failed);
    Ok(())
}

/// Exit non-zero when any per-file operation failed, so scripts can detect
/// partial failures (issue #228).
pub fn exit_if_failed(failed: usize) {
    if failed > 0 {
        std::process::exit(1);
    }
}

/// Epilogue for read-only commands (check tags, max amplitude): JSON output
/// without a summary block, nothing otherwise.
pub fn finish_without_summary(json_results: Vec<JsonFileResult>, opts: &Options) -> Result<()> {
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
