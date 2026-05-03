//! mp3rgain - Lossless MP3 volume adjustment
//! A modern mp3gain replacement written in Rust
//!
//! Command-line interface compatible with the original mp3gain.

mod cli;
mod json_output;
mod processors;
mod progress;

use anyhow::Result;
use colored::*;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use mp3rgain::id3v2;
use mp3rgain::mp4meta;
use mp3rgain::replaygain::{self, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::{
    analyze, db_to_steps, delete_ape_tag, find_max_amplitude, read_ape_tag_from_file, steps_to_db,
    Channel, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO, TAG_REPLAYGAIN_ALBUM_GAIN,
    TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_TRACK_GAIN, TAG_REPLAYGAIN_TRACK_PEAK,
};
use std::env;
use std::path::{Path, PathBuf};

use cli::options::{AacAlbumInfo, Options, OutputFormat, StoredTagMode};
use cli::parse_args::{expand_files_recursive, parse_args};
use cli::usage::print_usage;
use json_output::{JsonAlbumResult, JsonFileResult, JsonOutput, JsonSummary};
use processors::apply::{process_apply, process_apply_channel};
use processors::info::process_info;
use processors::replaygain::{process_apply_replaygain_with_album, process_track_gain};
use processors::undo::process_undo;
use processors::utils::restore_timestamp;
use progress::{
    create_analysis_progress_bar, create_progress_bar, finish_analysis_progress, progress_finish,
    progress_inc, progress_set_message, PROGRESS_THRESHOLD,
};

/// Extract filename from path, returning "unknown" if extraction fails
pub(crate) fn get_filename(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
}

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let opts = parse_args(&args[1..])?;
    run(opts)
}

fn run(mut opts: Options) -> Result<()> {
    // Validate options
    if opts.files.is_empty() {
        eprintln!("{}: no files specified", "error".red().bold());
        std::process::exit(1);
    }

    // Expand files if recursive mode
    if opts.recursive {
        opts.files = expand_files_recursive(&opts.files)?;
        if opts.files.is_empty() {
            eprintln!("{}: no audio files found (MP3/M4A)", "error".red().bold());
            std::process::exit(1);
        }
    }

    // -f option warning (assume MPEG2)
    if opts.assume_mpeg2 && !opts.quiet && opts.output_format == OutputFormat::Text {
        eprintln!(
            "{}: -f (assume MPEG2) is accepted for compatibility but has no effect",
            "note".cyan()
        );
    }

    // Determine action based on options
    if opts.max_amplitude_only {
        // -x: only find max amplitude
        return cmd_max_amplitude(&opts.files, &opts);
    }

    if opts.stored_tag_mode == StoredTagMode::Delete {
        // -s d: delete stored tag info
        return cmd_delete_tags(&opts.files, &opts);
    }

    if opts.stored_tag_mode == StoredTagMode::Check {
        // -s c: check/show stored tag info
        return cmd_check_tags(&opts.files, &opts);
    }

    if opts.undo {
        // -u: undo from APEv2 tags
        return cmd_undo(&opts.files, &opts);
    }

    if opts.album_gain && !opts.skip_album {
        // -a: apply album gain (ReplayGain)
        return cmd_album_gain(&opts.files, &opts);
    }

    if opts.track_gain || opts.skip_album {
        // -r or -e: apply track gain (ReplayGain)
        return cmd_track_gain(&opts.files, &opts);
    }

    if let Some((channel, steps)) = opts.channel_gain {
        // -l: apply channel-specific gain
        return cmd_apply_channel(&opts.files, channel, steps, &opts);
    }

    if let Some(steps) = opts.gain_steps {
        // -g: apply fixed gain steps
        cmd_apply(&opts.files, steps, &opts)
    } else {
        // Default: analyze files (mp3gain compatible)
        // With -d modifier, perform ReplayGain analysis
        cmd_info(&opts.files, &opts)
    }
}

// =============================================================================
// Commands
// =============================================================================

fn cmd_max_amplitude(files: &[PathBuf], opts: &Options) -> Result<()> {
    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{} Finding maximum amplitude for {} file(s)",
            "mp3rgain".green().bold(),
            files.len()
        );
        println!();
    }

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        match find_max_amplitude(file) {
            Ok(amp_result) => {
                let max_amp = amp_result.max_amplitude();
                let max_gain = amp_result.max_global_gain();
                let min_gain = amp_result.min_global_gain();
                // Convert to PCM scale (like mp3gain: 0-32768+)
                let max_pcm_sample = max_amp * 32768.0;
                let headroom_db = if max_amp > 0.0 {
                    -20.0 * max_amp.log10()
                } else {
                    f64::INFINITY
                };

                match opts.output_format {
                    OutputFormat::Text => {
                        if !opts.quiet {
                            println!("{}", filename.cyan().bold());
                            println!("  Max PCM sample: {:.6}", max_pcm_sample);
                            println!("  Headroom:       {:+.2} dB", headroom_db);
                            println!("  Max global_gain: {}", max_gain);
                            println!("  Min global_gain: {}", min_gain);
                            println!();
                        } else {
                            println!("{}\t{:.6}\t{:.2}", filename, max_pcm_sample, headroom_db);
                        }
                    }
                    OutputFormat::Tsv => {
                        println!(
                            "{}\t{:.6}\t{:.2}\t{}\t{}",
                            filename, max_pcm_sample, headroom_db, max_gain, min_gain
                        );
                    }
                    OutputFormat::Json => {
                        json_results.push(JsonFileResult {
                            file: file.display().to_string(),
                            max_amplitude: Some(max_pcm_sample),
                            headroom_db: Some(headroom_db),
                            max_gain: Some(max_gain),
                            min_gain: Some(min_gain),
                            ..Default::default()
                        });
                    }
                }
            }
            Err(e) => {
                if opts.output_format == OutputFormat::Json {
                    json_results.push(JsonFileResult {
                        file: file.display().to_string(),
                        status: Some("error".to_string()),
                        error: Some(e.to_string()),
                        ..Default::default()
                    });
                } else if !opts.quiet {
                    eprintln!("{} - {}", filename.red(), e);
                }
            }
        }

        progress_inc(&pb);
    }

    progress_finish(pb);

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

fn cmd_delete_tags(files: &[PathBuf], opts: &Options) -> Result<()> {
    let dry_run_prefix = opts.dry_run_prefix();

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{}{} {} ReplayGain tags from {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            if opts.dry_run {
                "Would delete"
            } else {
                "Deleting"
            },
            files.len()
        );
        println!();
    }

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        if opts.dry_run {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!(
                    "  {} [DRY RUN] {} (would delete tags)",
                    "~".cyan(),
                    filename
                );
            }
            json_results.push(JsonFileResult {
                file: file.display().to_string(),
                status: Some("dry_run".to_string()),
                dry_run: Some(true),
                ..Default::default()
            });
        } else {
            // Save original timestamp if needed
            let original_mtime = if opts.preserve_timestamp {
                std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
            } else {
                None
            };

            let delete_result = if mp4meta::is_aac_file(file) {
                // AAC: delete both ReplayGain and undo freeform tags
                mp4meta::delete_replaygain_tags(file).and_then(|()| mp4meta::delete_undo_tags(file))
            } else if opts.use_id3v2 {
                id3v2::delete_id3v2_replaygain(file)
            } else {
                delete_ape_tag(file)
            };

            match delete_result {
                Ok(()) => {
                    if let Some(mtime) = original_mtime {
                        restore_timestamp(file, mtime);
                    }

                    if opts.output_format == OutputFormat::Text && !opts.quiet {
                        println!("  {} {} (tags deleted)", "v".green(), filename);
                    }
                    successful += 1;
                    json_results.push(JsonFileResult {
                        file: file.display().to_string(),
                        status: Some("success".to_string()),
                        ..Default::default()
                    });
                }
                Err(e) => {
                    if opts.output_format == OutputFormat::Text && !opts.quiet {
                        eprintln!("  {} {} - {}", "x".red(), filename, e);
                    }
                    failed += 1;
                    json_results.push(JsonFileResult {
                        file: file.display().to_string(),
                        status: Some("error".to_string()),
                        error: Some(e.to_string()),
                        ..Default::default()
                    });
                }
            }
        }

        progress_inc(&pb);
    }

    progress_finish(pb);

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
    } else if opts.dry_run && !opts.quiet {
        println!();
        println!("{}", "No files were modified.".yellow());
    }

    Ok(())
}

/// Tag values and labels for display in cmd_check_tags
struct CheckTagInfo<'a> {
    undo: Option<&'a str>,
    minmax: Option<&'a str>,
    track_gain: Option<&'a str>,
    track_peak: Option<&'a str>,
    album_gain: Option<&'a str>,
    album_peak: Option<&'a str>,
    undo_label: &'a str,
    minmax_label: &'a str,
    no_tag_msg: &'a str,
}

impl CheckTagInfo<'_> {
    fn has_any(&self) -> bool {
        self.undo.is_some()
            || self.minmax.is_some()
            || self.track_gain.is_some()
            || self.track_peak.is_some()
            || self.album_gain.is_some()
            || self.album_peak.is_some()
    }

    fn display(
        &self,
        filename: &str,
        file_path: &Path,
        format: OutputFormat,
        json_results: &mut Vec<JsonFileResult>,
    ) {
        match format {
            OutputFormat::Text => {
                println!("{}", filename.cyan().bold());
                if let Some(v) = self.undo {
                    println!("  {:<25}{}", format!("{}:", self.undo_label), v);
                }
                if let Some(v) = self.minmax {
                    println!("  {:<25}{}", format!("{}:", self.minmax_label), v);
                }
                if let Some(v) = self.track_gain {
                    println!("  REPLAYGAIN_TRACK_GAIN: {}", v);
                }
                if let Some(v) = self.track_peak {
                    println!("  REPLAYGAIN_TRACK_PEAK: {}", v);
                }
                if let Some(v) = self.album_gain {
                    println!("  REPLAYGAIN_ALBUM_GAIN: {}", v);
                }
                if let Some(v) = self.album_peak {
                    println!("  REPLAYGAIN_ALBUM_PEAK: {}", v);
                }
                if !self.has_any() {
                    println!("  ({})", self.no_tag_msg);
                }
                println!();
            }
            OutputFormat::Tsv => {
                println!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                    filename,
                    self.undo.unwrap_or("-"),
                    self.minmax.unwrap_or("-"),
                    self.track_gain.unwrap_or("-"),
                    self.track_peak.unwrap_or("-"),
                    self.album_gain.unwrap_or("-"),
                    self.album_peak.unwrap_or("-")
                );
            }
            OutputFormat::Json => {
                json_results.push(JsonFileResult {
                    file: file_path.display().to_string(),
                    status: Some(if self.has_any() { "success" } else { "no_tag" }.to_string()),
                    ..Default::default()
                });
            }
        }
    }
}

fn cmd_check_tags(files: &[PathBuf], opts: &Options) -> Result<()> {
    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{} Checking stored tag info for {} file(s)",
            "mp3rgain".green().bold(),
            files.len()
        );
        println!();
    }

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        let is_aac = mp4meta::is_aac_file(file);

        if is_aac {
            // AAC: read iTunes freeform tags
            let undo_tags = mp4meta::read_undo_tags(file).unwrap_or_default();
            let rg_tags = mp4meta::read_replaygain_tags(file).unwrap_or_default();

            let undo = undo_tags.undo();
            let minmax = undo_tags.minmax();
            let track_gain = rg_tags.track_gain();
            let track_peak = rg_tags.track_peak();
            let album_gain = rg_tags.album_gain();
            let album_peak = rg_tags.album_peak();

            CheckTagInfo {
                undo,
                minmax,
                track_gain,
                track_peak,
                album_gain,
                album_peak,
                undo_label: "MP3RGAIN_UNDO",
                minmax_label: "MP3RGAIN_MINMAX",
                no_tag_msg: "no tags found",
            }
            .display(filename, file, opts.output_format, &mut json_results);
        } else if opts.use_id3v2 {
            // MP3 with -s i: read ID3v2 TXXX frames
            match id3v2::read_id3v2_replaygain(file) {
                Ok(rg) => {
                    let undo = rg.undo.as_deref();
                    let minmax = rg.minmax.as_deref();
                    let track_gain = rg.track_gain.as_deref();
                    let track_peak = rg.track_peak.as_deref();
                    let album_gain = rg.album_gain.as_deref();
                    let album_peak = rg.album_peak.as_deref();

                    CheckTagInfo {
                        undo,
                        minmax,
                        track_gain,
                        track_peak,
                        album_gain,
                        album_peak,
                        undo_label: "MP3GAIN_UNDO",
                        minmax_label: "MP3GAIN_MINMAX",
                        no_tag_msg: "no ID3v2 ReplayGain tags found",
                    }
                    .display(
                        filename,
                        file,
                        opts.output_format,
                        &mut json_results,
                    );
                }
                Err(e) => {
                    if opts.output_format != OutputFormat::Json {
                        eprintln!("{} - {}", filename.red(), e);
                    } else {
                        json_results.push(JsonFileResult {
                            file: file.display().to_string(),
                            status: Some("error".to_string()),
                            error: Some(e.to_string()),
                            ..Default::default()
                        });
                    }
                }
            }
        } else {
            // MP3: read APEv2 tags
            match read_ape_tag_from_file(file) {
                Ok(Some(tag)) => {
                    CheckTagInfo {
                        undo: tag.get(TAG_MP3GAIN_UNDO),
                        minmax: tag.get(TAG_MP3GAIN_MINMAX),
                        track_gain: tag.get(TAG_REPLAYGAIN_TRACK_GAIN),
                        track_peak: tag.get(TAG_REPLAYGAIN_TRACK_PEAK),
                        album_gain: tag.get(TAG_REPLAYGAIN_ALBUM_GAIN),
                        album_peak: tag.get(TAG_REPLAYGAIN_ALBUM_PEAK),
                        undo_label: "MP3GAIN_UNDO",
                        minmax_label: "MP3GAIN_MINMAX",
                        no_tag_msg: "no mp3gain tags found",
                    }
                    .display(
                        filename,
                        file,
                        opts.output_format,
                        &mut json_results,
                    );
                }
                Ok(None) => {
                    CheckTagInfo {
                        undo: None,
                        minmax: None,
                        track_gain: None,
                        track_peak: None,
                        album_gain: None,
                        album_peak: None,
                        undo_label: "MP3GAIN_UNDO",
                        minmax_label: "MP3GAIN_MINMAX",
                        no_tag_msg: "no APE tag found",
                    }
                    .display(
                        filename,
                        file,
                        opts.output_format,
                        &mut json_results,
                    );
                }
                Err(e) => {
                    if opts.output_format != OutputFormat::Json {
                        eprintln!("{} - {}", filename.red(), e);
                    } else {
                        json_results.push(JsonFileResult {
                            file: file.display().to_string(),
                            status: Some("error".to_string()),
                            error: Some(e.to_string()),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        progress_inc(&pb);
    }

    progress_finish(pb);

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

fn update_counters(result: &JsonFileResult, successful: &mut usize, failed: &mut usize) {
    match result.status.as_deref() {
        Some("success") => *successful += 1,
        Some("error") => *failed += 1,
        _ => {}
    }
}

fn create_json_summary(
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

fn print_dry_run_notice(opts: &Options) {
    if opts.dry_run && !opts.quiet && opts.output_format == OutputFormat::Text {
        println!();
        println!("{}", "No files were modified.".yellow());
    }
}

fn cmd_apply(files: &[PathBuf], steps: i32, opts: &Options) -> Result<()> {
    if steps == 0 {
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
        return Ok(());
    }

    let db_value = steps_to_db(steps);
    let dry_run_prefix = opts.dry_run_prefix();

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{}{} {} {} step(s) ({:+.1} dB) to {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            if opts.dry_run {
                "Would apply"
            } else {
                "Applying"
            },
            steps,
            db_value,
            files.len()
        );
        if opts.wrap_gain {
            println!("  {} Wrap mode enabled", "!".yellow());
        }
        println!();
    }

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        let result = process_apply(file, steps, opts)?;
        update_counters(&result, &mut successful, &mut failed);

        if opts.output_format == OutputFormat::Tsv {
            if let Ok(info) = analyze(file) {
                println!(
                    "{}\t{}\t{:.1}\t{:.6}\t{}\t{}",
                    filename,
                    steps,
                    db_value,
                    1.0,
                    info.max_gain(),
                    info.min_gain()
                );
            }
        }

        if opts.output_format == OutputFormat::Json {
            json_results.push(result);
        }

        progress_inc(&pb);
    }

    progress_finish(pb);

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

fn cmd_apply_channel(
    files: &[PathBuf],
    channel: Channel,
    steps: i32,
    opts: &Options,
) -> Result<()> {
    if steps == 0 {
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
        return Ok(());
    }

    let db_value = steps_to_db(steps);
    let dry_run_prefix = opts.dry_run_prefix();
    let channel_name = match channel {
        Channel::Left => "left",
        Channel::Right => "right",
        _ => unreachable!(),
    };

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{}{} {} {} step(s) ({:+.1} dB) to {} channel of {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            if opts.dry_run {
                "Would apply"
            } else {
                "Applying"
            },
            steps,
            db_value,
            channel_name,
            files.len()
        );
        println!();
    }

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        let result = process_apply_channel(file, channel, steps, opts)?;
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

fn cmd_info(files: &[PathBuf], opts: &Options) -> Result<()> {
    // Print mp3gain-compatible TSV header
    if opts.output_format == OutputFormat::Tsv {
        println!("File\tMP3 gain\tdB gain\tMax Amplitude\tMax global_gain\tMin global_gain");
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

    for file in files {
        let filename = get_filename(file);
        if let Some(ref pb) = file_pb {
            pb.set_message(filename.to_string());
        }

        let analysis_pb = create_analysis_progress_bar(&mp, file, opts);
        let result = process_info(file, opts, analysis_pb.as_ref())?;
        finish_analysis_progress(analysis_pb);
        json_results.push(result);

        if let Some(ref pb) = file_pb {
            pb.inc(1);
        }
    }

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

        if let Ok(album_rg) = replaygain::analyze_album(album_paths) {
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

fn cmd_undo(files: &[PathBuf], opts: &Options) -> Result<()> {
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

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        let result = process_undo(file, opts)?;
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

fn cmd_track_gain(files: &[PathBuf], opts: &Options) -> Result<()> {
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

fn cmd_album_gain(files: &[PathBuf], opts: &Options) -> Result<()> {
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
