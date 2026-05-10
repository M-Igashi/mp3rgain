use mp3rgain::replaygain::{self, ReplayGainResult, REPLAYGAIN_REFERENCE_DB};
use std::path::{Path, PathBuf};

#[derive(Default, Clone, PartialEq)]
pub enum FileStatus {
    #[default]
    Pending,
    Analyzing,
    Analyzed,
    Applying,
    Done,
    Error(String),
}

impl FileStatus {
    pub fn as_str(&self) -> &str {
        match self {
            FileStatus::Pending => "",
            FileStatus::Analyzing => "Analyzing...",
            FileStatus::Analyzed => "OK",
            FileStatus::Applying => "Applying...",
            FileStatus::Done => "Done",
            FileStatus::Error(_) => "Error",
        }
    }
}

#[derive(Default, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub filename: String,
    pub volume: Option<f64>,
    pub clipping: bool,
    pub track_gain: Option<f64>,
    pub track_clip: bool,
    pub album_volume: Option<f64>,
    pub album_gain: Option<f64>,
    pub album_clip: bool,
    pub status: FileStatus,
}

pub struct Mp3rgainApp {
    pub files: Vec<FileEntry>,
    pub target_volume: f64,
    pub selected_indices: Vec<usize>,
    pub total_progress: f32,
    pub is_processing: bool,
    pub status_message: String,
}

impl Mp3rgainApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            files: Vec::new(),
            target_volume: 89.0,
            selected_indices: Vec::new(),
            total_progress: 0.0,
            is_processing: false,
            status_message: String::new(),
        }
    }

    pub fn add_files(&mut self, paths: Vec<PathBuf>) {
        let mut added = 0;
        let mut skipped = 0;

        for path in paths {
            if mp3rgain::is_supported_audio_path(&path) && path.is_file() {
                if self.is_duplicate(&path) {
                    skipped += 1;
                    continue;
                }
                let filename = path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.files.push(FileEntry {
                    path,
                    filename,
                    ..Default::default()
                });
                added += 1;
            }
        }

        if skipped > 0 {
            self.status_message =
                format!("Added {} file(s), {} duplicate(s) skipped", added, skipped);
        } else if added > 0 {
            self.status_message = format!("Added {} file(s)", added);
        }
    }

    fn is_duplicate(&self, path: &Path) -> bool {
        self.files.iter().any(|f| f.path == path)
    }

    pub fn add_folder(&mut self, folder: PathBuf, recursive: bool) {
        let paths_to_add = mp3rgain::collect_audio_files(&folder, recursive).unwrap_or_default();
        self.add_files(paths_to_add);
    }

    pub fn remove_selected(&mut self) {
        let mut indices = self.selected_indices.clone();
        indices.sort_unstable();
        for &idx in indices.iter().rev() {
            if idx < self.files.len() {
                self.files.remove(idx);
            }
        }
        self.selected_indices.clear();
    }

    pub fn clear_files(&mut self) {
        self.files.clear();
        self.selected_indices.clear();
    }

    pub fn analyze_tracks(&mut self) {
        if self.files.is_empty() || !replaygain::is_available() {
            if !replaygain::is_available() {
                self.status_message = "ReplayGain feature not available".to_string();
            }
            return;
        }

        self.is_processing = true;
        self.total_progress = 0.0;

        let total = self.files.len();
        let mut analyzed = 0;
        let mut errors = 0;

        for (i, file) in self.files.iter_mut().enumerate() {
            file.status = FileStatus::Analyzing;
            self.total_progress = i as f32 / total as f32;

            match replaygain::analyze_track(&file.path) {
                Ok(result) => {
                    Self::populate_track_analysis(file, &result, self.target_volume);
                    file.status = FileStatus::Analyzed;
                    analyzed += 1;
                }
                Err(e) => {
                    file.status = FileStatus::Error(e.to_string());
                    errors += 1;
                }
            }
        }

        self.total_progress = 1.0;
        self.is_processing = false;
        self.status_message = Self::format_result_message("Analyzed", analyzed, errors);
    }

    pub fn analyze_album(&mut self) {
        if self.files.is_empty() || !replaygain::is_available() {
            if !replaygain::is_available() {
                self.status_message = "ReplayGain feature not available".to_string();
            }
            return;
        }

        self.is_processing = true;
        self.total_progress = 0.0;

        let paths: Vec<&std::path::Path> = self.files.iter().map(|f| f.path.as_path()).collect();

        // Use the lenient variant so a single bad file does not abort the
        // whole album scan — the failed file is shown as Error in the table
        // and the rest is analyzed normally (issue #144).
        match replaygain::analyze_album_lenient_with_index(&paths, None) {
            Ok(report) => {
                let album_gain =
                    self.target_volume - REPLAYGAIN_REFERENCE_DB + report.album.album_gain_db();
                let album_volume = REPLAYGAIN_REFERENCE_DB - report.album.album_gain_db();
                let album_clip = Self::would_clip(report.album.album_peak(), album_gain);

                for (track_idx, &file_idx) in report.successful_indices.iter().enumerate() {
                    let track_result = &report.album.tracks()[track_idx];
                    let file = &mut self.files[file_idx];
                    Self::populate_track_analysis(file, track_result, self.target_volume);
                    file.album_volume = Some(album_volume);
                    file.album_gain = Some(album_gain);
                    file.album_clip = album_clip;
                    file.status = FileStatus::Analyzed;
                }

                for (file_idx, msg) in &report.failures {
                    self.files[*file_idx].status = FileStatus::Error(msg.clone());
                }

                let analyzed = report.successful_indices.len();
                let skipped = report.failures.len();
                self.status_message = if skipped > 0 {
                    format!(
                        "Album analysis complete ({} tracks, {} skipped)",
                        analyzed, skipped
                    )
                } else {
                    format!("Album analysis complete ({} tracks)", analyzed)
                };
            }
            Err(e) => {
                self.status_message = format!("Album analysis failed: {}", e);
            }
        }

        self.total_progress = 1.0;
        self.is_processing = false;
    }

    /// Populate a file entry with track-level analysis results.
    /// Volume is displayed relative to ReplayGain reference (89 dB) for MP3Gain compatibility.
    fn populate_track_analysis(
        file: &mut FileEntry,
        result: &ReplayGainResult,
        target_volume: f64,
    ) {
        file.volume = Some(REPLAYGAIN_REFERENCE_DB - result.gain_db());
        file.clipping = result.peak() >= 1.0;
        let gain = target_volume - REPLAYGAIN_REFERENCE_DB + result.gain_db();
        file.track_gain = Some(gain);
        file.track_clip = Self::would_clip(result.peak(), gain);
    }

    fn format_result_message(action: &str, count: usize, errors: usize) -> String {
        if errors > 0 {
            format!("{} {} file(s), {} error(s)", action, count, errors)
        } else {
            format!("{} {} file(s)", action, count)
        }
    }

    fn would_clip(peak: f64, gain_db: f64) -> bool {
        let gain_linear = 10.0_f64.powf(gain_db / 20.0);
        peak * gain_linear > 1.0
    }

    pub fn apply_track_gain(&mut self) {
        if self.files.is_empty() {
            return;
        }

        self.is_processing = true;
        self.total_progress = 0.0;

        let total = self.files.len();
        let mut applied = 0;
        let mut errors = 0;

        for (i, file) in self.files.iter_mut().enumerate() {
            self.total_progress = i as f32 / total as f32;

            if let Some(gain_db) = file.track_gain {
                file.status = FileStatus::Applying;
                match mp3rgain::apply_gain_db(&file.path, gain_db) {
                    Ok(_) => {
                        file.status = FileStatus::Done;
                        applied += 1;
                    }
                    Err(e) => {
                        file.status = FileStatus::Error(e.to_string());
                        errors += 1;
                    }
                }
            }
        }

        self.total_progress = 1.0;
        self.is_processing = false;
        self.status_message = Self::format_result_message("Applied track gain to", applied, errors);
    }

    pub fn apply_album_gain(&mut self) {
        if self.files.is_empty() {
            return;
        }

        self.is_processing = true;
        self.total_progress = 0.0;

        let total = self.files.len();
        let mut applied = 0;
        let mut errors = 0;

        for (i, file) in self.files.iter_mut().enumerate() {
            self.total_progress = i as f32 / total as f32;

            if let Some(gain_db) = file.album_gain {
                file.status = FileStatus::Applying;
                match mp3rgain::apply_gain_db(&file.path, gain_db) {
                    Ok(_) => {
                        file.status = FileStatus::Done;
                        applied += 1;
                    }
                    Err(e) => {
                        file.status = FileStatus::Error(e.to_string());
                        errors += 1;
                    }
                }
            }
        }

        self.total_progress = 1.0;
        self.is_processing = false;
        self.status_message = Self::format_result_message("Applied album gain to", applied, errors);
    }
}

impl eframe::App for Mp3rgainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::ui::render(self, ctx);
    }
}
