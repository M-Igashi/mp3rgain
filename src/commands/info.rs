use anyhow::Result;
use colored::*;
use indicatif::MultiProgress;
use mp3rgain::replaygain::{self, AlbumAnalysisReport, ReplayGainResult};
use mp3rgain::{mp4meta, peak_to_pcm_sample, steps_to_db};
use rayon::prelude::*;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::threading::effective_threads;
use crate::commands::utils::{finish_without_summary, for_each_file_with_analysis_bar};
use crate::processors::info::{format_rg_row, process_info, scan_gain_range_for_row};
use crate::progress::{album_progress_callbacks, create_album_progress_pb_in};
use crate::util::get_filename;

pub fn cmd_info(files: &[PathBuf], opts: &Options) -> Result<()> {
    // Print mp3gain-compatible TSV header
    if opts.output_format == OutputFormat::Tsv {
        println!("File\tMP3 gain\tdB gain\tMax Amplitude\tMax global_gain\tMin global_gain");
    }

    // ReplayGain-based TSV/Text output: one album analysis pass provides both
    // the per-file rows and the album summary. The previous flow analyzed
    // per-track first and then re-decoded every file for the album summary,
    // doubling the runtime of the default command.
    if replaygain::is_available()
        && matches!(opts.output_format, OutputFormat::Tsv | OutputFormat::Text)
    {
        return cmd_info_replaygain(files, opts);
    }

    cmd_info_basic(files, opts)
}

/// Per-file row produced by the single-pass album analysis.
enum Row {
    Analyzed(ReplayGainResult),
    Failed(String),
}

fn cmd_info_replaygain(files: &[PathBuf], opts: &Options) -> Result<()> {
    let threads = effective_threads(opts);
    let parallel = threads > 1 && files.len() > 1;

    // Partition by container, keeping original indices so output stays in
    // input order. MP3 files are preferred for the album summary (mp3gain
    // parity when folders mix formats).
    let mut mp3_set: Vec<(usize, &Path)> = Vec::new();
    let mut mp4_set: Vec<(usize, &Path)> = Vec::new();
    for (i, f) in files.iter().enumerate() {
        if mp4meta::is_mp4_file(f) {
            mp4_set.push((i, f.as_path()));
        } else {
            mp3_set.push((i, f.as_path()));
        }
    }

    let mp3_report = analyze_set(&mp3_set, opts, threads, parallel);
    let mp4_report = analyze_set(&mp4_set, opts, threads, parallel);

    // Assemble per-file rows in input order.
    let mut rows: Vec<Option<Row>> = (0..files.len()).map(|_| None).collect();
    for (set, report) in [(&mp3_set, &mp3_report), (&mp4_set, &mp4_report)] {
        match report {
            Some(report) => {
                for (k, &set_idx) in report.successful_indices.iter().enumerate() {
                    rows[set[set_idx].0] = Some(Row::Analyzed(report.album.tracks()[k].clone()));
                }
                for (set_idx, msg) in &report.failures {
                    rows[set[*set_idx].0] = Some(Row::Failed(msg.clone()));
                }
            }
            None => {
                // Every file in this set failed the album pass; re-probe each
                // file to recover its individual error message (failed probes
                // are cheap — no decoding happens).
                for &(orig_idx, path) in set.iter() {
                    let msg = replaygain::analyze_track_with_index(path, opts.track_index)
                        .err()
                        .map(|e| e.to_string())
                        .unwrap_or_else(|| "analysis failed".to_string());
                    rows[orig_idx] = Some(Row::Failed(msg));
                }
            }
        }
    }

    // Scan gain ranges in parallel before emitting: the frame scan re-reads
    // each file, which the sequential emit loop below would serialize.
    let gain_ranges: Vec<(u8, u8)> = files
        .par_iter()
        .enumerate()
        .map(|(i, file)| match rows[i] {
            Some(Row::Analyzed(_)) => scan_gain_range_for_row(file),
            _ => (255, 0),
        })
        .collect();

    // Emit rows in input order and collect the album-level gain bounds.
    let mut any_ok = false;
    let mut album_max_gain: Option<u8> = None;
    let mut album_min_gain: Option<u8> = None;
    {
        let stdout = io::stdout();
        let mut handle = stdout.lock();
        for (i, file) in files.iter().enumerate() {
            match &rows[i] {
                Some(Row::Analyzed(rg)) => {
                    let (result, text) = format_rg_row(file, opts, rg, gain_ranges[i])?;
                    if !text.is_empty() {
                        handle.write_all(text.as_bytes())?;
                    }
                    any_ok = true;
                    album_max_gain = album_max_gain.max(result.max_gain);
                    album_min_gain = match (album_min_gain, result.min_gain) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    };
                }
                Some(Row::Failed(msg)) => {
                    eprintln!("{} - {}", get_filename(file).red(), msg);
                }
                None => {}
            }
        }
    }

    // Print album summary (mp3gain compatible) from the same analysis pass.
    // Prefer the MP3 report, but fall back to the MP4 one if every MP3
    // failed — otherwise the album summary would silently disappear.
    let summary_report = mp3_report.as_ref().or(mp4_report.as_ref());

    if any_ok {
        if let Some(report) = summary_report {
            // Match the apply path (-a): album_gain_steps() + gain_modifier_steps()
            let modifier_steps = opts.gain_modifier_steps();
            let album_gain_steps = report.album.album_gain_steps() + modifier_steps;
            let album_gain_db = report.album.album_gain_db() + steps_to_db(modifier_steps);
            let album_max_amp = peak_to_pcm_sample(report.album.album_peak());

            match opts.output_format {
                OutputFormat::Tsv => {
                    println!(
                        "\"Album\"\t{}\t{:.6}\t{:.6}\t{}\t{}",
                        album_gain_steps,
                        album_gain_db,
                        album_max_amp,
                        album_max_gain.unwrap_or(255),
                        album_min_gain.unwrap_or(0)
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

    Ok(())
}

/// Run one lenient album analysis over a partition, driving a progress bar.
/// Returns `None` for an empty set or when every file in the set failed.
fn analyze_set(
    set: &[(usize, &Path)],
    opts: &Options,
    threads: usize,
    parallel: bool,
) -> Option<AlbumAnalysisReport> {
    if set.is_empty() {
        return None;
    }
    let paths: Vec<&Path> = set.iter().map(|&(_, p)| p).collect();

    let show_progress = !opts.quiet && opts.output_format == OutputFormat::Text;
    let mp = MultiProgress::new();
    let pb = if show_progress {
        Some(create_album_progress_pb_in(&mp, paths.len(), parallel))
    } else {
        None
    };
    let file_names: Vec<&str> = set.iter().map(|&(_, p)| get_filename(p)).collect();
    let (on_progress, on_complete) = album_progress_callbacks(&pb, file_names);

    let report = if parallel {
        replaygain::analyze_album_lenient_parallel_with_completion(
            &paths,
            opts.track_index,
            threads,
            &on_complete,
        )
    } else {
        replaygain::analyze_album_lenient_with_progress(&paths, opts.track_index, &on_progress)
    };

    if let Some(pb) = pb {
        pb.finish_and_clear();
    }

    report.ok()
}

/// Basic per-file info (JSON output or builds without the replaygain feature).
fn cmd_info_basic(files: &[PathBuf], opts: &Options) -> Result<()> {
    let (json_results, _, _) =
        for_each_file_with_analysis_bar(files, opts, |file, analysis_pb| {
            process_info(file, opts, analysis_pb).map(|(r, t)| (Some(r), t))
        })?;

    finish_without_summary(json_results, opts)
}
