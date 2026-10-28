//! The fight's sounds, from the presented snapshots alone: a view asks for
//! a sound at a moment of the game (`Cues`), and the mixer (`audio`)
//! starts it when the view's clock reaches that moment, placed for the
//! camera as JA places a sound for its view; the player's own sounds are
//! not placed. A window plays to the sound device. A headless run captures
//! the mix on the game's clock, from 0 ms, to `audio.wav` in its run
//! folder, so `record`'s videos have their sound.
//!
//! Sounds go by the names JA's code gives them
//! (`sound/weapons/saber/saberhit1.wav`); one that is not there as a WAV is
//! read as the MP3 of the same name, as JA reads it (most of its sounds are
//! MP3s).

use std::collections::{BTreeMap, BTreeSet};

use audio::wav::WavFile;
use audio::{
    Bed, Bus, CAPTURE_RATE, Channel, Clip, Emitter, Falloff, Listener, Mixer, Output, Sound, Stream,
};
use bevy::prelude::*;
use console::{DumpDue, DumpFacts, GameClock, RunFolder};
use fmt_odyssey::ResType;
use sim::{Seat, SimId};
use store::{Key, Store};
use vfs::resolver::Resolver;

use crate::lifecycle_view::SceneCamera;
use crate::scene::Headless;
use crate::sim_host::{OwnSeat, PresentedSnapshot, Waited, shown_ms};
use crate::viewer_ja::Files;

const UNIT: f32 = asset_ja::UNIT;

/// The sounds the views ask for, decoded once, by name, and those that
/// could not be read.
#[derive(Default)]
pub struct Bank {
    clips: BTreeMap<String, Clip>,
    not_read: Vec<String>,
}

/// Reads and decodes `names`; those not found or not decoded go to the
/// notes.
pub fn load(
    files: &Files,
    names: impl IntoIterator<Item = impl AsRef<str>>,
    notes: &mut Vec<String>,
) -> Bank {
    let mut bank = Bank::default();
    let store = Store::open();
    let names: BTreeSet<String> = names
        .into_iter()
        .map(|name| name.as_ref().to_ascii_lowercase())
        .collect();
    for name in names {
        let bytes = files.read(&name).or_else(|| {
            let stem = name.strip_suffix(".wav")?;
            files.read(&format!("{stem}.mp3"))
        });
        let clip = bytes.map(|bytes| decoded(store.as_ref(), &bytes));
        bank.put(name, clip);
    }
    if !bank.not_read.is_empty() {
        notes.push(format!("sounds not read: {}", bank.not_read.join(", ")));
    }
    bank
}

/// A KOTOR sound's file by resref: a WAV of the install's layers, else of
/// a stream folder (music, sounds, voices), as the games look them up.
pub fn kotor_file(resolver: &Resolver, resref: &str) -> Option<Vec<u8>> {
    let wav = ResType::WAV;
    let location = resolver.find(resref, wav).map(|found| found.location);
    location
        .or_else(|| {
            resolver
                .streams
                .iter()
                .find_map(|layer| layer.resources.get(&(resref.to_ascii_lowercase(), wav)))
        })?
        .read()
        .ok()
}

impl Bank {
    /// KOTOR 1's sounds by resref (WAVs), named `k1/<resref>`.
    pub fn load_kotor(
        &mut self,
        resolver: Option<&Resolver>,
        resrefs: &[&str],
        notes: &mut Vec<String>,
    ) {
        let lost = self.not_read.len();
        for resref in resrefs {
            let clip = resolver
                .and_then(|r| kotor_file(r, resref))
                .map(|bytes| Clip::kotor(&bytes));
            self.put(format!("k1/{}", resref.to_ascii_lowercase()), clip);
        }
        if self.not_read.len() > lost {
            notes.push(format!(
                "KOTOR sounds not read: {}",
                self.not_read[lost..].join(", ")
            ));
        }
    }

    fn put(&mut self, name: String, clip: Option<Result<Clip, String>>) {
        match clip {
            Some(Ok(clip)) => {
                self.clips.insert(name, clip);
            }
            Some(Err(error)) => self.not_read.push(format!("{name} ({error})")),
            None => self.not_read.push(name),
        }
    }
}

/// `bytes` decoded, from the store when decoded before (MP3s took about a
/// millisecond each, a hundred and more of them a run), keyed by the
/// bytes themselves.
fn decoded(store: Option<&Store>, bytes: &[u8]) -> Result<Clip, String> {
    let Some(store) = store else {
        return Clip::decode(bytes);
    };
    let key = Key::of(&format!("clip/{}", Clip::VERSION), &[bytes]);
    if let Some(clip) = store
        .get(CLIPS, &key)
        .and_then(|stored| Clip::from_encoded(&stored))
    {
        return Ok(clip);
    }
    let clip = Clip::decode(bytes)?;
    let _ = store.put(CLIPS, &key, &clip.encode());
    Ok(clip)
}

const CLIPS: &str = "import/sound/clip";

/// A sound a view asks for: by name, where (units), whose, in which of its
/// slots, whether it follows its character, at which moment (the view's
/// ms), how it falls off, at what gain and speed (`audio::Sound`).
pub struct Cue {
    pub name: String,
    pub at: Vec3,
    pub entity: Option<SimId>,
    pub channel: Channel,
    pub follows: bool,
    pub ms: f32,
    pub falloff: Falloff,
    pub gain: f32,
    pub rate: f64,
}

impl Cue {
    /// A sound of nobody's, where it began.
    pub fn at(name: String, at: Vec3, ms: f32) -> Cue {
        Cue {
            name,
            at,
            entity: None,
            channel: Channel::Auto,
            follows: false,
            ms,
            falloff: Falloff::Normal,
            gain: 1.0,
            rate: 1.0,
        }
    }
}

/// The sounds asked for since the last frame.
#[derive(Resource, Default)]
pub struct Cues(pub Vec<Cue>);

/// A loop a view keeps playing this frame, by name, where (units), how it
/// falls off and at what gain.
pub struct Hold {
    pub name: String,
    pub at: Vec3,
    pub falloff: Falloff,
    pub gain: f32,
    /// How fast it plays (1 as recorded).
    pub rate: f64,
}

impl Hold {
    pub fn at(name: String, at: Vec3) -> Hold {
        Hold {
            name,
            at,
            falloff: Falloff::Normal,
            gain: 1.0,
            rate: 1.0,
        }
    }
}

/// The beds held this frame, by name, with what they play
/// (`audio::Mixer::bed`).
#[derive(Resource, Default)]
pub struct Beds(pub Vec<(String, Stream, Bed)>);

/// The loops held this frame (`audio::Mixer::hold`).
#[derive(Resource, Default)]
pub struct Loops(pub Vec<Hold>);

/// Clips a view read itself (a conversation's voiced lines), by name, for
/// the bank before this frame's cues.
#[derive(Resource, Default)]
pub struct Added(pub Vec<(String, Clip)>);

/// Clips to take out of the bank before this frame's (a module's sounds
/// once it is left).
#[derive(Resource, Default)]
pub struct Forget(pub Vec<String>);

/// Interface sounds (by bank name) to start now, heard alike everywhere,
/// whatever waits (`Mixer::play_now`); clips not yet in the bank come in
/// `Added`.
#[derive(Resource, Default)]
pub struct Now(pub Vec<String>);

/// The beds playing after the last mix (not those played to their end).
#[derive(Resource, Default)]
pub struct Sounding(pub BTreeSet<String>);

/// Characters' slots to silence now, before this frame's cues (a voiced
/// line cut short).
#[derive(Resource, Default)]
pub struct Hush(pub Vec<(SimId, Channel)>);

/// Each part of the mix's volume (0 silent, 1 as mixed), as the options
/// set them (`settings`); all 1 where there are no options.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Volumes(pub BTreeMap<Bus, f32>);

impl Default for Volumes {
    fn default() -> Volumes {
        Volumes(Bus::ALL.into_iter().map(|bus| (bus, 1.0)).collect())
    }
}

/// The environment variable that silences every part of the mix (`kjk
/// --mute` sets it), the settings' volumes untouched.
pub const MUTE: &str = "KJK_MUTE";

fn muted() -> bool {
    static MUTED: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var_os(MUTE).is_some_and(|value| value != "0"));
    *MUTED
}

/// Its systems; a session comes in with `install`.
pub struct SoundPlugin;

/// The mix goes by the scene's clock where no game runs (a viewer: the
/// weather's thunder), so a headless viewer's `audio.wav` keeps step with
/// its `record`.
#[derive(Resource)]
pub struct SceneClock;

/// The menu's mixer (`SoundPlugin::dormant`), on a window's device; none
/// headless (what the views ask for is let go each frame). A run that put
/// its own in first keeps it.
fn install_menu(app: &mut World) {
    if app.contains_resource::<Mixing>() {
        return;
    }
    let mixer = (!app.resource::<Headless>().0)
        .then(|| {
            Mixer::new(Output::Device)
                .inspect_err(|error| warn!(target: "sound", "no sound: {error}"))
                .ok()
        })
        .flatten();
    app.insert_resource(Mixing {
        mixer,
        bank: Bank::default(),
        capture: None,
        asked: BTreeMap::new(),
        unknown: BTreeSet::new(),
        cued: Default::default(),
    });
}

/// A session's sounds, put into `app`: the mixer opens on the window's
/// device (headless: captured to the run's `audio.wav`).
pub fn install(app: &mut World, bank: Bank) {
    let headless = app.resource::<Headless>().0;
    let folder = app.resource::<RunFolder>().0.clone();
    let output = if headless {
        Output::Capture
    } else {
        Output::Device
    };
    let mixer = Mixer::new(output)
        .inspect_err(|error| warn!(target: "sound", "no sound: {error}"))
        .ok();
    let capture = (mixer.is_some() && headless)
        .then(|| WavFile::create(&folder.join(console::AUDIO), CAPTURE_RATE))
        .and_then(|made| {
            made.inspect_err(|error| warn!(target: "sound", "no {}: {error}", console::AUDIO))
                .ok()
        });
    app.insert_resource(Mixing {
        mixer,
        bank,
        capture,
        asked: BTreeMap::new(),
        unknown: BTreeSet::new(),
        cued: Default::default(),
    });
}

#[derive(Resource)]
struct Mixing {
    /// `None` when no sound can be made (no device).
    mixer: Option<Mixer>,
    bank: Bank,
    capture: Option<WavFile>,
    /// Sounds the views asked for, by name without its number.
    asked: BTreeMap<String, usize>,
    /// Names asked for that the bank has not.
    unknown: BTreeSet<String>,
    /// The last `CUED_KEPT` sounds started: when (the view's ms) and which,
    /// for dumps.
    cued: std::collections::VecDeque<(f32, String)>,
}

/// How many of the sounds last started a dump lists (`Mixing::cued`).
const CUED_KEPT: usize = 256;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cues>()
            .init_resource::<Loops>()
            .init_resource::<Beds>()
            .init_resource::<Forget>()
            .init_resource::<Sounding>()
            .init_resource::<Added>()
            .init_resource::<Hush>()
            .init_resource::<Now>()
            .init_resource::<Volumes>()
            // After the views have asked, in `PostUpdate`.
            .add_systems(Last, (clock, mix).run_if(resource_exists::<Mixing>));
        install_menu(app.world_mut());
    }
}

/// The game's time the view shows, for `film`.
fn clock(
    presented: Option<Res<PresentedSnapshot>>,
    waited: Option<Res<Waited>>,
    mut clock: ResMut<GameClock>,
) {
    clock.0 = presented.map(|presented| shown_ms(&presented.0, waited.as_deref()));
}

#[allow(clippy::too_many_arguments)]
fn mix(
    presented: Option<Res<PresentedSnapshot>>,
    time: Res<Time<Real>>,
    seat: Option<Res<OwnSeat>>,
    mut mixing: ResMut<Mixing>,
    (mut cues, mut loops, mut beds): (ResMut<Cues>, ResMut<Loops>, ResMut<Beds>),
    (mut added, mut hush, mut forget, mut sounding): (
        ResMut<Added>,
        ResMut<Hush>,
        ResMut<Forget>,
        ResMut<Sounding>,
    ),
    mut heard_now: ResMut<Now>,
    cameras: Query<&GlobalTransform, SceneCamera>,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
    (waited, showing): (Option<Res<Waited>>, Option<Res<crate::movie_view::Showing>>),
    frozen: Option<Res<crate::sim_host::Frozen>>,
    volumes: Res<Volumes>,
    (scene_clock, scene_time, held): (
        Option<Res<SceneClock>>,
        Res<Time>,
        Option<Res<console::AnimationTime>>,
    ),
) {
    let mixing = &mut *mixing;
    for name in forget.0.drain(..) {
        mixing.bank.clips.remove(&name);
    }
    let Some(mixer) = &mut mixing.mixer else {
        added.0.clear();
        cues.0.clear();
        loops.0.clear();
        beds.0.clear();
        hush.0.clear();
        heard_now.0.clear();
        return;
    };
    mixing.bank.clips.extend(added.0.drain(..));
    let muted = muted();
    for (bus, volume) in &volumes.0 {
        mixer.set_volume(*bus, if muted { 0.0 } else { *volume });
    }
    for (who, channel) in hush.0.drain(..) {
        mixer.stop(who.0, channel);
    }
    // The game's time and what the world waited for movies and breaks; a
    // viewer's, its scene's; the menu's, real time. A movie up, or a break,
    // pauses every sound but the movie's (and the beds).
    mixer.hush(showing.is_some_and(|showing| showing.0) || frozen.is_some_and(|frozen| frozen.0));
    let waited_ms = waited.as_deref().map_or(0.0, |waited| waited.ms);
    let now = match (&presented, scene_clock.and(held)) {
        (Some(presented), _) => shown_ms(&presented.0, waited.as_deref()),
        (None, Some(held)) => {
            f64::from(crate::scene::animation_seconds(&scene_time, &held)) * 1000.0
        }
        (None, None) => time.elapsed_secs_f64() * 1000.0,
    };
    let current = presented.as_ref().map(|presented| presented.0.current());
    if let Some(camera) = cameras.iter().next() {
        let eye = crate::scene_fx::eye_of(camera);
        let seat = seat.map_or(Seat(0), |seat| seat.0);
        mixer.listen(Listener {
            origin: eye.eye,
            right: -eye.left,
            entity: current
                .and_then(|current| crate::cast::me(current, seat))
                .map(|id| id.0),
        });
    }
    for entity in current.iter().flat_map(|current| &current.entities) {
        mixer.place(entity.id.0, Vec3::from_array(entity.position) / UNIT);
    }
    for cue in cues.0.drain(..) {
        let Some(clip) = mixing.bank.clips.get(&cue.name) else {
            mixing.unknown.insert(cue.name);
            continue;
        };
        let sound = Sound {
            clip: clip.clone(),
            emitter: Emitter {
                at: cue.at,
                entity: cue.entity.map(|id| id.0),
                falloff: cue.falloff,
            },
            channel: cue.channel,
            follows: cue.follows,
            gain: cue.gain,
            rate: cue.rate,
        };
        mixer.play_at(f64::from(cue.ms) + waited_ms, sound);
        let stem = cue
            .name
            .rsplit_once('.')
            .map_or(cue.name.as_str(), |(stem, _)| stem);
        let stem = stem.trim_end_matches(char::is_numeric);
        match mixing.asked.get_mut(stem) {
            Some(count) => *count += 1,
            None => {
                mixing.asked.insert(stem.to_string(), 1);
            }
        }
        if mixing.cued.len() == CUED_KEPT {
            mixing.cued.pop_front();
        }
        mixing.cued.push_back((cue.ms, cue.name));
    }
    for name in heard_now.0.drain(..) {
        let Some(clip) = mixing.bank.clips.get(&name) else {
            mixing.unknown.insert(name);
            continue;
        };
        mixer.play_now(Sound {
            clip: clip.clone(),
            emitter: Emitter {
                at: Vec3::ZERO,
                entity: None,
                falloff: Falloff::Everywhere,
            },
            channel: Channel::Auto,
            follows: false,
            gain: 1.0,
            rate: 1.0,
        });
        if mixing.cued.len() == CUED_KEPT {
            mixing.cued.pop_front();
        }
        mixing.cued.push_back((now as f32, name));
    }
    for hold in loops.0.drain(..) {
        match mixing.bank.clips.get(&hold.name) {
            Some(clip) => mixer.hold(
                &hold.name,
                clip,
                (hold.at, hold.falloff, hold.gain, hold.rate),
            ),
            None => {
                mixing.unknown.insert(hold.name);
            }
        }
    }
    for (name, stream, bed) in beds.0.drain(..) {
        mixer.bed(name, stream, bed);
    }
    if let Some(capture) = &mut mixing.capture {
        // The file starts at the game's 0 ms.
        if mixer.clock().is_none() {
            let frames = (now * f64::from(CAPTURE_RATE) / 1000.0).round() as usize;
            write(capture, &vec![0.0; frames * 2]);
        }
        write(capture, &mixer.advance(now));
    } else {
        mixer.advance(now);
    }
    sounding.0 = mixer.beds().map(String::from).collect();
    // Published for a dump only: the last sounds started are many strings.
    if !dump_due.0 {
        return;
    }
    facts.0.insert(
        "sound".into(),
        serde_json::json!({
            "output": format!("{:?}", mixer.output()),
            "clock_ms": mixer.clock(),
            "voices": mixer.voices().0,
            "loops": mixer.voices().1,
            "beds": mixer.beds().collect::<Vec<_>>(),
            "asked": mixing.asked,
            "cued": mixing.cued,
            "not_read": mixing.bank.not_read,
            "started": mixer.counts().0,
            "unheard": mixer.counts().1,
            "listener": mixer.listener().origin.to_array(),
            "captured_s": mixing.capture.as_ref().map(WavFile::seconds),
            "unknown": mixing.unknown,
        }),
    );
}

fn write(capture: &mut WavFile, samples: &[f32]) {
    if !samples.is_empty()
        && let Err(error) = capture.append(samples)
    {
        warn!(target: "sound", "{}: {error}", console::AUDIO);
    }
}
