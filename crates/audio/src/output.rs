//! The stream kjk opens on the sound device itself, for a device that is
//! written in integer samples.
//!
//! kira's own stream always writes 32-bit float samples. Razer's 7.1
//! device, which THX Spatial Audio presents, garbles a float stream and
//! plays a 32-bit integer stream cleanly (cpal issue 892, kjk issue 402).
//! This stream renders kira's mix as floats and hands the device each
//! sample in the type the stream was opened with: 16-, 24- or 32-bit
//! integers, or 32-bit floats.
//!
//! kira's renderer lays the stereo mix out for the device. A device with one
//! channel gets the average of the two sides. A device with two or more
//! channels gets the left side in its first channel (front left) and the
//! right side in its second (front right), and silence in the others
//! (centre, the low frequency channel and the surrounds). The mix is made
//! at the device's own sample rate.

use std::sync::mpsc;

use kira::backend::Renderer;
use kira::backend::cpal::cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use kira::backend::cpal::cpal::{self, FromSample, I24, SampleFormat, SizedSample, StreamConfig};

/// How much mix the stream can be asked for at once before its buffer
/// grows, in seconds. A device asks for a few tens of milliseconds at a
/// time, so the buffer never grows while the game plays.
const MOST_ASKED: f64 = 0.5;

/// Fills `out` with the mix's next frames, `channels` samples to a frame,
/// each sample converted to the device's type. `mixed` is the float buffer
/// the renderer mixes into first.
pub(crate) fn fill<T>(renderer: &mut Renderer, mixed: &mut Vec<f32>, out: &mut [T], channels: u16)
where
    T: SizedSample + FromSample<f32>,
{
    mixed.clear();
    mixed.resize(out.len(), 0.0);
    renderer.on_start_processing();
    renderer.process(mixed, channels);
    for (sample, &value) in out.iter_mut().zip(mixed.iter()) {
        *sample = T::from_sample(value);
    }
}

/// A stream on the sound device that kjk writes itself. A thread of its
/// own holds the stream, because a stream cannot move between threads on
/// every system. The stream closes when this is dropped.
pub(crate) struct DeviceStream {
    _stop: mpsc::Sender<()>,
}

/// What a `DeviceStream` opens: the device (the system's default when
/// `None`), the stream's rate, channels and buffer, and the sample type.
pub(crate) struct Opening {
    pub device: Option<cpal::Device>,
    pub config: StreamConfig,
    pub format: SampleFormat,
}

impl DeviceStream {
    /// Opens the stream and starts it playing `renderer`'s mix.
    pub(crate) fn open(opening: Opening, renderer: Renderer) -> Result<DeviceStream, String> {
        let (stop, stopped) = mpsc::channel::<()>();
        let (opened, result) = mpsc::channel::<Result<(), String>>();
        std::thread::Builder::new()
            .name("sound device".into())
            .spawn(move || {
                let stream = match build(opening, renderer) {
                    Ok(stream) => stream,
                    Err(error) => {
                        let _ = opened.send(Err(error));
                        return;
                    }
                };
                let _ = opened.send(Ok(()));
                // Waits until the `DeviceStream` is dropped, then lets the
                // stream go with it.
                let _ = stopped.recv();
                drop(stream);
            })
            .map_err(|error| error.to_string())?;
        result
            .recv()
            .map_err(|_| "the sound device's thread ended".to_string())??;
        Ok(DeviceStream { _stop: stop })
    }
}

/// The started stream, for the opening's sample type.
fn build(opening: Opening, renderer: Renderer) -> Result<cpal::Stream, String> {
    match opening.format {
        SampleFormat::I16 => build_as::<i16>(opening, renderer),
        SampleFormat::I24 => build_as::<I24>(opening, renderer),
        SampleFormat::I32 => build_as::<i32>(opening, renderer),
        SampleFormat::F32 => build_as::<f32>(opening, renderer),
        other => Err(format!("kjk does not write {other} samples")),
    }
}

fn build_as<T>(opening: Opening, mut renderer: Renderer) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let device = match opening.device {
        Some(device) => device,
        None => cpal::default_host()
            .default_output_device()
            .ok_or("no default sound device")?,
    };
    let config = opening.config;
    let channels = config.channels;
    let room = f64::from(config.sample_rate) * MOST_ASKED * f64::from(channels);
    let mut mixed = Vec::with_capacity(room as usize);
    let stream = device
        .build_output_stream(
            config,
            move |out: &mut [T], _| fill(&mut renderer, &mut mixed, out, channels),
            // A stream error (the device run dry, or gone) is not fatal: the
            // system moves a stream on its default device to the new default.
            |_| {},
            None,
        )
        .map_err(|error| error.to_string())?;
    stream.play().map_err(|error| error.to_string())?;
    Ok(stream)
}
