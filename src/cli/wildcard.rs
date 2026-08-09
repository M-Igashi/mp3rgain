//! Shell-style wildcard expansion for Windows.
//!
//! Unix shells expand `*.mp3` before the process starts, but cmd.exe and
//! PowerShell hand the pattern through untouched. Without this, a Windows user
//! typing `mp3rgain *.mp3` gets "Failed to open '*.mp3'" (issue reported on the
//! foobar2000 forum). The original mp3gain got this for free from the MSVC
//! `setargv` runtime hook.

use std::path::{Path, PathBuf};

fn has_wildcard(s: &str) -> bool {
    s.contains('*') || s.contains('?')
}

/// Matches `name` against a pattern where `*` is any sequence of characters and
/// `?` is exactly one. Case-insensitive, matching Windows filesystem semantics.
fn wildcard_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();

    let mut pi = 0;
    let mut ni = 0;
    let mut star = None;
    let mut retry = 0;

    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            retry = ni;
            pi += 1;
        } else if let Some(s) = star {
            retry += 1;
            pi = s + 1;
            ni = retry;
        } else {
            return false;
        }
    }

    p[pi..].iter().all(|&c| c == '*')
}

/// Expands wildcards in the final component of each path.
///
/// Paths without a wildcard, and patterns matching nothing, are passed through
/// unchanged so the caller still reports the usual "file not found" error.
pub fn expand(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut result = Vec::with_capacity(paths.len());

    for path in paths {
        let pattern = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| has_wildcard(n))
            .map(str::to_string);

        let Some(pattern) = pattern else {
            result.push(path);
            continue;
        };

        let dir = path.parent().unwrap_or(Path::new(""));
        let search = if dir.as_os_str().is_empty() {
            Path::new(".")
        } else {
            dir
        };

        let Ok(entries) = std::fs::read_dir(search) else {
            result.push(path);
            continue;
        };

        let mut matches: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| wildcard_match(&pattern, n))
            })
            .map(|e| dir.join(e.file_name()))
            .collect();

        if matches.is_empty() {
            result.push(path);
        } else {
            matches.sort();
            result.append(&mut matches);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_star_and_question() {
        assert!(wildcard_match("*.m4a", "song.m4a"));
        assert!(wildcard_match("*.m4a", ".m4a"));
        assert!(!wildcard_match("*.m4a", "song.mp3"));
        assert!(wildcard_match("track??.mp3", "track01.mp3"));
        assert!(!wildcard_match("track??.mp3", "track1.mp3"));
        assert!(wildcard_match("*", "anything"));
        assert!(wildcard_match("a*b*c", "axxbyyc"));
        assert!(!wildcard_match("a*b*c", "axxbyy"));
    }

    #[test]
    fn matches_case_insensitively() {
        assert!(wildcard_match("*.m4a", "SONG.M4A"));
        assert!(wildcard_match("*.MP3", "song.mp3"));
    }

    #[test]
    fn expands_pattern_in_directory() {
        let dir = std::env::temp_dir().join("mp3rgain_wildcard_test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b.m4a", "a.m4a", "c.mp3"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }

        let expanded = expand(vec![dir.join("*.m4a")]);
        assert_eq!(expanded, vec![dir.join("a.m4a"), dir.join("b.m4a")]);

        // No match: the pattern survives so the caller reports it verbatim.
        let missing = dir.join("*.flac");
        assert_eq!(expand(vec![missing.clone()]), vec![missing]);

        // Plain paths are untouched, even nonexistent ones.
        let plain = dir.join("c.mp3");
        assert_eq!(expand(vec![plain.clone()]), vec![plain]);

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
