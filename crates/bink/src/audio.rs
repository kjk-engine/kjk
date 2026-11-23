//! Bink's DCT audio. Each packet holds blocks of `frame_len` samples per
//! channel. A block codes its frequencies in bands, each band with its own
//! quantiser, and an inverse DCT turns them into samples. The first
//! sixteenth of each block is cross-faded with the end of the block before,
//! so a block gives `frame_len - frame_len / 16` new samples per channel.

use crate::bits::Bits;
use crate::{Bink, Track};

/// The bands' upper frequencies in hertz (the critical bands WMA uses too).
const CRITICAL_FREQUENCIES: [u32; 25] = [
    100, 200, 300, 400, 510, 630, 770, 920, 1080, 1270, 1480, 1720, 2000, 2320, 2700, 3150, 3700,
    4400, 5300, 6400, 7700, 9500, 12000, 15500, 24500,
];
/// How many groups of 8 coefficients a run that shares one width covers.
const RUN_LENGTHS: [usize; 16] = [2, 3, 4, 5, 6, 8, 9, 10, 11, 12, 13, 14, 15, 16, 32, 64];

/// A decoded sound: its samples interleaved, from -1 to 1.
pub struct Sound {
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

impl Sound {
    /// Every audio packet of `track` decoded.
    pub fn of(bink: &Bink, track: usize) -> Result<Sound, String> {
        let coding = *bink
            .tracks
            .get(track)
            .ok_or(format!("no audio track {track}"))?;
        let mut audio = Audio::new(coding)?;
        let mut samples = Vec::new();
        for frame in 0..bink.frames() {
            audio.decode(bink.packets(frame)?.audio[track], &mut samples)?;
        }
        Ok(Sound {
            rate: coding.rate,
            channels: coding.channels,
            samples,
        })
    }

    /// The sound as a 16-bit PCM WAV file.
    pub fn wav(&self) -> Vec<u8> {
        let data = (self.samples.len() * 2) as u32;
        let channels = self.channels as u16;
        let mut wav = Vec::with_capacity(44 + data as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&self.rate.to_le_bytes());
        wav.extend_from_slice(&(self.rate * u32::from(channels) * 2).to_le_bytes());
        wav.extend_from_slice(&(channels * 2).to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data.to_le_bytes());
        for &sample in &self.samples {
            let value = (sample * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
            wav.extend_from_slice(&value.to_le_bytes());
        }
        wav
    }
}

/// The decoder of one audio track.
pub struct Audio {
    channels: usize,
    frame_len: usize,
    overlap: usize,
    /// Where each band starts, in coefficients, and the end of the last.
    bands: Vec<usize>,
    quantisers: [f32; 96],
    root: f32,
    dct: Dct,
    /// Each channel's last `overlap` samples of the block before.
    previous: Vec<Vec<f32>>,
    first: bool,
    /// Each channel's coefficients, then its samples.
    blocks: Vec<Vec<f32>>,
}

impl Audio {
    pub fn new(track: Track) -> Result<Audio, String> {
        if !track.dct {
            return Err("RDFT audio is not decoded here".into());
        }
        if track.rate == 0 {
            return Err("audio with a rate of 0".into());
        }
        let bits = match track.rate {
            ..22050 => 9,
            22050..44100 => 10,
            _ => 11,
        };
        let frame_len = 1usize << bits;
        let half_rate = track.rate.div_ceil(2);
        let band_count = (1..25)
            .find(|&count| half_rate <= CRITICAL_FREQUENCIES[count - 1])
            .unwrap_or(25);
        let mut bands = vec![2];
        bands.extend((1..band_count).map(|band| {
            (CRITICAL_FREQUENCIES[band - 1] as usize * frame_len / half_rate as usize) & !1
        }));
        bands.push(frame_len);
        let root = frame_len as f32 / ((frame_len as f32).sqrt() * 32768.0);
        Ok(Audio {
            channels: track.channels,
            frame_len,
            overlap: frame_len / 16,
            bands,
            // Each step is 0.664 dB.
            quantisers: std::array::from_fn(|i| (i as f32 * 0.152_891_65).exp() * root),
            root,
            dct: Dct::new(frame_len),
            previous: vec![vec![0.0; frame_len / 16]; track.channels],
            first: true,
            blocks: vec![vec![0.0; frame_len]; track.channels],
        })
    }

    /// Decodes one packet, adding its samples to `out`, interleaved.
    pub fn decode(&mut self, packet: &[u8], out: &mut Vec<f32>) -> Result<(), String> {
        if packet.len() <= 4 {
            return Ok(());
        }
        let mut bits = Bits::new(packet);
        // The packet's decoded size, which is not needed.
        bits.skip(32);
        while bits.left() > 0 {
            self.block(&mut bits);
            if bits.overrun() {
                return Err("an audio block runs past its packet".into());
            }
            bits.align32();
            let samples = self.frame_len - self.overlap;
            for i in 0..samples {
                out.extend(self.blocks.iter().map(|block| block[i]));
            }
        }
        Ok(())
    }

    fn block(&mut self, bits: &mut Bits) {
        bits.skip(2);
        for channel in 0..self.channels {
            let coefficients = &mut self.blocks[channel];
            coefficients[0] = float(bits) * self.root;
            coefficients[1] = float(bits) * self.root;
            let quantisers: Vec<f32> = (0..self.bands.len() - 1)
                .map(|_| self.quantisers[(bits.read(8) as usize).min(95)])
                .collect();
            let mut band = 0;
            let mut quantiser = quantisers[0];
            let mut i = 2;
            while i < self.frame_len {
                let end = if bits.bit() {
                    i + RUN_LENGTHS[bits.read(4) as usize] * 8
                } else {
                    i + 8
                }
                .min(self.frame_len);
                let width = bits.read(4);
                if width == 0 {
                    coefficients[i..end].fill(0.0);
                    i = end;
                    while self.bands[band] < i {
                        quantiser = quantisers[band];
                        band += 1;
                    }
                } else {
                    while i < end {
                        if self.bands[band] == i {
                            quantiser = quantisers[band];
                            band += 1;
                        }
                        let value = bits.read(width);
                        coefficients[i] = if value == 0 {
                            0.0
                        } else if bits.bit() {
                            -quantiser * value as f32
                        } else {
                            quantiser * value as f32
                        };
                        i += 1;
                    }
                }
            }
            coefficients[0] *= 2.0;
            self.dct.inverse(coefficients);
        }
        let count = (self.overlap * self.channels) as f32;
        for (channel, (block, previous)) in
            self.blocks.iter_mut().zip(&mut self.previous).enumerate()
        {
            if !self.first {
                for i in 0..self.overlap {
                    let weight = (channel + i * self.channels) as f32;
                    block[i] = (previous[i] * (count - weight) + block[i] * weight) / count;
                }
            }
            previous.copy_from_slice(&block[self.frame_len - self.overlap..]);
        }
        self.first = false;
    }
}

/// A float of 5 bits of exponent, 23 bits of mantissa and a sign bit.
fn float(bits: &mut Bits) -> f32 {
    let power = bits.read(5) as i32;
    let value = bits.read(23) as f32 * 2f32.powi(power - 23);
    if bits.bit() { -value } else { value }
}

/// An inverse DCT (DCT-III) of a power-of-two size N, done with a complex
/// FFT of the same size:
/// `y[n] = 2 / N × (x[0] / 2 + sum over k ≥ 1 of x[k] cos(π k (2n + 1) / 2N))`.
struct Dct {
    size: usize,
    /// `2 / N × e^(iπk / 2N)`, halved for k = 0.
    turns: Vec<(f32, f32)>,
    fft: Fft,
    work: Vec<(f32, f32)>,
}

impl Dct {
    fn new(size: usize) -> Dct {
        let turns = (0..size)
            .map(|k| {
                let angle = std::f64::consts::PI * k as f64 / (2 * size) as f64;
                let scale = if k == 0 { 1.0 } else { 2.0 } / size as f64;
                ((angle.cos() * scale) as f32, (angle.sin() * scale) as f32)
            })
            .collect();
        Dct {
            size,
            turns,
            fft: Fft::new(size),
            work: vec![(0.0, 0.0); size],
        }
    }

    fn inverse(&mut self, data: &mut [f32]) {
        for ((work, &(re, im)), &x) in self.work.iter_mut().zip(&self.turns).zip(data.iter()) {
            *work = (x * re, x * im);
        }
        self.fft.inverse(&mut self.work);
        let half = self.size / 2;
        for (m, &(re, _)) in self.work.iter().enumerate() {
            let n = if m < half {
                2 * m
            } else {
                2 * self.size - 1 - 2 * m
            };
            data[n] = re;
        }
    }
}

/// A radix-2 complex FFT with a positive exponent and no scaling.
struct Fft {
    twiddles: Vec<(f32, f32)>,
    reversed: Vec<usize>,
}

impl Fft {
    fn new(size: usize) -> Fft {
        let bits = size.trailing_zeros();
        Fft {
            twiddles: (0..size / 2)
                .map(|k| {
                    let angle = 2.0 * std::f64::consts::PI * k as f64 / size as f64;
                    (angle.cos() as f32, angle.sin() as f32)
                })
                .collect(),
            reversed: (0..size)
                .map(|i| i.reverse_bits() >> (usize::BITS - bits))
                .collect(),
        }
    }

    fn inverse(&self, data: &mut [(f32, f32)]) {
        let size = data.len();
        for (i, &j) in self.reversed.iter().enumerate() {
            if i < j {
                data.swap(i, j);
            }
        }
        let mut span = 1;
        while span < size {
            let step = size / (2 * span);
            for start in (0..size).step_by(2 * span) {
                for k in 0..span {
                    let (wr, wi) = self.twiddles[k * step];
                    let (ar, ai) = data[start + k];
                    let (br, bi) = data[start + k + span];
                    let (tr, ti) = (br * wr - bi * wi, br * wi + bi * wr);
                    data[start + k] = (ar + tr, ai + ti);
                    data[start + k + span] = (ar - tr, ai - ti);
                }
            }
            span *= 2;
        }
    }
}
