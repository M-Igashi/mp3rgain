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

use crate::error::{Error, Result};
use std::path::Path;

#[cfg(feature = "replaygain")]
use crate::mp4meta;
#[cfg(feature = "replaygain")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "replaygain")]
use std::sync::Arc;

#[cfg(feature = "replaygain")]
use symphonia::core::audio::{Audio, GenericAudioBufferRef};
#[cfg(feature = "replaygain")]
use symphonia::core::codecs::audio::{AudioDecoderOptions, CODEC_ID_NULL_AUDIO};
#[cfg(feature = "replaygain")]
use symphonia::core::formats::probe::Hint;
#[cfg(feature = "replaygain")]
use symphonia::core::formats::FormatOptions;
#[cfg(feature = "replaygain")]
use symphonia::core::io::{MediaSource, MediaSourceStream};
#[cfg(feature = "replaygain")]
use symphonia::core::meta::MetadataOptions;

/// ReplayGain reference level in dB SPL
/// Original mp3gain uses 89 dB (ReplayGain 1.0)
pub const REPLAYGAIN_REFERENCE_DB: f64 = 89.0;

/// Pink noise reference calibration constant
/// This is the loudness value produced by the ReplayGain algorithm when analyzing
/// the standard -14 dB FS pink noise reference signal. All loudness measurements
/// are compared against this reference to calculate the required gain adjustment.
/// Source: https://replaygain.hydrogenaud.io/calibration.html
const PINK_REF: f64 = 64.82;

/// Audio file type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum AudioFileType {
    /// MP3 file
    Mp3,
    /// AAC/M4A file
    Aac,
}

impl std::fmt::Display for AudioFileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioFileType::Mp3 => f.write_str("MP3"),
            AudioFileType::Aac => f.write_str("AAC"),
        }
    }
}

/// Result of ReplayGain analysis for a single track
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReplayGainResult {
    loudness_db: f64,
    gain_db: f64,
    peak: f64,
    sample_rate: u32,
    file_type: AudioFileType,
}

impl ReplayGainResult {
    #[allow(dead_code)]
    pub(crate) fn new(
        loudness_db: f64,
        gain_db: f64,
        peak: f64,
        sample_rate: u32,
        file_type: AudioFileType,
    ) -> Self {
        Self {
            loudness_db,
            gain_db,
            peak,
            sample_rate,
            file_type,
        }
    }

    pub fn loudness_db(&self) -> f64 {
        self.loudness_db
    }
    pub fn gain_db(&self) -> f64 {
        self.gain_db
    }
    pub fn peak(&self) -> f64 {
        self.peak
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
    pub fn file_type(&self) -> AudioFileType {
        self.file_type
    }

    /// Convert gain in dB to MP3 gain steps (1.5 dB per step)
    pub fn gain_steps(&self) -> i32 {
        (self.gain_db / crate::GAIN_STEP_DB).round() as i32
    }
}

impl std::fmt::Display for ReplayGainResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:+.2} dB (peak: {:.6})", self.gain_db, self.peak)
    }
}

/// Result of album gain analysis
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AlbumGainResult {
    tracks: Vec<ReplayGainResult>,
    album_loudness_db: f64,
    album_gain_db: f64,
    album_peak: f64,
}

impl AlbumGainResult {
    #[allow(dead_code)]
    pub(crate) fn new(
        tracks: Vec<ReplayGainResult>,
        album_loudness_db: f64,
        album_gain_db: f64,
        album_peak: f64,
    ) -> Self {
        Self {
            tracks,
            album_loudness_db,
            album_gain_db,
            album_peak,
        }
    }

    pub fn tracks(&self) -> &[ReplayGainResult] {
        &self.tracks
    }
    pub fn album_loudness_db(&self) -> f64 {
        self.album_loudness_db
    }
    pub fn album_gain_db(&self) -> f64 {
        self.album_gain_db
    }
    pub fn album_peak(&self) -> f64 {
        self.album_peak
    }

    /// Convert album gain in dB to MP3 gain steps
    pub fn album_gain_steps(&self) -> i32 {
        (self.album_gain_db / crate::GAIN_STEP_DB).round() as i32
    }
}

impl std::fmt::Display for AlbumGainResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Album: {:+.2} dB (peak: {:.6}, {} tracks)",
            self.album_gain_db,
            self.album_peak,
            self.tracks.len()
        )
    }
}

/// Report from a "lenient" album analysis that may skip files.
///
/// Returned by `analyze_album_lenient_*` family. `album` is computed from the
/// successfully-analyzed tracks only; `failures` lists `(file_index,
/// error_message)` pairs in input order; `successful_indices` maps
/// `album.tracks()[k]` back to `files[successful_indices[k]]`.
#[derive(Debug, Clone)]
pub struct AlbumAnalysisReport {
    pub album: AlbumGainResult,
    pub failures: Vec<(usize, String)>,
    pub successful_indices: Vec<usize>,
}

// =============================================================================
// Equal-loudness filter coefficients
// =============================================================================

/// Yule-Walker and Butterworth filter coefficients for equal-loudness weighting
/// These are the coefficients used in the original ReplayGain algorithm
/// Supporting all 12 sample rates from the original mp3gain
/// Reference: https://github.com/cpuimage/ReplayGainAnalysis/blob/master/gain_analysis.c
#[cfg(feature = "replaygain")]
mod filter_coeffs {
    // =========================================================================
    // 96000 Hz coefficients (ABYule[0], ABButter[0])
    // =========================================================================
    pub(super) const YULE_A_96000: [f64; 11] = [
        1.0,
        -7.22103125152679,
        24.7034187975904,
        -52.6825833623896,
        77.4825736677539,
        -82.0074753444205,
        63.1566097101925,
        -34.889569769245,
        13.2126852760198,
        -3.09445623301669,
        0.340344741393305,
    ];

    pub(super) const YULE_B_96000: [f64; 11] = [
        0.006471345933032,
        -0.02567678242161,
        0.049805860704367,
        -0.05823001743528,
        0.040611847441914,
        -0.010912036887501,
        -0.00901635868667,
        0.012448886238123,
        -0.007206683749426,
        0.002167156433951,
        -0.000261819276949,
    ];

    pub(super) const BUTTER_A_96000: [f64; 3] = [1.0, -1.98611621154089, 0.986211929160751];

    pub(super) const BUTTER_B_96000: [f64; 3] =
        [0.99308203517541, -1.98616407035082, 0.99308203517541];

    // =========================================================================
    // 88200 Hz coefficients (ABYule[1], ABButter[1])
    // =========================================================================
    pub(super) const YULE_A_88200: [f64; 11] = [
        1.0,
        -7.19001570087017,
        24.4109412087159,
        -51.6306373580801,
        75.3978476863163,
        -79.4164552507386,
        61.0373661948115,
        -33.7446462547014,
        12.8168791146274,
        -3.01332198541437,
        0.223619893831468,
    ];

    pub(super) const YULE_B_88200: [f64; 11] = [
        0.015415414474287,
        -0.07691359399407,
        0.196677418516518,
        -0.338855114128061,
        0.430094579594561,
        -0.415015413747894,
        0.304942508151101,
        -0.166191795926663,
        0.063198189938739,
        -0.015003978694525,
        0.001748085184539,
    ];

    pub(super) const BUTTER_A_88200: [f64; 3] = [1.0, -1.98488843762334, 0.979389350028798];

    pub(super) const BUTTER_B_88200: [f64; 3] =
        [0.992472550461293, -1.98494510092258, 0.992472550461293];

    // =========================================================================
    // 64000 Hz coefficients (ABYule[2], ABButter[2])
    // =========================================================================
    pub(super) const YULE_A_64000: [f64; 11] = [
        1.0,
        -5.74819833657784,
        16.246507961894,
        -29.9691822642542,
        40.027597579378,
        -40.3209196052655,
        30.8542077487718,
        -17.5965138737281,
        7.10690214103873,
        -1.82175564515191,
        0.223619893831468,
    ];

    pub(super) const YULE_B_64000: [f64; 11] = [
        0.021776466467053,
        -0.062376961003801,
        0.107731165328514,
        -0.150994515142316,
        0.170334807313632,
        -0.157984942890531,
        0.121639833268721,
        -0.074094040816409,
        0.031282852041061,
        -0.00755421235941,
        0.00117925454213,
    ];

    pub(super) const BUTTER_A_64000: [f64; 3] = [1.0, -1.97917472731008, 0.979389350028798];

    pub(super) const BUTTER_B_64000: [f64; 3] =
        [0.989641019334721, -1.97928203866944, 0.989641019334721];

    // =========================================================================
    // 48000 Hz coefficients (ABYule[3], ABButter[3])
    // =========================================================================
    pub(super) const YULE_A_48000: [f64; 11] = [
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

    pub(super) const YULE_B_48000: [f64; 11] = [
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

    pub(super) const BUTTER_A_48000: [f64; 3] = [1.0, -1.97223372919527, 0.97261396931306];

    pub(super) const BUTTER_B_48000: [f64; 3] =
        [0.98621192462708, -1.97242384925416, 0.98621192462708];

    // =========================================================================
    // 44100 Hz coefficients (ABYule[4], ABButter[4])
    // =========================================================================
    pub(super) const YULE_A_44100: [f64; 11] = [
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

    pub(super) const YULE_B_44100: [f64; 11] = [
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

    pub(super) const BUTTER_A_44100: [f64; 3] = [1.0, -1.96977855582618, 0.97022847566350];

    pub(super) const BUTTER_B_44100: [f64; 3] =
        [0.98500175787242, -1.97000351574484, 0.98500175787242];

    // =========================================================================
    // 32000 Hz coefficients (ABYule[5], ABButter[5])
    // =========================================================================
    pub(super) const YULE_A_32000: [f64; 11] = [
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

    pub(super) const YULE_B_32000: [f64; 11] = [
        0.15457299681924,
        -0.09331049056315,
        -0.06247880153653,
        0.02163541888798,
        -0.05588393329856,
        0.04781476674921,
        0.00222312597743,
        0.03174092540049,
        -0.01390589421898,
        0.00651420667831,
        -0.00881362733839,
    ];

    pub(super) const BUTTER_A_32000: [f64; 3] = [1.0, -1.95835380975398, 0.95920349965459];

    pub(super) const BUTTER_B_32000: [f64; 3] =
        [0.97938932735214, -1.95877865470428, 0.97938932735214];

    // =========================================================================
    // 24000 Hz coefficients (ABYule[6], ABButter[6])
    // =========================================================================
    pub(super) const YULE_A_24000: [f64; 11] = [
        1.0,
        -1.61273165137247,
        1.07977492259970,
        -0.25656257754070,
        -0.16276719120440,
        -0.22638893773906,
        0.39120800788284,
        -0.22138138954925,
        0.04500235387352,
        0.02005851806501,
        0.00302439095741,
    ];

    pub(super) const YULE_B_24000: [f64; 11] = [
        0.30296907319327,
        -0.22613988682123,
        -0.08587323730772,
        0.03282930172664,
        -0.00915702933434,
        -0.02364141202522,
        -0.00584456039913,
        0.06276101321749,
        -0.00000828086748,
        0.00205861885564,
        -0.02950134983287,
    ];

    pub(super) const BUTTER_A_24000: [f64; 3] = [1.0, -1.95002759149878, 0.95124613669835];

    pub(super) const BUTTER_B_24000: [f64; 3] =
        [0.97531843204928, -1.95063686409857, 0.97531843204928];

    // =========================================================================
    // 22050 Hz coefficients (ABYule[7], ABButter[7])
    // =========================================================================
    pub(super) const YULE_A_22050: [f64; 11] = [
        1.0,
        -1.49858979367799,
        0.87350271418188,
        0.12205022308084,
        -0.80774944671438,
        0.47854794562326,
        -0.12453458140019,
        -0.04067510197014,
        0.08333755284107,
        -0.04237348025746,
        0.02977207319925,
    ];

    pub(super) const YULE_B_22050: [f64; 11] = [
        0.33642304856132,
        -0.25572241425570,
        -0.11828570177555,
        0.11921148675203,
        -0.07834489609479,
        -0.00469977914380,
        -0.00589500224440,
        0.05724228140351,
        0.00832043980773,
        -0.01635381384540,
        -0.01760176568150,
    ];

    pub(super) const BUTTER_A_22050: [f64; 3] = [1.0, -1.94561023566527, 0.94705070426118];

    pub(super) const BUTTER_B_22050: [f64; 3] =
        [0.97316523498161, -1.94633046996323, 0.97316523498161];

    // =========================================================================
    // 16000 Hz coefficients (ABYule[8], ABButter[8])
    // =========================================================================
    pub(super) const YULE_A_16000: [f64; 11] = [
        1.0,
        -0.62820619233671,
        0.29661783706366,
        -0.37256372942400,
        0.00213767857124,
        -0.42029820170918,
        0.22199650564824,
        0.00613424350682,
        0.06747620744683,
        0.05784820375801,
        0.03222754072173,
    ];

    pub(super) const YULE_B_16000: [f64; 11] = [
        0.44915256608450,
        -0.14351757464547,
        -0.22784394429749,
        -0.01419140100551,
        0.04078262797139,
        -0.12398163381748,
        0.04078565135648,
        0.10478503600251,
        -0.01863887810927,
        -0.03193428438915,
        0.00541907748707,
    ];

    pub(super) const BUTTER_A_16000: [f64; 3] = [1.0, -1.92783286977036, 0.93034775234268];

    pub(super) const BUTTER_B_16000: [f64; 3] =
        [0.96454515552826, -1.92909031105652, 0.96454515552826];

    // =========================================================================
    // 12000 Hz coefficients (ABYule[9], ABButter[9])
    // =========================================================================
    pub(super) const YULE_A_12000: [f64; 11] = [
        1.0,
        -1.04800335126349,
        0.29156311971249,
        -0.26806001042947,
        0.00819999645858,
        0.45054734505008,
        -0.33032403314006,
        0.06739368333110,
        -0.04784254229033,
        0.01639907836189,
        0.01807364323573,
    ];

    pub(super) const YULE_B_12000: [f64; 11] = [
        0.56619470757641,
        -0.75464456939302,
        0.16242137742230,
        0.16744243493672,
        -0.18901604199609,
        0.30931782841830,
        -0.27562961986224,
        0.00647310677246,
        0.08647503780351,
        -0.03788984554840,
        -0.00588215443421,
    ];

    pub(super) const BUTTER_A_12000: [f64; 3] = [1.0, -1.91858953033784, 0.92177618768381];

    pub(super) const BUTTER_B_12000: [f64; 3] =
        [0.96009142950541, -1.92018285901082, 0.96009142950541];

    // =========================================================================
    // 11025 Hz coefficients (ABYule[10], ABButter[10])
    // =========================================================================
    pub(super) const YULE_A_11025: [f64; 11] = [
        1.0,
        -0.51035327095184,
        -0.31863563325245,
        -0.20256413484477,
        0.14728154134330,
        0.38952639978999,
        -0.23313271880868,
        -0.05246019024463,
        -0.02505961724053,
        0.02442357316099,
        0.01818801111503,
    ];

    pub(super) const YULE_B_11025: [f64; 11] = [
        0.58100494960553,
        -0.53174909058578,
        -0.14289799034253,
        0.17520704835522,
        0.02377945217615,
        0.15558449135573,
        -0.25344790059353,
        0.01628462406333,
        0.06920467763959,
        -0.03721611395801,
        -0.00749618797172,
    ];

    pub(super) const BUTTER_A_11025: [f64; 3] = [1.0, -1.91542108074780, 0.91885558323625];

    pub(super) const BUTTER_B_11025: [f64; 3] =
        [0.95856916599601, -1.91713833199203, 0.95856916599601];

    // =========================================================================
    // 8000 Hz coefficients (ABYule[11], ABButter[11])
    // =========================================================================
    pub(super) const YULE_A_8000: [f64; 11] = [
        1.0,
        -0.25049871956020,
        -0.43193942311114,
        -0.03424681017675,
        -0.04678328784242,
        0.26408300200955,
        0.15113130533216,
        -0.17556493366449,
        -0.18823009262115,
        0.05477720428674,
        0.04704409688120,
    ];

    pub(super) const YULE_B_8000: [f64; 11] = [
        0.53648789255105,
        -0.42163034350696,
        -0.00275953611929,
        0.04267842219415,
        -0.10214864179676,
        0.14590772289388,
        -0.02459864859345,
        -0.11202315195388,
        -0.04060034127000,
        0.04788665548180,
        -0.02217936801134,
    ];

    pub(super) const BUTTER_A_8000: [f64; 3] = [1.0, -1.88903307939452, 0.89487434461664];

    pub(super) const BUTTER_B_8000: [f64; 3] =
        [0.94597685600279, -1.89195371200558, 0.94597685600279];
}

/// Small constant to prevent denormal float slowdowns
/// Reference: gain_analysis.c filterYule() uses 1e-10 for this purpose
const DENORMAL_PREVENTION: f64 = 1e-10;

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
    fn new(sample_rate: u32) -> Option<Self> {
        use filter_coeffs::*;

        let (yule_a, yule_b, butter_a, butter_b) = match sample_rate {
            96000 => (YULE_A_96000, YULE_B_96000, BUTTER_A_96000, BUTTER_B_96000),
            88200 => (YULE_A_88200, YULE_B_88200, BUTTER_A_88200, BUTTER_B_88200),
            64000 => (YULE_A_64000, YULE_B_64000, BUTTER_A_64000, BUTTER_B_64000),
            48000 => (YULE_A_48000, YULE_B_48000, BUTTER_A_48000, BUTTER_B_48000),
            44100 => (YULE_A_44100, YULE_B_44100, BUTTER_A_44100, BUTTER_B_44100),
            32000 => (YULE_A_32000, YULE_B_32000, BUTTER_A_32000, BUTTER_B_32000),
            24000 => (YULE_A_24000, YULE_B_24000, BUTTER_A_24000, BUTTER_B_24000),
            22050 => (YULE_A_22050, YULE_B_22050, BUTTER_A_22050, BUTTER_B_22050),
            16000 => (YULE_A_16000, YULE_B_16000, BUTTER_A_16000, BUTTER_B_16000),
            12000 => (YULE_A_12000, YULE_B_12000, BUTTER_A_12000, BUTTER_B_12000),
            11025 => (YULE_A_11025, YULE_B_11025, BUTTER_A_11025, BUTTER_B_11025),
            8000 => (YULE_A_8000, YULE_B_8000, BUTTER_A_8000, BUTTER_B_8000),
            _ => return None, // Unsupported sample rate
        };

        Some(Self {
            yule_a,
            yule_b,
            butter_a,
            butter_b,
            yule_x: [0.0; 11],
            yule_y: [0.0; 11],
            butter_x: [0.0; 3],
            butter_y: [0.0; 3],
        })
    }

    fn process(&mut self, sample: f64) -> f64 {
        // Shift Yule-Walker filter history and insert new sample
        self.yule_x.copy_within(0..10, 1);
        self.yule_y.copy_within(0..10, 1);
        self.yule_x[0] = sample;

        // Apply Yule-Walker filter with denormal prevention.
        // The 1e-10 constant prevents denormal float slowdowns on silent audio
        // (see gain_analysis.c filterYule()). The explicit loop with a fixed
        // upper bound gives the optimizer a better shot at unrolling /
        // vectorizing this hot path than the iterator chain.
        let mut yule_out = DENORMAL_PREVENTION + self.yule_b[0] * self.yule_x[0];
        for i in 1..11 {
            yule_out += self.yule_b[i] * self.yule_x[i] - self.yule_a[i] * self.yule_y[i];
        }
        self.yule_y[0] = yule_out;

        // Shift Butterworth filter history and insert Yule output
        self.butter_x.copy_within(0..2, 1);
        self.butter_y.copy_within(0..2, 1);
        self.butter_x[0] = yule_out;

        // Apply Butterworth high-pass filter with denormal prevention
        let mut butter_out = DENORMAL_PREVENTION + self.butter_b[0] * self.butter_x[0];
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

/// Steps per dB for histogram resolution (matches original mp3gain)
const STEPS_PER_DB: f64 = 100.0;

/// Maximum histogram size
/// For 16-bit samples: mean_square ranges from ~0 to ~1B (32768²)
/// 10*log10(1B) ≈ 90 dB, so we need coverage from about 0 to 100 dB
/// Size = 100 dB * 100 steps/dB = 10000, plus margin = 12000
const HISTOGRAM_SIZE: usize = 12000;

/// Histogram offset to map dB values to array indices
/// For 16-bit samples, typical values are 40-90 dB (10*log10 of mean_square)
/// Offset of 2000 allows coverage from -20 dB to +100 dB
const HISTOGRAM_OFFSET: i32 = 2000;

/// RMS percentile for loudness calculation (95th percentile)
const RMS_PERCENTILE: f64 = 0.95;

/// Histogram data for ReplayGain analysis
/// This can be accumulated across multiple tracks for album gain calculation
#[cfg(feature = "replaygain")]
#[derive(Clone)]
struct LoudnessHistogram {
    /// Histogram of loudness values (RMS windows bucketed by dB)
    data: Vec<u32>,
}

#[cfg(feature = "replaygain")]
impl LoudnessHistogram {
    fn new() -> Self {
        Self {
            data: vec![0; HISTOGRAM_SIZE],
        }
    }

    /// Accumulate another histogram into this one (for album gain calculation)
    fn accumulate(&mut self, other: &LoudnessHistogram) {
        for (a, &b) in self.data.iter_mut().zip(other.data.iter()) {
            *a += b;
        }
    }

    /// Calculate loudness from histogram using 95th percentile
    fn get_loudness(&self) -> f64 {
        let total: u64 = self.data.iter().map(|&x| x as u64).sum();
        if total == 0 {
            return -20.0; // Default for empty histogram
        }

        let threshold = ((total as f64) * (1.0 - RMS_PERCENTILE)).ceil() as u64;
        let mut count = 0u64;

        for i in (0..HISTOGRAM_SIZE).rev() {
            count += self.data[i] as u64;
            if count >= threshold {
                return (i as i32 - HISTOGRAM_OFFSET) as f64 / STEPS_PER_DB;
            }
        }

        -20.0 // Default for lowest values
    }
}

/// Analyzer state for accumulating samples across buffers
#[cfg(feature = "replaygain")]
struct ReplayGainAnalyzer {
    /// Left channel sum of squares for current window
    lsum: f64,
    /// Right channel sum of squares for current window
    rsum: f64,
    /// Number of samples in current window
    totsamp: usize,
    /// Window size in samples (50ms worth)
    window_samples: usize,
    /// Histogram of loudness values
    histogram: LoudnessHistogram,
}

#[cfg(feature = "replaygain")]
impl ReplayGainAnalyzer {
    fn new(sample_rate: u32) -> Self {
        // 50ms window
        let window_samples = (sample_rate as usize * 50) / 1000;
        Self {
            lsum: 0.0,
            rsum: 0.0,
            totsamp: 0,
            window_samples,
            histogram: LoudnessHistogram::new(),
        }
    }

    /// Take ownership of the histogram, consuming the analyzer.
    fn into_histogram(self) -> LoudnessHistogram {
        self.histogram
    }

    /// Add a stereo sample pair (already filtered)
    fn add_sample(&mut self, left: f64, right: f64) {
        self.lsum += left * left;
        self.rsum += right * right;
        self.totsamp += 1;

        if self.totsamp >= self.window_samples {
            self.finish_window();
        }
    }

    /// Add a mono sample (already filtered)
    fn add_mono_sample(&mut self, sample: f64) {
        let sq = sample * sample;
        self.lsum += sq;
        self.rsum += sq;
        self.totsamp += 1;

        if self.totsamp >= self.window_samples {
            self.finish_window();
        }
    }

    /// Finish the current window and add to histogram
    fn finish_window(&mut self) {
        if self.totsamp == 0 {
            return;
        }

        // Calculate mean square value (average of both channels)
        // Original: (lsum + rsum) / totsamp * 0.5
        let mean_square = (self.lsum + self.rsum) / self.totsamp as f64 * 0.5;

        // Convert to histogram index
        // Original: STEPS_per_dB * 10.0 * log10(mean_square + 1e-37)
        let val = STEPS_PER_DB * 10.0 * (mean_square + 1e-37).log10();
        let idx = (val as i32 + HISTOGRAM_OFFSET) as usize;

        if idx < HISTOGRAM_SIZE {
            self.histogram.data[idx] += 1;
        }

        // Reset for next window
        self.lsum = 0.0;
        self.rsum = 0.0;
        self.totsamp = 0;
    }

    /// Calculate the loudness value from the histogram (95th percentile)
    fn get_loudness(&self) -> f64 {
        self.histogram.get_loudness()
    }
}

// =============================================================================
// Main analysis functions
// =============================================================================

/// Detect file type from path
#[cfg(feature = "replaygain")]
fn detect_file_type(file_path: &Path) -> AudioFileType {
    if mp4meta::is_aac_file(file_path) {
        AudioFileType::Aac
    } else {
        AudioFileType::Mp3
    }
}

// =============================================================================
// Progress-tracking media source
// =============================================================================

/// Media source wrapper that tracks read position for progress reporting
#[cfg(feature = "replaygain")]
struct ProgressMediaSource {
    inner: std::fs::File,
    position: Arc<AtomicU64>,
    total_size: u64,
}

#[cfg(feature = "replaygain")]
impl std::io::Read for ProgressMediaSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.position.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

#[cfg(feature = "replaygain")]
impl std::io::Seek for ProgressMediaSource {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        let new_pos = self.inner.seek(pos)?;
        self.position.store(new_pos, Ordering::Relaxed);
        Ok(new_pos)
    }
}

#[cfg(feature = "replaygain")]
impl MediaSource for ProgressMediaSource {
    fn is_seekable(&self) -> bool {
        true
    }

    fn byte_len(&self) -> Option<u64> {
        Some(self.total_size)
    }
}

/// Internal result containing both ReplayGainResult and histogram for album calculation
#[cfg(feature = "replaygain")]
struct TrackAnalysisInternal {
    result: ReplayGainResult,
    histogram: LoudnessHistogram,
}

/// Internal function to analyze a track and return both result and histogram
#[cfg(feature = "replaygain")]
fn analyze_track_internal(
    file_path: &Path,
    track_index: Option<u32>,
    progress: Option<&dyn Fn(u64, u64)>,
) -> Result<TrackAnalysisInternal> {
    // Detect file type
    let file_type = detect_file_type(file_path);

    // Open the media source
    let file = std::fs::File::open(file_path).map_err(|e| Error::io_open(file_path, e))?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);

    // Create media source with optional position tracking
    let position_tracker = progress.map(|_| Arc::new(AtomicU64::new(0)));

    let mss = if let Some(ref tracker) = position_tracker {
        let source = ProgressMediaSource {
            inner: file,
            position: Arc::clone(tracker),
            total_size: file_size,
        };
        MediaSourceStream::new(Box::new(source), Default::default())
    } else {
        MediaSourceStream::new(Box::new(file), Default::default())
    };

    // Probe the format
    let mut hint = Hint::new();
    if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| Error::ProbeFailed {
            path: file_path.to_path_buf(),
            source: Box::new(e),
        })?;

    // Find audio tracks
    let audio_tracks: Vec<_> = format
        .tracks()
        .iter()
        .filter(|t| {
            t.codec_params
                .as_ref()
                .and_then(|p| p.audio())
                .is_some_and(|a| a.codec != CODEC_ID_NULL_AUDIO)
        })
        .collect();

    if audio_tracks.is_empty() {
        return Err(Error::NoAudioTrack);
    }

    // Select track by index or default to first
    let track = match track_index {
        Some(idx) => {
            let idx = idx as usize;
            if idx >= audio_tracks.len() {
                return Err(Error::TrackIndexOutOfRange {
                    index: idx as u32,
                    count: audio_tracks.len(),
                });
            }
            audio_tracks[idx]
        }
        None => audio_tracks[0],
    };

    let track_id = track.id;
    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or(Error::NoAudioTrack)?;
    let sample_rate = audio_params
        .sample_rate
        .ok_or(Error::UnsupportedSampleRate(0))?;
    let channels = audio_params
        .channels
        .as_ref()
        .map(|c| c.count())
        .unwrap_or(2);

    // Create decoder
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(audio_params, &AudioDecoderOptions::default())
        .map_err(|e| Error::Decode(Box::new(e)))?;

    // Create filter for each channel
    let mut filters: Vec<EqualLoudnessFilter> = (0..channels)
        .map(|_| {
            EqualLoudnessFilter::new(sample_rate).ok_or(Error::UnsupportedSampleRate(sample_rate))
        })
        .collect::<Result<Vec<_>>>()?;

    let mut analyzer = ReplayGainAnalyzer::new(sample_rate);
    let mut peak: f64 = 0.0;

    // Process all packets
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => return Err(Error::Decode(Box::new(e))),
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(Error::Decode(Box::new(e))),
        };

        // Process audio buffer
        process_audio_buffer(&decoded, &mut filters, &mut analyzer, &mut peak);

        // Report progress
        if let (Some(cb), Some(ref tracker)) = (progress, &position_tracker) {
            cb(tracker.load(Ordering::Relaxed), file_size);
        }
    }

    // Report completion
    if let Some(cb) = progress {
        cb(file_size, file_size);
    }

    // Finish any remaining samples in the last window
    analyzer.finish_window();

    // Calculate loudness and gain
    let loudness_db = analyzer.get_loudness();
    let gain_db = PINK_REF - loudness_db;

    let result = ReplayGainResult::new(loudness_db, gain_db, peak, sample_rate, file_type);

    Ok(TrackAnalysisInternal {
        result,
        histogram: analyzer.into_histogram(),
    })
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
    let internal = analyze_track_internal(file_path, track_index, None)?;
    Ok(internal.result)
}

/// Analyze a single track with progress reporting
///
/// The callback receives `(bytes_read, total_bytes)` and is called after each
/// decoded packet. Use this to drive a progress bar during analysis.
///
/// Originally requested by @Sappharad in #106 (mp3gain-style byte progress).
#[cfg(feature = "replaygain")]
pub fn analyze_track_with_progress(
    file_path: &Path,
    track_index: Option<u32>,
    on_progress: &dyn Fn(u64, u64),
) -> Result<ReplayGainResult> {
    let internal = analyze_track_internal(file_path, track_index, Some(on_progress))?;
    Ok(internal.result)
}

/// Scale factor to convert normalized float samples to 16-bit integer range.
/// The original ReplayGain algorithm (and its PINK_REF calibration constant of 64.82)
/// was designed for non-normalized 16-bit integer samples (-32768 to 32767).
/// Symphonia decoders output normalized float samples (-1.0 to 1.0), so we must
/// scale them to match the original algorithm's expected input range.
/// Without this scaling, gain values are off by 20 * log10(32768) ≈ 90.31 dB.
const SAMPLE_SCALE_16BIT: f64 = 32768.0;

/// Process an audio buffer and feed filtered samples to the analyzer
#[cfg(feature = "replaygain")]
fn process_audio_buffer(
    buffer: &GenericAudioBufferRef,
    filters: &mut [EqualLoudnessFilter],
    analyzer: &mut ReplayGainAnalyzer,
    peak: &mut f64,
) {
    // Hoist `plane()` lookups outside the per-sample loop — calling
    // `buf.plane(N).unwrap()` per frame goes through symphonia's Option
    // unwrap on every sample (millions per file).
    match buffer {
        GenericAudioBufferRef::F32(buf) => {
            let channels = buf.num_planes();
            let frames = buf.frames();
            let left_plane = buf.plane(0).unwrap();
            let right_plane = (channels >= 2).then(|| buf.plane(1).unwrap());

            for frame in 0..frames {
                let left_norm = left_plane[frame] as f64;
                *peak = peak.max(left_norm.abs());
                let left_filtered = filters[0].process(left_norm * SAMPLE_SCALE_16BIT);

                if let Some(right_plane) = right_plane {
                    let right_norm = right_plane[frame] as f64;
                    *peak = peak.max(right_norm.abs());
                    let right_filtered = filters[1].process(right_norm * SAMPLE_SCALE_16BIT);
                    analyzer.add_sample(left_filtered, right_filtered);
                } else {
                    analyzer.add_mono_sample(left_filtered);
                }
            }
        }
        GenericAudioBufferRef::S16(buf) => {
            let channels = buf.num_planes();
            let frames = buf.frames();
            let left_plane = buf.plane(0).unwrap();
            let right_plane = (channels >= 2).then(|| buf.plane(1).unwrap());

            for frame in 0..frames {
                // S16 samples are already in the correct range for ReplayGain algorithm
                let left = left_plane[frame] as f64;
                *peak = peak.max((left / SAMPLE_SCALE_16BIT).abs());
                let left_filtered = filters[0].process(left);

                if let Some(right_plane) = right_plane {
                    let right = right_plane[frame] as f64;
                    *peak = peak.max((right / SAMPLE_SCALE_16BIT).abs());
                    let right_filtered = filters[1].process(right);
                    analyzer.add_sample(left_filtered, right_filtered);
                } else {
                    analyzer.add_mono_sample(left_filtered);
                }
            }
        }
        GenericAudioBufferRef::S32(buf) => {
            let channels = buf.num_planes();
            let frames = buf.frames();
            // Scale S32 to 16-bit range: divide by 2^16 to go from 32-bit to 16-bit range
            let scale = SAMPLE_SCALE_16BIT / 2147483648.0;
            let left_plane = buf.plane(0).unwrap();
            let right_plane = (channels >= 2).then(|| buf.plane(1).unwrap());

            for frame in 0..frames {
                let left = left_plane[frame] as f64 * scale;
                *peak = peak.max((left / SAMPLE_SCALE_16BIT).abs());
                let left_filtered = filters[0].process(left);

                if let Some(right_plane) = right_plane {
                    let right = right_plane[frame] as f64 * scale;
                    *peak = peak.max((right / SAMPLE_SCALE_16BIT).abs());
                    let right_filtered = filters[1].process(right);
                    analyzer.add_sample(left_filtered, right_filtered);
                } else {
                    analyzer.add_mono_sample(left_filtered);
                }
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
///
/// This implements the same algorithm as the original mp3gain:
/// - Accumulate all 50ms RMS window values from all tracks into a single histogram
/// - Calculate album loudness from the combined histogram using 95th percentile
/// - This properly weights each track by its duration (more windows = more influence)
#[cfg(feature = "replaygain")]
pub fn analyze_album_with_index(
    files: &[&Path],
    track_index: Option<u32>,
) -> Result<AlbumGainResult> {
    Ok(analyze_album_internal(files, track_index, None, false)?.album)
}

/// Analyze multiple tracks for album gain with progress reporting
///
/// The callback receives `(file_index, bytes_read, total_bytes)` and is called
/// after each decoded packet. `file_index` indicates which file is currently
/// being analyzed (0-based).
///
/// Companion to [`analyze_track_with_progress`]; both stem from the
/// progress-indication request in #106 (@Sappharad).
#[cfg(feature = "replaygain")]
pub fn analyze_album_with_progress(
    files: &[&Path],
    track_index: Option<u32>,
    on_progress: &dyn Fn(usize, u64, u64),
) -> Result<AlbumGainResult> {
    Ok(analyze_album_internal(files, track_index, Some(on_progress), false)?.album)
}

/// Lenient counterpart of [`analyze_album_with_index`]: files that fail to
/// analyze are skipped instead of aborting the whole album. Returns an
/// [`AlbumAnalysisReport`] describing both the album result and the skipped
/// files. Errors only when every file fails (or argument validation fails).
#[cfg(feature = "replaygain")]
pub fn analyze_album_lenient_with_index(
    files: &[&Path],
    track_index: Option<u32>,
) -> Result<AlbumAnalysisReport> {
    analyze_album_internal(files, track_index, None, true)
}

/// Lenient counterpart of [`analyze_album_with_progress`].
#[cfg(feature = "replaygain")]
pub fn analyze_album_lenient_with_progress(
    files: &[&Path],
    track_index: Option<u32>,
    on_progress: &dyn Fn(usize, u64, u64),
) -> Result<AlbumAnalysisReport> {
    analyze_album_internal(files, track_index, Some(on_progress), true)
}

#[cfg(feature = "replaygain")]
fn analyze_album_internal(
    files: &[&Path],
    track_index: Option<u32>,
    on_progress: Option<&dyn Fn(usize, u64, u64)>,
    skip_errors: bool,
) -> Result<AlbumAnalysisReport> {
    let mut track_results = Vec::with_capacity(files.len());
    let mut album_peak: f64 = 0.0;
    // Album histogram accumulates all track histograms (like B[] in original mp3gain)
    let mut album_histogram = LoudnessHistogram::new();
    let mut failures: Vec<(usize, String)> = Vec::new();
    let mut successful_indices: Vec<usize> = Vec::with_capacity(files.len());

    for (i, file) in files.iter().enumerate() {
        // Create a per-file progress callback that includes the file index
        let file_progress: Option<Box<dyn Fn(u64, u64) + '_>> =
            on_progress.map(|cb| Box::new(move |bytes, total| cb(i, bytes, total)) as _);

        // Analyze each track and get histogram
        match analyze_track_internal(file, track_index, file_progress.as_deref()) {
            Ok(internal) => {
                album_peak = album_peak.max(internal.result.peak);
                album_histogram.accumulate(&internal.histogram);
                track_results.push(internal.result);
                successful_indices.push(i);
            }
            Err(e) => {
                if skip_errors {
                    failures.push((i, format!("{}", e)));
                } else {
                    return Err(e);
                }
            }
        }
    }

    if track_results.is_empty() && !files.is_empty() {
        return Err(Error::AllFilesFailed { count: files.len() });
    }

    // Calculate album loudness from combined histogram (95th percentile)
    let album_loudness_db = album_histogram.get_loudness();
    let album_gain_db = PINK_REF - album_loudness_db;

    let album = AlbumGainResult::new(track_results, album_loudness_db, album_gain_db, album_peak);
    Ok(AlbumAnalysisReport {
        album,
        failures,
        successful_indices,
    })
}

/// Analyze multiple tracks for album gain in parallel.
///
/// Tracks are decoded and filtered concurrently using a rayon thread pool
/// (or the global pool when `threads <= 1`, in which case this falls back
/// to the serial implementation). The album histogram fold is associative,
/// and `track_results` is preserved in input order — the parallel result is
/// numerically identical to the serial one.
#[cfg(feature = "replaygain")]
pub fn analyze_album_parallel(
    files: &[&Path],
    track_index: Option<u32>,
    threads: usize,
) -> Result<AlbumGainResult> {
    if threads <= 1 || files.len() <= 1 {
        return Ok(analyze_album_internal(files, track_index, None, false)?.album);
    }
    Ok(analyze_album_parallel_internal::<fn(usize, &Path)>(files, track_index, None, false)?.album)
}

/// Analyze multiple tracks for album gain in parallel, with a per-file
/// completion callback.
///
/// `on_complete(idx, path)` is invoked from the rayon worker thread that
/// finished decoding `files[idx]`. The callback may be called concurrently
/// from multiple threads, so it must be `Sync`.
#[cfg(feature = "replaygain")]
pub fn analyze_album_parallel_with_completion<F>(
    files: &[&Path],
    track_index: Option<u32>,
    threads: usize,
    on_complete: &F,
) -> Result<AlbumGainResult>
where
    F: Fn(usize, &Path) + Sync,
{
    if threads <= 1 || files.len() <= 1 {
        // Serial fallback still drives the completion callback in input order.
        let result = analyze_album_internal(files, track_index, None, false)?.album;
        for (i, f) in files.iter().enumerate() {
            on_complete(i, f);
        }
        return Ok(result);
    }
    Ok(analyze_album_parallel_internal(files, track_index, Some(on_complete), false)?.album)
}

/// Lenient counterpart of [`analyze_album_parallel`]: files that fail to
/// analyze are skipped instead of aborting the whole album.
#[cfg(feature = "replaygain")]
pub fn analyze_album_lenient_parallel(
    files: &[&Path],
    track_index: Option<u32>,
    threads: usize,
) -> Result<AlbumAnalysisReport> {
    if threads <= 1 || files.len() <= 1 {
        return analyze_album_internal(files, track_index, None, true);
    }
    analyze_album_parallel_internal::<fn(usize, &Path)>(files, track_index, None, true)
}

/// Lenient counterpart of [`analyze_album_parallel_with_completion`].
#[cfg(feature = "replaygain")]
pub fn analyze_album_lenient_parallel_with_completion<F>(
    files: &[&Path],
    track_index: Option<u32>,
    threads: usize,
    on_complete: &F,
) -> Result<AlbumAnalysisReport>
where
    F: Fn(usize, &Path) + Sync,
{
    if threads <= 1 || files.len() <= 1 {
        let report = analyze_album_internal(files, track_index, None, true)?;
        for (i, f) in files.iter().enumerate() {
            on_complete(i, f);
        }
        return Ok(report);
    }
    analyze_album_parallel_internal(files, track_index, Some(on_complete), true)
}

#[cfg(feature = "replaygain")]
fn analyze_album_parallel_internal<F>(
    files: &[&Path],
    track_index: Option<u32>,
    on_complete: Option<&F>,
    skip_errors: bool,
) -> Result<AlbumAnalysisReport>
where
    F: Fn(usize, &Path) + Sync,
{
    use rayon::prelude::*;

    let mut track_results = Vec::with_capacity(files.len());
    let mut album_peak: f64 = 0.0;
    let mut album_histogram = LoudnessHistogram::new();
    let mut failures: Vec<(usize, String)> = Vec::new();
    let mut successful_indices: Vec<usize> = Vec::with_capacity(files.len());

    // par_iter().collect() preserves input order, which keeps album_peak
    // / album_histogram folding deterministic and matches the serial path
    // bit-for-bit. Strict mode short-circuits at the first error; lenient
    // collects all outcomes so failures can be reported alongside successes.
    if skip_errors {
        let internals: Vec<Result<TrackAnalysisInternal>> = files
            .par_iter()
            .enumerate()
            .map(|(i, file)| {
                let r = analyze_track_internal(file, track_index, None);
                if let Some(cb) = on_complete {
                    cb(i, file);
                }
                r
            })
            .collect();
        for (i, r) in internals.into_iter().enumerate() {
            match r {
                Ok(internal) => {
                    album_peak = album_peak.max(internal.result.peak);
                    album_histogram.accumulate(&internal.histogram);
                    track_results.push(internal.result);
                    successful_indices.push(i);
                }
                Err(e) => failures.push((i, format!("{}", e))),
            }
        }
    } else {
        // collect::<Result<Vec<_>>>() short-circuits at the first error,
        // matching the serial path's fail-fast behavior.
        let internals: Vec<TrackAnalysisInternal> = files
            .par_iter()
            .enumerate()
            .map(|(i, file)| {
                let r = analyze_track_internal(file, track_index, None);
                if let Some(cb) = on_complete {
                    cb(i, file);
                }
                r
            })
            .collect::<Result<Vec<_>>>()?;
        for (i, internal) in internals.into_iter().enumerate() {
            album_peak = album_peak.max(internal.result.peak);
            album_histogram.accumulate(&internal.histogram);
            track_results.push(internal.result);
            successful_indices.push(i);
        }
    }

    if track_results.is_empty() && !files.is_empty() {
        return Err(Error::AllFilesFailed { count: files.len() });
    }

    let album_loudness_db = album_histogram.get_loudness();
    let album_gain_db = PINK_REF - album_loudness_db;

    let album = AlbumGainResult::new(track_results, album_loudness_db, album_gain_db, album_peak);
    Ok(AlbumAnalysisReport {
        album,
        failures,
        successful_indices,
    })
}

// =============================================================================
// Stub implementations when feature is disabled
// =============================================================================

#[cfg(not(feature = "replaygain"))]
pub fn analyze_track(_file_path: &Path) -> Result<ReplayGainResult> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_track_with_index(
    _file_path: &Path,
    _track_index: Option<u32>,
) -> Result<ReplayGainResult> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_track_with_progress(
    _file_path: &Path,
    _track_index: Option<u32>,
    _on_progress: &dyn Fn(u64, u64),
) -> Result<ReplayGainResult> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album(_files: &[&Path]) -> Result<AlbumGainResult> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album_with_index(
    _files: &[&Path],
    _track_index: Option<u32>,
) -> Result<AlbumGainResult> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album_with_progress(
    _files: &[&Path],
    _track_index: Option<u32>,
    _on_progress: &dyn Fn(usize, u64, u64),
) -> Result<AlbumGainResult> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album_parallel(
    _files: &[&Path],
    _track_index: Option<u32>,
    _threads: usize,
) -> Result<AlbumGainResult> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album_parallel_with_completion<F>(
    _files: &[&Path],
    _track_index: Option<u32>,
    _threads: usize,
    _on_complete: &F,
) -> Result<AlbumGainResult>
where
    F: Fn(usize, &Path) + Sync,
{
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album_lenient_with_index(
    _files: &[&Path],
    _track_index: Option<u32>,
) -> Result<AlbumAnalysisReport> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album_lenient_with_progress(
    _files: &[&Path],
    _track_index: Option<u32>,
    _on_progress: &dyn Fn(usize, u64, u64),
) -> Result<AlbumAnalysisReport> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album_lenient_parallel(
    _files: &[&Path],
    _track_index: Option<u32>,
    _threads: usize,
) -> Result<AlbumAnalysisReport> {
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

#[cfg(not(feature = "replaygain"))]
pub fn analyze_album_lenient_parallel_with_completion<F>(
    _files: &[&Path],
    _track_index: Option<u32>,
    _threads: usize,
    _on_complete: &F,
) -> Result<AlbumAnalysisReport>
where
    F: Fn(usize, &Path) + Sync,
{
    Err(Error::FeatureNotAvailable {
        feature: "ReplayGain analysis",
        feature_flag: "replaygain",
    })
}

/// Check if ReplayGain feature is available
pub fn is_available() -> bool {
    cfg!(feature = "replaygain")
}

/// Result of peak amplitude analysis
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PeakAmplitudeResult {
    peak: f64,
    peak_pcm: f64,
    sample_rate: u32,
}

impl PeakAmplitudeResult {
    #[allow(dead_code)]
    pub(crate) fn new(peak: f64, peak_pcm: f64, sample_rate: u32) -> Self {
        Self {
            peak,
            peak_pcm,
            sample_rate,
        }
    }

    pub fn peak(&self) -> f64 {
        self.peak
    }
    pub fn peak_pcm(&self) -> f64 {
        self.peak_pcm
    }
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

impl std::fmt::Display for PeakAmplitudeResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "peak: {:.6} ({:.1} PCM)", self.peak, self.peak_pcm)
    }
}

/// Find the peak amplitude of an audio file by decoding the audio.
/// This properly decodes the audio to measure actual PCM sample values,
/// unlike the old method that estimated from global_gain fields.
///
/// Returns peak amplitude that can exceed 1.0 for clipping audio.
#[cfg(feature = "replaygain")]
pub fn find_peak_amplitude(file_path: &Path) -> Result<PeakAmplitudeResult> {
    let file = std::fs::File::open(file_path).map_err(|e| Error::io_open(file_path, e))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| Error::ProbeFailed {
            path: file_path.to_path_buf(),
            source: Box::new(e),
        })?;

    let track = format
        .tracks()
        .iter()
        .find(|t| {
            t.codec_params
                .as_ref()
                .and_then(|p| p.audio())
                .is_some_and(|a| a.codec != CODEC_ID_NULL_AUDIO)
        })
        .ok_or(Error::NoAudioTrack)?;

    let track_id = track.id;
    let audio_params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or(Error::NoAudioTrack)?;
    let sample_rate = audio_params
        .sample_rate
        .ok_or(Error::UnsupportedSampleRate(0))?;

    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(audio_params, &AudioDecoderOptions::default())
        .map_err(|e| Error::Decode(Box::new(e)))?;

    let mut max_peak: f64 = 0.0;

    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(e) => return Err(Error::Decode(Box::new(e))),
        };

        if packet.track_id != track_id {
            continue;
        }

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(_)) => continue,
            Err(e) => return Err(Error::Decode(Box::new(e))),
        };

        // Process each sample format and track peak
        // Symphonia's MP3 decoder outputs F32 samples in the range [-1.0, 1.0]
        // However, the decoder internally clips samples that exceed this range.
        // For accurate peak detection of potentially clipping audio, we need to
        // access the raw decoded values before normalization.
        //
        // The F32 buffer from Symphonia is already normalized and clipped.
        // To detect clipping, we check if the peak is exactly 1.0 (or very close),
        // which indicates the audio may have been clipped by the decoder.
        // Iterate plane-major so `plane(ch).unwrap()` happens once per channel
        // rather than once per sample.
        match &decoded {
            GenericAudioBufferRef::F32(buf) => {
                for ch in 0..buf.num_planes() {
                    for &sample in buf.plane(ch).unwrap() {
                        max_peak = max_peak.max((sample as f64).abs());
                    }
                }
            }
            GenericAudioBufferRef::S16(buf) => {
                for ch in 0..buf.num_planes() {
                    for &sample in buf.plane(ch).unwrap() {
                        // S16 samples: convert to normalized range
                        // This can exceed 1.0 if sample is at max (32767/32768 ≈ 0.99997)
                        let s = (sample as f64).abs() / SAMPLE_SCALE_16BIT;
                        max_peak = max_peak.max(s);
                    }
                }
            }
            GenericAudioBufferRef::S32(buf) => {
                for ch in 0..buf.num_planes() {
                    for &sample in buf.plane(ch).unwrap() {
                        let s = (sample as f64).abs() / 2147483648.0;
                        max_peak = max_peak.max(s);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(PeakAmplitudeResult::new(
        max_peak,
        max_peak * SAMPLE_SCALE_16BIT,
        sample_rate,
    ))
}

#[cfg(not(feature = "replaygain"))]
pub fn find_peak_amplitude(_file_path: &Path) -> Result<PeakAmplitudeResult> {
    Err(Error::FeatureNotAvailable {
        feature: "Peak amplitude analysis",
        feature_flag: "replaygain",
    })
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
        // Test all supported sample rates
        let supported_rates = [
            96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000,
        ];
        for rate in supported_rates {
            let filter = EqualLoudnessFilter::new(rate);
            assert!(filter.is_some(), "Sample rate {} should be supported", rate);
            let filter = filter.unwrap();
            assert_eq!(filter.yule_a.len(), 11);
            assert_eq!(filter.butter_a.len(), 3);
        }

        // Test unsupported sample rate
        let unsupported = EqualLoudnessFilter::new(99999);
        assert!(
            unsupported.is_none(),
            "Unsupported sample rate should return None"
        );
    }

    #[cfg(feature = "replaygain")]
    #[test]
    fn test_rms_calculation() {
        // Test that the analyzer correctly processes samples through the full filter chain
        let sample_rate = 44100u32;
        let mut filter = EqualLoudnessFilter::new(sample_rate).unwrap();
        let mut analyzer = ReplayGainAnalyzer::new(sample_rate);

        // Create a simple sine wave at 1kHz
        // Note: ReplayGain algorithm expects 16-bit range samples (-32768 to 32767)
        let frequency = 1000.0;
        let amplitude_normalized = 0.5; // Normalized amplitude (0.0 to 1.0)
        let amplitude = amplitude_normalized * SAMPLE_SCALE_16BIT; // Scale to 16-bit range
        let duration_samples = sample_rate as usize; // 1 second

        for i in 0..duration_samples {
            let t = i as f64 / sample_rate as f64;
            let sample = amplitude * (2.0 * std::f64::consts::PI * frequency * t).sin();
            let filtered = filter.process(sample);
            analyzer.add_mono_sample(filtered);
        }

        // Should have processed multiple windows (1 second = 20 windows at 50ms each)
        let loudness = analyzer.get_loudness();
        // Loudness should be a reasonable positive dB value for 16-bit range samples
        // After equal-loudness filtering, the value will vary based on frequency response
        assert!(
            loudness > 50.0,
            "Loudness should be above 50 dB: {}",
            loudness
        );
        assert!(
            loudness < 100.0,
            "Loudness should be below 100 dB: {}",
            loudness
        );
    }

    #[cfg(feature = "replaygain")]
    #[test]
    fn lenient_album_skips_failed_files() {
        // Issue #144: --skip-errors should let album analysis continue when a
        // single file fails to probe. Mix one good fixture with one bogus path.
        let good = std::path::PathBuf::from("tests/fixtures/test_stereo.mp3");
        let bad = std::path::PathBuf::from("tests/fixtures/this-file-does-not-exist.mp3");
        let files = vec![good.as_path(), bad.as_path()];

        let strict = analyze_album_with_index(&files, None);
        assert!(
            strict.is_err(),
            "strict variant must fail when any file is unreadable"
        );

        let report =
            analyze_album_lenient_with_index(&files, None).expect("lenient must skip the bad file");
        assert_eq!(report.album.tracks().len(), 1);
        assert_eq!(report.successful_indices, vec![0]);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].0, 1);
    }

    #[cfg(feature = "replaygain")]
    #[test]
    fn lenient_album_errors_when_all_fail() {
        let bad1 = std::path::PathBuf::from("tests/fixtures/missing-1.mp3");
        let bad2 = std::path::PathBuf::from("tests/fixtures/missing-2.mp3");
        let files = vec![bad1.as_path(), bad2.as_path()];

        let result = analyze_album_lenient_with_index(&files, None);
        assert!(matches!(result, Err(Error::AllFilesFailed { count: 2 })));
    }

    #[cfg(feature = "replaygain")]
    #[test]
    fn test_loudness_calculation() {
        // Test analyzer with known amplitude using a 1kHz sine wave
        // (DC is filtered out by the equal-loudness filter)
        let sample_rate = 44100u32;
        let mut filter = EqualLoudnessFilter::new(sample_rate).unwrap();
        let mut analyzer = ReplayGainAnalyzer::new(sample_rate);

        // Feed a 1kHz sine wave at 0.1 normalized amplitude
        // Note: ReplayGain algorithm expects 16-bit range samples
        let frequency = 1000.0;
        let amplitude_normalized = 0.1; // Normalized amplitude
        let amplitude = amplitude_normalized * SAMPLE_SCALE_16BIT; // Scale to 16-bit range (3276.8)
        let duration_samples = sample_rate as usize; // 1 second

        for i in 0..duration_samples {
            let t = i as f64 / sample_rate as f64;
            let sample = amplitude * (2.0 * std::f64::consts::PI * frequency * t).sin();
            let filtered = filter.process(sample);
            analyzer.add_mono_sample(filtered);
        }

        let loudness = analyzer.get_loudness();
        // For a sine wave at 3276.8 amplitude, after filtering the loudness
        // should be in a reasonable range for 16-bit audio
        assert!(
            loudness > 50.0 && loudness < 80.0,
            "Loudness {} should be between 50 and 80 dB for a 0.1 amplitude 1kHz sine",
            loudness
        );
    }
}
