use std::path::Path;

/// Extract filename from path, returning "unknown" if extraction fails
pub fn get_filename(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
}

/// Path as given on the command line, for machine-readable output.
///
/// TSV rows used to print the bare filename, which is ambiguous as soon as
/// more than one directory is scanned in a single run (reported on the
/// Hydrogenaudio forum). JSON has always carried the full path; TSV now
/// matches it.
pub fn get_path(path: &Path) -> std::borrow::Cow<'_, str> {
    path.to_string_lossy()
}
