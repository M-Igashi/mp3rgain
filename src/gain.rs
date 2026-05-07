use crate::analysis::{analyze, ChannelMode};
use crate::ape::{
    delete_ape_tag, parse_undo_values, read_ape_tag_from_file, write_ape_tag, ApeTag,
    TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO,
};
use crate::error::{Error, Result};
use crate::frame::{apply_gain_to_channel_data, apply_gain_to_data, GainMode};

use std::fs;
use std::path::Path;

/// MP3 gain step size in dB (fixed by format specification)
pub const GAIN_STEP_DB: f64 = 1.5;

/// Maximum global_gain value
pub const MAX_GAIN: u8 = 255;

/// Minimum global_gain value
pub const MIN_GAIN: u8 = 0;

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

    /// Get the opposite channel
    pub fn other(&self) -> Self {
        match self {
            Channel::Left => Channel::Right,
            Channel::Right => Channel::Left,
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

    /// Apply the configured gain adjustment to the given MP3 file.
    ///
    /// Returns the number of frames modified.
    pub fn apply(&self, file_path: &Path) -> Result<usize> {
        self.apply_to_path(file_path, file_path)
    }

    /// Apply the configured gain adjustment, reading from `read_from` and writing
    /// to `write_to`. When the two paths are the same, this is equivalent to
    /// [`apply`].
    ///
    /// Used by the `--temp-file` (`-t`) path so that the modified audio is written
    /// directly to the temp file without an intermediate full-file copy of the
    /// original (issue #135).
    pub fn apply_to_path(&self, read_from: &Path, write_to: &Path) -> Result<usize> {
        let same_path = read_from == write_to;

        if self.steps == 0 {
            if !same_path {
                fs::copy(read_from, write_to).map_err(|e| Error::io_write(write_to, e))?;
            }
            return Ok(0);
        }

        if let Some(channel) = self.channel {
            if !same_path {
                fs::copy(read_from, write_to).map_err(|e| Error::io_write(write_to, e))?;
            }
            if self.undo {
                apply_gain_channel_with_undo(write_to, channel, self.steps)
            } else {
                apply_gain_channel_impl(write_to, channel, self.steps)
            }
        } else if self.undo {
            let mode = if self.wrap {
                GainMode::Wrapping
            } else {
                GainMode::Saturating
            };
            apply_gain_with_undo_impl_to_path(read_from, write_to, self.steps, mode)
        } else {
            let mode = if self.wrap {
                GainMode::Wrapping
            } else {
                GainMode::Saturating
            };
            apply_gain_simple_to_path(read_from, write_to, self.steps, mode)
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

/// Undo gain changes based on APEv2 tag information
pub fn undo_gain(file_path: &Path) -> Result<usize> {
    let tag = read_ape_tag_from_file(file_path)?.ok_or(Error::NoApeTag)?;

    let undo_gain = tag.get_undo_gain().ok_or(Error::NoUndoTag)?;

    if undo_gain == 0 {
        return Ok(0);
    }

    // Apply inverse gain
    let frames = apply_gain(file_path, -undo_gain)?;

    // Update or remove undo tag
    let mut new_tag = tag.clone();
    new_tag.remove(TAG_MP3GAIN_UNDO);
    new_tag.remove(TAG_MP3GAIN_MINMAX);

    if new_tag.is_empty() {
        delete_ape_tag(file_path)?;
    } else {
        write_ape_tag(file_path, &new_tag)?;
    }

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

// =============================================================================
// Internal implementation functions
// =============================================================================

/// Simple gain application (no undo tag).
/// Reads from `read_from` and writes the modified bytes to `write_to`.
fn apply_gain_simple_to_path(
    read_from: &Path,
    write_to: &Path,
    gain_steps: i32,
    mode: GainMode,
) -> Result<usize> {
    let mut data = fs::read(read_from).map_err(|e| Error::io_read(read_from, e))?;

    let modified_frames = apply_gain_to_data(&mut data, gain_steps, mode);

    fs::write(write_to, &data).map_err(|e| Error::io_write(write_to, e))?;

    Ok(modified_frames)
}

/// Apply gain with APEv2 undo tag support (unified for both saturating and wrapping).
fn apply_gain_with_undo_impl_to_path(
    read_from: &Path,
    write_to: &Path,
    gain_steps: i32,
    mode: GainMode,
) -> Result<usize> {
    let analysis = analyze(read_from)?;

    let mut tag = read_ape_tag_from_file(read_from)?.unwrap_or_else(ApeTag::new);

    let existing_undo = tag.get_undo_gain().unwrap_or(0);
    let new_undo = existing_undo + gain_steps;
    let wrap = mode == GainMode::Wrapping;
    tag.set_undo_gain(new_undo, new_undo, wrap);

    if tag.get(TAG_MP3GAIN_MINMAX).is_none() {
        tag.set_minmax(analysis.min_gain(), analysis.max_gain());
    }

    let frames = apply_gain_simple_to_path(read_from, write_to, gain_steps, mode)?;

    write_ape_tag(write_to, &tag)?;

    Ok(frames)
}

/// Apply gain to a specific channel (no undo)
fn apply_gain_channel_impl(file_path: &Path, channel: Channel, gain_steps: i32) -> Result<usize> {
    let analysis = analyze(file_path)?;
    if analysis.channel_mode() == ChannelMode::Mono {
        return Err(Error::ChannelGainOnMono);
    }

    let mut data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;

    let modified_frames = apply_gain_to_channel_data(&mut data, channel.index(), gain_steps);

    fs::write(file_path, &data).map_err(|e| Error::io_write(file_path, e))?;

    Ok(modified_frames)
}

/// Apply channel-specific gain and store undo information in APEv2 tag
fn apply_gain_channel_with_undo(
    file_path: &Path,
    channel: Channel,
    gain_steps: i32,
) -> Result<usize> {
    let analysis = analyze(file_path)?;
    if analysis.channel_mode() == ChannelMode::Mono {
        return Err(Error::ChannelGainOnMono);
    }

    let mut tag = read_ape_tag_from_file(file_path)?.unwrap_or_else(ApeTag::new);

    let (existing_left, existing_right) = parse_undo_values(tag.get(TAG_MP3GAIN_UNDO));

    let (new_left, new_right) = match channel {
        Channel::Left => (existing_left + gain_steps, existing_right),
        Channel::Right => (existing_left, existing_right + gain_steps),
    };

    tag.set_undo_gain(new_left, new_right, false);

    if tag.get(TAG_MP3GAIN_MINMAX).is_none() {
        tag.set_minmax(analysis.min_gain(), analysis.max_gain());
    }

    let frames = apply_gain_channel_impl(file_path, channel, gain_steps)?;

    write_ape_tag(file_path, &tag)?;

    Ok(frames)
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
        assert_eq!(db_to_steps(2.25), 2);
    }

    #[test]
    fn test_steps_to_db() {
        assert_eq!(steps_to_db(0), 0.0);
        assert_eq!(steps_to_db(1), 1.5);
        assert_eq!(steps_to_db(-2), -3.0);
    }
}
