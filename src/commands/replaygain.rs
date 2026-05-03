use anyhow::Result;
use colored::*;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mp3rgain::replaygain::{self, REPLAYGAIN_REFERENCE_DB};
use std::path::PathBuf;

use crate::cli::options::{AacAlbumInfo, Options, OutputFormat};
use crate::commands::utils::{create_json_summary, print_dry_run_notice, update_counters};
use crate::json_output::{JsonAlbumResult, JsonFileResult, JsonOutput};
use crate::processors::replaygain::{process_apply_replaygain_with_album, process_track_gain};
use crate::progress::{
    create_analysis_progress_bar, create_progress_bar, finish_analysis_progress, progress_finish,
    progress_inc, progress_set_message, PROGRESS_THRESHOLD,
};
use crate::util::get_filename;

pub fn cmd_track_gain(files: &[PathBuf], opts: &Options) -> Result<()> {
    if !replaygain::is_available() {
        eprintln!(
            "{}: ReplayGain analysis requires the 'replaygain' feature",
            "error".red().bold()
        );
        eprintln!("  Install with: cargo install mp3rgain --features replaygain");
        std::process::exit(1);
    }

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

    let mp = MultiProgress::new();
    let file_pb = if !opts.quiet
        && opts.output_format == OutputFormat::Text
        && files.len() >= PROGRESS_THRESHOLD
    {
        let pb = mp.add(ProgressBar::new(files.len() as u64));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );
        Some(pb)
    } else {
        None
    };

    let mut json_results: Vec<JsonFileResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for file in files {
        let filename = get_filename(file);
        if let Some(ref pb) = file_pb {
            pb.set_message(filename.to_string());
        }

        let analysis_pb = create_analysis_progress_bar(&mp, file, opts);
        let result = process_track_gain(file, opts, analysis_pb.as_ref())?;
        finish_analysis_progress(analysis_pb);

        update_counters(&result, &mut successful, &mut failed);

        if opts.output_format == OutputFormat::Json {
            json_results.push(result);
        }

        if let Some(ref pb) = file_pb {
            pb.inc(1);
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
    if !replaygain::is_available() {
        eprintln!(
            "{}: ReplayGain analysis requires the 'replaygain' feature",
            "error".red().bold()
        );
        eprintln!("  Install with: cargo install mp3rgain --features replaygain");
        std::process::exit(1);
    }

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

    let show_progress = !opts.quiet && opts.output_format == OutputFormat::Text;
    let mp = MultiProgress::new();

    let album_analysis = if show_progress {
        let analysis_pb = mp.add(ProgressBar::new(0));
        analysis_pb.set_style(
            ProgressStyle::default_bar()
                .template("      [{bar:30.cyan/blue}] {bytes}/{total_bytes} {msg}")
                .unwrap()
                .progress_chars("=>-"),
        );

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
                                status: Some("skipped".to_string()),
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
            let mut json_results: Vec<JsonFileResult> = Vec::new();
            let mut successful = 0;
            let mut failed = 0;

            for (i, file) in files.iter().enumerate() {
                let filename = get_filename(file);
                progress_set_message(&pb, filename);

                let track_result = &album_result.tracks()[i];
                let result = process_apply_replaygain_with_album(
                    file,
                    steps,
                    track_result,
                    opts,
                    Some(&album_info),
                )?;
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
