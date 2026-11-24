//! The probes of `sound` (`kjk sim probes`, group "sound pause").

use audio::{Channel, Clip, Emitter, Falloff, Mixer, Output, Sound};
use bevy::math::Vec3;
use sim::probe::{Checks, check};

/// A pause (a menu that pauses the world, a break) stops the world's sounds
/// but lets the interface's play on (issue 389). The menu's own pause can
/// end for a frame while an action made on it takes effect, and that
/// must not stop the interface's sounds either.
pub fn pause_probes() -> Checks {
    let heard = |interface: bool, flicker: bool| -> Result<bool, String> {
        let mut mixer = Mixer::new(Output::Capture)?;
        mixer.advance(0.0);
        let sound = tone()?;
        if interface {
            mixer.play_now(sound);
        } else {
            mixer.play_at(0.0, sound);
            mixer.advance(10.0);
        }
        mixer.hush(true);
        if flicker {
            mixer.advance(20.0);
            mixer.hush(false);
            mixer.advance(40.0);
            mixer.hush(true);
        }
        mixer.advance(100.0);
        // The pause fades what it stops; well after the fade, a stopped
        // sound is silent.
        let later = mixer.advance(300.0);
        Ok(later.iter().any(|sample| sample.abs() > 0.01))
    };
    let both = |flicker: bool| -> Result<String, String> {
        let (interface, world) = (heard(true, flicker)?, heard(false, flicker)?);
        check(
            interface && !world,
            format!("interface sound heard {interface}, world sound heard {world}"),
        )
    };
    vec![
        ("interface sounds play over a pause", both(false)),
        (
            "interface sounds play over a pause that ends for a frame",
            both(true),
        ),
    ]
}

/// A second of a loud square wave, heard alike everywhere.
fn tone() -> Result<Sound, String> {
    const RATE: u32 = 22_050;
    let samples: Vec<u8> = (0..RATE)
        .flat_map(|at| {
            let level: i16 = if (at / 50) % 2 == 0 { 20_000 } else { -20_000 };
            level.to_le_bytes()
        })
        .collect();
    let size = u32::try_from(samples.len()).map_err(|error| error.to_string())?;
    // A PCM WAV: 16-bit, one channel.
    let wav = [
        b"RIFF".as_slice(),
        &(36 + size).to_le_bytes(),
        b"WAVEfmt ",
        &16u32.to_le_bytes(),
        &1u16.to_le_bytes(),
        &1u16.to_le_bytes(),
        &RATE.to_le_bytes(),
        &(RATE * 2).to_le_bytes(),
        &2u16.to_le_bytes(),
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
            falloff: Falloff::Everywhere,
        },
        channel: Channel::Auto,
        follows: false,
        gain: 1.0,
        rate: 1.0,
    })
}
