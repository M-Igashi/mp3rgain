use anyhow::Result;
use colored::*;
use mp3rgain::replaygain::{
    self, AlbumAnalysisReport, AlbumGainResult, AudioFileType, ReplayGainResult,
    REPLAYGAIN_REFERENCE_DB,
};
use mp3rgain::AacAlbumInfo;
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::cli::options::{Options, OutputFormat, StoredTagMode};
use crate::commands::threading::effective_threads;
use crate::commands::utils::{
    create_json_summary, exit_if_failed, finish_with_album_summary, finish_with_summary,
    for_each_file_with_analysis_bar, print_dry_run_notice, run_album_analysis, update_counters,
    TSV_HEADER,
};
use crate::json_output::{
    FileStatus, JsonAlbumResult, JsonDirectoryAlbum, JsonFileResult, JsonOutput,
};
use crate::processors::info::{scan_gain_range_for_row, tsv_rg_row};
use crate::processors::replaygain::{
    apply_is_noop, capped_tag_gain, process_apply_replaygain_with_album, process_track_gain,
};
use crate::progress::{create_progress_bar, progress_finish, progress_inc, progress_set_message};
use crate::util::get_filename;

fn print_target_with_modifier(opts: &Options) {
    let mode = opts.analysis_mode;
    let target = mode.target_lufs().unwrap_or(REPLAYGAIN_REFERENCE_DB);
    // `-d`/`-m` land on whole gain steps when frames are modified, but shift
    // the tag value exactly in --tags-only mode (issue #308).
    let modifier_db = opts.target_offset_db();
    if modifier_db != 0.0 {
        println!(
            "  Target: {:.1} {} ({} {} {} {:+.1} dB modifier)",
            target + modifier_db,
            mode.unit(),
            mode,
            target,
            mode.unit(),
            modifier_db,
        );
    } else {
        println!("  Target: {} {} ({})", target, mode.unit(), mode);
    }
}

fn require_replaygain_feature() {
    if !replaygain::is_available() {
        eprintln!(
            "{}: ReplayGain analysis requires the 'replaygain' feature",
            "error".red().bold()
        );
        eprintln!("  Install with: cargo install mp3rgain --features replaygain");
        std::process::exit(1);
    }
}

pub fn cmd_track_gain(files: &[PathBuf], opts: &Options) -> Result<()> {
    require_replaygain_feature();

    let dry_run_prefix = opts.dry_run_prefix();

    if opts.output_format == OutputFormat::Tsv {
        println!("{}", TSV_HEADER);
    }

    if opts.output_format == OutputFormat::Text && !opts.quiet {
        if opts.tags_only {
            println!(
                "{}{} Analyzing and {} track ReplayGain tags for {} file(s) (audio unchanged)",
                dry_run_prefix,
                "mp3rgain".green().bold(),
                if opts.dry_run {
                    "would write"
                } else {
                    "writing"
                },
                files.len()
            );
        } else {
            println!(
                "{}{} Analyzing and {} track gain to {} file(s)",
                dry_run_prefix,
                "mp3rgain".green().bold(),
                if opts.dry_run {
                    "would apply"
                } else {
                    "applying"
                },
                files.len()
            );
        }
        print_target_with_modifier(opts);
        println!();
    }

    let (json_results, successful, failed) =
        for_each_file_with_analysis_bar(files, opts, |file, analysis_pb| {
            process_track_gain(file, opts, analysis_pb).map(|(r, t)| (Some(r), t))
        })?;

    finish_with_summary(files.len(), json_results, successful, failed, opts)
}

/// `-s R` (issue #298): build an album report from stored tags. Requires
/// every file to carry parseable `REPLAYGAIN_TRACK_*` and `REPLAYGAIN_ALBUM_*`
/// values with no algorithm marker and a matching album gain. The stored
/// values are residuals relative to each file's current loudness, so a
/// partial or inconsistent set cannot be mixed with fresh analysis — any gap
/// returns `None` and the whole album is rescanned.
fn stored_album_report(files: &[PathBuf], opts: &Options) -> Option<AlbumAnalysisReport> {
    if !opts.stored_tags_usable() {
        return None;
    }
    let mut tracks = Vec::with_capacity(files.len());
    let mut album_values = Vec::with_capacity(files.len());
    for file in files {
        let tags = mp3rgain::read_gain_tags_auto(file, opts.tag_layout).ok()?;
        let values = tags.rg1_album_values()?;
        album_values.push((values.album_gain_db, values.album_peak));
        tracks.push(ReplayGainResult::from_stored_tags(
            values.track_gain_db,
            values.track_peak,
            AudioFileType::from_path(file),
            opts.analysis_mode,
        ));
    }
    let (album_gain, album_peak) = mp3rgain::consistent_album_gain(album_values)?;
    Some(AlbumAnalysisReport {
        album: AlbumGainResult::from_stored_tags(
            tracks,
            album_gain,
            album_peak,
            opts.analysis_mode,
        ),
        failures: Vec::new(),
        successful_indices: (0..files.len()).collect(),
    })
}

/// Outcome of one album run, before the JSON / exit-code epilogue.
struct AlbumRun {
    json_results: Vec<JsonFileResult>,
    album: Option<JsonAlbumResult>,
    successful: usize,
    failed: usize,
}

/// TSV header plus the text-mode banner shared by both album commands.
fn print_album_intro(file_count: usize, directories: Option<usize>, opts: &Options) {
    if opts.output_format == OutputFormat::Tsv {
        println!("{}", TSV_HEADER);
    }
    if opts.output_format == OutputFormat::Text && !opts.quiet {
        let scope = match directories {
            Some(n) => format!(" in {} directory(ies)", n),
            None => String::new(),
        };
        println!(
            "{}{} Analyzing album gain for {} file(s){}{}",
            opts.dry_run_prefix(),
            "mp3rgain".green().bold(),
            file_count,
            scope,
            if opts.tags_only {
                " (tags only, audio unchanged)"
            } else {
                ""
            }
        );
        print_target_with_modifier(opts);
        println!();
    }
}

pub fn cmd_album_gain(files: &[PathBuf], opts: &Options) -> Result<()> {
    require_replaygain_feature();
    print_album_intro(files.len(), None, opts);
    let run = run_album(files, opts)?;
    finish_with_album_summary(
        files.len(),
        run.json_results,
        run.album,
        run.successful,
        run.failed,
        opts,
    )
}

/// `-a --per-directory`: one album per parent directory (issue #324), the
/// grouping the GUI has used since #159. Directories are processed in path
/// order; one that fails analysis is counted and the run moves on to the
/// next, so a whole library can be tagged in a single invocation.
pub fn cmd_album_gain_per_directory(files: &[PathBuf], opts: &Options) -> Result<()> {
    require_replaygain_feature();
    let groups = group_by_parent(files);
    print_album_intro(files.len(), Some(groups.len()), opts);

    let mut json_results = Vec::with_capacity(files.len());
    let mut albums = Vec::with_capacity(groups.len());
    let (mut successful, mut failed) = (0, 0);
    for (i, (dir, group)) in groups.iter().enumerate() {
        if opts.output_format == OutputFormat::Text && !opts.quiet {
            if i > 0 {
                println!();
            }
            println!(
                "{} ({} file(s))",
                dir.display().to_string().bold(),
                group.len()
            );
        }
        let run = run_album(group, opts)?;
        if let Some(album) = run.album {
            albums.push(JsonDirectoryAlbum {
                directory: dir.display().to_string(),
                files: group.len(),
                album,
            });
        }
        json_results.extend(run.json_results);
        successful += run.successful;
        failed += run.failed;
    }

    if opts.output_format == OutputFormat::Json {
        let output = JsonOutput {
            files: Some(json_results),
            album: None,
            albums: Some(albums),
            summary: Some(create_json_summary(
                files.len(),
                successful,
                failed,
                opts.dry_run,
            )),
        };
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_dry_run_notice(opts);
    }
    exit_if_failed(failed);
    Ok(())
}

/// Group files by their immediate parent directory, as given on the command
/// line. Bare file names land in `.`.
fn group_by_parent(files: &[PathBuf]) -> BTreeMap<PathBuf, Vec<PathBuf>> {
    let mut groups: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for file in files {
        let dir = file
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        groups
            .entry(dir.to_path_buf())
            .or_default()
            .push(file.clone());
    }
    groups
}

/// Analyze and apply album gain to `files` as one album, returning what the
/// epilogue needs instead of printing JSON or exiting, so the per-directory
/// driver can aggregate several albums (issue #324).
fn run_album(files: &[PathBuf], opts: &Options) -> Result<AlbumRun> {
    let file_refs: Vec<&Path> = files.iter().map(|p| p.as_path()).collect();

    let threads = effective_threads(opts);
    let parallel = threads > 1 && files.len() > 1;

    // -s R: reuse stored album tags when every file carries a consistent
    // set; otherwise fall back to the full rescan (issue #298).
    let album_analysis = match stored_album_report(files, opts) {
        Some(report) => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!("  {} Using stored tags (no rescan)", "->".cyan());
            }
            Ok(report)
        }
        None => {
            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!("  {} Analyzing tracks...", "->".cyan());
            }
            run_album_analysis(&file_refs, opts, opts.skip_errors)
        }
    };

    match album_analysis {
        Ok(report) => {
            let AlbumAnalysisReport {
                album: album_result,
                failures,
                successful_indices,
            } = report;

            // Index failures by file position for O(1) lookup during the
            // apply phase (only --skip-errors produces non-empty failures).
            // Walk failures once, reporting skipped files in text mode.
            let failure_count = failures.len();
            let mut failure_msgs: Vec<Option<String>> = vec![None; files.len()];
            let report_skipped = opts.output_format == OutputFormat::Text && !opts.quiet;
            for (idx, msg) in failures {
                if report_skipped {
                    let filename = get_filename(&files[idx]);
                    eprintln!("  {} {} - {} (skipped)", "x".red(), filename, msg);
                }
                failure_msgs[idx] = Some(msg);
            }

            // Build a mapping from file index -> track-result index. Files that
            // failed analysis map to None.
            let mut file_to_track: Vec<Option<usize>> = vec![None; files.len()];
            for (track_idx, file_idx) in successful_indices.iter().enumerate() {
                file_to_track[*file_idx] = Some(track_idx);
            }

            // Apply gain modifier (-m steps + -d dB, combined into steps)
            let modifier_steps = opts.gain_modifier_steps();
            let modified_gain_steps = album_result.album_gain_steps() + modifier_steps;

            // --tags-only writes the album gain into the tag instead of the
            // frames, capped at the album peak's headroom under `-k` so every
            // file in the set still carries the same value (issue #308).
            let album_tag_gain_db = opts.tags_only.then(|| {
                capped_tag_gain(
                    album_result.album_gain_db() + opts.target_offset_db(),
                    album_result.album_peak(),
                    opts.prevent_clipping,
                )
            });

            let is_lufs = opts.analysis_mode.target_lufs().is_some();
            let json_album = JsonAlbumResult {
                loudness_db: album_result.album_loudness_db(),
                loudness_lufs: is_lufs.then(|| album_result.album_loudness_db()),
                analysis_mode: Some(opts.analysis_mode.name()),
                gain_db: album_result.album_gain_db(),
                // Nothing is applied to the audio in --tags-only mode.
                gain_steps: if opts.tags_only {
                    0
                } else {
                    modified_gain_steps
                },
                tag_gain_db: album_tag_gain_db,
                peak: album_result.album_peak(),
            };

            let album_info = AacAlbumInfo::from(&album_result);

            // mp3gain-compatible TSV rows, emitted before the apply so the
            // global_gain columns describe the files as they were scanned.
            if opts.output_format == OutputFormat::Tsv {
                emit_album_tsv_rows(files, &album_result, &file_to_track, opts)?;
            }

            if opts.output_format == OutputFormat::Text && !opts.quiet {
                println!();
                println!(
                    "  Album loudness: {:.1} {}",
                    album_result.album_loudness_db(),
                    opts.analysis_mode.unit()
                );
                match album_tag_gain_db {
                    Some(tag_gain) => println!(
                        "  Album gain:     {:+.2} dB (tag value, audio unchanged)",
                        tag_gain
                    ),
                    None => println!(
                        "  Album gain:     {:+.1} dB ({} steps{})",
                        album_result.album_gain_db(),
                        album_result.album_gain_steps(),
                        if modifier_steps != 0 {
                            format!(" + {} = {}", modifier_steps, modified_gain_steps)
                        } else {
                            String::new()
                        }
                    ),
                }
                println!("  Album peak:     {:.4}", album_result.album_peak());
                println!();
            }

            // Apply album gain to all files
            let steps = modified_gain_steps;

            // A net 0-step album adjustment still has work to do when tags
            // would be written or `-k` must attenuate a clipping track, the
            // same reasoning as the track path (issue #206). Skipping outright
            // loses the per-track REPLAYGAIN_* tags for an album that merely
            // happens to sit on target (reported on the Hydrogenaudio forum:
            // album gain -0.04 dB, yet track 3 wants +1.46 dB).
            let any_aac = album_result
                .tracks()
                .iter()
                .any(|t| t.file_type() == AudioFileType::Aac);
            let max_peak = album_result
                .tracks()
                .iter()
                .map(|t| t.peak())
                .fold(0.0, f64::max);
            if apply_is_noop(opts, steps, any_aac, max_peak) {
                let json_results: Vec<JsonFileResult> = if opts.output_format == OutputFormat::Json
                {
                    files
                        .iter()
                        .enumerate()
                        .map(|(i, file)| match file_to_track[i] {
                            Some(track_idx) => {
                                let track = &album_result.tracks()[track_idx];
                                JsonFileResult {
                                    status: Some(FileStatus::Skipped),
                                    gain_applied_steps: Some(0),
                                    gain_applied_db: Some(0.0),
                                    ..JsonFileResult::from_analysis(file, track)
                                }
                            }
                            None => JsonFileResult::error(
                                file,
                                failure_msgs[i].as_deref().unwrap_or("analysis failed"),
                            ),
                        })
                        .collect()
                } else {
                    if !opts.quiet {
                        println!("  {} No adjustment needed", ".".cyan());
                    }
                    Vec::new()
                };
                return Ok(AlbumRun {
                    json_results,
                    album: Some(json_album),
                    successful: 0,
                    failed: failure_count,
                });
            }

            let pb = create_progress_bar(files.len(), opts);
            let mut json_results: Vec<JsonFileResult> = Vec::with_capacity(files.len());
            let mut successful = 0;
            let mut failed = 0;
            // Post-apply (max, min) global_gain range per file, taken from the
            // apply pass so the album MINMAX step below doesn't re-analyze
            // every file (issue #232).
            let mut range_by_idx: Vec<Option<(u8, u8)>> = vec![None; files.len()];

            if parallel {
                let pb_ref = pb.as_ref();
                // Process only successfully-analyzed files in parallel.
                type Collected = (usize, JsonFileResult, String, Option<(u8, u8)>);
                let collected: Vec<Collected> = successful_indices
                    .par_iter()
                    .enumerate()
                    .map(|(track_idx, &file_idx)| -> Result<Collected> {
                        let file = &files[file_idx];
                        let track_result = &album_result.tracks()[track_idx];
                        let (result, text, range) = process_apply_replaygain_with_album(
                            file,
                            steps,
                            track_result,
                            opts,
                            Some(&album_info),
                        )?;
                        if let Some(pb) = pb_ref {
                            pb.set_message(get_filename(file).to_string());
                            pb.inc(1);
                        }
                        Ok((file_idx, result, text, range))
                    })
                    .collect::<Result<Vec<_>>>()?;

                let stdout = io::stdout();
                let mut handle = stdout.lock();
                for (_, _, text, _) in &collected {
                    if !text.is_empty() {
                        handle.write_all(text.as_bytes())?;
                    }
                }
                drop(handle);

                for (file_idx, _, _, range) in &collected {
                    range_by_idx[*file_idx] = *range;
                }

                // Re-assemble json_results in input file order, interleaving
                // failures with successes so the JSON output stays aligned
                // with the input list.
                if opts.output_format == OutputFormat::Json {
                    let mut by_index: Vec<Option<JsonFileResult>> =
                        (0..files.len()).map(|_| None).collect();
                    for (file_idx, result, _, _) in &collected {
                        by_index[*file_idx] = Some(result.clone());
                    }
                    for (i, slot) in by_index.iter_mut().enumerate() {
                        if slot.is_none() {
                            if let Some(msg) = &failure_msgs[i] {
                                *slot = Some(JsonFileResult::error(&files[i], msg));
                            }
                        }
                    }
                    for entry in by_index.into_iter().flatten() {
                        update_counters(&entry, &mut successful, &mut failed);
                        json_results.push(entry);
                    }
                } else {
                    for (_, result, _, _) in collected {
                        update_counters(&result, &mut successful, &mut failed);
                    }
                    failed += failure_count;
                }
            } else {
                for (i, file) in files.iter().enumerate() {
                    let filename = get_filename(file);
                    progress_set_message(&pb, filename);

                    let result = match file_to_track[i] {
                        Some(track_idx) => {
                            let track_result = &album_result.tracks()[track_idx];
                            let (result, text, range) = process_apply_replaygain_with_album(
                                file,
                                steps,
                                track_result,
                                opts,
                                Some(&album_info),
                            )?;
                            if !text.is_empty() {
                                print!("{}", text);
                            }
                            range_by_idx[i] = range;
                            result
                        }
                        None => JsonFileResult::error(
                            file,
                            failure_msgs[i].as_deref().unwrap_or("analysis failed"),
                        ),
                    };
                    update_counters(&result, &mut successful, &mut failed);

                    if opts.output_format == OutputFormat::Json {
                        json_results.push(result);
                    }

                    progress_inc(&pb);
                }
            }

            progress_finish(pb);

            // MP3GAIN_ALBUM_MINMAX: the album-wide post-apply global_gain range,
            // matching mp3gain's album (`-a`) mode (issue #210). Written to every
            // MP3 file after all gain is applied (the range is only known once the
            // whole album is done). APEv2 only — mp3gain has no AAC, and `-s i`
            // uses ID3v2; best-effort, so a tag hiccup never fails the album.
            // Skipped entirely in --tags-only mode: MP3GAIN_ALBUM_MINMAX
            // describes a global_gain range that a gain apply produced, and
            // no apply happened (issue #308).
            if !opts.dry_run
                && !opts.tags_only
                && opts.stored_tag_mode != StoredTagMode::Skip
                && !opts.tag_layout.mp3gain_in_id3v2()
            {
                let album_files: Vec<(&Path, Option<(u8, u8)>)> = successful_indices
                    .iter()
                    .map(|&i| (files[i].as_path(), range_by_idx[i]))
                    .collect();
                mp3rgain::write_album_minmax(&album_files);
            }

            Ok(AlbumRun {
                json_results,
                album: Some(json_album),
                successful,
                failed,
            })
        }
        Err(e) => {
            // Every file counts as failed. JSON gets one error entry per file
            // so the caller can see which album broke; text goes to stderr.
            let json_results = if opts.output_format == OutputFormat::Json {
                files
                    .iter()
                    .map(|f| JsonFileResult::error(f, e.to_string()))
                    .collect()
            } else {
                eprintln!("{}: Failed to analyze album: {}", "error".red().bold(), e);
                Vec::new()
            };
            Ok(AlbumRun {
                json_results,
                album: None,
                successful: 0,
                failed: files.len(),
            })
        }
    }
}

/// Per-file rows plus the `"Album"` summary row for `-a -o tsv`, matching what
/// `-o tsv` alone prints for the same set of files.
fn emit_album_tsv_rows(
    files: &[PathBuf],
    album_result: &AlbumGainResult,
    file_to_track: &[Option<usize>],
    opts: &Options,
) -> Result<()> {
    // The frame scan re-reads each file, so run it in parallel the way
    // cmd_info does rather than serializing it inside the emit loop.
    let gain_ranges: Vec<(u8, u8)> = files
        .par_iter()
        .enumerate()
        .map(|(i, file)| match file_to_track[i] {
            Some(_) => scan_gain_range_for_row(file),
            None => (255, 0),
        })
        .collect();

    let mut album_max_gain: Option<u8> = None;
    let mut album_min_gain: Option<u8> = None;
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    for (i, file) in files.iter().enumerate() {
        let Some(track_idx) = file_to_track[i] else {
            continue;
        };
        let track = &album_result.tracks()[track_idx];
        handle.write_all(tsv_rg_row(file, opts, track, gain_ranges[i]).as_bytes())?;
        let (max_gain, min_gain) = gain_ranges[i];
        album_max_gain = album_max_gain.max(Some(max_gain));
        album_min_gain = Some(album_min_gain.map_or(min_gain, |m: u8| m.min(min_gain)));
    }

    if album_max_gain.is_some() {
        let (album_gain_steps, album_gain_db) = opts.modified_gain(
            album_result.album_gain_steps(),
            album_result.album_gain_db(),
        );
        writeln!(
            handle,
            "\"Album\"\t{}\t{:.6}\t{:.6}\t{}\t{}",
            album_gain_steps,
            album_gain_db,
            opts.tsv_peak(album_result.album_peak()),
            album_max_gain.unwrap_or(255),
            album_min_gain.unwrap_or(0)
        )?;
    }

    Ok(())
}
