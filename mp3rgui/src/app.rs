use mp3rgain::replaygain::{self, REPLAYGAIN_REFERENCE_DB};
use std::path::PathBuf;

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
    pub file_progress: f32,
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
            file_progress: 0.0,
            total_progress: 0.0,
            is_processing: false,
            status_message: String::new(),
        }
    }

    pub fn add_files(&mut self, paths: Vec<PathBuf>) {
        let mut added = 0;
        let mut skipped = 0;

        for path in paths {
            if Self::is_supported_format(&path) && path.is_file() {
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

    fn is_supported_format(path: &PathBuf) -> bool {
        // Skip macOS resource fork files (._*)
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with("._") {
                return false;
            }
        }
        path.extension().map_or(false, |ext| {
            ext.eq_ignore_ascii_case("mp3")
                || ext.eq_ignore_ascii_case("m4a")
                || ext.eq_ignore_ascii_case("aac")
        })
    }

    fn is_duplicate(&self, path: &PathBuf) -> bool {
        self.files.iter().any(|f| f.path == *path)
    }

    pub fn add_folder(&mut self, folder: PathBuf, recursive: bool) {
        let mut paths_to_add = Vec::new();
        Self::collect_files_from_folder(&folder, recursive, &mut paths_to_add);
        self.add_files(paths_to_add);
    }

    fn collect_files_from_folder(folder: &PathBuf, recursive: bool, paths: &mut Vec<PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.is_dir() && recursive {
                    Self::collect_files_from_folder(&path, true, paths);
                } else if Self::is_supported_format(&path) {
                    paths.push(path);
                }
            }
        }
    }

    pub fn remove_selected(&mut self) {
        let mut indices: Vec<usize> = self.selected_indices.clone();
        indices.sort_by(|a, b| b.cmp(a));
        for idx in indices {
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
        self.file_progress = 0.0;
        self.total_progress = 0.0;

        let total = self.files.len();
        let mut analyzed = 0;
        let mut errors = 0;

        for (i, file) in self.files.iter_mut().enumerate() {
            file.status = FileStatus::Analyzing;
            self.total_progress = i as f32 / total as f32;

            match replaygain::analyze_track(&file.path) {
                Ok(result) => {
                    // Display volume relative to ReplayGain reference (89 dB) for MP3Gain compatibility
                    file.volume = Some(REPLAYGAIN_REFERENCE_DB - result.gain_db);
                    file.clipping = result.peak >= 1.0;
                    let gain = self.target_volume - REPLAYGAIN_REFERENCE_DB + result.gain_db;
                    file.track_gain = Some(gain);
                    file.track_clip = Self::would_clip(result.peak, gain);
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
        self.status_message = if errors > 0 {
            format!("Analyzed {} file(s), {} error(s)", analyzed, errors)
        } else {
            format!("Analyzed {} file(s)", analyzed)
        };
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

        match replaygain::analyze_album(&paths) {
            Ok(result) => {
                let album_gain =
                    self.target_volume - REPLAYGAIN_REFERENCE_DB + result.album_gain_db;

                for (i, file) in self.files.iter_mut().enumerate() {
                    if let Some(track_result) = result.tracks.get(i) {
                        // Display volume relative to ReplayGain reference (89 dB) for MP3Gain compatibility
                        file.volume = Some(REPLAYGAIN_REFERENCE_DB - track_result.gain_db);
                        file.clipping = track_result.peak >= 1.0;
                        let track_gain =
                            self.target_volume - REPLAYGAIN_REFERENCE_DB + track_result.gain_db;
                        file.track_gain = Some(track_gain);
                        file.track_clip = Self::would_clip(track_result.peak, track_gain);
                    }
                    // Display album volume relative to ReplayGain reference (89 dB) for MP3Gain compatibility
                    file.album_volume = Some(REPLAYGAIN_REFERENCE_DB - result.album_gain_db);
                    file.album_gain = Some(album_gain);
                    file.album_clip = Self::would_clip(result.album_peak, album_gain);
                    file.status = FileStatus::Analyzed;
                }
                self.status_message =
                    format!("Album analysis complete ({} tracks)", self.files.len());
            }
            Err(e) => {
                self.status_message = format!("Album analysis failed: {}", e);
            }
        }

        self.total_progress = 1.0;
        self.is_processing = false;
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
        self.status_message = if errors > 0 {
            format!(
                "Applied track gain to {} file(s), {} error(s)",
                applied, errors
            )
        } else {
            format!("Applied track gain to {} file(s)", applied)
        };
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
        self.status_message = if errors > 0 {
            format!(
                "Applied album gain to {} file(s), {} error(s)",
                applied, errors
            )
        } else {
            format!("Applied album gain to {} file(s)", applied)
        };
    }
}

impl eframe::App for Mp3rgainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::ui::render(self, ctx);
    }
}
