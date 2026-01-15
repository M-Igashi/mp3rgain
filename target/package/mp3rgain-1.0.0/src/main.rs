//! mp3rgain - Lossless MP3 volume adjustment
//! A modern mp3gain replacement written in Rust
//!
//! Command-line interface compatible with the original mp3gain.

use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use mp3rgain::mp4meta;
use mp3rgain::replaygain::{self, AudioFileType, ReplayGainResult, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::{
    analyze, apply_gain_channel_with_undo, apply_gain_with_undo, apply_gain_with_undo_wrap,
    db_to_steps, delete_ape_tag, find_max_amplitude, read_ape_tag_from_file, steps_to_db,
    undo_gain, Channel, GAIN_STEP_DB, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO,
    TAG_REPLAYGAIN_ALBUM_GAIN, TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_TRACK_GAIN,
    TAG_REPLAYGAIN_TRACK_PEAK,
};
use serde::Serialize;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const PROGRESS_THRESHOLD: usize = 5;

// =============================================================================
// Options
// =============================================================================

#[derive(Default, Clone, Copy, PartialEq)]
enum OutputFormat {
    #[default]
    Text,
    Json,
    Tsv, // Tab-separated values (database-friendly)
}

#[derive(Default, Clone, Copy, PartialEq)]
enum StoredTagMode {
    #[default]
    None, // Default behavior
    Check,    // -s c: Check/show stored tag info
    Delete,   // -s d: Delete stored tag info
    Skip,     // -s s: Skip (ignore) stored tag info
    Recalc,   // -s r: Force recalculation
    UseId3v2, // -s i: Use ID3v2 tags (not fully implemented, show warning)
    UseApev2, // -s a: Use APEv2 tags (default)
}

/// Album gain info for AAC files
struct AacAlbumInfo {
    album_gain_db: f64,
    album_peak: f64,
}

#[derive(Default)]
struct Options {
    // Gain options
    gain_steps: Option<i32>,              // -g <i>
    gain_db: Option<f64>,                 // -d <n>
    channel_gain: Option<(Channel, i32)>, // -l <channel> <gain>
    gain_modifier: i32,                   // -m <i>: modify suggested gain by integer

    // Mode options
    undo: bool,                     // -u
    stored_tag_mode: StoredTagMode, // -s <mode>
    track_gain: bool,               // -r (apply track gain)
    album_gain: bool,               // -a (apply album gain)
    skip_album: bool,               // -e: skip album analysis
    max_amplitude_only: bool,       // -x: only find max amplitude

    // Behavior options
    preserve_timestamp: bool,    // -p
    ignore_clipping: bool,       // -c
    prevent_clipping: bool,      // -k
    quiet: bool,                 // -q
    recursive: bool,             // -R
    dry_run: bool,               // -n or --dry-run
    output_format: OutputFormat, // -o <format>
    wrap_gain: bool,             // -w: wrap gain values
    use_temp_file: bool,         // -t: use temp file for writing
    assume_mpeg2: bool,          // -f: assume MPEG 2 Layer III

    // Files
    files: Vec<PathBuf>,
}

// =============================================================================
// JSON Output Structures
// =============================================================================

#[derive(Serialize)]
struct JsonOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<Vec<JsonFileResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    album: Option<JsonAlbumResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    summary: Option<JsonSummary>,
}

#[derive(Serialize, Clone, Default)]
struct JsonFileResult {
    file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frames: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mpeg_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    channel_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    min_gain: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_gain: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    avg_gain: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    headroom_steps: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    headroom_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gain_applied_steps: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gain_applied_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    loudness_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    peak: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_amplitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dry_run: Option<bool>,
}

#[derive(Serialize)]
struct JsonAlbumResult {
    loudness_db: f64,
    gain_db: f64,
    gain_steps: i32,
    peak: f64,
}

#[derive(Serialize)]
struct JsonSummary {
    total_files: usize,
    successful: usize,
    failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    dry_run: Option<bool>,
}

// =============================================================================
// Main
// =============================================================================

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        print_usage();
        return Ok(());
    }

    let opts = parse_args(&args[1..])?;
    run(opts)
}

fn parse_args(args: &[String]) -> Result<Options> {
    let mut opts = Options::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--dry-run" {
            opts.dry_run = true;
            i += 1;
            continue;
        }

        if arg == "--help" {
            print_usage();
            std::process::exit(0);
        }

        if arg == "--version" {
            print_version();
            std::process::exit(0);
        }

        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            let flag = &arg[1..];

            match flag {
                "g" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -g requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    opts.gain_steps = Some(
                        args[i]
                            .parse()
                            .map_err(|_| anyhow::anyhow!("invalid gain value: {}", args[i]))?,
                    );
                }
                "d" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -d requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    opts.gain_db = Some(
                        args[i]
                            .parse()
                            .map_err(|_| anyhow::anyhow!("invalid dB value: {}", args[i]))?,
                    );
                }
                "m" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -m requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    opts.gain_modifier = args[i]
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid modifier value: {}", args[i]))?;
                }
                "s" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -s requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    match args[i].as_str() {
                        "c" => opts.stored_tag_mode = StoredTagMode::Check,
                        "d" => opts.stored_tag_mode = StoredTagMode::Delete,
                        "s" => opts.stored_tag_mode = StoredTagMode::Skip,
                        "r" => opts.stored_tag_mode = StoredTagMode::Recalc,
                        "i" => {
                            opts.stored_tag_mode = StoredTagMode::UseId3v2;
                            eprintln!(
                                "{}: -s i (ID3v2 tags) not fully supported, using APEv2",
                                "warning".yellow().bold()
                            );
                        }
                        "a" => opts.stored_tag_mode = StoredTagMode::UseApev2,
                        other => {
                            eprintln!(
                                "{}: unknown -s mode '{}', use c/d/s/r/i/a",
                                "error".red().bold(),
                                other
                            );
                            std::process::exit(1);
                        }
                    }
                }
                "o" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -o requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    match args[i].to_lowercase().as_str() {
                        "json" => opts.output_format = OutputFormat::Json,
                        "text" => opts.output_format = OutputFormat::Text,
                        "tsv" | "db" => opts.output_format = OutputFormat::Tsv,
                        other => {
                            eprintln!(
                                "{}: unknown output format '{}', use 'text', 'json', or 'tsv'",
                                "error".red().bold(),
                                other
                            );
                            std::process::exit(1);
                        }
                    }
                }
                "l" => {
                    // -l <channel> <gain> : apply gain to specific channel
                    i += 1;
                    if i >= args.len() {
                        eprintln!(
                            "{}: -l requires two arguments: <channel> <gain>",
                            "error".red().bold()
                        );
                        std::process::exit(1);
                    }
                    let channel_arg: usize = args[i].parse().map_err(|_| {
                        anyhow::anyhow!(
                            "invalid channel number: {} (use 0 for left, 1 for right)",
                            args[i]
                        )
                    })?;
                    let channel = Channel::from_index(channel_arg).ok_or_else(|| {
                        anyhow::anyhow!(
                            "invalid channel: {} (use 0 for left, 1 for right)",
                            channel_arg
                        )
                    })?;

                    i += 1;
                    if i >= args.len() {
                        eprintln!(
                            "{}: -l requires two arguments: <channel> <gain>",
                            "error".red().bold()
                        );
                        std::process::exit(1);
                    }
                    let gain: i32 = args[i]
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid gain value: {}", args[i]))?;

                    opts.channel_gain = Some((channel, gain));
                }
                "r" => opts.track_gain = true,
                "a" => opts.album_gain = true,
                "e" => opts.skip_album = true,
                "x" => opts.max_amplitude_only = true,
                "u" => opts.undo = true,
                "p" => opts.preserve_timestamp = true,
                "c" => opts.ignore_clipping = true,
                "k" => opts.prevent_clipping = true,
                "q" => opts.quiet = true,
                "R" => opts.recursive = true,
                "n" => opts.dry_run = true,
                "w" => opts.wrap_gain = true,
                "t" => opts.use_temp_file = true,
                "f" => opts.assume_mpeg2 = true,
                "v" | "-version" => {
                    print_version();
                    std::process::exit(0);
                }
                "h" | "-help" => {
                    print_usage();
                    std::process::exit(0);
                }
                // Handle combined short flags like -qp, -kc, etc.
                _ if flag.chars().all(|c| "pqckuranRewxtf".contains(c)) => {
                    for c in flag.chars() {
                        match c {
                            'p' => opts.preserve_timestamp = true,
                            'q' => opts.quiet = true,
                            'c' => opts.ignore_clipping = true,
                            'k' => opts.prevent_clipping = true,
                            'u' => opts.undo = true,
                            'r' => opts.track_gain = true,
                            'a' => opts.album_gain = true,
                            'n' => opts.dry_run = true,
                            'R' => opts.recursive = true,
                            'e' => opts.skip_album = true,
                            'w' => opts.wrap_gain = true,
                            'x' => opts.max_amplitude_only = true,
                            't' => opts.use_temp_file = true,
                            'f' => opts.assume_mpeg2 = true,
                            _ => {}
                        }
                    }
                }
                // Handle -g with attached value (e.g., -g2)
                _ if flag.starts_with('g') => {
                    let val = &flag[1..];
                    opts.gain_steps = Some(
                        val.parse()
                            .map_err(|_| anyhow::anyhow!("invalid gain value: {}", val))?,
                    );
                }
                // Handle -d with attached value (e.g., -d4.5)
                _ if flag.starts_with('d') => {
                    let val = &flag[1..];
                    opts.gain_db = Some(
                        val.parse()
                            .map_err(|_| anyhow::anyhow!("invalid dB value: {}", val))?,
                    );
                }
                // Handle -m with attached value (e.g., -m2)
                _ if flag.starts_with('m') => {
                    let val = &flag[1..];
                    opts.gain_modifier = val
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid modifier value: {}", val))?;
                }
                _ => {
                    eprintln!("{}: unknown option: -{}", "warning".yellow().bold(), flag);
                }
            }
        } else if !arg.starts_with("--") {
            // It's a file
            opts.files.push(PathBuf::from(arg));
        }

        i += 1;
    }

    Ok(opts)
}

fn expand_files_recursive(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();

    for path in paths {
        if path.is_dir() {
            collect_audio_files(path, &mut result)?;
        } else {
            result.push(path.clone());
        }
    }

    result.sort();
    Ok(result)
}

fn collect_audio_files(dir: &Path, result: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_audio_files(&path, result)?;
        } else if let Some(ext) = path.extension() {
            if ext.eq_ignore_ascii_case("mp3")
                || ext.eq_ignore_ascii_case("m4a")
                || ext.eq_ignore_ascii_case("aac")
                || ext.eq_ignore_ascii_case("mp4")
            {
                result.push(path);
            }
        }
    }

    Ok(())
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

    if opts.channel_gain.is_some() {
        // -l: apply channel-specific gain
        let (channel, steps) = opts.channel_gain.unwrap();
        return cmd_apply_channel(&opts.files, channel, steps, &opts);
    }

    if opts.gain_steps.is_some() || opts.gain_db.is_some() {
        // -g or -d: apply gain
        let steps = match (opts.gain_steps, opts.gain_db) {
            (Some(g), _) => g,
            (_, Some(d)) => db_to_steps(d),
            _ => unreachable!(),
        };
        cmd_apply(&opts.files, steps, &opts)
    } else {
        // Default: show info (like mp3gain without -r or -a)
        cmd_info(&opts.files, &opts)
    }
}

// =============================================================================
// Progress Bar
// =============================================================================

fn create_progress_bar(total: usize, opts: &Options) -> Option<ProgressBar> {
    if opts.quiet || opts.output_format != OutputFormat::Text || total < PROGRESS_THRESHOLD {
        return None;
    }

    let pb = ProgressBar::new(total as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} {msg}")
            .unwrap()
            .progress_chars("=>-"),
    );
    Some(pb)
}

fn progress_set_message(pb: &Option<ProgressBar>, msg: &str) {
    if let Some(ref pb) = pb {
        pb.set_message(msg.to_string());
    }
}

fn progress_inc(pb: &Option<ProgressBar>) {
    if let Some(ref pb) = pb {
        pb.inc(1);
    }
}

fn progress_finish(pb: Option<ProgressBar>) {
    if let Some(pb) = pb {
        pb.finish_and_clear();
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
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        progress_set_message(&pb, filename);

        match find_max_amplitude(file) {
            Ok((max_amp, max_gain, min_gain)) => {
                let headroom_db = if max_amp > 0.0 {
                    -20.0 * max_amp.log10()
                } else {
                    f64::INFINITY
                };

                match opts.output_format {
                    OutputFormat::Text => {
                        if !opts.quiet {
                            println!("{}", filename.cyan().bold());
                            println!("  Max amplitude:  {:.6}", max_amp);
                            println!("  Headroom:       {:+.2} dB", headroom_db);
                            println!("  Max global_gain: {}", max_gain);
                            println!("  Min global_gain: {}", min_gain);
                            println!();
                        } else {
                            println!("{}\t{:.6}\t{:.2}", filename, max_amp, headroom_db);
                        }
                    }
                    OutputFormat::Tsv => {
                        println!(
                            "{}\t{:.6}\t{:.2}\t{}\t{}",
                            filename, max_amp, headroom_db, max_gain, min_gain
                        );
                    }
                    OutputFormat::Json => {
                        json_results.push(JsonFileResult {
                            file: file.display().to_string(),
                            max_amplitude: Some(max_amp),
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
    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };

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
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
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

            match delete_ape_tag(file) {
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
            summary: Some(JsonSummary {
                total_files: files.len(),
                successful,
                failed,
                dry_run: if opts.dry_run { Some(true) } else { None },
            }),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if opts.dry_run && !opts.quiet {
        println!();
        println!("{}", "No files were modified.".yellow());
    }

    Ok(())
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
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        progress_set_message(&pb, filename);

        match read_ape_tag_from_file(file) {
            Ok(Some(tag)) => {
                let undo = tag.get(TAG_MP3GAIN_UNDO);
                let minmax = tag.get(TAG_MP3GAIN_MINMAX);
                let track_gain = tag.get(TAG_REPLAYGAIN_TRACK_GAIN);
                let track_peak = tag.get(TAG_REPLAYGAIN_TRACK_PEAK);
                let album_gain = tag.get(TAG_REPLAYGAIN_ALBUM_GAIN);
                let album_peak = tag.get(TAG_REPLAYGAIN_ALBUM_PEAK);

                match opts.output_format {
                    OutputFormat::Text => {
                        println!("{}", filename.cyan().bold());
                        if let Some(v) = undo {
                            println!("  MP3GAIN_UNDO:         {}", v);
                        }
                        if let Some(v) = minmax {
                            println!("  MP3GAIN_MINMAX:       {}", v);
                        }
                        if let Some(v) = track_gain {
                            println!("  REPLAYGAIN_TRACK_GAIN: {}", v);
                        }
                        if let Some(v) = track_peak {
                            println!("  REPLAYGAIN_TRACK_PEAK: {}", v);
                        }
                        if let Some(v) = album_gain {
                            println!("  REPLAYGAIN_ALBUM_GAIN: {}", v);
                        }
                        if let Some(v) = album_peak {
                            println!("  REPLAYGAIN_ALBUM_PEAK: {}", v);
                        }
                        if undo.is_none() && minmax.is_none() && track_gain.is_none() {
                            println!("  (no mp3gain tags found)");
                        }
                        println!();
                    }
                    OutputFormat::Tsv => {
                        println!(
                            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                            filename,
                            undo.unwrap_or("-"),
                            minmax.unwrap_or("-"),
                            track_gain.unwrap_or("-"),
                            track_peak.unwrap_or("-"),
                            album_gain.unwrap_or("-"),
                            album_peak.unwrap_or("-")
                        );
                    }
                    OutputFormat::Json => {
                        let result = JsonFileResult {
                            file: file.display().to_string(),
                            status: Some("success".to_string()),
                            ..Default::default()
                        };
                        // Note: we can add tag info to JSON if needed
                        json_results.push(result);
                    }
                }
            }
            Ok(None) => match opts.output_format {
                OutputFormat::Text => {
                    println!("{}", filename.cyan().bold());
                    println!("  (no APE tag found)");
                    println!();
                }
                OutputFormat::Tsv => {
                    println!("{}\t-\t-\t-\t-\t-\t-", filename);
                }
                OutputFormat::Json => {
                    json_results.push(JsonFileResult {
                        file: file.display().to_string(),
                        status: Some("no_tag".to_string()),
                        ..Default::default()
                    });
                }
            },
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

fn cmd_apply(files: &[PathBuf], steps: i32, opts: &Options) -> Result<()> {
    if steps == 0 {
        if opts.output_format == OutputFormat::Json {
            let output = JsonOutput {
                files: Some(vec![]),
                album: None,
                summary: Some(JsonSummary {
                    total_files: files.len(),
                    successful: 0,
                    failed: 0,
                    dry_run: if opts.dry_run { Some(true) } else { None },
                }),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !opts.quiet {
            println!("{}: gain is 0, nothing to do", "info".cyan());
        }
        return Ok(());
    }

    let db_value = steps_to_db(steps);
    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };

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
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        progress_set_message(&pb, filename);

        let result = process_apply(file, steps, opts)?;
        match opts.output_format {
            OutputFormat::Json => {
                if result.status.as_deref() == Some("success") {
                    successful += 1;
                } else if result.status.as_deref() == Some("error") {
                    failed += 1;
                }
                json_results.push(result);
            }
            OutputFormat::Tsv => {
                // TSV output for apply: file, mp3_gain, db_gain, max_amp, max_global_gain, min_global_gain
                if let Ok(info) = analyze(file) {
                    println!(
                        "{}\t{}\t{:.1}\t{:.6}\t{}\t{}",
                        filename,
                        steps,
                        db_value,
                        1.0, // max amplitude placeholder
                        info.max_gain,
                        info.min_gain
                    );
                }
            }
            OutputFormat::Text => {
                if result.status.as_deref() == Some("success") {
                    successful += 1;
                } else if result.status.as_deref() == Some("error") {
                    failed += 1;
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
            summary: Some(JsonSummary {
                total_files: files.len(),
                successful,
                failed,
                dry_run: if opts.dry_run { Some(true) } else { None },
            }),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if opts.dry_run && !opts.quiet && opts.output_format == OutputFormat::Text {
        println!();
        println!("{}", "No files were modified.".yellow());
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
                summary: Some(JsonSummary {
                    total_files: files.len(),
                    successful: 0,
                    failed: 0,
                    dry_run: if opts.dry_run { Some(true) } else { None },
                }),
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        } else if !opts.quiet {
            println!("{}: gain is 0, nothing to do", "info".cyan());
        }
        return Ok(());
    }

    let db_value = steps_to_db(steps);
    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };
    let channel_name = match channel {
        Channel::Left => "left",
        Channel::Right => "right",
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
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        progress_set_message(&pb, filename);

        let result = process_apply_channel(file, channel, steps, opts)?;
        if opts.output_format == OutputFormat::Json {
            if result.status.as_deref() == Some("success") {
                successful += 1;
            } else if result.status.as_deref() == Some("error") {
                failed += 1;
            }
            json_results.push(result);
        } else if result.status.as_deref() == Some("success") {
            successful += 1;
        } else if result.status.as_deref() == Some("error") {
            failed += 1;
        }

        progress_inc(&pb);
    }

    progress_finish(pb);

    if opts.output_format == OutputFormat::Json {
        let output = JsonOutput {
            files: Some(json_results),
            album: None,
            summary: Some(JsonSummary {
                total_files: files.len(),
                successful,
                failed,
                dry_run: if opts.dry_run { Some(true) } else { None },
            }),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if opts.dry_run && !opts.quiet && opts.output_format == OutputFormat::Text {
        println!();
        println!("{}", "No files were modified.".yellow());
    }

    Ok(())
}

fn cmd_info(files: &[PathBuf], opts: &Options) -> Result<()> {
    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();

    for file in files {
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        progress_set_message(&pb, filename);

        let result = process_info(file, opts)?;
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
            summary: None,
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    }

    Ok(())
}

fn cmd_undo(files: &[PathBuf], opts: &Options) -> Result<()> {
    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };

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
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        progress_set_message(&pb, filename);

        let result = process_undo(file, opts)?;
        if opts.output_format == OutputFormat::Json {
            if result.status.as_deref() == Some("success") {
                successful += 1;
            } else if result.status.as_deref() == Some("error") {
                failed += 1;
            }
            json_results.push(result);
        } else if result.status.as_deref() == Some("success") {
            successful += 1;
        } else if result.status.as_deref() == Some("error") {
            failed += 1;
        }

        progress_inc(&pb);
    }

    progress_finish(pb);

    if opts.output_format == OutputFormat::Json {
        let output = JsonOutput {
            files: Some(json_results),
            album: None,
            summary: Some(JsonSummary {
                total_files: files.len(),
                successful,
                failed,
                dry_run: if opts.dry_run { Some(true) } else { None },
            }),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if opts.dry_run && !opts.quiet && opts.output_format == OutputFormat::Text {
        println!();
        println!("{}", "No files were modified.".yellow());
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

    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };

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

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();
    let mut successful = 0;
    let mut failed = 0;

    for file in files {
        let filename = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        progress_set_message(&pb, filename);

        let result = process_track_gain(file, opts)?;
        if opts.output_format == OutputFormat::Json {
            if result.status.as_deref() == Some("success") {
                successful += 1;
            } else if result.status.as_deref() == Some("error") {
                failed += 1;
            }
            json_results.push(result);
        } else if result.status.as_deref() == Some("success") {
            successful += 1;
        } else if result.status.as_deref() == Some("error") {
            failed += 1;
        }

        progress_inc(&pb);
    }

    progress_finish(pb);

    if opts.output_format == OutputFormat::Json {
        let output = JsonOutput {
            files: Some(json_results),
            album: None,
            summary: Some(JsonSummary {
                total_files: files.len(),
                successful,
                failed,
                dry_run: if opts.dry_run { Some(true) } else { None },
            }),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else if opts.dry_run && !opts.quiet && opts.output_format == OutputFormat::Text {
        println!();
        println!("{}", "No files were modified.".yellow());
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

    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };

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

    match replaygain::analyze_album(&file_refs) {
        Ok(album_result) => {
            // Apply gain modifier
            let modified_gain_steps = album_result.album_gain_steps() + opts.gain_modifier;

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!();
                println!("  Album loudness: {:.1} dB", album_result.album_loudness_db);
                println!(
                    "  Album gain:     {:+.1} dB ({} steps{})",
                    album_result.album_gain_db,
                    album_result.album_gain_steps(),
                    if opts.gain_modifier != 0 {
                        format!(" + {} = {}", opts.gain_modifier, modified_gain_steps)
                    } else {
                        String::new()
                    }
                );
                println!("  Album peak:     {:.4}", album_result.album_peak);
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
                            let track = &album_result.tracks[i];
                            JsonFileResult {
                                file: file.display().to_string(),
                                status: Some("skipped".to_string()),
                                loudness_db: Some(track.loudness_db),
                                peak: Some(track.peak),
                                gain_applied_steps: Some(0),
                                gain_applied_db: Some(0.0),
                                ..Default::default()
                            }
                        })
                        .collect();

                    let output = JsonOutput {
                        files: Some(json_results),
                        album: Some(JsonAlbumResult {
                            loudness_db: album_result.album_loudness_db,
                            gain_db: album_result.album_gain_db,
                            gain_steps: modified_gain_steps,
                            peak: album_result.album_peak,
                        }),
                        summary: Some(JsonSummary {
                            total_files: files.len(),
                            successful: 0,
                            failed: 0,
                            dry_run: if opts.dry_run { Some(true) } else { None },
                        }),
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
                let filename = file
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown");
                progress_set_message(&pb, filename);

                let track_result = &album_result.tracks[i];
                let album_info = AacAlbumInfo {
                    album_gain_db: album_result.album_gain_db,
                    album_peak: album_result.album_peak,
                };
                let result = process_apply_replaygain_with_album(
                    file,
                    steps,
                    track_result,
                    opts,
                    Some(&album_info),
                )?;
                if opts.output_format == OutputFormat::Json {
                    if result.status.as_deref() == Some("success") {
                        successful += 1;
                    } else if result.status.as_deref() == Some("error") {
                        failed += 1;
                    }
                    json_results.push(result);
                } else if result.status.as_deref() == Some("success") {
                    successful += 1;
                } else if result.status.as_deref() == Some("error") {
                    failed += 1;
                }

                progress_inc(&pb);
            }

            progress_finish(pb);

            if opts.output_format == OutputFormat::Json {
                let output = JsonOutput {
                    files: Some(json_results),
                    album: Some(JsonAlbumResult {
                        loudness_db: album_result.album_loudness_db,
                        gain_db: album_result.album_gain_db,
                        gain_steps: modified_gain_steps,
                        peak: album_result.album_peak,
                    }),
                    summary: Some(JsonSummary {
                        total_files: files.len(),
                        successful,
                        failed,
                        dry_run: if opts.dry_run { Some(true) } else { None },
                    }),
                };
                println!("{}", serde_json::to_string_pretty(&output)?);
            } else if opts.dry_run && !opts.quiet && opts.output_format == OutputFormat::Text {
                println!();
                println!("{}", "No files were modified.".yellow());
            }
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Json {
                let output = JsonOutput {
                    files: None,
                    album: None,
                    summary: Some(JsonSummary {
                        total_files: files.len(),
                        successful: 0,
                        failed: files.len(),
                        dry_run: if opts.dry_run { Some(true) } else { None },
                    }),
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

// =============================================================================
// File processing
// =============================================================================

fn apply_with_temp_file<F>(file: &PathBuf, operation: F, opts: &Options) -> Result<usize>
where
    F: FnOnce(&Path) -> Result<usize>,
{
    if opts.use_temp_file {
        // Create temp file in the same directory
        let parent = file.parent().unwrap_or(Path::new("."));
        let temp_path = parent.join(format!(".mp3rgain_temp_{}.mp3", std::process::id()));

        // Copy original to temp
        fs::copy(file, &temp_path)?;

        // Apply operation to temp file
        match operation(&temp_path) {
            Ok(frames) => {
                // Replace original with temp
                fs::rename(&temp_path, file)?;
                Ok(frames)
            }
            Err(e) => {
                // Clean up temp file on error
                let _ = fs::remove_file(&temp_path);
                Err(e)
            }
        }
    } else {
        operation(file)
    }
}

fn process_apply(file: &PathBuf, steps: i32, opts: &Options) -> Result<JsonFileResult> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp && !opts.dry_run {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    // Check for clipping and possibly prevent it
    let mut actual_steps = steps;
    let mut warning_msg: Option<String> = None;

    if steps > 0 && !opts.wrap_gain {
        if let Ok(info) = analyze(file) {
            if steps > info.headroom_steps {
                if opts.prevent_clipping {
                    // -k: automatically reduce gain to prevent clipping
                    let original_steps = steps;
                    actual_steps = info.headroom_steps;
                    if opts.output_format == OutputFormat::Text && !opts.quiet {
                        eprintln!(
                            "  {} {}{} - gain reduced from {} to {} steps to prevent clipping",
                            "!".yellow(),
                            dry_run_prefix,
                            filename,
                            original_steps,
                            actual_steps
                        );
                    }
                    warning_msg = Some(format!(
                        "gain reduced from {} to {} steps to prevent clipping",
                        original_steps, actual_steps
                    ));
                } else if !opts.ignore_clipping && !opts.quiet {
                    // Show warning but continue
                    if opts.output_format == OutputFormat::Text {
                        eprintln!(
                            "  {} {}{} - clipping warning: requested {} steps but only {} headroom",
                            "!".yellow(),
                            dry_run_prefix,
                            filename,
                            steps,
                            info.headroom_steps
                        );
                        eprintln!(
                            "      Use -c to ignore clipping warnings or -k to prevent clipping"
                        );
                    }
                    warning_msg = Some(format!(
                        "clipping warning: requested {} steps but only {} headroom",
                        steps, info.headroom_steps
                    ));
                }
            }
        }
    }

    // Dry run: don't actually modify
    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            println!(
                "  {} [DRY RUN] {} (would apply {} steps)",
                "~".cyan(),
                filename,
                actual_steps
            );
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some("dry_run".to_string()),
            gain_applied_steps: Some(actual_steps),
            gain_applied_db: Some(steps_to_db(actual_steps)),
            warning: warning_msg,
            dry_run: Some(true),
            ..Default::default()
        });
    }

    let apply_result = if opts.wrap_gain {
        apply_with_temp_file(file, |f| apply_gain_with_undo_wrap(f, actual_steps), opts)
    } else {
        apply_with_temp_file(file, |f| apply_gain_with_undo(f, actual_steps), opts)
    };

    match apply_result {
        Ok(frames) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!("  {} {} ({} frames)", "v".green(), filename, frames);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("success".to_string()),
                frames: Some(frames),
                gain_applied_steps: Some(actual_steps),
                gain_applied_db: Some(steps_to_db(actual_steps)),
                warning: warning_msg,
                ..Default::default()
            })
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!("  {} {} - {}", "x".red(), filename, e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("error".to_string()),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}

fn process_apply_channel(
    file: &PathBuf,
    channel: Channel,
    steps: i32,
    opts: &Options,
) -> Result<JsonFileResult> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let channel_name = match channel {
        Channel::Left => "left",
        Channel::Right => "right",
    };

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp && !opts.dry_run {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    // Dry run: don't actually modify
    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            println!(
                "  {} [DRY RUN] {} (would apply {} steps to {} channel)",
                "~".cyan(),
                filename,
                steps,
                channel_name
            );
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some("dry_run".to_string()),
            gain_applied_steps: Some(steps),
            gain_applied_db: Some(steps_to_db(steps)),
            dry_run: Some(true),
            ..Default::default()
        });
    }

    match apply_gain_channel_with_undo(file, channel, steps) {
        Ok(frames) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!(
                    "  {} {} ({} frames, {} channel)",
                    "v".green(),
                    filename,
                    frames,
                    channel_name
                );
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("success".to_string()),
                frames: Some(frames),
                gain_applied_steps: Some(steps),
                gain_applied_db: Some(steps_to_db(steps)),
                ..Default::default()
            })
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!("  {} {} - {}", "x".red(), filename, e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("error".to_string()),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}

fn process_info(file: &Path, opts: &Options) -> Result<JsonFileResult> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    match analyze(file) {
        Ok(info) => {
            match opts.output_format {
                OutputFormat::Text => {
                    if opts.quiet {
                        // Quiet mode: tab-separated output
                        println!(
                            "{}\t{}\t{}\t{}\t{:.1}\t{}\t{:.1}",
                            filename,
                            info.frame_count,
                            info.min_gain,
                            info.max_gain,
                            info.avg_gain,
                            info.headroom_steps,
                            info.headroom_db
                        );
                    } else {
                        println!("{}", filename.cyan().bold());
                        println!(
                            "  Format:      {} Layer III, {}",
                            info.mpeg_version, info.channel_mode
                        );
                        println!("  Frames:      {}", info.frame_count);
                        println!(
                            "  Gain range:  {} - {} (avg: {:.1})",
                            info.min_gain, info.max_gain, info.avg_gain
                        );
                        println!(
                            "  Headroom:    {} steps ({:+.1} dB)",
                            info.headroom_steps.to_string().green(),
                            info.headroom_db
                        );
                        println!();
                    }
                }
                OutputFormat::Tsv => {
                    // TSV format: File, MP3 gain, dB gain, Max Amplitude, Max global_gain, Min global_gain
                    println!(
                        "{}\t{}\t{:.1}\t{:.6}\t{}\t{}",
                        filename,
                        info.headroom_steps,
                        info.headroom_db,
                        1.0, // placeholder for max amplitude
                        info.max_gain,
                        info.min_gain
                    );
                }
                OutputFormat::Json => {}
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                mpeg_version: Some(info.mpeg_version),
                channel_mode: Some(info.channel_mode),
                frames: Some(info.frame_count),
                min_gain: Some(info.min_gain),
                max_gain: Some(info.max_gain),
                avg_gain: Some(info.avg_gain),
                headroom_steps: Some(info.headroom_steps),
                headroom_db: Some(info.headroom_db),
                ..Default::default()
            })
        }
        Err(e) => {
            if opts.output_format != OutputFormat::Json {
                eprintln!("{} - {}", filename.red(), e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("error".to_string()),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}

fn process_undo(file: &PathBuf, opts: &Options) -> Result<JsonFileResult> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp && !opts.dry_run {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    // Dry run: just analyze what would be done
    if opts.dry_run {
        // Try to read the undo tag to see what would happen
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            println!("  {} [DRY RUN] {} (would undo)", "~".cyan(), filename);
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some("dry_run".to_string()),
            dry_run: Some(true),
            ..Default::default()
        });
    }

    match undo_gain(file) {
        Ok(frames) => {
            if frames == 0 {
                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    println!(
                        "  {} {}{} (no changes to undo)",
                        ".".cyan(),
                        dry_run_prefix,
                        filename
                    );
                }

                Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some("skipped".to_string()),
                    frames: Some(0),
                    ..Default::default()
                })
            } else {
                // Restore timestamp if needed
                if let Some(mtime) = original_mtime {
                    restore_timestamp(file, mtime);
                }

                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    println!(
                        "  {} {} ({} frames restored)",
                        "v".green(),
                        filename,
                        frames
                    );
                }

                Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some("success".to_string()),
                    frames: Some(frames),
                    ..Default::default()
                })
            }
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!("  {} {} - {}", "x".red(), filename, e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("error".to_string()),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}

fn process_track_gain(file: &PathBuf, opts: &Options) -> Result<JsonFileResult> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "  {} {}Analyzing {}...",
            "->".cyan(),
            dry_run_prefix,
            filename
        );
    }

    match replaygain::analyze_track(file) {
        Ok(result) => {
            // Apply gain modifier
            let base_steps = result.gain_steps();
            let modified_steps = base_steps + opts.gain_modifier;

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!(
                    "      Loudness: {:.1} dB, Gain: {:+.1} dB ({} steps{}), Peak: {:.4}",
                    result.loudness_db,
                    result.gain_db,
                    base_steps,
                    if opts.gain_modifier != 0 {
                        format!(" + {} = {}", opts.gain_modifier, modified_steps)
                    } else {
                        String::new()
                    },
                    result.peak
                );
            }

            if modified_steps == 0 {
                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    println!("  {} {} (no adjustment needed)", ".".cyan(), filename);
                }
                return Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some("skipped".to_string()),
                    loudness_db: Some(result.loudness_db),
                    peak: Some(result.peak),
                    gain_applied_steps: Some(0),
                    gain_applied_db: Some(0.0),
                    ..Default::default()
                });
            }

            process_apply_replaygain(file, modified_steps, &result, opts)
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!("  {} {} - {}", "x".red(), filename, e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("error".to_string()),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}

fn process_apply_replaygain(
    file: &PathBuf,
    steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
) -> Result<JsonFileResult> {
    process_apply_replaygain_with_album(file, steps, result, opts, None)
}

fn process_apply_replaygain_with_album(
    file: &PathBuf,
    steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    album_info: Option<&AacAlbumInfo>,
) -> Result<JsonFileResult> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    let dry_run_prefix = if opts.dry_run { "[DRY RUN] " } else { "" };

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp && !opts.dry_run {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    // Check for clipping if not ignored
    let mut actual_steps = steps;
    let mut warning_msg: Option<String> = None;

    if steps > 0 && !opts.wrap_gain {
        // Check if applying this gain would cause clipping
        let gain_linear = 10.0_f64.powf(result.gain_db / 20.0);
        let new_peak = result.peak * gain_linear;
        if new_peak > 1.0 {
            if opts.prevent_clipping {
                // Calculate the maximum safe gain
                let max_safe_db = -20.0 * result.peak.log10();
                let max_safe_steps = db_to_steps(max_safe_db);
                actual_steps = max_safe_steps.max(0);

                if opts.output_format == OutputFormat::Text && !opts.quiet {
                    eprintln!(
                        "  {} {}{} - gain reduced from {} to {} steps to prevent clipping (peak: {:.4})",
                        "!".yellow(),
                        dry_run_prefix,
                        filename,
                        steps,
                        actual_steps,
                        result.peak
                    );
                }
                warning_msg = Some(format!(
                    "gain reduced from {} to {} steps to prevent clipping (peak: {:.4})",
                    steps, actual_steps, result.peak
                ));
            } else if !opts.ignore_clipping && !opts.quiet {
                if opts.output_format == OutputFormat::Text {
                    eprintln!(
                        "  {} {}{} - clipping warning: peak would be {:.2} (>{:.2})",
                        "!".yellow(),
                        dry_run_prefix,
                        filename,
                        new_peak,
                        1.0
                    );
                    eprintln!("      Use -c to ignore clipping warnings or -k to prevent clipping");
                }
                warning_msg = Some(format!(
                    "clipping warning: peak would be {:.2} (>1.00)",
                    new_peak
                ));
            }
        }
    }

    // Dry run: don't actually modify
    if opts.dry_run {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            let format_info = match result.file_type {
                AudioFileType::Aac => " (tags only)",
                AudioFileType::Mp3 => "",
            };
            println!(
                "  {} [DRY RUN] {} (would apply {:+.1} dB, {} steps{})",
                "~".cyan(),
                filename,
                steps_to_db(actual_steps),
                actual_steps,
                format_info
            );
        }
        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some("dry_run".to_string()),
            loudness_db: Some(result.loudness_db),
            peak: Some(result.peak),
            gain_applied_steps: Some(actual_steps),
            gain_applied_db: Some(steps_to_db(actual_steps)),
            warning: warning_msg,
            dry_run: Some(true),
            ..Default::default()
        });
    }

    // Handle AAC/M4A files differently - only write ReplayGain tags
    if result.file_type == AudioFileType::Aac {
        return process_apply_replaygain_aac_with_album(
            file,
            actual_steps,
            result,
            opts,
            warning_msg,
            original_mtime,
            album_info,
        );
    }

    // MP3: Apply gain to audio frames
    let apply_result = if opts.wrap_gain {
        apply_with_temp_file(file, |f| apply_gain_with_undo_wrap(f, actual_steps), opts)
    } else {
        apply_with_temp_file(file, |f| apply_gain_with_undo(f, actual_steps), opts)
    };

    match apply_result {
        Ok(frames) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!(
                    "  {} {} ({} frames, {:+.1} dB)",
                    "v".green(),
                    filename,
                    frames,
                    steps_to_db(actual_steps)
                );
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("success".to_string()),
                frames: Some(frames),
                loudness_db: Some(result.loudness_db),
                peak: Some(result.peak),
                gain_applied_steps: Some(actual_steps),
                gain_applied_db: Some(steps_to_db(actual_steps)),
                warning: warning_msg,
                ..Default::default()
            })
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!("  {} {} - {}", "x".red(), filename, e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("error".to_string()),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}

/// Apply ReplayGain to AAC/M4A files with optional album info
fn process_apply_replaygain_aac_with_album(
    file: &PathBuf,
    _actual_steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
    warning_msg: Option<String>,
    original_mtime: Option<std::time::SystemTime>,
    album_info: Option<&AacAlbumInfo>,
) -> Result<JsonFileResult> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Create ReplayGain tags for AAC
    let mut tags = mp4meta::ReplayGainTags::new();
    tags.set_track(result.gain_db, result.peak);

    // Add album tags if available
    if let Some(album) = album_info {
        tags.set_album(album.album_gain_db, album.album_peak);
    }

    // Write tags to file
    match mp4meta::write_replaygain_tags(file, &tags) {
        Ok(()) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            let tag_type = if album_info.is_some() {
                "track+album tags"
            } else {
                "tags"
            };

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!(
                    "  {} {} ({} written, {:+.1} dB)",
                    "v".green(),
                    filename,
                    tag_type,
                    result.gain_db
                );
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("success".to_string()),
                loudness_db: Some(result.loudness_db),
                peak: Some(result.peak),
                gain_applied_steps: Some(result.gain_steps()),
                gain_applied_db: Some(result.gain_db),
                warning: warning_msg,
                ..Default::default()
            })
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                eprintln!("  {} {} - {}", "x".red(), filename, e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some("error".to_string()),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}

fn restore_timestamp(file: &PathBuf, mtime: SystemTime) {
    let _ = std::fs::File::options()
        .write(true)
        .open(file)
        .and_then(|f| f.set_times(std::fs::FileTimes::new().set_modified(mtime)));
}

// =============================================================================
// Help / Version
// =============================================================================

fn print_version() {
    println!("mp3rgain version {}", VERSION);
    println!("A modern mp3gain replacement written in Rust");
    println!();
    println!("Each gain step = {} dB", GAIN_STEP_DB);
}

fn print_usage() {
    println!("{} version {}", "mp3rgain".green().bold(), VERSION);
    println!("Lossless MP3 volume adjustment - a modern mp3gain replacement");
    println!();
    println!("{}", "USAGE:".cyan().bold());
    println!("    mp3rgain [OPTIONS] <FILES>...");
    println!();
    println!("{}", "OPTIONS:".cyan().bold());
    println!(
        "    -g <i>      Apply gain of i steps (each step = {} dB)",
        GAIN_STEP_DB
    );
    println!("    -d <n>      Apply gain of n dB (rounded to nearest step)");
    println!("    -l <c> <g>  Apply gain to left (0) or right (1) channel only");
    println!("    -m <i>      Modify suggested gain by integer i");
    println!("    -r          Apply Track gain (ReplayGain analysis)");
    println!("    -a          Apply Album gain (ReplayGain analysis)");
    println!("    -e          Skip album analysis (even with multiple files)");
    println!("    -u          Undo gain changes (restore from APEv2 tag)");
    println!("    -x          Only find max amplitude of file");
    println!("    -s <mode>   Stored tag handling:");
    println!("                  c = check/show stored tag info");
    println!("                  d = delete stored tag info");
    println!("                  s = skip (ignore) stored tag info");
    println!("                  r = force recalculation");
    println!("                  i = use ID3v2 tags (not fully supported)");
    println!("                  a = use APEv2 tags (default)");
    println!("    -p          Preserve original file timestamp");
    println!("    -c          Ignore clipping warnings");
    println!("    -k          Prevent clipping (automatically limit gain)");
    println!("    -w          Wrap gain values (instead of clamping)");
    println!("    -t          Use temp file for writing (safer, required for some ops)");
    println!("    -f          Assume MPEG 2 Layer III (compatibility, no effect)");
    println!("    -q          Quiet mode (less output)");
    println!("    -R          Process directories recursively");
    println!("    -n          Dry-run mode (show what would be done)");
    println!("    --dry-run   Same as -n");
    println!("    -o <fmt>    Output format: 'text' (default), 'json', or 'tsv'");
    println!("    -v          Show version");
    println!("    -h          Show this help");
    println!();
    println!("{}", "EXAMPLES:".cyan().bold());
    println!("    mp3rgain song.mp3              Show file info");
    println!("    mp3rgain -g 2 song.mp3         Apply +2 steps (+3.0 dB)");
    println!("    mp3rgain -g -3 song.mp3        Apply -3 steps (-4.5 dB)");
    println!("    mp3rgain -d 4.5 song.mp3       Apply +4.5 dB (rounds to +3 steps)");
    println!("    mp3rgain -r song.mp3           Analyze and apply track gain");
    println!("    mp3rgain -a *.mp3              Analyze and apply album gain");
    println!("    mp3rgain -r -m 2 *.mp3         Apply track gain + 2 steps");
    println!("    mp3rgain -e *.mp3              Track gain only (skip album calc)");
    println!("    mp3rgain -u song.mp3           Undo previous gain changes");
    println!("    mp3rgain -x song.mp3           Show max amplitude only");
    println!("    mp3rgain -s c *.mp3            Check stored tag info");
    println!("    mp3rgain -s d *.mp3            Delete stored tag info");
    println!("    mp3rgain -g 2 -p song.mp3      Apply gain, preserve timestamp");
    println!("    mp3rgain -k -g 5 song.mp3      Apply gain with clipping prevention");
    println!("    mp3rgain -w -g 10 song.mp3     Apply gain with wrapping");
    println!("    mp3rgain -t -g 2 song.mp3      Apply gain using temp file");
    println!("    mp3rgain -R /path/to/music     Process directory recursively");
    println!("    mp3rgain -n -g 2 *.mp3         Dry-run (preview changes)");
    println!("    mp3rgain -o json song.mp3      Output in JSON format");
    println!("    mp3rgain -o tsv *.mp3          Output in tab-separated format");
    println!("    mp3rgain -l 0 3 song.mp3       Apply +3 steps to left channel");
    println!("    mp3rgain -l 1 -2 song.mp3      Apply -2 steps to right channel");
    println!();
    println!("{}", "NOTES:".cyan().bold());
    println!(
        "    - Each gain step = {} dB (fixed by MP3 specification)",
        GAIN_STEP_DB
    );
    println!("    - Changes are lossless and reversible");
    println!("    - Gain changes are stored in APEv2 tags for undo support");
    println!("    - Progress bar shown automatically for 5+ files");
    if replaygain::is_available() {
        println!(
            "    - ReplayGain analysis is {} (target: {} dB)",
            "enabled".green(),
            REPLAYGAIN_REFERENCE_DB
        );
    } else {
        println!();
        println!("{}", "REPLAYGAIN:".yellow().bold());
        println!("    -r and -a options require the 'replaygain' feature:");
        println!("    cargo install mp3rgain --features replaygain");
    }
}
