use std::path::Path;

/// Extract filename from path, returning "unknown" if extraction fails
pub fn get_filename(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
}
