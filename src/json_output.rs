use mp3rgain::replaygain::ReplayGainResult;
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Success,
    Error,
    Skipped,
    DryRun,
    Info,
    NoTag,
}

#[derive(Serialize)]
pub struct JsonOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<JsonFileResult>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<JsonAlbumResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<JsonSummary>,
}

#[derive(Serialize, Clone, Default)]
pub struct JsonFileResult {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<FileStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mpeg_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_gain: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_gain: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avg_gain: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headroom_steps: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headroom_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_applied_steps: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gain_applied_db: Option<f64>,
    /// `--tags-only` (issue #308): the absolute `REPLAYGAIN_TRACK_GAIN` value
    /// written, which `-k` may have capped below the measured gain. Absent in
    /// every other mode, where the tag holds a residual instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_gain_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loudness_db: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loudness_lufs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis_mode: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_amplitude: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}

impl JsonFileResult {
    /// Error record for `file` — the shared shape every per-file command
    /// emits when an operation fails.
    pub fn error(file: &Path, e: impl std::fmt::Display) -> Self {
        Self {
            file: file.display().to_string(),
            status: Some(FileStatus::Error),
            error: Some(e.to_string()),
            ..Default::default()
        }
    }

    /// Base record for `file` carrying a ReplayGain analysis (loudness, mode,
    /// peak). Call sites layer their status / gain fields on top with struct
    /// update syntax.
    pub fn from_analysis(file: &Path, result: &ReplayGainResult) -> Self {
        Self {
            file: file.display().to_string(),
            loudness_db: Some(result.loudness_db()),
            loudness_lufs: result.loudness_lufs(),
            analysis_mode: Some(result.analysis_mode().name()),
            peak: Some(result.peak()),
            ..Default::default()
        }
    }
}

#[derive(Serialize, Clone, Copy)]
pub struct JsonAlbumResult {
    pub loudness_db: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loudness_lufs: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub analysis_mode: Option<&'static str>,
    pub gain_db: f64,
    pub gain_steps: i32,
    /// `--tags-only`: the absolute `REPLAYGAIN_ALBUM_GAIN` value written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag_gain_db: Option<f64>,
    pub peak: f64,
}

#[derive(Serialize)]
pub struct JsonSummary {
    pub total_files: usize,
    pub successful: usize,
    pub failed: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
}
