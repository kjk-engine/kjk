//! The games' movies as kjk plays them (`docs/SOUND.md`, "Movies").
//! KOTOR's are Bink; nothing here decodes Bink. `ffmpeg` turns each one,
//! once, into a movie of our own (`ffmpeg::Ffmpeg::transcode`), kept in the
//! store: its pictures as JPEGs, one a frame, and its sound as one MP3 (or
//! FLAC where `ffmpeg` has no MP3 encoder), which the mixer plays.
//!
//! The file: `KJKMOVIE`, the picture's width and height, the frame rate
//! as a fraction, the frame count, the sound's coding (0 none, 1 MP3, 2
//! FLAC), offset and length, then each frame's offset and length, then the
//! bytes. Numbers are little-endian `u32`s, offsets and lengths `u64`s.

pub mod ffmpeg;

use std::ops::Range;

const MAGIC: &[u8; 8] = b"KJKMOVIE";

/// How a movie's sound is coded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coding {
    Mp3,
    Flac,
}

impl Coding {
    fn code(coding: Option<Coding>) -> u32 {
        match coding {
            None => 0,
            Some(Coding::Mp3) => 1,
            Some(Coding::Flac) => 2,
        }
    }

    fn from_code(code: u32) -> Result<Option<Coding>, String> {
        match code {
            0 => Ok(None),
            1 => Ok(Some(Coding::Mp3)),
            2 => Ok(Some(Coding::Flac)),
            _ => Err(format!("sound coding {code}")),
        }
    }
}

/// A movie: JPEG frames at a steady rate, and its sound.
pub struct Movie {
    bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// Frames a second, as a fraction (Bink's 29.97 is 2997/100).
    pub rate: (u32, u32),
    frames: Vec<Range<usize>>,
    sound: Option<(Coding, Range<usize>)>,
}

impl Movie {
    /// The file of a movie of `width` × `height` at `rate` frames a second
    /// made of `frames` (JPEGs) and `sound`.
    pub fn encode(
        (width, height): (u32, u32),
        rate: (u32, u32),
        frames: &[&[u8]],
        sound: Option<(Coding, &[u8])>,
    ) -> Vec<u8> {
        let header = MAGIC.len() + 4 * 6 + 16 + frames.len() * 16;
        let mut out = Vec::with_capacity(
            header + frames.iter().map(|f| f.len()).sum::<usize>() + sound.map_or(0, |s| s.1.len()),
        );
        out.extend_from_slice(MAGIC);
        for value in [width, height, rate.0, rate.1, frames.len() as u32] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        let mut at = header as u64;
        out.extend_from_slice(&Coding::code(sound.map(|s| s.0)).to_le_bytes());
        let sound_len = sound.map_or(0, |s| s.1.len() as u64);
        for value in [at, sound_len] {
            out.extend_from_slice(&value.to_le_bytes());
        }
        at += sound_len;
        for frame in frames {
            for value in [at, frame.len() as u64] {
                out.extend_from_slice(&value.to_le_bytes());
            }
            at += frame.len() as u64;
        }
        if let Some((_, sound)) = sound {
            out.extend_from_slice(sound);
        }
        for frame in frames {
            out.extend_from_slice(frame);
        }
        out
    }

    pub fn decode(bytes: Vec<u8>) -> Result<Movie, String> {
        let mut at = MAGIC.len();
        if bytes.get(..at) != Some(&MAGIC[..]) {
            return Err("not a movie".into());
        }
        let u32_at = |at: &mut usize| -> Result<u32, String> {
            let word = bytes.get(*at..*at + 4).ok_or("cut short")?;
            *at += 4;
            Ok(u32::from_le_bytes(word.try_into().expect("four bytes")))
        };
        let [width, height, num, den, count] = [(); 5].map(|_| u32_at(&mut at));
        let (width, height, rate, count) = (width?, height?, (num?, den?), count?);
        let coding = Coding::from_code(u32_at(&mut at)?)?;
        let range = |at: &mut usize| -> Result<Range<usize>, String> {
            let pair = bytes.get(*at..*at + 16).ok_or("cut short")?;
            *at += 16;
            let start = u64::from_le_bytes(pair[..8].try_into().expect("eight bytes")) as usize;
            let len = u64::from_le_bytes(pair[8..].try_into().expect("eight bytes")) as usize;
            let end = start.checked_add(len).filter(|end| *end <= bytes.len());
            end.map(|end| start..end)
                .ok_or_else(|| "a part past the end".into())
        };
        let sound_range = range(&mut at)?;
        let frames = (0..count)
            .map(|_| range(&mut at))
            .collect::<Result<Vec<_>, _>>()?;
        if rate.0 == 0 || rate.1 == 0 {
            return Err("no frame rate".into());
        }
        Ok(Movie {
            width,
            height,
            rate,
            frames,
            sound: coding.map(|coding| (coding, sound_range)),
            bytes,
        })
    }

    pub fn frames(&self) -> usize {
        self.frames.len()
    }

    /// Frame `n`'s JPEG.
    pub fn frame(&self, n: usize) -> Option<&[u8]> {
        self.frames.get(n).map(|range| &self.bytes[range.clone()])
    }

    /// The frame shown `seconds` in (the last one past the end).
    pub fn frame_at(&self, seconds: f64) -> usize {
        let n = (seconds.max(0.0) * f64::from(self.rate.0) / f64::from(self.rate.1)) as usize;
        n.min(self.frames.len().saturating_sub(1))
    }

    /// How long its pictures run.
    pub fn seconds(&self) -> f64 {
        self.frames.len() as f64 * f64::from(self.rate.1) / f64::from(self.rate.0)
    }

    pub fn sound(&self) -> Option<(Coding, &[u8])> {
        self.sound
            .as_ref()
            .map(|(coding, range)| (*coding, &self.bytes[range.clone()]))
    }
}

/// A frame's picture: width, height and RGBA pixels.
pub fn picture(jpeg: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let image = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
        .map_err(|error| error.to_string())?
        .to_rgba8();
    Ok((image.width(), image.height(), image.into_raw()))
}

/// The JPEGs of a stream of them one after another (`ffmpeg`'s
/// `image2pipe`), each from its start marker to its end marker. A JPEG's
/// segments say their lengths; after the start of scan its coded data runs
/// to the next marker that is neither a stuffed `FF 00` nor a restart.
pub fn split_jpegs(stream: &[u8]) -> Result<Vec<&[u8]>, String> {
    let mut frames = Vec::new();
    let mut at = 0;
    let byte = |at: usize| {
        stream
            .get(at)
            .copied()
            .ok_or_else(|| format!("a JPEG cut short at byte {at}"))
    };
    while at < stream.len() {
        if byte(at)? != 0xff || byte(at + 1)? != 0xd8 {
            return Err(format!("no JPEG starts at byte {at}"));
        }
        let start = at;
        at += 2;
        loop {
            if byte(at)? != 0xff {
                return Err(format!("no marker at byte {at}"));
            }
            match byte(at + 1)? {
                // Fill before a marker.
                0xff => at += 1,
                0xd9 => {
                    at += 2;
                    break;
                }
                0x01 | 0xd0..=0xd7 => at += 2,
                marker => {
                    let len = usize::from(u16::from_be_bytes([byte(at + 2)?, byte(at + 3)?]));
                    at += 2 + len;
                    if marker == 0xda {
                        loop {
                            let next = stream[at.min(stream.len())..]
                                .iter()
                                .position(|b| *b == 0xff)
                                .map(|offset| at + offset)
                                .ok_or("a JPEG's scan runs off the end")?;
                            match byte(next + 1)? {
                                0x00 | 0xd0..=0xd7 => at = next + 2,
                                _ => {
                                    at = next;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
        frames.push(&stream[start..at]);
    }
    Ok(frames)
}
