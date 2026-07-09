use crate::worker::{
    self, ApplyJob, ApplyOptionsUi, CheckTagsJob, DeleteTagsJob, StoredTagsView, UndoJob,
    WorkerEvent, WorkerHandle,
};
use mp3rgain::replaygain::{self, ReplayGainResult, REPLAYGAIN_REFERENCE_DB};
use mp3rgain::{db_to_linear, db_to_steps, would_clip, AacAlbumInfo, Channel};
use std::collections::HashSet;
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
    /// Album-level ReplayGain summary for the folder this file belongs to,
    /// populated by Album Analysis. Used to write `replaygain_album_*` tags
    /// on Apply Album Gain. Per-file (not global) so adding multiple folders
    /// treats each as its own album (issue #159).
    pub album_info: Option<AacAlbumInfo>,
    /// Pre-existing ReplayGain / undo tags read from the file, populated by
    /// the "Check Stored Tags" action. `None` = not scanned yet.
    pub stored_tags: Option<StoredTagsView>,
    /// Peaks parsed from stored ReplayGain tags on import (issue #233).
    /// Fallback for clip recomputation when `track_result` is None.
    pub stored_track_peak: Option<f64>,
    pub stored_album_peak: Option<f64>,
}

/// State for the "Apply Manual Gain" modal. `open` toggles visibility;
/// `steps` is preserved across closes so the next open shows the same value.
#[derive(Default)]
pub struct ManualGainModal {
    pub open: bool,
    pub steps: i32,
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

/// Which table column the rows are currently sorted by (issue #167).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Filename,
    Volume,
    TrackGain,
    AlbumVolume,
    AlbumGain,
    Status,
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
    /// Read-only stored-tag scan triggered automatically when files are
    /// imported. Unlike `CheckTags`, it also fills the Volume / Gain columns
    /// from any existing ReplayGain tags so already-analyzed files show their
    /// values without re-scanning (issue #203).
    ImportScan,
}

/// Settings persisted across launches (issue #202). Window geometry and egui
/// memory (e.g. table column widths) are persisted by eframe automatically;
/// this only carries the app-specific toggles.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedSettings {
    apply_options: ApplyOptionsUi,
    target_volume: f64,
    // `serde(default)` so settings saved by older builds (which lack these
    // keys) still deserialize instead of falling back to full defaults.
    #[serde(default)]
    show_filename_only: bool,
    #[serde(default)]
    single_album: bool,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            apply_options: ApplyOptionsUi::default(),
            target_volume: 89.0,
            show_filename_only: false,
            single_album: false,
        }
    }
}

/// Storage key for [`PersistedSettings`].
const SETTINGS_KEY: &str = "mp3rgui_settings";

pub struct Mp3rgainApp {
    pub files: Vec<FileEntry>,
    pub target_volume: f64,
    pub selected_indices: Vec<usize>,
    pub total_progress: f32,
    pub is_processing: bool,
    pub status_message: String,
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

    /// Last `target_volume` we propagated into the file rows. Used by
    /// `recompute_targets_if_changed` to detect Target edits and refresh
    /// the gain columns without rerunning analysis (issue #161 item 1).
    last_target_volume: f64,

    /// Active sort column, or `None` for insertion order (issue #167).
    pub sort_column: Option<SortColumn>,
    /// Direction of the active sort. Ignored when `sort_column` is `None`.
    pub sort_descending: bool,

    /// Cached display order, recomputed lazily when `display_order_dirty`
    /// is set. The old per-frame re-sort allocated per comparison and
    /// dominated frame time on large tables (issue #190).
    display_order_cache: Vec<usize>,
    /// Set whenever the sort key or any row data changes.
    display_order_dirty: bool,

    /// File indices added since the last import scan, awaiting an automatic
    /// stored-tag read (issue #203). Drained by `start_import_scan` on the
    /// next idle frame.
    pending_import_scan: Vec<usize>,

    /// Paths dropped (or otherwise added) while a worker was running. They
    /// would previously be silently discarded (issue #235); instead they are
    /// queued here and added when the worker finishes.
    pending_drops: Vec<PathBuf>,

    /// When true, the Path/File column shows only the file name; the full
    /// path stays available on hover (issue #223). Off = full path, the
    /// pre-existing behavior.
    pub show_filename_only: bool,

    /// When true, Album Analysis / Apply Album Gain treat every loaded file
    /// as a single album regardless of directory (issue #224). Off = each
    /// folder is its own album, the default (issue #159).
    pub single_album: bool,
}

impl Mp3rgainApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Restore the user's checkboxes / target from the previous session
        // (issue #202). Window geometry + column widths are handled by eframe.
        let settings: PersistedSettings = cc
            .storage
            .and_then(|s| eframe::get_value(s, SETTINGS_KEY))
            .unwrap_or_default();
        Self {
            files: Vec::new(),
            target_volume: settings.target_volume,
            selected_indices: Vec::new(),
            total_progress: 0.0,
            is_processing: false,
            status_message: String::new(),
            apply_options: settings.apply_options,
            confirm_delete_tags: false,
            manual_gain_modal: ManualGainModal::default(),
            channel_gain_modal: ChannelGainModal::default(),
            selection_anchor: None,
            worker: None,
            worker_kind: None,
            started_files: 0,
            total_files_in_job: 0,
            last_target_volume: settings.target_volume,
            sort_column: None,
            sort_descending: false,
            display_order_cache: Vec::new(),
            display_order_dirty: true,
            pending_import_scan: Vec::new(),
            pending_drops: Vec::new(),
            show_filename_only: settings.show_filename_only,
            single_album: settings.single_album,
        }
    }

    /// Toggle the sort state when the given header is clicked. First click on
    /// a column sorts ascending; subsequent clicks cycle desc → unsorted.
    pub fn toggle_sort(&mut self, column: SortColumn) {
        match self.sort_column {
            Some(active) if active == column => {
                if !self.sort_descending {
                    self.sort_descending = true;
                } else {
                    self.sort_column = None;
                    self.sort_descending = false;
                }
            }
            _ => {
                self.sort_column = Some(column);
                self.sort_descending = false;
            }
        }
        self.display_order_dirty = true;
    }

    /// Indices into `self.files` in current display (sort) order, served
    /// from the cache. The returned Vec is a clone so the table can iterate
    /// it while mutably borrowing `self` for click handling.
    pub fn display_order(&mut self) -> Vec<usize> {
        if self.display_order_dirty {
            self.display_order_cache = self.compute_display_order();
            self.display_order_dirty = false;
        }
        self.display_order_cache.clone()
    }

    /// Sort `0..files.len()` by the active column. String columns
    /// precompute one key per row instead of allocating lowercased copies
    /// in every comparison (issue #190).
    fn compute_display_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.files.len()).collect();
        let Some(col) = self.sort_column else {
            return order;
        };
        let desc = self.sort_descending;
        match col {
            SortColumn::Filename => {
                let keys: Vec<String> = self
                    .files
                    .iter()
                    .map(|f| f.filename.to_lowercase())
                    .collect();
                order.sort_by(|&a, &b| cmp_str(&keys[a], &keys[b], desc));
            }
            SortColumn::Status => {
                let keys: Vec<String> = self
                    .files
                    .iter()
                    .map(|f| f.status.label().to_lowercase())
                    .collect();
                order.sort_by(|&a, &b| cmp_str(&keys[a], &keys[b], desc));
            }
            SortColumn::Volume => order
                .sort_by(|&a, &b| cmp_opt_f64(self.files[a].volume, self.files[b].volume, desc)),
            SortColumn::TrackGain => order.sort_by(|&a, &b| {
                cmp_opt_f64(self.files[a].track_gain, self.files[b].track_gain, desc)
            }),
            SortColumn::AlbumVolume => order.sort_by(|&a, &b| {
                cmp_opt_f64(self.files[a].album_volume, self.files[b].album_volume, desc)
            }),
            SortColumn::AlbumGain => order.sort_by(|&a, &b| {
                cmp_opt_f64(self.files[a].album_gain, self.files[b].album_gain, desc)
            }),
        }
        order
    }

    /// Detect Target edits and shift each row's track_gain / album_gain by
    /// the delta. Cheap (just arithmetic on cached values), so it can run
    /// every frame (issue #161 item 1). Clipping flags are recomputed
    /// against the cached pre-apply peak.
    pub fn recompute_targets_if_changed(&mut self) {
        if (self.target_volume - self.last_target_volume).abs() < f64::EPSILON {
            return;
        }
        self.display_order_dirty = true;
        let delta = self.target_volume - self.last_target_volume;
        for file in &mut self.files {
            if let Some(g) = file.track_gain {
                file.track_gain = Some(g + delta);
            }
            if let Some(g) = file.album_gain {
                file.album_gain = Some(g + delta);
            }
            let analyzed_peak = file.track_result.as_ref().map(|t| t.peak());
            if let Some(peak) = analyzed_peak.or(file.stored_track_peak) {
                file.track_clip = file
                    .track_gain
                    .map(|g| would_clip(peak, g))
                    .unwrap_or(false);
            }
            if let Some(peak) = analyzed_peak.or(file.stored_album_peak) {
                file.album_clip = file
                    .album_gain
                    .map(|g| would_clip(peak, g))
                    .unwrap_or(false);
            }
        }
        self.last_target_volume = self.target_volume;
    }

    pub fn add_files(&mut self, paths: Vec<PathBuf>) {
        if self.is_processing {
            if !paths.is_empty() {
                self.pending_drops.extend(paths);
                self.status_message = format!(
                    "{} file(s) queued, will be added when processing finishes",
                    self.pending_drops.len()
                );
            }
            return;
        }
        let first_new = self.files.len();
        let mut added = 0;
        let mut skipped = 0;

        // Set lookup instead of scanning `files` per added path, which is
        // O(n²) when dropping a large folder.
        let mut known: HashSet<PathBuf> = self.files.iter().map(|f| f.path.clone()).collect();

        for path in paths {
            if mp3rgain::is_supported_audio_path(&path) && path.is_file() {
                if !known.insert(path.clone()) {
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

        if added > 0 {
            self.display_order_dirty = true;
            // Queue the new rows for an automatic existing-tag read (issue
            // #203). The scan starts on the next idle frame.
            self.pending_import_scan.extend(first_new..self.files.len());
        }
        if skipped > 0 {
            self.status_message =
                format!("Added {} file(s), {} duplicate(s) skipped", added, skipped);
        } else if added > 0 {
            self.status_message = format!("Added {} file(s)", added);
        }
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
        // Removing rows shifts indices, so the anchor and any queued import
        // scan are stale.
        self.selection_anchor = None;
        self.pending_import_scan.clear();
        self.display_order_dirty = true;
    }

    pub fn clear_files(&mut self) {
        if self.is_processing {
            return;
        }
        self.files.clear();
        self.selected_indices.clear();
        self.selection_anchor = None;
        self.pending_import_scan.clear();
        self.display_order_dirty = true;
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
                // Range must follow visible (display) order so that a sorted
                // table still extends "between these two visible rows"
                // (issue #167).
                self.selected_indices = self.range_in_display_order(anchor, idx);
                // Anchor stays put.
            }
            ClickMode::RangeAdd => {
                let anchor = self.selection_anchor.unwrap_or(idx);
                for i in self.range_in_display_order(anchor, idx) {
                    if !self.selected_indices.contains(&i) {
                        self.selected_indices.push(i);
                    }
                }
            }
        }
    }

    /// File indices spanning the visible range between two file indices,
    /// inclusive. The two endpoints are translated to display positions via
    /// `compute_display_order`, the inclusive range is collected in display
    /// order, then mapped back to file indices.
    fn range_in_display_order(&self, anchor: usize, idx: usize) -> Vec<usize> {
        let order = self.compute_display_order();
        let anchor_pos = order.iter().position(|&i| i == anchor);
        let idx_pos = order.iter().position(|&i| i == idx);
        match (anchor_pos, idx_pos) {
            (Some(a), Some(b)) => {
                let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                order[lo..=hi].to_vec()
            }
            _ => vec![idx],
        }
    }

    pub fn cancel_current_work(&mut self) {
        if let Some(worker) = self.worker.as_ref() {
            worker.request_cancel();
            self.status_message = "Cancelling...".to_string();
        }
    }

    /// Open the platform file manager focused on `path` (issue #161 item 4).
    /// Best-effort: failures land in `status_message` so the user gets a hint
    /// instead of a silent no-op.
    pub fn reveal_in_file_manager(&mut self, path: &Path) {
        let result = open_in_file_manager(path);
        if let Err(msg) = result {
            self.status_message = format!("Could not open file location: {}", msg);
        }
    }

    pub fn start_analyze_tracks(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing || !replaygain::is_available() {
            if !replaygain::is_available() {
                self.status_message = "ReplayGain feature not available".to_string();
            }
            return;
        }

        // Issue #161: act on the current selection (or all files when nothing
        // is selected). Selected rows go to Pending; unselected rows keep their
        // existing state so partial analyses don't wipe prior results.
        // Issue #159: also clear stale per-file album_info on rescan.
        let targets = self.target_indices();
        for &idx in &targets {
            if let Some(f) = self.files.get_mut(idx) {
                f.status = FileStatus::Pending;
                f.album_info = None;
            }
        }

        let jobs: Vec<(usize, PathBuf)> = targets
            .iter()
            .filter_map(|&i| self.files.get(i).map(|f| (i, f.path.clone())))
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

        // Issue #161: scope to current selection (or all files when none
        // selected). Issue #159: group those targets by parent directory so
        // each folder is treated as its own album.
        let targets = self.target_indices();
        for &idx in &targets {
            if let Some(f) = self.files.get_mut(idx) {
                f.status = FileStatus::Pending;
                f.album_info = None;
            }
        }

        let group_jobs: Vec<Vec<(usize, PathBuf)>> = if self.single_album {
            // Issue #224: treat every target as one album regardless of
            // directory, so multi-disc sets in subfolders share one album
            // gain. A single group produces one album_info for the batch.
            let jobs: Vec<(usize, PathBuf)> = targets
                .iter()
                .filter_map(|&idx| self.files.get(idx).map(|f| (idx, f.path.clone())))
                .collect();
            if jobs.is_empty() {
                Vec::new()
            } else {
                vec![jobs]
            }
        } else {
            // Issue #159: group by parent directory so each folder is its
            // own album.
            let mut groups: std::collections::BTreeMap<PathBuf, Vec<(usize, PathBuf)>> =
                std::collections::BTreeMap::new();
            for &idx in &targets {
                if let Some(f) = self.files.get(idx) {
                    let parent = f
                        .path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(PathBuf::new);
                    groups
                        .entry(parent)
                        .or_default()
                        .push((idx, f.path.clone()));
                }
            }
            groups.into_values().collect()
        };
        let total: usize = group_jobs.iter().map(|g| g.len()).sum();

        self.begin_worker(
            WorkerKind::AlbumAnalysis,
            total,
            worker::spawn_album_analysis(ctx.clone(), group_jobs),
        );
    }

    pub fn start_apply_track_gain(&mut self, ctx: &egui::Context) {
        if self.files.is_empty() || self.is_processing {
            return;
        }

        // Issue #161: act on the current selection (or all files when none
        // selected).
        let targets = self.target_indices();
        let jobs: Vec<ApplyJob> = targets
            .iter()
            .filter_map(|&idx| self.files.get(idx).map(|f| (idx, f)))
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
            worker::spawn_apply(ctx.clone(), jobs, "track gain", ui_opts, false),
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

    /// `-x`: scan each file for its max amplitude / headroom. Decodes the
    /// audio for the true peak but skips the loudness analysis, so it is
    /// lighter than Track Analysis (not decode-free).
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

    /// Read existing ReplayGain tags from files queued by `add_files` and fill
    /// the Volume / Gain columns from them, so already-analyzed files show
    /// their values on import without a full re-scan (issue #203). Reuses the
    /// read-only stored-tag worker; only runs while idle so it never collides
    /// with another job.
    fn start_import_scan(&mut self, ctx: &egui::Context) {
        if self.is_processing || self.pending_import_scan.is_empty() {
            return;
        }
        let indices = std::mem::take(&mut self.pending_import_scan);
        let jobs: Vec<CheckTagsJob> = indices
            .into_iter()
            .filter_map(|idx| {
                self.files.get(idx).map(|f| CheckTagsJob {
                    idx,
                    path: f.path.clone(),
                })
            })
            .collect();
        if jobs.is_empty() {
            return;
        }
        let count = jobs.len();
        let use_id3v2 = self.apply_options.use_id3v2;
        self.begin_worker(
            WorkerKind::ImportScan,
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
            worker::spawn_apply(ctx.clone(), jobs, "manual gain", ui_opts, false),
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
            worker::spawn_apply(ctx.clone(), jobs, "channel gain", ui_opts, false),
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

        // Issue #161: act on the current selection (or all files when none
        // selected). Issue #159: use each file's per-folder album_info so
        // tracks from different folders get the album RG tags for their own
        // album.
        let targets = self.target_indices();
        let jobs: Vec<ApplyJob> = targets
            .iter()
            .filter_map(|&idx| self.files.get(idx).map(|f| (idx, f)))
            .filter_map(|(idx, f)| {
                f.album_gain.map(|gain_db| ApplyJob {
                    idx,
                    path: f.path.clone(),
                    steps: db_to_steps(gain_db),
                    track_result: f.track_result.clone(),
                    album_info: f.album_info,
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
        let single_album = self.single_album;
        self.begin_worker(
            WorkerKind::AlbumApply,
            count,
            worker::spawn_apply(ctx.clone(), jobs, "album gain", ui_opts, single_album),
        );
    }

    fn begin_worker(&mut self, kind: WorkerKind, total: usize, handle: WorkerHandle) {
        // The start_* callers just reset row statuses to Pending, which a
        // Status sort must observe.
        self.display_order_dirty = true;
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
            WorkerKind::ImportScan => "Reading existing ReplayGain values...".to_string(),
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
        // Worker events mutate sortable row fields (volume, gains, status),
        // so any applied event invalidates the cached display order.
        if !events.is_empty() {
            self.display_order_dirty = true;
        }
        for event in events {
            self.apply_event(event);
        }
        if worker_finished {
            self.worker = None;
            self.worker_kind = None;
            self.is_processing = false;
            if !self.pending_drops.is_empty() {
                let paths = std::mem::take(&mut self.pending_drops);
                self.add_files(paths);
            }
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
                        // The import scan fills values in its own handler.
                        Some(WorkerKind::CheckTags) | Some(WorkerKind::ImportScan) => {}
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
                let album_clip = would_clip(album_info.album_peak, album_gain);

                for (idx, track_result) in successful {
                    if let Some(file) = self.files.get_mut(idx) {
                        Self::populate_track_analysis(file, &track_result, target);
                        file.track_result = Some(track_result);
                        file.album_volume = Some(album_volume);
                        file.album_gain = Some(album_gain);
                        file.album_clip = album_clip;
                        file.album_info = Some(album_info);
                        file.status = FileStatus::Analyzed;
                    }
                }
                for (idx, msg) in failures {
                    if let Some(file) = self.files.get_mut(idx) {
                        file.track_result = None;
                        file.status = FileStatus::Error(msg);
                    }
                }
            }
            WorkerEvent::AlbumAnalysisFailed(msg) => {
                self.status_message = format!("Album analysis failed: {}", msg);
            }
            WorkerEvent::FileApplied { idx, actual_steps } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::Done;
                    // File contents changed; cached tag snapshot is stale.
                    file.stored_tags = None;
                    // Shift the displayed volume / gain columns by the gain
                    // that was actually written so the user sees the
                    // post-apply state without rescanning (issue #160).
                    if actual_steps != 0 {
                        Self::shift_displayed_values(file, actual_steps);
                    }
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
            WorkerEvent::FileUndone { idx, steps_undone } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::Done;
                    file.stored_tags = None;
                    // Reverse the post-apply display shift so the row
                    // returns to its pre-apply numbers (issue #171). The
                    // file's bytes are back to the original state, so a
                    // shift of `-steps_undone` lines the display up with
                    // reality.
                    if steps_undone != 0 {
                        Self::shift_displayed_values(file, -steps_undone);
                    }
                }
            }
            WorkerEvent::FileUndoSkipped { idx } => {
                if let Some(file) = self.files.get_mut(idx) {
                    file.status = FileStatus::NoChangesToUndo;
                }
            }
            WorkerEvent::StoredTagsRead { idx, view } => {
                let target = self.target_volume;
                let is_import = self.worker_kind == Some(WorkerKind::ImportScan);
                if let Some(file) = self.files.get_mut(idx) {
                    if is_import {
                        Self::populate_from_stored_tags(file, &view, target);
                    }
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
                    file.stored_track_peak = None;
                    file.stored_album_peak = None;
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
        file.track_clip = would_clip(result.peak(), gain);
    }

    /// Fill a freshly-imported row's Volume / Gain columns from any existing
    /// ReplayGain tags read off the file (issue #203). The stored
    /// `REPLAYGAIN_*_GAIN` values are relative to the 89 dB reference, the
    /// same convention `populate_track_analysis` uses, so the displayed gain
    /// re-targets to the current Target. Only touches rows still Pending so a
    /// real analysis is never overwritten. Leaves `track_result` None — the
    /// tags carry no full analysis — so applying gain afterwards falls back to
    /// the headroom-based clipping check, exactly like manual gain.
    fn populate_from_stored_tags(file: &mut FileEntry, view: &StoredTagsView, target_volume: f64) {
        if file.status != FileStatus::Pending {
            return;
        }
        let mut found = false;
        if let Some(track_gain_db) = view.track_gain.as_deref().and_then(parse_db) {
            file.volume = Some(REPLAYGAIN_REFERENCE_DB - track_gain_db);
            let gain = target_volume - REPLAYGAIN_REFERENCE_DB + track_gain_db;
            file.track_gain = Some(gain);
            if let Some(peak) = view.track_peak.as_deref().and_then(parse_linear) {
                file.clipping = peak >= 1.0;
                file.track_clip = would_clip(peak, gain);
                file.stored_track_peak = Some(peak);
            }
            found = true;
        }
        if let Some(album_gain_db) = view.album_gain.as_deref().and_then(parse_db) {
            file.album_volume = Some(REPLAYGAIN_REFERENCE_DB - album_gain_db);
            let album_gain = target_volume - REPLAYGAIN_REFERENCE_DB + album_gain_db;
            file.album_gain = Some(album_gain);
            if let Some(peak) = view.album_peak.as_deref().and_then(parse_linear) {
                file.album_clip = would_clip(peak, album_gain);
                file.stored_album_peak = Some(peak);
            }
            found = true;
        }
        if found {
            file.status = FileStatus::Analyzed;
        }
    }

    /// Shift the row's cached display values by the dB that was actually
    /// applied (or, for undo, the negative of what was rolled back).
    /// Lets the user see the post-apply / post-undo state without
    /// rerunning analysis (issues #160, #171). The cached
    /// `track_result.peak()` is also rewritten so subsequent
    /// prevent-clipping checks see the file's current peak (issue #172).
    fn shift_displayed_values(file: &mut FileEntry, actual_steps: i32) {
        let db_applied = mp3rgain::steps_to_db(actual_steps);

        if let Some(v) = file.volume {
            file.volume = Some(v + db_applied);
        }
        if let Some(g) = file.track_gain {
            file.track_gain = Some(g - db_applied);
        }
        if let Some(v) = file.album_volume {
            file.album_volume = Some(v + db_applied);
        }
        if let Some(g) = file.album_gain {
            file.album_gain = Some(g - db_applied);
        }
        if let Some(track) = file.track_result.take() {
            let new_peak = track.peak() * db_to_linear(db_applied);
            file.clipping = new_peak >= 1.0;
            file.track_clip = file
                .track_gain
                .map(|g| would_clip(new_peak, g))
                .unwrap_or(false);
            file.album_clip = file
                .album_gain
                .map(|g| would_clip(new_peak, g))
                .unwrap_or(false);
            file.track_result = Some(track.with_peak(new_peak));
        } else {
            let scale = db_to_linear(db_applied);
            if let Some(new_peak) = file.stored_track_peak.map(|p| p * scale) {
                file.stored_track_peak = Some(new_peak);
                file.clipping = new_peak >= 1.0;
                file.track_clip = file
                    .track_gain
                    .map(|g| would_clip(new_peak, g))
                    .unwrap_or(false);
            }
            if let Some(new_peak) = file.stored_album_peak.map(|p| p * scale) {
                file.stored_album_peak = Some(new_peak);
                file.album_clip = file
                    .album_gain
                    .map(|g| would_clip(new_peak, g))
                    .unwrap_or(false);
            }
        }
    }
}

/// Parse a stored ReplayGain gain string such as `"+3.50 dB"` into dB.
/// Tolerant of the optional `dB` suffix and surrounding whitespace.
fn parse_db(s: &str) -> Option<f64> {
    s.trim().trim_end_matches("dB").trim().parse::<f64>().ok()
}

/// Parse a stored linear peak string such as `"0.988553"`.
fn parse_linear(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok()
}

/// Compare two `Option<f64>` values, putting `None` always at the bottom
/// regardless of direction (spreadsheet-style empty-cell handling).
fn cmp_opt_f64(a: Option<f64>, b: Option<f64>, desc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (Some(x), Some(y)) => {
            let o = x.partial_cmp(&y).unwrap_or(Ordering::Equal);
            if desc {
                o.reverse()
            } else {
                o
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Compare two pre-lowercased sort keys with empty-strings-last semantics.
fn cmp_str(a: &str, b: &str, desc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a.is_empty(), b.is_empty()) {
        (false, false) => {
            let o = a.cmp(b);
            if desc {
                o.reverse()
            } else {
                o
            }
        }
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (true, true) => Ordering::Equal,
    }
}

/// Platform-specific reveal-in-file-manager. macOS uses `open -R` so Finder
/// highlights the file inside its folder; Windows uses `explorer /select,`
/// for the same effect; Linux falls back to opening the parent directory
/// (xdg-open has no general "select" verb).
fn open_in_file_manager(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        // raw_arg, not arg: explorer needs `/select,"path"` quoted exactly;
        // arg() quotes the whole token for spaced paths → opens Documents.
        let win_path = path.to_string_lossy().replace('/', "\\");
        std::process::Command::new("explorer.exe")
            .raw_arg(format!("/select,\"{}\"", win_path))
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        let dir = path.parent().unwrap_or(path);
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}

impl eframe::App for Mp3rgainApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_worker_events();
        // Kick off an automatic stored-tag read for freshly imported files
        // once any prior worker has finished (issue #203).
        self.start_import_scan(ctx);
        // Run after the user's toolbar input is already in self.target_volume
        // (which is mutated by the DragValue in toolbar.rs from the previous
        // frame), and before this frame's render reads track_gain.
        self.recompute_targets_if_changed();
        crate::ui::render(self, ctx);
    }

    /// Persist the user's settings (issue #202). Called by eframe on the
    /// auto-save timer and on shutdown.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let settings = PersistedSettings {
            apply_options: self.apply_options,
            target_volume: self.target_volume,
            show_filename_only: self.show_filename_only,
            single_album: self.single_album,
        };
        eframe::set_value(storage, SETTINGS_KEY, &settings);
    }
}
