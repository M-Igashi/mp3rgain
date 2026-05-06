use serde::Serialize;

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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loudness_db: Option<f64>,
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

#[derive(Serialize, Clone, Copy)]
pub struct JsonAlbumResult {
    pub loudness_db: f64,
    pub gain_db: f64,
    pub gain_steps: i32,
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
