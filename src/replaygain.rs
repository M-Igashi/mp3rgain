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

/// Pink noise reference calibration constant
/// This is the loudness value produced by the ReplayGain algorithm when analyzing
/// the standard -14 dB FS pink noise reference signal. All loudness measurements
/// are compared against this reference to calculate the required gain adjustment.
/// Source: https://replaygain.hydrogenaud.io/calibration.html
const PINK_REF: f64 = 64.82;

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

/// Yule-Walker and Butterworth filter coefficients for equal-loudness weighting
/// These are the coefficients used in the original ReplayGain algorithm
/// Supporting all 12 sample rates from the original mp3gain
/// Reference: https://github.com/cpuimage/ReplayGainAnalysis/blob/master/gain_analysis.c
#[cfg(feature = "replaygain")]
mod filter_coeffs {
    // =========================================================================
    // 96000 Hz coefficients (ABYule[0], ABButter[0])
    // =========================================================================
    pub const YULE_A_96000: [f64; 11] = [
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

    pub const YULE_B_96000: [f64; 11] = [
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

    pub const BUTTER_A_96000: [f64; 3] = [1.0, -1.98611621154089, 0.986211929160751];

    pub const BUTTER_B_96000: [f64; 3] = [0.99308203517541, -1.98616407035082, 0.99308203517541];

    // =========================================================================
    // 88200 Hz coefficients (ABYule[1], ABButter[1])
    // =========================================================================
    pub const YULE_A_88200: [f64; 11] = [
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

    pub const YULE_B_88200: [f64; 11] = [
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

    pub const BUTTER_A_88200: [f64; 3] = [1.0, -1.98488843762334, 0.979389350028798];

    pub const BUTTER_B_88200: [f64; 3] = [0.992472550461293, -1.98494510092258, 0.992472550461293];

    // =========================================================================
    // 64000 Hz coefficients (ABYule[2], ABButter[2])
    // =========================================================================
    pub const YULE_A_64000: [f64; 11] = [
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

    pub const YULE_B_64000: [f64; 11] = [
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

    pub const BUTTER_A_64000: [f64; 3] = [1.0, -1.97917472731008, 0.979389350028798];

    pub const BUTTER_B_64000: [f64; 3] = [0.989641019334721, -1.97928203866944, 0.989641019334721];

    // =========================================================================
    // 48000 Hz coefficients (ABYule[3], ABButter[3])
    // =========================================================================
    pub const YULE_A_48000: [f64; 11] = [
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

    pub const BUTTER_A_48000: [f64; 3] = [1.0, -1.97223372919527, 0.97261396931306];

    pub const BUTTER_B_48000: [f64; 3] = [0.98621192462708, -1.97242384925416, 0.98621192462708];

    // =========================================================================
    // 44100 Hz coefficients (ABYule[4], ABButter[4])
    // =========================================================================
    pub const YULE_A_44100: [f64; 11] = [
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

    pub const BUTTER_A_44100: [f64; 3] = [1.0, -1.96977855582618, 0.97022847566350];

    pub const BUTTER_B_44100: [f64; 3] = [0.98500175787242, -1.97000351574484, 0.98500175787242];

    // =========================================================================
    // 32000 Hz coefficients (ABYule[5], ABButter[5])
    // =========================================================================
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

    pub const YULE_B_32000: [f64; 11] = [
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

    pub const BUTTER_A_32000: [f64; 3] = [1.0, -1.95835380975398, 0.95920349965459];

    pub const BUTTER_B_32000: [f64; 3] = [0.97938932735214, -1.95877865470428, 0.97938932735214];

    // =========================================================================
    // 24000 Hz coefficients (ABYule[6], ABButter[6])
    // =========================================================================
    pub const YULE_A_24000: [f64; 11] = [
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

    pub const YULE_B_24000: [f64; 11] = [
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

    pub const BUTTER_A_24000: [f64; 3] = [1.0, -1.95002759149878, 0.95124613669835];

    pub const BUTTER_B_24000: [f64; 3] = [0.97531843204928, -1.95063686409857, 0.97531843204928];

    // =========================================================================
    // 22050 Hz coefficients (ABYule[7], ABButter[7])
    // =========================================================================
    pub const YULE_A_22050: [f64; 11] = [
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

    pub const YULE_B_22050: [f64; 11] = [
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

    pub const BUTTER_A_22050: [f64; 3] = [1.0, -1.94561023566527, 0.94705070426118];

    pub const BUTTER_B_22050: [f64; 3] = [0.97316523498161, -1.94633046996323, 0.97316523498161];

    // =========================================================================
    // 16000 Hz coefficients (ABYule[8], ABButter[8])
    // =========================================================================
    pub const YULE_A_16000: [f64; 11] = [
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

    pub const YULE_B_16000: [f64; 11] = [
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

    pub const BUTTER_A_16000: [f64; 3] = [1.0, -1.92783286977036, 0.93034775234268];

    pub const BUTTER_B_16000: [f64; 3] = [0.96454515552826, -1.92909031105652, 0.96454515552826];

    // =========================================================================
    // 12000 Hz coefficients (ABYule[9], ABButter[9])
    // =========================================================================
    pub const YULE_A_12000: [f64; 11] = [
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

    pub const YULE_B_12000: [f64; 11] = [
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

    pub const BUTTER_A_12000: [f64; 3] = [1.0, -1.91858953033784, 0.92177618768381];

    pub const BUTTER_B_12000: [f64; 3] = [0.96009142950541, -1.92018285901082, 0.96009142950541];

    // =========================================================================
    // 11025 Hz coefficients (ABYule[10], ABButter[10])
    // =========================================================================
    pub const YULE_A_11025: [f64; 11] = [
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

    pub const YULE_B_11025: [f64; 11] = [
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

    pub const BUTTER_A_11025: [f64; 3] = [1.0, -1.91542108074780, 0.91885558323625];

    pub const BUTTER_B_11025: [f64; 3] = [0.95856916599601, -1.91713833199203, 0.95856916599601];

    // =========================================================================
    // 8000 Hz coefficients (ABYule[11], ABButter[11])
    // =========================================================================
    pub const YULE_A_8000: [f64; 11] = [
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

    pub const YULE_B_8000: [f64; 11] = [
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

    pub const BUTTER_A_8000: [f64; 3] = [1.0, -1.88903307939452, 0.89487434461664];

    pub const BUTTER_B_8000: [f64; 3] = [0.94597685600279, -1.89195371200558, 0.94597685600279];
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

        // Apply Yule-Walker filter with denormal prevention
        // The 1e-10 constant prevents denormal float slowdowns on silent audio
        // Reference: gain_analysis.c filterYule()
        let yule_out = DENORMAL_PREVENTION
            + self.yule_b[0] * self.yule_x[0]
            + (1..11)
                .map(|i| self.yule_b[i] * self.yule_x[i] - self.yule_a[i] * self.yule_y[i])
                .sum::<f64>();
        self.yule_y[0] = yule_out;

        // Shift Butterworth filter history and insert Yule output
        self.butter_x.copy_within(0..2, 1);
        self.butter_y.copy_within(0..2, 1);
        self.butter_x[0] = yule_out;

        // Apply Butterworth high-pass filter with denormal prevention
        let butter_out = DENORMAL_PREVENTION
            + self.butter_b[0] * self.butter_x[0]
            + (1..3)
                .map(|i| self.butter_b[i] * self.butter_x[i] - self.butter_a[i] * self.butter_y[i])
                .sum::<f64>();
        self.butter_y[0] = butter_out;

        butter_out
    }
}

// =============================================================================
// RMS and loudness calculation
// =============================================================================

/// Steps per dB for histogram resolution (matches original mp3gain)
const STEPS_PER_DB: f64 = 100.0;

/// Maximum histogram size (covers -70 dB to +10 dB range)
const HISTOGRAM_SIZE: usize = 12000;

/// Histogram offset to handle negative dB values
const HISTOGRAM_OFFSET: i32 = 7000;

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
        for (i, &count) in other.data.iter().enumerate() {
            self.data[i] += count;
        }
    }

    /// Calculate loudness from histogram using 95th percentile
    fn get_loudness(&self) -> f64 {
        let total: u64 = self.data.iter().map(|&x| x as u64).sum();
        if total == 0 {
            return -70.0;
        }

        let threshold = ((total as f64) * (1.0 - RMS_PERCENTILE)).ceil() as u64;
        let mut count = 0u64;

        for i in (0..HISTOGRAM_SIZE).rev() {
            count += self.data[i] as u64;
            if count >= threshold {
                return (i as i32 - HISTOGRAM_OFFSET) as f64 / STEPS_PER_DB;
            }
        }

        -70.0
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

    /// Get a reference to the histogram for accumulation
    fn get_histogram(&self) -> &LoudnessHistogram {
        &self.histogram
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
    if mp4meta::is_mp4_file(file_path) {
        AudioFileType::Aac
    } else {
        AudioFileType::Mp3
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
) -> Result<TrackAnalysisInternal> {
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
        .map(|_| {
            EqualLoudnessFilter::new(sample_rate).ok_or_else(|| {
                anyhow::anyhow!(
                    "Unsupported sample rate: {} Hz. Supported rates: 96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000",
                    sample_rate
                )
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let mut analyzer = ReplayGainAnalyzer::new(sample_rate);
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
        process_audio_buffer(&decoded, &mut filters, &mut analyzer, &mut peak);
    }

    // Finish any remaining samples in the last window
    analyzer.finish_window();

    // Calculate loudness and gain
    let loudness_db = analyzer.get_loudness();
    let gain_db = PINK_REF - loudness_db;

    let result = ReplayGainResult {
        loudness_db,
        gain_db,
        peak,
        sample_rate,
        file_type,
    };

    Ok(TrackAnalysisInternal {
        result,
        histogram: analyzer.get_histogram().clone(),
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
    let internal = analyze_track_internal(file_path, track_index)?;
    Ok(internal.result)
}

/// Process an audio buffer and feed filtered samples to the analyzer
#[cfg(feature = "replaygain")]
fn process_audio_buffer(
    buffer: &AudioBufferRef,
    filters: &mut [EqualLoudnessFilter],
    analyzer: &mut ReplayGainAnalyzer,
    peak: &mut f64,
) {
    match buffer {
        AudioBufferRef::F32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();

            for frame in 0..frames {
                // Get samples for each channel
                let left = buf.chan(0)[frame] as f64;
                *peak = peak.max(left.abs());
                let left_filtered = filters[0].process(left);

                if channels >= 2 {
                    let right = buf.chan(1)[frame] as f64;
                    *peak = peak.max(right.abs());
                    let right_filtered = filters[1].process(right);
                    analyzer.add_sample(left_filtered, right_filtered);
                } else {
                    analyzer.add_mono_sample(left_filtered);
                }
            }
        }
        AudioBufferRef::S16(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let scale = 1.0 / 32768.0;

            for frame in 0..frames {
                let left = buf.chan(0)[frame] as f64 * scale;
                *peak = peak.max(left.abs());
                let left_filtered = filters[0].process(left);

                if channels >= 2 {
                    let right = buf.chan(1)[frame] as f64 * scale;
                    *peak = peak.max(right.abs());
                    let right_filtered = filters[1].process(right);
                    analyzer.add_sample(left_filtered, right_filtered);
                } else {
                    analyzer.add_mono_sample(left_filtered);
                }
            }
        }
        AudioBufferRef::S32(buf) => {
            let channels = buf.spec().channels.count();
            let frames = buf.frames();
            let scale = 1.0 / 2147483648.0;

            for frame in 0..frames {
                let left = buf.chan(0)[frame] as f64 * scale;
                *peak = peak.max(left.abs());
                let left_filtered = filters[0].process(left);

                if channels >= 2 {
                    let right = buf.chan(1)[frame] as f64 * scale;
                    *peak = peak.max(right.abs());
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
    let mut track_results = Vec::with_capacity(files.len());
    let mut album_peak: f64 = 0.0;
    // Album histogram accumulates all track histograms (like B[] in original mp3gain)
    let mut album_histogram = LoudnessHistogram::new();

    for file in files {
        // Analyze each track and get histogram
        let internal = analyze_track_internal(file, track_index)?;
        album_peak = album_peak.max(internal.result.peak);

        // Accumulate track histogram into album histogram
        album_histogram.accumulate(&internal.histogram);

        track_results.push(internal.result);
    }

    // Calculate album loudness from combined histogram (95th percentile)
    let album_loudness_db = album_histogram.get_loudness();
    let album_gain_db = PINK_REF - album_loudness_db;

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
        // Test that the analyzer correctly processes samples
        let sample_rate = 44100u32;
        let mut analyzer = ReplayGainAnalyzer::new(sample_rate);

        // Create a simple sine wave at 1kHz
        let frequency = 1000.0;
        let amplitude = 0.5;
        let duration_samples = sample_rate as usize; // 1 second

        for i in 0..duration_samples {
            let t = i as f64 / sample_rate as f64;
            let sample = amplitude * (2.0 * std::f64::consts::PI * frequency * t).sin();
            analyzer.add_mono_sample(sample);
        }

        // Should have processed multiple windows (1 second = 20 windows at 50ms each)
        let loudness = analyzer.get_loudness();
        // Loudness should be a reasonable negative dB value
        assert!(loudness < 0.0, "Loudness should be negative: {}", loudness);
        assert!(
            loudness > -70.0,
            "Loudness should be above -70 dB: {}",
            loudness
        );
    }

    #[cfg(feature = "replaygain")]
    #[test]
    fn test_loudness_calculation() {
        // Test analyzer with known amplitude
        let sample_rate = 44100u32;
        let mut analyzer = ReplayGainAnalyzer::new(sample_rate);

        // Feed constant amplitude samples (simulating DC or very low frequency)
        let amplitude = 0.1;
        let duration_samples = sample_rate as usize; // 1 second

        for _ in 0..duration_samples {
            analyzer.add_mono_sample(amplitude);
        }

        let loudness = analyzer.get_loudness();
        // For constant amplitude 0.1, mean_square = 0.01
        // 10 * log10(0.01) = -20 dB
        assert!(
            (loudness - (-20.0)).abs() < 1.0,
            "Loudness {} should be close to -20 dB",
            loudness
        );
    }
}
