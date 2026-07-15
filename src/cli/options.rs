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
    Check,  // -s c: Check/show stored tag info
    Delete, // -s d: Delete stored tag info
    Skip,   // -s s: Skip (don't write) stored tag info
}

#[derive(Default)]
pub struct Options {
    // Gain options
    pub gain_steps: Option<i32>,              // -g <i>
    pub gain_modifier_db: f64, // -d <n>: modify suggested dB gain (mp3gain compatible)
    pub channel_gain: Option<(Channel, i32)>, // -l <channel> <gain>
    pub gain_modifier: i32,    // -m <i>: modify suggested gain by integer steps

    // Mode options
    pub undo: bool, // -u
    // -s <mode>. Orthogonal to `use_id3v2`: the mode says *what* to do with
    // stored tags (check / delete / skip writing), while `use_id3v2` says
    // *where* they live (-s i: ID3v2 frames, -s a: APEv2, the default).
    pub stored_tag_mode: StoredTagMode,
    pub use_id3v2: bool,    // -s i: use ID3v2 tags instead of APEv2 (-s a resets)
    pub force_recalc: bool, // -s r: accepted for compatibility (always recalculates)
    pub track_gain: bool,   // -r (apply track gain)
    pub album_gain: bool,   // -a (apply album gain)
    pub skip_album: bool,   // -e: skip album analysis
    pub max_amplitude_only: bool, // -x: only find max amplitude
    pub track_index: Option<u32>, // -i <index>: track index for multi-track files

    // Behavior options
    pub preserve_timestamp: bool,    // -p
    pub ignore_clipping: bool,       // -c
    pub prevent_clipping: bool,      // -k
    pub quiet: bool,                 // -q
    pub recursive: bool,             // -R
    pub dry_run: bool,               // -n or --dry-run
    pub output_format: OutputFormat, // -o <format>
    pub wrap_gain: bool,             // -w: wrap gain values
    pub use_temp_file: bool,         // -t: accepted for compatibility (writes are always atomic)
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

    /// Combined `-m` (steps) and `-d` (dB) modifier expressed as mp3 gain steps.
    /// `-d` is rounded to the nearest 1.5 dB step; sub-step values silently
    /// round to zero, matching mp3gain's quantized step model.
    pub fn gain_modifier_steps(&self) -> i32 {
        self.gain_modifier + mp3rgain::db_to_steps(self.gain_modifier_db)
    }
}
