//! WAV as the games ship it: RIFF PCM or IMA ADPCM, or an MP3 stream
//! behind a RIFF header. KOTOR's voice files declare 8-bit PCM with an
//! empty `data` chunk and carry an MP3 (ID3 tag, then frames) after it;
//! some files have bytes before `RIFF` (its ambience a whole MP3 frame,
//! `FF F3 60 C4`, with the RIFF at 0x1D6).

use crate::FormatError;
use crate::reader::Reader;

const FORMAT: &str = "WAV";
/// How far into a file `RIFF` may start.
const MOST_LEADING: usize = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coding {
    Pcm {
        channels: u16,
        rate: u32,
        bits: u16,
    },
    Mp3,
    /// IMA ADPCM (format 0x11): 4 bits a sample in blocks of `block`
    /// bytes.
    Ima {
        channels: u16,
        rate: u32,
        block: u16,
    },
}

/// A sound's coding and its coded bytes, and the file from its `RIFF` on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wav<'a> {
    pub coding: Coding,
    pub data: &'a [u8],
    pub riff: &'a [u8],
}

impl<'a> Wav<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Wav<'a>, FormatError> {
        let start = bytes
            .windows(4)
            .take(MOST_LEADING)
            .position(|window| window == b"RIFF")
            .ok_or(FormatError::Unsupported {
                format: FORMAT,
                what: "no RIFF header".into(),
            })?;
        let bytes = &bytes[start..];
        let reader = Reader::new(FORMAT, bytes);
        if reader.slice(8, 4)? != b"WAVE" {
            return Err(FormatError::Unsupported {
                format: FORMAT,
                what: "a RIFF that is not WAVE".into(),
            });
        }
        let mut at = 12;
        let mut pcm = None;
        while at + 8 <= bytes.len() {
            let id = reader.slice(at, 4)?;
            let len = reader.u32(at + 4)? as usize;
            let body = at + 8;
            match id {
                b"fmt " => {
                    let tag = reader.u16(body)?;
                    let (channels, rate) = (reader.u16(body + 2)?, reader.u32(body + 4)?);
                    pcm = Some((
                        tag,
                        if tag == 0x11 {
                            Coding::Ima {
                                channels,
                                rate,
                                block: reader.u16(body + 12)?,
                            }
                        } else {
                            Coding::Pcm {
                                channels,
                                rate,
                                bits: reader.u16(body + 14)?,
                            }
                        },
                    ));
                }
                b"data" => {
                    let end = body.saturating_add(len).min(bytes.len());
                    let data = &bytes[body..end];
                    let rest = &bytes[body..];
                    let wav = |coding, data| Wav {
                        coding,
                        data,
                        riff: bytes,
                    };
                    return Ok(match pcm {
                        Some((0x55, _)) => wav(Coding::Mp3, data),
                        _ if len == 0 || is_mp3(rest) => wav(Coding::Mp3, rest),
                        Some((1 | 0x11, coding)) => wav(coding, data),
                        _ => {
                            return Err(FormatError::Unsupported {
                                format: FORMAT,
                                what: "a coding other than PCM, IMA ADPCM or MP3".into(),
                            });
                        }
                    });
                }
                _ => {}
            }
            at = body.saturating_add(len + (len & 1));
        }
        Err(FormatError::Unsupported {
            format: FORMAT,
            what: "no data chunk".into(),
        })
    }

    /// How long it plays (milliseconds), if it can be told.
    pub fn duration_ms(&self) -> Option<u32> {
        match self.coding {
            Coding::Pcm {
                channels,
                rate,
                bits,
            } => {
                let frame = u64::from(channels) * u64::from(bits).div_ceil(8);
                (frame > 0 && rate > 0)
                    .then(|| (self.data.len() as u64 * 1000 / frame / u64::from(rate)) as u32)
            }
            Coding::Mp3 => mp3_duration_ms(self.data),
            Coding::Ima { channels, rate, .. } => (channels > 0 && rate > 0).then(|| {
                // Two samples a byte, less the blocks' headers (close enough).
                (self.data.len() as u64 * 2 * 1000 / u64::from(channels) / u64::from(rate)) as u32
            }),
        }
    }

    /// Its IMA ADPCM layout; `None` for another coding, or one that
    /// cannot be read (no channel, or blocks no larger than their headers).
    pub fn ima(&self) -> Option<Ima> {
        let Coding::Ima {
            channels,
            rate,
            block,
        } = self.coding
        else {
            return None;
        };
        let (channels, block) = (usize::from(channels), usize::from(block));
        (channels > 0 && block > 4 * channels).then_some(Ima {
            channels,
            rate,
            block,
        })
    }
}

/// IMA ADPCM's layout (`Coding::Ima`): its blocks decode one at a time, so
/// a long sound can be decoded as it plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ima {
    pub channels: usize,
    pub rate: u32,
    /// Bytes a block.
    pub block: usize,
}

impl Ima {
    /// Block `index` of `data` (the coded bytes) as interleaved 16-bit
    /// samples; `None` past the last, or for a last too short for its
    /// headers.
    pub fn block(&self, data: &[u8], index: usize) -> Option<Vec<i16>> {
        let at = index.checked_mul(self.block)?;
        let block = data.get(at..data.len().min(at + self.block))?;
        ima::decode(block, self.channels)
    }
}

/// IMA ADPCM as WAV files carry it: each block starts with every channel's
/// first sample (16 bits) and step index (a byte, then one unused), then
/// four bytes of each channel in turn, each byte two samples, the low
/// nibble first. Each sample moves the last by a step the table gives,
/// the step's index moved by how large the nibble is.
mod ima {
    const STEPS: [i32; 89] = [
        7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60,
        66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371,
        408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878,
        2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845,
        8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086,
        29794, 32767,
    ];
    const MOVES: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

    #[derive(Clone, Copy)]
    struct Channel {
        sample: i32,
        index: i32,
    }

    impl Channel {
        fn next(&mut self, nibble: u8) -> i16 {
            let step = STEPS[self.index as usize];
            let mut diff = step >> 3;
            if nibble & 1 != 0 {
                diff += step >> 2;
            }
            if nibble & 2 != 0 {
                diff += step >> 1;
            }
            if nibble & 4 != 0 {
                diff += step;
            }
            self.sample = if nibble & 8 != 0 {
                self.sample - diff
            } else {
                self.sample + diff
            }
            .clamp(i32::from(i16::MIN), i32::from(i16::MAX));
            self.index = (self.index + MOVES[usize::from(nibble & 7)]).clamp(0, 88);
            self.sample as i16
        }
    }

    /// One block as interleaved 16-bit samples; `None` when it is too
    /// short for every channel's header.
    pub(super) fn decode(block: &[u8], channels: usize) -> Option<Vec<i16>> {
        if block.len() < 4 * channels {
            return None;
        }
        let mut state: Vec<Channel> = (0..channels)
            .map(|c| Channel {
                sample: i32::from(i16::from_le_bytes([block[4 * c], block[4 * c + 1]])),
                index: i32::from(block[4 * c + 2]).min(88),
            })
            .collect();
        let mut out = Vec::with_capacity(block.len() * 2);
        out.extend(state.iter().map(|c| c.sample as i16));
        // Each round: four bytes (eight samples) of every channel.
        for round in block[4 * channels..].chunks_exact(4 * channels) {
            let mut eight = vec![[0i16; 8]; channels];
            for (c, bytes) in round.as_chunks::<4>().0.iter().enumerate() {
                for (i, byte) in bytes.iter().enumerate() {
                    eight[c][2 * i] = state[c].next(byte & 0x0f);
                    eight[c][2 * i + 1] = state[c].next(byte >> 4);
                }
            }
            for i in 0..8 {
                out.extend(eight.iter().map(|samples| samples[i]));
            }
        }
        Some(out)
    }
}

fn is_mp3(bytes: &[u8]) -> bool {
    bytes.starts_with(b"ID3") || frame(bytes).is_some()
}

/// An MPEG audio layer III frame header at the start of `bytes`: its
/// length in bytes, samples and sample rate.
fn frame(bytes: &[u8]) -> Option<(usize, u32, u32)> {
    let [a, b, c, ..] = *bytes else {
        return None;
    };
    if a != 0xff || b & 0xe0 != 0xe0 || (b >> 1) & 3 != 1 {
        return None;
    }
    let version = (b >> 3) & 3;
    let (bitrate_index, rate_index, padding) = (c >> 4, (c >> 2) & 3, u32::from((c >> 1) & 1));
    if version == 1 || bitrate_index == 0 || bitrate_index == 15 || rate_index == 3 {
        return None;
    }
    const MPEG1: [u32; 15] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
    ];
    const MPEG2: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];
    const RATES: [u32; 3] = [44100, 48000, 32000];
    let mpeg1 = version == 3;
    let kbps = if mpeg1 { MPEG1 } else { MPEG2 }[usize::from(bitrate_index)];
    // MPEG 2 halves MPEG 1's rates, MPEG 2.5 quarters them.
    let rate = RATES[usize::from(rate_index)]
        >> match version {
            3 => 0,
            2 => 1,
            _ => 2,
        };
    let (samples, scale) = if mpeg1 { (1152, 144) } else { (576, 72) };
    let len = (scale * kbps * 1000 / rate + padding) as usize;
    Some((len, samples, rate))
}

/// An MP3 stream's length, from its frames (after any ID3v2 tag).
fn mp3_duration_ms(bytes: &[u8]) -> Option<u32> {
    let mut at = 0;
    if bytes.starts_with(b"ID3") && bytes.len() >= 10 {
        let size = bytes[6..10]
            .iter()
            .fold(0usize, |size, &byte| (size << 7) | usize::from(byte & 0x7f));
        at = 10 + size;
    }
    let mut micros = 0u64;
    let mut frames = 0;
    while at + 4 <= bytes.len() {
        match frame(&bytes[at..]) {
            Some((len, samples, rate)) if len > 4 => {
                micros += u64::from(samples) * 1_000_000 / u64::from(rate);
                frames += 1;
                at += len;
            }
            _ => at += 1,
        }
    }
    (frames > 0).then_some((micros / 1000) as u32)
}
