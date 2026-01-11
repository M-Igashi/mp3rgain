//! mp3rgain - Lossless MP3 volume adjustment
//! A modern mp3gain replacement written in Rust
//!
//! Command-line interface compatible with the original mp3gain.

use anyhow::Result;
use colored::*;
use mp3rgain::replaygain::{self, ReplayGainResult, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::{analyze, apply_gain_with_undo, db_to_steps, steps_to_db, undo_gain, GAIN_STEP_DB};
use std::env;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const VERSION: &str = env!("CARGO_PKG_VERSION");

// =============================================================================
// Options
// =============================================================================

#[derive(Default)]
struct Options {
    // Gain options
    gain_steps: Option<i32>, // -g <i>
    gain_db: Option<f64>,    // -d <n>

    // Mode options
    undo: bool,       // -u
    check_only: bool, // -s c (check/analysis only)
    track_gain: bool, // -r (apply track gain)
    album_gain: bool, // -a (apply album gain)

    // Behavior options
    preserve_timestamp: bool, // -p
    ignore_clipping: bool,    // -c
    quiet: bool,              // -q

    // Files
    files: Vec<PathBuf>,
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

        if arg.starts_with('-') && arg.len() > 1 {
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
                "r" => opts.track_gain = true,
                "a" => opts.album_gain = true,
                "u" => opts.undo = true,
                "p" => opts.preserve_timestamp = true,
                "c" => opts.ignore_clipping = true,
                "q" => opts.quiet = true,
                "v" | "-version" => {
                    print_version();
                    std::process::exit(0);
                }
                "h" | "-help" => {
                    print_usage();
                    std::process::exit(0);
                }
                // Handle combined short flags like -qp
                _ if flag.chars().all(|c| "pqcura".contains(c)) => {
                    for c in flag.chars() {
                        match c {
                            'p' => opts.preserve_timestamp = true,
                            'q' => opts.quiet = true,
                            'c' => opts.ignore_clipping = true,
                            'u' => opts.undo = true,
                            'r' => opts.track_gain = true,
                            'a' => opts.album_gain = true,
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
        } else {
            // It's a file
            opts.files.push(PathBuf::from(arg));
        }

        i += 1;
    }

    Ok(opts)
}

fn run(opts: Options) -> Result<()> {
    // Validate options
    if opts.files.is_empty() {
        eprintln!("{}: no files specified", "error".red().bold());
        std::process::exit(1);
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
// Commands
// =============================================================================

fn cmd_apply(files: &[PathBuf], steps: i32, opts: &Options) -> Result<()> {
    if steps == 0 {
        if !opts.quiet {
            println!("{}: gain is 0, nothing to do", "info".cyan());
        }
        return Ok(());
    }

    let db_value = steps_to_db(steps);
    if !opts.quiet {
        println!(
            "{} Applying {} step(s) ({:+.1} dB) to {} file(s)",
            "mp3rgain".green().bold(),
            steps,
            db_value,
            files.len()
        );
        println!();
    }

    for file in files {
        process_apply(file, steps, opts)?;
    }

    Ok(())
}

fn cmd_info(files: &[PathBuf], opts: &Options) -> Result<()> {
    for file in files {
        process_info(file, opts)?;
    }
    Ok(())
}

fn cmd_undo(files: &[PathBuf], opts: &Options) -> Result<()> {
    if !opts.quiet {
        println!(
            "{} Undoing gain changes on {} file(s)",
            "mp3rgain".green().bold(),
            files.len()
        );
        println!();
    }

    for file in files {
        process_undo(file, opts)?;
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

    if !opts.quiet {
        println!(
            "{} Analyzing and applying track gain to {} file(s)",
            "mp3rgain".green().bold(),
            files.len()
        );
        println!("  Target: {} dB (ReplayGain 1.0)", REPLAYGAIN_REFERENCE_DB);
        println!();
    }

    for file in files {
        process_track_gain(file, opts)?;
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

    if !opts.quiet {
        println!(
            "{} Analyzing album gain for {} file(s)",
            "mp3rgain".green().bold(),
            files.len()
        );
        println!("  Target: {} dB (ReplayGain 1.0)", REPLAYGAIN_REFERENCE_DB);
        println!();
    }

    // First, analyze all tracks
    if !opts.quiet {
        println!("  {} Analyzing tracks...", "→".cyan());
    }

    let file_refs: Vec<&std::path::Path> = files.iter().map(|p| p.as_path()).collect();

    match replaygain::analyze_album(&file_refs) {
        Ok(album_result) => {
            if !opts.quiet {
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
                if !opts.quiet {
                    println!("  {} No adjustment needed", "·".cyan());
                }
                return Ok(());
            }

            for (i, file) in files.iter().enumerate() {
                let track_result = &album_result.tracks[i];
                process_apply_replaygain(file, steps, track_result, opts)?;
            }
        }
        Err(e) => {
            eprintln!("{}: Failed to analyze album: {}", "error".red().bold(), e);
            std::process::exit(1);
        }
    }

    Ok(())
}

// =============================================================================
// File processing
// =============================================================================

fn process_apply(file: &PathBuf, steps: i32, opts: &Options) -> Result<()> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    // Check for clipping if not ignored
    if !opts.ignore_clipping && steps > 0 {
        if let Ok(info) = analyze(file) {
            if steps > info.headroom_steps && !opts.quiet {
                eprintln!(
                    "  {} {} - clipping warning: requested {} steps but only {} headroom",
                    "!".yellow(),
                    filename,
                    steps,
                    info.headroom_steps
                );
                eprintln!("      Use -c to ignore clipping warnings");
            }
        }
    }

    match apply_gain_with_undo(file, steps) {
        Ok(frames) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if !opts.quiet {
                println!("  {} {} ({} frames)", "✓".green(), filename, frames);
            }
        }
        Err(e) => {
            if !opts.quiet {
                eprintln!("  {} {} - {}", "✗".red(), filename, e);
            }
        }
    }

    Ok(())
}

fn process_info(file: &Path, opts: &Options) -> Result<()> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    match analyze(file) {
        Ok(info) => {
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
        Err(e) => {
            eprintln!("{} - {}", filename.red(), e);
        }
    }

    Ok(())
}

fn process_undo(file: &PathBuf, opts: &Options) -> Result<()> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    match undo_gain(file) {
        Ok(frames) => {
            if frames == 0 {
                if !opts.quiet {
                    println!("  {} {} (no changes to undo)", "·".cyan(), filename);
                }
            } else {
                // Restore timestamp if needed
                if let Some(mtime) = original_mtime {
                    restore_timestamp(file, mtime);
                }

                if !opts.quiet {
                    println!(
                        "  {} {} ({} frames restored)",
                        "✓".green(),
                        filename,
                        frames
                    );
                }
            }
        }
        Err(e) => {
            if !opts.quiet {
                eprintln!("  {} {} - {}", "✗".red(), filename, e);
            }
        }
    }

    Ok(())
}

fn process_track_gain(file: &PathBuf, opts: &Options) -> Result<()> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    if !opts.quiet {
        println!("  {} Analyzing {}...", "→".cyan(), filename);
    }

    match replaygain::analyze_track(file) {
        Ok(result) => {
            if !opts.quiet {
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
                if !opts.quiet {
                    println!("  {} {} (no adjustment needed)", "·".cyan(), filename);
                }
                return Ok(());
            }

            process_apply_replaygain(file, steps, &result, opts)?;
        }
        Err(e) => {
            eprintln!("  {} {} - {}", "✗".red(), filename, e);
        }
    }

    Ok(())
}

fn process_apply_replaygain(
    file: &PathBuf,
    steps: i32,
    result: &ReplayGainResult,
    opts: &Options,
) -> Result<()> {
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    // Save original timestamp if needed
    let original_mtime = if opts.preserve_timestamp {
        std::fs::metadata(file).ok().and_then(|m| m.modified().ok())
    } else {
        None
    };

    // Check for clipping if not ignored
    if !opts.ignore_clipping && steps > 0 {
        // Check if applying this gain would cause clipping
        // Peak of 1.0 means full scale; we need headroom for the gain
        let gain_linear = 10.0_f64.powf(result.gain_db / 20.0);
        let new_peak = result.peak * gain_linear;
        if new_peak > 1.0 && !opts.quiet {
            eprintln!(
                "  {} {} - clipping warning: peak would be {:.2} (>{:.2})",
                "!".yellow(),
                filename,
                new_peak,
                1.0
            );
            eprintln!("      Use -c to ignore clipping warnings");
        }
    }

    match apply_gain_with_undo(file, steps) {
        Ok(frames) => {
            // Restore timestamp if needed
            if let Some(mtime) = original_mtime {
                restore_timestamp(file, mtime);
            }

            if !opts.quiet {
                println!(
                    "  {} {} ({} frames, {:+.1} dB)",
                    "✓".green(),
                    filename,
                    frames,
                    steps_to_db(steps)
                );
            }
        }
        Err(e) => {
            eprintln!("  {} {} - {}", "✗".red(), filename, e);
        }
    }

    Ok(())
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
        "    -g <i>    Apply gain of i steps (each step = {} dB)",
        GAIN_STEP_DB
    );
    println!("    -d <n>    Apply gain of n dB (rounded to nearest step)");
    println!("    -r        Apply Track gain (ReplayGain analysis)");
    println!("    -a        Apply Album gain (ReplayGain analysis)");
    println!("    -u        Undo gain changes (restore from APEv2 tag)");
    println!("    -s c      Check/show file info (analysis only)");
    println!("    -p        Preserve original file timestamp");
    println!("    -c        Ignore clipping warnings");
    println!("    -q        Quiet mode (less output)");
    println!("    -v        Show version");
    println!("    -h        Show this help");
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
    println!("    mp3rgain -s c *.mp3            Check all MP3 files");
    println!();
    println!("{}", "NOTES:".cyan().bold());
    println!(
        "    - Each gain step = {} dB (fixed by MP3 specification)",
        GAIN_STEP_DB
    );
    println!("    - Changes are lossless and reversible");
    println!("    - Gain changes are stored in APEv2 tags for undo support");
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
