//! ITU-R BS.1770-4 integrated loudness measurement (K-weighting + gating).
//!
//! This is the measurement engine behind the opt-in ReplayGain 2.0 (`--rg2`)
//! and EBU R128 (`--r128`) analysis modes (issues #269 / #270). It runs
//! alongside — never instead of — the ReplayGain 1.0 analyzer in
//! [`crate::replaygain`], which stays the default for mp3gain compatibility.
//!
//! The pipeline per BS.1770-4:
//!
//! 1. K-weighting: a high-shelf ("head") filter cascaded with an RLB
//!    high-pass, per channel. The spec tabulates coefficients only for
//!    48 kHz; they are derived analytically here for any sample rate.
//! 2. Gating blocks: 400 ms windows with 75% overlap (100 ms step).
//!    Block energy is the channel-weighted mean square of the filtered
//!    signal (weights: 1.0 for L/R/C, 1.41 for surround, 0 for LFE).
//! 3. Gating: blocks below -70 LUFS are dropped (absolute gate); the mean
//!    of the survivors minus 10 LU forms the relative gate; integrated
//!    loudness is the mean energy of blocks passing both gates.
//!
//! Input samples are normalized floats (digital full scale = 1.0) — unlike
//! the RG1 analyzer, which expects 16-bit-scaled samples, because LUFS is
//! defined relative to full scale.

/// The K-weighting cascade has ~+0.691 dB of gain at 997 Hz; BS.1770 cancels
/// it with this constant so a 997 Hz reference tone reads its dBFS level.
const LOUDNESS_OFFSET: f64 = -0.691;

/// Absolute gating threshold from BS.1770-4.
const ABSOLUTE_GATE_LUFS: f64 = -70.0;

/// Relative gate sits this many LU below the mean of the absolutely-gated
/// blocks; in energy terms a factor of 10^(-10/10) = 0.1.
const RELATIVE_GATE_FACTOR: f64 = 0.1;

fn energy_to_loudness(energy: f64) -> f64 {
    LOUDNESS_OFFSET + 10.0 * energy.log10()
}

fn loudness_to_energy(lufs: f64) -> f64 {
    10f64.powf((lufs - LOUDNESS_OFFSET) / 10.0)
}

/// Second-order IIR section, direct form II transposed.
#[derive(Clone)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(coeffs: (f64, f64, f64, f64, f64)) -> Self {
        let (b0, b1, b2, a1, a2) = coeffs;
        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Stage-1 shelving filter coefficients `(b0, b1, b2, a1, a2)`.
///
/// Analytic derivation of the BS.1770 pre-filter for an arbitrary sample
/// rate (same parameterization as libebur128, after Brecht De Man,
/// "Evaluation of Implementations of the EBU R128 Loudness Measurement").
/// At 48 kHz this reproduces the coefficient table in BS.1770-4.
fn shelf_coefficients(sample_rate: f64) -> (f64, f64, f64, f64, f64) {
    let f0 = 1681.974450955533;
    let gain_db = 3.999843853973347;
    let q = 0.7071752369554196;

    let k = (std::f64::consts::PI * f0 / sample_rate).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;

    (
        (vh + vb * k / q + k * k) / a0,
        2.0 * (k * k - vh) / a0,
        (vh - vb * k / q + k * k) / a0,
        2.0 * (k * k - 1.0) / a0,
        (1.0 - k / q + k * k) / a0,
    )
}

/// Stage-2 RLB high-pass filter coefficients `(b0, b1, b2, a1, a2)`.
fn highpass_coefficients(sample_rate: f64) -> (f64, f64, f64, f64, f64) {
    let f0 = 38.13547087602444;
    let q = 0.5003270373238773;

    let k = (std::f64::consts::PI * f0 / sample_rate).tan();
    let a0 = 1.0 + k / q + k * k;

    (
        1.0,
        -2.0,
        1.0,
        2.0 * (k * k - 1.0) / a0,
        (1.0 - k / q + k * k) / a0,
    )
}

/// K-weighting filter for one channel: shelf then RLB high-pass.
#[derive(Clone)]
struct KWeightingFilter {
    shelf: Biquad,
    highpass: Biquad,
}

impl KWeightingFilter {
    fn new(sample_rate: u32) -> Self {
        let fs = sample_rate as f64;
        Self {
            shelf: Biquad::new(shelf_coefficients(fs)),
            highpass: Biquad::new(highpass_coefficients(fs)),
        }
    }

    #[inline]
    fn process(&mut self, sample: f64) -> f64 {
        self.highpass.process(self.shelf.process(sample))
    }
}

/// BS.1770 channel weights by channel count, assuming the usual ordering
/// (L R [C] [LFE] Ls Rs). Mono and stereo — the only layouts MP3 and almost
/// all AAC files use — are weight 1.0; the LFE of a 5.1 layout is excluded.
/// Unknown layouts fall back to 1.0 everywhere.
fn channel_weights(channels: usize) -> Vec<f64> {
    match channels {
        4 => vec![1.0, 1.0, 1.41, 1.41],
        5 => vec![1.0, 1.0, 1.0, 1.41, 1.41],
        6 => vec![1.0, 1.0, 1.0, 0.0, 1.41, 1.41],
        n => vec![1.0; n],
    }
}

/// Channel-weighted mean-square energies of the 400 ms gating blocks of one
/// or more tracks. Album loudness is measured over the concatenation of the
/// album's tracks, so per-track block lists are kept and merged with
/// [`BlockEnergies::accumulate`] before gating — mirroring how the RG1 path
/// merges per-track histograms.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BlockEnergies {
    energies: Vec<f64>,
}

impl BlockEnergies {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of gating blocks measured.
    pub fn len(&self) -> usize {
        self.energies.len()
    }

    pub fn is_empty(&self) -> bool {
        self.energies.is_empty()
    }

    /// Append another track's blocks (album accumulation).
    pub fn accumulate(&mut self, other: &BlockEnergies) {
        self.energies.extend_from_slice(&other.energies);
    }

    /// Integrated loudness in LUFS after absolute and relative gating.
    ///
    /// Returns `f64::NEG_INFINITY` when no block survives the gates
    /// (silence or near-silence) — callers should treat that as "no
    /// measurable loudness" rather than a huge gain.
    pub fn integrated_lufs(&self) -> f64 {
        let absolute_gate = loudness_to_energy(ABSOLUTE_GATE_LUFS);

        let mut sum = 0.0;
        let mut count = 0usize;
        for &e in &self.energies {
            if e > absolute_gate {
                sum += e;
                count += 1;
            }
        }
        if count == 0 {
            return f64::NEG_INFINITY;
        }

        let relative_gate = (sum / count as f64) * RELATIVE_GATE_FACTOR;
        let gate = absolute_gate.max(relative_gate);

        let mut sum = 0.0;
        let mut count = 0usize;
        for &e in &self.energies {
            if e > gate {
                sum += e;
                count += 1;
            }
        }
        if count == 0 {
            return f64::NEG_INFINITY;
        }

        energy_to_loudness(sum / count as f64)
    }
}

/// Streaming BS.1770 analyzer for one track.
///
/// Feed decoded frames with [`add_frame`](Self::add_frame), then take the
/// gating blocks with [`into_blocks`](Self::into_blocks). A trailing partial
/// block is discarded, as the spec measures only complete 400 ms blocks.
pub struct Bs1770Analyzer {
    filters: Vec<KWeightingFilter>,
    weights: Vec<f64>,
    /// Samples per 100 ms sub-block (rounded for rates not divisible by 10,
    /// e.g. 11025 Hz).
    subblock_len: usize,
    /// Weighted sum of squares accumulating in the current sub-block.
    subblock_sum: f64,
    subblock_samples: usize,
    /// Sums of the last up-to-3 completed sub-blocks, oldest first; a gating
    /// block is these plus the sub-block that just completed (75% overlap).
    recent: [f64; 3],
    recent_len: usize,
    blocks: BlockEnergies,
}

impl Bs1770Analyzer {
    /// `channels` beyond the first are analyzed with BS.1770 channel weights;
    /// see [`channel_weights`] for the layout assumption.
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let channels = channels.max(1);
        Self {
            filters: vec![KWeightingFilter::new(sample_rate); channels],
            weights: channel_weights(channels),
            subblock_len: (sample_rate as usize + 5) / 10,
            subblock_sum: 0.0,
            subblock_samples: 0,
            recent: [0.0; 3],
            recent_len: 0,
            blocks: BlockEnergies::new(),
        }
    }

    /// Add one frame of normalized samples (full scale = 1.0), one per
    /// channel. Extra samples beyond the configured channel count are
    /// ignored; missing ones count as silence.
    #[inline]
    pub fn add_frame(&mut self, frame: &[f64]) {
        let mut acc = 0.0;
        for ((filter, &weight), &sample) in self.filters.iter_mut().zip(&self.weights).zip(frame) {
            let y = filter.process(sample);
            acc += weight * y * y;
        }
        self.subblock_sum += acc;
        self.subblock_samples += 1;
        if self.subblock_samples >= self.subblock_len {
            self.finish_subblock();
        }
    }

    fn finish_subblock(&mut self) {
        let sum = self.subblock_sum;
        if self.recent_len == 3 {
            let block_sum = self.recent.iter().sum::<f64>() + sum;
            self.blocks
                .energies
                .push(block_sum / (4 * self.subblock_len) as f64);
            self.recent.rotate_left(1);
            self.recent[2] = sum;
        } else {
            self.recent[self.recent_len] = sum;
            self.recent_len += 1;
        }
        self.subblock_sum = 0.0;
        self.subblock_samples = 0;
    }

    /// Finish analysis, returning the gating blocks for this track.
    pub fn into_blocks(self) -> BlockEnergies {
        self.blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// EBU Tech 3341 allows a +/-0.1 LU deviation on the compliance cases.
    const TOLERANCE: f64 = 0.1;

    #[test]
    fn coefficients_match_bs1770_reference_at_48khz() {
        // Coefficient tables from ITU-R BS.1770-4, Tables 1 and 2.
        let (b0, b1, b2, a1, a2) = shelf_coefficients(48000.0);
        assert!((b0 - 1.53512485958697).abs() < 1e-6);
        assert!((b1 - -2.69169618940638).abs() < 1e-6);
        assert!((b2 - 1.19839281085285).abs() < 1e-6);
        assert!((a1 - -1.69065929318241).abs() < 1e-6);
        assert!((a2 - 0.73248077421585).abs() < 1e-6);

        let (b0, b1, b2, a1, a2) = highpass_coefficients(48000.0);
        assert_eq!(b0, 1.0);
        assert_eq!(b1, -2.0);
        assert_eq!(b2, 1.0);
        assert!((a1 - -1.99004745483398).abs() < 1e-5);
        assert!((a2 - 0.99007225036621).abs() < 1e-5);
    }

    fn append_sine(samples: &mut Vec<f64>, level_dbfs: f64, seconds: f64, sample_rate: u32) {
        let amplitude = 10f64.powf(level_dbfs / 20.0);
        let count = (seconds * sample_rate as f64) as usize;
        let step = 2.0 * std::f64::consts::PI * 997.0 / sample_rate as f64;
        for n in 0..count {
            samples.push(amplitude * (step * n as f64).sin());
        }
    }

    fn integrated_stereo(samples: &[f64], sample_rate: u32) -> f64 {
        let mut analyzer = Bs1770Analyzer::new(sample_rate, 2);
        for &s in samples {
            analyzer.add_frame(&[s, s]);
        }
        analyzer.into_blocks().integrated_lufs()
    }

    /// EBU Tech 3341 case 1: 997 Hz stereo sine at -23 dBFS reads -23 LUFS.
    #[test]
    fn tech3341_case1_minus23_sine() {
        for &rate in &[48000u32, 44100] {
            let mut samples = Vec::new();
            append_sine(&mut samples, -23.0, 20.0, rate);
            let lufs = integrated_stereo(&samples, rate);
            assert!(
                (lufs - -23.0).abs() < TOLERANCE,
                "expected -23 LUFS at {} Hz, got {:.3}",
                rate,
                lufs
            );
        }
    }

    /// EBU Tech 3341 case 2: same tone at -33 dBFS reads -33 LUFS.
    #[test]
    fn tech3341_case2_minus33_sine() {
        let mut samples = Vec::new();
        append_sine(&mut samples, -33.0, 20.0, 48000);
        let lufs = integrated_stereo(&samples, 48000);
        assert!((lufs - -33.0).abs() < TOLERANCE, "got {:.3}", lufs);
    }

    /// Relative gate (Tech 3341 case 3 shape): quiet -36 dBFS lead-in/out
    /// around a -23 dBFS body must be gated out, reading -23 LUFS overall.
    #[test]
    fn relative_gate_excludes_quiet_passages() {
        let mut samples = Vec::new();
        append_sine(&mut samples, -36.0, 2.5, 48000);
        append_sine(&mut samples, -23.0, 15.0, 48000);
        append_sine(&mut samples, -36.0, 2.5, 48000);
        let lufs = integrated_stereo(&samples, 48000);
        assert!((lufs - -23.0).abs() < TOLERANCE, "got {:.3}", lufs);
    }

    /// Absolute gate: silence around the tone must not drag loudness down.
    #[test]
    fn absolute_gate_excludes_silence() {
        let mut samples = vec![0.0; 5 * 48000];
        append_sine(&mut samples, -23.0, 20.0, 48000);
        samples.extend(std::iter::repeat(0.0).take(5 * 48000));
        let lufs = integrated_stereo(&samples, 48000);
        assert!((lufs - -23.0).abs() < TOLERANCE, "got {:.3}", lufs);
    }

    /// A tone in only one channel carries half the energy: -3.01 LU lower.
    #[test]
    fn single_channel_reads_3db_below_stereo() {
        let mut samples = Vec::new();
        append_sine(&mut samples, -23.0, 20.0, 48000);
        let mut analyzer = Bs1770Analyzer::new(48000, 2);
        for &s in &samples {
            analyzer.add_frame(&[s, 0.0]);
        }
        let lufs = analyzer.into_blocks().integrated_lufs();
        assert!((lufs - -26.01).abs() < TOLERANCE, "got {:.3}", lufs);
    }

    #[test]
    fn silence_has_no_measurable_loudness() {
        let samples = vec![0.0; 10 * 48000];
        let lufs = integrated_stereo(&samples, 48000);
        assert!(lufs.is_infinite() && lufs < 0.0);
    }

    /// Album accumulation equals measuring the concatenated tracks: two
    /// equal-length tracks at -23 and -33 dBFS average (in energy) to
    /// -23 + 10*log10((1 + 0.1) / 2) ~= -25.6 LUFS, with both tracks
    /// above the relative gate.
    #[test]
    fn accumulate_merges_tracks_like_concatenation() {
        let mut a = Vec::new();
        append_sine(&mut a, -23.0, 20.0, 48000);
        let mut b = Vec::new();
        append_sine(&mut b, -33.0, 20.0, 48000);

        let mut analyzer_a = Bs1770Analyzer::new(48000, 2);
        for &s in &a {
            analyzer_a.add_frame(&[s, s]);
        }
        let mut analyzer_b = Bs1770Analyzer::new(48000, 2);
        for &s in &b {
            analyzer_b.add_frame(&[s, s]);
        }

        let mut album = analyzer_a.into_blocks();
        album.accumulate(&analyzer_b.into_blocks());

        let expected = -23.0 + 10.0 * (1.1f64 / 2.0).log10();
        let lufs = album.integrated_lufs();
        assert!(
            (lufs - expected).abs() < TOLERANCE,
            "expected {:.3}, got {:.3}",
            expected,
            lufs
        );
    }

    /// 11025 Hz is not divisible by 10; the rounded 100 ms sub-block must
    /// still produce a sane measurement (997 Hz is well below Nyquist).
    #[test]
    fn odd_sample_rate_11025() {
        let mut samples = Vec::new();
        append_sine(&mut samples, -23.0, 20.0, 11025);
        let lufs = integrated_stereo(&samples, 11025);
        assert!((lufs - -23.0).abs() < 0.2, "got {:.3}", lufs);
    }
}
