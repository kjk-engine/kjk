//! Sound files decoded a piece at a time, the one way a `Clip` (decoded
//! whole) and a `Stream` (as it plays) are both decoded.

use std::io::Cursor;
use std::sync::Arc;

use fmt_odyssey::wav::{Coding, Ima, Wav};
use kira::Frame;
use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::audio::conv::FromSample;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoder;
use symphonia::core::formats::{FormatReader, TrackType};
use symphonia::core::io::MediaSourceStream;

/// How far into a file a WAV's `RIFF` is looked for.
const JUNK_MOST: usize = 1024;

/// A sound's coded bytes, shared by whatever decodes them.
#[derive(Clone)]
pub(crate) enum Coded {
    /// A file symphonia reads: an MP3, a PCM WAV, a FLAC.
    File(Arc<[u8]>),
    /// IMA ADPCM, which it does not (its blocks are decoded by `Ima`).
    Ima(Arc<[u8]>, Ima),
}

impl Coded {
    /// A WAV, MP3 or FLAC file. Bytes before a WAV's `RIFF` (some of
    /// KOTOR's carry a header of their own there) are passed over.
    pub(crate) fn file(bytes: &[u8]) -> Coded {
        let at = bytes
            .windows(4)
            .take(JUNK_MOST)
            .position(|w| w == b"RIFF")
            .unwrap_or(0);
        Coded::File(bytes[at..].into())
    }

    /// A KOTOR WAV: its MP3 taken out of its RIFF, IMA ADPCM by its
    /// blocks, a PCM file as it is.
    pub(crate) fn kotor(bytes: &[u8]) -> Result<Coded, String> {
        let wav = Wav::parse(bytes).map_err(|error| error.to_string())?;
        match wav.coding {
            Coding::Mp3 => Ok(Coded::file(wav.data)),
            Coding::Ima { .. } => wav
                .ima()
                .map(|ima| Coded::Ima(wav.data.into(), ima))
                .ok_or_else(|| "an IMA ADPCM layout that does not read".into()),
            Coding::Pcm { .. } => Ok(Coded::file(wav.riff)),
        }
    }

    /// The bytes kept.
    pub(crate) fn len(&self) -> usize {
        match self {
            Coded::File(bytes) | Coded::Ima(bytes, _) => bytes.len(),
        }
    }
}

/// Where a decoder is in its sound.
enum Source {
    File {
        reader: Box<dyn FormatReader>,
        decoder: Box<dyn AudioDecoder>,
        track: u32,
    },
    Ima {
        data: Arc<[u8]>,
        ima: Ima,
        next: usize,
    },
}

/// A sound decoded from its start, a piece (a packet, a block) at a time,
/// into stereo frames as kira plays them.
pub(crate) struct Decoder {
    rate: u32,
    source: Source,
}

impl Decoder {
    pub(crate) fn open(coded: &Coded) -> Result<Decoder, String> {
        match coded {
            Coded::File(bytes) => {
                let stream = MediaSourceStream::new(
                    Box::new(Cursor::new(bytes.clone())),
                    Default::default(),
                );
                let reader = symphonia::default::get_probe()
                    .probe(
                        &Default::default(),
                        stream,
                        Default::default(),
                        Default::default(),
                    )
                    .map_err(|error| error.to_string())?;
                let track = reader
                    .default_track(TrackType::Audio)
                    .ok_or("no audio track")?;
                let Some(CodecParameters::Audio(params)) = track.codec_params.as_ref() else {
                    return Err("no audio track".into());
                };
                let rate = params.sample_rate.ok_or("no sample rate")?;
                let decoder = symphonia::default::get_codecs()
                    .make_audio_decoder(params, &Default::default())
                    .map_err(|error| error.to_string())?;
                let track = track.id;
                Ok(Decoder {
                    rate,
                    source: Source::File {
                        reader,
                        decoder,
                        track,
                    },
                })
            }
            Coded::Ima(data, ima) => {
                if !(1..=2).contains(&ima.channels) {
                    return Err(STEREO_MOST.into());
                }
                Ok(Decoder {
                    rate: ima.rate,
                    source: Source::Ima {
                        data: data.clone(),
                        ima: *ima,
                        next: 0,
                    },
                })
            }
        }
    }

    /// Frames a second.
    pub(crate) fn rate(&self) -> u32 {
        self.rate
    }

    /// The next frames (never none), `None` at the end.
    pub(crate) fn next(&mut self) -> Result<Option<Vec<Frame>>, String> {
        loop {
            let frames = match &mut self.source {
                Source::File {
                    reader,
                    decoder,
                    track,
                } => {
                    let packet = match reader.next_packet() {
                        Ok(Some(packet)) => packet,
                        Ok(None) => return Ok(None),
                        Err(error) => return Err(error.to_string()),
                    };
                    if packet.track_id != *track {
                        continue;
                    }
                    let buffer = decoder.decode(&packet).map_err(|error| error.to_string())?;
                    frames(&buffer)?
                }
                Source::Ima { data, ima, next } => {
                    let Some(samples) = ima.block(data, *next) else {
                        return Ok(None);
                    };
                    *next += 1;
                    let sample = |s: &i16| f32::from_sample(*s);
                    match ima.channels {
                        1 => samples
                            .iter()
                            .map(|s| Frame::from_mono(sample(s)))
                            .collect(),
                        _ => samples
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|[left, right]| Frame::new(sample(left), sample(right)))
                            .collect(),
                    }
                }
            };
            if !frames.is_empty() {
                return Ok(Some(frames));
            }
        }
    }
}

const STEREO_MOST: &str = "more than two channels";

/// A decoded packet's frames: mono heard in both ears.
fn frames(buffer: &GenericAudioBufferRef) -> Result<Vec<Frame>, String> {
    let mut planes: Vec<Vec<f32>> = Vec::new();
    buffer.copy_to_vecs_planar(&mut planes);
    match planes.as_slice() {
        [mono] => Ok(mono.iter().map(|&s| Frame::from_mono(s)).collect()),
        [left, right] => Ok(left
            .iter()
            .zip(right)
            .map(|(&left, &right)| Frame::new(left, right))
            .collect()),
        _ => Err(STEREO_MOST.into()),
    }
}
