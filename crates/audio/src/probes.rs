//! The probes of the sound device's stream (kjk issue 402).
//!
//! A Mac cannot present the 6- and 8-channel devices that Windows spatial
//! sound presents, so these probes feed a known stereo signal through the
//! same code that fills a device's stream, for made-up devices. Each device
//! has 1, 2, 4, 6 or 8 channels, a sample rate of 44.1 kHz or 48 kHz, and
//! takes 32-bit floats or 16-, 24- or 32-bit integers.

use glam::Vec3;
use kira::backend::cpal::cpal::{FromSample, I24, SampleFormat, SizedSample};

use crate::device;
use crate::{Channel, Clip, Emitter, Falloff, Mixer, Output, Sound};

/// The rate of the test signal's clip.
const CLIP_RATE: u32 = 22_050;
/// The test signal's left side: a square wave of 220.5 Hz, which turns
/// over every 50 of the clip's samples. Its right side turns over every
/// 100 samples, at 110.25 Hz, so swapped sides are told apart.
const LEFT_TURN: u32 = 50;
const RIGHT_TURN: u32 = 100;
/// How much of the mix each probe reads, in seconds.
const SECONDS: f64 = 0.25;

/// The probes, each a name and its outcome (`Ok` with what was measured,
/// `Err` with what went wrong).
pub fn device_layouts() -> Vec<(String, Result<String, String>)> {
    let mut probes = Vec::new();
    for channels in [1u16, 2, 4, 6, 8] {
        for rate in [44_100u32, 48_000] {
            for format in [
                SampleFormat::F32,
                SampleFormat::I16,
                SampleFormat::I24,
                SampleFormat::I32,
            ] {
                let name = format!("a {channels}-channel {rate} Hz {format} device");
                probes.push((name, layout(channels, rate, format)));
            }
        }
    }
    for channels in [1u16, 2, 4, 6, 8] {
        let written = device::windows_format(channels);
        let wanted = if channels > 2 {
            SampleFormat::I32
        } else {
            SampleFormat::F32
        };
        let outcome = if written == wanted {
            Ok(format!("written in {written}"))
        } else {
            Err(format!("written in {written}, not {wanted}"))
        };
        probes.push((
            format!("a {channels}-channel Windows device's sample type"),
            outcome,
        ));
    }
    probes
}

/// Checks one device: its first channel carries the left side and its
/// second the right, at the device's rate, and its other channels are
/// silent. A one-channel device carries the average of the two sides.
fn layout(channels: u16, rate: u32, format: SampleFormat) -> Result<String, String> {
    let device = match format {
        SampleFormat::I16 => heard::<i16>(channels, rate)?,
        SampleFormat::I24 => heard::<I24>(channels, rate)?,
        SampleFormat::I32 => heard::<i32>(channels, rate)?,
        _ => heard::<f32>(channels, rate)?,
    };
    let stereo = heard::<f32>(2, rate)?;
    let (left, right): (Vec<f32>, Vec<f32>) = stereo
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[left, right]| (left, right))
        .unzip();
    // Each side of the reference turns over as often as its square wave
    // does at the device's rate; a mix made at another rate turns over more
    // or less often.
    let expect = |turn: u32| SECONDS * f64::from(CLIP_RATE) / f64::from(turn);
    for (side, samples, turn) in [("left", &left, LEFT_TURN), ("right", &right, RIGHT_TURN)] {
        let turns = turns(samples) as f64;
        if (turns - expect(turn)).abs() > 3.0 {
            return Err(format!(
                "the {side} side turns over {turns} times in {SECONDS} s, not {}",
                expect(turn)
            ));
        }
    }
    // The steps of the device's sample type, as floats.
    let step = match format {
        SampleFormat::I16 => 2.0 / 32_768.0,
        SampleFormat::I24 => 2.0 / 8_388_608.0,
        _ => 1e-6,
    };
    let frames = left.len();
    let got = device.len() / usize::from(channels);
    if got != frames {
        return Err(format!("{got} frames, not {frames}"));
    }
    for (at, frame) in device.chunks_exact(usize::from(channels)).enumerate() {
        let wanted = |channel: usize| match (channels, channel) {
            (1, _) => (left[at] + right[at]) / 2.0,
            (_, 0) => left[at],
            (_, 1) => right[at],
            _ => 0.0,
        };
        for (channel, &sample) in frame.iter().enumerate() {
            if (sample - wanted(channel)).abs() > step {
                return Err(format!(
                    "frame {at}, channel {channel} is {sample}, not {}",
                    wanted(channel)
                ));
            }
        }
    }
    Ok(format!("{frames} frames in the right channels"))
}

/// `SECONDS` of the test signal as a device with `channels` channels at
/// `rate` gets it, in samples of type `T`, read back as floats.
fn heard<T>(channels: u16, rate: u32) -> Result<Vec<f32>, String>
where
    T: SizedSample + FromSample<f32>,
    f32: FromSample<T>,
{
    let mut mixer = Mixer::new(Output::Capture)?;
    mixer.advance(0.0);
    mixer.play_now(signal()?);
    let frames = (f64::from(rate) * SECONDS) as usize;
    let samples: Vec<T> = mixer.device_frames(rate, channels, frames);
    Ok(samples.into_iter().map(FromSample::from_sample_).collect())
}

/// How many times `samples` changes sign.
fn turns(samples: &[f32]) -> usize {
    samples
        .iter()
        .filter(|sample| sample.abs() > 0.01)
        .collect::<Vec<_>>()
        .windows(2)
        .filter(|pair| pair[0].signum() != pair[1].signum())
        .count()
}

/// A second of the test signal: a stereo 16-bit WAV, heard alike in both
/// ears wherever the listener is.
fn signal() -> Result<Sound, String> {
    let square = |at: u32, turn: u32| -> i16 {
        if (at / turn).is_multiple_of(2) {
            20_000
        } else {
            -20_000
        }
    };
    let samples: Vec<u8> = (0..CLIP_RATE)
        .flat_map(|at| {
            let left = square(at, LEFT_TURN).to_le_bytes();
            let right = square(at, RIGHT_TURN).to_le_bytes();
            [left, right].concat()
        })
        .collect();
    let size = u32::try_from(samples.len()).map_err(|error| error.to_string())?;
    let wav = [
        b"RIFF".as_slice(),
        &(36 + size).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
        // PCM, two channels.
        &1u16.to_le_bytes(),
        &2u16.to_le_bytes(),
        &CLIP_RATE.to_le_bytes(),
        &(CLIP_RATE * 4).to_le_bytes(),
        &4u16.to_le_bytes(),
        &16u16.to_le_bytes(),
        b"data",
        &size.to_le_bytes(),
        &samples,
    ]
    .concat();
    Ok(Sound {
        clip: Clip::decode(&wav)?,
        emitter: Emitter {
            at: Vec3::ZERO,
            entity: None,
            falloff: Falloff::Unplaced,
        },
        channel: Channel::Auto,
        follows: false,
        gain: 1.0,
        rate: 1.0,
    })
}
