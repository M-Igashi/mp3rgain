//! Unified apply-gain pipeline shared between the CLI and GUI frontends.
//!
//! Both `mp3rgain` (CLI) and `mp3rgui` (GUI) drive the same per-file
//! work — clipping check, atomic temp-file write, undo & ReplayGain tag
//! writing, mtime restoration — by populating an [`ApplyOptions`] and
//! calling [`apply_with_options`].
//!
//! Frontend-specific concerns (output formatting, progress reporting,
//! interactive cancellation, dry-run early-return) stay on the caller
//! side; this module only does the file work.
//!
//! Lifted out of `src/processors/{apply,replaygain}.rs` for issue #153
//! so the GUI no longer has to compose behavior out of low-level
//! primitives and silently miss undo / ReplayGain tags (issue #149/#150
//! / #151).

use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

#[cfg(feature = "aac")]
use crate::aac;
use crate::error::{Error, Result};
use crate::frame::SaturationStats;
use crate::gain::{
    apply_gain_to_peak, peak_to_headroom_db, steps_to_db, Channel, GainOptions, GAIN_STEP_DB,
    MAX_GAIN,
};
use crate::replaygain::ReplayGainResult;
use crate::{ape, id3v2, mp4meta, TagLayout};

/// Per-process counter for `.mp3rgain_temp_*` filenames so parallel
/// apply tasks operating on files in the same directory don't collide.
/// Lifted from `src/processors/utils.rs`.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// AAC analysis cached between the clipping check and the apply step so a
/// single apply never walks the bitstream twice (issue #188).
#[cfg(feature = "aac")]
type AacAnalysisCache = Option<aac::AacAnalysis>;
#[cfg(not(feature = "aac"))]
type AacAnalysisCache = Option<std::convert::Infallible>;

/// Album-level ReplayGain info passed alongside per-track results.
///
/// Used to populate the album fields of the ReplayGain tags
/// (`replaygain_album_gain` / `replaygain_album_peak`) when
/// [`ApplyOptions::write_replaygain_tags`] is on.
#[derive(Debug, Clone, Copy)]
pub struct AacAlbumInfo {
    pub album_gain_db: f64,
    pub album_peak: f64,
}

/// Driver configuration for [`apply_with_options`].
///
/// Construct via [`ApplyOptions::new`] and tweak fields directly — this
/// struct is data-only, no builder methods.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ApplyOptions {
    /// Requested gain in MP3 gain steps (1 step = 1.5 dB).
    pub steps: i32,

    /// Per-track ReplayGain analysis. Provides `peak()` for the
    /// ReplayGain-based clipping check and `gain_db()` / `peak()` for
    /// ReplayGain tag writing.
    pub track_result: Option<ReplayGainResult>,

    /// Album-level info that feeds the album fields of the ReplayGain
    /// tags.
    pub album_info: Option<AacAlbumInfo>,

    /// `-k`: if the requested gain would clip, cap it at the available
    /// headroom instead of applying as-is.
    pub prevent_clipping: bool,

    /// `-w`: wrap around 0–255 instead of saturating. Also disables the
    /// clipping check (wrap mode is intentionally lossy).
    pub wrap: bool,

    /// `-p`: restore the file's mtime after writing.
    pub preserve_timestamp: bool,

    /// Record an undo tag (MP4 freeform for AAC; for MP3 the container is
    /// [`Self::tag_layout`] — APEv2 unless `-s i`).
    pub write_undo: bool,

    /// Write ReplayGain metadata tags. Requires [`Self::track_result`] to
    /// be set. AAC writes to mp4 freeform metadata; MP3 follows
    /// [`Self::tag_layout`] (issue #204).
    pub write_replaygain_tags: bool,

    /// MP3 only — which container(s) the tags go in. Defaults to
    /// [`TagLayout::Split`].
    pub tag_layout: TagLayout,

    /// `-l`: MP3 only — apply gain to a single channel (Stereo / Dual
    /// Channel) instead of all channels. AAC has no per-channel apply
    /// path; setting this on an AAC file is an error.
    pub channel: Option<Channel>,

    /// Skip the pre-apply clipping check entirely. For callers that will
    /// never surface [`ApplyReport::clipping_detected`] (`-c` / `-q`, or the
    /// channel path), the headroom check's full-file frame walk is pure
    /// waste (issue #232; the read itself is shared with the apply step
    /// since issue #251). Ignored when [`Self::prevent_clipping`] is on —
    /// `-k` needs the check to cap the gain.
    pub skip_clipping_check: bool,
}

impl ApplyOptions {
    /// Construct with a requested step count and "safe" defaults
    /// (undo on, no clipping prevention, no tag writing).
    pub fn new(steps: i32) -> Self {
        Self {
            steps,
            track_result: None,
            album_info: None,
            prevent_clipping: false,
            wrap: false,
            preserve_timestamp: false,
            write_undo: true,
            write_replaygain_tags: false,
            tag_layout: TagLayout::default(),
            channel: None,
            skip_clipping_check: false,
        }
    }
}

/// Outcome of an [`apply_with_options`] call.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ApplyReport {
    /// MP3 frames or AAC gain fields actually modified.
    pub modified: usize,

    /// Steps actually applied (= [`ApplyOptions::steps`] unless
    /// [`ApplyOptions::prevent_clipping`] capped it, or unless steps==0).
    pub actual_steps: i32,

    /// True when `actual_steps < requested_steps` due to clipping
    /// prevention.
    pub clipping_prevented: bool,

    /// Clipping diagnostic the caller can format into a warning when
    /// `prevent_clipping` is off. `None` if no check ran (steps<=0 or
    /// wrap mode).
    pub clipping_detected: Option<ClippingDetection>,

    /// MP3 global_gain values that clamped at 0 (silence) during a
    /// saturating apply, where the requested gain couldn't be fully
    /// applied (issue #207). Always 0 for wrap mode and for [`predict_apply`].
    pub saturated_low: usize,

    /// MP3 global_gain values that clamped at 255 (distortion) during a
    /// saturating apply (issue #207).
    pub saturated_high: usize,

    /// Post-apply (max, min) global_gain range across the modified MP3
    /// frames, recorded during the apply pass so callers don't need to
    /// re-read the file (issue #231). `None` for AAC, channel-specific
    /// applies, dry runs, and zero-frame applies.
    pub gain_range: Option<(u8, u8)>,
}

/// Per-strategy clipping signal.
#[derive(Debug, Clone, Copy)]
pub enum ClippingDetection {
    /// `analyze()`-based: file's available headroom in gain steps.
    Headroom(i32),
    /// ReplayGain-based: peak that would result from the requested gain.
    Peak(f64),
}

/// Apply gain to `file_path` according to `opts`.
///
/// Does, in order:
/// 1. Optional clipping check (headroom-based for `-g`/`-l`, peak-based
///    when [`ApplyOptions::track_result`] is set).
/// 2. Gain application — MP3 APE / MP3 ID3v2 / AAC, via temp file +
///    atomic rename. The MP3 undo and ReplayGain tags (container chosen by
///    [`ApplyOptions::tag_layout`] — issue #204) are folded into the same
///    write (issues #227, #232).
/// 3. Remaining ReplayGain tag writes: AAC mp4 metadata, and the APEv2
///    re-write for wrap/saturated or channel applies.
/// 4. Mtime restoration when [`ApplyOptions::preserve_timestamp`] is on.
pub fn apply_with_options(file_path: &Path, opts: &ApplyOptions) -> Result<ApplyReport> {
    let is_aac = mp4meta::is_aac_file(file_path);

    if is_aac && opts.channel.is_some() {
        return Err(Error::ChannelGainOnAac);
    }

    let original_mtime = if opts.preserve_timestamp {
        read_mtime(file_path)
    } else {
        None
    };

    // 1) Clipping check + cap.
    //
    // The AAC bitstream analysis is cached so the apply step below can reuse
    // it instead of a second scan (issue #188). MINMAX / ReplayGain are now
    // recorded from a *post-apply* scan (issue #210), so no pre-apply MP3
    // analysis needs to be threaded through here.
    // The MP3 headroom check likewise keeps the bytes it read so the apply
    // step below reuses them instead of a second full read + frame walk of
    // the unchanged file (issue #251).
    let mut aac_analysis: AacAnalysisCache = None;
    let mut mp3_data: Option<Vec<u8>> = None;
    let (actual_steps, clipping_prevented, clipping_detected) =
        check_clipping(file_path, opts, is_aac, &mut aac_analysis, &mut mp3_data)?;

    // 2) Apply gain to bytes. MP3 reports global_gain saturation (issue
    // #207); AAC clamps in its own path and isn't tallied here.
    //
    // The MP3 paths fold the tag writes into the same visible write as the
    // gain apply (issue #232): ID3v2 writes undo + minmax + ReplayGain TXXX
    // frames onto the temp file before the rename, and the APE path embeds
    // the arithmetic `REPLAYGAIN_*` items in the tag the gain apply already
    // rewrites. Wrap mode and channel applies keep the separate APE
    // ReplayGain write in step 3 below.
    let mut saturation = SaturationStats::default();
    let mut ape_rg_folded = false;
    let modified = if is_aac {
        apply_aac_bytes(file_path, actual_steps, opts, aac_analysis)?
    } else if opts.tag_layout.mp3gain_in_id3v2() {
        saturation = apply_mp3_id3v2_bytes(file_path, actual_steps, opts, mp3_data.take())?;
        saturation.frames
    } else {
        // Under Split the ReplayGain values belong in ID3v2, so nothing is
        // folded into the APE write here — step 3 handles them.
        let folded_rg = if opts.write_replaygain_tags
            && opts.tag_layout == TagLayout::Ape
            && opts.channel.is_none()
            && !opts.wrap
        {
            compute_rg_residual(file_path, opts, actual_steps, false).map(|r| r.to_ape())
        } else {
            None
        };
        ape_rg_folded = folded_rg.is_some();
        saturation =
            apply_mp3_ape_bytes(file_path, actual_steps, opts, folded_rg, mp3_data.take())?;
        saturation.frames
    };

    // 3) Remaining ReplayGain tag writes.
    //
    // AAC writes to mp4 freeform metadata (always arithmetic — AAC clamps
    // internally, and mp3gain has no AAC, so it is interop-neutral). The MP3
    // paths were handled in step 2, except: under saturation or `-w` wrap the
    // loudness shift is not uniform, so the modified file is re-analyzed for
    // the true post-apply residual and the APE items rewritten; channel
    // applies keep the legacy separate write.
    if opts.write_replaygain_tags && !opts.tag_layout.mp3gain_in_id3v2() {
        let needs_reanalysis =
            !is_aac && (opts.wrap || saturation.saturated_low > 0 || saturation.saturated_high > 0);
        if is_aac {
            if let Some(res) = compute_rg_residual(file_path, opts, actual_steps, false) {
                let mut tags = mp4meta::ReplayGainTags::default();
                tags.set_track(res.track_gain_db, res.track_peak);
                if let Some((album_gain, album_peak)) = res.album {
                    tags.set_album(album_gain, album_peak);
                }
                tags.set_algorithm(res.mode);
                mp4meta::write_replaygain_tags(file_path, &tags)?;
            }
        } else if opts.tag_layout == TagLayout::Split {
            // Clear any APEv2 ReplayGain first: mp3gain or an earlier `-s a`
            // run may have left values there, and they would now disagree
            // with the authoritative ID3v2 ones.
            ape::remove_ape_replaygain(file_path)?;
            if let Some(res) = compute_rg_residual(file_path, opts, actual_steps, needs_reanalysis)
            {
                id3v2::write_id3v2_replaygain(file_path, &res.to_id3v2())?;
            }
        } else if !ape_rg_folded || needs_reanalysis {
            if let Some(res) = compute_rg_residual(file_path, opts, actual_steps, needs_reanalysis)
            {
                ape::write_ape_replaygain(file_path, &res.to_ape())?;
            }
        }
    }

    // 4) Restore mtime.
    if let Some(mtime) = original_mtime {
        restore_timestamp(file_path, mtime);
    }

    let gain_range = (!is_aac && opts.channel.is_none() && saturation.frames > 0)
        .then_some((saturation.max_gain, saturation.min_gain));

    Ok(ApplyReport {
        modified,
        actual_steps,
        clipping_prevented,
        clipping_detected,
        saturated_low: saturation.saturated_low,
        saturated_high: saturation.saturated_high,
        gain_range,
    })
}

/// Dry-run companion to [`apply_with_options`]: runs the same clipping
/// check (headroom-based or ReplayGain-peak based depending on `opts`)
/// without touching the file. `report.modified` is always 0.
///
/// Frontends drive `-n` / a "Dry run" toggle through this so the
/// "would apply N steps" message lines up with what a real apply
/// would do.
pub fn predict_apply(file_path: &Path, opts: &ApplyOptions) -> Result<ApplyReport> {
    let is_aac = mp4meta::is_aac_file(file_path);
    let mut aac_analysis: AacAnalysisCache = None;
    let (actual_steps, clipping_prevented, clipping_detected) =
        check_clipping(file_path, opts, is_aac, &mut aac_analysis, &mut None)?;
    Ok(ApplyReport {
        modified: 0,
        actual_steps,
        clipping_prevented,
        clipping_detected,
        saturated_low: 0,
        saturated_high: 0,
        gain_range: None,
    })
}

fn check_clipping(
    file_path: &Path,
    opts: &ApplyOptions,
    is_aac: bool,
    aac_analysis: &mut AacAnalysisCache,
    mp3_data: &mut Option<Vec<u8>>,
) -> Result<(i32, bool, Option<ClippingDetection>)> {
    let steps = opts.steps;
    if opts.wrap || (opts.skip_clipping_check && !opts.prevent_clipping) {
        return Ok((steps, false, None));
    }

    // ReplayGain-peak branch.
    //
    // Runs for any step count, including `steps <= 0` (issue #206): a
    // track already at the reference loudness nets 0 gain steps, but if it
    // already clips (peak > 1.0) `-k` must still attenuate it below
    // unity. The headroom branch below keeps its `steps <= 0` early-out —
    // there, only positive gain can introduce clipping.
    if let Some(track) = opts.track_result.as_ref() {
        let new_peak = apply_gain_to_peak(track.peak(), steps_to_db(steps));
        if new_peak > 1.0 {
            if opts.prevent_clipping {
                // `new_peak > 1.0` implies `peak > 0`, so headroom is
                // well-defined here.
                let max_safe_db = peak_to_headroom_db(track.peak()).unwrap_or(0.0);
                // Floor (not round) so the cap never exceeds true headroom —
                // round() would, e.g., turn 0.8 dB of headroom into 1 step
                // (1.5 dB) and re-introduce clipping. Negative results are
                // allowed: when the source file already clips (peak > 1.0),
                // headroom is negative and the cap legitimately needs to
                // attenuate the file below its current loudness to remove
                // clipping — matching the original MP3GainGUI behavior
                // (issue #173).
                let max_safe_steps = (max_safe_db / GAIN_STEP_DB).floor() as i32;
                return Ok((
                    max_safe_steps,
                    true,
                    Some(ClippingDetection::Peak(new_peak)),
                ));
            }
            return Ok((steps, false, Some(ClippingDetection::Peak(new_peak))));
        }
        return Ok((steps, false, None));
    }

    // Headroom-based branch (no ReplayGain analysis available). Lowering
    // or holding gain (steps <= 0) can never push the peak up, so there is
    // nothing to check.
    if steps <= 0 {
        return Ok((steps, false, None));
    }

    let headroom = if is_aac {
        #[cfg(feature = "aac")]
        {
            let analysis = aac::analyze_aac_gains(file_path).ok();
            let headroom = analysis
                .as_ref()
                .map(|a| (MAX_GAIN as i32).saturating_sub(a.max_gain() as i32));
            *aac_analysis = analysis;
            headroom
        }
        #[cfg(not(feature = "aac"))]
        {
            let _ = file_path;
            let _ = &aac_analysis;
            None
        }
    } else {
        // Read once and keep the bytes in `mp3_data` so the apply step can
        // reuse them instead of a second full read + frame walk (issue #251).
        // A failed read leaves `None`; the apply step re-reads and surfaces
        // the proper I/O error.
        let data = std::fs::read(file_path).ok();
        let headroom = data
            .as_deref()
            .and_then(|d| crate::analyze_data(d).ok())
            .map(|i| i.headroom_steps());
        *mp3_data = data;
        headroom
    };

    if let Some(h) = headroom {
        if steps > h {
            if opts.prevent_clipping {
                return Ok((h, true, Some(ClippingDetection::Headroom(h))));
            }
            return Ok((steps, false, Some(ClippingDetection::Headroom(h))));
        }
    }

    Ok((steps, false, None))
}

#[cfg(feature = "aac")]
fn apply_aac_bytes(
    file_path: &Path,
    steps: i32,
    opts: &ApplyOptions,
    analysis: AacAnalysisCache,
) -> Result<usize> {
    with_temp_file(file_path, |r, w| {
        if opts.write_undo {
            aac::apply_aac_gain_with_undo_to_path_with_analysis(r, w, steps, analysis)
        } else {
            aac::apply_aac_gain_to_path_with_analysis(r, w, steps, analysis)
        }
    })
}

#[cfg(not(feature = "aac"))]
fn apply_aac_bytes(
    _file_path: &Path,
    _steps: i32,
    _opts: &ApplyOptions,
    _analysis: AacAnalysisCache,
) -> Result<usize> {
    Err(Error::FeatureNotAvailable {
        feature: "AAC support",
        feature_flag: "aac",
    })
}

/// Post-apply residual ReplayGain values — the gain a player should still
/// apply on top of the gain already baked into global_gain, stored at
/// 6-decimal precision per mp3gain convention (issue #210).
struct RgResidual {
    track_gain_db: f64,
    track_peak: f64,
    album: Option<(f64, f64)>,
    mode: crate::replaygain::AnalysisMode,
}

impl RgResidual {
    fn to_ape(&self) -> ape::ApeReplayGain {
        ape::ApeReplayGain {
            track_gain: Some(ape::format_rg_gain(self.track_gain_db)),
            track_peak: Some(ape::format_rg_peak(self.track_peak)),
            album_gain: self.album.map(|(g, _)| ape::format_rg_gain(g)),
            album_peak: self.album.map(|(_, p)| ape::format_rg_peak(p)),
            algorithm: self.mode.algorithm_tag().map(str::to_string),
        }
    }

    fn to_id3v2(&self) -> id3v2::Id3v2ReplayGain {
        let ape = self.to_ape();
        id3v2::Id3v2ReplayGain {
            track_gain: ape.track_gain,
            track_peak: ape.track_peak,
            album_gain: ape.album_gain,
            album_peak: ape.album_peak,
            algorithm: ape.algorithm,
            ..Default::default()
        }
    }
}

/// Compute the residual track/album ReplayGain after applying `actual_steps`.
///
/// Absent global_gain saturation, applying N steps shifts loudness by exactly
/// N*1.5 dB, so the residual is arithmetic. With `reanalyze` (wrap mode or a
/// saturating apply) the shift is not uniform, so the already-modified file at
/// `modified_path` is re-analyzed for the true post-apply values, falling back
/// to arithmetic on analysis failure.
fn compute_rg_residual(
    modified_path: &Path,
    opts: &ApplyOptions,
    actual_steps: i32,
    reanalyze: bool,
) -> Option<RgResidual> {
    let track = opts.track_result.as_ref()?;
    // Re-analysis must use the mode the original measurement used, or the
    // residual would mix an RG1 reading into an RG2/R128 gain value.
    let mode = track.analysis_mode();

    let arithmetic = || {
        let db = steps_to_db(actual_steps);
        (
            track.gain_db() - db,
            apply_gain_to_peak(track.peak(), db),
            db,
        )
    };
    let (track_gain_db, track_peak, applied_db) = if reanalyze {
        // Match the original measurement exactly: same mode, and true peak
        // when the original peak was a true peak (issue #292).
        let reopts = crate::replaygain::TrackAnalysisOptions {
            mode,
            true_peak: track.is_true_peak(),
            ..Default::default()
        };
        match crate::replaygain::analyze_track_with_options(modified_path, &reopts) {
            Ok(post) => (
                post.gain_db(),
                post.peak(),
                track.gain_db() - post.gain_db(),
            ),
            Err(_) => arithmetic(),
        }
    } else {
        arithmetic()
    };

    // Album residual: the same loudness shift applies to the album gain/peak
    // (the album value is uniform across the set).
    let album = opts.album_info.map(|a| {
        (
            a.album_gain_db - applied_db,
            apply_gain_to_peak(a.album_peak, applied_db),
        )
    });

    Some(RgResidual {
        track_gain_db,
        track_peak,
        album,
        mode,
    })
}

fn apply_mp3_ape_bytes(
    file_path: &Path,
    steps: i32,
    opts: &ApplyOptions,
    replaygain: Option<ape::ApeReplayGain>,
    preread: Option<Vec<u8>>,
) -> Result<SaturationStats> {
    with_temp_file(file_path, |r, w| {
        let mut gain = GainOptions::new(steps)
            .wrap(opts.wrap)
            .undo(opts.write_undo);
        if let Some(ch) = opts.channel {
            gain = gain.channel(ch);
        }
        if let Some(rg) = replaygain {
            gain = gain.replaygain(rg);
        }
        gain.apply_to_path_with_stats_preread(r, w, preread)
    })
}

fn apply_mp3_id3v2_bytes(
    file_path: &Path,
    steps: i32,
    opts: &ApplyOptions,
    preread: Option<Vec<u8>>,
) -> Result<SaturationStats> {
    // APE undo is never written in `-s i` mode; undo/minmax and the
    // REPLAYGAIN_* values go into TXXX frames instead. All frames are written
    // onto the temp file before the rename so gain + tags land in one visible
    // write — a failed tag write leaves the original untouched (issues #227,
    // #232).
    with_temp_file(file_path, |r, w| {
        let mut gain = GainOptions::new(steps).wrap(opts.wrap).undo(false);
        if let Some(ch) = opts.channel {
            gain = gain.channel(ch);
        }
        let stats = gain.apply_to_path_with_stats_preread(r, w, preread)?;

        let mut rg = id3v2::Id3v2ReplayGain::default();

        if opts.write_undo {
            let (delta_left, delta_right) = match opts.channel {
                Some(Channel::Left) => (steps, 0),
                Some(Channel::Right) => (0, steps),
                None => (steps, steps),
            };
            let existing = id3v2::read_id3v2_replaygain(w).unwrap_or_default();
            let (existing_left, existing_right) = ape::parse_undo_values(existing.undo.as_deref());

            // MP3GAIN_UNDO stores the *undo* delta (mp3gain convention, issue
            // #210): applying `+delta` makes the stored undo `-delta`,
            // accumulating by subtraction onto any prior undo value.
            rg.undo = Some(ape::format_undo_value(
                existing_left - delta_left,
                existing_right - delta_right,
                opts.wrap,
            ));

            // MP3GAIN_MINMAX is the *post-apply* global_gain range (mp3gain
            // convention). A full apply already tracked it in the saturation
            // stats (issue #231); channel and zero-step applies only touch a
            // subset (or nothing), so scan the modified temp file instead.
            let (min, max) = if opts.channel.is_none() && stats.frames > 0 {
                (stats.min_gain, stats.max_gain)
            } else {
                let post = crate::analyze(w)?;
                (post.min_gain(), post.max_gain())
            };
            rg.minmax = Some(ape::format_minmax(min, max));
        }

        if opts.write_replaygain_tags {
            let reanalyze = opts.wrap || stats.saturated_low > 0 || stats.saturated_high > 0;
            if let Some(res) = compute_rg_residual(w, opts, steps, reanalyze) {
                let values = res.to_id3v2();
                rg.track_gain = values.track_gain;
                rg.track_peak = values.track_peak;
                rg.album_gain = values.album_gain;
                rg.album_peak = values.album_peak;
                rg.algorithm = values.algorithm;
            }
        }

        if rg.undo.is_some() || rg.track_gain.is_some() {
            id3v2::write_id3v2_replaygain_direct(w, &rg)?;
        }
        Ok(stats)
    })
}

/// Build a unique `.mp3rgain_temp_*` sibling path for `file` so parallel tasks
/// writing in the same directory never collide (one process-wide counter).
fn temp_sibling_path(file: &Path, ext: &str) -> std::path::PathBuf {
    let parent = file.parent().unwrap_or(Path::new("."));
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".mp3rgain_temp_{}_{}.{}",
        std::process::id(),
        counter,
        ext
    ))
}

/// Retry `op` on transient Windows sharing violations (issue #303).
///
/// On SMB shares an antivirus scanner, Windows Search, or the SMB
/// redirector's handle caching can briefly hold the temp file between our
/// close and the next open, failing it with `ERROR_SHARING_VIOLATION` (32)
/// or `ERROR_LOCK_VIOLATION` (33). Back off and retry; any other error is
/// returned immediately. No-op wrapper on non-Windows.
#[cfg(windows)]
fn retry_sharing_violation<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut delay = std::time::Duration::from_millis(10);
    for _ in 0..8 {
        match op() {
            Err(e) if matches!(e.raw_os_error(), Some(32) | Some(33)) => {
                std::thread::sleep(delay);
                delay *= 2;
            }
            other => return other,
        }
    }
    op()
}

#[cfg(not(windows))]
fn retry_sharing_violation<T>(mut op: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    op()
}

/// Copy `original`'s permissions onto `temp`, fsync it, and rename it over
/// `original` (issue #227).
fn persist_temp(original: &Path, temp: &Path) -> Result<()> {
    let finish = || -> std::io::Result<()> {
        // fsync needs a writable handle on Windows, and must happen before
        // the permission copy in case the original mode is read-only.
        retry_sharing_violation(|| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(temp)?
                .sync_all()
        })?;
        if let Ok(meta) = std::fs::metadata(original) {
            retry_sharing_violation(|| std::fs::set_permissions(temp, meta.permissions()))?;
        }
        retry_sharing_violation(|| std::fs::rename(temp, original))
    };
    finish().map_err(|e| Error::io_write(original, e))
}

/// Atomically replace `path`'s contents: write a sibling temp file, copy
/// permissions, fsync, rename. A failure leaves the original untouched
/// (issue #227).
pub(crate) fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    with_temp_file(path, |original, temp| {
        std::fs::write(temp, data).map_err(|e| Error::io_write(original, e))
    })
}

/// Run `operation(original, temp)` against a fresh sibling temp path, then
/// fsync the temp file and rename it over the original (issue #227). The temp
/// file is removed on failure, leaving the original untouched.
pub(crate) fn with_temp_file<T, F>(file: &Path, operation: F) -> Result<T>
where
    F: FnOnce(&Path, &Path) -> Result<T>,
{
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("tmp");
    let temp_path = temp_sibling_path(file, ext);
    let result = operation(file, &temp_path).and_then(|value| {
        persist_temp(file, &temp_path)?;
        Ok(value)
    });
    if result.is_err() {
        let _ = retry_sharing_violation(|| std::fs::remove_file(&temp_path));
    }
    result
}

/// Restore a previously-captured modified-time on `file`.
///
/// Silently no-ops on failure — timestamp preservation is best-effort.
pub fn restore_timestamp(file: &Path, mtime: SystemTime) {
    let _ = std::fs::File::options()
        .write(true)
        .open(file)
        .and_then(|f| f.set_times(std::fs::FileTimes::new().set_modified(mtime)));
}

/// Read the current modified-time of `path`, returning `None` on any failure.
pub fn read_mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}

/// Aggregate the post-apply `global_gain` range across an album's MP3 files
/// and write `MP3GAIN_ALBUM_MINMAX` (`min,max`) to each, matching mp3gain's
/// album (`-a`) mode (issue #210).
///
/// The album-wide range is only known once every file has had its gain
/// applied, so callers invoke this *after* the per-file apply pass. Shared by
/// the CLI (`cmd_album_gain`) and the GUI apply worker so both frontends stay
/// in parity (the GUI path was missing it in 2.9.0).
///
/// Each file is paired with the `(max, min)` range from
/// [`ApplyReport::gain_range`] so the apply pass's scan is reused instead of
/// re-analyzing every file (issue #232); files without one (AAC members,
/// zero-frame applies, failed applies) fall back to a fresh `analyze()`.
///
/// AAC members are skipped — the MP3 analyzer rejects them and mp3gain has no
/// AAC. Best-effort: a failed scan or tag write on one file is ignored so a
/// metadata hiccup never fails the album operation. Intended for the default
/// APEv2 path; skip the call when writing ID3v2 (`-s i`) or when stored-tag
/// writing is disabled.
pub fn write_album_minmax(files: &[(&Path, Option<(u8, u8)>)]) {
    use rayon::prelude::*;

    // The fallback analyze() is a full-file frame walk, so run the range
    // collection in parallel like the apply pass that precedes it (#252).
    let ranges: Vec<Option<(u8, u8)>> = files
        .par_iter()
        .map(|&(file, range)| {
            range.or_else(|| {
                crate::analyze(file)
                    .ok()
                    .map(|a| (a.max_gain(), a.min_gain()))
            })
        })
        .collect();

    let mut album_min = u8::MAX;
    let mut album_max = u8::MIN;
    let mut mp3_files: Vec<&Path> = Vec::new();
    for (&(file, _), range) in files.iter().zip(&ranges) {
        if let Some((max, min)) = *range {
            album_min = album_min.min(min);
            album_max = album_max.max(max);
            mp3_files.push(file);
        }
    }
    mp3_files.par_iter().for_each(|file| {
        let _ = ape::write_ape_album_minmax(file, album_min, album_max);
    });
}

#[cfg(test)]
#[cfg(feature = "replaygain")]
mod tests {
    use super::*;
    use crate::replaygain::AudioFileType;

    fn track_with_peak(peak: f64) -> ReplayGainResult {
        ReplayGainResult::new(
            0.0,
            0.0,
            peak,
            44_100,
            AudioFileType::Mp3,
            Default::default(),
        )
    }

    fn opts_with_track(steps: i32, peak: f64, prevent_clipping: bool) -> ApplyOptions {
        let mut opts = ApplyOptions::new(steps);
        opts.track_result = Some(track_with_peak(peak));
        opts.prevent_clipping = prevent_clipping;
        opts
    }

    /// Issue #162: a peak of 0.9 leaves ~0.915 dB of headroom. The old
    /// `round()`-based cap returned 1 step (1.5 dB) — overshooting headroom
    /// and re-introducing clipping. The `floor()` fix must return 0 steps.
    #[test]
    fn prevent_clipping_caps_at_floor_not_round() {
        let opts = opts_with_track(5, 0.9, true);
        let (steps, prevented, _) =
            check_clipping(Path::new("unused"), &opts, false, &mut None, &mut None).unwrap();
        assert!(prevented);
        assert_eq!(steps, 0);
        let new_peak = 0.9 * 10.0_f64.powf(steps_to_db(steps) / 20.0);
        assert!(new_peak <= 1.0, "capped output still clips ({new_peak})");
    }

    /// Sweep a range of sub-unity peaks. The cap may never push the new
    /// peak above 1.0.
    #[test]
    fn prevent_clipping_never_overshoots_headroom() {
        for &peak in &[0.55_f64, 0.6, 0.7, 0.8, 0.85, 0.9, 0.95, 0.99] {
            let opts = opts_with_track(20, peak, true);
            let (steps, prevented, _) =
                check_clipping(Path::new("unused"), &opts, false, &mut None, &mut None).unwrap();
            assert!(prevented, "peak {peak} should trigger prevention");
            let new_peak = peak * 10.0_f64.powf(steps_to_db(steps) / 20.0);
            assert!(
                new_peak <= 1.0,
                "peak {peak} -> capped steps {steps} still clips ({new_peak})"
            );
        }
    }

    /// Sanity: when there is no clipping risk, prevent_clipping must pass
    /// the requested steps through unchanged.
    #[test]
    fn prevent_clipping_passthrough_when_safe() {
        let opts = opts_with_track(3, 0.5, true);
        let (steps, prevented, _) =
            check_clipping(Path::new("unused"), &opts, false, &mut None, &mut None).unwrap();
        assert!(!prevented);
        assert_eq!(steps, 3);
    }

    /// Issue #173: when the source file already clips (peak > 1.0), the cap
    /// must be allowed to go negative so the file is attenuated below its
    /// current loudness, matching the original MP3GainGUI behavior. The old
    /// `max(0)` clamp left such files clipping because it pinned the cap at
    /// 0 steps.
    #[test]
    fn prevent_clipping_returns_negative_for_already_clipping_source() {
        // peak 1.2 = ~ -1.58 dB of (negative) headroom -> floor(-1.58/1.5)
        // = -2 steps (= -3 dB). Resulting peak = 1.2 * 10^(-3/20) = 0.85.
        let opts = opts_with_track(1, 1.2, true);
        let (steps, prevented, _) =
            check_clipping(Path::new("unused"), &opts, false, &mut None, &mut None).unwrap();
        assert!(prevented);
        assert!(steps < 0, "expected negative steps, got {steps}");
        let new_peak = 1.2 * 10.0_f64.powf(steps_to_db(steps) / 20.0);
        assert!(
            new_peak <= 1.0,
            "capped output still clips ({new_peak}) at steps={steps}"
        );
    }

    /// Issue #206: a track already at the reference loudness nets 0 gain
    /// steps. If it already clips (peak > 1.0), `-k` must still attenuate it
    /// — the old `steps <= 0` early-out skipped the peak check entirely.
    #[test]
    fn prevent_clipping_caps_zero_step_clipping_track() {
        let opts = opts_with_track(0, 1.2, true);
        let (steps, prevented, _) =
            check_clipping(Path::new("unused"), &opts, false, &mut None, &mut None).unwrap();
        assert!(prevented);
        assert!(steps < 0, "expected attenuation, got {steps}");
        let new_peak = 1.2 * 10.0_f64.powf(steps_to_db(steps) / 20.0);
        assert!(new_peak <= 1.0, "capped output still clips ({new_peak})");
    }

    /// Issue #206: a 0-step track that does not clip must pass through
    /// unchanged with no clipping signal.
    #[test]
    fn zero_step_non_clipping_track_is_noop() {
        let opts = opts_with_track(0, 0.8, true);
        let (steps, prevented, detected) =
            check_clipping(Path::new("unused"), &opts, false, &mut None, &mut None).unwrap();
        assert_eq!(steps, 0);
        assert!(!prevented);
        assert!(detected.is_none());
    }

    /// Sweep clipping peaks (> 1.0). Cap must always produce a non-clipping
    /// post-apply peak — independent of how many positive steps were
    /// requested.
    #[test]
    fn prevent_clipping_never_overshoots_for_clipping_source() {
        for &peak in &[1.001_f64, 1.05, 1.1, 1.2, 1.5, 2.0] {
            let opts = opts_with_track(5, peak, true);
            let (steps, prevented, _) =
                check_clipping(Path::new("unused"), &opts, false, &mut None, &mut None).unwrap();
            assert!(prevented, "peak {peak} should trigger prevention");
            let new_peak = peak * 10.0_f64.powf(steps_to_db(steps) / 20.0);
            assert!(
                new_peak <= 1.0,
                "peak {peak} -> capped steps {steps} still clips ({new_peak})"
            );
        }
    }
}
