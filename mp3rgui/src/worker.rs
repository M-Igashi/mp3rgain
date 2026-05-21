//! Background workers for the four batch operations (analyze tracks,
//! analyze album, apply track gain, apply album gain).
//!
//! The pre-#152 GUI ran these synchronously on the egui main thread, so
//! the window froze and `total_progress` / `status_message` mutations
//! were invisible until the loop finished. Workers now run on
//! `std::thread::spawn`-ed threads and report progress through an
//! `mpsc::channel`; the UI side drains it from `update()` and calls
//! `ctx.request_repaint()` so egui actually redraws.

use mp3rgain::apply::{apply_with_options, ApplyOptions};
use mp3rgain::replaygain::{self, ReplayGainResult};
use mp3rgain::AacAlbumInfo;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;

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

    FileApplied {
        idx: usize,
    },
    FileApplyFailed {
        idx: usize,
        message: String,
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
}

/// User-facing apply toggles, captured at the moment the worker is
/// spawned. Worker combines these with the per-job data to build the
/// final `ApplyOptions`.
#[derive(Debug, Clone, Copy)]
pub struct ApplyOptionsUi {
    pub prevent_clipping: bool,
    pub wrap: bool,
    pub preserve_timestamp: bool,
    pub use_id3v2: bool,
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
        }
    }
}

/// Spawn a serial track-analysis worker.
///
/// Files are processed in order; cancel is checked between files.
pub fn spawn_track_analysis(ctx: egui::Context, files: Vec<(usize, PathBuf)>) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let mut analyzed = 0usize;
        let mut errors = 0usize;

        for (idx, path) in files {
            if cancel_w.load(Ordering::Relaxed) {
                send(&tx, &ctx, WorkerEvent::Cancelled);
                return;
            }
            send(&tx, &ctx, WorkerEvent::FileStart { idx });

            match replaygain::analyze_track(&path) {
                Ok(result) => {
                    analyzed += 1;
                    send(&tx, &ctx, WorkerEvent::TrackAnalyzed { idx, result });
                }
                Err(e) => {
                    errors += 1;
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
        }

        send(
            &tx,
            &ctx,
            WorkerEvent::Done {
                message: format_result_message("Analyzed", analyzed, errors),
            },
        );
    });

    WorkerHandle { rx, cancel }
}

/// Spawn the album-analysis worker. Uses the parallel variant from the
/// library (auto thread count) and reports per-file completion via the
/// `on_complete` callback.
pub fn spawn_album_analysis(
    ctx: egui::Context,
    indexed_paths: Vec<(usize, PathBuf)>,
) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let paths: Vec<PathBuf> = indexed_paths.iter().map(|(_, p)| p.clone()).collect();
        let original_indices: Vec<usize> = indexed_paths.iter().map(|(i, _)| *i).collect();

        let path_refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
        let threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);

        let tx_cb = tx.clone();
        let ctx_cb = ctx.clone();
        let indices_cb = original_indices.clone();
        let cancel_cb = Arc::clone(&cancel_w);
        let on_complete = move |idx_in_paths: usize, _path: &Path| {
            // Cancellation is best-effort: parallel analysis can't be
            // interrupted mid-file, but we can suppress further repaints
            // and final events.
            if cancel_cb.load(Ordering::Relaxed) {
                return;
            }
            if let Some(&original_idx) = indices_cb.get(idx_in_paths) {
                let _ = tx_cb.send(WorkerEvent::FileStart { idx: original_idx });
                ctx_cb.request_repaint();
            }
        };

        let result = replaygain::analyze_album_lenient_parallel_with_completion(
            &path_refs,
            None,
            threads,
            &on_complete,
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
                let analyzed = successful.len();
                let skipped = failures.len();

                send(
                    &tx,
                    &ctx,
                    WorkerEvent::AlbumAnalysisDone {
                        successful,
                        failures,
                        album_info,
                    },
                );

                let message = if skipped > 0 {
                    format!(
                        "Album analysis complete ({} tracks, {} skipped)",
                        analyzed, skipped
                    )
                } else {
                    format!("Album analysis complete ({} tracks)", analyzed)
                };
                send(&tx, &ctx, WorkerEvent::Done { message });
            }
            Err(e) => {
                send(&tx, &ctx, WorkerEvent::AlbumAnalysisFailed(e.to_string()));
                send(
                    &tx,
                    &ctx,
                    WorkerEvent::Done {
                        message: format!("Album analysis failed: {}", e),
                    },
                );
            }
        }
    });

    WorkerHandle { rx, cancel }
}

/// Spawn the apply worker (used by both Track Gain and Album Gain). Each
/// job is processed in turn; the cancel flag is checked between files.
pub fn spawn_apply(
    ctx: egui::Context,
    jobs: Vec<ApplyJob>,
    action_label: &'static str,
    ui_opts: ApplyOptionsUi,
) -> WorkerHandle {
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_w = Arc::clone(&cancel);

    thread::spawn(move || {
        let mut applied = 0usize;
        let mut errors = 0usize;

        for job in jobs {
            if cancel_w.load(Ordering::Relaxed) {
                send(&tx, &ctx, WorkerEvent::Cancelled);
                return;
            }
            send(&tx, &ctx, WorkerEvent::FileStart { idx: job.idx });

            let opts = build_apply_options(job.steps, job.track_result, job.album_info, ui_opts);
            match apply_with_options(&job.path, &opts) {
                Ok(_) => {
                    applied += 1;
                    send(&tx, &ctx, WorkerEvent::FileApplied { idx: job.idx });
                }
                Err(e) => {
                    errors += 1;
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
        }

        let suffix = if errors > 0 {
            format!(", {} error(s)", errors)
        } else {
            String::new()
        };
        send(
            &tx,
            &ctx,
            WorkerEvent::Done {
                message: format!("Applied {} to {} file(s){}", action_label, applied, suffix),
            },
        );
    });

    WorkerHandle { rx, cancel }
}

/// Build the final `ApplyOptions` by combining always-on safety rails
/// (undo, RG tag write, atomic temp file) with the user-toggleable
/// flags from the Options panel.
fn build_apply_options(
    steps: i32,
    track_result: Option<ReplayGainResult>,
    album_info: Option<AacAlbumInfo>,
    ui_opts: ApplyOptionsUi,
) -> ApplyOptions {
    let mut opts = ApplyOptions::new(steps);
    opts.track_result = track_result;
    opts.album_info = album_info;
    // Always-on safety rails.
    opts.write_undo = true;
    opts.write_replaygain_tags = true;
    opts.use_temp_file = true;
    // User-toggleable.
    opts.prevent_clipping = ui_opts.prevent_clipping;
    opts.wrap = ui_opts.wrap;
    opts.preserve_timestamp = ui_opts.preserve_timestamp;
    opts.use_id3v2 = ui_opts.use_id3v2;
    opts
}

fn send(tx: &Sender<WorkerEvent>, ctx: &egui::Context, event: WorkerEvent) {
    let _ = tx.send(event);
    ctx.request_repaint();
}

fn format_result_message(action: &str, count: usize, errors: usize) -> String {
    if errors > 0 {
        format!("{} {} file(s), {} error(s)", action, count, errors)
    } else {
        format!("{} {} file(s)", action, count)
    }
}
