//! End-to-end tests that drive the `mp3rgain` binary.
//!
//! `integration_tests.rs` covers the library API; the command layer
//! (`src/commands/`) decides *whether* a file is processed at all, and that
//! logic is only reachable through the CLI. Cargo exposes the built binary as
//! `CARGO_BIN_EXE_mp3rgain`, so no extra tooling is needed.

use mp3rgain::{read_ape_tag_from_file, TAG_MP3GAIN_UNDO, TAG_REPLAYGAIN_TRACK_GAIN};
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

/// `REPLAYGAIN_TRACK_GAIN` from wherever it is stored, so tests that only care
/// *whether* the file was tagged stay independent of the container layout.
fn track_gain_tag(file: &Path) -> Option<String> {
    mp3rgain::read_id3v2_replaygain(file)
        .expect("reading the ID3v2 tag should not fail")
        .track_gain
        .or_else(|| {
            read_ape_tag_from_file(file)
                .expect("reading the APE tag should not fail")
                .and_then(|tag| tag.get(TAG_REPLAYGAIN_TRACK_GAIN).map(str::to_string))
        })
}

/// An album whose gain rounds to 0 steps must still get per-track
/// ReplayGain tags. Reported on the Hydrogenaudio forum: album gain -0.04 dB
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

fn id3v2_track_gain(file: &Path) -> Option<String> {
    mp3rgain::read_id3v2_replaygain(file)
        .expect("reading the ID3v2 tag should not fail")
        .track_gain
}

fn ape_has_replaygain(file: &Path) -> bool {
    read_ape_tag_from_file(file)
        .expect("reading the APE tag should not fail")
        .is_some_and(|tag| tag.get(TAG_REPLAYGAIN_TRACK_GAIN).is_some())
}

fn ape_has_undo(file: &Path) -> bool {
    read_ape_tag_from_file(file)
        .expect("reading the APE tag should not fail")
        .is_some_and(|tag| tag.get(TAG_MP3GAIN_UNDO).is_some())
}

/// The default layout sends each tag family to the container its readers use:
/// `REPLAYGAIN_*` to ID3v2 (ffmpeg does not read APEv2 on MP3 at all, and
/// Rockbox only handles APE tags for WavPack/Musepack), `MP3GAIN_*` to APEv2
/// where the mp3gain lineage looks.
#[test]
fn default_layout_splits_replaygain_into_id3v2() {
    let album = TempAlbum::new(&["test_stereo.mp3"]);
    let file = &album.files[0];

    let mut args = vec!["-r"];
    args.extend_from_slice(&album.args());
    assert!(run(&args).status.success());

    assert!(
        id3v2_track_gain(file).is_some(),
        "ReplayGain should land in ID3v2 by default"
    );
    assert!(
        !ape_has_replaygain(file),
        "APEv2 should not carry a second, divergent copy of the ReplayGain values"
    );
    assert!(
        ape_has_undo(file),
        "MP3GAIN_UNDO belongs in APEv2 — that is where mp3gain reads it"
    );
}

/// `-s a` keeps the historical mp3gain-identical layout.
#[test]
fn ape_layout_keeps_everything_in_apev2() {
    let album = TempAlbum::new(&["test_stereo.mp3"]);
    let file = &album.files[0];

    let mut args = vec!["-r", "-s", "a"];
    args.extend_from_slice(&album.args());
    assert!(run(&args).status.success());

    assert!(
        ape_has_replaygain(file),
        "-s a should write ReplayGain to APEv2"
    );
    assert!(ape_has_undo(file));
    assert_eq!(
        id3v2_track_gain(file),
        None,
        "-s a should leave ID3v2 alone"
    );
}

/// Re-running under the default layout on a file previously tagged with `-s a`
/// must clear the APEv2 ReplayGain copy — otherwise a reader that prefers
/// APEv2 would keep seeing values that no longer match the ID3v2 ones.
#[test]
fn default_layout_clears_stale_apev2_replaygain() {
    let album = TempAlbum::new(&["test_stereo.mp3"]);
    let file = &album.files[0];
    let files = album.args();

    let mut ape_args = vec!["-r", "-s", "a"];
    ape_args.extend_from_slice(&files);
    assert!(run(&ape_args).status.success());
    assert!(
        ape_has_replaygain(file),
        "precondition: APEv2 ReplayGain present"
    );

    let mut args = vec!["-r"];
    args.extend_from_slice(&files);
    assert!(run(&args).status.success());

    assert!(
        !ape_has_replaygain(file),
        "stale APEv2 ReplayGain should be removed once ID3v2 owns the values"
    );
    assert!(id3v2_track_gain(file).is_some());
    assert!(ape_has_undo(file), "MP3GAIN_UNDO must survive the cleanup");
}

/// Undo has to find the undo tag under the split layout, and restore the
/// audio exactly.
#[test]
fn undo_works_under_the_default_layout() {
    let album = TempAlbum::new(&["test_stereo.mp3"]);
    let file = &album.files[0];
    let before = fs::read(Path::new("tests/fixtures/test_stereo.mp3")).unwrap();

    let mut args = vec!["-r"];
    args.extend_from_slice(&album.args());
    assert!(run(&args).status.success());

    let mut undo_args = vec!["-u"];
    undo_args.extend_from_slice(&album.args());
    let out = run(&undo_args);
    assert!(out.status.success(), "undo failed: {:?}", out);

    // Compare the audio payload only: the tags are expected to differ.
    let after = fs::read(file).unwrap();
    assert_eq!(
        audio_payload(&before),
        audio_payload(&after),
        "undo should restore the audio bit-for-bit"
    );
}

/// Strip a leading ID3v2 tag and a trailing APEv2 tag, leaving the frames.
fn audio_payload(data: &[u8]) -> &[u8] {
    let start = if data.starts_with(b"ID3") && data.len() > 10 {
        let size = ((data[6] as usize) << 21)
            | ((data[7] as usize) << 14)
            | ((data[8] as usize) << 7)
            | (data[9] as usize);
        10 + size
    } else {
        0
    };
    let end = data
        .windows(8)
        .rposition(|w| w == b"APETAGEX")
        .filter(|&i| i > start)
        .unwrap_or(data.len());
    &data[start..end]
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
