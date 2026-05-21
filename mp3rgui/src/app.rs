use crate::worker::{self, ApplyJob, ApplyOptionsUi, UndoJob, WorkerEvent, WorkerHandle};
use mp3rgain::replaygain::{self, ReplayGainResult, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::{db_to_steps, AacAlbumInfo};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;

#[derive(Default, Clone, PartialEq)]
pub enum FileStatus {
    #[default]
    Pending,
    Analyzing,
    Analyzed,
    Applying,
    Undoing,
    Done,
    NoChangesToUndo,
    Error(String),
}

impl FileStatus {
    pub fn as_str(&self) -> &str {
        match self {
            FileStatus::Pending => "",
            FileStatus::Analyzing => "Analyzing...",
            FileStatus::Analyzed => "OK",
            FileStatus::Applying => "Applying...",
            FileStatus::Undoing => "Undoing...",
            FileStatus::Done => "Done",
            FileStatus::NoChangesToUndo => "Nothing to undo",
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
    /// Cached per-track ReplayGain analysis. Required by `apply_with_options`
    /// for the peak-based clipping check and for writing
    /// `replaygain_track_*` tags on apply.
    pub track_result: Option<ReplayGainResult>,
}

/// What kind of work the active worker is doing — drives messaging and
/// final-event handling.
#[derive(Clone, Copy, PartialEq)]
enum WorkerKind {
    TrackAnalysis,
    AlbumAnalysis,
    TrackApply,
    AlbumApply,
    Undo,
}

pub struct Mp3rgainApp {
    pub files: Vec<FileEntry>,
    pub target_volume: f64,
    pub selected_indices: Vec<usize>,
    pub total_progress: f32,
    pub is_processing: bool,
    pub status_message: String,
    /// Most-recent album analysis. Feeds `replaygain_album_*` tag fields
    /// when `apply_album_gain` runs.
    pub album_info: Option<AacAlbumInfo>,
    /// User-toggleable apply flags surfaced in the Options panel.
    pub apply_options: ApplyOptionsUi,

    /// Active worker thread + its mpsc receiver and cancel flag.
    /// `None` when nothing is running.
    worker: Option<WorkerHandle>,
    /// What kind of work the active worker is doing.
    worker_kind: Option<WorkerKind>,
    /// Counter incremented as worker emits `FileStart` events — drives the
    /// progress bar without needing to know the total in advance.
    started_files: usize,
    /// Total files the worker was launched against (denominator for the
    /// progress bar).
    total_files_in_job: usize,
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
            album_info: None,
            apply_options: ApplyOptionsUi::default(),
            worker: None,
            worker_kind: None,
            started_files: 0,
            total_files_in_job: 0,
        }
    }

    pub fn add_files(&mut self, paths: Vec<PathBuf>) {
        if self.is_processing {
            return;
        }
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
        if self.is_processing {
            return;
        }
        let paths_to_add = mp3rgain::collect_audio_files(&folder, recursive).unwrap_or_default();
        self.add_files(paths_to_add);
    }

    pub fn remove_selected(&mut self) {
        if self.is_processing {
            return;
        }
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
        if self.is_processing {
            return;
        }
        self.files.clear();
        self.selected_indices.clear();
        self.album_info = None;
    }

    pub fn cancel_current_work(&mut self) {
        if let Some(worker) = self.worker.as_ref() {
            worker.request_cancel();
            self.status_message = "Cancelling...".to_string();
        }
    }

    pub fn start_analyze_tracks(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing || !replaygain::is_available() {
            if !replaygain::is_available() {
                self.status_message = "ReplayGain feature not available".to_string();
            }
            return;
        }

        // Track-only analysis invalidates any previously-computed album info.
        self.album_info = None;
        for file in &mut self.files {
            file.status = FileStatus::Pending;
        }

        let jobs: Vec<(usize, PathBuf)> = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (i, f.path.clone()))
            .collect();
        self.begin_worker(
            WorkerKind::TrackAnalysis,
            jobs.len(),
            worker::spawn_track_analysis(ctx.clone(), jobs),
        );
    }

    pub fn start_analyze_album(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing || !replaygain::is_available() {
            if !replaygain::is_available() {
                self.status_message = "ReplayGain feature not available".to_string();
            }
            return;
        }

        self.album_info = None;
        for file in &mut self.files {
            file.status = FileStatus::Pending;
        }

        let jobs: Vec<(usize, PathBuf)> = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (i, f.path.clone()))
            .collect();
        self.begin_worker(
            WorkerKind::AlbumAnalysis,
            jobs.len(),
            worker::spawn_album_analysis(ctx.clone(), jobs),
        );
    }

    pub fn start_apply_track_gain(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing {
            return;
        }

        let jobs: Vec<ApplyJob> = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, f)| {
                f.track_gain.map(|gain_db| ApplyJob {
                    idx,
                    path: f.path.clone(),
                    steps: db_to_steps(gain_db),
                    track_result: f.track_result.clone(),
                    album_info: None,
                })
            })
            .collect();

        if jobs.is_empty() {
            self.status_message = "No track gain values — run Track Analysis first".to_string();
            return;
        }

        for &job_idx in jobs.iter().map(|j| &j.idx) {
            self.files[job_idx].status = FileStatus::Pending;
        }
        let count = jobs.len();
        let ui_opts = self.apply_options;
        self.begin_worker(
            WorkerKind::TrackApply,
            count,
            worker::spawn_apply(ctx.clone(), jobs, "track gain", ui_opts),
        );
    }

    /// Undo gain changes on selected files (or all files when no selection).
    /// Library calls dispatch internally on file format and `use_id3v2`.
    pub fn start_undo(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing {
            return;
        }

        let indices: Vec<usize> = if self.selected_indices.is_empty() {
            (0..self.files.len()).collect()
        } else {
            let mut v = self.selected_indices.clone();
            v.sort_unstable();
            v.dedup();
            v
        };

        let jobs: Vec<UndoJob> = indices
            .iter()
            .filter_map(|&idx| {
                self.files.get(idx).map(|f| UndoJob {
                    idx,
                    path: f.path.clone(),
                })
            })
            .collect();

        if jobs.is_empty() {
            return;
        }

        for &job_idx in jobs.iter().map(|j| &j.idx) {
            self.files[job_idx].status = FileStatus::Pending;
        }
        let count = jobs.len();
        let ui_opts = self.apply_options;
        self.begin_worker(
            WorkerKind::Undo,
            count,
            worker::spawn_undo(ctx.clone(), jobs, ui_opts),
        );
    }

    pub fn start_apply_album_gain(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing {
            return;
        }

        let album_info = self.album_info;
        let jobs: Vec<ApplyJob> = self
            .files
            .iter()
            .enumerate()
            .filter_map(|(idx, f)| {
                f.album_gain.map(|gain_db| ApplyJob {
                    idx,
                    path: f.path.clone(),
                    steps: db_to_steps(gain_db),
                    track_result: f.track_result.clone(),
                    album_info,
                })
            })
            .collect();

        if jobs.is_empty() {
            self.status_message = "No album gain values — run Album Analysis first".to_string();
            return;
        }

        for &job_idx in jobs.iter().map(|j| &j.idx) {
            self.files[job_idx].status = FileStatus::Pending;
        }
        let count = jobs.len();
        let ui_opts = self.apply_options;
        self.begin_worker(
            WorkerKind::AlbumApply,
            count,
            worker::spawn_apply(ctx.clone(), jobs, "album gain", ui_opts),
        );
    }

    fn begin_worker(&mut self, kind: WorkerKind, total: usize, handle: WorkerHandle) {
        self.worker = Some(handle);
        self.worker_kind = Some(kind);
        self.is_processing = true;
        self.total_progress = 0.0;
        self.status_message = match kind {
            WorkerKind::TrackAnalysis => "Analyzing tracks...".to_string(),
            WorkerKind::AlbumAnalysis => "Analyzing album...".to_string(),
            WorkerKind::TrackApply => "Applying track gain...".to_string(),
            WorkerKind::AlbumApply => "Applying album gain...".to_string(),
            WorkerKind::Undo => "Undoing gain changes...".to_string(),
        };
        self.started_files = 0;
        self.total_files_in_job = total;
    }

    /// Drain pending worker events into UI state. Called from `update()`.
    pub fn pump_worker_events(&mut self) {
        // Drain into a local Vec first so we can hand `&mut self` to
        // `apply_event` without conflicting with the receiver borrow.
        let mut events = Vec::new();
        let mut worker_finished = false;
        if let Some(worker) = self.worker.as_ref() {
            loop {
                match worker.rx.try_recv() {
                    Ok(event) => events.push(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        worker_finished = true;
                        break;
                    }
                }
            }
        }
        for event in events {
            self.apply_event(event);
        }
        if worker_finished {
            self.worker = None;
            self.worker_kind = None;
            self.is_processing = false;
        }
    }

    fn apply_event(&mut self, event: WorkerEvent) {
        match event {
            WorkerEvent::FileStart { idx } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = match self.worker_kind {
                        Some(WorkerKind::TrackApply) | Some(WorkerKind::AlbumApply) => {
                            FileStatus::Applying
                        }
                        Some(WorkerKind::Undo) => FileStatus::Undoing,
                        _ => FileStatus::Analyzing,
                    };
                }
                self.started_files = self.started_files.saturating_add(1);
                self.bump_progress();
            }
            WorkerEvent::TrackAnalyzed { idx, result } => {
                let target = self.target_volume;
                if let Some(file) = self.files.get_mut(idx) {
                    Self::populate_track_analysis(file, &result, target);
                    file.track_result = Some(result);
                    file.status = FileStatus::Analyzed;
                }
            }
            WorkerEvent::TrackAnalysisFailed { idx, message } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.track_result = None;
                    file.status = FileStatus::Error(message);
                }
            }
            WorkerEvent::AlbumAnalysisDone {
                successful,
                failures,
                album_info,
            } => {
                let target = self.target_volume;
                let album_gain = target - REPLAYGAIN_REFERENCE_DB + album_info.album_gain_db;
                let album_volume = REPLAYGAIN_REFERENCE_DB - album_info.album_gain_db;
                let album_clip = Self::would_clip(album_info.album_peak, album_gain);

                for (idx, track_result) in successful {
                    if let Some(file) = self.files.get_mut(idx) {
                        Self::populate_track_analysis(file, &track_result, target);
                        file.track_result = Some(track_result);
                        file.album_volume = Some(album_volume);
                        file.album_gain = Some(album_gain);
                        file.album_clip = album_clip;
                        file.status = FileStatus::Analyzed;
                    }
                }
                for (idx, msg) in failures {
                    if let Some(file) = self.files.get_mut(idx) {
                        file.track_result = None;
                        file.status = FileStatus::Error(msg);
                    }
                }

                self.album_info = Some(album_info);
            }
            WorkerEvent::AlbumAnalysisFailed(msg) => {
                self.status_message = format!("Album analysis failed: {}", msg);
            }
            WorkerEvent::FileApplied { idx } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::Done;
                }
            }
            WorkerEvent::FileApplyFailed { idx, message } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::Error(message);
                }
            }
            WorkerEvent::FileUndone { idx } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::Done;
                }
            }
            WorkerEvent::FileUndoSkipped { idx } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::NoChangesToUndo;
                }
            }
            WorkerEvent::Cancelled => {
                self.status_message = "Cancelled".to_string();
                self.total_progress = 0.0;
            }
            WorkerEvent::Done { message } => {
                self.status_message = message;
                self.total_progress = 1.0;
            }
        }
    }

    fn bump_progress(&mut self) {
        if self.total_files_in_job == 0 {
            return;
        }
        self.total_progress = (self.started_files as f32) / (self.total_files_in_job as f32);
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

    fn would_clip(peak: f64, gain_db: f64) -> bool {
        let gain_linear = 10.0_f64.powf(gain_db / 20.0);
        peak * gain_linear > 1.0
    }
}

impl eframe::App for Mp3rgainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_worker_events();
        crate::ui::render(self, ctx);
    }
}
