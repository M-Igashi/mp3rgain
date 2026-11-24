use anyhow::Result;
use mp3rgain::AudioHashVersion;
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat};
use crate::commands::utils::{finish_without_summary, for_each_file};
use crate::json_output::JsonFileResult;
use crate::processors::utils::{report_file_error, report_unsupported_format};
use crate::util::{get_filename, get_path};

/// `--audio-hash` (issue #491): one `<hash>  <path>` line per file, the shape
/// of `sha256sum`, and nothing else on stdout, so the output can be sorted
/// and compared as it is.
pub fn cmd_audio_hash(files: &[PathBuf], version: AudioHashVersion, opts: &Options) -> Result<()> {
    if opts.output_format == OutputFormat::Tsv {
        println!("File\tAudio hash");
    }
    let (json_results, _, failed) = for_each_file(files, opts, |file| {
        Ok(process_audio_hash(file, version, opts))
    })?;
    finish_without_summary(json_results, failed, opts)
}

fn process_audio_hash(
    file: &Path,
    version: AudioHashVersion,
    opts: &Options,
) -> (Option<JsonFileResult>, String) {
    match mp3rgain::audio_hash(file, version) {
        Ok(hash) => match opts.output_format {
            OutputFormat::Text => (None, format!("{}  {}\n", hash, get_path(file))),
            OutputFormat::Tsv => (None, format!("{}\t{}\n", get_path(file), hash)),
            OutputFormat::Json => (
                Some(JsonFileResult {
                    file: file.display().to_string(),
                    audio_hash: Some(hash.to_string()),
                    ..Default::default()
                }),
                String::new(),
            ),
        },
        // The error's own text says "not supported for gain adjustment".
        Err(mp3rgain::Error::UnsupportedFormat { format }) => (
            Some(report_unsupported_format(
                file,
                get_filename(file),
                &format!("{format} has no audio hash"),
                opts,
            )),
            String::new(),
        ),
        Err(e) => (
            Some(report_file_error(file, get_filename(file), e, opts)),
            String::new(),
        ),
    }
}
