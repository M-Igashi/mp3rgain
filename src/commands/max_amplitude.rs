use anyhow::Result;
use colored::*;
use mp3rgain::{find_max_amplitude, peak_to_headroom_db, peak_to_pcm_sample};
use rayon::prelude::*;
use std::fmt::Write as _;
use std::io::{self, Write as IoWrite};
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::threading::effective_threads;
use crate::json_output::{FileStatus, JsonFileResult, JsonOutput};
use crate::progress::{create_progress_bar, progress_finish, progress_inc, progress_set_message};
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

    let pb = create_progress_bar(files.len(), opts);
    let mut json_results: Vec<JsonFileResult> = Vec::new();

    let parallel = effective_threads(opts) > 1 && files.len() > 1;

    if parallel {
        let pb_ref = pb.as_ref();
        let collected: Vec<(Option<JsonFileResult>, String)> = files
            .par_iter()
            .map(|file| -> Result<(Option<JsonFileResult>, String)> {
                let r = process_max_amplitude(file, opts);
                if let Some(pb) = pb_ref {
                    pb.set_message(get_filename(file).to_string());
                    pb.inc(1);
                }
                Ok(r)
            })
            .collect::<Result<Vec<_>>>()?;

        let stdout = io::stdout();
        let mut handle = stdout.lock();
        for (_, text) in &collected {
            if !text.is_empty() {
                handle.write_all(text.as_bytes())?;
            }
        }
        drop(handle);

        for (result, _) in collected {
            if let Some(r) = result {
                json_results.push(r);
            }
        }
    } else {
        for file in files {
            let filename = get_filename(file);
            progress_set_message(&pb, filename);

            let (result, text) = process_max_amplitude(file, opts);
            if !text.is_empty() {
                print!("{}", text);
            }
            if let Some(r) = result {
                json_results.push(r);
            }

            progress_inc(&pb);
        }
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
