use anyhow::Result;
use colored::*;
use mp3rgain::find_max_amplitude;
use std::path::PathBuf;

use crate::cli::options::{Options, OutputFormat};
use crate::json_output::{JsonFileResult, JsonOutput};
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

    for file in files {
        let filename = get_filename(file);
        progress_set_message(&pb, filename);

        match find_max_amplitude(file) {
            Ok(amp_result) => {
                let max_amp = amp_result.max_amplitude();
                let max_gain = amp_result.max_global_gain();
                let min_gain = amp_result.min_global_gain();
                // Convert to PCM scale (like mp3gain: 0-32768+)
                let max_pcm_sample = max_amp * 32768.0;
                let headroom_db = if max_amp > 0.0 {
                    -20.0 * max_amp.log10()
                } else {
                    f64::INFINITY
                };

                match opts.output_format {
                    OutputFormat::Text => {
                        if !opts.quiet {
                            println!("{}", filename.cyan().bold());
                            println!("  Max PCM sample: {:.6}", max_pcm_sample);
                            println!("  Headroom:       {:+.2} dB", headroom_db);
                            println!("  Max global_gain: {}", max_gain);
                            println!("  Min global_gain: {}", min_gain);
                            println!();
                        } else {
                            println!("{}\t{:.6}\t{:.2}", filename, max_pcm_sample, headroom_db);
                        }
                    }
                    OutputFormat::Tsv => {
                        println!(
                            "{}\t{:.6}\t{:.2}\t{}\t{}",
                            filename, max_pcm_sample, headroom_db, max_gain, min_gain
                        );
                    }
                    OutputFormat::Json => {
                        json_results.push(JsonFileResult {
                            file: file.display().to_string(),
                            max_amplitude: Some(max_pcm_sample),
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
