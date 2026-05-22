use crate::worker::{
    self, ApplyJob, ApplyOptionsUi, CheckTagsJob, DeleteTagsJob, StoredTagsView, UndoJob,
    WorkerEvent, WorkerHandle,
};
use mp3rgain::replaygain::{self, ReplayGainResult, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::{db_to_steps, AacAlbumInfo, Channel};
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
    /// Dry-run apply finished without touching the file. `steps` is what
    /// would have been applied; `clipping_prevented` indicates the cap.
    DryRunPredicted {
        steps: i32,
        clipping_prevented: bool,
    },
    Error(String),
}

impl FileStatus {
    /// Short label for the Status column. For dynamic variants only the
    /// category is returned; the detail goes in a tooltip / extra label.
    pub fn label(&self) -> String {
        match self {
            FileStatus::Pending => "".into(),
            FileStatus::Analyzing => "Analyzing...".into(),
            FileStatus::Analyzed => "OK".into(),
            FileStatus::Applying => "Applying...".into(),
            FileStatus::Undoing => "Undoing...".into(),
            FileStatus::Done => "Done".into(),
            FileStatus::NoChangesToUndo => "Nothing to undo".into(),
            FileStatus::DryRunPredicted {
                steps,
                clipping_prevented,
            } => {
                if *clipping_prevented {
                    format!("Dry run: +{} steps (capped)", steps)
                } else {
                    format!("Dry run: {:+} steps", steps)
                }
            }
            FileStatus::Error(_) => "Error".into(),
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
    /// Pre-existing ReplayGain / undo tags read from the file, populated by
    /// the "Check Stored Tags" action. `None` = not scanned yet.
    pub stored_tags: Option<StoredTagsView>,
}

/// State for the "Apply Manual Gain" modal. `open` toggles visibility;
/// `steps` is preserved across closes so the next open shows the same value.
pub struct ManualGainModal {
    pub open: bool,
    pub steps: i32,
}

impl Default for ManualGainModal {
    fn default() -> Self {
        Self {
            open: false,
            steps: 0,
        }
    }
}

/// State for the "Apply Channel Gain" modal (`-l`).
pub struct ChannelGainModal {
    pub open: bool,
    pub channel: Channel,
    pub steps: i32,
}

impl Default for ChannelGainModal {
    fn default() -> Self {
        Self {
            open: false,
            channel: Channel::Left,
            steps: 0,
        }
    }
}

/// Modifier-combo classification for a table-row click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClickMode {
    /// Plain click. Replaces the selection with `idx`.
    Replace,
    /// Cmd / Ctrl + click. Toggles `idx` in/out of the selection.
    Toggle,
    /// Shift + click. Selects the inclusive range anchor..=idx, replacing.
    Range,
    /// Shift + Cmd / Ctrl + click. Adds the inclusive range to the existing
    /// selection (set union).
    RangeAdd,
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
    CheckTags,
    MaxAmplitude,
    DeleteTags,
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

    /// Set true while the "Delete stored tags" confirmation modal is up.
    /// The destructive worker is only spawned after the user confirms.
    pub confirm_delete_tags: bool,

    /// Manual-gain modal state. Lives across opens so the user can tweak
    /// the same value across runs.
    pub manual_gain_modal: ManualGainModal,

    /// Channel-gain modal state.
    pub channel_gain_modal: ChannelGainModal,

    /// Last row the user clicked on (without Shift). Acts as the anchor for
    /// subsequent Shift+click range selections, the way Finder / Explorer
    /// behave. `None` when no anchor has been set yet (fresh table, or after
    /// a full clear).
    pub selection_anchor: Option<usize>,

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
            confirm_delete_tags: false,
            manual_gain_modal: ManualGainModal::default(),
            channel_gain_modal: ChannelGainModal::default(),
            selection_anchor: None,
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
        self.selection_anchor = None;
        self.album_info = None;
    }

    /// Replace the current selection with every file in the table. Used by
    /// the Cmd+A / Ctrl+A shortcut.
    pub fn select_all(&mut self) {
        if self.is_processing || self.files.is_empty() {
            return;
        }
        self.selected_indices = (0..self.files.len()).collect();
        // Anchor at the first row so a follow-up Shift+click extends a sane
        // range. (Without this, an Esc-then-Cmd-A flow would lose the
        // anchor and Shift+click would behave like a plain click.)
        self.selection_anchor = Some(0);
    }

    /// Drop the current selection. Used by Escape (when no modal is open).
    pub fn clear_selection(&mut self) {
        self.selected_indices.clear();
        self.selection_anchor = None;
    }

    /// Apply a click on `idx` with the given modifier combination. Anchor
    /// management follows Finder: a plain click moves the anchor, a Shift
    /// click extends from the anchor without moving it, a Cmd/Ctrl click
    /// toggles a single row and updates the anchor.
    pub fn click_row(&mut self, idx: usize, mode: ClickMode) {
        if idx >= self.files.len() {
            return;
        }
        match mode {
            ClickMode::Replace => {
                self.selected_indices.clear();
                self.selected_indices.push(idx);
                self.selection_anchor = Some(idx);
            }
            ClickMode::Toggle => {
                if let Some(pos) = self.selected_indices.iter().position(|&i| i == idx) {
                    self.selected_indices.remove(pos);
                } else {
                    self.selected_indices.push(idx);
                }
                self.selection_anchor = Some(idx);
            }
            ClickMode::Range => {
                let anchor = self.selection_anchor.unwrap_or(idx);
                let (lo, hi) = if anchor <= idx {
                    (anchor, idx)
                } else {
                    (idx, anchor)
                };
                self.selected_indices = (lo..=hi).collect();
                // Anchor stays put.
            }
            ClickMode::RangeAdd => {
                let anchor = self.selection_anchor.unwrap_or(idx);
                let (lo, hi) = if anchor <= idx {
                    (anchor, idx)
                } else {
                    (idx, anchor)
                };
                for i in lo..=hi {
                    if !self.selected_indices.contains(&i) {
                        self.selected_indices.push(i);
                    }
                }
            }
        }
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
                    channel: None,
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

    /// Selected files, or all loaded files when nothing is selected.
    /// Returned indices are sorted and deduplicated.
    pub fn target_indices(&self) -> Vec<usize> {
        if self.selected_indices.is_empty() {
            (0..self.files.len()).collect()
        } else {
            let mut v = self.selected_indices.clone();
            v.sort_unstable();
            v.dedup();
            v
        }
    }

    /// `-s d`: delete stored RG / undo tags from selected files. Destructive,
    /// so the UI funnels the click through `confirm_delete_tags` first and
    /// this is only called after the user confirms.
    pub fn start_delete_tags(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing {
            return;
        }

        let indices = self.target_indices();
        let jobs: Vec<DeleteTagsJob> = indices
            .iter()
            .filter_map(|&idx| {
                self.files.get(idx).map(|f| DeleteTagsJob {
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
        let use_id3v2 = self.apply_options.use_id3v2;
        let preserve = self.apply_options.preserve_timestamp;
        self.begin_worker(
            WorkerKind::DeleteTags,
            count,
            worker::spawn_delete_tags(ctx.clone(), jobs, use_id3v2, preserve),
        );
    }

    /// `-x`: scan each file for its max amplitude / headroom without any
    /// ReplayGain decoding. Faster than Track Analysis.
    pub fn start_find_max_amplitude(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing {
            return;
        }
        for file in &mut self.files {
            file.status = FileStatus::Pending;
        }
        let jobs: Vec<(usize, PathBuf)> = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| (i, f.path.clone()))
            .collect();
        let count = jobs.len();
        self.begin_worker(
            WorkerKind::MaxAmplitude,
            count,
            worker::spawn_find_max_amplitude(ctx.clone(), jobs),
        );
    }

    /// Scan every loaded file for existing ReplayGain / undo tags and
    /// populate `FileEntry::stored_tags`. Read-only.
    pub fn start_check_stored_tags(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing {
            return;
        }

        let jobs: Vec<CheckTagsJob> = self
            .files
            .iter()
            .enumerate()
            .map(|(idx, f)| CheckTagsJob {
                idx,
                path: f.path.clone(),
            })
            .collect();
        let count = jobs.len();
        let use_id3v2 = self.apply_options.use_id3v2;
        self.begin_worker(
            WorkerKind::CheckTags,
            count,
            worker::spawn_check_stored_tags(ctx.clone(), jobs, use_id3v2),
        );
    }

    /// `-g`: apply a fixed step count to the selected files (or all when no
    /// selection). Bypasses ReplayGain — `track_result` / `album_info` are
    /// left None so `apply_with_options` uses the headroom-based clipping
    /// check.
    pub fn start_apply_manual_gain(&mut self, ctx: &egui::Context, steps: i32) {
        if self.files.is_empty() || self.is_processing || steps == 0 {
            return;
        }

        let jobs: Vec<ApplyJob> = self
            .target_indices()
            .iter()
            .filter_map(|&idx| {
                self.files.get(idx).map(|f| ApplyJob {
                    idx,
                    path: f.path.clone(),
                    steps,
                    track_result: None,
                    album_info: None,
                    channel: None,
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
            WorkerKind::TrackApply,
            count,
            worker::spawn_apply(ctx.clone(), jobs, "manual gain", ui_opts),
        );
    }

    /// `-l`: apply gain to a single channel of the targeted files. AAC files
    /// are silently skipped (channel gain is MP3-only).
    pub fn start_apply_channel_gain(&mut self, ctx: &egui::Context, channel: Channel, steps: i32) {
        if self.files.is_empty() || self.is_processing || steps == 0 {
            return;
        }

        let jobs: Vec<ApplyJob> = self
            .target_indices()
            .iter()
            .filter_map(|&idx| self.files.get(idx).map(|f| (idx, f)))
            .filter(|(_, f)| !mp3rgain::mp4meta::is_aac_file(&f.path))
            .map(|(idx, f)| ApplyJob {
                idx,
                path: f.path.clone(),
                steps,
                track_result: None,
                album_info: None,
                channel: Some(channel),
            })
            .collect();
        if jobs.is_empty() {
            self.status_message =
                "Channel gain only applies to MP3 — selection had no eligible files".into();
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
            worker::spawn_apply(ctx.clone(), jobs, "channel gain", ui_opts),
        );
    }

    /// Undo gain changes on selected files (or all files when no selection).
    /// Library calls dispatch internally on file format and `use_id3v2`.
    pub fn start_undo(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing {
            return;
        }

        let jobs: Vec<UndoJob> = self
            .target_indices()
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
                    channel: None,
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
            WorkerKind::CheckTags => "Checking stored tags...".to_string(),
            WorkerKind::MaxAmplitude => "Finding max amplitude...".to_string(),
            WorkerKind::DeleteTags => "Deleting stored tags...".to_string(),
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
                    match self.worker_kind {
                        Some(WorkerKind::TrackApply)
                        | Some(WorkerKind::AlbumApply)
                        | Some(WorkerKind::DeleteTags) => {
                            file.status = FileStatus::Applying;
                        }
                        Some(WorkerKind::Undo) => {
                            file.status = FileStatus::Undoing;
                        }
                        // Tag scan is read-only; don't disturb the
                        // user-visible status (e.g. Analyzed) for the row.
                        Some(WorkerKind::CheckTags) => {}
                        Some(WorkerKind::MaxAmplitude) => {
                            file.status = FileStatus::Analyzing;
                        }
                        _ => file.status = FileStatus::Analyzing,
                    }
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
                    // File contents changed; cached tag snapshot is stale.
                    file.stored_tags = None;
                }
            }
            WorkerEvent::FileApplyDryRun {
                idx,
                actual_steps,
                clipping_prevented,
            } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::DryRunPredicted {
                        steps: actual_steps,
                        clipping_prevented,
                    };
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
                    file.stored_tags = None;
                }
            }
            WorkerEvent::FileUndoSkipped { idx } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::NoChangesToUndo;
                }
            }
            WorkerEvent::StoredTagsRead { idx, view } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.stored_tags = Some(view);
                }
            }
            WorkerEvent::MaxAmplitudeFound {
                idx,
                peak,
                headroom_db,
            } => {
                if let Some(file) = self.files.get_mut(idx) {
                    // Reuse the existing Volume column to show headroom.
                    // ReplayGain volume and max-amp headroom are different
                    // measures but both express "loudness ceiling"; the
                    // Volume column header tooltip explains both.
                    file.volume = headroom_db;
                    file.clipping = peak >= 1.0;
                    // Max amplitude is not a ReplayGain analysis — clear
                    // any prior RG-derived gain so the row doesn't claim
                    // an out-of-date target.
                    file.track_gain = None;
                    file.track_clip = false;
                    file.track_result = None;
                    file.status = FileStatus::Analyzed;
                }
            }
            WorkerEvent::MaxAmplitudeFailed { idx, message } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::Error(message);
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
