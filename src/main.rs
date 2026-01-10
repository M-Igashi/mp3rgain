//! mp3rgain - Lossless MP3 volume adjustment
//! A modern mp3gain replacement written in Rust

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::*;
use mp3rgain::{analyze, apply_gain, db_to_steps, steps_to_db, GAIN_STEP_DB};
use std::path::PathBuf;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "mp3rgain")]
#[command(author, version, about = "Lossless MP3 volume adjustment - a modern mp3gain replacement")]
#[command(after_help = "EXAMPLES:
    mp3rgain apply -g 2 song.mp3       Apply +2 steps (+3.0 dB)
    mp3rgain apply -d 4.5 song.mp3     Apply +4.5 dB (rounds to +3 steps)
    mp3rgain apply -g -3 *.mp3         Reduce volume by 3 steps (-4.5 dB)
    mp3rgain info song.mp3             Show current gain info
    mp3rgain undo song.mp3 -g 2        Undo previous +2 step adjustment

NOTES:
    Each gain step = 1.5 dB (fixed by MP3 specification)
    Changes are lossless and reversible")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply gain adjustment to MP3 files
    Apply {
        /// Gain in steps (each step = 1.5 dB)
        #[arg(short = 'g', long, allow_hyphen_values = true, conflicts_with = "db")]
        gain: Option<i32>,
        
        /// Gain in decibels (rounded to nearest step)
        #[arg(short = 'd', long, allow_hyphen_values = true, conflicts_with = "gain")]
        db: Option<f64>,
        
        /// MP3 files to process
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    
    /// Show gain information for MP3 files
    Info {
        /// MP3 files to analyze
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    
    /// Undo a previous gain adjustment
    Undo {
        /// Original gain that was applied (in steps)
        #[arg(short = 'g', long, allow_hyphen_values = true, conflicts_with = "db")]
        gain: Option<i32>,
        
        /// Original gain that was applied (in dB)
        #[arg(short = 'd', long, allow_hyphen_values = true, conflicts_with = "gain")]
        db: Option<f64>,
        
        /// MP3 files to restore
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    
    /// Show version information
    Version,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Apply { gain, db, files } => {
            let steps = match (gain, db) {
                (Some(g), _) => g,
                (_, Some(d)) => db_to_steps(d),
                _ => {
                    eprintln!("{}: specify either --gain or --db", "error".red().bold());
                    std::process::exit(1);
                }
            };
            
            if steps == 0 {
                println!("{}: gain is 0, nothing to do", "info".cyan());
                return Ok(());
            }
            
            let db_value = steps_to_db(steps);
            println!(
                "{} Applying {} step(s) ({:+.1} dB) to {} file(s)",
                "mp3rgain".green().bold(),
                steps,
                db_value,
                files.len()
            );
            println!();
            
            for file in &files {
                process_apply(file, steps)?;
            }
            
            Ok(())
        }
        
        Commands::Info { files } => {
            for file in &files {
                process_info(file)?;
            }
            Ok(())
        }
        
        Commands::Undo { gain, db, files } => {
            let steps = match (gain, db) {
                (Some(g), _) => -g,  // Negate to undo
                (_, Some(d)) => -db_to_steps(d),
                _ => {
                    eprintln!("{}: specify the original gain with --gain or --db", "error".red().bold());
                    std::process::exit(1);
                }
            };
            
            let db_value = steps_to_db(steps);
            println!(
                "{} Undoing {} step(s) ({:+.1} dB) on {} file(s)",
                "mp3rgain".green().bold(),
                -steps,
                -db_value,
                files.len()
            );
            println!();
            
            for file in &files {
                process_apply(file, steps)?;
            }
            
            Ok(())
        }
        
        Commands::Version => {
            println!("{} {}", "mp3rgain".green().bold(), VERSION);
            println!("Lossless MP3 volume adjustment");
            println!("A modern mp3gain replacement written in Rust");
            println!();
            println!("Each gain step = {} dB", GAIN_STEP_DB);
            Ok(())
        }
    }
}

fn process_apply(file: &PathBuf, steps: i32) -> Result<()> {
    let filename = file.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    
    match apply_gain(file, steps) {
        Ok(frames) => {
            println!(
                "  {} {} ({} frames)",
                "✓".green(),
                filename,
                frames
            );
        }
        Err(e) => {
            eprintln!(
                "  {} {} - {}",
                "✗".red(),
                filename,
                e
            );
        }
    }
    
    Ok(())
}

fn process_info(file: &PathBuf) -> Result<()> {
    let filename = file.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    
    match analyze(file) {
        Ok(info) => {
            println!("{}", filename.cyan().bold());
            println!("  Format:      {} Layer III, {}", info.mpeg_version, info.channel_mode);
            println!("  Frames:      {}", info.frame_count);
            println!("  Gain range:  {} - {} (avg: {:.1})", info.min_gain, info.max_gain, info.avg_gain);
            println!(
                "  Headroom:    {} steps ({:+.1} dB)",
                info.headroom_steps.to_string().green(),
                info.headroom_db
            );
            println!();
        }
        Err(e) => {
            eprintln!("{} - {}", filename.red(), e);
        }
    }
    
    Ok(())
}
