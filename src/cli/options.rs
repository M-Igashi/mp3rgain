use mp3rgain::replaygain::AnalysisMode;
use mp3rgain::{Channel, TagLayout};
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
    // -s <mode>. Orthogonal to `tag_layout`: the mode says *what* to do with
    // stored tags (check / delete / skip writing), while `tag_layout` says
    // *where* they live (-s i: all ID3v2, -s a: all APEv2, default: split).
    pub stored_tag_mode: StoredTagMode,
    pub tag_layout: TagLayout, // -s i / -s a
    pub force_recalc: bool,    // -s r: accepted for compatibility (recalculation is the default)
    // -s R: reuse stored REPLAYGAIN_* tags instead of re-analyzing, rescanning
    // only files whose tags are missing (mp3gain's default behavior, issue #298).
    pub use_stored_tags: bool,
    pub track_gain: bool, // -r (apply track gain)
    pub album_gain: bool, // -a (apply album gain)
    // --rg2 / --r128: opt-in BS.1770 loudness modes (issue #269).
    // Default Rg1 keeps mp3gain-identical values.
    pub analysis_mode: AnalysisMode,
    // --true-peak: BS.1770-4 Annex 2 true peak for REPLAYGAIN_*_PEAK in the
    // BS.1770 modes (issue #292). Requires --rg2 or --r128.
    pub true_peak: bool,
    // --tags-only: analyze as usual but write absolute REPLAYGAIN_* values
    // without modifying a single audio frame (issue #308), the loudgain /
    // rsgain workflow. Requires -r / -a / -e.
    pub tags_only: bool,
    pub skip_album: bool,         // -e: skip album analysis
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

    /// Whether `-s R` may reuse stored tags at all. Stored values can't be
    /// trusted when `-s r` forces recalculation, a `-d`/`-m` modifier shifts
    /// the target, or a BS.1770 mode is selected (`REPLAYGAIN_ALGORITHM`
    /// can't distinguish the RG2 and R128 targets), so those force a rescan.
    pub fn stored_tags_usable(&self) -> bool {
        self.use_stored_tags
            && !self.force_recalc
            && self.analysis_mode == AnalysisMode::Rg1
            && self.target_offset_db() == 0.0
    }

    /// A measured gain shifted by the `-m` / `-d` modifiers, as the apply and
    /// info paths report it: `(steps, dB)`, both quantized to whole
    /// `global_gain` steps so the two columns always agree. Every caller used
    /// to spell this out, and one carried a comment asking to be kept in sync
    /// with the others.
    pub fn modified_gain(&self, base_steps: i32, base_db: f64) -> (i32, f64) {
        let modifier_steps = self.gain_modifier_steps();
        (
            base_steps + modifier_steps,
            base_db + mp3rgain::steps_to_db(modifier_steps),
        )
    }

    /// Combined `-m` (steps) and `-d` (dB) modifier expressed as mp3 gain steps.
    /// `-d` is rounded to the nearest 1.5 dB step; sub-step values silently
    /// round to zero, matching mp3gain's quantized step model.
    pub fn gain_modifier_steps(&self) -> i32 {
        self.gain_modifier + mp3rgain::db_to_steps(self.gain_modifier_db)
    }

    /// How far `-d` / `-m` shift the target, in dB.
    ///
    /// Normally this is [`Self::gain_modifier_steps`] converted back to dB,
    /// because the shift has to land on a whole `global_gain` step. In
    /// `--tags-only` mode nothing is quantized, since the shift only moves a
    /// float in a tag, so `-d` applies exactly and `-m` contributes its steps
    /// (issue #308).
    pub fn target_offset_db(&self) -> f64 {
        if self.tags_only {
            self.gain_modifier_db + mp3rgain::steps_to_db(self.gain_modifier)
        } else {
            mp3rgain::steps_to_db(self.gain_modifier_steps())
        }
    }
}
