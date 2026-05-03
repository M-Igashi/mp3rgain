//! mp3rgain - Lossless MP3 volume adjustment
//! A modern mp3gain replacement written in Rust
//!
//! Command-line interface compatible with the original mp3gain.

mod cli;
mod commands;
mod json_output;
mod processors;
mod progress;

use anyhow::Result;
use colored::*;
use std::env;
use std::path::Path;

use cli::options::{Options, OutputFormat, StoredTagMode};
use cli::parse_args::{expand_files_recursive, parse_args};
use cli::usage::print_usage;
use commands::apply::{cmd_apply, cmd_apply_channel};
use commands::info::cmd_info;
use commands::max_amplitude::cmd_max_amplitude;
use commands::replaygain::{cmd_album_gain, cmd_track_gain};
use commands::tags::{cmd_check_tags, cmd_delete_tags};
use commands::undo::cmd_undo;

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
