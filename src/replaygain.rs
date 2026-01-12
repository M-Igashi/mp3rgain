//! ReplayGain analysis module
//!
//! This module implements the ReplayGain 1.0 algorithm for calculating
//! the perceived loudness of audio tracks. The algorithm uses:
//!
//! 1. Equal-loudness filter (ITU-R BS.468 / A-weighting approximation)
//! 2. RMS calculation in 50ms windows
//! 3. 95th percentile statistical analysis
//!
//! Supports both MP3 and AAC/M4A files when compiled with the replaygain feature.
//!
//! Reference: https://wiki.hydrogenaud.io/index.php?title=ReplayGain_specification

#[cfg(feature = "replaygain")]
use anyhow::Context;
use anyhow::Result;
use std::path::Path;

#[cfg(feature = "replaygain")]
use crate::mp4meta;

#[cfg(feature = "replaygain")]
use symphonia::core::audio::{AudioBufferRef, Signal};
#[cfg(feature = "replaygain")]
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
#[cfg(feature = "replaygain")]
use symphonia::core::formats::FormatOptions;
#[cfg(feature = "replaygain")]
use symphonia::core::io::MediaSourceStream;
#[cfg(feature = "replaygain")]
use symphonia::core::meta::MetadataOptions;
#[cfg(feature = "replaygain")]
use symphonia::core::probe::Hint;

/// ReplayGain reference level in dB SPL
/// Original mp3gain uses 89 dB (ReplayGain 1.0)
pub const REPLAYGAIN_REFERENCE_DB: f64 = 89.0;

/// Audio file type
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AudioFileType {
    /// MP3 file
    Mp3,
    /// AAC/M4A file
    Aac,
}

/// Result of ReplayGain analysis for a single track
#[derive(Debug, Clone)]
pub struct ReplayGainResult {
    /// Calculated loudness in dB
    pub loudness_db: f64,
    /// Recommended gain adjustment to reach reference level (in dB)
    pub gain_db: f64,
    /// Peak amplitude (0.0 to 1.0)
    pub peak: f64,
    /// Sample rate of the audio
    pub sample_rate: u32,
    /// File type (MP3 or AAC)
    pub file_type: AudioFileType,
}

impl ReplayGainResult {
    /// Convert gain in dB to MP3 gain steps (1.5 dB per step)
    pub fn gain_steps(&self) -> i32 {
        (self.gain_db / crate::GAIN_STEP_DB).round() as i32
    }
}

/// Result of album gain analysis
#[derive(Debug, Clone)]
pub struct AlbumGainResult {
    /// Individual track results
    pub tracks: Vec<ReplayGainResult>,
    /// Combined album loudness in dB
    pub album_loudness_db: f64,
    /// Recommended album gain adjustment (in dB)
    pub album_gain_db: f64,
    /// Album peak amplitude
    pub album_peak: f64,
}

impl AlbumGainResult {
    /// Convert album gain in dB to MP3 gain steps
    pub fn album_gain_steps(&self) -> i32 {
        (self.album_gain_db / crate::GAIN_STEP_DB).round() as i32
    }
}

// =============================================================================
// Equal-loudness filter coefficients
// =============================================================================

/// Yule-Walker filter coefficients for equal-loudness weighting
/// These are the coefficients used in the original ReplayGain algorithm
/// for 44100 Hz sample rate (will be interpolated for other rates)
#[cfg(feature = "replaygain")]
mod filter_coeffs {
    /// A coefficients for Yule-Walker filter at 44100 Hz
    pub const YULE_A_44100: [f64; 11] = [
        1.0,
        -3.84664617118067,
        7.81501653005538,
        -11.34170355132042,
        13.05504219327545,
        -12.28759895145294,
        9.48293806319790,
        -5.87257861775999,
        2.75465861874613,
        -0.86984376593551,
        0.13919314567432,
    ];

    /// B coefficients for Yule-Walker filter at 44100 Hz
    pub const YULE_B_44100: [f64; 11] = [
        0.05418656406430,
        -0.02911007808948,
        -0.00848709379851,
        -0.00851165645469,
        -0.00834990904936,
        0.02245293253339,
        -0.02596338512915,
        0.01624864962975,
        -0.00240879051584,
        0.00674613682247,
        -0.00187763777362,
    ];

    /// A coefficients for Butter filter (high-pass) at 44100 Hz
    pub const BUTTER_A_44100: [f64; 3] = [1.0, -1.96977855582618, 0.97022847566350];

    /// B coefficients for Butter filter (high-pass) at 44100 Hz
    pub const BUTTER_B_44100: [f64; 3] = [0.98500175787242, -1.97000351574484, 0.98500175787242];

    /// A coefficients for Yule-Walker filter at 48000 Hz
    pub const YULE_A_48000: [f64; 11] = [
        1.0,
        -3.47845948550071,
        6.36317777566148,
        -8.54751527471874,
        9.47693607801280,
        -8.81498681370155,
        6.85401540936998,
        -4.39470996079559,
        2.19611684890774,
        -0.75104302451432,
        0.13149317958808,
    ];

    /// B coefficients for Yule-Walker filter at 48000 Hz
    pub const YULE_B_48000: [f64; 11] = [
        0.03857599435200,
        -0.02160367184185,
        -0.00123395316851,
        -0.00009291677959,
        -0.01655260341619,
        0.02161526843274,
        -0.02074045215285,
        0.00594298065125,
        0.00306428023191,
        0.00012025322027,
        0.00288463683916,
    ];

    /// A coefficients for Butter filter at 48000 Hz
    pub const BUTTER_A_48000: [f64; 3] = [1.0, -1.97223372919527, 0.97261396931306];

    /// B coefficients for Butter filter at 48000 Hz
    pub const BUTTER_B_48000: [f64; 3] = [0.98621192462708, -1.97242384925416, 0.98621192462708];

    /// A coefficients for Yule-Walker filter at 32000 Hz
    pub const YULE_A_32000: [f64; 11] = [
        1.0,
        -2.37898834973084,
        2.84868151156327,
        -2.64577170229825,
        2.23697657451713,
        -1.67148153367602,
        1.00595954808547,
        -0.45953458054983,
        0.16378164858596,
        -0.05032077717131,
        0.02347897407020,
    ];

    /// B coefficients for Yule-Walker filter at 32000 Hz
    pub const YULE_B_32000: [f64; 11] = [
        0.00549836071843,
        -0.00528297328296,
        -0.00426998268581,
        -0.00180414805164,
        -0.00032550931093,
        0.00252831508428,
        -0.00331474531993,
        0.00311096798626,
        -0.00166102790290,
        0.00042903502747,
        0.00023777076452,
    ];

    /// A coefficients for Butter filter at 32000 Hz
    pub const BUTTER_A_32000: [f64; 3] = [1.0, -1.95466019695138, 0.95531569668911];

    /// B coefficients for Butter filter at 32000 Hz
    pub const BUTTER_B_32000: [f64; 3] = [0.97743085512243, -1.95486171024486, 0.97743085512243];
}

/// Equal-loudness filter state
#[cfg(feature = "replaygain")]
struct EqualLoudnessFilter {
    /// Yule-Walker filter A coefficients
    yule_a: [f64; 11],
    /// Yule-Walker filter B coefficients
    yule_b: [f64; 11],
    /// Butter filter A coefficients
    butter_a: [f64; 3],
    /// Butter filter B coefficients
    butter_b: [f64; 3],
    /// Yule filter state (input history)
    yule_x: [f64; 11],
    /// Yule filter state (output history)
    yule_y: [f64; 11],
    /// Butter filter state (input history)
    butter_x: [f64; 3],
    /// Butter filter state (output history)
    butter_y: [f64; 3],
}

#[cfg(feature = "replaygain")]
impl EqualLoudnessFilter {
    fn new(sample_rate: u32) -> Self {
        use filter_coeffs::*;

        let (yule_a, yule_b, butter_a, butter_b) = match sample_rate {
            48000 => (YULE_A_48000, YULE_B_48000, BUTTER_A_48000, BUTTER_B_48000),
            32000 => (YULE_A_32000, YULE_B_32000, BUTTER_A_32000, BUTTER_B_32000),
            _ => (YULE_A_44100, YULE_B_44100, BUTTER_A_44100, BUTTER_B_44100), // Default to 44100
        };

        Self {
            yule_a,
            yule_b,
            butter_a,
            butter_b,
            yule_x: [0.0; 11],
            yule_y: [0.0; 11],
            butter_x: [0.0; 3],
            butter_y: [0.0; 3],
        }
    }

    fn process(&mut self, sample: f64) -> f64 {
        // Apply Yule-Walker filter
        // Shift history
        for i in (1..11).rev() {
            self.yule_x[i] = self.yule_x[i - 1];
            self.yule_y[i] = self.yule_y[i - 1];
        }
        self.yule_x[0] = sample;

        let mut yule_out = self.yule_b[0] * self.yule_x[0];
        for i in 1..11 {
            yule_out += self.yule_b[i] * self.yule_x[i] - self.yule_a[i] * self.yule_y[i];
        }
        self.yule_y[0] = yule_out;

        // Apply Butterworth high-pass filter
        // Shift history
        for i in (1..3).rev() {
            self.butter_x[i] = self.butter_x[i - 1];
            self.butter_y[i] = self.butter_y[i - 1];
        }
        self.butter_x[0] = yule_out;

        let mut butter_out = self.butter_b[0] * self.butter_x[0];
        for i in 1..3 {
            butter_out += self.butter_b[i] * self.butter_x[i] - self.butter_a[i] * self.butter_y[i];
        }
        self.butter_y[0] = butter_out;

        butter_out
    }
}

// =============================================================================
// RMS and loudness calculation
// =============================================================================

/// Calculate RMS values for all 50ms windows
#[cfg(feature = "replaygain")]
fn calculate_rms_windows(samples: &[f64], sample_rate: u32) -> Vec<f64> {
    // Calculate window size based on sample rate (50ms)
    let window_size = (sample_rate as usize * 50) / 1000;
    if window_size == 0 || samples.len() < window_size {
        return Vec::new();
    }

    let num_windows = samples.len() / window_size;
    let mut rms_values = Vec::with_capacity(num_windows);

    for i in 0..num_windows {
        let start = i * window_size;
        let end = start + window_size;

        let sum_squares: f64 = samples[start..end].iter().map(|s| s * s).sum();
        let rms = (sum_squares / window_size as f64).sqrt();
        rms_values.push(rms);
    }

    rms_values
}

/// Calculate loudness from RMS values using 95th percentile
#[cfg(feature = "replaygain")]
fn calculate_loudness(rms_values: &[f64]) -> f64 {
    if rms_values.is_empty() {
        return -70.0; // Very quiet
    }

    // Filter out very quiet windows (below -70 dB)
    let min_rms = 10.0_f64.powf(-70.0 / 20.0);
    let mut filtered: Vec<f64> = rms_values
        .iter()
        .filter(|&&v| v > min_rms)
        .copied()
        .collect();

    if filtered.is_empty() {
        return -70.0;
    }

    // Sort for percentile calculation
    filtered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Get 95th percentile
    let idx = ((filtered.len() as f64 * 0.95) as usize).saturating_sub(1);
    let percentile_rms = filtered[idx.min(filtered.len() - 1)];

    // Convert to dB
    if percentile_rms > 0.0 {
        20.0 * percentile_rms.log10()
    } else {
        -70.0
    }
}

// =============================================================================
// Main analysis functions
// =============================================================================

/// Detect file type from path
#[cfg(feature = "replaygain")]
fn detect_file_type(file_path: &Path) -> AudioFileType {
    if mp4meta::is_mp4_file(file_path) {
        AudioFileType::Aac
    } else {
        AudioFileType::Mp3
    }
}

/// Analyze a single track and calculate ReplayGain
#[cfg(feature = "replaygain")]
pub fn analyze_track(file_path: &Path) -> Result<ReplayGainResult> {
    analyze_track_with_index(file_path, None)
}

/// Analyze a single track with optional track index selection
#[cfg(feature = "replaygain")]
pub fn analyze_track_with_index(
    file_path: &Path,
    track_index: Option<u32>,
) -> Result<ReplayGainResult> {
    // Detect file type
    let file_type = detect_file_type(file_path);

    // Open the media source
    let file = std::fs::File::open(file_path)
        .with_context(|| format!("Failed to open: {}", file_path.display()))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Probe the format
    let mut hint = Hint::new();
    if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .with_context(|| format!("Failed to probe format: {}", file_path.display()))?;

    let mut format = probed.format;

    // Find audio tracks
    let audio_tracks: Vec<_> = format
        .tracks()
        .iter()
        .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .collect();

    if audio_tracks.is_empty() {
        anyhow::bail!("No audio track found");
    }

    // Select track by index or default to first
    let track = match track_index {
        Some(idx) => {
            let idx = idx as usize;
            if idx >= audio_tracks.len() {
                anyhow::bail!(
                    "Track index {} out of range (file has {} audio track(s))",
                    idx,
                    audio_tracks.len()
                );
            }
            audio_tracks[idx]
        }
        None => audio_tracks[0],
    };

    let track_id = track.id;
    let sample_rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| anyhow::anyhow!("Unknown sample rate"))?;
    let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);

    // Create decoder
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .with_context(|| "Failed to create decoder")?;

    // Create filter for each channel
    let mut filters: Vec<EqualLoudnessFilter> = (0..channels)
        .map(|_| EqualLoudnessFilter::new(sample_rate))
        .collect();

    let mut all_filtered_samples: Vec<f64> = Vec::new();
    let mut peak: f64 = 0.0;

    // Process all packets
    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(e) => return Err(e.into()),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        };

        // Process audio buffer
        process_audio_buffer(&decoded, &mut filters, &mut all_filtered_samples, &mut peak);
    }

    // Calculate RMS windows and loudness
    let rms_values = calculate_rms_windows(&all_filtered_samples, sample_rate);
    let loudness_db = calculate_loudness(&rms_values);

    // Calculate gain needed to reach reference level
    let gain_db = REPLAYGAIN_REFERENCE_DB + loudness_db; // loudness_db is negative

    Ok(ReplayGainResult {
        loudness_db,
        gain_db,
        peak,
        sample_rate,
        file_type,
    })
}

/// Process an audio buffer and extract filtered samples
#[cfg(feature = "replaygain")]
fn process_audio_buffer(
    buffer: &AudioBufferRef,
    filters: &mut [EqualLoudnessFilter],
    all_samples: &mut Vec<f64>,
    peak: &mut f64,
) {
    match buffer {
        AudioBufferRef::F32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();

            for frame in 0..frames {
                let mut sum = 0.0;
                for ch in 0..channels {
                    let sample = buf.chan(ch)[frame] as f64;
                    *peak = peak.max(sample.abs());

                    // Apply equal-loudness filter
                    let filtered = if ch < filters.len() {
                        filters[ch].process(sample)
                    } else {
                        sample
                    };
                    sum += filtered * filtered;
                }
                // Store combined RMS contribution for this frame
                all_samples.push((sum / channels as f64).sqrt());
            }
        }
        AudioBufferRef::S16(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let scale = 1.0 / 32768.0;

            for frame in 0..frames {
                let mut sum = 0.0;
                for ch in 0..channels {
                    let sample = buf.chan(ch)[frame] as f64 * scale;
                    *peak = peak.max(sample.abs());

                    let filtered = if ch < filters.len() {
                        filters[ch].process(sample)
                    } else {
                        sample
                    };
                    sum += filtered * filtered;
                }
                all_samples.push((sum / channels as f64).sqrt());
            }
        }
        AudioBufferRef::S32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let scale = 1.0 / 2147483648.0;

            for frame in 0..frames {
                let mut sum = 0.0;
                for ch in 0..channels {
                    let sample = buf.chan(ch)[frame] as f64 * scale;
                    *peak = peak.max(sample.abs());

                    let filtered = if ch < filters.len() {
                        filters[ch].process(sample)
                    } else {
                        sample
                    };
                    sum += filtered * filtered;
                }
                all_samples.push((sum / channels as f64).sqrt());
            }
        }
        _ => {
            // Unsupported format, skip
        }
    }
}

/// Analyze multiple tracks for album gain
#[cfg(feature = "replaygain")]
pub fn analyze_album(files: &[&Path]) -> Result<AlbumGainResult> {
    analyze_album_with_index(files, None)
}

/// Analyze multiple tracks for album gain with optional track index selection
#[cfg(feature = "replaygain")]
pub fn analyze_album_with_index(
    files: &[&Path],
    track_index: Option<u32>,
) -> Result<AlbumGainResult> {
    let mut track_results = Vec::with_capacity(files.len());
    let mut album_peak: f64 = 0.0;

    for file in files {
        // Analyze each track
        let result = analyze_track_with_index(file, track_index)?;
        album_peak = album_peak.max(result.peak);

        // We need to re-analyze to get raw RMS values for album calculation
        // This is a simplified approach - a more efficient implementation would
        // cache the RMS values during track analysis
        track_results.push(result);
    }

    // For album gain, we combine all tracks' loudness measurements
    // The proper way is to combine all RMS windows, but for simplicity
    // we use a weighted average based on track loudness
    let total_linear: f64 = track_results
        .iter()
        .map(|r| 10.0_f64.powf(r.loudness_db / 10.0))
        .sum();
    let album_loudness_db = 10.0 * (total_linear / track_results.len() as f64).log10();
    let album_gain_db = REPLAYGAIN_REFERENCE_DB + album_loudness_db;

    Ok(AlbumGainResult {
        tracks: track_results,
        album_loudness_db,
        album_gain_db,
        album_peak,
    })
}

// =============================================================================
// Stub implementations when feature is disabled
// =============================================================================

#[cfg(not(feature = "replaygain"))]
pub fn analyze_track(_file_path: &Path) -> Result<ReplayGainResult> {
    anyhow::bail!(
        "ReplayGain analysis requires the 'replaygain' feature.\n\
        Install with: cargo install mp3rgain --features replaygain"
    )
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_track_with_index(
    _file_path: &Path,
    _track_index: Option<u32>,
) -> Result<ReplayGainResult> {
    anyhow::bail!(
        "ReplayGain analysis requires the 'replaygain' feature.\n\
        Install with: cargo install mp3rgain --features replaygain"
    )
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album(_files: &[&Path]) -> Result<AlbumGainResult> {
    anyhow::bail!(
        "ReplayGain analysis requires the 'replaygain' feature.\n\
        Install with: cargo install mp3rgain --features replaygain"
    )
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album_with_index(
    _files: &[&Path],
    _track_index: Option<u32>,
) -> Result<AlbumGainResult> {
    anyhow::bail!(
        "ReplayGain analysis requires the 'replaygain' feature.\n\
        Install with: cargo install mp3rgain --features replaygain"
    )
}

/// Check if ReplayGain feature is available
pub fn is_available() -> bool {
    cfg!(feature = "replaygain")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replaygain_availability() {
        // This test just verifies the stub functions compile
        let available = is_available();
        #[cfg(feature = "replaygain")]
        assert!(available);
        #[cfg(not(feature = "replaygain"))]
        assert!(!available);
    }

    #[cfg(feature = "replaygain")]
    #[test]
    fn test_filter_creation() {
        let filter = EqualLoudnessFilter::new(44100);
        assert_eq!(filter.yule_a.len(), 11);
        assert_eq!(filter.butter_a.len(), 3);
    }

    #[cfg(feature = "replaygain")]
    #[test]
    fn test_rms_calculation() {
        // Create a simple sine wave at 1kHz
        let sample_rate = 44100;
        let duration_samples = sample_rate; // 1 second
        let frequency = 1000.0;
        let amplitude = 0.5;

        let samples: Vec<f64> = (0..duration_samples)
            .map(|i| {
                let t = i as f64 / sample_rate as f64;
                amplitude * (2.0 * std::f64::consts::PI * frequency * t).sin()
            })
            .collect();

        let rms_values = calculate_rms_windows(&samples, sample_rate as u32);
        assert!(!rms_values.is_empty());

        // RMS of a sine wave should be amplitude / sqrt(2)
        let expected_rms = amplitude / std::f64::consts::SQRT_2;
        for rms in &rms_values {
            assert!(
                (*rms - expected_rms).abs() < 0.01,
                "RMS {} differs from expected {}",
                rms,
                expected_rms
            );
        }
    }

    #[cfg(feature = "replaygain")]
    #[test]
    fn test_loudness_calculation() {
        // Test with known RMS values
        let rms_values: Vec<f64> = vec![0.1, 0.1, 0.1, 0.1, 0.1];
        let loudness = calculate_loudness(&rms_values);
        // 0.1 in dB is 20 * log10(0.1) = -20 dB
        assert!((loudness - (-20.0)).abs() < 0.1);
    }
}
