use anyhow::Result;
use colored::*;
use indicatif::MultiProgress;
use mp3rgain::replaygain::{self, AlbumAnalysisReport, AlbumGainResult, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::{steps_to_db, AacAlbumInfo};
use rayon::prelude::*;
use std::cell::Cell;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::threading::effective_threads;
use crate::commands::utils::{create_json_summary, print_dry_run_notice, update_counters};
use crate::json_output::{FileStatus, JsonAlbumResult, JsonFileResult, JsonOutput};
use crate::processors::replaygain::{process_apply_replaygain_with_album, process_track_gain};
use crate::progress::{
    create_album_progress_pb_in, create_analysis_progress_bar, create_file_count_pb_in,
    create_progress_bar, finish_analysis_progress, progress_finish, progress_inc,
    progress_set_message,
};
use crate::util::get_filename;

fn print_target_with_modifier(opts: &Options) {
    let modifier_steps = opts.gain_modifier_steps();
    if modifier_steps != 0 {
        let modifier_db = steps_to_db(modifier_steps);
        println!(
            "  Target: {:.1} dB (ReplayGain {} dB {:+.1} dB modifier)",
            REPLAYGAIN_REFERENCE_DB + modifier_db,
            REPLAYGAIN_REFERENCE_DB,
            modifier_db,
        );
    } else {
        println!("  Target: {} dB (ReplayGain 1.0)", REPLAYGAIN_REFERENCE_DB);
    }
}

fn require_replaygain_feature() {
    if !replaygain::is_available() {
        eprintln!(
            "{}: ReplayGain analysis requires the 'replaygain' feature",
            "error".red().bold()
        );
        eprintln!("  Install with: cargo install mp3rgain --features replaygain");
        std::process::exit(1);
    }
}

/// Wrap a strict `AlbumGainResult` in the lenient report shape so the rest of
/// `cmd_album_gain` can handle both paths uniformly.
fn lift_strict(album: AlbumGainResult, file_count: usize) -> AlbumAnalysisReport {
    AlbumAnalysisReport {
        album,
        failures: Vec::new(),
        successful_indices: (0..file_count).collect(),
    }
}

fn failure_json_result(file: &Path, msg: Option<String>) -> JsonFileResult {
    JsonFileResult {
        file: file.display().to_string(),
        status: Some(FileStatus::Error),
        error: msg,
        ..Default::default()
    }
}

pub fn cmd_track_gain(files: &[PathBuf], opts: &Options) -> Result<()> {
    require_replaygain_feature();

    let dry_run_prefix = opts.dry_run_prefix();

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{}{} Analyzing and {} track gain to {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            if opts.dry_run {
                "would apply"
            } else {
                "applying"
            },
            files.len()
        );
        print_target_with_modifier(opts);
        println!();
    }

    let threads = effective_threads(opts);
    let parallel = threads > 1 && files.len() > 1;

    let mp = MultiProgress::new();
    let file_pb = create_file_count_pb_in(&mp, files.len(), opts);

    let mut json_results: Vec<JsonFileResult> = Vec::with_capacity(files.len());
    let mut successful = 0;
    let mut failed = 0;

    if parallel {
        let file_pb_ref = file_pb.as_ref();
        let collected: Vec<(JsonFileResult, String)> = files
            .par_iter()
            .map(|file| -> Result<(JsonFileResult, String)> {
                let r = process_track_gain(file, opts, None)?;
                if let Some(pb) = file_pb_ref {
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
            if let Some(ref pb) = file_pb {
                pb.set_message(filename.to_string());
            }

            let analysis_pb = create_analysis_progress_bar(&mp, file, opts);
            let (result, text) = process_track_gain(file, opts, analysis_pb.as_ref())?;
            finish_analysis_progress(analysis_pb);

            if !text.is_empty() {
                print!("{}", text);
            }

            update_counters(&result, &mut successful, &mut failed);

            if opts.output_format == OutputFormat::Json {
                json_results.push(result);
            }

            if let Some(ref pb) = file_pb {
                pb.inc(1);
            }
        }
    }

    if let Some(pb) = file_pb {
        pb.finish_and_clear();
    }

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

pub fn cmd_album_gain(files: &[PathBuf], opts: &Options) -> Result<()> {
    require_replaygain_feature();

    let dry_run_prefix = opts.dry_run_prefix();

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{}{} Analyzing album gain for {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            files.len()
        );
        print_target_with_modifier(opts);
        println!();
    }

    // First, analyze all tracks
    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!("  {} Analyzing tracks...", "->".cyan());
    }

    let file_refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();

    let threads = effective_threads(opts);
    let parallel = threads > 1 && files.len() > 1;

    let show_progress = !opts.quiet && opts.output_format == OutputFormat::Text;
    let mp = MultiProgress::new();

    // One progress bar shared between strict/lenient and serial/parallel paths.
    // Serial paths drive byte-level progress via `on_progress`; parallel paths
    // drive file-count progress via `on_complete`.
    let analysis_pb = if show_progress {
        Some(create_album_progress_pb_in(&mp, files.len(), parallel))
    } else {
        None
    };
    let file_names: Vec<&str> = files.iter().map(|f| get_filename(f)).collect();
    let files_len = files.len();

    let pb_for_progress = analysis_pb.clone();
    // Track the most recent file_idx so we only allocate a new message
    // string when the current file changes (otherwise the closure runs
    // once per decoded packet — ~9k calls per minute of audio).
    let last_message_idx: Cell<Option<usize>> = Cell::new(None);
    let on_progress = move |file_idx: usize, bytes: u64, total: u64| {
        if let Some(pb) = &pb_for_progress {
            pb.set_length(total);
            pb.set_position(bytes);
            if last_message_idx.get() != Some(file_idx) {
                pb.set_message(format!(
                    "({}/{}) {}",
                    file_idx + 1,
                    files_len,
                    file_names[file_idx]
                ));
                last_message_idx.set(Some(file_idx));
            }
        }
    };
    let pb_for_complete = analysis_pb.clone();
    let on_complete = move |completed_idx: usize, _path: &Path| {
        if let Some(pb) = &pb_for_complete {
            pb.set_position((completed_idx + 1) as u64);
        }
    };

    let album_analysis: mp3rgain::error::Result<AlbumAnalysisReport> =
        match (parallel, opts.skip_errors) {
            (false, false) => {
                replaygain::analyze_album_with_progress(&file_refs, opts.track_index, &on_progress)
                    .map(|album| lift_strict(album, files.len()))
            }
            (true, false) => replaygain::analyze_album_parallel_with_completion(
                &file_refs,
                opts.track_index,
                threads,
                &on_complete,
            )
            .map(|album| lift_strict(album, files.len())),
            (false, true) => replaygain::analyze_album_lenient_with_progress(
                &file_refs,
                opts.track_index,
                &on_progress,
            ),
            (true, true) => replaygain::analyze_album_lenient_parallel_with_completion(
                &file_refs,
                opts.track_index,
                threads,
                &on_complete,
            ),
        };

    if let Some(pb) = analysis_pb {
        pb.finish_and_clear();
    }

    match album_analysis {
        Ok(report) => {
            let AlbumAnalysisReport {
                album: album_result,
                failures,
                successful_indices,
            } = report;

            // Index failures by file position for O(1) lookup during the
            // apply phase (only --skip-errors produces non-empty failures).
            // Walk failures once, reporting skipped files in text mode.
            let failure_count = failures.len();
            let mut failure_msgs: Vec<Option<String>> = vec![None; files.len()];
            let report_skipped = opts.output_format == OutputFormat::Text && !opts.quiet;
            for (idx, msg) in failures {
                if report_skipped {
                    let filename = get_filename(&files[idx]);
                    eprintln!("  {} {} - {} (skipped)", "x".red(), filename, msg);
                }
                failure_msgs[idx] = Some(msg);
            }

            // Build a mapping from file index -> track-result index. Files that
            // failed analysis map to None.
            let mut file_to_track: Vec<Option<usize>> = vec![None; files.len()];
            for (track_idx, file_idx) in successful_indices.iter().enumerate() {
                file_to_track[*file_idx] = Some(track_idx);
            }

            // Apply gain modifier (-m steps + -d dB, combined into steps)
            let modifier_steps = opts.gain_modifier_steps();
            let modified_gain_steps = album_result.album_gain_steps() + modifier_steps;

            let json_album = JsonAlbumResult {
                loudness_db: album_result.album_loudness_db(),
                gain_db: album_result.album_gain_db(),
                gain_steps: modified_gain_steps,
                peak: album_result.album_peak(),
            };

            let album_info = AacAlbumInfo {
                album_gain_db: album_result.album_gain_db(),
                album_peak: album_result.album_peak(),
            };

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!();
                println!(
                    "  Album loudness: {:.1} dB",
                    album_result.album_loudness_db()
                );
                println!(
                    "  Album gain:     {:+.1} dB ({} steps{})",
                    album_result.album_gain_db(),
                    album_result.album_gain_steps(),
                    if modifier_steps != 0 {
                        format!(" + {} = {}", modifier_steps, modified_gain_steps)
                    } else {
                        String::new()
                    }
                );
                println!("  Album peak:     {:.4}", album_result.album_peak());
                println!();
            }

            // Apply album gain to all files
            let steps = modified_gain_steps;

            if steps == 0 {
                if opts.output_format == OutputFormat::Json {
                    let json_results: Vec<JsonFileResult> = files
                        .iter()
                        .enumerate()
                        .map(|(i, file)| match file_to_track[i] {
                            Some(track_idx) => {
                                let track = &album_result.tracks()[track_idx];
                                JsonFileResult {
                                    file: file.display().to_string(),
                                    status: Some(FileStatus::Skipped),
                                    loudness_db: Some(track.loudness_db()),
                                    peak: Some(track.peak()),
                                    gain_applied_steps: Some(0),
                                    gain_applied_db: Some(0.0),
                                    ..Default::default()
                                }
                            }
                            None => failure_json_result(file, failure_msgs[i].clone()),
                        })
                        .collect();

                    let output = JsonOutput {
                        files: Some(json_results),
                        album: Some(json_album),
                        summary: Some(create_json_summary(
                            files.len(),
                            0,
                            failure_count,
                            opts.dry_run,
                        )),
                    };
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else if !opts.quiet {
                    println!("  {} No adjustment needed", ".".cyan());
                }
                return Ok(());
            }

            let pb = create_progress_bar(files.len(), opts);
            let mut json_results: Vec<JsonFileResult> = Vec::with_capacity(files.len());
            let mut successful = 0;
            let mut failed = 0;

            if parallel {
                let pb_ref = pb.as_ref();
                // Process only successfully-analyzed files in parallel.
                let collected: Vec<(usize, JsonFileResult, String)> = successful_indices
                    .par_iter()
                    .enumerate()
                    .map(
                        |(track_idx, &file_idx)| -> Result<(usize, JsonFileResult, String)> {
                            let file = &files[file_idx];
                            let track_result = &album_result.tracks()[track_idx];
                            let (result, text) = process_apply_replaygain_with_album(
                                file,
                                steps,
                                track_result,
                                opts,
                                Some(&album_info),
                            )?;
                            if let Some(pb) = pb_ref {
                                pb.set_message(get_filename(file).to_string());
                                pb.inc(1);
                            }
                            Ok((file_idx, result, text))
                        },
                    )
                    .collect::<Result<Vec<_>>>()?;

                let stdout = io::stdout();
                let mut handle = stdout.lock();
                for (_, _, text) in &collected {
                    if !text.is_empty() {
                        handle.write_all(text.as_bytes())?;
                    }
                }
                drop(handle);

                // Re-assemble json_results in input file order, interleaving
                // failures with successes so the JSON output stays aligned
                // with the input list.
                if opts.output_format == OutputFormat::Json {
                    let mut by_index: Vec<Option<JsonFileResult>> =
                        (0..files.len()).map(|_| None).collect();
                    for (file_idx, result, _) in &collected {
                        by_index[*file_idx] = Some(result.clone());
                    }
                    for (i, slot) in by_index.iter_mut().enumerate() {
                        if slot.is_none() {
                            if let Some(msg) = failure_msgs[i].clone() {
                                *slot = Some(failure_json_result(&files[i], Some(msg)));
                            }
                        }
                    }
                    for entry in by_index.into_iter().flatten() {
                        update_counters(&entry, &mut successful, &mut failed);
                        json_results.push(entry);
                    }
                } else {
                    for (_, result, _) in collected {
                        update_counters(&result, &mut successful, &mut failed);
                    }
                    failed += failure_count;
                }
            } else {
                for (i, file) in files.iter().enumerate() {
                    let filename = get_filename(file);
                    progress_set_message(&pb, filename);

                    let result = match file_to_track[i] {
                        Some(track_idx) => {
                            let track_result = &album_result.tracks()[track_idx];
                            let (result, text) = process_apply_replaygain_with_album(
                                file,
                                steps,
                                track_result,
                                opts,
                                Some(&album_info),
                            )?;
                            if !text.is_empty() {
                                print!("{}", text);
                            }
                            result
                        }
                        None => failure_json_result(file, failure_msgs[i].clone()),
                    };
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
                    album: Some(json_album),
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
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Json {
                let output = JsonOutput {
                    files: None,
                    album: None,
                    summary: Some(create_json_summary(
                        files.len(),
                        0,
                        files.len(),
                        opts.dry_run,
                    )),
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else {
                eprintln!("{}: Failed to analyze album: {}", "error".red().bold(), e);
            }
            std::process::exit(1);
        }
    }

    Ok(())
}
