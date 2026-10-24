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

/// `--album-by`: what counts as one album under `-a` (issues #324, #331, #333).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AlbumGrouping {
    /// `-a` alone: every file given is pooled into one album, which is what
    /// mp3gain's `-a` does. Unchanged.
    #[default]
    Pooled,
    /// `--album-by=dir` (and its alias `--per-directory`): one album per
    /// parent directory. Works on untagged libraries, and splits a release
    /// whose discs live in subdirectories.
    Dir,
    /// `--album-by=tag`: one album per release, taken from the tags, so discs
    /// in subdirectories and trees of mixed depth both come out right.
    Tag,
    /// `--album-depth N`: one album per directory N levels below each root
    /// argument. The grouping unit for a library that is not tagged: it needs
    /// no shell expansion, has no command-line length ceiling, and behaves the
    /// same on Windows, where `*/*` is not expanded into argv at all.
    Depth(usize),
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
    pub skip_album: bool, // -e: skip album analysis
    // --album-by / --album-depth: with -a, what counts as one album
    // (issues #324, #331, #333). `--per-directory` aliases `--album-by=dir`.
    pub album_by: AlbumGrouping,
    // Whether a long track may be divided across workers (issue #337). Set
    // once per run from the file count against the thread count: a run that
    // already has a file per thread gains nothing from dividing and pays for
    // the warm-up regions and the seeks.
    pub chunk_tracks: bool,
    // The path arguments as typed, before -R expanded them. `--album-depth`
    // counts levels from these, and `expand_files_recursive` is where the
    // provenance would otherwise be lost.
    pub arg_roots: Vec<PathBuf>,
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
    /// Whether human-readable progress and results go to the terminal:
    /// `-o text` (the default) without `-q`.
    pub fn text_output(&self) -> bool {
        self.output_format == OutputFormat::Text && !self.quiet
    }

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
    /// The tags also describe the first audio track only, so they are not a
    /// result for `-i <n>` with n other than 0 (issue #375).
    pub fn stored_tags_usable(&self) -> bool {
        self.use_stored_tags
            && !self.force_recalc
            && self.analysis_mode == AnalysisMode::Rg1
            && self.target_offset_db() == 0.0
            && self.track_index.unwrap_or(0) == 0
    }

    /// The TSV `Max Amplitude` column for a normalized peak. RG1 keeps
    /// mp3gain's 16-bit sample scale (`peak * 32768`) so scripts written for
    /// mp3gain keep working. The BS.1770 modes are outside mp3gain's world
    /// anyway (and a true peak above 1.0 makes no sense as a sample value),
    /// so they print the ReplayGain float peak: the number written to
    /// `REPLAYGAIN_*_PEAK` and reported by `-o json` (issue #323).
    pub fn tsv_peak(&self, peak: f64) -> f64 {
        if self.analysis_mode == AnalysisMode::Rg1 {
            mp3rgain::peak_to_pcm_sample(peak)
        } else {
            peak
        }
    }

    /// The gain steps an apply makes for a measured `gain_db`, as mp3gain
    /// computes them: `-d` is added in dB before the one rounding to whole
    /// steps, and `-m` is added after it (issue #416). Rounding `-d` on its
    /// own and adding the two step counts could land one step off, as an
    /// analysis of +0.80 dB with `-d 0.80` did (2 steps against mp3gain's 1).
    pub fn modified_steps(&self, gain_db: f64) -> i32 {
        mp3rgain::db_to_steps(gain_db + self.gain_modifier_db) + self.gain_modifier
    }

    /// A measured gain shifted by the `-m` / `-d` modifiers, as the
    /// mp3gain-compatible analysis output reports it: `(steps, dB)`. The
    /// steps are [`Self::modified_steps`]; the dB is mp3gain's printed value,
    /// the measured gain plus `-d`, which leaves `-m` out.
    pub fn modified_gain(&self, gain_db: f64) -> (i32, f64) {
        (
            self.modified_steps(gain_db),
            gain_db + self.gain_modifier_db,
        )
    }

    /// How far `-d` / `-m` shift the target, in dB: `-d` exactly and `-m` by
    /// whole steps. An apply rounds the shifted gain to whole steps once
    /// ([`Self::modified_steps`]); `--tags-only` writes it as it is, since a
    /// tag holds a float (issue #308).
    pub fn target_offset_db(&self) -> f64 {
        self.gain_modifier_db + mp3rgain::steps_to_db(self.gain_modifier)
    }
}
