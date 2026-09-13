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

/// Issue #305: `-u -s d` must undo the frame-level gain before deleting the
/// tags. The old dispatch deleted first, destroying MP3GAIN_UNDO and making
/// the applied gain permanently irreversible.
#[test]
fn undo_with_delete_tags_undoes_before_deleting() {
    let album = TempAlbum::new(&["test_stereo.mp3"]);
    let file = &album.files[0];
    let original_avg = mp3rgain::analyze(file).unwrap().avg_gain();

    // Apply -2 steps, which records MP3GAIN_UNDO. (Negative, because the
    // fixture's global_gain values sit at the 255 ceiling, where a positive
    // apply saturates into a no-op.)
    let out = run(&["-g", "-2", file.to_str().unwrap()]);
    assert!(out.status.success(), "apply failed: {:?}", out);
    assert_ne!(
        mp3rgain::analyze(file).unwrap().avg_gain(),
        original_avg,
        "setup: apply should have changed the gain"
    );

    let out = run(&["-u", "-s", "d", file.to_str().unwrap()]);
    assert!(out.status.success(), "undo+delete failed: {:?}", out);

    assert_eq!(
        mp3rgain::analyze(file).unwrap().avg_gain(),
        original_avg,
        "-u -s d did not undo the frame-level gain"
    );
    assert!(
        read_ape_tag_from_file(file).unwrap().is_none()
            || read_ape_tag_from_file(file)
                .unwrap()
                .is_some_and(|t| t.get(TAG_MP3GAIN_UNDO).is_none()),
        "tags were not deleted"
    );
}

/// Issue #305: `-u -s d` on a file that has no undo info must still delete
/// the tags instead of failing.
#[test]
fn undo_with_delete_tags_without_undo_info_still_deletes() {
    let album = TempAlbum::new(&["test_mono.mp3"]);
    let file = &album.files[0];

    let out = run(&["-u", "-s", "d", file.to_str().unwrap()]);
    assert!(out.status.success(), "undo+delete failed: {:?}", out);
    let text = stdout_of(&out);
    assert!(
        text.contains("no changes to undo, tags deleted"),
        "unexpected output: {}",
        text
    );
}

/// Track gain in dB parsed from wherever the tag was stored.
fn track_gain_db(file: &Path) -> Option<f64> {
    track_gain_tag(file).and_then(|s| mp3rgain::ape::parse_rg_gain(&s))
}

/// `(min_gain, max_gain)` global_gain range, the cheapest proof that the
/// audio frames were or were not rewritten.
fn gain_range(file: &Path) -> (u8, u8) {
    let info = mp3rgain::analyze(file).expect("analyze should succeed");
    (info.min_gain(), info.max_gain())
}

/// mp3gain-style suggested track gain in dB, from the TSV report.
fn suggested_gain_db(file: &Path) -> f64 {
    let out = run(&["-o", "tsv", file.to_str().unwrap()]);
    let text = stdout_of(&out);
    let row = text
        .lines()
        .nth(1)
        .expect("TSV output should carry a data row");
    row.split('\t')
        .nth(2)
        .expect("dB gain column")
        .parse()
        .expect("dB gain should parse")
}

fn json_of(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout_of(output)).expect("output should be JSON")
}

/// Run `args` against `file` in JSON mode and return the single file record.
fn json_file_record(args: &[&str], file: &Path) -> serde_json::Value {
    let mut argv = args.to_vec();
    argv.extend_from_slice(&["-o", "json", file.to_str().unwrap()]);
    let out = run(&argv);
    assert!(out.status.success(), "run {:?} failed: {:?}", args, out);
    json_of(&out)["files"][0].clone()
}

/// Issue #308: `--tags-only` writes the full ReplayGain value and leaves every
/// audio frame alone, so the listener can still switch ReplayGain off in their
/// player.
#[test]
fn tags_only_writes_absolute_gain_without_touching_audio() {
    let album = TempAlbum::new(&["test_mono.mp3"]);
    let file = &album.files[0];
    let before = gain_range(file);

    let out = run(&[
        "-r",
        "--tags-only",
        "-c",
        "-o",
        "json",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "tags-only run failed: {:?}", out);
    let json = json_of(&out);
    let entry = &json["files"][0];

    // Nothing was applied to the frames...
    assert_eq!(entry["gain_applied_steps"].as_i64(), Some(0));
    assert_eq!(gain_range(file), before, "audio frames were rewritten");

    // ...so the tag holds the *full* suggested gain rather than the residual
    // the apply path leaves behind. Contrast with a real -r run on an
    // identical copy, which bakes the gain into the frames and tags only
    // what is left over.
    let tag_gain = entry["tag_gain_db"].as_f64().expect("tag_gain_db");
    let written = track_gain_db(file).expect("REPLAYGAIN_TRACK_GAIN should be written");
    assert!((written - tag_gain).abs() < 0.01, "written {}", written);

    let applied = TempAlbum::new(&["test_mono.mp3"]);
    let applied_file = &applied.files[0];
    let suggested = suggested_gain_db(applied_file);
    assert!(
        (written - suggested).abs() < 0.01,
        "tags-only wrote {} but the analysis suggests {}",
        written,
        suggested
    );

    // The apply path tags what is left over after baking gain into the
    // frames, so its value is the same measurement minus whatever it applied.
    let record = json_file_record(&["-r", "-c"], applied_file);
    let applied_db = record["gain_applied_db"].as_f64().expect("gain_applied_db");
    let residual = track_gain_db(applied_file).expect("residual gain");
    assert!(
        (residual - (suggested - applied_db)).abs() < 0.05,
        "apply wrote {} but the residual of {} after {} dB is {}",
        residual,
        suggested,
        applied_db,
        suggested - applied_db
    );

    // No gain change happened, so nothing describes one.
    if let Some(tag) = read_ape_tag_from_file(file).unwrap() {
        assert!(
            tag.get(TAG_MP3GAIN_UNDO).is_none(),
            "--tags-only wrote an undo tag"
        );
    }
}

/// Issue #308, album mode: every file gets the same album value, and no
/// `MP3GAIN_ALBUM_MINMAX` is written since no apply produced a gain range.
#[test]
fn tags_only_album_writes_shared_album_tag_and_no_minmax() {
    let album = TempAlbum::new(&["test_stereo.mp3", "test_mono.mp3"]);
    let files = album.args();
    let before: Vec<(u8, u8)> = album.files.iter().map(|f| gain_range(f)).collect();

    let mut args = vec!["-a", "--tags-only", "-c"];
    args.extend_from_slice(&files);
    let out = run(&args);
    assert!(
        out.status.success(),
        "album tags-only run failed: {:?}",
        out
    );

    let mut album_gains = Vec::new();
    for (file, before) in album.files.iter().zip(&before) {
        assert_eq!(gain_range(file), *before, "audio frames were rewritten");
        let rg = mp3rgain::read_id3v2_replaygain(file).expect("ID3v2 read");
        album_gains.push(
            rg.album_gain
                .as_deref()
                .and_then(mp3rgain::ape::parse_rg_gain)
                .expect("REPLAYGAIN_ALBUM_GAIN should be written"),
        );
        // MP3GAIN_ALBUM_MINMAX is an APEv2 item describing a post-apply
        // global_gain range; there was no apply, so nothing should have been
        // appended at all.
        assert!(
            read_ape_tag_from_file(file).unwrap().is_none(),
            "--tags-only appended an APEv2 tag in the default split layout"
        );
    }
    assert_eq!(
        album_gains[0], album_gains[1],
        "album gain must be identical across the album"
    );
}

/// Issue #308: `-d` shifts the written value exactly. A sub-step value like
/// 0.4 dB rounds to zero steps in the apply path, but a tag holds a float, so
/// here it has to land verbatim.
#[test]
fn tags_only_d_modifier_shifts_written_value_exactly() {
    let album = TempAlbum::new(&["test_mono.mp3"]);
    let file = &album.files[0];

    let out = run(&["-r", "--tags-only", "-c", file.to_str().unwrap()]);
    assert!(out.status.success(), "baseline run failed: {:?}", out);
    let base = track_gain_db(file).expect("baseline gain");

    let out = run(&[
        "-r",
        "--tags-only",
        "-d",
        "0.4",
        "-c",
        file.to_str().unwrap(),
    ]);
    assert!(out.status.success(), "-d run failed: {:?}", out);
    let shifted = track_gain_db(file).expect("shifted gain");
    assert!(
        (shifted - (base + 0.4)).abs() < 0.001,
        "expected {} + 0.4, got {}",
        base,
        shifted
    );
}

/// Issue #308: `-k` caps the written value at the file's headroom so a player
/// applying the tag cannot push the signal past unity.
#[test]
fn tags_only_k_caps_written_gain_at_headroom() {
    let album = TempAlbum::new(&["test_mono.mp3"]);
    let file = &album.files[0];

    // +60 dB of pregain is past any real file's headroom, so the written value
    // would clip on playback whatever the fixture happens to measure.
    let uncapped = json_file_record(&["-r", "--tags-only", "-d", "60", "-c"], file);
    let peak = uncapped["peak"].as_f64().expect("peak");
    let wanted = uncapped["tag_gain_db"].as_f64().expect("tag_gain_db");
    assert!(peak > 0.0, "fixture should not be digital silence");
    assert!(
        peak * 10f64.powf(wanted / 20.0) > 1.0,
        "setup: the uncapped value should clip on playback"
    );

    let capped = json_file_record(&["-r", "--tags-only", "-d", "60", "-k"], file);
    let got = capped["tag_gain_db"].as_f64().expect("tag_gain_db");
    assert!(
        got < wanted,
        "-k left the written gain at {} instead of capping it",
        got
    );
    let played_peak = peak * 10f64.powf(got / 20.0);
    assert!(
        played_peak <= 1.0 + 1e-9,
        "capped tag gain still clips: peak {} * gain {} dB = {}",
        peak,
        got,
        played_peak
    );
    // The tag must actually carry the capped value, not just report it.
    let written = track_gain_db(file).expect("REPLAYGAIN_TRACK_GAIN");
    assert!((written - got).abs() < 0.01, "written {}", written);
}

/// Reported on the Hydrogenaudio forum: TSV rows printed the bare filename,
/// which collides as soon as more than one album is scanned in a single run.
#[test]
fn tsv_rows_carry_the_path_as_given() {
    let album = TempAlbum::new(&["test_mono.mp3"]);
    let file = &album.files[0];
    let path = file.to_str().unwrap();

    let text = stdout_of(&run(&["-o", "tsv", path]));
    let row = text.lines().nth(1).expect("TSV data row");
    assert_eq!(row.split('\t').next(), Some(path), "row: {}", row);
}

/// Reported on the Hydrogenaudio forum (issue #323): `-o tsv` printed the
/// peak on mp3gain's 16-bit sample scale while `-o json` printed the
/// ReplayGain float. RG1 keeps the mp3gain scale for compatibility; the
/// BS.1770 modes report the same float the tags and JSON carry.
#[test]
fn tsv_peak_matches_json_in_rg2_mode_and_mp3gain_scale_in_rg1() {
    let album = TempAlbum::new(&["test_mono.mp3"]);
    let path = album.files[0].to_str().unwrap();

    let tsv_peak = |mode: &[&str]| -> f64 {
        let mut argv = mode.to_vec();
        argv.extend_from_slice(&["-r", "-n", "-o", "tsv", path]);
        let text = stdout_of(&run(&argv));
        let row = text.lines().nth(1).expect("TSV data row");
        row.split('\t').nth(3).unwrap().parse().unwrap()
    };
    let json_peak = |mode: &[&str]| -> f64 {
        let mut argv = mode.to_vec();
        argv.extend_from_slice(&["-r", "-n", "-o", "json", path]);
        json_of(&run(&argv))["files"][0]["peak"]
            .as_f64()
            .expect("json peak")
    };

    let rg1_json = json_peak(&[]);
    assert!((tsv_peak(&[]) - rg1_json * 32768.0).abs() < 0.01);

    let rg2_json = json_peak(&["--rg2"]);
    assert!((tsv_peak(&["--rg2"]) - rg2_json).abs() < 1e-6);
    assert!(rg2_json < 2.0, "float peak, not a sample value: {rg2_json}");
}

/// Requested on the Hydrogenaudio forum (issue #324): `-a --per-directory`
/// computes one album gain per folder, so a library can be tagged in one
/// run. Each directory must get the same album gain it would get alone.
#[test]
fn per_directory_album_gain_matches_each_directory_run_alone() {
    let root = TempAlbum::new(&[]);
    let mut dirs = Vec::new();
    for (name, fixtures) in [
        ("A", vec!["test_mono.mp3", "test_vbr.mp3"]),
        ("B", vec!["test_stereo.mp3", "test_joint_stereo.mp3"]),
    ] {
        let dir = root.dir.join(name);
        fs::create_dir(&dir).unwrap();
        for f in fixtures {
            fs::copy(Path::new("tests/fixtures").join(f), dir.join(f)).unwrap();
        }
        dirs.push(dir);
    }
    let root_arg = root.dir.to_str().unwrap();

    let out = run(&["-a", "--per-directory", "-n", "-R", "-o", "json", root_arg]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json = json_of(&out);
    let albums = json["albums"].as_array().expect("albums array");
    assert_eq!(albums.len(), 2);
    assert_eq!(json["files"].as_array().unwrap().len(), 4);
    assert_eq!(json["summary"]["total_files"], 4);

    for (dir, album) in dirs.iter().zip(albums) {
        assert_eq!(album["directory"], dir.to_str().unwrap());
        assert_eq!(album["files"], 2);
        let alone = json_of(&run(&[
            "-a",
            "-n",
            "-R",
            "-o",
            "json",
            dir.to_str().unwrap(),
        ]));
        assert_eq!(
            album["gain_db"],
            alone["album"]["gain_db"],
            "{}",
            dir.display()
        );
    }
    assert_ne!(
        albums[0]["gain_db"], albums[1]["gain_db"],
        "the two directories should not share one album gain"
    );

    // Without the flag everything is still one album.
    let pooled = json_of(&run(&["-a", "-n", "-R", "-o", "json", root_arg]));
    assert!(pooled["albums"].is_null());
    assert!(pooled["album"]["gain_db"].is_number());
}

/// Albums run concurrently under `--per-directory` (issue #332), so the
/// output can no longer rely on one album finishing before the next starts.
/// Every observable part of the run — group order, gain values, text layout —
/// must still match what the serial `-j 1` path produces.
#[test]
fn per_directory_output_is_identical_whatever_the_thread_count() {
    let root = TempAlbum::new(&[]);
    for (name, fixtures) in [
        ("A", vec!["test_mono.mp3", "test_vbr.mp3"]),
        ("B", vec!["test_stereo.mp3", "test_joint_stereo.mp3"]),
        ("C", vec!["test_vbr.mp3", "test_aac.m4a"]),
        ("D", vec!["test_mono.mp3"]),
    ] {
        let dir = root.dir.join(name);
        fs::create_dir(&dir).unwrap();
        for f in fixtures {
            fs::copy(Path::new("tests/fixtures").join(f), dir.join(f)).unwrap();
        }
    }
    let root_arg = root.dir.to_str().unwrap();

    for format in ["json", "text", "tsv"] {
        let args = |jobs: &'static str| {
            vec![
                "-a",
                "--per-directory",
                "-n",
                "-R",
                "-o",
                format,
                "-j",
                jobs,
                root_arg,
            ]
        };
        let serial = run(&args("1"));
        let parallel = run(&args("4"));
        assert!(serial.status.success() && parallel.status.success());
        assert_eq!(
            stdout_of(&serial),
            stdout_of(&parallel),
            "-o {format} differs between -j 1 and -j 4"
        );
    }
}

/// Write a two-minute MP3 built by repeating a one-second fixture's frames.
///
/// MP3 frames concatenate, so this needs no encoder. The ID3 tag and the
/// leading Xing/Info frame are dropped because they declare a one-second frame
/// count, and a track that claims to be one second long is never divided.
fn write_long_mp3(dst: &Path) {
    let fixture = fs::read("tests/fixtures/test_stereo.mp3").expect("fixture");
    let size = &fixture[6..10];
    let id3_len = 10
        + (((size[0] & 0x7f) as usize) << 21
            | ((size[1] & 0x7f) as usize) << 14
            | ((size[2] & 0x7f) as usize) << 7
            | (size[3] & 0x7f) as usize);
    // 320 kbps at 44.1 kHz is a 1044-byte frame, plus one padding byte.
    let body = &fixture[id3_len + 1045..];
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    fs::write(dst, body.repeat(120)).expect("write long mp3");
}

/// A long track is divided across workers when the pool has room for it
/// (issue #337). Each piece starts its filters from zero and runs them through
/// a warm-up region it discards, so the result is not guaranteed bit-identical
/// the way the pipeline in #341 is: the filter state at a piece boundary can
/// differ from the whole-file state in its last bit, which carries through to
/// the reported loudness.
///
/// The bound asserted here is 1e-9 dB against a measured worst case of 7e-15
/// on MP3 and AAC corpora, so it is six orders of margin and would still catch
/// a real error such as a misaligned piece or a dropped block.
#[test]
fn chunked_analysis_matches_the_whole_file_pass() {
    let album = TempAlbum::new(&[]);
    let long = album.dir.join("long.mp3");
    write_long_mp3(&long);
    let path = long.to_str().unwrap();

    for mode in [
        vec!["--rg2", "--true-peak"],
        vec!["--r128", "--true-peak"],
        vec!["--rg2"],
    ] {
        let measure = |jobs: &'static str| -> (f64, f64) {
            let mut args = vec!["-r"];
            args.extend_from_slice(&mode);
            args.extend_from_slice(&["-n", "-o", "json", "-j", jobs, path]);
            let json = json_of(&run(&args));
            let file = &json["files"][0];
            (
                file["loudness_db"].as_f64().expect("loudness"),
                file["peak"].as_f64().expect("peak"),
            )
        };
        let (whole_loudness, whole_peak) = measure("1");
        let (chunked_loudness, chunked_peak) = measure("8");
        assert!(
            (whole_loudness - chunked_loudness).abs() < 1e-9,
            "{mode:?}: {whole_loudness} vs {chunked_loudness}"
        );
        // The true-peak history is exact at a piece boundary, since the
        // warm-up feeds the meter the real preceding samples, so the peak has
        // no tolerance to spend.
        assert_eq!(whole_peak, chunked_peak, "{mode:?}");
    }
}

/// RG1 is never divided: its equal-loudness filter settles far more slowly
/// than the two biquads of K-weighting, and it is the path whose values have
/// to match mp3gain bit for bit.
#[test]
fn rg1_is_never_chunked() {
    let album = TempAlbum::new(&[]);
    let long = album.dir.join("long.mp3");
    write_long_mp3(&long);
    let path = long.to_str().unwrap();

    let measure =
        |jobs: &'static str| stdout_of(&run(&["-r", "-n", "-o", "json", "-j", jobs, path]));
    assert_eq!(measure("1"), measure("8"), "RG1 must be byte-identical");
}

/// The decode and the analysis run on separate threads once `-j` allows it
/// (issue #337). The analyzer still sees every frame once and in order, so
/// this has to be byte-identical to the single-threaded path, not merely
/// close. `--true-peak` is the case that actually exercises it, being over
/// half the analysis cost.
#[test]
fn pipelined_analysis_matches_the_single_threaded_path() {
    let album = TempAlbum::new(&[
        "test_mono.mp3",
        "test_stereo.mp3",
        "test_vbr.mp3",
        "test_joint_stereo.mp3",
        "test_aac.m4a",
    ]);
    for mode in [
        vec!["--rg2", "--true-peak"],
        vec!["--r128", "--true-peak"],
        vec!["--rg2"],
    ] {
        for command in [vec!["-r"], vec!["-a"]] {
            let render = |jobs: &'static str| {
                let mut args = command.clone();
                args.extend_from_slice(&mode);
                args.extend_from_slice(&["-n", "-o", "json", "-j", jobs]);
                args.extend(album.args());
                stdout_of(&run(&args))
            };
            assert_eq!(
                render("1"),
                render("4"),
                "{command:?} {mode:?} differs between -j 1 and -j 4"
            );
        }
    }
}

#[test]
fn per_directory_requires_album_mode() {
    for flag in ["--per-directory", "--album-by=tag"] {
        let out = run(&[flag, "-r", "tests/fixtures/test_mono.mp3"]);
        assert!(!out.status.success(), "{flag}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("requires -a"),
            "{flag}"
        );
    }
    let out = run(&[
        "-a",
        "--album-by=bogus",
        "-n",
        "tests/fixtures/test_mono.mp3",
    ]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown --album-by mode"));
}

/// Write `fixture` into `dst` behind a minimal ID3v2.3 tag. Tagging through an
/// external tool would make these tests depend on ffmpeg being installed on
/// every CI runner, and the frames needed here are four text frames.
fn write_tagged_mp3(fixture: &str, dst: &Path, frames: &[(&str, &str)]) {
    let audio = fs::read(Path::new("tests/fixtures").join(fixture)).expect("fixture");
    let mut body = Vec::new();
    for (id, text) in frames {
        // Text frame payload: encoding byte, then the text. TXXX carries
        // "description\0value" in that same payload.
        let mut payload = vec![0u8]; // ISO-8859-1
        payload.extend_from_slice(text.as_bytes());
        body.extend_from_slice(id.as_bytes());
        body.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&payload);
    }
    let size = body.len() as u32;
    let mut out = Vec::from(*b"ID3\x03\x00\x00");
    // The header length is synchsafe: 7 bits per byte.
    out.extend_from_slice(&[
        ((size >> 21) & 0x7f) as u8,
        ((size >> 14) & 0x7f) as u8,
        ((size >> 7) & 0x7f) as u8,
        (size & 0x7f) as u8,
    ]);
    out.extend_from_slice(&body);
    out.extend_from_slice(&audio);
    fs::create_dir_all(dst.parent().unwrap()).unwrap();
    fs::write(dst, out).expect("write tagged mp3");
}

fn albums_of(out: &Output) -> Vec<serde_json::Value> {
    json_of(out)["albums"].as_array().expect("albums").clone()
}

/// The bug in #331: a release whose discs live in subdirectories gets one
/// album gain per disc under `dir` grouping, which is wrong for both
/// compatibility targets. `tag` grouping is the fix, and the value it produces
/// has to be the one the whole release would get pooled into a single album.
#[test]
fn album_by_tag_keeps_a_multi_disc_release_as_one_album() {
    let root = TempAlbum::new(&[]);
    let common = [("TALB", "The Wall"), ("TPE2", "Pink Floyd")];
    for (disc, track, fixture) in [
        (1, 1, "test_mono.mp3"),
        (1, 2, "test_vbr.mp3"),
        (2, 1, "test_stereo.mp3"),
        (2, 2, "test_joint_stereo.mp3"),
    ] {
        let mut frames = common.to_vec();
        let (disc_s, track_s) = (disc.to_string(), track.to_string());
        frames.push(("TPOS", &disc_s));
        frames.push(("TRCK", &track_s));
        let dst = root.dir.join(format!("disc{disc}")).join(fixture);
        write_tagged_mp3(fixture, &dst, &frames);
    }
    let root_arg = root.dir.to_str().unwrap();

    let by_tag = albums_of(&run(&[
        "-a",
        "--album-by=tag",
        "-n",
        "-R",
        "-o",
        "json",
        root_arg,
    ]));
    assert_eq!(by_tag.len(), 1, "both discs are one release");
    assert_eq!(by_tag[0]["album_title"], "The Wall");
    assert_eq!(by_tag[0]["album_artist"], "Pink Floyd");
    assert_eq!(by_tag[0]["files"], 4);
    assert!(
        by_tag[0]["directory"].is_null(),
        "tag groups are not directories"
    );

    // The value must match pooling the same files with plain -a.
    let pooled = json_of(&run(&["-a", "-n", "-R", "-o", "json", root_arg]));
    assert_eq!(by_tag[0]["gain_db"], pooled["album"]["gain_db"]);

    // dir grouping still splits them, which is the behavior #331 reported.
    let by_dir = albums_of(&run(&[
        "-a",
        "--album-by=dir",
        "-n",
        "-R",
        "-o",
        "json",
        root_arg,
    ]));
    assert_eq!(by_dir.len(), 2);
    assert_ne!(by_dir[0]["gain_db"], by_dir[1]["gain_db"]);
}

/// gcocatre on #333: several releases of one album share the artist and album
/// string, and the ways users tell them apart are too varied to enumerate. So
/// nothing is guessed. A repeated (disc, track) inside one group is structural
/// proof that more than one release is in it, and that is what gets reported.
#[test]
fn album_by_tag_reports_two_releases_sharing_one_album_string() {
    let root = TempAlbum::new(&[]);
    for (edition, fixture) in [("1973", "test_mono.mp3"), ("2011", "test_stereo.mp3")] {
        write_tagged_mp3(
            fixture,
            &root.dir.join(edition).join("01.mp3"),
            &[
                ("TALB", "The Dark Side of the Moon"),
                ("TPE2", "Pink Floyd"),
                ("TRCK", "1"),
            ],
        );
    }
    let out = run(&[
        "-a",
        "--album-by=tag",
        "-n",
        "-R",
        "-o",
        "json",
        root.dir.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(albums_of(&out).len(), 1, "they do group together");
    assert!(
        stderr.contains("track 1 appears 2 times"),
        "the collision has to be reported: {stderr}"
    );
    assert!(
        stderr.contains("1973") && stderr.contains("2011"),
        "{stderr}"
    );
    assert!(stderr.contains("--album-by=dir"), "{stderr}");
}

/// The same two releases, tagged the way Picard tags them, must come apart
/// without any warning: MUSICBRAINZ_ALBUMID is the one field that separates
/// them without guessing.
#[test]
fn album_by_tag_separates_releases_by_musicbrainz_album_id() {
    let root = TempAlbum::new(&[]);
    for (edition, fixture) in [("1973", "test_mono.mp3"), ("2011", "test_stereo.mp3")] {
        write_tagged_mp3(
            fixture,
            &root.dir.join(edition).join("01.mp3"),
            &[
                ("TALB", "The Dark Side of the Moon"),
                ("TPE2", "Pink Floyd"),
                ("TRCK", "1"),
                (
                    "TXXX",
                    &format!("MusicBrainz Album Id\u{0}release-{edition}"),
                ),
            ],
        );
    }
    let out = run(&[
        "-a",
        "--album-by=tag",
        "-n",
        "-R",
        "-o",
        "json",
        root.dir.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(albums_of(&out).len(), 2, "two releases, two album gains");
    assert!(
        !stderr.contains("appears"),
        "no collision to report: {stderr}"
    );
}

/// Pooling every untagged file in a library into one album is the one outcome
/// that must not happen silently.
#[test]
fn album_by_tag_falls_back_to_directory_without_an_album_tag() {
    let root = TempAlbum::new(&[]);
    for (dir, fixture) in [("a", "test_mono.mp3"), ("b", "test_stereo.mp3")] {
        let dst = root.dir.join(dir).join(fixture);
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        fs::copy(Path::new("tests/fixtures").join(fixture), &dst).unwrap();
    }
    let out = run(&[
        "-a",
        "--album-by=tag",
        "-n",
        "-R",
        "-o",
        "json",
        root.dir.to_str().unwrap(),
    ]);
    let albums = albums_of(&out);
    assert_eq!(albums.len(), 2, "not pooled into one album");
    for album in &albums {
        assert!(
            album["directory"].is_string(),
            "reported as a directory group"
        );
        assert!(album["album_title"].is_null());
    }
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("no ALBUM tag"),
        "the fallback has to be reported"
    );
}

/// Build `<root>/Pink Floyd/The Wall/disc{1,2}` plus a single-disc Metallica
/// album, so one tree exercises every depth the grouping can be asked for.
fn write_depth_corpus(root: &Path) {
    for (dir, fixture, disc, track) in [
        ("Pink Floyd/The Wall/disc1", "test_mono.mp3", "1", "1"),
        ("Pink Floyd/The Wall/disc1", "test_vbr.mp3", "1", "2"),
        ("Pink Floyd/The Wall/disc2", "test_stereo.mp3", "2", "1"),
        (
            "Pink Floyd/The Wall/disc2",
            "test_joint_stereo.mp3",
            "2",
            "2",
        ),
    ] {
        write_tagged_mp3(
            fixture,
            &root.join(dir).join(fixture),
            &[
                ("TALB", "The Wall"),
                ("TPE2", "Pink Floyd"),
                ("TPOS", disc),
                ("TRCK", track),
            ],
        );
    }
    for (fixture, track) in [("test_mono.mp3", "1"), ("test_stereo.mp3", "2")] {
        write_tagged_mp3(
            fixture,
            &root.join("Metallica/Ride the Lightning").join(fixture),
            &[
                ("TALB", "Ride the Lightning"),
                ("TPE2", "Metallica"),
                ("TPOS", "1"),
                ("TRCK", track),
            ],
        );
    }
}

/// `--album-depth N` is the grouping unit for a library that is not tagged
/// (issue #331): N levels below each root argument, with no glob and no
/// command-line length ceiling, so it works the same on Windows.
#[test]
fn album_depth_groups_at_the_requested_level() {
    let root = TempAlbum::new(&[]);
    write_depth_corpus(&root.dir);
    let root_arg = root.dir.to_str().unwrap();

    let directories = |depth: &str| -> Vec<String> {
        let out = run(&[
            "-a",
            "--album-depth",
            depth,
            "-n",
            "-R",
            "-o",
            "json",
            root_arg,
        ]);
        albums_of(&out)
            .iter()
            .map(|a| {
                // Relative to the temp root, with the separator normalized:
                // Windows reports the same groups as `\\Pink Floyd`.
                a["directory"]
                    .as_str()
                    .expect("depth groups are directories")
                    .trim_start_matches(root_arg)
                    .trim_start_matches(['/', '\\'])
                    .replace('\\', "/")
            })
            .collect()
    };

    assert_eq!(directories("0"), vec![""], "the root itself is one album");
    assert_eq!(directories("1"), vec!["Metallica", "Pink Floyd"]);
    assert_eq!(
        directories("2"),
        vec!["Metallica/Ride the Lightning", "Pink Floyd/The Wall"]
    );
    // Deeper than the tree: every directory that exists becomes its own album.
    assert_eq!(
        directories("3"),
        vec![
            "Metallica/Ride the Lightning",
            "Pink Floyd/The Wall/disc1",
            "Pink Floyd/The Wall/disc2",
        ]
    );
}

/// `--album-depth 0` is what `--album-by=arg` would have been: one album per
/// directory argument. That is why `arg` was dropped rather than implemented.
#[test]
fn album_depth_zero_is_one_album_per_root_argument() {
    let root = TempAlbum::new(&[]);
    write_depth_corpus(&root.dir);
    let floyd = root.dir.join("Pink Floyd");
    let metallica = root.dir.join("Metallica");

    let out = run(&[
        "-a",
        "--album-depth",
        "0",
        "-n",
        "-R",
        "-o",
        "json",
        floyd.to_str().unwrap(),
        metallica.to_str().unwrap(),
    ]);
    let albums = albums_of(&out);
    assert_eq!(albums.len(), 2, "one album per argument");
    assert_eq!(albums[0]["files"], 2, "Metallica sorts first");
    assert_eq!(albums[1]["files"], 4, "both Floyd discs in one album");
}

/// The multi-disc case from #331, solved without tags: grouping two levels
/// down puts both disc folders in the same album, and the value has to match
/// pooling the same files with plain `-a`.
#[test]
fn album_depth_keeps_a_multi_disc_release_together() {
    let root = TempAlbum::new(&[]);
    write_depth_corpus(&root.dir);
    let floyd = root.dir.join("Pink Floyd");
    let floyd_arg = floyd.to_str().unwrap();

    let by_depth = albums_of(&run(&[
        "-a",
        "--album-depth",
        "1",
        "-n",
        "-R",
        "-o",
        "json",
        floyd_arg,
    ]));
    assert_eq!(by_depth.len(), 1);
    assert_eq!(by_depth[0]["files"], 4);

    let pooled = json_of(&run(&["-a", "-n", "-R", "-o", "json", floyd_arg]));
    assert_eq!(by_depth[0]["gain_db"], pooled["album"]["gain_db"]);
}

/// Directory grouping cannot see that two sibling folders are one release, so
/// it has to say so instead of writing a per-disc album gain in silence.
#[test]
fn dir_grouping_warns_when_a_release_is_split_across_sibling_folders() {
    let root = TempAlbum::new(&[]);
    write_depth_corpus(&root.dir);
    let out = run(&[
        "-a",
        "--album-by=dir",
        "-n",
        "-R",
        "-o",
        "json",
        root.dir.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(albums_of(&out).len(), 3, "dir grouping still splits them");
    assert!(
        stderr.contains(r#"ALBUM "The Wall" is split across 2 directories"#),
        "{stderr}"
    );
    assert!(stderr.contains("--album-by=tag"), "{stderr}");
    // The single-disc album next to it is not a split release.
    assert!(!stderr.contains("Ride the Lightning"), "{stderr}");
}

/// The mirror case, raised by gcocatre on #333: two editions of one album in
/// sibling folders are *already* grouped correctly by directory, so pointing
/// the user at `--album-by=tag` would merge them wrongly. Same (disc, track)
/// in both folders is what separates this from a multi-disc release.
#[test]
fn dir_grouping_stays_quiet_for_two_editions_of_one_album() {
    let root = TempAlbum::new(&[]);
    for edition in ["1973", "2011"] {
        for (fixture, track) in [("test_mono.mp3", "1"), ("test_stereo.mp3", "2")] {
            write_tagged_mp3(
                fixture,
                &root.dir.join(edition).join(fixture),
                &[
                    ("TALB", "The Dark Side of the Moon"),
                    ("TPE2", "Pink Floyd"),
                    ("TPOS", "1"),
                    ("TRCK", track),
                ],
            );
        }
    }
    let out = run(&[
        "-a",
        "--album-by=dir",
        "-n",
        "-R",
        "-o",
        "json",
        root.dir.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(albums_of(&out).len(), 2);
    assert!(
        !stderr.contains("split across"),
        "no advice to give: {stderr}"
    );
}

#[test]
fn album_depth_rejects_contradictory_invocations() {
    let dir = TempAlbum::new(&["test_mono.mp3"]);
    let arg = dir.dir.to_str().unwrap();
    for (args, expected) in [
        (vec!["-a", "--album-depth", "2", "-n", arg], "requires -R"),
        (
            vec![
                "-a",
                "--album-depth",
                "2",
                "--album-by=tag",
                "-n",
                "-R",
                arg,
            ],
            "mutually exclusive",
        ),
        (
            vec!["-a", "--album-depth", "x", "-n", "-R", arg],
            "needs a level count",
        ),
        (
            vec!["--album-depth", "2", "-r", "-n", "-R", arg],
            "requires -a",
        ),
    ] {
        let out = run(&args);
        assert!(!out.status.success(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(expected),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

/// `--per-directory` shipped in 3.6.1 and keeps working unchanged.
#[test]
fn per_directory_is_an_alias_for_album_by_dir() {
    let root = TempAlbum::new(&[]);
    for (dir, fixture) in [("a", "test_mono.mp3"), ("b", "test_stereo.mp3")] {
        let dst = root.dir.join(dir).join(fixture);
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        fs::copy(Path::new("tests/fixtures").join(fixture), &dst).unwrap();
    }
    let root_arg = root.dir.to_str().unwrap();
    for format in ["json", "text", "tsv"] {
        let alias = run(&["-a", "--per-directory", "-n", "-R", "-o", format, root_arg]);
        let explicit = run(&["-a", "--album-by=dir", "-n", "-R", "-o", format, root_arg]);
        assert_eq!(stdout_of(&alias), stdout_of(&explicit), "-o {format}");
    }
}

/// Reported on the Hydrogenaudio forum: `-o tsv` only produced rows for the
/// bare analysis command. Combined with `-r` or `-a` it printed nothing at all.
#[test]
fn tsv_rows_are_emitted_by_the_gain_applying_commands() {
    for args in [
        vec!["-r", "-n"],
        vec!["-a", "-n"],
        vec!["-e", "-n"],
        vec!["-g", "1", "-n"],
    ] {
        let album = TempAlbum::new(&["test_mono.mp3"]);
        let path = album.files[0].to_str().unwrap();

        let mut argv = args.clone();
        argv.extend_from_slice(&["-o", "tsv", path]);
        let text = stdout_of(&run(&argv));

        assert!(
            text.starts_with("File\tMP3 gain\t"),
            "{:?} lost the TSV header: {:?}",
            args,
            text
        );
        let row = text
            .lines()
            .nth(1)
            .unwrap_or_else(|| panic!("{:?} emitted no TSV row", args));
        assert_eq!(row.split('\t').next(), Some(path), "row: {}", row);
    }
}

/// `-a -o tsv` reports the same recommended gain as `-o tsv` alone, including
/// the trailing `"Album"` summary row.
#[test]
fn tsv_album_mode_matches_the_analysis_only_rows() {
    let album = TempAlbum::new(&["test_mono.mp3", "test_stereo.mp3"]);
    let mut analysis = vec!["-o", "tsv"];
    analysis.extend(album.args());
    let expected = stdout_of(&run(&analysis));

    let mut applied = vec!["-o", "tsv", "-a", "-n"];
    applied.extend(album.args());
    assert_eq!(stdout_of(&run(&applied)), expected);
    assert!(expected.contains("\"Album\"\t"), "{}", expected);
}

/// Issue #228 gave the writing commands a non-zero exit on failure but left
/// the read-only ones (info, `-s c`, `-x`) reporting success for a file they
/// could not even open, in every output format.
#[test]
fn read_only_commands_exit_non_zero_on_an_unreadable_file() {
    let album = TempAlbum::new(&["test_mono.mp3"]);
    let good = album.files[0].to_str().unwrap().to_string();
    let missing = album
        .dir
        .join("no_such_file.mp3")
        .to_str()
        .unwrap()
        .to_string();

    for command in [vec![], vec!["-s", "c"], vec!["-x"]] {
        for format in [vec![], vec!["-o", "tsv"], vec!["-o", "json"]] {
            let base: Vec<&str> = command.iter().chain(format.iter()).copied().collect();

            let mut ok = base.clone();
            ok.push(&good);
            assert!(
                run(&ok).status.success(),
                "{:?} failed on a readable file",
                ok
            );

            for files in [vec![&missing], vec![&good, &missing]] {
                let mut argv = base.clone();
                argv.extend(files.iter().map(|f| f.as_str()));
                assert!(
                    !run(&argv).status.success(),
                    "{:?} exited 0 despite an unreadable file",
                    argv
                );
            }
        }
    }
}

/// The value printed after `label`, e.g. `Max global_gain:` in the `-x` report
/// or `Max mp3 global gain field:` in the default info output.
fn labeled_value(text: &str, label: &str) -> String {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(label))
        .unwrap_or_else(|| panic!("{:?} not found in:\n{}", label, text))
        .trim()
        .to_string()
}

/// The `Max global_gain` / `Min global_gain` columns of the first TSV data row.
fn tsv_gain_columns(text: &str) -> (String, String) {
    let row = text
        .lines()
        .nth(1)
        .unwrap_or_else(|| panic!("TSV output should carry a data row:\n{}", text));
    let columns: Vec<&str> = row.split('\t').collect();
    assert_eq!(columns.len(), 6, "unexpected TSV row: {:?}", row);
    (columns[4].to_string(), columns[5].to_string())
}

/// Issue #329: the info row scanned the global_gain range with the MP3-only
/// analyzer, which fails on AAC, so every AAC file fell back to the (255, 0)
/// accumulator seed and printed it as if it were measured. The info paths must
/// agree with `-x`, which has always dispatched on the container.
#[test]
fn aac_info_row_reports_the_same_global_gain_range_as_max_amplitude() {
    let album = TempAlbum::new(&["test_aac.m4a"]);
    let file = album.files[0].to_str().unwrap();

    let x_report = stdout_of(&run(&["-x", file]));
    let max = labeled_value(&x_report, "Max global_gain:");
    let min = labeled_value(&x_report, "Min global_gain:");
    assert_ne!(
        (max.as_str(), min.as_str()),
        ("255", "0"),
        "the fixture should have a real gain range to compare against"
    );

    assert_eq!(
        tsv_gain_columns(&stdout_of(&run(&["-o", "tsv", file]))),
        (max.clone(), min.clone()),
        "-o tsv disagrees with -x"
    );

    let info = stdout_of(&run(&[file]));
    assert_eq!(labeled_value(&info, "Max mp3 global gain field:"), max);
    assert_eq!(labeled_value(&info, "Min mp3 global gain field:"), min);
}

/// Issue #330: a raw ADTS `.aac` stream is a first-class input. Every gain
/// path used to fail it with "No valid MP3 frames found" (the MP3 scanner
/// running on an AAC bitstream), which also poisoned the exit code of a
/// library scan that was otherwise fine. It now scans, reports and adjusts
/// like AAC in an MP4.
#[test]
fn raw_adts_reports_the_same_global_gain_range_as_max_amplitude() {
    let album = TempAlbum::new(&["test_adts.aac"]);
    let file = album.files[0].to_str().unwrap();

    let x_report = stdout_of(&run(&["-x", file]));
    let max = labeled_value(&x_report, "Max global_gain:");
    let min = labeled_value(&x_report, "Min global_gain:");
    assert_ne!(
        (max.as_str(), min.as_str()),
        ("255", "0"),
        "ADTS should report a measured range, not the accumulator seed"
    );

    let tsv = stdout_of(&run(&["-o", "tsv", file]));
    assert_eq!(
        tsv_gain_columns(&tsv),
        (max.clone(), min.clone()),
        "-o tsv disagrees with -x"
    );
    assert!(
        tsv.lines().any(|line| line.starts_with("\"Album\"")),
        "the album summary row should still be emitted:\n{}",
        tsv
    );

    let info = stdout_of(&run(&[file]));
    assert_eq!(labeled_value(&info, "Max mp3 global gain field:"), max);
    assert_eq!(labeled_value(&info, "Min mp3 global gain field:"), min);
}

/// The apply/undo round trip on a raw ADTS stream: the gain lands in the
/// bitstream, the undo info lands in ID3v2 (there is no `moov` to hold the
/// freeform atoms the M4A path uses), and `-u` restores the audio bytes
/// exactly (issue #330).
#[test]
fn raw_adts_gain_is_applied_and_undone_losslessly() {
    let album = TempAlbum::new(&["test_adts.aac"]);
    let file = album.files[0].to_str().unwrap();
    let original = fs::read(&album.files[0]).expect("read fixture");

    let out = run(&["-g", "3", file]);
    assert!(out.status.success(), "ADTS apply failed: {:?}", out);
    let applied = fs::read(&album.files[0]).expect("read applied");
    assert_ne!(
        adts_audio(&applied),
        adts_audio(&original),
        "gain not applied"
    );

    // The undo value goes to ID3v2 for ADTS whatever the tag layout, since a
    // raw stream has nowhere else to put it.
    assert!(
        mp3rgain::read_id3v2_replaygain(&album.files[0])
            .expect("reading the ID3v2 tag should not fail")
            .undo
            .is_some(),
        "ADTS apply wrote no MP3GAIN_UNDO"
    );

    let out = run(&["-u", file]);
    assert!(out.status.success(), "ADTS undo failed: {:?}", out);
    assert_eq!(
        adts_audio(&fs::read(&album.files[0]).expect("read undone")),
        adts_audio(&original),
        "undo did not restore the ADTS audio byte-for-byte"
    );
}

/// A raw ADTS file in a library no longer fails the whole run. This is the
/// exact reproducer from issue #330: `-R -r` over a directory holding one
/// `.aac` and one `.mp3` exited 1 even though both files were fine.
#[test]
fn a_library_holding_a_raw_adts_file_exits_zero() {
    let album = TempAlbum::new(&["test_adts.aac", "test_mono.mp3"]);
    let out = run(&["-R", "-r", "-c", album.dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "a library with a raw ADTS file should not exit non-zero: {:?}",
        out
    );
}

/// Issue #330: a file mp3rgain cannot adjust because of its *format* is
/// reported as a skip, not a failure, so one ALAC track in a library does not
/// make the whole run exit non-zero. ALAC used to surface symphonia's
/// "unsupported audio codec", which reads as a genuine error.
#[test]
fn an_unsupported_format_is_skipped_rather_than_failed() {
    let album = TempAlbum::new(&["test_alac.m4a", "test_mono.mp3"]);
    let out = run(&["-R", "-r", "-c", album.dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "an ALAC file in a library should not set the exit code: {:?}",
        out
    );

    let alac = album.files[0].to_str().unwrap();
    let out = run(&["-o", "json", "-r", alac]);
    assert!(
        out.status.success(),
        "ALAC alone should exit zero: {:?}",
        out
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout_of(&out)).expect("JSON output expected");
    assert_eq!(json["files"][0]["status"], "skipped");
    assert_eq!(json["summary"]["failed"], 0);
}

/// Album mode agrees with the track path: an unsupported member is dropped
/// from the set without failing the run, and an album with nothing adjustable
/// in it at all is a set of skips rather than "all files failed" (issue #330).
#[test]
fn album_mode_skips_unsupported_members_without_failing() {
    let mixed = TempAlbum::new(&["test_alac.m4a", "test_mono.mp3"]);
    let out = run(&["-R", "-a", "-c", mixed.dir.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "an ALAC member should not fail the album: {:?}",
        out
    );

    let alac_only = TempAlbum::new(&["test_alac.m4a"]);
    let out = run(&["-o", "json", "-a", alac_only.files[0].to_str().unwrap()]);
    assert!(
        out.status.success(),
        "an album with nothing adjustable should not fail: {:?}",
        out
    );
    let json: serde_json::Value =
        serde_json::from_str(&stdout_of(&out)).expect("JSON output expected");
    assert_eq!(json["files"][0]["status"], "skipped");
    assert_eq!(json["summary"]["failed"], 0);
}

/// The counterpart to the skips above: a genuinely broken file must still
/// fail, or the exit code stops meaning anything (issue #330).
#[test]
fn a_genuinely_unreadable_file_still_fails() {
    let album = TempAlbum::new(&["test_mono.mp3"]);
    let broken = album.dir.join("broken.mp3");
    fs::write(&broken, b"this is not an MP3 at all").expect("write broken file");

    for args in [
        vec!["-R", "-r", "-c"],
        vec!["-R", "-a", "-c", "--skip-errors"],
    ] {
        let mut args = args;
        args.push(album.dir.to_str().unwrap());
        let out = run(&args);
        assert!(
            !out.status.success(),
            "a corrupt file must still set the exit code ({:?}): {:?}",
            args,
            out
        );
    }
}

/// The ADTS audio, with any ID3v2 tag mp3rgain wrote skipped. Undo restores
/// the frames byte-for-byte; the empty tag container the `id3` crate leaves
/// behind is not part of that guarantee (MP3 undo behaves the same way).
fn adts_audio(data: &[u8]) -> &[u8] {
    if data.len() < 10 || &data[..3] != b"ID3" {
        return data;
    }
    let size = ((data[6] as usize & 0x7F) << 21)
        | ((data[7] as usize & 0x7F) << 14)
        | ((data[8] as usize & 0x7F) << 7)
        | (data[9] as usize & 0x7F);
    &data[10 + size..]
}
