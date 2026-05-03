//! mp3rgain - Lossless MP3 volume adjustment
//! A modern mp3gain replacement written in Rust
//!
//! Command-line interface compatible with the original mp3gain.

mod cli;
mod commands;
mod json_output;
mod processors;
mod progress;
mod util;

use anyhow::Result;
use std::env;

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        cli::usage::print_usage();
        return Ok(());
    }

    let opts = cli::parse_args::parse_args(&args[1..])?;
    commands::run(opts)
}
