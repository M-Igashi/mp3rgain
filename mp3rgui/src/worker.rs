//! Background workers for the four batch operations (analyze tracks,
//! analyze album, apply track gain, apply album gain).
//!
//! The pre-#152 GUI ran these synchronously on the egui main thread, so
//! the window froze and `total_progress` / `status_message` mutations
//! were invisible until the loop finished. Workers now run on
//! `std::thread::spawn`-ed threads and report progress through an
//! `mpsc::channel`; the UI side drains it from `update()` and calls
//! `ctx.request_repaint()` so egui actually redraws.

use mp3rgain::apply::{
    apply_with_options, predict_apply, read_mtime, restore_timestamp, write_album_minmax,
    ApplyOptions,
};
use mp3rgain::replaygain::{self, ReplayGainResult};
use mp3rgain::{
    id3v2, mp4meta, read_ape_tag_from_file, AacAlbumInfo, Channel, TAG_MP3GAIN_MINMAX,
    TAG_MP3GAIN_UNDO, TAG_REPLAYGAIN_ALBUM_GAIN, TAG_REPLAYGAIN_ALBUM_PEAK,
    TAG_REPLAYGAIN_TRACK_GAIN, TAG_REPLAYGAIN_TRACK_PEAK,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// Stored-tag snapshot for one file, populated by `spawn_check_stored_tags`.
/// All fields are pre-formatted strings so the UI can render them verbatim.
/// `None` means the tag was absent (not an error).
#[derive(Default, Clone)]
pub struct StoredTagsView {
    pub format: Option<&'static str>,
    pub track_gain: Option<String>,
    pub track_peak: Option<String>,
    pub album_gain: Option<String>,
    pub album_peak: Option<String>,
    pub undo: Option<String>,
    pub minmax: Option<String>,
}

impl StoredTagsView {
    pub fn is_empty(&self) -> bool {
        self.track_gain.is_none()
            && self.track_peak.is_none()
            && self.album_gain.is_none()
            && self.album_peak.is_none()
            && self.undo.is_none()
            && self.minmax.is_none()
    }
}

/// One message from a worker thread to the UI thread.
pub enum WorkerEvent {
    /// Worker is about to process `idx`. UI shows "Analyzing..." /
    /// "Applying..." for that row.
    FileStart {
        idx: usize,
    },

    TrackAnalyzed {
        idx: usize,
        result: ReplayGainResult,
    },
    TrackAnalysisFailed {
        idx: usize,
        message: String,
    },

    /// Album analysis result, applied to many rows at once.
    AlbumAnalysisDone {
        successful: Vec<(usize, ReplayGainResult)>,
        failures: Vec<(usize, String)>,
        album_info: AacAlbumInfo,
    },
    AlbumAnalysisFailed(String),

    /// Apply succeeded for `idx`. `actual_steps` is what `apply_with_options`
    /// actually wrote (may differ from the requested steps when prevent_clipping
    /// capped the gain). The UI uses it to refresh the row's volume / gain
    /// columns in place, so the user sees the post-apply numbers without
    /// having to re-analyze (issue #160).
    FileApplied {
        idx: usize,
        actual_steps: i32,
    },
    /// Dry-run analog of `FileApplied`: predict_apply succeeded, no bytes
    /// changed. UI shows "Would apply N steps" in the row status.
    FileApplyDryRun {
        idx: usize,
        actual_steps: i32,
        clipping_prevented: bool,
    },
    FileApplyFailed {
        idx: usize,
        message: String,
    },

    /// Stored-tag scan completed for `idx`. `view` carries pre-formatted
    /// strings; `view.is_empty()` distinguishes "no tags" from "all tags
    /// read successfully and present".
    StoredTagsRead {
        idx: usize,
        view: StoredTagsView,
    },

    /// `-x` Find Max Amplitude result for `idx`. Lighter than
    /// `TrackAnalyzed`: decodes for the true peak but skips the loudness
    /// analysis.
    MaxAmplitudeFound {
        idx: usize,
        peak: f64,
        headroom_db: Option<f64>,
    },
    MaxAmplitudeFailed {
        idx: usize,
        message: String,
    },

    /// Undo succeeded on `idx`. `steps_undone` is the left-channel gain
    /// step value that was stored in the undo tag — i.e. the cumulative
    /// gain that was applied to this file across previous applies, and
    /// that the undo just rolled back. The UI uses it to reverse the
    /// post-apply display shift so the volume / gain columns return to
    /// their pre-apply values without forcing a re-analyze (issue #171).
    FileUndone {
        idx: usize,
        steps_undone: i32,
    },
    /// `frames == 0` outcome: undo ran but the file had no recorded changes
    /// to roll back. Kept separate so the row status reads "no changes" rather
    /// than "Done" or "Error".
    FileUndoSkipped {
        idx: usize,
    },

    /// Worker observed the cancel flag and stopped early.
    Cancelled,

    /// Final event. UI clears `is_processing` and sets `status_message`.
    Done {
        message: String,
    },
}

/// UI-side handle to a running worker.
pub struct WorkerHandle {
    pub rx: Receiver<WorkerEvent>,
    pub cancel: Arc<AtomicBool>,
}

impl WorkerHandle {
    pub fn request_cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// A single apply job. Built by the UI side, consumed by the worker.
pub struct ApplyJob {
    pub idx: usize,
    pub path: PathBuf,
    pub steps: i32,
    pub track_result: Option<ReplayGainResult>,
    pub album_info: Option<AacAlbumInfo>,
    /// Per-channel gain (`-l`). `None` means the gain hits all channels.
    pub channel: Option<Channel>,
}

/// A single undo job.
pub struct UndoJob {
    pub idx: usize,
    pub path: PathBuf,
}

/// A single stored-tag scan job.
pub struct CheckTagsJob {
    pub idx: usize,
    pub path: PathBuf,
}

/// A single stored-tag deletion job.
pub struct DeleteTagsJob {
    pub idx: usize,
    pub path: PathBuf,
}

/// User-facing apply toggles, captured at the moment the worker is
/// spawned. Worker combines these with the per-job data to build the
/// final `ApplyOptions`.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ApplyOptionsUi {
    pub prevent_clipping: bool,
    pub wrap: bool,
    pub preserve_timestamp: bool,
    pub use_id3v2: bool,
    /// When true, Apply Track / Album Gain runs in dry-run mode: the worker
    /// reports what `apply_with_options` *would* do but doesn't touch the
    /// file. Equivalent to the CLI's -n flag.
    pub dry_run: bool,
}

impl Default for ApplyOptionsUi {
    fn default() -> Self {
        // Safe defaults: prevent clipping, keep mtime, no wrap, no
        // ID3v2-on-MP3 (the existing APE undo path is still the more
        // widely compatible option).
        Self {
            prevent_clipping: true,
            wrap: false,
            preserve_timestamp: true,
            use_id3v2: false,
            dry_run: false,
        }
    }
}

/// Spawn a parallel track-analysis worker.
///
/// Files run through [`run_job_pool`], so total wall time is comparable to
/// Album Analysis (issue #158, which was previously serial). Each completed
/// file emits a `TrackAnalyzed` event immediately so the table updates as
/// work happens instead of in a single batch at the end. Cancellation is
/// checked between files.
pub fn spawn_track_analysis(ctx: egui::Context, files: Vec<(usize, PathBuf)>) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let analyzed = AtomicUsize::new(0);
        let errors = AtomicUsize::new(0);

        {
            let tx = tx.clone();
            let ctx = ctx.clone();
            let (analyzed, errors) = (&analyzed, &errors);
            run_job_pool(files, &cancel_w, move |(idx, path)| {
                send(&tx, &ctx, WorkerEvent::FileStart { idx });
                match replaygain::analyze_track(&path) {
                    Ok(result) => {
                        analyzed.fetch_add(1, Ordering::Relaxed);
                        send(&tx, &ctx, WorkerEvent::TrackAnalyzed { idx, result });
                    }
                    Err(e) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        send(
                            &tx,
                            &ctx,
                            WorkerEvent::TrackAnalysisFailed {
                                idx,
                                message: e.to_string(),
                            },
                        );
                    }
                }
            });
        }

        if cancel_w.load(Ordering::Relaxed) {
            send(&tx, &ctx, WorkerEvent::Cancelled);
            return;
        }

        send(
            &tx,
            &ctx,
            WorkerEvent::Done {
                message: format_result_message(
                    "Analyzed",
                    analyzed.load(Ordering::Relaxed),
                    errors.load(Ordering::Relaxed),
                ),
            },
        );
    });

    WorkerHandle { rx, cancel }
}

/// Spawn the album-analysis worker. Uses the parallel variant from the
/// library (auto thread count) and reports per-file completion via the
/// `on_complete` callback.
/// Spawn an album-analysis worker for one or more groups of files.
///
/// Each inner Vec is treated as its own album, so loading files from
/// multiple folders no longer collapses them into a single album with
/// the wrong gain (issue #159). Groups are analyzed sequentially; the
/// per-group call still fans out across cores via the library's
/// rayon-backed parallel analyzer, so a single-group invocation is
/// behavior-identical to the pre-fix code.
pub fn spawn_album_analysis(
    ctx: egui::Context,
    groups: Vec<Vec<(usize, PathBuf)>>,
) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let group_count = groups.len();
        let threads = available_threads();

        let mut analyzed_total = 0usize;
        let mut skipped_total = 0usize;
        let mut error_groups = 0usize;

        for group in groups {
            if cancel_w.load(Ordering::Relaxed) {
                send(&tx, &ctx, WorkerEvent::Cancelled);
                return;
            }
            if group.is_empty() {
                continue;
            }

            let paths: Vec<PathBuf> = group.iter().map(|(_, p)| p.clone()).collect();
            let original_indices: Vec<usize> = group.iter().map(|(i, _)| *i).collect();
            let path_refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();

            let tx_cb = tx.clone();
            let ctx_cb = ctx.clone();
            let indices_cb = original_indices.clone();
            let cancel_cb = Arc::clone(&cancel_w);
            let on_complete = move |idx_in_paths: usize, _path: &Path| {
                if cancel_cb.load(Ordering::Relaxed) {
                    return;
                }
                if let Some(&original_idx) = indices_cb.get(idx_in_paths) {
                    let _ = tx_cb.send(WorkerEvent::FileStart { idx: original_idx });
                    ctx_cb.request_repaint();
                }
            };

            let result = replaygain::analyze_album_with_options(
                &path_refs,
                &replaygain::AlbumAnalysisOptions {
                    threads,
                    skip_errors: true,
                    on_complete: Some(&on_complete),
                    cancel: Some(&cancel_w),
                    ..Default::default()
                },
            );

            if cancel_w.load(Ordering::Relaxed) {
                send(&tx, &ctx, WorkerEvent::Cancelled);
                return;
            }

            match result {
                Ok(report) => {
                    let successful: Vec<(usize, ReplayGainResult)> = report
                        .successful_indices
                        .iter()
                        .zip(report.album.tracks().iter())
                        .map(|(&pos, track)| (original_indices[pos], track.clone()))
                        .collect();
                    let failures: Vec<(usize, String)> = report
                        .failures
                        .iter()
                        .map(|(pos, msg)| (original_indices[*pos], msg.clone()))
                        .collect();
                    let album_info = AacAlbumInfo {
                        album_gain_db: report.album.album_gain_db(),
                        album_peak: report.album.album_peak(),
                    };
                    analyzed_total += successful.len();
                    skipped_total += failures.len();

                    send(
                        &tx,
                        &ctx,
                        WorkerEvent::AlbumAnalysisDone {
                            successful,
                            failures,
                            album_info,
                        },
                    );
                }
                Err(e) => {
                    error_groups += 1;
                    send(&tx, &ctx, WorkerEvent::AlbumAnalysisFailed(e.to_string()));
                }
            }
        }

        let message = match (group_count, error_groups) {
            (1, 0) if skipped_total == 0 => {
                format!("Album analysis complete ({} tracks)", analyzed_total)
            }
            (1, 0) => format!(
                "Album analysis complete ({} tracks, {} skipped)",
                analyzed_total, skipped_total
            ),
            (_, 0) if skipped_total == 0 => format!(
                "Album analysis complete ({} albums, {} tracks)",
                group_count, analyzed_total
            ),
            (_, 0) => format!(
                "Album analysis complete ({} albums, {} tracks, {} skipped)",
                group_count, analyzed_total, skipped_total
            ),
            (_, n) => format!(
                "Album analysis: {} tracks analyzed, {} skipped, {} album(s) failed",
                analyzed_total, skipped_total, n
            ),
        };
        send(&tx, &ctx, WorkerEvent::Done { message });
    });

    WorkerHandle { rx, cancel }
}

/// Spawn the apply worker (used by both Track Gain and Album Gain).
///
/// Jobs run through [`run_job_pool`], matching the parallel Track Analysis
/// path so wall time scales with cores (issue #158 follow-up comment). Each
/// file's apply is independent — the temp-file rename in
/// `apply_with_options` uses an AtomicU64 counter so concurrent writes in
/// the same directory don't collide. Cancellation is checked between files.
pub fn spawn_apply(
    ctx: egui::Context,
    jobs: Vec<ApplyJob>,
    action_label: &'static str,
    ui_opts: ApplyOptionsUi,
    // When true (album-gain apply with issue #224 enabled), the album-wide
    // MINMAX range is stamped across the whole batch instead of per folder.
    // Ignored by track/manual/channel gain, whose jobs carry no album_info.
    single_album: bool,
) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let applied = AtomicUsize::new(0);
        let errors = AtomicUsize::new(0);

        // Album-gain applies carry `album_info` on every job. Capture their
        // MP3 paths now (before `jobs` is consumed) so we can write the
        // album-wide MP3GAIN_ALBUM_MINMAX after all files are applied — the
        // same mp3gain-parity step the CLI does (issue #210). APEv2 only and
        // not in dry-run.
        let album_minmax_paths: Vec<PathBuf> = if !ui_opts.dry_run && !ui_opts.use_id3v2 {
            jobs.iter()
                .filter(|j| j.album_info.is_some())
                .map(|j| j.path.clone())
                .collect()
        } else {
            Vec::new()
        };
        // Post-apply (max, min) global_gain range per album file, taken from
        // the apply reports so the MINMAX step below doesn't re-analyze every
        // file (issue #232).
        let gain_ranges: Mutex<BTreeMap<PathBuf, (u8, u8)>> = Mutex::new(BTreeMap::new());

        {
            let tx = tx.clone();
            let ctx = ctx.clone();
            let (applied, errors) = (&applied, &errors);
            let gain_ranges = &gain_ranges;
            run_job_pool(jobs, &cancel_w, move |job: ApplyJob| {
                send(&tx, &ctx, WorkerEvent::FileStart { idx: job.idx });

                let album_member =
                    !ui_opts.dry_run && !ui_opts.use_id3v2 && job.album_info.is_some();
                let opts = build_apply_options(
                    job.steps,
                    job.track_result,
                    job.album_info,
                    job.channel,
                    ui_opts,
                );
                let result = if ui_opts.dry_run {
                    predict_apply(&job.path, &opts)
                } else {
                    apply_with_options(&job.path, &opts)
                };
                match result {
                    Ok(report) => {
                        if album_member {
                            if let Some(range) = report.gain_range {
                                gain_ranges.lock().unwrap().insert(job.path.clone(), range);
                            }
                        }
                        applied.fetch_add(1, Ordering::Relaxed);
                        if ui_opts.dry_run {
                            send(
                                &tx,
                                &ctx,
                                WorkerEvent::FileApplyDryRun {
                                    idx: job.idx,
                                    actual_steps: report.actual_steps,
                                    clipping_prevented: report.clipping_prevented,
                                },
                            );
                        } else {
                            send(
                                &tx,
                                &ctx,
                                WorkerEvent::FileApplied {
                                    idx: job.idx,
                                    actual_steps: report.actual_steps,
                                },
                            );
                        }
                    }
                    Err(e) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        send(
                            &tx,
                            &ctx,
                            WorkerEvent::FileApplyFailed {
                                idx: job.idx,
                                message: e.to_string(),
                            },
                        );
                    }
                }
            });
        }

        if cancel_w.load(Ordering::Relaxed) {
            send(&tx, &ctx, WorkerEvent::Cancelled);
            return;
        }

        // Album-wide MP3GAIN_ALBUM_MINMAX, written once the whole album has
        // been applied (mp3gain parity, issue #210). No-op for track/manual
        // gain (empty list) and for the dry-run / ID3v2 paths. By default the
        // GUI treats each folder as its own album (issue #159), so the range
        // is stamped per parent directory; in single-album mode (issue #224)
        // it spans the whole batch to match the one shared album gain.
        if !album_minmax_paths.is_empty() {
            type MinmaxEntry<'a> = (&'a Path, Option<(u8, u8)>);
            let gain_ranges = gain_ranges.into_inner().unwrap();
            if single_album {
                let all: Vec<MinmaxEntry> = album_minmax_paths
                    .iter()
                    .map(|p| (p.as_path(), gain_ranges.get(p).copied()))
                    .collect();
                write_album_minmax(&all);
            } else {
                let mut by_folder: BTreeMap<PathBuf, Vec<MinmaxEntry>> = BTreeMap::new();
                for p in &album_minmax_paths {
                    let parent = p.parent().map(Path::to_path_buf).unwrap_or_default();
                    by_folder
                        .entry(parent)
                        .or_default()
                        .push((p.as_path(), gain_ranges.get(p).copied()));
                }
                for group in by_folder.values() {
                    write_album_minmax(group);
                }
            }
        }

        let verb = if ui_opts.dry_run {
            "Dry-ran"
        } else {
            "Applied"
        };
        send(
            &tx,
            &ctx,
            WorkerEvent::Done {
                message: format_result_message(
                    &format!("{} {} on", verb, action_label),
                    applied.load(Ordering::Relaxed),
                    errors.load(Ordering::Relaxed),
                ),
            },
        );
    });

    WorkerHandle { rx, cancel }
}

/// Spawn the undo worker. Runs jobs through [`run_job_pool`] (undo is
/// dominated by per-file I/O and, for AAC, a bitstream re-analysis, so it
/// parallelizes the same way apply does). Dispatches per file to the
/// correct undo path:
///   - AAC: `mp3rgain::aac::undo_aac_gain`
///   - MP3 + `use_id3v2`: `mp3rgain::undo_gain_id3v2`
///   - MP3 (default APE): `mp3rgain::undo_gain`
///
/// Mirrors the CLI's `process_undo` dispatch so behavior matches.
pub fn spawn_undo(ctx: egui::Context, jobs: Vec<UndoJob>, ui_opts: ApplyOptionsUi) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let undone = AtomicUsize::new(0);
        let skipped = AtomicUsize::new(0);
        let errors = AtomicUsize::new(0);

        {
            let tx = tx.clone();
            let ctx = ctx.clone();
            let (undone, skipped, errors) = (&undone, &skipped, &errors);
            run_job_pool(jobs, &cancel_w, move |job: UndoJob| {
                send(&tx, &ctx, WorkerEvent::FileStart { idx: job.idx });

                let original_mtime = saved_mtime(&job.path, ui_opts.preserve_timestamp);

                // Peek at the undo tag before running undo, so we can tell the
                // UI how many steps to reverse on the display. We can't read
                // it after undo because undo_gain_auto deletes the tag (issue #171).
                let steps_undone =
                    mp3rgain::read_undo_steps(&job.path, ui_opts.use_id3v2).unwrap_or(0);

                let result = mp3rgain::undo_gain_auto(&job.path, ui_opts.use_id3v2);
                match result {
                    Ok(0) => {
                        skipped.fetch_add(1, Ordering::Relaxed);
                        send(&tx, &ctx, WorkerEvent::FileUndoSkipped { idx: job.idx });
                    }
                    Ok(_) => {
                        if let Some(mtime) = original_mtime {
                            restore_timestamp(&job.path, mtime);
                        }
                        undone.fetch_add(1, Ordering::Relaxed);
                        send(
                            &tx,
                            &ctx,
                            WorkerEvent::FileUndone {
                                idx: job.idx,
                                steps_undone,
                            },
                        );
                    }
                    Err(e) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        send(
                            &tx,
                            &ctx,
                            WorkerEvent::FileApplyFailed {
                                idx: job.idx,
                                message: e.to_string(),
                            },
                        );
                    }
                }
            });
        }

        if cancel_w.load(Ordering::Relaxed) {
            send(&tx, &ctx, WorkerEvent::Cancelled);
            return;
        }

        let mut parts: Vec<String> = Vec::new();
        parts.push(format!("Undone {} file(s)", undone.load(Ordering::Relaxed)));
        let skipped = skipped.load(Ordering::Relaxed);
        let errors = errors.load(Ordering::Relaxed);
        if skipped > 0 {
            parts.push(format!("{} had no changes to undo", skipped));
        }
        if errors > 0 {
            parts.push(format!("{} error(s)", errors));
        }
        send(
            &tx,
            &ctx,
            WorkerEvent::Done {
                message: parts.join(", "),
            },
        );
    });

    WorkerHandle { rx, cancel }
}

/// Spawn the stored-tag deletion worker. Destructive: removes APE /
/// ID3v2 RG / MP4 freeform RG+undo tags from each file, fanned out via
/// [`run_job_pool`].
///
/// Dispatch mirrors the CLI's `process_delete_tags`:
///   - AAC: `mp4meta::delete_replaygain_tags` + `delete_undo_tags`
///   - MP3 + `use_id3v2`: `mp3rgain::delete_id3v2_replaygain`
///   - MP3 (default APE): `mp3rgain::delete_ape_tag`
pub fn spawn_delete_tags(
    ctx: egui::Context,
    jobs: Vec<DeleteTagsJob>,
    use_id3v2: bool,
    preserve_timestamp: bool,
) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let deleted = AtomicUsize::new(0);
        let errors = AtomicUsize::new(0);

        {
            let tx = tx.clone();
            let ctx = ctx.clone();
            let (deleted, errors) = (&deleted, &errors);
            run_job_pool(jobs, &cancel_w, move |job: DeleteTagsJob| {
                send(&tx, &ctx, WorkerEvent::FileStart { idx: job.idx });

                let original_mtime = saved_mtime(&job.path, preserve_timestamp);

                let result = mp3rgain::delete_gain_tags_auto(&job.path, use_id3v2);

                match result {
                    Ok(()) => {
                        if let Some(m) = original_mtime {
                            restore_timestamp(&job.path, m);
                        }
                        deleted.fetch_add(1, Ordering::Relaxed);
                        // Tag deletion doesn't change audio levels, so the row's
                        // volume/gain columns stay valid — pass 0 steps so the UI
                        // doesn't shift them.
                        send(
                            &tx,
                            &ctx,
                            WorkerEvent::FileApplied {
                                idx: job.idx,
                                actual_steps: 0,
                            },
                        );
                    }
                    Err(e) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        send(
                            &tx,
                            &ctx,
                            WorkerEvent::FileApplyFailed {
                                idx: job.idx,
                                message: e.to_string(),
                            },
                        );
                    }
                }
            });
        }

        if cancel_w.load(Ordering::Relaxed) {
            send(&tx, &ctx, WorkerEvent::Cancelled);
            return;
        }

        send(
            &tx,
            &ctx,
            WorkerEvent::Done {
                message: format_result_message(
                    "Deleted stored tags from",
                    deleted.load(Ordering::Relaxed),
                    errors.load(Ordering::Relaxed),
                ),
            },
        );
    });

    WorkerHandle { rx, cancel }
}

/// Spawn the max-amplitude scanner. Mirrors the CLI's `-x` /
/// `cmd_max_amplitude`: walks MP3 frame headers (or AAC `global_gain`
/// fields) for the gain range and decodes the audio peak via
/// `find_peak_amplitude` (no loudness analysis, but still a full decode —
/// hence the [`run_job_pool`] fan-out).
pub fn spawn_find_max_amplitude(ctx: egui::Context, files: Vec<(usize, PathBuf)>) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let found = AtomicUsize::new(0);
        let errors = AtomicUsize::new(0);

        {
            let tx = tx.clone();
            let ctx = ctx.clone();
            let (found, errors) = (&found, &errors);
            run_job_pool(files, &cancel_w, move |(idx, path)| {
                send(&tx, &ctx, WorkerEvent::FileStart { idx });

                match mp3rgain::find_max_amplitude(&path) {
                    Ok(amp) => {
                        found.fetch_add(1, Ordering::Relaxed);
                        let peak = amp.max_amplitude();
                        let headroom_db = mp3rgain::peak_to_headroom_db(peak);
                        send(
                            &tx,
                            &ctx,
                            WorkerEvent::MaxAmplitudeFound {
                                idx,
                                peak,
                                headroom_db,
                            },
                        );
                    }
                    Err(e) => {
                        errors.fetch_add(1, Ordering::Relaxed);
                        send(
                            &tx,
                            &ctx,
                            WorkerEvent::MaxAmplitudeFailed {
                                idx,
                                message: e.to_string(),
                            },
                        );
                    }
                }
            });
        }

        if cancel_w.load(Ordering::Relaxed) {
            send(&tx, &ctx, WorkerEvent::Cancelled);
            return;
        }

        send(
            &tx,
            &ctx,
            WorkerEvent::Done {
                message: format_result_message(
                    "Max amplitude scanned on",
                    found.load(Ordering::Relaxed),
                    errors.load(Ordering::Relaxed),
                ),
            },
        );
    });

    WorkerHandle { rx, cancel }
}

/// Spawn the stored-tag scanner. Iterates files serially (tag reads are
/// I/O-light) and emits `StoredTagsRead` for each.
///
/// Dispatch mirrors the CLI's `process_check_tags`:
///   - AAC: MP4 freeform RG + undo
///   - MP3 + `use_id3v2`: ID3v2 TXXX RG
///   - MP3: APE
pub fn spawn_check_stored_tags(
    ctx: egui::Context,
    jobs: Vec<CheckTagsJob>,
    use_id3v2: bool,
) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let total = jobs.len();
        for job in jobs {
            if cancel_w.load(Ordering::Relaxed) {
                send(&tx, &ctx, WorkerEvent::Cancelled);
                return;
            }
            send(&tx, &ctx, WorkerEvent::FileStart { idx: job.idx });
            let view = read_stored_tags(&job.path, use_id3v2);
            send(
                &tx,
                &ctx,
                WorkerEvent::StoredTagsRead { idx: job.idx, view },
            );
        }

        send(
            &tx,
            &ctx,
            WorkerEvent::Done {
                message: format!("Checked stored tags for {} file(s)", total),
            },
        );
    });

    WorkerHandle { rx, cancel }
}

fn read_stored_tags(path: &Path, use_id3v2: bool) -> StoredTagsView {
    if mp4meta::is_aac_file(path) {
        let undo_tags = mp4meta::read_undo_tags(path).unwrap_or_default();
        let rg_tags = mp4meta::read_replaygain_tags(path).unwrap_or_default();
        return StoredTagsView {
            format: Some("MP4"),
            track_gain: rg_tags.track_gain().map(str::to_string),
            track_peak: rg_tags.track_peak().map(str::to_string),
            album_gain: rg_tags.album_gain().map(str::to_string),
            album_peak: rg_tags.album_peak().map(str::to_string),
            undo: undo_tags.undo().map(str::to_string),
            minmax: undo_tags.minmax().map(str::to_string),
        };
    }
    if use_id3v2 {
        let rg = id3v2::read_id3v2_replaygain(path).unwrap_or_default();
        return StoredTagsView {
            format: Some("ID3v2"),
            track_gain: rg.track_gain,
            track_peak: rg.track_peak,
            album_gain: rg.album_gain,
            album_peak: rg.album_peak,
            undo: rg.undo,
            minmax: rg.minmax,
        };
    }
    // APE
    if let Ok(Some(tag)) = read_ape_tag_from_file(path) {
        return StoredTagsView {
            format: Some("APE"),
            track_gain: tag.get(TAG_REPLAYGAIN_TRACK_GAIN).map(str::to_string),
            track_peak: tag.get(TAG_REPLAYGAIN_TRACK_PEAK).map(str::to_string),
            album_gain: tag.get(TAG_REPLAYGAIN_ALBUM_GAIN).map(str::to_string),
            album_peak: tag.get(TAG_REPLAYGAIN_ALBUM_PEAK).map(str::to_string),
            undo: tag.get(TAG_MP3GAIN_UNDO).map(str::to_string),
            minmax: tag.get(TAG_MP3GAIN_MINMAX).map(str::to_string),
        };
    }
    StoredTagsView {
        format: Some("APE"),
        ..Default::default()
    }
}

/// Build the final `ApplyOptions` by combining always-on safety rails
/// (undo, RG tag write, atomic temp file) with the user-toggleable
/// flags from the Options panel.
fn build_apply_options(
    steps: i32,
    track_result: Option<ReplayGainResult>,
    album_info: Option<AacAlbumInfo>,
    channel: Option<Channel>,
    ui_opts: ApplyOptionsUi,
) -> ApplyOptions {
    let mut opts = ApplyOptions::new(steps);
    opts.track_result = track_result;
    opts.album_info = album_info;
    opts.channel = channel;
    // Always-on safety rails.
    opts.write_undo = true;
    opts.write_replaygain_tags = channel.is_none();
    opts.use_temp_file = true;
    // User-toggleable.
    opts.prevent_clipping = ui_opts.prevent_clipping;
    opts.wrap = ui_opts.wrap;
    opts.preserve_timestamp = ui_opts.preserve_timestamp;
    opts.use_id3v2 = ui_opts.use_id3v2;
    opts
}

/// Worker thread count: available cores, falling back to 1.
fn available_threads() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

/// mtime snapshot to restore after a write when "preserve timestamp" is on.
fn saved_mtime(path: &Path, preserve: bool) -> Option<std::time::SystemTime> {
    if preserve {
        read_mtime(path)
    } else {
        None
    }
}

/// Drain `jobs` through a pool of `available_parallelism` scoped worker
/// threads, calling `per_job` for each job. Blocks until the queue is empty
/// or `cancel` is observed (checked between jobs). Each pool thread gets its
/// own clone of `per_job` — captures like `mpsc::Sender` and `egui::Context`
/// are cheap reference-counted clones. Jobs start in their original order.
fn run_job_pool<J, F>(jobs: Vec<J>, cancel: &AtomicBool, per_job: F)
where
    J: Send,
    F: Fn(J) + Clone + Send,
{
    let pool_size = available_threads().min(jobs.len());

    // Popping takes from the Vec's tail; reverse once so files are picked
    // up in the order the caller listed them.
    let mut jobs = jobs;
    jobs.reverse();
    let queue = Mutex::new(jobs);

    thread::scope(|scope| {
        for _ in 0..pool_size {
            let per_job = per_job.clone();
            let queue = &queue;
            scope.spawn(move || loop {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let next = {
                    let mut q = queue.lock().expect("worker job queue poisoned");
                    q.pop()
                };
                let Some(job) = next else { break };
                per_job(job);
            });
        }
    });
}

fn send(tx: &Sender<WorkerEvent>, ctx: &egui::Context, event: WorkerEvent) {
    let _ = tx.send(event);
    ctx.request_repaint();
}

/// Build the final status message: `"<action> <count> file(s)"` plus a
/// `", N error(s)"` suffix when any job failed. `action` carries any
/// worker-specific phrasing (e.g. "Applied Track Gain on").
fn format_result_message(action: &str, count: usize, errors: usize) -> String {
    if errors > 0 {
        format!("{} {} file(s), {} error(s)", action, count, errors)
    } else {
        format!("{} {} file(s)", action, count)
    }
}
