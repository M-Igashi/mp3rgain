//! End-to-end tests that drive the `mp3rgain` binary.
//!
//! `integration_tests.rs` covers the library API; the command layer
//! (`src/commands/`) decides *whether* a file is processed at all, and that
//! logic is only reachable through the CLI. Cargo exposes the built binary as
//! `CARGO_BIN_EXE_mp3rgain`, so no extra tooling is needed.

use mp3rgain::{read_ape_tag_from_file, TAG_REPLAYGAIN_TRACK_GAIN};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A temp directory holding copies of the requested fixtures, removed on drop
/// so a failing assertion never leaves files behind.
struct TempAlbum {
    dir: PathBuf,
    files: Vec<PathBuf>,
}

impl TempAlbum {
    fn new(fixtures: &[&str]) -> Self {
        let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("mp3rgain_cli_{}_{}", std::process::id(), id));
        fs::create_dir_all(&dir).expect("create temp dir");

        let files = fixtures
            .iter()
            .map(|name| {
                let dst = dir.join(name);
                fs::copy(Path::new("tests/fixtures").join(name), &dst).expect("copy fixture");
                dst
            })
            .collect();

        TempAlbum { dir, files }
    }

    fn args(&self) -> Vec<&str> {
        self.files.iter().map(|p| p.to_str().unwrap()).collect()
    }
}

impl Drop for TempAlbum {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mp3rgain"))
        .args(args)
        .output()
        .expect("failed to run mp3rgain")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Album gain in steps, read from the JSON dry-run report.
fn album_gain_steps(files: &[&str]) -> i64 {
    let mut args = vec!["-a", "-n", "-s", "s", "-o", "json"];
    args.extend_from_slice(files);
    let out = run(&args);
    let json: serde_json::Value =
        serde_json::from_str(&stdout_of(&out)).expect("album dry run should emit JSON");
    json["album"]["gain_steps"]
        .as_i64()
        .expect("album report should carry gain_steps")
}

fn track_gain_tag(file: &Path) -> Option<String> {
    read_ape_tag_from_file(file)
        .expect("reading the APE tag should not fail")
        .and_then(|tag| tag.get(TAG_REPLAYGAIN_TRACK_GAIN).map(str::to_string))
}

/// An album whose gain rounds to 0 steps must still get per-track
/// ReplayGain tags. Reported on the foobar2000 forum: album gain -0.04 dB
/// (0 steps) made mp3rgain skip the album outright, discarding a +1.46 dB
/// track gain it had already measured. The track path fixed the same class
/// of bug in issue #206.
#[test]
fn album_at_zero_steps_still_writes_replaygain_tags() {
    let album = TempAlbum::new(&["test_stereo.mp3", "test_mono.mp3"]);
    let files = album.args();

    // Cancel the album gain with -m so the run lands on exactly 0 steps,
    // whatever the fixtures happen to measure.
    let offset = (-album_gain_steps(&files)).to_string();
    let mut args = vec!["-a", "-m", offset.as_str()];
    args.extend_from_slice(&files);
    let out = run(&args);
    assert!(out.status.success(), "album run failed: {:?}", out);

    let gains: Vec<Option<String>> = album.files.iter().map(|f| track_gain_tag(f)).collect();
    for (file, gain) in album.files.iter().zip(&gains) {
        assert!(
            gain.is_some(),
            "{} got no REPLAYGAIN_TRACK_GAIN despite a 0-step album gain",
            file.display()
        );
    }

    // The whole point of writing them: the per-track values are not the
    // album value, so dropping them loses real information.
    assert_ne!(
        gains[0], gains[1],
        "fixtures should have distinct track gains for this test to mean anything"
    );
}

/// `-s s` opts out of tag writing, so a 0-step album has genuinely nothing
/// left to do and keeps the cheap skip.
#[test]
fn album_at_zero_steps_skips_when_tags_are_disabled() {
    let album = TempAlbum::new(&["test_stereo.mp3", "test_mono.mp3"]);
    let files = album.args();

    let offset = (-album_gain_steps(&files)).to_string();
    let mut args = vec!["-a", "-s", "s", "-m", offset.as_str()];
    args.extend_from_slice(&files);
    let out = run(&args);
    assert!(out.status.success(), "album run failed: {:?}", out);
    assert!(
        stdout_of(&out).contains("No adjustment needed"),
        "expected the skip path, got: {}",
        stdout_of(&out)
    );

    for file in &album.files {
        assert_eq!(
            track_gain_tag(file),
            None,
            "{} should have no tags in -s s mode",
            file.display()
        );
    }
}

/// Track mode already had this behaviour (issue #206); keep it covered so the
/// two paths can't drift apart again.
#[test]
fn track_at_zero_steps_still_writes_replaygain_tags() {
    let album = TempAlbum::new(&["test_stereo.mp3"]);
    let files = album.args();

    let mut args = vec!["-r"];
    args.extend_from_slice(&files);
    assert!(run(&args).status.success());

    // Second pass: the file now sits on target, so the gain is 0 steps.
    let out = run(&args);
    assert!(out.status.success(), "second track run failed: {:?}", out);
    assert!(
        track_gain_tag(&album.files[0]).is_some(),
        "an already-normalized track should keep its ReplayGain tags"
    );
}
