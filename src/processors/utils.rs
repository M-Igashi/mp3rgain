use anyhow::Result;
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use crate::cli::options::Options;

pub fn apply_with_temp_file<F>(file: &Path, operation: F, opts: &Options) -> Result<usize>
where
    F: FnOnce(&Path) -> Result<usize>,
{
    if opts.use_temp_file {
        // Create temp file in the same directory
        let parent = file.parent().unwrap_or(Path::new("."));
        let temp_path = parent.join(format!(".mp3rgain_temp_{}.mp3", std::process::id()));

        // Copy original to temp
        fs::copy(file, &temp_path)?;

        // Apply operation to temp file
        match operation(&temp_path) {
            Ok(frames) => {
                // Replace original with temp
                fs::rename(&temp_path, file)?;
                Ok(frames)
            }
            Err(e) => {
                // Clean up temp file on error
                let _ = fs::remove_file(&temp_path);
                Err(e)
            }
        }
    } else {
        operation(file)
    }
}

pub fn restore_timestamp(file: &Path, mtime: SystemTime) {
    let _ = std::fs::File::options()
        .write(true)
        .open(file)
        .and_then(|f| f.set_times(std::fs::FileTimes::new().set_modified(mtime)));
}
