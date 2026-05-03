use anyhow::Result;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mp3rgain::replaygain;
use mp3rgain::{db_to_steps, mp4meta};
use rayon::prelude::*;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::threading::effective_threads;
use crate::json_output::{JsonFileResult, JsonOutput};
use crate::processors::info::process_info;
use crate::progress::{create_analysis_progress_bar, finish_analysis_progress, PROGRESS_THRESHOLD};
use crate::util::get_filename;

pub fn cmd_info(files: &[PathBuf], opts: &Options) -> Result<()> {
    // Print mp3gain-compatible TSV header
    if opts.output_format == OutputFormat::Tsv {
        println!("File\tMP3 gain\tdB gain\tMax Amplitude\tMax global_gain\tMin global_gain");
    }

    let threads = effective_threads(opts);
    let parallel = threads > 1 && files.len() > 1;

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

    let json_results: Vec<JsonFileResult> = if parallel {
        // Parallel: run process_info per file in rayon's pool. Per-file
        // analysis progress bars would interleave confusingly when N files
        // run concurrently, so we skip them in parallel mode and only show
        // the file-count progress bar.
        let file_pb_ref = file_pb.as_ref();
        let collected: Vec<(JsonFileResult, String)> = files
            .par_iter()
            .map(|file| -> Result<(JsonFileResult, String)> {
                let r = process_info(file, opts, None)?;
                if let Some(pb) = file_pb_ref {
                    pb.set_message(get_filename(file).to_string());
                    pb.inc(1);
                }
                Ok(r)
            })
            .collect::<Result<Vec<_>>>()?;

        // Emit captured stdout in input order so TSV/Text output stays
        // deterministic regardless of completion order.
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        for (_, text) in &collected {
            if !text.is_empty() {
                handle.write_all(text.as_bytes())?;
            }
        }
        drop(handle);

        collected.into_iter().map(|(r, _)| r).collect()
    } else {
        // Serial path preserves the original behavior, including the
        // per-file analysis progress bar that the issue's `-j 1` clause
        // promises to keep working.
        let mut json_results: Vec<JsonFileResult> = Vec::with_capacity(files.len());
        for file in files {
            let filename = get_filename(file);
            if let Some(ref pb) = file_pb {
                pb.set_message(filename.to_string());
            }

            let analysis_pb = create_analysis_progress_bar(&mp, file, opts);
            let (result, text) = process_info(file, opts, analysis_pb.as_ref())?;
            finish_analysis_progress(analysis_pb);

            if !text.is_empty() {
                print!("{}", text);
            }

            json_results.push(result);

            if let Some(ref pb) = file_pb {
                pb.inc(1);
            }
        }
        json_results
    };

    if let Some(pb) = file_pb {
        pb.finish_and_clear();
    }

    // Print album summary (mp3gain compatible)
    let show_album_summary = !files.is_empty()
        && replaygain::is_available()
        && matches!(opts.output_format, OutputFormat::Tsv | OutputFormat::Text)
        && json_results.iter().any(|r| r.gain_applied_db.is_some());

    if show_album_summary {
        // Prefer MP3 files for album analysis; fall back to MP4/AAC
        let (aac_files, mp3_files): (Vec<&Path>, Vec<&Path>) = files
            .iter()
            .map(|f| f.as_path())
            .partition(|f| mp4meta::is_mp4_file(f));
        let album_paths = if !mp3_files.is_empty() {
            &mp3_files
        } else {
            &aac_files
        };

        let album_rg = if parallel {
            replaygain::analyze_album_parallel(album_paths, opts.track_index, threads)
        } else {
            replaygain::analyze_album(album_paths)
        };
        if let Ok(album_rg) = album_rg {
            let album_gain_db = album_rg.album_gain_db() + opts.gain_modifier_db;
            let album_gain_steps = db_to_steps(album_gain_db);
            let album_max_amp = album_rg.album_peak() * 32768.0;

            let album_max_gain = json_results
                .iter()
                .filter_map(|r| r.max_gain)
                .max()
                .unwrap_or(255);
            let album_min_gain = json_results
                .iter()
                .filter_map(|r| r.min_gain)
                .min()
                .unwrap_or(0);

            match opts.output_format {
                OutputFormat::Tsv => {
                    println!(
                        "\"Album\"\t{}\t{:.6}\t{:.6}\t{}\t{}",
                        album_gain_steps,
                        album_gain_db,
                        album_max_amp,
                        album_max_gain,
                        album_min_gain
                    );
                }
                OutputFormat::Text => {
                    if !opts.quiet {
                        println!();
                        println!(
                            "Recommended \"Album\" dB change for all files: {:.6}",
                            album_gain_db
                        );
                        println!(
                            "Recommended \"Album\" mp3 gain change for all files: {}",
                            album_gain_steps
                        );
                    }
                }
                OutputFormat::Json => {}
            }
        }
    }

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
