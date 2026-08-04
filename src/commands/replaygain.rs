use anyhow::Result;
use colored::*;
use mp3rgain::replaygain::{self, AlbumAnalysisReport, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::{steps_to_db, AacAlbumInfo};
use rayon::prelude::*;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::commands::threading::effective_threads;
use crate::commands::utils::{
    create_json_summary, exit_if_failed, finish_with_summary, for_each_file_with_analysis_bar,
    print_dry_run_notice, run_album_analysis, update_counters,
};
use crate::json_output::{FileStatus, JsonAlbumResult, JsonFileResult, JsonOutput};
use crate::processors::replaygain::{process_apply_replaygain_with_album, process_track_gain};
use crate::progress::{create_progress_bar, progress_finish, progress_inc, progress_set_message};
use crate::util::get_filename;

fn print_target_with_modifier(opts: &Options) {
    let mode = opts.analysis_mode;
    let target = mode.target_lufs().unwrap_or(REPLAYGAIN_REFERENCE_DB);
    let modifier_steps = opts.gain_modifier_steps();
    if modifier_steps != 0 {
        let modifier_db = steps_to_db(modifier_steps);
        println!(
            "  Target: {:.1} {} ({} {} {} {:+.1} dB modifier)",
            target + modifier_db,
            mode.unit(),
            mode,
            target,
            mode.unit(),
            modifier_db,
        );
    } else {
        println!("  Target: {} {} ({})", target, mode.unit(), mode);
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

    let (json_results, successful, failed) =
        for_each_file_with_analysis_bar(files, opts, |file, analysis_pb| {
            process_track_gain(file, opts, analysis_pb).map(|(r, t)| (Some(r), t))
        })?;

    finish_with_summary(files.len(), json_results, successful, failed, opts)
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

    let album_analysis = run_album_analysis(&file_refs, opts, threads, parallel, opts.skip_errors);

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

            let is_lufs = opts.analysis_mode.target_lufs().is_some();
            let json_album = JsonAlbumResult {
                loudness_db: album_result.album_loudness_db(),
                loudness_lufs: is_lufs.then(|| album_result.album_loudness_db()),
                analysis_mode: Some(opts.analysis_mode.name()),
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
                    "  Album loudness: {:.1} {}",
                    album_result.album_loudness_db(),
                    opts.analysis_mode.unit()
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
                                    status: Some(FileStatus::Skipped),
                                    gain_applied_steps: Some(0),
                                    gain_applied_db: Some(0.0),
                                    ..JsonFileResult::from_analysis(file, track)
                                }
                            }
                            None => JsonFileResult::error(
                                file,
                                failure_msgs[i].as_deref().unwrap_or("analysis failed"),
                            ),
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
                exit_if_failed(failure_count);
                return Ok(());
            }

            let pb = create_progress_bar(files.len(), opts);
            let mut json_results: Vec<JsonFileResult> = Vec::with_capacity(files.len());
            let mut successful = 0;
            let mut failed = 0;
            // Post-apply (max, min) global_gain range per file, taken from the
            // apply pass so the album MINMAX step below doesn't re-analyze
            // every file (issue #232).
            let mut range_by_idx: Vec<Option<(u8, u8)>> = vec![None; files.len()];

            if parallel {
                let pb_ref = pb.as_ref();
                // Process only successfully-analyzed files in parallel.
                type Collected = (usize, JsonFileResult, String, Option<(u8, u8)>);
                let collected: Vec<Collected> = successful_indices
                    .par_iter()
                    .enumerate()
                    .map(|(track_idx, &file_idx)| -> Result<Collected> {
                        let file = &files[file_idx];
                        let track_result = &album_result.tracks()[track_idx];
                        let (result, text, range) = process_apply_replaygain_with_album(
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
                        Ok((file_idx, result, text, range))
                    })
                    .collect::<Result<Vec<_>>>()?;

                let stdout = io::stdout();
                let mut handle = stdout.lock();
                for (_, _, text, _) in &collected {
                    if !text.is_empty() {
                        handle.write_all(text.as_bytes())?;
                    }
                }
                drop(handle);

                for (file_idx, _, _, range) in &collected {
                    range_by_idx[*file_idx] = *range;
                }

                // Re-assemble json_results in input file order, interleaving
                // failures with successes so the JSON output stays aligned
                // with the input list.
                if opts.output_format == OutputFormat::Json {
                    let mut by_index: Vec<Option<JsonFileResult>> =
                        (0..files.len()).map(|_| None).collect();
                    for (file_idx, result, _, _) in &collected {
                        by_index[*file_idx] = Some(result.clone());
                    }
                    for (i, slot) in by_index.iter_mut().enumerate() {
                        if slot.is_none() {
                            if let Some(msg) = &failure_msgs[i] {
                                *slot = Some(JsonFileResult::error(&files[i], msg));
                            }
                        }
                    }
                    for entry in by_index.into_iter().flatten() {
                        update_counters(&entry, &mut successful, &mut failed);
                        json_results.push(entry);
                    }
                } else {
                    for (_, result, _, _) in collected {
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
                            let (result, text, range) = process_apply_replaygain_with_album(
                                file,
                                steps,
                                track_result,
                                opts,
                                Some(&album_info),
                            )?;
                            if !text.is_empty() {
                                print!("{}", text);
                            }
                            range_by_idx[i] = range;
                            result
                        }
                        None => JsonFileResult::error(
                            file,
                            failure_msgs[i].as_deref().unwrap_or("analysis failed"),
                        ),
                    };
                    update_counters(&result, &mut successful, &mut failed);

                    if opts.output_format == OutputFormat::Json {
                        json_results.push(result);
                    }

                    progress_inc(&pb);
                }
            }

            progress_finish(pb);

            // MP3GAIN_ALBUM_MINMAX: the album-wide post-apply global_gain range,
            // matching mp3gain's album (`-a`) mode (issue #210). Written to every
            // MP3 file after all gain is applied (the range is only known once the
            // whole album is done). APEv2 only — mp3gain has no AAC, and `-s i`
            // uses ID3v2; best-effort, so a tag hiccup never fails the album.
            if !opts.dry_run && opts.stored_tag_mode != StoredTagMode::Skip && !opts.use_id3v2 {
                let album_files: Vec<(&Path, Option<(u8, u8)>)> = successful_indices
                    .iter()
                    .map(|&i| (files[i].as_path(), range_by_idx[i]))
                    .collect();
                mp3rgain::write_album_minmax(&album_files);
            }

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
            exit_if_failed(failed);
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
