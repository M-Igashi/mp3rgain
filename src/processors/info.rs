use anyhow::Result;
use colored::*;
use mp3rgain::replaygain::{AlbumGainResult, ReplayGainResult};
use mp3rgain::{analyze, mp4meta, peak_to_pcm_sample};
use std::fmt::Write as _;
use std::path::Path;

use crate::cli::options::{Options, OutputFormat};
use crate::json_output::{FileStatus, JsonFileResult};
use crate::util::{get_filename, get_path};

/// Scan the file's global_gain range for an info row, as `(max, min)`.
///
/// `mp3rgain::gain_range` dispatches on the container, so AAC files report the
/// same values `-x` does (issue #329: the MP3-only scanner failed on AAC, so
/// every AAC file took the fallback branch).
///
/// `None` means the file could not be scanned at all — a corrupt stream, or a
/// container none of the scanners handles. It prints as `-`; the fallback used
/// to be (255, 0), the pair `analyze_data` starts its accumulators at, which
/// reads as a real full-range measurement. Raw ADTS `.aac` was the case that
/// surfaced this and now scans like any other AAC stream (issue #330).
pub fn scan_gain_range_for_row(file: &Path) -> Option<(u8, u8)> {
    mp3rgain::gain_range(file).ok().map(|(min, max)| (max, min))
}

/// The two global_gain columns as they are printed: the measured values, or
/// `-` for a file that could not be scanned, the same way the M4A rows below
/// mark a column that does not apply.
pub fn gain_range_fields(gain_range: Option<(u8, u8)>) -> (String, String) {
    match gain_range {
        Some((max, min)) => (max.to_string(), min.to_string()),
        None => ("-".to_string(), "-".to_string()),
    }
}

/// One mp3gain-compatible TSV row from a ReplayGain analysis result.
/// Shared by the info command and the gain-applying commands (`-r`, `-a`),
/// which emit the recommended change before touching the frames.
pub fn tsv_rg_row(
    file: &Path,
    opts: &Options,
    rg_result: &ReplayGainResult,
    gain_range: Option<(u8, u8)>,
) -> String {
    let (gain_steps, gain_db) = opts.modified_gain(rg_result.gain_db());
    let (max_gain, min_gain) = gain_range_fields(gain_range);
    format!(
        "{}\t{}\t{:.6}\t{:.6}\t{}\t{}\n",
        get_path(file),
        gain_steps,
        gain_db,
        opts.tsv_peak(rg_result.peak()),
        max_gain,
        min_gain
    )
}

/// The `"Album"` row closing a TSV listing, whose global_gain columns span
/// every per-file row that could be scanned. Shared by `-o tsv` and
/// `-a -o tsv`, which print the same rows for the same files (issue #318).
pub fn tsv_album_row(
    opts: &Options,
    album: &AlbumGainResult,
    gain_ranges: impl IntoIterator<Item = Option<(u8, u8)>>,
) -> String {
    let range = gain_ranges
        .into_iter()
        .flatten()
        .reduce(|(max_a, min_a), (max_b, min_b)| (max_a.max(max_b), min_a.min(min_b)));
    let (gain_steps, gain_db) = opts.modified_gain(album.album_gain_db());
    let (max_gain, min_gain) = gain_range_fields(range);
    format!(
        "\"Album\"\t{}\t{:.6}\t{:.6}\t{}\t{}\n",
        gain_steps,
        gain_db,
        opts.tsv_peak(album.album_peak()),
        max_gain,
        min_gain
    )
}

/// The mp3gain-compatible text block or TSV row for one file of `cmd_info`'s
/// single-pass ReplayGain flow, which pre-computes `gain_range` in parallel
/// instead of re-scanning each file inside its sequential emit loop.
pub fn format_rg_row(
    file: &Path,
    opts: &Options,
    rg_result: &ReplayGainResult,
    gain_range: Option<(u8, u8)>,
) -> Result<String> {
    let mut out = String::new();
    let filename = get_filename(file);

    let (max_gain_field, min_gain_field) = gain_range_fields(gain_range);

    // Gain with the -m / -d modifiers folded in, the same way the apply
    // paths report it.
    let (gain_steps, gain_db) = opts.modified_gain(rg_result.gain_db());

    // Max Amplitude scaled to 32768 (mp3gain format for beets), from the
    // ReplayGain peak rather than a second decode (issue #135). beets divides
    // by 32768, so we output peak * 32768.
    let max_amplitude_scaled = peak_to_pcm_sample(rg_result.peak());

    match opts.output_format {
        OutputFormat::Tsv => {
            out.push_str(&tsv_rg_row(file, opts, rg_result, gain_range));
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
                writeln!(out, "  Max mp3 global gain field: {}", max_gain_field)?;
                writeln!(out, "  Min mp3 global gain field: {}", min_gain_field)?;
                writeln!(out)?;
            }
        }
        OutputFormat::Json => {}
    }

    Ok(out)
}

/// The `-o json` info record: MP3 frame statistics rather than a ReplayGain
/// analysis, which `cmd_info` runs for text and TSV only.
pub fn process_info(file: &Path) -> JsonFileResult {
    // An MP4 has no MP3 frames to report on.
    if mp4meta::is_mp4_file(file) {
        return JsonFileResult {
            file: file.display().to_string(),
            status: Some(FileStatus::Info),
            ..Default::default()
        };
    }

    match analyze(file) {
        Ok(info) => JsonFileResult {
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
        },
        Err(e) => JsonFileResult::error(file, e),
    }
}

#[cfg(test)]
mod tests {
    use super::gain_range_fields;

    /// Issue #329: an unscannable file must not print (255, 0), the pair the
    /// gain accumulators start at, as if it were a measurement.
    #[test]
    fn an_unscannable_range_prints_as_dashes() {
        assert_eq!(
            gain_range_fields(None),
            ("-".to_string(), "-".to_string()),
            "an unscannable file must not report a measured range"
        );
        assert_eq!(
            gain_range_fields(Some((181, 110))),
            ("181".to_string(), "110".to_string())
        );
    }
}
