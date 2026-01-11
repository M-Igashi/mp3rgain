//! mp3rgain - Lossless MP3 volume adjustment
//! A modern mp3gain replacement written in Rust
//!
//! Command-line interface compatible with the original mp3gain.

use anyhow::Result;
use colored::*;
use mp3rgain::{analyze, apply_gain, db_to_steps, steps_to_db, GAIN_STEP_DB};
use std::env;
use std::path::PathBuf;
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
                _ if flag.chars().all(|c| "pqcu".contains(c)) => {
                    for c in flag.chars() {
                        match c {
                            'p' => opts.preserve_timestamp = true,
                            'q' => opts.quiet = true,
                            'c' => opts.ignore_clipping = true,
                            'u' => opts.undo = true,
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

    if opts.undo {
        eprintln!(
            "{}: -u (undo from tags) is not yet supported",
            "error".red().bold()
        );
        eprintln!("To undo a previous gain change, apply the inverse gain:");
        eprintln!("  mp3rgain -g -2 file.mp3    # undo a previous +2 step change");
        std::process::exit(1);
    }

    // Determine action
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
            if steps > info.headroom_steps {
                if !opts.quiet {
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
    }

    match apply_gain(file, steps) {
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

fn process_info(file: &PathBuf, opts: &Options) -> Result<()> {
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
    println!("    mp3rgain -g 2 -p song.mp3      Apply gain, preserve timestamp");
    println!("    mp3rgain -s c *.mp3            Check all MP3 files");
    println!();
    println!("{}", "NOTES:".cyan().bold());
    println!(
        "    - Each gain step = {} dB (fixed by MP3 specification)",
        GAIN_STEP_DB
    );
    println!("    - Changes are lossless and reversible");
    println!("    - To undo: apply the inverse gain (e.g., -g -2 to undo -g 2)");
    println!();
    println!("{}", "NOT YET IMPLEMENTED:".yellow().bold());
    println!("    -r        Apply Track gain (requires ReplayGain analysis)");
    println!("    -a        Apply Album gain (requires ReplayGain analysis)");
    println!("    -u        Undo based on stored tag info");
}
