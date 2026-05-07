use anyhow::Result;
use colored::*;
use indicatif::MultiProgress;
use mp3rgain::replaygain::{self, REPLAYGAIN_REFERENCE_DB};
use rayon::prelude::*;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::cli::options::{AacAlbumInfo, Options, OutputFormat};
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
        println!("  Target: {} dB (ReplayGain 1.0)", REPLAYGAIN_REFERENCE_DB);
        if opts.gain_modifier != 0 {
            println!("  Gain modifier: {:+} steps", opts.gain_modifier);
        }
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
        println!("  Target: {} dB (ReplayGain 1.0)", REPLAYGAIN_REFERENCE_DB);
        if opts.gain_modifier != 0 {
            println!("  Gain modifier: {:+} steps", opts.gain_modifier);
        }
        println!();
    }

    // First, analyze all tracks
    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!("  {} Analyzing tracks...", "->".cyan());
    }

    let file_refs: Vec<&std::path::Path> = files.iter().map(|p| p.as_path()).collect();

    let threads = effective_threads(opts);
    let parallel = threads > 1 && files.len() > 1;

    let show_progress = !opts.quiet && opts.output_format == OutputFormat::Text;
    let mp = MultiProgress::new();

    let album_analysis = if show_progress && !parallel {
        let analysis_pb = create_album_progress_pb_in(&mp, files.len(), false);

        let file_names: Vec<&str> = files.iter().map(|f| get_filename(f)).collect();
        let result = replaygain::analyze_album_with_progress(
            &file_refs,
            opts.track_index,
            &|file_idx, bytes, total| {
                analysis_pb.set_length(total);
                analysis_pb.set_position(bytes);
                analysis_pb.set_message(format!(
                    "({}/{}) {}",
                    file_idx + 1,
                    files.len(),
                    file_names[file_idx]
                ));
            },
        );

        analysis_pb.finish_and_clear();
        result
    } else if show_progress && parallel {
        // Per-file byte bars would interleave in parallel mode, so show a
        // file-count bar driven by completion notifications instead.
        let analysis_pb = create_album_progress_pb_in(&mp, files.len(), true);

        let pb_ref = &analysis_pb;
        let result = replaygain::analyze_album_parallel_with_completion(
            &file_refs,
            opts.track_index,
            threads,
            &|completed_idx, _path| {
                pb_ref.set_position((completed_idx + 1) as u64);
            },
        );

        analysis_pb.finish_and_clear();
        result
    } else if parallel {
        replaygain::analyze_album_parallel(&file_refs, opts.track_index, threads)
    } else {
        replaygain::analyze_album_with_index(&file_refs, opts.track_index)
    };

    match album_analysis {
        Ok(album_result) => {
            // Apply gain modifier
            let modified_gain_steps = album_result.album_gain_steps() + opts.gain_modifier;

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
                    if opts.gain_modifier != 0 {
                        format!(" + {} = {}", opts.gain_modifier, modified_gain_steps)
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
                        .map(|(i, file)| {
                            let track = &album_result.tracks()[i];
                            JsonFileResult {
                                file: file.display().to_string(),
                                status: Some(FileStatus::Skipped),
                                loudness_db: Some(track.loudness_db()),
                                peak: Some(track.peak()),
                                gain_applied_steps: Some(0),
                                gain_applied_db: Some(0.0),
                                ..Default::default()
                            }
                        })
                        .collect();

                    let output = JsonOutput {
                        files: Some(json_results),
                        album: Some(json_album),
                        summary: Some(create_json_summary(files.len(), 0, 0, opts.dry_run)),
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
                let collected: Vec<(JsonFileResult, String)> = files
                    .par_iter()
                    .enumerate()
                    .map(|(i, file)| -> Result<(JsonFileResult, String)> {
                        let track_result = &album_result.tracks()[i];
                        let r = process_apply_replaygain_with_album(
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
                for (i, file) in files.iter().enumerate() {
                    let filename = get_filename(file);
                    progress_set_message(&pb, filename);

                    let track_result = &album_result.tracks()[i];
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
