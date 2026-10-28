use std::collections::BTreeMap;
use std::sync::Mutex;

use glam::Vec3;
use std::time::Duration;

use kira::backend::cpal::{CpalBackend, CpalBackendSettings};
use kira::backend::{Backend, Renderer};
use kira::effect::compressor::CompressorBuilder;
use kira::effect::volume_control::VolumeControlBuilder;
use kira::sound::PlaybackState;
use kira::sound::static_sound::StaticSoundHandle;
use kira::track::{MainTrackBuilder, TrackBuilder, TrackHandle};
use kira::{
    AudioManager, AudioManagerSettings, Capacities, Decibels, Panning, PlaybackRate, Tween,
};

use crate::clip::Clip;
use crate::spatial::{self, Emitter, Falloff, Listener};
use crate::stream::{BedData, BedHandle, Stream};

/// The capture's sample rate.
pub const CAPTURE_RATE: u32 = 44_100;
/// Sounds playing at once, past which the oldest gives way (the game's
/// mixer channels).
const MOST_VOICES: usize = 32;
/// The whole mix's volume (the game's default `s_volume`).
const MASTER: f32 = 0.5;
/// Past this (dB, after `MASTER`) the mix is held down, fast, rather than
/// clipped: a fight's sounds over its music add up past full.
const LIMIT: f64 = -3.0;

/// Where the mix goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// The default sound device, as it plays.
    Device,
    /// Samples handed back by `Mixer::advance`, on the game's clock.
    Capture,
}

/// The mix's parts the options' sliders turn up and down (KOTOR's music,
/// voice-over, sound effects and movie volumes), each a track of its own
/// into the whole mix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bus {
    Music,
    Voice,
    Effects,
    Movie,
}

impl Bus {
    pub const ALL: [Bus; 4] = [Bus::Music, Bus::Voice, Bus::Effects, Bus::Movie];
}

/// A character's sound slots: a new sound in a slot cuts off the one
/// there (any voice slot cuts off another), except `Auto`, which only
/// adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Auto,
    Weapon,
    Voice,
    Item,
    Body,
}

impl Channel {
    fn cuts_off(self, other: Channel) -> bool {
        self != Channel::Auto && self == other
    }
}

/// One sound to play: the clip, from where, in which slot, whether it
/// follows its character (`Mixer::place`) or stays where it began, at what
/// gain (1 as made) and speed (1 as made; faster is higher).
#[derive(Clone)]
pub struct Sound {
    pub clip: Clip,
    pub emitter: Emitter,
    pub channel: Channel,
    pub follows: bool,
    pub gain: f32,
    pub rate: f64,
}

/// How a bed (`Mixer::bed`) plays: its part of the mix, its gain, whether
/// it loops, and how long it takes to fade in, out, or to a new gain (ms).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bed {
    pub bus: Bus,
    pub gain: f32,
    pub looping: bool,
    pub fade: f64,
}

/// A loop's holder: where it has it, how it falls off there and at what
/// gain.
pub type Holder = (Vec3, Falloff, f32, f64);

/// A bed playing (or played to its end, while still held).
struct Playing {
    handle: BedHandle,
    bed: Bed,
}

struct Voice {
    handle: StaticSoundHandle,
    emitter: Emitter,
    channel: Channel,
    follows: bool,
    gain: f32,
    /// The order it began in.
    started: u64,
}

/// kira's output: the device's backend, or the renderer the capture
/// drives itself (behind a lock only so the mixer can be shared).
enum Backing {
    Device(CpalBackend),
    Capture(Option<Box<Mutex<Renderer>>>),
}

impl Backend for Backing {
    type Settings = Output;
    type Error = String;

    fn setup(output: Output, buffer: usize) -> Result<(Backing, u32), String> {
        match output {
            Output::Device => CpalBackend::setup(CpalBackendSettings::default(), buffer)
                .map(|(backend, rate)| (Backing::Device(backend), rate))
                .map_err(|error| error.to_string()),
            Output::Capture => Ok((Backing::Capture(None), CAPTURE_RATE)),
        }
    }

    fn start(&mut self, renderer: Renderer) -> Result<(), String> {
        match self {
            Backing::Device(backend) => backend.start(renderer).map_err(|error| error.to_string()),
            Backing::Capture(slot) => {
                *slot = Some(Box::new(Mutex::new(renderer)));
                Ok(())
            }
        }
    }
}

/// The view's sounds on the game's clock (ms): each plays when the clock
/// reaches its time, and every voice is placed again for the listener as
/// the clock moves.
pub struct Mixer {
    manager: AudioManager<Backing>,
    output: Output,
    listener: Listener,
    voices: Vec<Voice>,
    /// Sounds waiting for their time, in the order they were asked for.
    queue: Vec<(f64, Sound)>,
    /// The clock: how far the mix has come (`None` before the first
    /// `advance`).
    clock: Option<f64>,
    started: u64,
    /// Sounds that began out of earshot, never heard.
    unheard: u64,
    /// The loops held since the last `advance`, by name: the clip, and
    /// each holder's.
    held: BTreeMap<String, (Clip, Vec<Holder>)>,
    /// The loops playing, by name.
    loops: BTreeMap<String, StaticSoundHandle>,
    /// The beds held since the last `advance`, and those playing, by name.
    held_beds: BTreeMap<String, (Stream, Bed)>,
    beds: BTreeMap<String, Playing>,
    /// Since when (the clock, ms) every sound but the beds is paused
    /// (`hush`).
    hushed: Option<f64>,
    /// Each part's track and its volume (1 as mixed).
    buses: BTreeMap<Bus, (TrackHandle, f32)>,
}

impl Mixer {
    pub fn new(output: Output) -> Result<Mixer, String> {
        let main_track_builder = MainTrackBuilder::default()
            .with_effect(VolumeControlBuilder::new(decibels(MASTER)))
            .with_effect(
                CompressorBuilder::new()
                    .threshold(LIMIT)
                    .ratio(20.0)
                    .attack_duration(Duration::from_micros(100))
                    .release_duration(Duration::from_millis(150)),
            );
        let mut manager = AudioManager::new(AudioManagerSettings {
            capacities: Capacities::default(),
            main_track_builder,
            internal_buffer_size: 128,
            backend_settings: output,
        })?;
        let buses = Bus::ALL
            .into_iter()
            .map(|bus| {
                let track = manager
                    .add_sub_track(TrackBuilder::new())
                    .map_err(|error| error.to_string())?;
                Ok((bus, (track, 1.0)))
            })
            .collect::<Result<_, String>>()?;
        Ok(Mixer {
            manager,
            output,
            listener: Listener::default(),
            voices: Vec::new(),
            queue: Vec::new(),
            clock: None,
            started: 0,
            unheard: 0,
            held: BTreeMap::new(),
            loops: BTreeMap::new(),
            held_beds: BTreeMap::new(),
            beds: BTreeMap::new(),
            hushed: None,
            buses,
        })
    }

    /// `bus` at `volume` (0 silent, 1 as mixed), glided to over 50 ms.
    pub fn set_volume(&mut self, bus: Bus, volume: f32) {
        let (track, was) = self.buses.get_mut(&bus).expect("every bus has a track");
        if *was != volume {
            *was = volume;
            track.set_volume(decibels(volume), fade(50.0));
        }
    }

    fn track(&mut self, bus: Bus) -> &mut TrackHandle {
        &mut self.buses.get_mut(&bus).expect("every bus has a track").0
    }

    pub fn output(&self) -> Output {
        self.output
    }

    /// The clock (ms), once it runs.
    pub fn clock(&self) -> Option<f64> {
        self.clock
    }

    /// Sounds playing, and loops.
    pub fn voices(&self) -> (usize, usize) {
        (self.voices.len(), self.loops.len())
    }

    /// The beds playing (not those played to their end), by name.
    pub fn beds(&self) -> impl Iterator<Item = &str> {
        self.beds
            .iter()
            .filter(|(_, playing)| !playing.handle.stopped())
            .map(|(name, _)| name.as_str())
    }

    /// Sounds begun, and of the others those out of earshot.
    pub fn counts(&self) -> (u64, u64) {
        (self.started, self.unheard)
    }

    pub fn listener(&self) -> &Listener {
        &self.listener
    }

    /// Plays `sound` when the clock reaches `ms` (at once if it is past).
    /// Asked for while hushed, it is placed as if asked before the pause,
    /// which then moves it on with the rest.
    pub fn play_at(&mut self, ms: f64, sound: Sound) {
        let paused = self
            .hushed
            .map_or(0.0, |since| self.clock.unwrap_or(since) - since);
        self.queue.push((ms - paused, sound));
    }

    /// Starts `sound` now, whatever waits (a movie up, a break): the
    /// interface's sounds, which KOTOR's menus make over a paused world.
    pub fn play_now(&mut self, sound: Sound) {
        self.start(sound);
    }

    /// Where `entity` is now: its sounds that follow it go with it.
    pub fn place(&mut self, entity: u32, at: Vec3) {
        for voice in &mut self.voices {
            if voice.follows && voice.emitter.entity == Some(entity) {
                voice.emitter.at = at;
            }
        }
    }

    /// Keeps the loop `name` (`clip`) playing at `at` until the next
    /// `advance`. Loops are held afresh for every advance, as the game makes
    /// them every frame; the holders of one name play as one sound, each
    /// ear's gains summed (at most full), at the first holder's speed, in
    /// step with the clock wherever it starts. A loop is never its holder's
    /// own, so never unplaced.
    pub fn hold(&mut self, name: &str, clip: &Clip, at: Holder) {
        self.held
            .entry(name.to_string())
            .or_insert_with(|| (clip.clone(), Vec::new()))
            .1
            .push(at);
    }

    /// Keeps the bed `name` (`stream`) playing until the next `advance`,
    /// heard alike in both ears wherever the listener is (music, ambience,
    /// a movie's sound). A bed begun fades in from silence, one no longer
    /// held fades out, a new gain is glided to, each over the bed's fade.
    /// One played to its end that is not looping stays silent while it is
    /// held: held again after a break, it begins anew.
    pub fn bed(&mut self, name: String, stream: Stream, bed: Bed) {
        self.held_beds.insert(name, (stream, bed));
    }

    /// Pauses every sound but the beds (a movie is up: the world waits
    /// for it), or lets them go on where they were. Sounds waiting for
    /// their time wait the pause out too: they keep their place after what
    /// was playing.
    pub fn hush(&mut self, hush: bool) {
        let clock = self.clock.unwrap_or(0.0);
        match (hush, self.hushed) {
            (true, None) => {
                self.hushed = Some(clock);
                let handles = self.voices.iter_mut().map(|voice| &mut voice.handle);
                for handle in handles.chain(self.loops.values_mut()) {
                    handle.pause(Tween::default());
                }
            }
            (false, Some(since)) => {
                self.hushed = None;
                for (at, _) in &mut self.queue {
                    *at += clock - since;
                }
                let handles = self.voices.iter_mut().map(|voice| &mut voice.handle);
                for handle in handles.chain(self.loops.values_mut()) {
                    handle.resume(Tween::default());
                }
            }
            _ => {}
        }
    }

    /// Stops `entity`'s sound in `channel` now, and any waiting to start
    /// there (a line cut short).
    pub fn stop(&mut self, entity: u32, channel: Channel) {
        let theirs =
            |emitter: &Emitter, slot: Channel| emitter.entity == Some(entity) && slot == channel;
        self.queue
            .retain(|(_, sound)| !theirs(&sound.emitter, sound.channel));
        self.voices.retain_mut(|voice| {
            let stop = theirs(&voice.emitter, voice.channel);
            if stop {
                voice.handle.stop(Tween::default());
            }
            !stop
        });
    }

    /// Who hears from now on.
    pub fn listen(&mut self, listener: Listener) {
        self.listener = listener;
    }

    /// Moves the clock to `ms`: the sounds due start at their times, and
    /// every voice is placed for the listener. A capture gives back the
    /// mix made on the way (interleaved left and right, `CAPTURE_RATE`).
    pub fn advance(&mut self, ms: f64) -> Vec<f32> {
        let mut mixed = Vec::new();
        let clock = *self.clock.get_or_insert(ms);
        let mut due = Vec::new();
        let hushed = self.hushed.is_some();
        self.queue.retain(|(at, sound)| {
            let now = *at <= ms && !hushed;
            if now {
                due.push((at.max(clock), sound.clone()));
            }
            !now
        });
        due.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (at, sound) in due {
            self.render_to(at, &mut mixed);
            self.start(sound);
        }
        if hushed {
            self.held.clear();
        } else {
            self.respatialize();
            self.keep_loops();
        }
        self.keep_beds();
        self.render_to(ms.max(clock), &mut mixed);
        mixed
    }

    /// The loops held since the last advance play, placed; the others stop.
    fn keep_loops(&mut self) {
        let held = std::mem::take(&mut self.held);
        let clock = self.clock.expect("set by advance") / 1000.0;
        self.loops.retain(|name, handle| {
            let kept = held.contains_key(name);
            if !kept {
                handle.stop(Tween::default());
            }
            kept
        });
        for (name, (clip, places)) in held {
            let (left, right) = places
                .iter()
                .map(|&(at, falloff, gain, _)| {
                    let emitter = Emitter {
                        at,
                        entity: None,
                        falloff,
                    };
                    let (left, right) = spatial::gains(&self.listener, &emitter);
                    (left * gain, right * gain)
                })
                .fold((0.0f32, 0.0f32), |(l, r), (left, right)| {
                    (l + left, r + right)
                });
            let (volume, panning) = stereo(left.min(1.0), right.min(1.0));
            let rate = places.first().map_or(1.0, |&(.., rate)| rate);
            if let Some(handle) = self.loops.get_mut(&name) {
                handle.set_volume(volume, Tween::default());
                handle.set_panning(panning, Tween::default());
                handle.set_playback_rate(PlaybackRate(rate), Tween::default());
                continue;
            }
            let data = clip
                .0
                .loop_region(..)
                .start_position(clock % clip.seconds().max(f64::EPSILON))
                .volume(volume)
                .panning(panning)
                .playback_rate(PlaybackRate(rate));
            if let Ok(handle) = self.track(Bus::Effects).play(data) {
                self.loops.insert(name, handle);
            }
        }
    }

    /// The beds held since the last advance play; the others fade out.
    fn keep_beds(&mut self) {
        let held = std::mem::take(&mut self.held_beds);
        self.beds.retain(|name, playing| {
            let kept = held.contains_key(name);
            if !kept {
                playing.handle.stop(fade(playing.bed.fade));
            }
            kept
        });
        for (name, (stream, bed)) in held {
            if let Some(playing) = self.beds.get_mut(&name) {
                if playing.bed.gain != bed.gain {
                    playing
                        .handle
                        .set_volume(decibels(bed.gain), fade(bed.fade));
                }
                playing.bed = bed;
                continue;
            }
            let data = BedData {
                stream,
                volume: decibels(bed.gain),
                fade_in: fade(bed.fade),
                looping: bed.looping,
                inline: self.output == Output::Capture,
            };
            if let Ok(handle) = self.track(bed.bus).play(data) {
                self.beds.insert(name, Playing { handle, bed });
            }
        }
    }

    fn render_to(&mut self, ms: f64, mixed: &mut Vec<f32>) {
        let clock = self.clock.expect("set by advance");
        if ms <= clock {
            return;
        }
        if let Backing::Capture(Some(renderer)) = self.manager.backend_mut() {
            let renderer = renderer.get_mut().expect("the renderer is only used here");
            let rate = f64::from(CAPTURE_RATE);
            let frames = ((ms * rate / 1000.0).round() - (clock * rate / 1000.0).round()) as usize;
            let begin = mixed.len();
            mixed.resize(begin + frames * 2, 0.0);
            renderer.on_start_processing();
            renderer.process(&mut mixed[begin..], 2);
        }
        self.clock = Some(ms);
    }

    fn start(&mut self, sound: Sound) {
        let (left, right) = spatial::gains(&self.listener, &sound.emitter);
        if left == 0.0 && right == 0.0 {
            self.unheard += 1;
            return;
        }
        let own = sound.emitter.entity.is_some() && sound.emitter.entity == self.listener.entity;
        let taken = self.voices.iter().position(|voice| {
            voice.emitter.entity == sound.emitter.entity && sound.channel.cuts_off(voice.channel)
        });
        let freed =
            taken.or_else(|| (self.voices.len() >= MOST_VOICES).then(|| self.oldest(own))?);
        if let Some(at) = freed {
            self.voices.swap_remove(at).handle.stop(Tween::default());
        } else if self.voices.len() >= MOST_VOICES {
            return;
        }
        let (volume, panning) = stereo(left * sound.gain, right * sound.gain);
        let data = sound
            .clip
            .0
            .clone()
            .volume(volume)
            .panning(panning)
            .playback_rate(PlaybackRate(sound.rate));
        let bus = match sound.channel {
            Channel::Voice => Bus::Voice,
            _ => Bus::Effects,
        };
        let Ok(handle) = self.track(bus).play(data) else {
            return;
        };
        self.started += 1;
        self.voices.push(Voice {
            handle,
            emitter: sound.emitter,
            channel: sound.channel,
            follows: sound.follows,
            gain: sound.gain,
            started: self.started,
        });
    }

    /// The voice that gives way to a new one: the oldest, but never the
    /// listener's own for another's.
    fn oldest(&self, own: bool) -> Option<usize> {
        self.voices
            .iter()
            .enumerate()
            .filter(|(_, voice)| {
                own || voice.emitter.entity.is_none()
                    || voice.emitter.entity != self.listener.entity
            })
            .min_by_key(|(_, voice)| voice.started)
            .map(|(at, _)| at)
    }

    /// Every voice's ears for where it and the listener are now; a voice
    /// gone silent (out of earshot, but not a voice's) stops.
    fn respatialize(&mut self) {
        let listener = self.listener;
        self.voices.retain_mut(|voice| {
            if voice.handle.state() == PlaybackState::Stopped {
                return false;
            }
            let (left, right) = spatial::gains(&listener, &voice.emitter);
            if left == 0.0 && right == 0.0 && voice.channel != Channel::Voice {
                voice.handle.stop(Tween::default());
                return false;
            }
            let (volume, panning) = stereo(left * voice.gain, right * voice.gain);
            voice.handle.set_volume(volume, Tween::default());
            voice.handle.set_panning(panning, Tween::default());
            true
        });
    }
}

/// kira's volume and panning for the ears' gains: its panning keeps the
/// power (both ears at 1 in the middle), so the volume carries the rest.
fn stereo(left: f32, right: f32) -> (Decibels, Panning) {
    let power = left * left + right * right;
    if power == 0.0 {
        return (Decibels::SILENCE, Panning::CENTER);
    }
    let towards_right = right * right / power;
    (
        decibels((power / 2.0).sqrt()),
        Panning(towards_right * 2.0 - 1.0),
    )
}

/// A straight glide over `ms`.
fn fade(ms: f64) -> Tween {
    Tween {
        duration: Duration::from_secs_f64(ms.max(0.0) / 1000.0),
        ..Tween::default()
    }
}

fn decibels(amplitude: f32) -> Decibels {
    if amplitude <= 0.0 {
        Decibels::SILENCE
    } else {
        Decibels((20.0 * amplitude.log10()).max(Decibels::SILENCE.0))
    }
}
