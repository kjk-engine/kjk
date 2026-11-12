//! The sound device the mix plays on, and the stream it opens there.
//!
//! On macOS and Windows the system's default device and its own default
//! stream are used as kira picks them. Linux sound goes through ALSA, and
//! ALSA's defaults go wrong in a few ways there, so the stream is chosen
//! here instead (`linux`):
//!
//! - kira always writes 32-bit float samples. A device whose preferred
//!   format is 16- or 32-bit integers cannot open a float stream with the
//!   settings it reports as its default, so a float configuration the device
//!   supports is looked for, and another device (`pipewire`, `pulse`,
//!   `default`) when this one has none.
//! - Each period is asked to be `LINUX_PERIOD` frames, within what the
//!   device allows. Very short periods on PipeWire's and PulseAudio's ALSA
//!   plugins run dry under a game's load and crackle.
//! - The device is named to kira, which then stops asking ALSA for the
//!   default device every half second. Each such ask opens the device again
//!   while the game plays on it, and a different answer restarts the stream.
//!
//! Each choice is tried in turn (`candidates`), down to kira's own defaults,
//! so a device that refuses one still plays.

use kira::backend::cpal::CpalBackendSettings;
use kira::backend::cpal::cpal::traits::{DeviceTrait, HostTrait};
use kira::backend::cpal::cpal::{
    BufferSize, Device, SampleFormat, StreamConfig, SupportedBufferSize, SupportedStreamConfig,
    SupportedStreamConfigRange,
};

/// The frames in each period of a Linux stream: about 21 ms at 48 kHz.
pub const LINUX_PERIOD: u32 = 1024;

/// The ALSA devices tried when the default one has no float stream, best
/// first. PipeWire's and PulseAudio's plugins and ALSA's own default all
/// convert samples to what the hardware takes.
const ALSA_FALLBACKS: [&str; 3] = ["pipewire", "pulse", "default"];

/// The device settings to try, best first. The last one is always kira's
/// own defaults.
pub fn candidates() -> Vec<CpalBackendSettings> {
    let mut candidates = if cfg!(target_os = "linux") {
        linux()
    } else {
        Vec::new()
    };
    candidates.push(CpalBackendSettings::default());
    candidates
}

/// The Linux choices: a float stream with `LINUX_PERIOD`-frame periods on
/// the default device (or a fallback device), then the same stream with the
/// device's own period.
fn linux() -> Vec<CpalBackendSettings> {
    let host = kira::backend::cpal::cpal::default_host();
    let default = host.default_output_device();
    let others = host
        .output_devices()
        .map(|devices| devices.collect::<Vec<_>>())
        .unwrap_or_else(|_| Vec::new());
    let mut fallbacks: Vec<Device> = others
        .into_iter()
        .filter_map(|device| {
            let rank = ALSA_FALLBACKS
                .iter()
                .position(|name| *name == pcm_name(&device))?;
            Some((rank, device))
        })
        .collect::<std::collections::BTreeMap<_, _>>()
        .into_values()
        .collect();
    if let Some(default) = &default {
        fallbacks.retain(|device| device.id().ok() != default.id().ok());
    }
    let Some((device, config)) = default
        .into_iter()
        .chain(fallbacks)
        .find_map(|device| float_config(&device).map(|config| (device, config)))
    else {
        return Vec::new();
    };
    let own_period = StreamConfig {
        buffer_size: BufferSize::Default,
        ..config
    };
    let mut choices = vec![CpalBackendSettings {
        device: Some(device.clone()),
        config: Some(config),
    }];
    if own_period != config {
        choices.push(CpalBackendSettings {
            device: Some(device),
            config: Some(own_period),
        });
    }
    choices
}

/// The PCM name of an ALSA device (`default`, `pipewire`, `hw:0,0`).
fn pcm_name(device: &Device) -> String {
    device
        .id()
        .map(|id| id.id().to_string())
        .unwrap_or_else(|_| String::new())
}

/// A float stream `device` can open (`pick`), or `None`.
fn float_config(device: &Device) -> Option<StreamConfig> {
    let ranges: Vec<_> = device.supported_output_configs().ok()?.collect();
    pick(device.default_output_config().ok().as_ref(), &ranges)
}

/// The stream to open, from the device's default and the ranges it
/// supports: the default when it is float, else the best float range
/// (`cmp_default_heuristics`: stereo, then 48 kHz or 44.1 kHz), at the
/// default's rate when that range has it. Its periods are `LINUX_PERIOD`
/// frames, within the range's limits, when the range has limits.
/// `None` when no range is float.
pub fn pick(
    default: Option<&SupportedStreamConfig>,
    ranges: &[SupportedStreamConfigRange],
) -> Option<StreamConfig> {
    let chosen = match default {
        Some(default) if default.sample_format() == SampleFormat::F32 => *default,
        _ => {
            let best = ranges
                .iter()
                .filter(|range| range.sample_format() == SampleFormat::F32)
                .max_by(|a, b| a.cmp_default_heuristics(b))
                .copied()?;
            match default {
                Some(default) if best.contains_rate(default.sample_rate()) => {
                    best.with_sample_rate(default.sample_rate())
                }
                _ => best
                    .try_with_standard_sample_rate()
                    .unwrap_or_else(|| best.with_max_sample_rate()),
            }
        }
    };
    let buffer_size = match chosen.buffer_size() {
        SupportedBufferSize::Range { min, max } => {
            BufferSize::Fixed(LINUX_PERIOD.clamp(*min, (*max).max(*min)))
        }
        SupportedBufferSize::Unknown => BufferSize::Default,
    };
    Some(StreamConfig {
        buffer_size,
        ..chosen.config()
    })
}

