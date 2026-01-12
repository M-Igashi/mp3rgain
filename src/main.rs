//! mp3rgain - Lossless MP3 volume adjustment
//! A modern mp3gain replacement written in Rust
//!
//! Command-line interface compatible with the original mp3gain.

use anyhow::Result;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use mp3rgain::replaygain::{self, ReplayGainResult, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::{
    analyze, apply_gain_channel_with_undo, apply_gain_with_undo, db_to_steps, steps_to_db,
    undo_gain, Channel, GAIN_STEP_DB,
};
use serde::Serialize;
use std::env;
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
}

#[derive(Default)]
struct Options {
    // Gain options
    gain_steps: Option<i32>,              // -g <i>
    gain_db: Option<f64>,                 // -d <n>
    channel_gain: Option<(Channel, i32)>, // -l <channel> <gain>

    // Mode options
    undo: bool,       // -u
    check_only: bool, // -s c (check/analysis only)
    track_gain: bool, // -r (apply track gain)
    album_gain: bool, // -a (apply album gain)

    // Behavior options
    preserve_timestamp: bool,    // -p
    ignore_clipping: bool,       // -c
    prevent_clipping: bool,      // -k
    quiet: bool,                 // -q
    recursive: bool,             // -R
    dry_run: bool,               // -n or --dry-run
    output_format: OutputFormat, // -o <format>

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
                "s" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -s requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    match args[i].as_str() {
                        "c" => opts.check_only = true,
                        other => {
                            eprintln!(
                                "{}: -s {} is not yet supported",
                                "warning".yellow().bold(),
                                other
                            );
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
                        other => {
                            eprintln!(
                                "{}: unknown output format '{}', use 'text' or 'json'",
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
                "u" => opts.undo = true,
                "p" => opts.preserve_timestamp = true,
                "c" => opts.ignore_clipping = true,
                "k" => opts.prevent_clipping = true,
                "q" => opts.quiet = true,
                "R" => opts.recursive = true,
                "n" => opts.dry_run = true,
                "v" | "-version" => {
                    print_version();
                    std::process::exit(0);
                }
                "h" | "-help" => {
                    print_usage();
                    std::process::exit(0);
                }
                // Handle combined short flags like -qp, -kc, etc.
                _ if flag.chars().all(|c| "pqckuranR".contains(c)) => {
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
            collect_mp3_files(path, &mut result)?;
        } else {
            result.push(path.clone());
        }
    }

    result.sort();
    Ok(result)
}

fn collect_mp3_files(dir: &Path, result: &mut Vec<PathBuf>) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            collect_mp3_files(&path, result)?;
        } else if let Some(ext) = path.extension() {
            if ext.eq_ignore_ascii_case("mp3") {
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
            eprintln!("{}: no MP3 files found", "error".red().bold());
            std::process::exit(1);
        }
    }

    // Determine action
    if opts.undo {
        // -u: undo from APEv2 tags
        return cmd_undo(&opts.files, &opts);
    }

    if opts.album_gain {
        // -a: apply album gain (ReplayGain)
        return cmd_album_gain(&opts.files, &opts);
    }

    if opts.track_gain {
        // -r: apply track gain (ReplayGain)
        return cmd_track_gain(&opts.files, &opts);
    }

    if opts.channel_gain.is_some() {
        // -l: apply channel-specific gain
        let (channel, steps) = opts.channel_gain.unwrap();
        return cmd_apply_channel(&opts.files, channel, steps, &opts);
    }

    if opts.check_only {
        // -s c: analysis only
        cmd_info(&opts.files, &opts)
    } else if opts.gain_steps.is_some() || opts.gain_db.is_some() {
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
    if opts.quiet || opts.output_format == OutputFormat::Json || total < PROGRESS_THRESHOLD {
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

    if opts.output_format != OutputFormat::Json && !opts.quiet {
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
    } else if opts.dry_run && !opts.quiet {
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

    if opts.output_format != OutputFormat::Json && !opts.quiet {
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
    } else if opts.dry_run && !opts.quiet {
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

    if opts.output_format != OutputFormat::Json && !opts.quiet {
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
    } else if opts.dry_run && !opts.quiet {
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

    if opts.output_format != OutputFormat::Json && !opts.quiet {
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
    } else if opts.dry_run && !opts.quiet {
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

    if opts.output_format != OutputFormat::Json && !opts.quiet {
        println!(
            "{}{} Analyzing album gain for {} file(s)",
            dry_run_prefix,
            "mp3rgain".green().bold(),
            files.len()
        );
        println!("  Target: {} dB (ReplayGain 1.0)", REPLAYGAIN_REFERENCE_DB);
        println!();
    }

    // First, analyze all tracks
    if opts.output_format != OutputFormat::Json && !opts.quiet {
        println!("  {} Analyzing tracks...", "->".cyan());
    }

    let file_refs: Vec<&std::path::Path> = files.iter().map(|p| p.as_path()).collect();

    match replaygain::analyze_album(&file_refs) {
        Ok(album_result) => {
            if opts.output_format != OutputFormat::Json && !opts.quiet {
                println!();
                println!("  Album loudness: {:.1} dB", album_result.album_loudness_db);
                println!(
                    "  Album gain:     {:+.1} dB ({} steps)",
                    album_result.album_gain_db,
                    album_result.album_gain_steps()
                );
                println!("  Album peak:     {:.4}", album_result.album_peak);
                println!();
            }

            // Apply album gain to all files
            let steps = album_result.album_gain_steps();

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
                            gain_steps: album_result.album_gain_steps(),
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
                let result = process_apply_replaygain(file, steps, track_result, opts)?;
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
                        gain_steps: album_result.album_gain_steps(),
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
            } else if opts.dry_run && !opts.quiet {
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

    if steps > 0 {
        if let Ok(info) = analyze(file) {
            if steps > info.headroom_steps {
                if opts.prevent_clipping {
                    // -k: automatically reduce gain to prevent clipping
                    let original_steps = steps;
                    actual_steps = info.headroom_steps;
                    if opts.output_format != OutputFormat::Json && !opts.quiet {
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
                    eprintln!(
                        "  {} {}{} - clipping warning: requested {} steps but only {} headroom",
                        "!".yellow(),
                        dry_run_prefix,
                        filename,
                        steps,
                        info.headroom_steps
                    );
                    eprintln!("      Use -c to ignore clipping warnings or -k to prevent clipping");
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
        if opts.output_format != OutputFormat::Json && !opts.quiet {
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

    match apply_gain_with_undo(file, actual_steps) {
        Ok(frames) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format != OutputFormat::Json && !opts.quiet {
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
            if opts.output_format != OutputFormat::Json && !opts.quiet {
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
        if opts.output_format != OutputFormat::Json && !opts.quiet {
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

            if opts.output_format != OutputFormat::Json && !opts.quiet {
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
            if opts.output_format != OutputFormat::Json && !opts.quiet {
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
            if opts.output_format != OutputFormat::Json {
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
        if opts.output_format != OutputFormat::Json && !opts.quiet {
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
                if opts.output_format != OutputFormat::Json && !opts.quiet {
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

                if opts.output_format != OutputFormat::Json && !opts.quiet {
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
            if opts.output_format != OutputFormat::Json && !opts.quiet {
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

    if opts.output_format != OutputFormat::Json && !opts.quiet {
        println!(
            "  {} {}Analyzing {}...",
            "->".cyan(),
            dry_run_prefix,
            filename
        );
    }

    match replaygain::analyze_track(file) {
        Ok(result) => {
            if opts.output_format != OutputFormat::Json && !opts.quiet {
                println!(
                    "      Loudness: {:.1} dB, Gain: {:+.1} dB ({} steps), Peak: {:.4}",
                    result.loudness_db,
                    result.gain_db,
                    result.gain_steps(),
                    result.peak
                );
            }

            let steps = result.gain_steps();
            if steps == 0 {
                if opts.output_format != OutputFormat::Json && !opts.quiet {
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

            process_apply_replaygain(file, steps, &result, opts)
        }
        Err(e) => {
            if opts.output_format != OutputFormat::Json && !opts.quiet {
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

    if steps > 0 {
        // Check if applying this gain would cause clipping
        let gain_linear = 10.0_f64.powf(result.gain_db / 20.0);
        let new_peak = result.peak * gain_linear;
        if new_peak > 1.0 {
            if opts.prevent_clipping {
                // Calculate the maximum safe gain
                let max_safe_db = -20.0 * result.peak.log10();
                let max_safe_steps = db_to_steps(max_safe_db);
                actual_steps = max_safe_steps.max(0);

                if opts.output_format != OutputFormat::Json && !opts.quiet {
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
                if opts.output_format != OutputFormat::Json {
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
        if opts.output_format != OutputFormat::Json && !opts.quiet {
            println!(
                "  {} [DRY RUN] {} (would apply {:+.1} dB, {} steps)",
                "~".cyan(),
                filename,
                steps_to_db(actual_steps),
                actual_steps
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

    match apply_gain_with_undo(file, actual_steps) {
        Ok(frames) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if opts.output_format != OutputFormat::Json && !opts.quiet {
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
            if opts.output_format != OutputFormat::Json && !opts.quiet {
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
    println!("    -r          Apply Track gain (ReplayGain analysis)");
    println!("    -a          Apply Album gain (ReplayGain analysis)");
    println!("    -u          Undo gain changes (restore from APEv2 tag)");
    println!("    -s c        Check/show file info (analysis only)");
    println!("    -p          Preserve original file timestamp");
    println!("    -c          Ignore clipping warnings");
    println!("    -k          Prevent clipping (automatically limit gain)");
    println!("    -q          Quiet mode (less output)");
    println!("    -R          Process directories recursively");
    println!("    -n          Dry-run mode (show what would be done)");
    println!("    --dry-run   Same as -n");
    println!("    -o <fmt>    Output format: 'text' (default) or 'json'");
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
    println!("    mp3rgain -u song.mp3           Undo previous gain changes");
    println!("    mp3rgain -g 2 -p song.mp3      Apply gain, preserve timestamp");
    println!("    mp3rgain -k -g 5 song.mp3      Apply gain with clipping prevention");
    println!("    mp3rgain -R /path/to/music     Process directory recursively");
    println!("    mp3rgain -n -g 2 *.mp3         Dry-run (preview changes)");
    println!("    mp3rgain -o json song.mp3      Output in JSON format");
    println!("    mp3rgain -s c *.mp3            Check all MP3 files");
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
