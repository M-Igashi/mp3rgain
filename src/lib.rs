//! # mp3rgain
//!
//! Lossless MP3 volume adjustment library - a modern mp3gain replacement.
//!
//! This library provides lossless MP3 volume adjustment by modifying
//! the `global_gain` field in each frame's side information.
//!
//! ## Features
//!
//! - **Lossless**: No re-encoding, preserves audio quality
//! - **Fast**: Direct binary manipulation, no audio decoding
//! - **Compatible**: Works with all MP3 files (MPEG1/2/2.5 Layer III)
//! - **Reversible**: Changes can be undone by applying negative gain
//!
//! ## Optional Features
//!
//! - **replaygain**: Enable ReplayGain analysis (requires symphonia)
//!   - Track gain calculation (`-r` flag)
//!   - Album gain calculation (`-a` flag)
//!
//! ## Example
//!
//! ```no_run
//! use mp3rgain::{apply_gain, apply_gain_db, analyze, GainOptions, Channel};
//! use std::path::Path;
//!
//! // Simple gain adjustment: +2 steps (+3.0 dB)
//! let frames = apply_gain(Path::new("song.mp3"), 2).unwrap();
//! println!("Modified {} frames", frames);
//!
//! // Or specify gain in dB directly
//! let frames = apply_gain_db(Path::new("song.mp3"), 4.5).unwrap();
//!
//! // Builder pattern for advanced options
//! GainOptions::new(5)
//!     .wrap(true)
//!     .undo(true)
//!     .apply(Path::new("song.mp3")).unwrap();
//!
//! // Channel-specific gain with undo support
//! GainOptions::new(3)
//!     .channel(Channel::Left)
//!     .undo(true)
//!     .apply(Path::new("song.mp3")).unwrap();
//! ```
//!
//! ## Modules
//!
//! - [`analysis`] - MP3 file analysis and amplitude detection
//! - [`gain`] - Gain adjustment operations and the [`GainOptions`] builder
//! - [`ape`] - APEv2 tag reading, writing, and management
//! - [`replaygain`] - ReplayGain loudness analysis
//! - [`mp4meta`] - MP4/M4A metadata handling
//! - [`aac`] - AAC bitstream parsing (feature-gated)
//!
//! ## Technical Details
//!
//! Each gain step equals 1.5 dB (fixed by MP3 specification).
//! The global_gain field is 8 bits, allowing values 0-255.

#[cfg(feature = "aac")]
pub mod aac;
#[cfg(feature = "aac")]
mod aac_codebooks;

pub mod analysis;
pub mod ape;
pub mod error;
mod frame;
pub mod gain;
pub mod mp4meta;
pub mod replaygain;

// Re-export commonly used items at crate root for convenience
pub use analysis::{
    analyze, find_max_amplitude, is_mono, ChannelMode, MaxAmplitudeResult, Mp3Analysis, MpegVersion,
};
pub use ape::{
    delete_ape_tag, read_ape_tag, read_ape_tag_from_file, write_ape_tag, ApeItem, ApeTag,
    TAG_MP3GAIN_ALBUM_MINMAX, TAG_MP3GAIN_MINMAX, TAG_MP3GAIN_UNDO, TAG_REPLAYGAIN_ALBUM_GAIN,
    TAG_REPLAYGAIN_ALBUM_PEAK, TAG_REPLAYGAIN_TRACK_GAIN, TAG_REPLAYGAIN_TRACK_PEAK,
};
pub use error::{Error, Result};
pub use gain::{
    apply_gain, apply_gain_db, db_to_steps, steps_to_db, undo_gain, Channel, GainOptions,
    GAIN_STEP_DB, MAX_GAIN, MIN_GAIN,
};
