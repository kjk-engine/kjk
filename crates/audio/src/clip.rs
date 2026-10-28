use kira::Frame;
use kira::sound::static_sound::{StaticSoundData, StaticSoundSettings};

use crate::decode::{Coded, Decoder};

const MAGIC: &[u8; 4] = b"KJKA";

/// A sound decoded whole, ready to play any number of times at once.
#[derive(Clone)]
pub struct Clip(pub(crate) StaticSoundData);

impl Clip {
    /// The version of `encode`'s bytes: bump when they change.
    pub const VERSION: u32 = 1;

    /// A WAV (PCM), MP3 or FLAC file (`Coded::file`).
    pub fn decode(bytes: &[u8]) -> Result<Clip, String> {
        Clip::whole(&Coded::file(bytes))
    }

    /// A KOTOR WAV (`Coded::kotor`).
    pub fn kotor(bytes: &[u8]) -> Result<Clip, String> {
        Clip::whole(&Coded::kotor(bytes)?)
    }

    fn whole(coded: &Coded) -> Result<Clip, String> {
        let mut decoder = Decoder::open(coded)?;
        let mut frames = Vec::new();
        while let Some(more) = decoder.next()? {
            frames.extend(more);
        }
        Ok(Clip(StaticSoundData {
            sample_rate: decoder.rate(),
            frames: frames.into(),
            settings: StaticSoundSettings::default(),
            slice: None,
        }))
    }

    /// The decoded samples, for keeping (`docs/IMPORT.md`): little-endian,
    /// each sample by its bits, so a clip read back plays exactly as the
    /// clip decoded.
    pub fn encode(&self) -> Vec<u8> {
        let frames = &self.0.frames;
        let mut out = Vec::with_capacity(16 + frames.len() * 8);
        out.extend(MAGIC);
        out.extend(Clip::VERSION.to_le_bytes());
        out.extend(self.0.sample_rate.to_le_bytes());
        out.extend((frames.len() as u32).to_le_bytes());
        for frame in frames.iter() {
            out.extend(frame.left.to_bits().to_le_bytes());
            out.extend(frame.right.to_bits().to_le_bytes());
        }
        out
    }

    /// A clip `encode` wrote; `None` if it does not read.
    pub fn from_encoded(bytes: &[u8]) -> Option<Clip> {
        let (head, samples) = bytes.split_first_chunk::<16>()?;
        let word = |at: usize| u32::from_le_bytes(head[at..at + 4].try_into().expect("4 bytes"));
        if &head[..4] != MAGIC || word(4) != Clip::VERSION || samples.len() != word(12) as usize * 8
        {
            return None;
        }
        let frames: Vec<Frame> = samples
            .as_chunks::<4>()
            .0
            .as_chunks::<2>()
            .0
            .iter()
            .map(|[left, right]| Frame {
                left: f32::from_bits(u32::from_le_bytes(*left)),
                right: f32::from_bits(u32::from_le_bytes(*right)),
            })
            .collect();
        Some(Clip(StaticSoundData {
            sample_rate: word(8),
            frames: frames.into(),
            settings: StaticSoundSettings::default(),
            slice: None,
        }))
    }

    pub fn seconds(&self) -> f64 {
        self.0.duration().as_secs_f64()
    }
}
