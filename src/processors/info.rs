use anyhow::Result;
use colored::*;
use indicatif::ProgressBar;
use mp3rgain::replaygain::{self, ReplayGainResult};
use mp3rgain::{analyze, mp4meta, peak_to_pcm_sample, steps_to_db};
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::progress::update_analysis_progress;
use crate::util::get_filename;

/// Scan the file's global_gain range for an info row. MP3-only: AAC files
/// fail the frame scan and get the (255, 0) placeholder mp3gain also prints.
pub fn scan_gain_range_for_row(file: &Path) -> (u8, u8) {
    analyze(file)
        .map(|info| (info.max_gain(), info.min_gain()))
        .unwrap_or((255, 0))
}

/// Format one mp3gain-compatible per-file row from a ReplayGain analysis
/// result. Shared by the per-file path below and the single-pass album flow
/// in `cmd_info`, which pre-computes `gain_range` in parallel instead of
/// re-scanning each file inside its sequential emit loop.
pub fn format_rg_row(
    file: &Path,
    opts: &Options,
    rg_result: &ReplayGainResult,
    gain_range: (u8, u8),
) -> Result<(JsonFileResult, String)> {
    let mut out = String::new();
    let filename = get_filename(file);

    // Reuse the ReplayGain peak instead of re-decoding the audio
    // via find_max_amplitude (issue #135).
    let max_amp = rg_result.peak();
    let (max_gain, min_gain) = gain_range;

    // Calculate gain with modifier, matching the apply paths (-m steps + -d dB,
    // combined into steps via gain_modifier_steps)
    let modifier_steps = opts.gain_modifier_steps();
    let gain_steps = rg_result.gain_steps() + modifier_steps;
    let gain_db = rg_result.gain_db() + steps_to_db(modifier_steps);

    // Max Amplitude scaled to 32768 (mp3gain format for beets)
    // beets divides by 32768, so we output peak * 32768
    let max_amplitude_scaled = peak_to_pcm_sample(rg_result.peak());

    match opts.output_format {
        OutputFormat::Tsv => {
            writeln!(
                out,
                "{}\t{}\t{:.6}\t{:.6}\t{}\t{}",
                filename, gain_steps, gain_db, max_amplitude_scaled, max_gain, min_gain
            )?;
        }
        OutputFormat::Text => {
            if !opts.quiet {
                writeln!(out, "{}", filename.cyan().bold())?;
                writeln!(out, "  Recommended \"Track\" dB change: {:.6}", gain_db)?;
                writeln!(
                    out,
                    "  Recommended \"Track\" mp3 gain change: {}",
                    gain_steps
                )?;
                writeln!(
                    out,
                    "  Max PCM sample at current gain: {:.6}",
                    max_amplitude_scaled
                )?;
                writeln!(out, "  Max mp3 global gain field: {}", max_gain)?;
                writeln!(out, "  Min mp3 global gain field: {}", min_gain)?;
                writeln!(out)?;
            }
        }
        OutputFormat::Json => {}
    }

    Ok((
        JsonFileResult {
            file: file.display().to_string(),
            loudness_db: Some(rg_result.loudness_db()),
            gain_applied_db: Some(gain_db),
            gain_applied_steps: Some(gain_steps),
            peak: Some(rg_result.peak()),
            max_amplitude: Some(max_amp),
            max_gain: Some(max_gain),
            min_gain: Some(min_gain),
            ..Default::default()
        },
        out,
    ))
}

pub fn process_info(
    file: &Path,
    opts: &Options,
    analysis_pb: Option<&ProgressBar>,
) -> Result<(JsonFileResult, String)> {
    let mut out = String::new();
    let result = process_info_into(file, opts, analysis_pb, &mut out)?;
    Ok((result, out))
}

fn process_info_into(
    file: &Path,
    opts: &Options,
    analysis_pb: Option<&ProgressBar>,
    out: &mut String,
) -> Result<JsonFileResult> {
    let filename = get_filename(file);

    // Perform ReplayGain analysis for TSV/Text output (mp3gain compatible)
    if matches!(opts.output_format, OutputFormat::Tsv | OutputFormat::Text)
        && replaygain::is_available()
    {
        let rg_result = if let Some(pb) = analysis_pb {
            replaygain::analyze_track_with_progress(file, opts.track_index, &|bytes, total| {
                update_analysis_progress(&Some(pb.clone()), bytes, total);
            })
        } else {
            replaygain::analyze_track_with_index(file, opts.track_index)
        };

        match rg_result {
            Ok(rg_result) => {
                let (result, text) =
                    format_rg_row(file, opts, &rg_result, scan_gain_range_for_row(file))?;
                out.push_str(&text);
                return Ok(result);
            }
            Err(e) => {
                eprintln!("{} - {}", filename.red(), e);
                return Ok(JsonFileResult {
                    file: file.display().to_string(),
                    status: Some(FileStatus::Error),
                    error: Some(e.to_string()),
                    ..Default::default()
                });
            }
        }
    }

    // Check if this is an M4A/AAC file - if so, show appropriate message
    if mp4meta::is_mp4_file(file) {
        let codec = mp4meta::detect_mp4_audio_codec(file);
        let is_alac = matches!(codec, Some(mp4meta::Mp4AudioCodec::Alac));
        let format_str = if is_alac { "M4A/ALAC" } else { "M4A/AAC" };

        match opts.output_format {
            OutputFormat::Text => {
                if opts.quiet {
                    writeln!(out, "{}\t{}\t-\t-\t-\t-\t-", filename, format_str)?;
                } else {
                    writeln!(out, "{}", filename.cyan().bold())?;
                    writeln!(out, "  Format:      {}", format_str)?;
                    if is_alac {
                        writeln!(
                            out,
                            "  {}",
                            "ALAC files are not supported for gain adjustment".yellow()
                        )?;
                    } else {
                        writeln!(
                            out,
                            "  {}",
                            "Note: Use -r or -a for ReplayGain analysis".yellow()
                        )?;
                    }
                    writeln!(out)?;
                }
            }
            OutputFormat::Tsv => {
                writeln!(out, "{}\t-\t-\t-\t-\t-", filename)?;
            }
            OutputFormat::Json => {}
        }

        return Ok(JsonFileResult {
            file: file.display().to_string(),
            status: Some(FileStatus::Info),
            ..Default::default()
        });
    }

    // MP3 file: use basic analysis
    match analyze(file) {
        Ok(info) => {
            match opts.output_format {
                OutputFormat::Text => {
                    if opts.quiet {
                        // Quiet mode: tab-separated output
                        writeln!(
                            out,
                            "{}\t{}\t{}\t{}\t{:.1}\t{}\t{:.1}",
                            filename,
                            info.frame_count(),
                            info.min_gain(),
                            info.max_gain(),
                            info.avg_gain(),
                            info.headroom_steps(),
                            info.headroom_db()
                        )?;
                    } else {
                        writeln!(out, "{}", filename.cyan().bold())?;
                        writeln!(
                            out,
                            "  Format:      {} Layer III, {}",
                            info.mpeg_version(),
                            info.channel_mode()
                        )?;
                        writeln!(out, "  Frames:      {}", info.frame_count())?;
                        writeln!(
                            out,
                            "  Gain range:  {} - {} (avg: {:.1})",
                            info.min_gain(),
                            info.max_gain(),
                            info.avg_gain()
                        )?;
                        writeln!(
                            out,
                            "  Headroom:    {} steps ({:+.1} dB)",
                            info.headroom_steps().to_string().green(),
                            info.headroom_db()
                        )?;
                        writeln!(out)?;
                    }
                }
                OutputFormat::Tsv => {
                    // Fallback TSV (ReplayGain not available): basic info
                    writeln!(
                        out,
                        "{}\t{}\t{:.1}\t{:.6}\t{}\t{}",
                        filename,
                        info.headroom_steps(),
                        info.headroom_db(),
                        1.0,
                        info.max_gain(),
                        info.min_gain()
                    )?;
                }
                OutputFormat::Json => {}
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                mpeg_version: Some(info.mpeg_version().to_string()),
                channel_mode: Some(info.channel_mode().to_string()),
                frames: Some(info.frame_count()),
                min_gain: Some(info.min_gain()),
                max_gain: Some(info.max_gain()),
                avg_gain: Some(info.avg_gain()),
                headroom_steps: Some(info.headroom_steps()),
                headroom_db: Some(info.headroom_db()),
                ..Default::default()
            })
        }
        Err(e) => {
            if opts.output_format != OutputFormat::Json {
                eprintln!("{} - {}", filename.red(), e);
            }

            Ok(JsonFileResult {
                file: file.display().to_string(),
                status: Some(FileStatus::Error),
                error: Some(e.to_string()),
                ..Default::default()
            })
        }
    }
}
