use anyhow::Result;
use colored::*;
use mp3rgain::{find_max_amplitude, peak_to_headroom_db, peak_to_pcm_sample};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::utils::{finish_without_summary, for_each_file};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::util::get_filename;

pub fn cmd_max_amplitude(files: &[PathBuf], opts: &Options) -> Result<()> {
    if opts.output_format == OutputFormat::Text && !opts.quiet {
        println!(
            "{} Finding maximum amplitude for {} file(s)",
            "mp3rgain".green().bold(),
            files.len()
        );
        println!();
    }

    let (json_results, _, _) =
        for_each_file(files, opts, |file| Ok(process_max_amplitude(file, opts)))?;

    finish_without_summary(json_results, opts)
}

fn process_max_amplitude(file: &Path, opts: &Options) -> (Option<JsonFileResult>, String) {
    let filename = get_filename(file);
    let mut out = String::new();

    match find_max_amplitude(file) {
        Ok(amp_result) => {
            let max_amp = amp_result.max_amplitude();
            let max_gain = amp_result.max_global_gain();
            let min_gain = amp_result.min_global_gain();
            // Convert to PCM scale (like mp3gain: 0-32768+)
            let max_pcm_sample = peak_to_pcm_sample(max_amp);
            let headroom_db: Option<f64> = peak_to_headroom_db(max_amp);
            let headroom_text = match headroom_db {
                Some(d) => format!("{:+.2}", d),
                None => "(silent)".to_string(),
            };

            match opts.output_format {
                OutputFormat::Text => {
                    if !opts.quiet {
                        writeln!(out, "{}", filename.cyan().bold()).ok();
                        writeln!(out, "  Max PCM sample: {:.6}", max_pcm_sample).ok();
                        writeln!(out, "  Headroom:       {} dB", headroom_text).ok();
                        writeln!(out, "  Max global_gain: {}", max_gain).ok();
                        writeln!(out, "  Min global_gain: {}", min_gain).ok();
                        writeln!(out).ok();
                    } else {
                        writeln!(
                            out,
                            "{}\t{:.6}\t{}",
                            filename, max_pcm_sample, headroom_text
                        )
                        .ok();
                    }
                    (None, out)
                }
                OutputFormat::Tsv => {
                    writeln!(
                        out,
                        "{}\t{:.6}\t{}\t{}\t{}",
                        filename, max_pcm_sample, headroom_text, max_gain, min_gain
                    )
                    .ok();
                    (None, out)
                }
                OutputFormat::Json => (
                    Some(JsonFileResult {
                        file: file.display().to_string(),
                        max_amplitude: Some(max_pcm_sample),
                        headroom_db,
                        max_gain: Some(max_gain),
                        min_gain: Some(min_gain),
                        ..Default::default()
                    }),
                    out,
                ),
            }
        }
        Err(e) => {
            if opts.output_format == OutputFormat::Json {
                (
                    Some(JsonFileResult {
                        file: file.display().to_string(),
                        status: Some(FileStatus::Error),
                        error: Some(e.to_string()),
                        ..Default::default()
                    }),
                    out,
                )
            } else {
                if !opts.quiet {
                    eprintln!("{} - {}", filename.red(), e);
                }
                (None, out)
            }
        }
    }
}
