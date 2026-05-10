use mp3rgain::Channel;
use std::path::PathBuf;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Tsv, // Tab-separated values (database-friendly)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StoredTagMode {
    #[default]
    None, // Default behavior
    Check,    // -s c: Check/show stored tag info
    Delete,   // -s d: Delete stored tag info
    Skip,     // -s s: Skip (ignore) stored tag info
    Recalc,   // -s r: Force recalculation
    UseApev2, // -s a: Use APEv2 tags (default)
}

/// Album gain info for AAC files
pub struct AacAlbumInfo {
    pub album_gain_db: f64,
    pub album_peak: f64,
}

#[derive(Default)]
pub struct Options {
    // Gain options
    pub gain_steps: Option<i32>,              // -g <i>
    pub gain_modifier_db: f64, // -d <n>: modify suggested dB gain (mp3gain compatible)
    pub channel_gain: Option<(Channel, i32)>, // -l <channel> <gain>
    pub gain_modifier: i32,    // -m <i>: modify suggested gain by integer steps

    // Mode options
    pub undo: bool,                     // -u
    pub stored_tag_mode: StoredTagMode, // -s <mode>
    pub use_id3v2: bool,                // -s i: use ID3v2 tags instead of APEv2
    pub track_gain: bool,               // -r (apply track gain)
    pub album_gain: bool,               // -a (apply album gain)
    pub skip_album: bool,               // -e: skip album analysis
    pub max_amplitude_only: bool,       // -x: only find max amplitude
    pub track_index: Option<u32>,       // -i <index>: track index for multi-track files

    // Behavior options
    pub preserve_timestamp: bool,    // -p
    pub ignore_clipping: bool,       // -c
    pub prevent_clipping: bool,      // -k
    pub quiet: bool,                 // -q
    pub recursive: bool,             // -R
    pub dry_run: bool,               // -n or --dry-run
    pub output_format: OutputFormat, // -o <format>
    pub wrap_gain: bool,             // -w: wrap gain values
    pub use_temp_file: bool,         // -t: use temp file for writing
    pub assume_mpeg2: bool,          // -f: assume MPEG 2 Layer III
    pub skip_errors: bool,           // --skip-errors: skip files that fail to analyze

    // Parallelism
    // -j N / --threads N. None = auto (available_parallelism). Some(0) = auto. Some(1) = serial.
    pub threads: Option<usize>,

    // Files
    pub files: Vec<PathBuf>,
}

impl Options {
    pub fn dry_run_prefix(&self) -> &'static str {
        if self.dry_run {
            "[DRY RUN] "
        } else {
            ""
        }
    }
}
