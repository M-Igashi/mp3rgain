//! Average bitrate and bitrate mode of an audio file, for display (issue #408).

use crate::frame::{frame_at, iterate_frames, skip_id3v2};
use crate::mp4meta;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Whether every frame of a stream has the same bitrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitrateMode {
    /// Every frame has the same bitrate (CBR).
    Constant,
    /// The bitrate changes from frame to frame (VBR or ABR).
    Variable,
}

/// A file's average bitrate, from [`read_bitrate`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bitrate {
    /// Average bitrate in kbps.
    pub kbps: f64,
    /// `None` for MP4: AAC frames differ in size whatever the encoder's rate
    /// control was set to, so the stream cannot tell CBR from VBR.
    pub mode: Option<BitrateMode>,
}

/// How much of an MP3 to read after its ID3v2 tag when looking for the first
/// frame. The largest Layer III frame is 1441 bytes, so this leaves room for
/// junk before it.
const HEAD_LEN: u64 = 16 * 1024;

/// Average bitrate of an MP3 or MP4 file, or `None` when it cannot be read
/// (raw ADTS, an unreadable file, no frames).
///
/// An MP3 with a Xing, Info or VBRI header in its first frame is judged by
/// that header, which needs only the start of the file: LAME writes `Info`
/// for CBR, while `Xing` and `VBRI` mark VBR and carry the frame and byte
/// counts the average comes from. A file without one has every frame read,
/// which costs a full read. An MP4 reads only `moov`.
pub fn read_bitrate(file_path: &Path) -> Option<Bitrate> {
    if mp4meta::is_mp4_file(file_path) {
        return mp4meta::audio_bitrate_kbps(file_path).map(|kbps| Bitrate { kbps, mode: None });
    }
    #[cfg(feature = "aac")]
    if crate::adts::is_adts_file(file_path) {
        return None;
    }

    let mut file = fs::File::open(file_path).ok()?;
    let mut id3 = [0u8; 10];
    file.read_exact(&mut id3).ok()?;
    file.seek(SeekFrom::Start(skip_id3v2(&id3) as u64)).ok()?;
    let mut head = Vec::new();
    file.take(HEAD_LEN).read_to_end(&mut head).ok()?;
    header_bitrate(&head).or_else(|| scanned_bitrate(&fs::read(file_path).ok()?))
}

/// Bitrate from the VBR header in the first frame of `data`, which starts
/// where the audio does. `None` when there is no header, or a Xing header
/// lacks the frame or byte count.
fn header_bitrate(data: &[u8]) -> Option<Bitrate> {
    let (pos, header) = (0..data.len().saturating_sub(3))
        .find_map(|pos| frame_at(data, pos, data.len()).map(|header| (pos, header)))?;
    let field = |offset: usize| {
        let b = data.get(pos + offset..pos + offset + 4)?;
        Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };

    let xing = 4 + header.side_info_len();
    let (frames, bytes) = match data.get(pos + xing..pos + xing + 4)? {
        b"Info" => {
            return Some(Bitrate {
                kbps: header.bitrate_kbps as f64,
                mode: Some(BitrateMode::Constant),
            })
        }
        // Flag bits 0 and 1 mark the frame and byte counts, which follow the
        // flags in that order.
        b"Xing" if field(xing + 4)? & 3 == 3 => (field(xing + 8)?, field(xing + 12)?),
        // VBRI sits 32 bytes after the header whatever the side info length,
        // followed by version, delay and quality (2 bytes each), the byte
        // count and the frame count.
        _ if data.get(pos + 36..pos + 40) == Some(&b"VBRI"[..]) => (field(50)?, field(46)?),
        _ => return None,
    };
    if frames == 0 || bytes == 0 {
        return None;
    }
    let seconds = frames as f64 * header.samples_per_frame() as f64 / header.sample_rate as f64;
    Some(Bitrate {
        kbps: bytes as f64 * 8.0 / seconds / 1000.0,
        mode: Some(BitrateMode::Variable),
    })
}

/// Bitrate from every frame of `data`. `iterate_frames` keeps only frames
/// matching the first one's version and sample rate, so all have the same
/// duration and the mean of their bitrates is the average over time.
fn scanned_bitrate(data: &[u8]) -> Option<Bitrate> {
    let mut first = None;
    let mut constant = true;
    let mut total = 0u64;
    let frames = iterate_frames(data, |_, header, _| {
        constant &= *first.get_or_insert(header.bitrate_kbps) == header.bitrate_kbps;
        total += header.bitrate_kbps as u64;
    })
    .ok()?;
    Some(Bitrate {
        kbps: total as f64 / frames as f64,
        mode: Some(if constant {
            BitrateMode::Constant
        } else {
            BitrateMode::Variable
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// A silent MPEG-1 stereo 44.1 kHz frame at bitrate index `index`.
    fn frame(index: u8) -> Vec<u8> {
        let kbps = [
            0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ][index as usize];
        let mut frame = vec![0u8; 1152 * kbps * 125 / 44100];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, index << 4, 0x00]);
        frame
    }

    #[test]
    fn info_header_reads_as_cbr() {
        let bitrate = read_bitrate(&fixture("test_stereo.mp3")).unwrap();
        assert_eq!(bitrate.kbps, 128.0);
        assert_eq!(bitrate.mode, Some(BitrateMode::Constant));
    }

    /// ffprobe reports 57759 b/s for this file, from the same Xing counts.
    #[test]
    fn xing_header_reads_as_vbr() {
        let bitrate = read_bitrate(&fixture("test_vbr.mp3")).unwrap();
        assert_eq!(bitrate.kbps.round(), 58.0);
        assert_eq!(bitrate.mode, Some(BitrateMode::Variable));
    }

    #[test]
    fn headerless_streams_are_scanned() {
        let cbr: Vec<u8> = (0..4).flat_map(|_| frame(9)).collect();
        assert_eq!(header_bitrate(&cbr), None);
        assert_eq!(
            scanned_bitrate(&cbr),
            Some(Bitrate {
                kbps: 128.0,
                mode: Some(BitrateMode::Constant)
            })
        );

        let vbr: Vec<u8> = [9, 11, 11, 13].into_iter().flat_map(frame).collect();
        assert_eq!(
            scanned_bitrate(&vbr),
            Some(Bitrate {
                kbps: 192.0,
                mode: Some(BitrateMode::Variable)
            })
        );
    }

    /// No fixture carries a Fraunhofer VBRI header: 100 frames in 52245
    /// bytes is 160 kbps at 44.1 kHz.
    #[test]
    fn vbri_header_reads_as_vbr() {
        let mut data = frame(9);
        data[36..40].copy_from_slice(b"VBRI");
        data[46..50].copy_from_slice(&52245u32.to_be_bytes());
        data[50..54].copy_from_slice(&100u32.to_be_bytes());
        data.extend(frame(9));
        let bitrate = header_bitrate(&data).unwrap();
        assert_eq!(bitrate.kbps.round(), 160.0);
        assert_eq!(bitrate.mode, Some(BitrateMode::Variable));
    }

    /// ffprobe reports 65807 and 88400 b/s, from the same stsz and mdhd.
    #[test]
    fn mp4_reads_bitrate_without_mode() {
        for (name, kbps) in [("test_aac.m4a", 65.807), ("test_alac.m4a", 88.4)] {
            let bitrate = read_bitrate(&fixture(name)).unwrap();
            assert!((bitrate.kbps - kbps).abs() < 0.001, "{name}: {bitrate:?}");
            assert_eq!(bitrate.mode, None);
        }
    }

    #[cfg(feature = "aac")]
    #[test]
    fn adts_has_no_bitrate() {
        assert_eq!(read_bitrate(&fixture("test_adts.aac")), None);
    }
}
