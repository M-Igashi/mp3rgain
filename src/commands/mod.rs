pub mod apply;
pub mod info;
pub mod max_amplitude;
pub mod replaygain;
pub mod tags;
pub mod threading;
pub mod undo;
pub mod utils;

use anyhow::Result;
use colored::*;

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::cli::parse_args::expand_files_recursive;

use apply::{cmd_apply, cmd_apply_channel};
use info::cmd_info;
use max_amplitude::cmd_max_amplitude;
use replaygain::{cmd_album_gain, cmd_track_gain};
use tags::{cmd_check_tags, cmd_delete_tags};
use undo::cmd_undo;

/// Validate options and dispatch to the appropriate `cmd_*` handler.
pub fn run(mut opts: Options) -> Result<()> {
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

    // Configure the global rayon pool from -j / --threads / MP3RGAIN_THREADS.
    // Default is std::thread::available_parallelism(); -j 1 forces serial.
    threading::install_global_pool(threading::effective_threads(&opts));

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
