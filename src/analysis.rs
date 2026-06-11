use crate::error::{Error, Result};
use crate::frame::{iterate_frames, read_gain_at, scan_gain_range};
use crate::gain::{GAIN_STEP_DB, MAX_GAIN};

use std::fs;
use std::path::Path;

/// Result of MP3 file analysis
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Mp3Analysis {
    frame_count: usize,
    mpeg_version: MpegVersion,
    channel_mode: ChannelMode,
    min_gain: u8,
    max_gain: u8,
    avg_gain: f64,
    headroom_steps: i32,
}

impl Mp3Analysis {
    pub(crate) fn new(
        frame_count: usize,
        mpeg_version: MpegVersion,
        channel_mode: ChannelMode,
        min_gain: u8,
        max_gain: u8,
        avg_gain: f64,
        headroom_steps: i32,
    ) -> Self {
        Self {
            frame_count,
            mpeg_version,
            channel_mode,
            min_gain,
            max_gain,
            avg_gain,
            headroom_steps,
        }
    }

    pub fn frame_count(&self) -> usize {
        self.frame_count
    }
    pub fn mpeg_version(&self) -> MpegVersion {
        self.mpeg_version
    }
    pub fn channel_mode(&self) -> ChannelMode {
        self.channel_mode
    }
    pub fn min_gain(&self) -> u8 {
        self.min_gain
    }
    pub fn max_gain(&self) -> u8 {
        self.max_gain
    }
    pub fn avg_gain(&self) -> f64 {
        self.avg_gain
    }
    pub fn headroom_steps(&self) -> i32 {
        self.headroom_steps
    }
    pub fn headroom_db(&self) -> f64 {
        self.headroom_steps as f64 * GAIN_STEP_DB
    }
}

impl std::fmt::Display for Mp3Analysis {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}, {} frames, headroom: {:+.1} dB",
            self.mpeg_version,
            self.channel_mode,
            self.frame_count,
            self.headroom_db()
        )
    }
}

/// MPEG version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MpegVersion {
    Mpeg1,
    Mpeg2,
    Mpeg25,
}

impl MpegVersion {
    /// Get the string representation of this MPEG version
    pub fn as_str(&self) -> &'static str {
        match self {
            MpegVersion::Mpeg1 => "MPEG1",
            MpegVersion::Mpeg2 => "MPEG2",
            MpegVersion::Mpeg25 => "MPEG2.5",
        }
    }
}

impl std::fmt::Display for MpegVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Channel mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ChannelMode {
    Stereo,
    JointStereo,
    DualChannel,
    Mono,
}

impl ChannelMode {
    /// Get the number of audio channels for this mode
    pub fn channel_count(&self) -> usize {
        match self {
            ChannelMode::Mono => 1,
            _ => 2,
        }
    }

    /// Get the string representation of this channel mode
    pub fn as_str(&self) -> &'static str {
        match self {
            ChannelMode::Stereo => "Stereo",
            ChannelMode::JointStereo => "Joint Stereo",
            ChannelMode::DualChannel => "Dual Channel",
            ChannelMode::Mono => "Mono",
        }
    }
}

impl std::fmt::Display for ChannelMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of maximum amplitude analysis
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MaxAmplitudeResult {
    max_amplitude: f64,
    max_global_gain: u8,
    min_global_gain: u8,
}

impl MaxAmplitudeResult {
    pub(crate) fn new(max_amplitude: f64, max_global_gain: u8, min_global_gain: u8) -> Self {
        Self {
            max_amplitude,
            max_global_gain,
            min_global_gain,
        }
    }

    pub fn max_amplitude(&self) -> f64 {
        self.max_amplitude
    }
    pub fn max_global_gain(&self) -> u8 {
        self.max_global_gain
    }
    pub fn min_global_gain(&self) -> u8 {
        self.min_global_gain
    }
}

impl std::fmt::Display for MaxAmplitudeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "amplitude: {:.6}, gain range: {}-{}",
            self.max_amplitude, self.min_global_gain, self.max_global_gain
        )
    }
}

/// Analyze an MP3 file and return gain statistics
///
/// # Arguments
/// * `file_path` - Path to MP3 file
///
/// # Returns
/// * Analysis results including frame count, gain range, and headroom
pub fn analyze(file_path: &Path) -> Result<Mp3Analysis> {
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    analyze_data(&data)
}

/// Analyze MP3 data already loaded in memory (see [`analyze`]).
pub fn analyze_data(data: &[u8]) -> Result<Mp3Analysis> {
    let mut min_gain = 255u8;
    let mut max_gain = 0u8;
    let mut total_gain: u64 = 0;
    let mut gain_count: u64 = 0;
    let mut first_version = None;
    let mut first_channel_mode = None;

    let frame_count = iterate_frames(data, |_pos, header, locations| {
        if first_version.is_none() {
            first_version = Some(header.version);
            first_channel_mode = Some(header.channel_mode);
        }

        for loc in locations {
            let gain = read_gain_at(data, loc);
            min_gain = min_gain.min(gain);
            max_gain = max_gain.max(gain);
            total_gain += gain as u64;
            gain_count += 1;
        }
    })?;

    if frame_count == 0 {
        return Err(Error::NoMp3Frames);
    }

    let avg_gain = total_gain as f64 / gain_count as f64;
    let headroom_steps = (MAX_GAIN - max_gain) as i32;

    Ok(Mp3Analysis::new(
        frame_count,
        first_version.unwrap(),
        first_channel_mode.unwrap(),
        min_gain,
        max_gain,
        avg_gain,
        headroom_steps,
    ))
}

/// Find maximum amplitude in an MP3 or AAC file.
///
/// MP3 files use [`scan_gain_range`] for the global_gain min/max and
/// `replaygain::find_peak_amplitude` for the decoded peak.
/// AAC files use `aac::analyze_aac_gains` for the gain range and the same
/// `find_peak_amplitude` call for the peak (issue #173: previously the
/// function would `Error::NoMp3Frames` on AAC inputs because the
/// MP3-frame scanner found nothing).
///
/// Without the `replaygain` feature, the peak is estimated from
/// `global_gain` headroom (MP3 only) — AAC inputs return an error in
/// that build.
///
/// Note: The max_amplitude is normalized (0.0 to 1.0+), where values > 1.0 indicate clipping.
/// To get the value in 16-bit PCM scale (like mp3gain), multiply by 32768.
#[cfg(feature = "replaygain")]
pub fn find_max_amplitude(file_path: &Path) -> Result<MaxAmplitudeResult> {
    use crate::replaygain;

    let peak_result = replaygain::find_peak_amplitude(file_path)?;
    let (min_gain, max_gain) = read_gain_range(file_path)?;

    Ok(MaxAmplitudeResult::new(
        peak_result.peak(),
        max_gain,
        min_gain,
    ))
}

/// Find maximum amplitude in an MP3 file (fallback without replaygain feature)
#[cfg(not(feature = "replaygain"))]
pub fn find_max_amplitude(file_path: &Path) -> Result<MaxAmplitudeResult> {
    let (min_gain, max_gain) = read_gain_range(file_path)?;

    let headroom_steps = (MAX_GAIN - max_gain) as i32;
    let headroom_db = headroom_steps as f64 * GAIN_STEP_DB;
    let max_amplitude = crate::gain::db_to_linear(-headroom_db);

    Ok(MaxAmplitudeResult::new(max_amplitude, max_gain, min_gain))
}

/// Read the min/max gain values from a file, dispatching by format.
/// MP3 uses the frame scanner; AAC uses the per-frame `global_gain` scan
/// from [`crate::aac::analyze_aac_gains`].
fn read_gain_range(file_path: &Path) -> Result<(u8, u8)> {
    if crate::mp4meta::is_aac_file(file_path) {
        #[cfg(feature = "aac")]
        {
            let analysis = crate::aac::analyze_aac_gains(file_path)?;
            return Ok((analysis.min_gain(), analysis.max_gain()));
        }
        #[cfg(not(feature = "aac"))]
        {
            return Err(Error::FeatureNotAvailable {
                feature: "AAC support",
                feature_flag: "aac",
            });
        }
    }
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    scan_gain_range(&data)
}

/// Check if an MP3 file is mono
pub fn is_mono(file_path: &Path) -> Result<bool> {
    let analysis = analyze(file_path)?;
    Ok(analysis.channel_mode() == ChannelMode::Mono)
}
