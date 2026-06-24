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
use crate::{ape, id3v2, mp4meta};

/// Per-process counter for `.mp3rgain_temp_*` filenames so parallel
/// apply tasks operating on files in the same directory don't collide.
/// Lifted from `src/processors/utils.rs`.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// AAC analysis cached between the clipping check and the apply step so a
/// single apply never walks the bitstream twice (issue #188). Mirrors the
/// MP3 `mp3_analysis` cache from #135.
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

    /// `-t`: write through a sibling temp file and rename atomically.
    pub use_temp_file: bool,

    /// Record an undo tag (APE for MP3, MP4 freeform for AAC, ID3v2 TXXX
    /// when [`Self::use_id3v2`] is on).
    pub write_undo: bool,

    /// Write ReplayGain metadata tags. Requires [`Self::track_result`] to
    /// be set. AAC writes to mp4 freeform metadata; MP3 writes ID3v2 TXXX
    /// frames when [`Self::use_id3v2`] is on, otherwise APEv2 `REPLAYGAIN_*`
    /// items (issue #204).
    pub write_replaygain_tags: bool,

    /// `-s i`: MP3 only — use ID3v2 TXXX frames for undo and ReplayGain
    /// instead of APE.
    pub use_id3v2: bool,

    /// `-l`: MP3 only — apply gain to a single channel (Stereo / Dual
    /// Channel) instead of all channels. AAC has no per-channel apply
    /// path; setting this on an AAC file is an error.
    pub channel: Option<Channel>,
}

impl ApplyOptions {
    /// Construct with a requested step count and "safe" defaults
    /// (undo on, no temp file, no clipping prevention, no tag writing).
    pub fn new(steps: i32) -> Self {
        Self {
            steps,
            track_result: None,
            album_info: None,
            prevent_clipping: false,
            wrap: false,
            preserve_timestamp: false,
            use_temp_file: false,
            write_undo: true,
            write_replaygain_tags: false,
            use_id3v2: false,
            channel: None,
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
/// 2. Gain application — MP3 APE / MP3 ID3v2 / AAC, with optional temp
///    file + atomic rename.
/// 3. ID3v2 undo tag write (MP3 + [`ApplyOptions::use_id3v2`]).
/// 4. ReplayGain tag write (AAC mp4 metadata, MP3 ID3v2 TXXX with
///    [`ApplyOptions::use_id3v2`], or MP3 APEv2 by default — issue #204).
/// 5. Mtime restoration when [`ApplyOptions::preserve_timestamp`] is on.
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
    // Both branches cache their analysis so the apply step below can
    // reuse it instead of a second file scan: the MP3 `analyze(file)`
    // result feeds the ID3v2 undo write (issue #135), the AAC bitstream
    // analysis feeds the gain application itself (issue #188).
    let mut mp3_analysis: Option<crate::Mp3Analysis> = None;
    let mut aac_analysis: AacAnalysisCache = None;
    let (actual_steps, clipping_prevented, clipping_detected) = check_clipping(
        file_path,
        opts,
        is_aac,
        &mut mp3_analysis,
        &mut aac_analysis,
    )?;

    // 2) Apply gain to bytes. MP3 reports global_gain saturation (issue
    // #207); AAC clamps in its own path and isn't tallied here.
    let mut saturation = SaturationStats::default();
    let modified = if is_aac {
        apply_aac_bytes(file_path, actual_steps, opts, aac_analysis)?
    } else if opts.use_id3v2 {
        saturation = apply_mp3_id3v2_bytes(file_path, actual_steps, opts, &mut mp3_analysis)?;
        saturation.frames
    } else {
        saturation = apply_mp3_ape_bytes(file_path, actual_steps, opts)?;
        saturation.frames
    };

    // 3) ReplayGain tag write.
    //
    // AAC writes to mp4 freeform metadata; MP3 writes ID3v2 TXXX frames in
    // `-s i` mode, otherwise APEv2 `REPLAYGAIN_*` items (the default,
    // mp3gain-compatible mode — issue #204). All three carry the same
    // analysis values; only the container differs.
    if opts.write_replaygain_tags {
        if let Some(track) = opts.track_result.as_ref() {
            if is_aac {
                let mut tags = mp4meta::ReplayGainTags::default();
                tags.set_track(track.gain_db(), track.peak());
                if let Some(album) = opts.album_info {
                    tags.set_album(album.album_gain_db, album.album_peak);
                }
                mp4meta::write_replaygain_tags(file_path, &tags)?;
            } else if opts.use_id3v2 {
                let rg = id3v2::Id3v2ReplayGain {
                    track_gain: Some(format!("{:+.2} dB", track.gain_db())),
                    track_peak: Some(format!("{:.6}", track.peak())),
                    album_gain: opts
                        .album_info
                        .map(|a| format!("{:+.2} dB", a.album_gain_db)),
                    album_peak: opts.album_info.map(|a| format!("{:.6}", a.album_peak)),
                    ..Default::default()
                };
                id3v2::write_id3v2_replaygain(file_path, &rg)?;
            } else {
                let rg = ape::ApeReplayGain {
                    track_gain: Some(format!("{:+.2} dB", track.gain_db())),
                    track_peak: Some(format!("{:.6}", track.peak())),
                    album_gain: opts
                        .album_info
                        .map(|a| format!("{:+.2} dB", a.album_gain_db)),
                    album_peak: opts.album_info.map(|a| format!("{:.6}", a.album_peak)),
                };
                ape::write_ape_replaygain(file_path, &rg)?;
            }
        }
    }

    // 4) Restore mtime.
    if let Some(mtime) = original_mtime {
        restore_timestamp(file_path, mtime);
    }

    Ok(ApplyReport {
        modified,
        actual_steps,
        clipping_prevented,
        clipping_detected,
        saturated_low: saturation.saturated_low,
        saturated_high: saturation.saturated_high,
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
    let mut mp3_analysis: Option<crate::Mp3Analysis> = None;
    let mut aac_analysis: AacAnalysisCache = None;
    let (actual_steps, clipping_prevented, clipping_detected) = check_clipping(
        file_path,
        opts,
        is_aac,
        &mut mp3_analysis,
        &mut aac_analysis,
    )?;
    Ok(ApplyReport {
        modified: 0,
        actual_steps,
        clipping_prevented,
        clipping_detected,
        saturated_low: 0,
        saturated_high: 0,
    })
}

fn check_clipping(
    file_path: &Path,
    opts: &ApplyOptions,
    is_aac: bool,
    mp3_analysis: &mut Option<crate::Mp3Analysis>,
    aac_analysis: &mut AacAnalysisCache,
) -> Result<(i32, bool, Option<ClippingDetection>)> {
    let steps = opts.steps;
    if opts.wrap {
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
        let info = crate::analyze(file_path).ok();
        let headroom = info.as_ref().map(|i| i.headroom_steps());
        *mp3_analysis = info;
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
    with_temp_file(file_path, opts.use_temp_file, |r, w| {
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

fn apply_mp3_ape_bytes(
    file_path: &Path,
    steps: i32,
    opts: &ApplyOptions,
) -> Result<SaturationStats> {
    with_temp_file(file_path, opts.use_temp_file, |r, w| {
        let mut gain = GainOptions::new(steps)
            .wrap(opts.wrap)
            .undo(opts.write_undo);
        if let Some(ch) = opts.channel {
            gain = gain.channel(ch);
        }
        gain.apply_to_path_with_stats(r, w)
    })
}

fn apply_mp3_id3v2_bytes(
    file_path: &Path,
    steps: i32,
    opts: &ApplyOptions,
    mp3_analysis: &mut Option<crate::Mp3Analysis>,
) -> Result<SaturationStats> {
    // Need pre-apply analysis if undo will be written. Reuse the cached
    // one from the clipping-check pass when available (issue #135).
    if opts.write_undo && mp3_analysis.is_none() {
        *mp3_analysis = crate::analyze(file_path).ok();
    }

    // APE undo is never written in `-s i` mode; the undo goes into a
    // TXXX:MP3GAIN_UNDO frame instead, written below.
    let stats = with_temp_file(file_path, opts.use_temp_file, |r, w| {
        let mut gain = GainOptions::new(steps).wrap(opts.wrap).undo(false);
        if let Some(ch) = opts.channel {
            gain = gain.channel(ch);
        }
        gain.apply_to_path_with_stats(r, w)
    })?;

    if opts.write_undo {
        let (delta_left, delta_right) = match opts.channel {
            Some(Channel::Left) => (steps, 0),
            Some(Channel::Right) => (0, steps),
            None => (steps, steps),
        };
        write_id3v2_undo_after_apply(
            file_path,
            delta_left,
            delta_right,
            opts.wrap,
            mp3_analysis.as_ref(),
        )?;
    }
    Ok(stats)
}

/// Build a unique `.mp3rgain_temp_*` sibling path for `file`. Shared by the
/// MP3 temp-file write and the MP4 `atomic_write` so parallel tasks writing
/// in the same directory never collide (one process-wide counter).
pub(crate) fn temp_sibling_path(file: &Path, ext: &str) -> std::path::PathBuf {
    let parent = file.parent().unwrap_or(Path::new("."));
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(
        ".mp3rgain_temp_{}_{}.{}",
        std::process::id(),
        counter,
        ext
    ))
}

fn with_temp_file<T, F>(file: &Path, use_temp: bool, operation: F) -> Result<T>
where
    F: FnOnce(&Path, &Path) -> Result<T>,
{
    if !use_temp {
        return operation(file, file);
    }

    let temp_path = temp_sibling_path(file, "mp3");

    match operation(file, &temp_path) {
        Ok(value) => {
            std::fs::rename(&temp_path, file).map_err(|e| Error::io_write(file, e))?;
            Ok(value)
        }
        Err(e) => {
            let _ = std::fs::remove_file(&temp_path);
            Err(e)
        }
    }
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

fn write_id3v2_undo_after_apply(
    file: &Path,
    delta_left: i32,
    delta_right: i32,
    wrap: bool,
    analysis: Option<&crate::Mp3Analysis>,
) -> Result<()> {
    let existing_rg = id3v2::read_id3v2_replaygain(file).unwrap_or_default();
    let (existing_left, existing_right) = ape::parse_undo_values(existing_rg.undo.as_deref());

    let owned;
    let (min, max) = match analysis {
        Some(a) => (a.min_gain(), a.max_gain()),
        None => {
            owned = crate::analyze(file)?;
            (owned.min_gain(), owned.max_gain())
        }
    };

    id3v2::write_id3v2_undo(
        file,
        existing_left + delta_left,
        existing_right + delta_right,
        wrap,
        min,
        max,
    )
}

#[cfg(test)]
#[cfg(feature = "replaygain")]
mod tests {
    use super::*;
    use crate::replaygain::AudioFileType;

    fn track_with_peak(peak: f64) -> ReplayGainResult {
        ReplayGainResult::new(0.0, 0.0, peak, 44_100, AudioFileType::Mp3)
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
