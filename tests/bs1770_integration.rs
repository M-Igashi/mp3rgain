//! End-to-end checks of the BS.1770 analysis path (issue #270): decode via
//! symphonia -> K-weighting -> gating -> gain. The expected LUFS values were
//! cross-checked against `ffmpeg -af ebur128` (agreement within 0.05 LU).

#![cfg(feature = "replaygain")]

use mp3rgain::replaygain::{
    analyze_album_with_options, analyze_track, analyze_track_with_mode, AlbumAnalysisOptions,
    AnalysisMode, R128_REFERENCE_LUFS, RG2_REFERENCE_LUFS,
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
