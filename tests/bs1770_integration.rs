//! End-to-end checks of the BS.1770 analysis path (issue #270): decode via
//! symphonia -> K-weighting -> gating -> gain. The expected LUFS values were
//! cross-checked against `ffmpeg -af ebur128` (agreement within 0.05 LU).

#![cfg(feature = "replaygain")]

use mp3rgain::replaygain::{
    analyze_album_with_options, analyze_track, analyze_track_with_mode, analyze_track_with_options,
    AlbumAnalysisOptions, AnalysisMode, TrackAnalysisOptions, R128_REFERENCE_LUFS,
    RG2_REFERENCE_LUFS,
};
use std::path::Path;

fn lufs(file: &str) -> f64 {
    analyze_track_with_mode(
        Path::new(&format!("tests/fixtures/{}", file)),
        None,
        AnalysisMode::Rg2,
        None,
    )
    .unwrap()
    .loudness_db()
}

#[test]
fn fixture_lufs_matches_ffmpeg_ebur128() {
    assert!((lufs("test_mono.mp3") - -22.2).abs() < 0.1);
    assert!((lufs("test_joint_stereo.mp3") - -22.2).abs() < 0.1);
    assert!((lufs("test_vbr.mp3") - -21.8).abs() < 0.1);
}

#[test]
fn rg2_and_r128_share_measurement_and_differ_by_target() {
    let path = Path::new("tests/fixtures/test_mono.mp3");
    let rg2 = analyze_track_with_mode(path, None, AnalysisMode::Rg2, None).unwrap();
    let r128 = analyze_track_with_mode(path, None, AnalysisMode::R128, None).unwrap();

    assert_eq!(rg2.loudness_db(), r128.loudness_db());
    assert!((rg2.gain_db() - (RG2_REFERENCE_LUFS - rg2.loudness_db())).abs() < 1e-12);
    assert!((r128.gain_db() - (R128_REFERENCE_LUFS - r128.loudness_db())).abs() < 1e-12);
    assert_eq!(rg2.analysis_mode(), AnalysisMode::Rg2);
    assert_eq!(r128.analysis_mode(), AnalysisMode::R128);
}

#[test]
fn default_mode_is_unchanged_rg1() {
    let path = Path::new("tests/fixtures/test_mono.mp3");
    let default = analyze_track(path).unwrap();
    let rg1 = analyze_track_with_mode(path, None, AnalysisMode::Rg1, None).unwrap();
    assert_eq!(default, rg1);
    assert_eq!(default.analysis_mode(), AnalysisMode::Rg1);
}

/// Issue #292: `--true-peak` measures a BS.1770-4 Annex 2 true peak. It can
/// never read below the sample peak, must not change the loudness/gain
/// measurement, and stays within a fraction of a dB of the sample peak on
/// ordinary program material.
#[test]
fn true_peak_ge_sample_peak_and_loudness_unchanged() {
    for file in ["test_mono.mp3", "test_joint_stereo.mp3", "test_vbr.mp3"] {
        let path_string = format!("tests/fixtures/{}", file);
        let path = Path::new(&path_string);
        let sample = analyze_track_with_mode(path, None, AnalysisMode::Rg2, None).unwrap();
        let tp = analyze_track_with_options(
            path,
            &TrackAnalysisOptions {
                mode: AnalysisMode::Rg2,
                true_peak: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert!(!sample.is_true_peak());
        assert!(tp.is_true_peak());
        assert!(tp.peak() >= sample.peak(), "{file}");
        // Inter-sample overshoot on normal material is well under 3 dB.
        assert!(tp.peak() < sample.peak() * 1.5, "{file}");
        assert_eq!(tp.loudness_db(), sample.loudness_db(), "{file}");
        assert_eq!(tp.gain_db(), sample.gain_db(), "{file}");
    }
}

/// The flag is ignored in RG1 mode: peak stays mp3gain's MAX_AMPLITUDE
/// semantics, bit-identical to a plain RG1 analysis.
#[test]
fn true_peak_ignored_in_rg1_mode() {
    let path = Path::new("tests/fixtures/test_mono.mp3");
    let plain = analyze_track(path).unwrap();
    let flagged = analyze_track_with_options(
        path,
        &TrackAnalysisOptions {
            true_peak: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(plain, flagged);
    assert!(!flagged.is_true_peak());
}

#[test]
fn album_analysis_with_true_peak() {
    let mono = Path::new("tests/fixtures/test_mono.mp3");
    let vbr = Path::new("tests/fixtures/test_vbr.mp3");
    let sample = analyze_album_with_options(
        &[mono, vbr],
        &AlbumAnalysisOptions {
            mode: AnalysisMode::Rg2,
            ..Default::default()
        },
    )
    .unwrap();
    let tp = analyze_album_with_options(
        &[mono, vbr],
        &AlbumAnalysisOptions {
            mode: AnalysisMode::Rg2,
            true_peak: true,
            ..Default::default()
        },
    )
    .unwrap();

    assert!(tp.album.album_peak() >= sample.album.album_peak());
    assert_eq!(
        tp.album.album_loudness_db(),
        sample.album.album_loudness_db()
    );
    for track in tp.album.tracks() {
        assert!(track.is_true_peak());
    }
}

#[test]
fn album_analysis_in_rg2_mode() {
    let mono = Path::new("tests/fixtures/test_mono.mp3");
    let vbr = Path::new("tests/fixtures/test_vbr.mp3");
    let report = analyze_album_with_options(
        &[mono, vbr],
        &AlbumAnalysisOptions {
            mode: AnalysisMode::Rg2,
            ..Default::default()
        },
    )
    .unwrap();

    let album = &report.album;
    let (lo, hi) = (
        album
            .tracks()
            .iter()
            .map(|t| t.loudness_db())
            .fold(f64::INFINITY, f64::min),
        album
            .tracks()
            .iter()
            .map(|t| t.loudness_db())
            .fold(f64::NEG_INFINITY, f64::max),
    );
    // Album loudness over the concatenation lies between the track extremes.
    assert!(album.album_loudness_db() >= lo && album.album_loudness_db() <= hi);
    assert!(
        (album.album_gain_db() - (RG2_REFERENCE_LUFS - album.album_loudness_db())).abs() < 1e-12
    );
}
