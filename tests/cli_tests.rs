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

#[test]
fn per_directory_requires_album_mode() {
    let out = run(&["--per-directory", "-r", "tests/fixtures/test_mono.mp3"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--per-directory requires -a"));
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
