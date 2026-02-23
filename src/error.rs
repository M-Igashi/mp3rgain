//! Custom error types for mp3rgain.

use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

/// All errors that can occur in mp3rgain operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    // -------------------------------------------------------------------------
    // I/O errors
    // -------------------------------------------------------------------------
    #[error("Failed to read '{path}': {source}")]
    IoRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to write '{path}': {source}")]
    IoWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to open '{path}': {source}")]
    IoOpen {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    // -------------------------------------------------------------------------
    // MP3 errors
    // -------------------------------------------------------------------------
    #[error("No valid MP3 frames found")]
    NoMp3Frames,

    #[error("Cannot apply channel-specific gain to mono file. Use -g for mono files.")]
    ChannelGainOnMono,

    #[error("No APE tag found - cannot undo")]
    NoApeTag,

    #[error("No MP3GAIN_UNDO tag found - cannot undo")]
    NoUndoTag,

    // -------------------------------------------------------------------------
    // ReplayGain / decoder errors
    // -------------------------------------------------------------------------
    #[error("No audio track found")]
    NoAudioTrack,

    #[error("Track index {index} out of range (file has {count} audio track(s))")]
    TrackIndexOutOfRange { index: u32, count: usize },

    #[error("Unsupported sample rate: {0} Hz")]
    UnsupportedSampleRate(u32),

    #[error("The '{feature}' feature is not available. Rebuild with --features {feature_flag}.")]
    FeatureNotAvailable {
        feature: &'static str,
        feature_flag: &'static str,
    },

    #[error("Failed to probe audio format in '{path}': {source}")]
    ProbeFailed {
        path: PathBuf,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("Audio decode error: {0}")]
    Decode(#[source] Box<dyn std::error::Error + Send + Sync>),

    // -------------------------------------------------------------------------
    // MP4 / AAC errors
    // -------------------------------------------------------------------------
    #[error("No moov box found in MP4 file")]
    NoMoovBox,

    #[error("Not an MP4 file: {path}")]
    NotMp4File { path: PathBuf },

    #[error("AAC bitstream parse error: {message}")]
    AacParse { message: String },

    #[error("No AAC audio track found")]
    NoAacTrack,

    #[error("Failed to parse any AAC samples ({warnings} errors)")]
    AacParseFailure { warnings: u32 },
}

// -----------------------------------------------------------------------------
// Convenience constructors for I/O errors
// -----------------------------------------------------------------------------

pub fn io_read(path: &std::path::Path, source: std::io::Error) -> Error {
    Error::IoRead {
        path: path.to_path_buf(),
        source,
    }
}

pub fn io_write(path: &std::path::Path, source: std::io::Error) -> Error {
    Error::IoWrite {
        path: path.to_path_buf(),
        source,
    }
}

pub fn io_open(path: &std::path::Path, source: std::io::Error) -> Error {
    Error::IoOpen {
        path: path.to_path_buf(),
        source,
    }
}
