//! A decoder for Bink 1 movies, the format of KOTOR's and TSL's cutscenes.
//!
//! It reads revision "i" of Bink 1, which every movie of both games uses,
//! including the high-resolution movies that mods ship. Its video is YUV
//! 4:2:0 in 8×8 blocks (`video`), and its sound is Bink's DCT audio
//! (`audio`). A file of another kind (Bink 2, an older revision, a grey or
//! RDFT-audio movie) is refused with a reason, and kjk then falls back to
//! `ffmpeg`.
//!
//! This decoder is written for kjk from the format's description. Its
//! tables (`tables`) are the format's own numbers. `kjk movie compare`
//! compares its output with `ffmpeg`'s on the games' own movies.

mod audio;
mod bits;
mod tables;
mod video;

pub use audio::{Audio, Sound};
pub use video::Video;

use std::ops::Range;

/// A video flag: the movie has an alpha plane.
const FLAG_ALPHA: u32 = 0x0010_0000;
/// A video flag: the movie is grey, without colour planes.
const FLAG_GREY: u32 = 0x0002_0000;
/// An audio flag: the track is stereo.
const AUDIO_STEREO: u16 = 0x2000;
/// An audio flag: the track is coded with the DCT (not the RDFT).
const AUDIO_DCT: u16 = 0x1000;

/// An audio track's coding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Track {
    pub rate: u32,
    pub channels: usize,
    /// Whether it is DCT audio. RDFT audio is not decoded here.
    pub dct: bool,
}

/// A Bink 1 movie's header and the places of its frames.
pub struct Bink {
    bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The frame rate as a fraction.
    pub rate: (u32, u32),
    pub alpha: bool,
    pub tracks: Vec<Track>,
    frames: Vec<Range<usize>>,
}

/// One frame's packets: one for each audio track, then the video.
pub struct Packets<'a> {
    pub audio: Vec<&'a [u8]>,
    pub video: &'a [u8],
}

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, String> {
    bytes
        .get(at..at + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| "the file ends inside its header".to_string())
}

fn u16_at(bytes: &[u8], at: usize) -> Result<u16, String> {
    bytes
        .get(at..at + 2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .ok_or_else(|| "the file ends inside its header".to_string())
}

impl Bink {
    /// Reads a movie's header and frame index. Only Bink 1 revision "i" is
    /// read. Any other file gives an error that says what it is.
    pub fn parse(bytes: Vec<u8>) -> Result<Bink, String> {
        match bytes.get(..4) {
            Some(b"BIKi") => {}
            Some([b'B', b'I', b'K', revision]) => {
                return Err(format!(
                    "Bink 1 revision \"{}\" is not decoded here",
                    char::from(*revision)
                ));
            }
            Some([b'K', b'B', b'2', _]) => return Err("Bink 2 is not decoded here".into()),
            _ => return Err("not a Bink movie".into()),
        }
        let count = u32_at(&bytes, 8)? as usize;
        let width = u32_at(&bytes, 20)?;
        let height = u32_at(&bytes, 24)?;
        let rate = (u32_at(&bytes, 28)?, u32_at(&bytes, 32)?);
        let flags = u32_at(&bytes, 36)?;
        let track_count = u32_at(&bytes, 40)? as usize;
        if width == 0 || height == 0 || width > 7680 || height > 4320 {
            return Err(format!("a picture of {width}×{height}"));
        }
        if rate.0 == 0 || rate.1 == 0 {
            return Err("no frame rate".into());
        }
        if flags & FLAG_GREY != 0 {
            return Err("a grey movie is not decoded here".into());
        }
        if track_count > 256 {
            return Err(format!("{track_count} audio tracks"));
        }
        // After each track's largest decoded size come its rate and flags,
        // then every track's id.
        let tracks_at = 44 + 4 * track_count;
        let tracks = (0..track_count)
            .map(|i| {
                let rate = u16_at(&bytes, tracks_at + 4 * i)?;
                let flags = u16_at(&bytes, tracks_at + 4 * i + 2)?;
                Ok(Track {
                    rate: u32::from(rate),
                    channels: if flags & AUDIO_STEREO != 0 { 2 } else { 1 },
                    dct: flags & AUDIO_DCT != 0,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let index_at = tracks_at + 8 * track_count;
        let end = bytes.len();
        let starts = (0..count)
            .map(|i| Ok((u32_at(&bytes, index_at + 4 * i)? & !1) as usize))
            .collect::<Result<Vec<_>, String>>()?;
        let frames = starts
            .iter()
            .enumerate()
            .map(|(i, &start)| {
                let next = starts.get(i + 1).copied().unwrap_or(end).min(end);
                if start > next {
                    return Err(format!("frame {i} is out of order"));
                }
                Ok(start..next)
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Bink {
            bytes,
            width,
            height,
            rate,
            alpha: flags & FLAG_ALPHA != 0,
            tracks,
            frames,
        })
    }

    /// How many frames the movie has.
    pub fn frames(&self) -> usize {
        self.frames.len()
    }

    /// Frame `n`'s audio and video packets.
    pub fn packets(&self, n: usize) -> Result<Packets<'_>, String> {
        let range = self.frames.get(n).ok_or(format!("no frame {n}"))?;
        let mut data = &self.bytes[range.clone()];
        let mut audio = Vec::with_capacity(self.tracks.len());
        for _ in &self.tracks {
            let size = u32_at(data, 0)? as usize;
            let packet = data
                .get(4..4 + size)
                .ok_or(format!("frame {n}'s audio runs past it"))?;
            audio.push(packet);
            data = &data[4 + size..];
        }
        Ok(Packets { audio, video: data })
    }
}
