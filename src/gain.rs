use crate::analysis::{analyze_data, ChannelMode};
use crate::ape::{
    parse_undo_values, parse_undo_wrap, read_ape_tag, replace_ape_tag, ApeReplayGain,
    TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO,
};
use crate::error::{Error, Result};
use crate::frame::{apply_gain_to_data, scan_gain_range, GainMode, SaturationStats};

use std::fs;
use std::path::Path;

/// MP3 gain step size in dB (fixed by format specification).
///
/// One `global_gain` step scales amplitude by 2^(1/4), i.e. exactly
/// `20*log10(2)/4 ≈ 1.50515 dB` — commonly rounded to "1.5 dB" in docs and
/// UI. mp3gain itself divides by `5.0 * log10(2.0)` when converting dB to
/// steps, so using the exact value here matches its rounding. The previous
/// approximation of 1.5 made post-apply residual ReplayGain tags drift by
/// ~0.005 dB per applied step (issue #291).
pub const GAIN_STEP_DB: f64 = 1.505_149_978_319_906;

/// Maximum global_gain value
pub const MAX_GAIN: u8 = 255;

/// Channel selection for independent gain adjustment
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Channel {
    /// Left channel (channel 0)
    Left,
    /// Right channel (channel 1)
    Right,
}

impl Channel {
    /// Get channel index (0 for left, 1 for right)
    pub fn index(&self) -> usize {
        match self {
            Channel::Left => 0,
            Channel::Right => 1,
        }
    }

    /// Create from index (0 = left, 1 = right)
    pub fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Channel::Left),
            1 => Some(Channel::Right),
            _ => None,
        }
    }

    /// Lowercase short name (`"left"` / `"right"`) for log/CLI output.
    pub fn name(&self) -> &'static str {
        match self {
            Channel::Left => "left",
            Channel::Right => "right",
        }
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Channel::Left => f.write_str("Left"),
            Channel::Right => f.write_str("Right"),
        }
    }
}

/// Options for applying gain adjustment to an MP3 file.
///
/// Use the builder pattern to configure how gain should be applied:
///
/// ```no_run
/// use mp3rgain::GainOptions;
/// use std::path::Path;
///
/// // Simple gain adjustment (+3 dB = 2 steps)
/// GainOptions::new(2).apply(Path::new("song.mp3")).unwrap();
///
/// // Gain in dB with undo support
/// GainOptions::from_db(4.5).undo(true).apply(Path::new("song.mp3")).unwrap();
///
/// // Wrapping mode with undo
/// GainOptions::new(5).wrap(true).undo(true).apply(Path::new("song.mp3")).unwrap();
///
/// // Channel-specific gain
/// use mp3rgain::Channel;
/// GainOptions::new(3).channel(Channel::Left).undo(true).apply(Path::new("song.mp3")).unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct GainOptions {
    steps: i32,
    wrap: bool,
    undo: bool,
    channel: Option<Channel>,
    replaygain: Option<ApeReplayGain>,
}

impl GainOptions {
    /// Create gain options from gain steps (1 step = 1.5 dB).
    ///
    /// Positive values increase volume, negative values decrease it.
    pub fn new(steps: i32) -> Self {
        Self {
            steps,
            wrap: false,
            undo: false,
            channel: None,
            replaygain: None,
        }
    }

    /// Create gain options from a dB value (rounded to nearest step).
    pub fn from_db(db: f64) -> Self {
        Self::new(db_to_steps(db))
    }

    /// Use wrapping mode instead of saturating (values wrap around 0-255 range).
    ///
    /// Default is saturating mode (values clamp to 0-255).
    /// Note: wrapping mode is ignored for channel-specific gain (always saturating).
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Store undo information in APEv2 tag for later reversal.
    pub fn undo(mut self, undo: bool) -> Self {
        self.undo = undo;
        self
    }

    /// Apply gain to a specific channel only (Left or Right).
    ///
    /// Returns an error if the file is mono.
    pub fn channel(mut self, channel: Channel) -> Self {
        self.channel = Some(channel);
        self
    }

    /// Fold `REPLAYGAIN_*` items into the same APEv2 tag write as the gain
    /// apply, avoiding a second full-file rewrite (issue #232). Ignored for
    /// channel-specific applies.
    pub(crate) fn replaygain(mut self, rg: ApeReplayGain) -> Self {
        self.replaygain = Some(rg);
        self
    }

    /// Apply the configured gain adjustment to the given MP3 file.
    ///
    /// Returns the number of frames modified.
    pub fn apply(&self, file_path: &Path) -> Result<usize> {
        self.apply_to_path(file_path, file_path)
    }

    /// Apply the configured gain adjustment, reading from `read_from` and writing
    /// to `write_to`. When the two paths are the same, this is equivalent to
    /// [`Self::apply`].
    ///
    /// Used by the `--temp-file` (`-t`) path so that the modified audio is written
    /// directly to the temp file without an intermediate full-file copy of the
    /// original (issue #135).
    pub fn apply_to_path(&self, read_from: &Path, write_to: &Path) -> Result<usize> {
        Ok(self.apply_to_path_with_stats(read_from, write_to)?.frames)
    }

    /// [`apply_to_path`] variant that also reports global_gain saturation
    /// (issue #207). The unified apply pipeline uses this to surface a
    /// "values clamped at 0/255" warning; the public API keeps the bare
    /// frame count.
    pub(crate) fn apply_to_path_with_stats(
        &self,
        read_from: &Path,
        write_to: &Path,
    ) -> Result<SaturationStats> {
        self.apply_to_path_with_stats_preread(read_from, write_to, None)
    }

    /// [`apply_to_path_with_stats`] variant that accepts the file's bytes if
    /// a caller already read them. The apply pipeline's clipping check reads
    /// the file for its headroom scan; handing that buffer through here saves
    /// a second full read of the unchanged file (issue #251). `read_from` is
    /// still used for error reporting and when `preread` is `None`.
    pub(crate) fn apply_to_path_with_stats_preread(
        &self,
        read_from: &Path,
        write_to: &Path,
        preread: Option<Vec<u8>>,
    ) -> Result<SaturationStats> {
        let same_path = read_from == write_to;
        let read = |preread: Option<Vec<u8>>| match preread {
            Some(data) => Ok(data),
            None => fs::read(read_from).map_err(|e| Error::io_read(read_from, e)),
        };

        if self.steps == 0 {
            // No undo/minmax on a zero-step apply (nothing to undo), but the
            // ReplayGain items still need to be recorded when requested.
            if let Some(rg) = &self.replaygain {
                let data = read(preread)?;
                let mut tag = read_ape_tag(&data).unwrap_or_default();
                tag.set_replaygain(rg);
                let new_data = replace_ape_tag(&data, &tag);
                fs::write(write_to, &new_data).map_err(|e| Error::io_write(write_to, e))?;
            } else if !same_path {
                match preread {
                    Some(data) => {
                        fs::write(write_to, &data).map_err(|e| Error::io_write(write_to, e))?;
                    }
                    None => {
                        fs::copy(read_from, write_to).map_err(|e| Error::io_write(write_to, e))?;
                    }
                }
            }
            return Ok(SaturationStats::default());
        }

        let data = read(preread)?;
        if let Some(channel) = self.channel {
            if self.undo {
                apply_gain_channel_with_undo(data, write_to, channel, self.steps)
            } else {
                apply_gain_channel_impl(data, write_to, channel, self.steps)
            }
        } else {
            let mode = if self.wrap {
                GainMode::Wrapping
            } else {
                GainMode::Saturating
            };
            if self.undo {
                apply_gain_with_undo_impl_to_path(
                    data,
                    write_to,
                    self.steps,
                    mode,
                    self.replaygain.as_ref(),
                )
            } else {
                apply_gain_simple_to_path(
                    data,
                    write_to,
                    self.steps,
                    mode,
                    self.replaygain.as_ref(),
                )
            }
        }
    }
}

/// Apply gain adjustment to MP3 file (lossless, saturating mode).
///
/// This is a convenience function equivalent to `GainOptions::new(gain_steps).apply(file_path)`.
///
/// # Arguments
/// * `file_path` - Path to MP3 file
/// * `gain_steps` - Number of 1.5dB steps to apply (positive = louder)
///
/// # Returns
/// * Number of frames modified
pub fn apply_gain(file_path: &Path, gain_steps: i32) -> Result<usize> {
    GainOptions::new(gain_steps).apply(file_path)
}

/// Apply gain adjustment in dB (converted to nearest step).
///
/// This is a convenience function equivalent to `GainOptions::from_db(gain_db).apply(file_path)`.
pub fn apply_gain_db(file_path: &Path, gain_db: f64) -> Result<usize> {
    GainOptions::from_db(gain_db).apply(file_path)
}

/// Apply the inverse of recorded undo deltas (`left`, `right`) to MP3 data.
///
/// Equal deltas take the whole-file path (honoring `wrap`); unequal deltas
/// are undone per channel (channel gain is always saturating).
///
/// `left`/`right` are the stored `MP3GAIN_UNDO` deltas in mp3gain's
/// convention — the gain to *re-add* to restore the original — so they are
/// applied directly (issue #210). This is what makes cross-tool undo work:
/// mp3gain stores `-N` after applying `+N`, and we apply that `-N` as-is.
pub(crate) fn apply_undo_to_data(data: &mut [u8], left: i32, right: i32, wrap: bool) -> usize {
    if left == right {
        let mode = if wrap {
            GainMode::Wrapping
        } else {
            GainMode::Saturating
        };
        apply_gain_to_data(data, left, mode, None).frames
    } else {
        let left_frames = apply_gain_to_data(data, left, GainMode::Saturating, Some(0)).frames;
        let right_frames = apply_gain_to_data(data, right, GainMode::Saturating, Some(1)).frames;
        left_frames.max(right_frames)
    }
}

/// Undo gain changes based on APEv2 tag information
pub fn undo_gain(file_path: &Path) -> Result<usize> {
    let mut data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    let mut tag = read_ape_tag(&data).ok_or(Error::NoApeTag)?;

    let undo_value = tag.get(TAG_MP3GAIN_UNDO).ok_or(Error::NoUndoTag)?;
    let (left, right) = parse_undo_values(Some(undo_value));
    let wrap = parse_undo_wrap(Some(undo_value));

    if left == 0 && right == 0 {
        return Ok(0);
    }

    let frames = apply_undo_to_data(&mut data, left, right, wrap);

    tag.remove(TAG_MP3GAIN_UNDO);
    tag.remove(TAG_MP3GAIN_MINMAX);

    let new_data = replace_ape_tag(&data, &tag);
    crate::apply::atomic_write(file_path, &new_data)?;

    Ok(frames)
}

/// Convert dB gain to MP3 gain steps
pub fn db_to_steps(db: f64) -> i32 {
    (db / GAIN_STEP_DB).round() as i32
}

/// Convert MP3 gain steps to dB
pub fn steps_to_db(steps: i32) -> f64 {
    steps as f64 * GAIN_STEP_DB
}

/// Scale a normalized peak (0.0..=1.0+) to the 16-bit PCM sample range mp3gain
/// uses in its TSV/info output. `peak * 32768.0`.
pub fn peak_to_pcm_sample(peak: f64) -> f64 {
    peak * 32768.0
}

/// Headroom in dB before clipping for a normalized peak. Returns `None` for
/// silent input (peak <= 0) where the dB value would be undefined.
pub fn peak_to_headroom_db(peak: f64) -> Option<f64> {
    if peak > 0.0 {
        Some(-20.0 * peak.log10())
    } else {
        None
    }
}

/// Linear gain ratio for a dB value (`10^(db/20)`).
pub fn db_to_linear(db: f64) -> f64 {
    10.0_f64.powf(db / 20.0)
}

/// Predicted normalized peak after applying `gain_db` to `peak`.
pub fn apply_gain_to_peak(peak: f64, gain_db: f64) -> f64 {
    peak * db_to_linear(gain_db)
}

/// Whether applying `gain_db` to `peak` would clip (post-gain peak > 1.0).
pub fn would_clip(peak: f64, gain_db: f64) -> bool {
    apply_gain_to_peak(peak, gain_db) > 1.0
}

// =============================================================================
// Internal implementation functions
// =============================================================================

/// Simple gain application (no undo tag).
/// Modifies the file's bytes in `data` and writes them to `write_to`.
fn apply_gain_simple_to_path(
    mut data: Vec<u8>,
    write_to: &Path,
    gain_steps: i32,
    mode: GainMode,
    replaygain: Option<&ApeReplayGain>,
) -> Result<SaturationStats> {
    let stats = apply_gain_to_data(&mut data, gain_steps, mode, None);

    if let Some(rg) = replaygain {
        let mut tag = read_ape_tag(&data).unwrap_or_default();
        tag.set_replaygain(rg);
        let new_data = replace_ape_tag(&data, &tag);
        fs::write(write_to, &new_data).map_err(|e| Error::io_write(write_to, e))?;
    } else {
        fs::write(write_to, &data).map_err(|e| Error::io_write(write_to, e))?;
    }

    Ok(stats)
}

/// Apply gain with APEv2 undo tag support (unified for both saturating and wrapping).
///
/// Single-buffer pipeline: one read, all analysis/tag/gain work in memory,
/// one write. The previous version re-read the file for each step (4 reads +
/// 2 writes per apply), which dominated batch throughput.
fn apply_gain_with_undo_impl_to_path(
    mut data: Vec<u8>,
    write_to: &Path,
    gain_steps: i32,
    mode: GainMode,
    replaygain: Option<&ApeReplayGain>,
) -> Result<SaturationStats> {
    let mut tag = read_ape_tag(&data).unwrap_or_default();

    // Accumulate into BOTH channel slots independently: a prior `-l`
    // channel apply may have left them asymmetric, and collapsing them
    // into a single value corrupts the right channel's undo history.
    //
    // MP3GAIN_UNDO stores the *undo* delta (mp3gain convention, issue #210):
    // the value to re-add to restore the original. Applying `+gain_steps`
    // makes the stored undo `-gain_steps`, so it accumulates by subtraction.
    // NOTE: AAC uses the opposite sign (stores the applied gain) — see the
    // convention note on `crate::ape::format_undo_value`.
    let (existing_left, existing_right) = parse_undo_values(tag.get(TAG_MP3GAIN_UNDO));
    let wrap = mode == GainMode::Wrapping;
    tag.set_undo_gain(
        existing_left.saturating_sub(gain_steps),
        existing_right.saturating_sub(gain_steps),
        wrap,
    );

    let stats = apply_gain_to_data(&mut data, gain_steps, mode, None);

    // MP3GAIN_MINMAX records the *post-apply* global_gain range (mp3gain
    // convention). A full apply touches every gain location, so the stats
    // already carry the exact range — no re-scan needed (issue #232). The
    // zero-frame check keeps the old NoMp3Frames validation for non-MP3 input.
    if stats.frames == 0 {
        return Err(Error::NoMp3Frames);
    }
    tag.set_minmax(stats.min_gain, stats.max_gain);

    if let Some(rg) = replaygain {
        tag.set_replaygain(rg);
    }

    let new_data = replace_ape_tag(&data, &tag);
    fs::write(write_to, &new_data).map_err(|e| Error::io_write(write_to, e))?;

    Ok(stats)
}

/// Apply gain to a specific channel (no undo)
fn apply_gain_channel_impl(
    mut data: Vec<u8>,
    write_to: &Path,
    channel: Channel,
    gain_steps: i32,
) -> Result<SaturationStats> {
    let analysis = analyze_data(&data)?;
    if analysis.channel_mode() == ChannelMode::Mono {
        return Err(Error::ChannelGainOnMono);
    }

    let stats = apply_gain_to_data(
        &mut data,
        gain_steps,
        GainMode::Saturating,
        Some(channel.index()),
    );

    fs::write(write_to, &data).map_err(|e| Error::io_write(write_to, e))?;

    Ok(stats)
}

/// Apply channel-specific gain and store undo information in APEv2 tag
fn apply_gain_channel_with_undo(
    mut data: Vec<u8>,
    write_to: &Path,
    channel: Channel,
    gain_steps: i32,
) -> Result<SaturationStats> {
    let analysis = analyze_data(&data)?;
    if analysis.channel_mode() == ChannelMode::Mono {
        return Err(Error::ChannelGainOnMono);
    }

    let mut tag = read_ape_tag(&data).unwrap_or_default();

    let (existing_left, existing_right) = parse_undo_values(tag.get(TAG_MP3GAIN_UNDO));

    // Undo delta accumulates by subtraction (mp3gain convention, issue #210).
    let (new_left, new_right) = match channel {
        Channel::Left => (existing_left - gain_steps, existing_right),
        Channel::Right => (existing_left, existing_right - gain_steps),
    };

    tag.set_undo_gain(new_left, new_right, false);

    let stats = apply_gain_to_data(
        &mut data,
        gain_steps,
        GainMode::Saturating,
        Some(channel.index()),
    );

    // MP3GAIN_MINMAX records the *post-apply* global_gain range (mp3gain
    // convention); re-scan the modified buffer and overwrite any prior value.
    if let Ok((min, max)) = scan_gain_range(&data) {
        tag.set_minmax(min, max);
    }

    let new_data = replace_ape_tag(&data, &tag);
    fs::write(write_to, &new_data).map_err(|e| Error::io_write(write_to, e))?;

    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_db_to_steps() {
        assert_eq!(db_to_steps(0.0), 0);
        assert_eq!(db_to_steps(1.5), 1);
        assert_eq!(db_to_steps(3.0), 2);
        assert_eq!(db_to_steps(-1.5), -1);
        // 2.25 / 1.50515 = 1.495 -> 1 step, matching mp3gain's own rounding
        // (it divides by 5*log10(2), not by 1.5).
        assert_eq!(db_to_steps(2.25), 1);
    }

    #[test]
    fn test_steps_to_db() {
        assert_eq!(steps_to_db(0), 0.0);
        assert!((steps_to_db(1) - 1.505_149_978).abs() < 1e-9);
        assert!((steps_to_db(-2) - -3.010_299_957).abs() < 1e-9);
    }

    /// The exact step constant: 20*log10(2)/4.
    #[test]
    fn gain_step_matches_physical_constant() {
        assert!((GAIN_STEP_DB - 20.0 * 2.0_f64.log10() / 4.0).abs() < 1e-15);
    }

    /// Issue #291: the residual after N steps must be exact, so applying and
    /// re-measuring cannot drift. 7 steps at the old 1.5 constant drifted by
    /// 0.036 dB; with the exact constant the round-trip is lossless.
    #[test]
    fn steps_to_db_roundtrip_has_no_drift() {
        for steps in -15..=15 {
            let db = steps_to_db(steps);
            assert_eq!(db_to_steps(db), steps);
            // physical shift of `steps` global_gain steps
            let physical = 20.0 * (2.0_f64.powf(steps as f64 / 4.0)).log10();
            assert!((db - physical).abs() < 1e-9, "steps={steps}");
        }
    }
}
