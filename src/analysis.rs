use crate::error::{Error, Result};
use crate::frame::{first_frame_header, iterate_frames, read_gain_at, scan_gain_range, skip_id3v2};
use crate::gain::{steps_to_db, MAX_GAIN};

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
        steps_to_db(self.headroom_steps)
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
/// The file is read from disk once: the same buffer feeds the gain-range
/// scan and the decoder (previously each did its own full read).
///
/// Note: The max_amplitude is normalized (0.0 to 1.0+), where values > 1.0 indicate clipping.
/// To get the value in 16-bit PCM scale (like mp3gain), multiply by 32768.
#[cfg(feature = "replaygain")]
pub fn find_max_amplitude(file_path: &Path) -> Result<MaxAmplitudeResult> {
    use crate::replaygain;

    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    let (min_gain, max_gain) = gain_range_of(&data)?;
    let peak_result = replaygain::find_peak_amplitude_in_data(file_path, data)?;

    Ok(MaxAmplitudeResult::new(
        peak_result.peak(),
        max_gain,
        min_gain,
    ))
}

/// Find maximum amplitude in an MP3 file (fallback without replaygain feature)
#[cfg(not(feature = "replaygain"))]
pub fn find_max_amplitude(file_path: &Path) -> Result<MaxAmplitudeResult> {
    let data = fs::read(file_path).map_err(|e| Error::io_read(file_path, e))?;
    let (min_gain, max_gain) = gain_range_of(&data)?;

    let headroom_steps = (MAX_GAIN - max_gain) as i32;
    let headroom_db = steps_to_db(headroom_steps);
    let max_amplitude = crate::gain::db_to_linear(-headroom_db);

    Ok(MaxAmplitudeResult::new(max_amplitude, max_gain, min_gain))
}

/// Min/max `global_gain` of a file already in memory, dispatching by
/// container. MP3 uses the frame scanner; AAC uses the per-frame scan from
/// [`crate::aac::analyze_aac_gains`].
fn gain_range_of(data: &[u8]) -> Result<(u8, u8)> {
    if crate::mp4meta::is_aac_data(data) {
        #[cfg(feature = "aac")]
        {
            let analysis = crate::aac::analyze_aac_gains_from_data(data)?;
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
    scan_gain_range(data)
}

/// Channel mode of the first MP3 frame, reading only the head of the file.
///
/// [`analyze`] walks every frame to compute gain statistics, but a mono
/// check or a Joint Stereo warning needs one header. The ID3v2 tag is
/// skipped by its declared size (cover art can run to megabytes) and a
/// 64 KiB window read after it, which holds the first frame plus the sync
/// check on the one following. The rare stream that starts later than that
/// falls back to a full read.
pub fn read_channel_mode(file_path: &Path) -> Result<ChannelMode> {
    use std::io::{Read, Seek, SeekFrom};
    const WINDOW: usize = 64 * 1024;

    let read_err = |e| Error::io_read(file_path, e);
    let mut file = fs::File::open(file_path).map_err(|e| Error::io_open(file_path, e))?;
    let mut head = [0u8; 10];
    let n = file.read(&mut head).map_err(read_err)?;
    let start = skip_id3v2(&head[..n]) as u64;
    file.seek(SeekFrom::Start(start)).map_err(read_err)?;

    let mut window = vec![0u8; WINDOW];
    let mut filled = 0;
    while filled < window.len() {
        let n = file.read(&mut window[filled..]).map_err(read_err)?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    window.truncate(filled);
    if let Some(header) = first_frame_header(&window) {
        return Ok(header.channel_mode);
    }

    let data = fs::read(file_path).map_err(read_err)?;
    first_frame_header(&data)
        .map(|h| h.channel_mode)
        .ok_or(Error::NoMp3Frames)
}

/// Check if an MP3 file is mono
pub fn is_mono(file_path: &Path) -> Result<bool> {
    Ok(read_channel_mode(file_path)? == ChannelMode::Mono)
}
