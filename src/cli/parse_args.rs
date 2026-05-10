use anyhow::Result;
use colored::*;
use mp3rgain::Channel;
use std::path::PathBuf;

use super::options::{Options, OutputFormat, StoredTagMode};
use super::usage::{print_usage, print_version};

pub fn parse_args(args: &[String]) -> Result<Options> {
    let mut opts = Options::default();
    let mut i = 0;

    while i < args.len() {
        let arg = &args[i];

        if arg == "--dry-run" {
            opts.dry_run = true;
            i += 1;
            continue;
        }

        if arg == "--skip-errors" {
            opts.skip_errors = true;
            i += 1;
            continue;
        }

        if arg == "--help" {
            print_usage();
            std::process::exit(0);
        }

        if arg == "--version" {
            print_version();
            std::process::exit(0);
        }

        if arg == "--threads" {
            i += 1;
            if i >= args.len() {
                eprintln!("{}: --threads requires an argument", "error".red().bold());
                std::process::exit(1);
            }
            opts.threads = Some(
                args[i]
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid --threads value: {}", args[i]))?,
            );
            i += 1;
            continue;
        }

        if let Some(rest) = arg.strip_prefix("--threads=") {
            opts.threads = Some(
                rest.parse()
                    .map_err(|_| anyhow::anyhow!("invalid --threads value: {}", rest))?,
            );
            i += 1;
            continue;
        }

        if arg.starts_with('-') && arg.len() > 1 && !arg.starts_with("--") {
            let flag = &arg[1..];

            match flag {
                "g" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -g requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    opts.gain_steps = Some(
                        args[i]
                            .parse()
                            .map_err(|_| anyhow::anyhow!("invalid gain value: {}", args[i]))?,
                    );
                }
                "d" => {
                    // mp3gain compatible: -d modifies the suggested dB gain
                    // (adjusts target level relative to 89 dB reference)
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -d requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    opts.gain_modifier_db = args[i]
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid dB value: {}", args[i]))?;
                }
                "m" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -m requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    opts.gain_modifier = args[i]
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid modifier value: {}", args[i]))?;
                }
                "s" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -s requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    match args[i].as_str() {
                        "c" => opts.stored_tag_mode = StoredTagMode::Check,
                        "d" => opts.stored_tag_mode = StoredTagMode::Delete,
                        "s" => opts.stored_tag_mode = StoredTagMode::Skip,
                        "r" => opts.stored_tag_mode = StoredTagMode::Recalc,
                        "i" => {
                            opts.use_id3v2 = true;
                        }
                        "a" => opts.stored_tag_mode = StoredTagMode::UseApev2,
                        other => {
                            eprintln!(
                                "{}: unknown -s mode '{}', use c/d/s/r/i/a",
                                "error".red().bold(),
                                other
                            );
                            std::process::exit(1);
                        }
                    }
                }
                "o" => {
                    // mp3gain compatibility: -o without argument means TSV output
                    // Check if next arg is a valid format specifier
                    let next_is_format = if i + 1 < args.len() {
                        matches!(
                            args[i + 1].to_lowercase().as_str(),
                            "json" | "text" | "tsv" | "db"
                        )
                    } else {
                        false
                    };

                    if next_is_format {
                        i += 1;
                        match args[i].to_lowercase().as_str() {
                            "json" => opts.output_format = OutputFormat::Json,
                            "text" => opts.output_format = OutputFormat::Text,
                            "tsv" | "db" => opts.output_format = OutputFormat::Tsv,
                            _ => unreachable!(),
                        }
                    } else {
                        // mp3gain compatible: -o alone means TSV
                        opts.output_format = OutputFormat::Tsv;
                    }
                }
                "l" => {
                    // -l <channel> <gain> : apply gain to specific channel
                    i += 1;
                    if i >= args.len() {
                        eprintln!(
                            "{}: -l requires two arguments: <channel> <gain>",
                            "error".red().bold()
                        );
                        std::process::exit(1);
                    }
                    let channel_arg: usize = args[i].parse().map_err(|_| {
                        anyhow::anyhow!(
                            "invalid channel number: {} (use 0 for left, 1 for right)",
                            args[i]
                        )
                    })?;
                    let channel = Channel::from_index(channel_arg).ok_or_else(|| {
                        anyhow::anyhow!(
                            "invalid channel: {} (use 0 for left, 1 for right)",
                            channel_arg
                        )
                    })?;

                    i += 1;
                    if i >= args.len() {
                        eprintln!(
                            "{}: -l requires two arguments: <channel> <gain>",
                            "error".red().bold()
                        );
                        std::process::exit(1);
                    }
                    let gain: i32 = args[i]
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid gain value: {}", args[i]))?;

                    opts.channel_gain = Some((channel, gain));
                }
                "r" => opts.track_gain = true,
                "a" => opts.album_gain = true,
                "e" => opts.skip_album = true,
                "x" => opts.max_amplitude_only = true,
                "i" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -i requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    opts.track_index = Some(
                        args[i]
                            .parse()
                            .map_err(|_| anyhow::anyhow!("invalid track index: {}", args[i]))?,
                    );
                }
                "j" => {
                    i += 1;
                    if i >= args.len() {
                        eprintln!("{}: -j requires an argument", "error".red().bold());
                        std::process::exit(1);
                    }
                    opts.threads = Some(
                        args[i]
                            .parse()
                            .map_err(|_| anyhow::anyhow!("invalid -j value: {}", args[i]))?,
                    );
                }
                "u" => opts.undo = true,
                "p" => opts.preserve_timestamp = true,
                "c" => opts.ignore_clipping = true,
                "k" => opts.prevent_clipping = true,
                "q" => opts.quiet = true,
                "R" => opts.recursive = true,
                "n" => opts.dry_run = true,
                "w" => opts.wrap_gain = true,
                "t" => opts.use_temp_file = true,
                "f" => opts.assume_mpeg2 = true,
                "v" | "-version" => {
                    print_version();
                    std::process::exit(0);
                }
                "h" | "-help" => {
                    print_usage();
                    std::process::exit(0);
                }
                // Handle combined short flags like -qp, -kc, etc.
                _ if flag.chars().all(|c| "pqckuranRewxtf".contains(c)) => {
                    for c in flag.chars() {
                        match c {
                            'p' => opts.preserve_timestamp = true,
                            'q' => opts.quiet = true,
                            'c' => opts.ignore_clipping = true,
                            'k' => opts.prevent_clipping = true,
                            'u' => opts.undo = true,
                            'r' => opts.track_gain = true,
                            'a' => opts.album_gain = true,
                            'n' => opts.dry_run = true,
                            'R' => opts.recursive = true,
                            'e' => opts.skip_album = true,
                            'w' => opts.wrap_gain = true,
                            'x' => opts.max_amplitude_only = true,
                            't' => opts.use_temp_file = true,
                            'f' => opts.assume_mpeg2 = true,
                            _ => {}
                        }
                    }
                }
                // Handle -g with attached value (e.g., -g2)
                _ if flag.starts_with('g') => {
                    let val = &flag[1..];
                    opts.gain_steps = Some(
                        val.parse()
                            .map_err(|_| anyhow::anyhow!("invalid gain value: {}", val))?,
                    );
                }
                // Handle -d with attached value (e.g., -d4.5)
                _ if flag.starts_with('d') => {
                    let val = &flag[1..];
                    opts.gain_modifier_db = val
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid dB value: {}", val))?;
                }
                // Handle -m with attached value (e.g., -m2)
                _ if flag.starts_with('m') => {
                    let val = &flag[1..];
                    opts.gain_modifier = val
                        .parse()
                        .map_err(|_| anyhow::anyhow!("invalid modifier value: {}", val))?;
                }
                // Handle -i with attached value (e.g., -i1)
                _ if flag.starts_with('i') => {
                    let val = &flag[1..];
                    opts.track_index = Some(
                        val.parse()
                            .map_err(|_| anyhow::anyhow!("invalid track index: {}", val))?,
                    );
                }
                // Handle -j with attached value (e.g., -j4)
                _ if flag.starts_with('j') => {
                    let val = &flag[1..];
                    opts.threads = Some(
                        val.parse()
                            .map_err(|_| anyhow::anyhow!("invalid -j value: {}", val))?,
                    );
                }
                _ => {
                    eprintln!("{}: unknown option: -{}", "warning".yellow().bold(), flag);
                }
            }
        } else if !arg.starts_with("--") {
            // It's a file
            opts.files.push(PathBuf::from(arg));
        }

        i += 1;
    }

    Ok(opts)
}

pub fn expand_files_recursive(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();

    for path in paths {
        if path.is_dir() {
            result.extend(mp3rgain::collect_audio_files(path, true)?);
        } else {
            result.push(path.clone());
        }
    }

    result.sort();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mp3rgain::Channel;

    fn args(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_are_empty() {
        let opts = parse_args(&[]).unwrap();
        assert!(opts.files.is_empty());
        assert_eq!(opts.gain_steps, None);
        assert_eq!(opts.gain_modifier, 0);
        assert_eq!(opts.gain_modifier_db, 0.0);
        assert!(!opts.track_gain);
        assert!(!opts.album_gain);
        assert!(!opts.dry_run);
        assert!(!opts.quiet);
    }

    #[test]
    fn collects_positional_files() {
        let opts = parse_args(&args(&["a.mp3", "b.mp3", "c.mp3"])).unwrap();
        assert_eq!(opts.files.len(), 3);
        assert_eq!(opts.files[0].to_str(), Some("a.mp3"));
        assert_eq!(opts.files[2].to_str(), Some("c.mp3"));
    }

    #[test]
    fn g_flag_with_separate_value() {
        let opts = parse_args(&args(&["-g", "2", "song.mp3"])).unwrap();
        assert_eq!(opts.gain_steps, Some(2));
        assert_eq!(opts.files.len(), 1);
    }

    #[test]
    fn g_flag_with_attached_value() {
        let opts = parse_args(&args(&["-g2", "song.mp3"])).unwrap();
        assert_eq!(opts.gain_steps, Some(2));
    }

    #[test]
    fn g_flag_negative() {
        let opts = parse_args(&args(&["-g", "-3", "song.mp3"])).unwrap();
        assert_eq!(opts.gain_steps, Some(-3));
    }

    #[test]
    fn d_flag_db_modifier() {
        let opts = parse_args(&args(&["-d", "4.5", "song.mp3"])).unwrap();
        assert_eq!(opts.gain_modifier_db, 4.5);
    }

    #[test]
    fn d_flag_attached_value() {
        let opts = parse_args(&args(&["-d4.5", "song.mp3"])).unwrap();
        assert_eq!(opts.gain_modifier_db, 4.5);
    }

    #[test]
    fn m_flag_modifier_steps() {
        let opts = parse_args(&args(&["-m", "2", "song.mp3"])).unwrap();
        assert_eq!(opts.gain_modifier, 2);
    }

    #[test]
    fn boolean_flags_individual() {
        let opts = parse_args(&args(&[
            "-r", "-p", "-q", "-k", "-c", "-w", "-t", "-R", "-n",
        ]))
        .unwrap();
        assert!(opts.track_gain);
        assert!(opts.preserve_timestamp);
        assert!(opts.quiet);
        assert!(opts.prevent_clipping);
        assert!(opts.ignore_clipping);
        assert!(opts.wrap_gain);
        assert!(opts.use_temp_file);
        assert!(opts.recursive);
        assert!(opts.dry_run);
    }

    #[test]
    fn combined_short_flags() {
        let opts = parse_args(&args(&["-qp", "song.mp3"])).unwrap();
        assert!(opts.quiet);
        assert!(opts.preserve_timestamp);

        let opts = parse_args(&args(&["-kc", "song.mp3"])).unwrap();
        assert!(opts.prevent_clipping);
        assert!(opts.ignore_clipping);

        let opts = parse_args(&args(&["-rn", "song.mp3"])).unwrap();
        assert!(opts.track_gain);
        assert!(opts.dry_run);
    }

    #[test]
    fn dry_run_long_flag() {
        let opts = parse_args(&args(&["--dry-run", "song.mp3"])).unwrap();
        assert!(opts.dry_run);
    }

    #[test]
    fn track_and_album_flags() {
        let opts = parse_args(&args(&["-r", "song.mp3"])).unwrap();
        assert!(opts.track_gain);
        assert!(!opts.album_gain);

        let opts = parse_args(&args(&["-a", "*.mp3"])).unwrap();
        assert!(opts.album_gain);
        assert!(!opts.track_gain);

        let opts = parse_args(&args(&["-e", "song.mp3"])).unwrap();
        assert!(opts.skip_album);
    }

    #[test]
    fn undo_flag() {
        let opts = parse_args(&args(&["-u", "song.mp3"])).unwrap();
        assert!(opts.undo);
    }

    #[test]
    fn max_amplitude_flag() {
        let opts = parse_args(&args(&["-x", "song.mp3"])).unwrap();
        assert!(opts.max_amplitude_only);
    }

    #[test]
    fn s_modes() {
        assert_eq!(
            parse_args(&args(&["-s", "c"])).unwrap().stored_tag_mode,
            StoredTagMode::Check
        );
        assert_eq!(
            parse_args(&args(&["-s", "d"])).unwrap().stored_tag_mode,
            StoredTagMode::Delete
        );
        assert_eq!(
            parse_args(&args(&["-s", "s"])).unwrap().stored_tag_mode,
            StoredTagMode::Skip
        );
        assert_eq!(
            parse_args(&args(&["-s", "r"])).unwrap().stored_tag_mode,
            StoredTagMode::Recalc
        );
        assert_eq!(
            parse_args(&args(&["-s", "a"])).unwrap().stored_tag_mode,
            StoredTagMode::UseApev2
        );
        // -s i sets use_id3v2 instead of stored_tag_mode
        let opts = parse_args(&args(&["-s", "i"])).unwrap();
        assert!(opts.use_id3v2);
    }

    #[test]
    fn output_formats() {
        assert_eq!(
            parse_args(&args(&["-o", "json", "song.mp3"]))
                .unwrap()
                .output_format,
            OutputFormat::Json
        );
        assert_eq!(
            parse_args(&args(&["-o", "text", "song.mp3"]))
                .unwrap()
                .output_format,
            OutputFormat::Text
        );
        assert_eq!(
            parse_args(&args(&["-o", "tsv", "song.mp3"]))
                .unwrap()
                .output_format,
            OutputFormat::Tsv
        );
        assert_eq!(
            parse_args(&args(&["-o", "db", "song.mp3"]))
                .unwrap()
                .output_format,
            OutputFormat::Tsv
        );
    }

    #[test]
    fn output_format_case_insensitive() {
        assert_eq!(
            parse_args(&args(&["-o", "JSON", "song.mp3"]))
                .unwrap()
                .output_format,
            OutputFormat::Json
        );
    }

    #[test]
    fn o_alone_means_tsv_mp3gain_compat() {
        // -o alone (no format arg or trailing only files) defaults to TSV
        let opts = parse_args(&args(&["-o", "song.mp3"])).unwrap();
        assert_eq!(opts.output_format, OutputFormat::Tsv);
        assert_eq!(opts.files.len(), 1);
    }

    #[test]
    fn channel_gain_left() {
        let opts = parse_args(&args(&["-l", "0", "3", "song.mp3"])).unwrap();
        assert_eq!(opts.channel_gain, Some((Channel::Left, 3)));
    }

    #[test]
    fn channel_gain_right_negative() {
        let opts = parse_args(&args(&["-l", "1", "-2", "song.mp3"])).unwrap();
        assert_eq!(opts.channel_gain, Some((Channel::Right, -2)));
    }

    #[test]
    fn track_index_separate() {
        let opts = parse_args(&args(&["-i", "1", "song.m4a"])).unwrap();
        assert_eq!(opts.track_index, Some(1));
    }

    #[test]
    fn track_index_attached() {
        let opts = parse_args(&args(&["-i1", "song.m4a"])).unwrap();
        assert_eq!(opts.track_index, Some(1));
    }

    #[test]
    fn assume_mpeg2_flag() {
        let opts = parse_args(&args(&["-f", "song.mp3"])).unwrap();
        assert!(opts.assume_mpeg2);
    }

    #[test]
    fn j_flag_separate_value() {
        let opts = parse_args(&args(&["-j", "4", "song.mp3"])).unwrap();
        assert_eq!(opts.threads, Some(4));
    }

    #[test]
    fn j_flag_attached_value() {
        let opts = parse_args(&args(&["-j8", "song.mp3"])).unwrap();
        assert_eq!(opts.threads, Some(8));
    }

    #[test]
    fn j_flag_zero_means_auto() {
        let opts = parse_args(&args(&["-j", "0", "song.mp3"])).unwrap();
        assert_eq!(opts.threads, Some(0));
    }

    #[test]
    fn j_flag_one_serial() {
        let opts = parse_args(&args(&["-j", "1", "song.mp3"])).unwrap();
        assert_eq!(opts.threads, Some(1));
    }

    #[test]
    fn threads_long_flag() {
        let opts = parse_args(&args(&["--threads", "2", "song.mp3"])).unwrap();
        assert_eq!(opts.threads, Some(2));
    }

    #[test]
    fn threads_long_flag_equals() {
        let opts = parse_args(&args(&["--threads=6", "song.mp3"])).unwrap();
        assert_eq!(opts.threads, Some(6));
    }

    #[test]
    fn invalid_j_returns_error() {
        let result = parse_args(&args(&["-j", "abc"]));
        assert!(result.is_err());
        let result = parse_args(&args(&["-jabc"]));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_gain_returns_error() {
        let result = parse_args(&args(&["-g", "abc"]));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_db_returns_error() {
        let result = parse_args(&args(&["-d", "xyz"]));
        assert!(result.is_err());
    }

    #[test]
    fn invalid_attached_gain_returns_error() {
        let result = parse_args(&args(&["-gabc"]));
        assert!(result.is_err());
    }

    #[test]
    fn skip_errors_long_flag() {
        let opts = parse_args(&args(&["--skip-errors", "song.mp3"])).unwrap();
        assert!(opts.skip_errors);
        assert_eq!(opts.files.len(), 1);

        let opts = parse_args(&args(&["song.mp3"])).unwrap();
        assert!(!opts.skip_errors);
    }

    #[test]
    fn flags_and_files_interleaved() {
        let opts = parse_args(&args(&["-g", "2", "a.mp3", "-p", "b.mp3"])).unwrap();
        assert_eq!(opts.gain_steps, Some(2));
        assert!(opts.preserve_timestamp);
        assert_eq!(opts.files.len(), 2);
    }
}
