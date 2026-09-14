//! Custom error types for mp3rgain.

use std::path::{Path, PathBuf};

/// `Result` with this crate's [`Error`] as the error type.
pub type Result<T> = std::result::Result<T, Error>;

/// All errors that can occur in mp3rgain operations.
///
/// `#[non_exhaustive]`, so match with a `_` arm. Two classifiers are worth
/// knowing before matching on variants directly:
/// [`is_unsupported_format`](Self::is_unsupported_format) separates "this
/// format is not adjustable" from a genuine failure, and
/// [`refine_format`](Self::refine_format) is what turns the former into
/// [`UnsupportedFormat`](Self::UnsupportedFormat).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    // I/O
    /// Reading a file failed. Built by [`Error::io_read`].
    #[error("Failed to read '{path}': {source}")]
    IoRead {
        /// The file that could not be read.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// Writing a file failed. Built by [`Error::io_write`].
    ///
    /// Writes go to a temp file that is renamed over the original, so this
    /// generally means the original is still intact.
    #[error("Failed to write '{path}': {source}")]
    IoWrite {
        /// The file that could not be written.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    /// Opening a file failed. Built by [`Error::io_open`].
    #[error("Failed to open '{path}': {source}")]
    IoOpen {
        /// The file that could not be opened.
        path: PathBuf,
        /// The underlying I/O failure.
        #[source]
        source: std::io::Error,
    },

    // MP3
    /// The MP3 frame scanner found no frame it would accept.
    ///
    /// Either the file is not an MP3, or it is too damaged to parse. A file
    /// that is a container mp3rgain recognises but cannot adjust reaches
    /// [`UnsupportedFormat`](Self::UnsupportedFormat) instead once
    /// [`refine_format`](Self::refine_format) has seen it.
    #[error("No valid MP3 frames found")]
    NoMp3Frames,

    /// Per-channel gain (`-l`) was requested for a mono stream, which has only
    /// one `global_gain` per granule to move.
    #[error("Cannot apply channel-specific gain to mono file. Use -g for mono files.")]
    ChannelGainOnMono,

    /// Per-channel gain (`-l`) was requested for an AAC bitstream, in MP4 or as
    /// a raw ADTS stream. The AAC path adjusts every channel element together
    /// and has no per-channel form.
    #[error("Channel-specific gain is not supported for AAC/M4A files")]
    ChannelGainOnAac,

    /// Undo was requested for an MP3 with no APEv2 tag at all, so there is
    /// nothing recording what to roll back.
    #[error("No APE tag found - cannot undo")]
    NoApeTag,

    /// The tag exists but carries no `MP3GAIN_UNDO` item, so no gain this tool
    /// applied is recorded in it.
    #[error("No MP3GAIN_UNDO tag found - cannot undo")]
    NoUndoTag,

    // ReplayGain / decoder
    /// The container was parsed but declares no audio track, or the track it
    /// declares carries no decodable codec parameters.
    #[error("No audio track found")]
    NoAudioTrack,

    /// `-i N` named a track the file does not have.
    #[error("Track index {index} out of range (file has {count} audio track(s))")]
    TrackIndexOutOfRange {
        /// The track index that was asked for.
        index: u32,
        /// How many audio tracks the file actually has.
        count: usize,
    },

    /// The stream's sample rate has no equal-loudness filter coefficients.
    ///
    /// ReplayGain 1.0 is defined for the twelve rates the reference
    /// implementation tabulates (8 kHz through 96 kHz). `0` means the
    /// container declared no rate at all.
    #[error("Unsupported sample rate: {0} Hz")]
    UnsupportedSampleRate(u32),

    /// The operation needs a Cargo feature this build does not have.
    #[error("The '{feature}' feature is not available. Rebuild with --features {feature_flag}.")]
    FeatureNotAvailable {
        /// Human-readable name of the capability, e.g. `"ReplayGain analysis"`.
        feature: &'static str,
        /// The Cargo feature flag to rebuild with, e.g. `"replaygain"`.
        feature_flag: &'static str,
    },

    /// symphonia could not identify the container.
    #[error("Failed to probe audio format in '{path}': {source}")]
    ProbeFailed {
        /// The file that could not be probed.
        path: PathBuf,
        /// symphonia's probe error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// Decoding the audio failed in a way the analysis could not skip past.
    ///
    /// Individual malformed packets are skipped rather than surfaced, so this
    /// means the stream stopped being decodable.
    #[error("Audio decode error: {0}")]
    Decode(#[source] Box<dyn std::error::Error + Send + Sync>),

    // MP4 / AAC
    /// An MP4 file carries no `moov` box, so it has no sample table to locate
    /// the AAC frames with and no place to store metadata.
    #[error("No moov box found in MP4 file")]
    NoMoovBox,

    /// An MP4-only operation was given a file whose `ftyp` brand mp3rgain does
    /// not accept.
    #[error("Not an MP4 file: {path}")]
    NotMp4File {
        /// The file that is not an accepted MP4.
        path: PathBuf,
    },

    /// The AAC bitstream parser rejected a structure it cannot safely rewrite.
    ///
    /// mp3rgain only ever writes into `global_gain` fields it has positively
    /// located, so anything that makes a location uncertain stops the parse
    /// rather than guessing.
    #[error("AAC bitstream parse error: {message}")]
    AacParse {
        /// What the parser objected to.
        message: String,
    },

    /// The MP4 has a `moov` box but no `mp4a` track inside it.
    #[error("No AAC audio track found")]
    NoAacTrack,

    // Format support
    /// A container mp3rgain recognises but cannot adjust the gain of: ALAC, or
    /// DRM-protected M4P.
    ///
    /// This is a *skip*, not a failure. Frontends report it without setting the
    /// exit code, so one such file in a library does not make a whole scan look
    /// failed. See [`is_unsupported_format`](Self::is_unsupported_format).
    #[error("{format} is not supported for gain adjustment")]
    UnsupportedFormat {
        /// Name of the format, for the user-facing message.
        format: &'static str,
    },

    /// Every AAC sample in the file failed to parse, so no `global_gain` field
    /// could be located anywhere.
    #[error("Failed to parse any AAC samples ({warnings} errors)")]
    AacParseFailure {
        /// How many samples failed.
        warnings: u32,
    },

    /// Album analysis ran and no member could be analyzed.
    ///
    /// A partial failure is not this: with `skip_errors` the survivors form the
    /// album and the failures are reported per file.
    #[error("All {count} file(s) failed to analyze")]
    AllFilesFailed {
        /// How many files were in the album.
        count: usize,
    },

    /// The caller's cancellation flag was observed at a file boundary. Files
    /// already being decoded run to completion.
    #[error("Operation cancelled")]
    Cancelled,

    // ID3v2
    /// Reading or writing the ID3v2 tag failed.
    #[error("ID3v2 tag error: {message}")]
    Id3v2Error {
        /// What the `id3` crate reported.
        message: String,
    },

    /// Undo was requested for a file whose ID3v2 tag carries no `MP3GAIN_UNDO`
    /// TXXX frame. The ID3v2 counterpart of [`NoUndoTag`](Self::NoUndoTag).
    #[error("No ID3v2 undo tag found - cannot undo")]
    NoId3v2UndoTag,
}

impl Error {
    /// Re-label a "cannot read this file's audio" failure as
    /// [`Self::UnsupportedFormat`] when `path` turns out to be a container
    /// mp3rgain recognizes but cannot process — ALAC or DRM-protected M4P
    /// (issue #330).
    ///
    /// Without this, an ALAC file fails analysis with symphonia's
    /// "unsupported audio codec" and the MP3 apply path reports
    /// [`Self::NoMp3Frames`] for a file that was never an MP3, both of which
    /// read as genuine failures. Costs a 128-byte header read, and only on the
    /// failure path.
    pub fn refine_format(self, path: &Path) -> Self {
        if !matches!(
            self,
            Self::NoMp3Frames | Self::Decode(_) | Self::ProbeFailed { .. }
        ) {
            return self;
        }
        match crate::mp4meta::unsupported_audio_format(path) {
            Some(format) => Self::UnsupportedFormat { format },
            None => self,
        }
    }

    /// True for [`Self::UnsupportedFormat`]: the file's *format* is the
    /// problem, not the file or the run, so frontends report it as a skipped
    /// file rather than a failure that sets the exit code.
    pub fn is_unsupported_format(&self) -> bool {
        matches!(self, Self::UnsupportedFormat { .. })
    }

    /// Build an [`IoRead`](Self::IoRead) carrying the path that failed.
    pub fn io_read(path: &Path, source: std::io::Error) -> Self {
        Self::IoRead {
            path: path.to_path_buf(),
            source,
        }
    }

    /// Build an [`IoWrite`](Self::IoWrite) carrying the path that failed.
    pub fn io_write(path: &Path, source: std::io::Error) -> Self {
        Self::IoWrite {
            path: path.to_path_buf(),
            source,
        }
    }

    /// Build an [`IoOpen`](Self::IoOpen) carrying the path that failed.
    pub fn io_open(path: &Path, source: std::io::Error) -> Self {
        Self::IoOpen {
            path: path.to_path_buf(),
            source,
        }
    }
}
