//! The world's own sound (I20, `docs/SOUND.md`, "What plays when"), from
//! the presented snapshots and the area's files, render side only:
//!
//! - **music**: the area's day track (its night track in an area that is
//!   night), first 20 s after the area is entered, played through, then
//!   again 20 s after it ends (`asset_odyssey::sound::Song`, the rule
//!   `world.area_music.wait_ms`);
//! - **battle music** while the area's battle flag is up
//!   (`battle_music`: a script's, or a seat's body in a fight with a foe
//!   near, until 10 s after its last blow), faded in over the music; when
//!   it drops it fades and one of its stingers plays, then the music
//!   begins again;
//! - **ambience**: the area's loop, at its volume;
//! - what scripts set (`snapshot::AreaSound`) wins over the area's: a
//!   track started or stopped, changed, its volume;
//! - **sound objects** (UTS): looping ones held where they stand, the
//!   rest one sound after another (at random or in order) with their
//!   interval, volume and pitch varied, each falling off from its near to
//!   its far distance; on or off as placed or as scripts switch them;
//! - **doors and containers**: their opened and closed sounds when the
//!   snapshot shows them open or close; one play made (remains, a script's)
//!   by its placeable appearance's (remains rummaged: `pl_corpse_open`);
//! - **footsteps**: a character's, each time its footstep cycle (`move_body`'s
//!   `footstep_cycle`, moving as it walks or runs) passes a quarter, as Quake's
//!   player steps: one of its appearance's three for the floor
//!   under it (traced: the walkmesh face's `surfacemat.2da` row), at its
//!   soles; a rolling or hovering droid's sound held while it moves. One
//!   standing still steps where its KOTOR clip's own `snd_Footstep` events
//!   fall (`ClipSteps`: a cutscene's stunt, a script's clip), as the
//!   executable's walk and run clips step.
//!   Players walk with row 0 (`Soft`, a human's).
//! - **scripts' sounds** (`PlaySound`, `sim::notice::Sound`): each once,
//!   at and with the object that played it (the module's and the area's
//!   everywhere alike), as soon as its file is read.
//!
//! Before a game, the menu's music: K1's main menu `mus_theme_cult`, its
//! character creation `mus_theme_rep` (as reone and xoreos), looped. Under
//! the credits (`ui::screens::credits`), their `credits` alone, looped.
//!
//! Nothing plays while a movie does (`movie_view::Showing`). The area's
//! sound and every file it plays (its objects', its footsteps', its music,
//! battle music, stingers and ambience) are read with its scene as the
//! module loads (`Preread`, `lifecycle::Installer`), the clips decoded and
//! the beds kept coded (`Keep`), so the world's first frame plays them; a
//! swap whose scene does not carry them (an install with nothing to draw)
//! has them read on a thread of the view's own. A track a script switches to is read when first asked for.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use asset_ja::fx::Rng;
use asset_odyssey::sound::{AreaSound, Emitter, Group, Moves, Song, Tables, Voice};
use audio::{Bed, Bus, Clip, Falloff, Stream};
use bevy::prelude::*;
use sim::module::ObjectKind;
use sim::{Instances, Kind, Snapshot, Track, look, sound};
use vfs::Game;
use vfs::resolver::Resolver;

use crate::installs::Install;
use crate::scene::SceneContent;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap, view_ms};
use crate::sound::{Added, Beds, Cue, Cues, Forget, Hold, Loops, Sounding};
use collision::{Contents, Extents};

const UNIT: f32 = asset_ja::UNIT;
/// The music's gain with no area's tables read (the menu's, the credits'):
/// `Streams_Music`'s volume over `Non_Player_Chat`'s in both games'
/// `prioritygroups.2da` (64 and 90), as `group_bed` gives it.
const MENU_MUSIC_GAIN: f32 = audio::spatial::EAR_SHARE * 64.0 / 90.0;
/// A footstep's gain; how far down the floor is looked for under a
/// character's box origin, and how far above its soles that is (units);
/// the speed (units a second) below which nothing steps or rolls.
const STEP: f32 = 0.6;
const FLOOR: f32 = 64.0;
const SOLES: f32 = 24.0;
const STILL: f32 = 5.0;
/// `footstepsounds.2da`'s humanoid rows, whose steps are JA's
/// (`ja_footsteps`).
const SOFT: u8 = 0;
const HARD: u8 = 1;
/// The ground speed a step is a running one from (units a second; JA runs
/// at 250 and walks at half that or less).
const RUNNING: f32 = 180.0;
/// Players' footstep row (`Soft`).
const PLAYER_STEPS: u8 = 0;
/// How long the music and the ambience take to fade in or out, and the
/// battle music (ms).
const FADE: f64 = 1000.0;
const BATTLE_FADE: f64 = 1500.0;
/// The music played through, the battle music looped, the stinger at once.
const SONG: Bed = Bed {
    bus: Bus::Music,
    gain: MENU_MUSIC_GAIN,
    looping: false,
    fade: FADE,
};
const BATTLE: Bed = Bed {
    bus: Bus::Music,
    gain: MENU_MUSIC_GAIN,
    looping: true,
    fade: BATTLE_FADE,
};
const STINGER: Bed = Bed {
    bus: Bus::Music,
    gain: MENU_MUSIC_GAIN,
    looping: false,
    fade: 0.0,
};
/// The area's ambience, looped.
const AMBIENT: Bed = Bed {
    bus: Bus::Effects,
    gain: 0.0,
    looping: true,
    fade: FADE,
};

pub struct AmbiencePlugin;

impl Plugin for AmbiencePlugin {
    fn build(&self, app: &mut App) {
        let in_game = resource_exists::<PresentedSnapshot>.and_then(resource_exists::<SceneSwap>);
        app.init_resource::<Ambience>()
            .init_resource::<Heard>()
            .init_resource::<HeardAt>()
            .init_resource::<crate::made::Exploded>()
            .init_resource::<ClipSteps>()
            .add_systems(
                PostUpdate,
                (
                    receive,
                    crate::grunts::grunts.run_if(resource_exists::<PresentedSnapshot>),
                    play.run_if(in_game),
                    menu.run_if(not(resource_exists::<PresentedSnapshot>)),
                )
                    .chain(),
            );
    }
}

/// K1's menu music: the main menu's, and character creation's.
const MENU_MUSIC: &str = "mus_theme_cult";
const CHARGEN_MUSIC: &str = "mus_theme_rep";
/// The credits' music.
const CREDITS_MUSIC: &str = "credits";

/// `file` looped as the music, once it is read (asked for now).
fn looped(ambience: &mut Ambience, beds: &mut Beds, file: &str) {
    wanted(ambience, Keep::Bed, [file]);
    if let Some(stream) = ambience.beds.get(file) {
        beds.0.push((
            named(file),
            stream.clone(),
            Bed {
                looping: true,
                ..SONG
            },
        ));
    }
}

/// The menu's music while its screens are up and no movie is.
fn menu(
    ui: Option<Res<crate::ui_view::Ui>>,
    (showing, headless): (
        Option<Res<crate::movie_view::Showing>>,
        Res<crate::scene::Headless>,
    ),
    mut ambience: ResMut<Ambience>,
    mut beds: ResMut<Beds>,
) {
    // A headless menu is silent (`sound`).
    let Some(ui) = ui.filter(|ui| ui.stack.is_open() && !headless.0) else {
        return;
    };
    if showing.is_some_and(|showing| showing.0) {
        return;
    }
    let file = if ui.stack.screens().any(|screen| screen.0 == "chargen") {
        CHARGEN_MUSIC
    } else {
        MENU_MUSIC
    };
    ambience.worker.get_or_insert_with(Worker::start);
    looped(&mut ambience, &mut beds, file);
}

/// The footsteps KOTOR clips' own events asked for (`snd_Footstep`,
/// `clip_events`): by whom, and where its body stands (units).
#[derive(Resource, Default)]
pub struct ClipSteps(pub Vec<(sim::SimId, Vec3)>);

/// The sounds scripts played for this player (`PlaySound`), to be heard.
#[derive(Resource, Default)]
pub struct Heard(pub Vec<sim::notice::Sound>);

/// The sounds the game itself plays at a place in the world for this
/// player, each by its resref and its place (game space, metres), to be
/// heard once there. A mine that went off is heard this way
/// (`made::exploded`): the mine is gone by then, so no object carries the
/// sound.
#[derive(Resource, Default)]
pub struct HeardAt(pub Vec<(String, [f32; 3])>);

/// How long a script's sound waits for its file before it is dropped (ms):
/// one heard late is out of place.
const HEARD_WAIT: f64 = 2000.0;

/// What the view's thread is asked to read.
enum Ask {
    /// An area's sound and every file it plays now.
    Area {
        generation: u32,
        game: String,
        module: String,
        area: String,
    },
    /// The module whose files later asks read, its area's sound already
    /// read by the scene.
    Module { game: String, module: String },
    /// More files, by resref, kept alike.
    Files(Keep, Vec<String>),
    /// Placeable templates' appearances (those play made).
    Appearances(Vec<String>),
}

/// What it read: the area's sound for a swap, files, and placeable
/// templates' appearances.
enum Read {
    Area { generation: u32, area: Box<Preread> },
    Files(Vec<Got>),
    Appearances(Vec<(String, Option<u16>)>),
}

/// How a file is kept: decoded whole, a cue's or a loop's (`Clip`), or as
/// its coded bytes, a bed's (music, ambience), decoded as it plays
/// (`Stream`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Keep {
    Clip,
    Bed,
}

/// A file read, as `Keep` has it.
#[derive(Clone)]
enum Kept {
    Clip(Box<Clip>),
    Bed(Stream),
}

/// A file by resref, how it is kept, and it read (or why not).
type Got = (String, Keep, Result<Kept, String>);

/// An area's sound and every file it plays, read.
#[derive(Clone)]
pub struct Preread {
    pub module: String,
    pub area: String,
    sound: AreaSound,
    tables: Tables,
    files: Vec<Got>,
}

/// Area `area` of `module` (`resolver` has the module) read as `Preread`;
/// `None` when its GIT does not read.
pub fn preread(resolver: &Resolver, module: &str, area: &str) -> Option<Preread> {
    let source = crate::modules::ResolverSource(resolver);
    let sound = asset_odyssey::sound::area(&source, area)?;
    let tables = Tables::read(&source);
    let files = area_files(&sound, &tables)
        .into_iter()
        .map(|(name, keep)| got(resolver, name, keep))
        .collect();
    Some(Preread {
        module: module.to_string(),
        area: area.to_string(),
        sound,
        tables,
        files,
    })
}

/// What the views ask for a line of a sound set by (`line_file`):
/// `*ssf/<row>/<slot>`, a creature's sound set by its `soundset.2da` row,
/// or `*ssf/pc<portrait>/<slot>`, a player's by its portrait
/// (`creature::player_sound_set`).
const LINE_FILE: &str = "*ssf/";

/// The sound a line of a sound set plays (`LINE_FILE`'s rest).
fn line_sound(resolver: &Resolver, line: &str) -> Option<String> {
    let (who, slot) = line.split_once('/')?;
    let source = crate::modules::ResolverSource(resolver);
    let tables = &mut asset_odyssey::tables::Tables::default();
    let row = match who.strip_prefix("pc") {
        Some(portrait) => {
            asset_odyssey::creature::player_sound_set(&source, portrait.parse().ok()?, tables)?
        }
        None => who.parse().ok()?,
    };
    asset_odyssey::creature::sound_set_line(&source, row, slot.parse().ok()?, tables)
}

fn got(resolver: &Resolver, name: String, keep: Keep) -> Got {
    let read = (|| {
        let file = match name.strip_prefix(LINE_FILE) {
            Some(line) => {
                let sound = line_sound(resolver, line).ok_or("no such line")?;
                info!(target: "sound", line, %sound, "sound-set line");
                sound
            }
            None => name.clone(),
        };
        let bytes = crate::sound::kotor_file(resolver, &file).ok_or("not found")?;
        match keep {
            Keep::Clip => Clip::kotor(&bytes).map(|clip| Kept::Clip(Box::new(clip))),
            Keep::Bed => Stream::kotor(&bytes).map(Kept::Bed),
        }
    })();
    (name, keep, read)
}

/// Every file an area plays as its files set it: its objects', its
/// creatures' and the players' footsteps (clips), and its tracks (day and
/// night), the battle music's stingers among them (beds).
fn area_files(sound: &AreaSound, tables: &Tables) -> BTreeSet<(String, Keep)> {
    let tracks = &sound.tracks;
    let rows: BTreeSet<u8> = sound.step_rows().chain([PLAYER_STEPS]).collect();
    let music = [tracks.music_day, tracks.music_night, tracks.battle]
        .into_iter()
        .filter_map(|row| tables.music(row));
    let ambient = [tracks.ambient_day, tracks.ambient_night]
        .into_iter()
        .filter_map(|row| tables.ambient(row));
    let clips = sound
        .files()
        .chain(rows.iter().flat_map(|row| tables.step_files(*row)))
        .map(|file| (file, Keep::Clip));
    let beds = music
        .chain(tables.stingers(tracks.battle).iter().map(String::as_str))
        .chain(ambient)
        .map(|file| (file, Keep::Bed));
    clips
        .chain(beds)
        .map(|(file, keep)| (file.to_string(), keep))
        .collect()
}

/// The thread's channels (the receiver locked only to share the view).
struct Worker {
    ask: Sender<Ask>,
    read: Mutex<Receiver<Read>>,
}

impl Worker {
    fn start() -> Worker {
        let (ask, asked) = mpsc::channel::<Ask>();
        let (told, read) = mpsc::channel::<Read>();
        std::thread::Builder::new()
            .name("ambience".into())
            .spawn(move || {
                // The install, the process's (`installs`).
                let mut install: Option<Arc<Install>> = None;
                let mut module: Option<Resolver> = None;
                while let Ok(ask) = asked.recv() {
                    match ask {
                        Ask::Module { game, module: name } => {
                            module = open(&mut install, &game, &name);
                        }
                        Ask::Area {
                            generation,
                            game,
                            module: name,
                            area,
                        } => {
                            module = open(&mut install, &game, &name);
                            let Some(resolver) = &module else { continue };
                            match preread(resolver, &name, &area) {
                                Some(area) => {
                                    let _ = told.send(Read::Area {
                                        generation,
                                        area: Box::new(area),
                                    });
                                }
                                None => warn!(
                                    target: "sound",
                                    "{area}: no music or ambience, its GIT does not read"
                                ),
                            }
                        }
                        Ask::Files(keep, names) => {
                            // The module's, else the install's (the menu's: K1).
                            if module.is_none() && install.is_none() {
                                install = opened(Game::Kotor1);
                            }
                            let base = install.as_ref().and_then(|install| install.base().ok());
                            let Some(resolver) = module.as_ref().or(base) else {
                                continue;
                            };
                            let files = names
                                .into_iter()
                                .map(|name| got(resolver, name, keep))
                                .collect();
                            let _ = told.send(Read::Files(files));
                        }
                        Ask::Appearances(templates) => {
                            let Some(resolver) = &module else { continue };
                            let source = crate::modules::ResolverSource(resolver);
                            let read = templates
                                .into_iter()
                                .map(|template| {
                                    let appearance = asset_odyssey::sound::placeable_appearance(
                                        &source, &template,
                                    );
                                    (template, appearance)
                                })
                                .collect();
                            let _ = told.send(Read::Appearances(read));
                        }
                    }
                }
            })
            .expect("the ambience thread starts");
        Worker {
            ask,
            read: Mutex::new(read),
        }
    }
}

fn opened(game: Game) -> Option<Arc<Install>> {
    crate::installs::open(&crate::discover(), game).ok()
}

/// Module `name` of game `game` (a key), `install` made that game's first:
/// `None` when either is not found.
fn open(install: &mut Option<Arc<Install>>, game: &str, name: &str) -> Option<Resolver> {
    let game = Game::from_key(game)?;
    if install.as_ref().is_none_or(|had| had.game != game) {
        *install = opened(game);
    }
    install.as_ref()?.resolver(Some(name)).ok()
}

/// A sound object's turn: the next sound's moment (view ms), and which
/// sound comes next in order.
#[derive(Debug, Clone, Default)]
struct Turn {
    next: Option<f64>,
    index: usize,
    /// Every sound played once (one that is not continuous stops).
    done: bool,
    /// The scripts' starts it last began on (`ObjectState::plays`).
    plays: Option<u8>,
    /// When a script's fading stop began (`ObjectState::fade`).
    fading: Option<f64>,
}

#[derive(Resource, Default)]
struct Ambience {
    worker: Option<Worker>,
    /// The swap asked for, and its area once read.
    generation: Option<u32>,
    area: Option<(AreaSound, Tables)>,
    /// Clips read, by resref (with their length, s), beds read, files
    /// asked for, failed.
    clips: BTreeMap<String, f64>,
    beds: BTreeMap<String, Stream>,
    asked: BTreeMap<Keep, BTreeSet<String>>,
    failed: BTreeSet<String>,
    /// Music rows asked for that `ambientmusic.2da` lacks, each warned of
    /// once an area.
    unlisted: BTreeSet<u16>,
    /// The music's turn (its track by bed name), and the scripts' music
    /// state as last seen (`sound::*`), so a script's start is seen once.
    song: Option<Song>,
    music_state: u8,
    /// The fight's music while it lasts; then its stinger, and whether it
    /// was heard yet.
    fighting: bool,
    /// The area's battle flag (`battle_music`).
    fight: crate::battle_music::Fight,
    stinger: Option<(String, bool)>,
    turns: BTreeMap<u32, Turn>,
    /// Doors' states and containers' tenants as last seen, by object.
    moved: BTreeMap<u32, u64>,
    /// Placeable templates' appearances, by template (`None` until read,
    /// or with none).
    appearances: BTreeMap<String, Option<u16>>,
    /// Each character's footstep cycle as last seen, by id.
    cycles: BTreeMap<u32, u8>,
    /// Sounds made, for `dump`: footsteps, doors' and containers' by file.
    steps: u64,
    moves: BTreeMap<String, u32>,
    /// The rolling droids' and hovers' sounds held, by file.
    rolled: BTreeSet<String>,
    rng: Option<Rng>,
    /// What plays, for `dump`, and what played last frame (each change is
    /// logged, so a run's log and trace show the tracks).
    playing: BTreeMap<&'static str, String>,
    played: BTreeMap<&'static str, String>,
    cued: u64,
    /// Scripts' sounds waiting for their files (with when they came, view
    /// ms), and how many played.
    heard: Vec<(sim::notice::Sound, Option<[f32; 3]>, f64)>,
    scripted: u64,
}

/// The bank's name for an area's sound (and the mixer's for its beds): its
/// own, so forgetting the area's never takes a sound the session's views
/// loaded (`sound::Bank`).
fn named(resref: &str) -> String {
    format!("area/{resref}")
}

fn receive(mut ambience: ResMut<Ambience>, mut added: ResMut<Added>) {
    let ambience = &mut *ambience;
    let Some(worker) = &ambience.worker else {
        return;
    };
    let read = worker.read.lock().expect("the ambience's channel");
    let came: Vec<Read> = std::iter::from_fn(|| read.try_recv().ok()).collect();
    drop(read);
    for read in came {
        match read {
            Read::Area { generation, area } => {
                if ambience.generation == Some(generation) {
                    take(ambience, *area, &mut added);
                }
            }
            Read::Files(files) => took(ambience, files, &mut added),
            Read::Appearances(read) => ambience.appearances.extend(read),
        }
    }
}

/// An area's sound read (`Preread`) made the one playing, its files
/// taken.
fn take(ambience: &mut Ambience, read: Preread, added: &mut Added) {
    for (name, keep, _) in &read.files {
        ambience
            .asked
            .entry(*keep)
            .or_default()
            .insert(name.clone());
    }
    took(ambience, read.files, added);
    ambience.area = Some((read.sound, read.tables));
    ambience.appearances.clear();
}

/// Files read: clips into the bank, beds kept here; those that did not
/// read noted.
fn took(ambience: &mut Ambience, files: Vec<Got>, added: &mut Added) {
    for (name, keep, read) in files {
        match read {
            Ok(Kept::Clip(clip)) => {
                ambience.clips.insert(name.clone(), clip.seconds());
                added.0.push((named(&name), *clip));
            }
            Ok(Kept::Bed(stream)) => {
                ambience.beds.insert(name, stream);
            }
            Err(why) => {
                let what = match keep {
                    Keep::Bed => "music or ambience",
                    Keep::Clip => "sound",
                };
                warn!(target: "sound", "{what} {name} not played: {why}");
                ambience.failed.insert(name);
            }
        }
    }
}

/// The files to read now, kept as `keep`: those wanted and not yet read
/// or asked for.
fn wanted<'a>(ambience: &mut Ambience, keep: Keep, files: impl IntoIterator<Item = &'a str>) {
    let asked = ambience.asked.entry(keep).or_default();
    let new: Vec<String> = files
        .into_iter()
        .filter(|file| !asked.contains(*file))
        .map(String::from)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if new.is_empty() {
        return;
    }
    asked.extend(new.iter().cloned());
    if let Some(worker) = &ambience.worker {
        let _ = worker.ask.send(Ask::Files(keep, new));
    }
}

/// `bed` at the executables' gain for a sound of `group` (its
/// `prioritygroups.2da` volume) times `volume` (0 to 127), straight, as
/// they set a streamed sound's, over a conversation's voice's group's: the
/// voice plays at full (an unplaced cue, `EAR_SHARE` in each ear), so the
/// music and the ambience keep the games' balance with it.
fn group_bed(bed: Bed, tables: &Tables, group: Group, volume: u8) -> Bed {
    let voice = tables.group_gain(Group::Conversation).max(f32::EPSILON);
    Bed {
        gain: audio::spatial::EAR_SHARE * tables.group_gain(group) / voice
            * f32::from(volume.min(127))
            / 127.0,
        ..bed
    }
}

/// The row a track plays: a script's day or night, else the area's.
fn row(track: &Track, night: bool, day_row: u16, night_row: u16) -> u16 {
    match (night, track.day, track.night) {
        (true, _, Some(row)) => row,
        (true, _, None) => night_row,
        (false, Some(row), _) => row,
        (false, None, _) => day_row,
    }
}

#[allow(clippy::too_many_arguments)]
fn play(
    (swap, scene): (Res<SceneSwap>, Option<Res<SceneContent>>),
    presented: Res<PresentedSnapshot>,
    seat: Option<Res<OwnSeat>>,
    showing: Option<Res<crate::movie_view::Showing>>,
    mut ambience: ResMut<Ambience>,
    (mut beds, mut loops, mut cues, mut forget): (
        ResMut<Beds>,
        ResMut<Loops>,
        ResMut<Cues>,
        ResMut<Forget>,
    ),
    mut clip_steps: ResMut<ClipSteps>,
    mut added: ResMut<Added>,
    sounding: Res<Sounding>,
    (mut heard, mut heard_at): (ResMut<Heard>, ResMut<HeardAt>),
    mut facts: ResMut<console::DumpFacts>,
    ui: Option<Res<crate::ui_view::Ui>>,
) {
    let ambience = &mut *ambience;
    ambience.worker.get_or_insert_with(Worker::start);
    // Heard this frame or not at all.
    let asked = std::mem::take(&mut clip_steps.0);
    // A soft landing is a footstep, as in JA.
    let landed: BTreeSet<sim::SimId> = presented
        .1
        .iter()
        .flat_map(|snapshot| &snapshot.entities)
        .filter(|entity| {
            entity
                .player
                .is_some_and(|player| player.events & sim::motion::events::LAND != 0)
        })
        .map(|entity| entity.id)
        .collect();
    let module = swap.level.module().cloned();
    if ambience.generation != Some(swap.generation) {
        ambience.generation = Some(swap.generation);
        ambience.area = None;
        // The last area's music stops here, and the new area's waits the
        // music wait (a load's too).
        ambience.song = Some(Song::Entered);
        ambience.music_state = sound::AS_PLACED;
        ambience.fighting = false;
        ambience.fight = Default::default();
        ambience.stinger = None;
        ambience.turns.clear();
        ambience.moved.clear();
        ambience.cycles.clear();
        ambience.heard.clear();
        ambience.rng = Some(Rng(0x5eed ^ swap.generation as i32));
        // The last area's sounds leave the bank.
        forget.0.extend(
            std::mem::take(&mut ambience.clips)
                .into_keys()
                .map(|name| named(&name)),
        );
        ambience.beds.clear();
        ambience.asked.clear();
        ambience.failed.clear();
        ambience.unlisted.clear();
        // The scene's own read when it is this area's (a swap without a
        // new scene keeps the last one), else one asked of the thread;
        // either way the thread is told the module, as the files asked
        // later are its (a TSL creature's sound-set row read in K1's
        // table is another character's voice).
        let worker = ambience.worker.as_ref().expect("started above");
        let carried = scene
            .as_ref()
            .and_then(|scene| scene.0.sound.as_ref())
            .filter(|read| {
                module
                    .as_ref()
                    .is_some_and(|module| read.module == module.name && read.area == module.area)
            });
        if let Some(module) = &module {
            let (game, name) = (module.game.clone(), module.name.clone());
            let _ = worker.ask.send(match carried {
                Some(_) => Ask::Module { game, module: name },
                None => Ask::Area {
                    generation: swap.generation,
                    game,
                    module: name,
                    area: module.area.clone(),
                },
            });
        }
        if let Some(read) = carried {
            take(ambience, read.clone(), &mut added);
        }
    }
    let (Some(module), Some(_)) = (module, &ambience.area) else {
        heard.0.clear();
        heard_at.0.clear();
        return;
    };
    let current = presented.0.current();
    let now = view_ms(&presented.0);
    ambience.playing.clear();
    let silent = showing.is_some_and(|showing| showing.0);
    // A placed sound waits for its file like a script's. It names no
    // object: its place is its own.
    let placed = heard_at.0.drain(..).map(|(file, at)| {
        let sound = sim::notice::Sound::new(script_ncs::vm::ObjectId::INVALID.0, &file);
        (sound, Some(at), now)
    });
    let came: Vec<_> = (heard.0.drain(..).map(|sound| (sound, None, now)))
        .chain(placed)
        .collect();
    let ending = ui.is_some_and(|ui| ui.stack.has("credits"));
    if silent || ending {
        // The music begins again after the movie; an area's first wait
        // begins after it.
        if ambience.song != Some(Song::Entered) {
            ambience.song = None;
        }
        ambience.heard.clear();
        if ending && !silent {
            looped(ambience, &mut beds, CREDITS_MUSIC);
        }
    } else {
        ambience.heard.extend(came);
        let views = scene
            .as_ref()
            .map_or(&[][..], |scene| &scene.0.creature_views[..]);
        scripted(ambience, (current, views), now, &mut cues);
        let seat = seat.map_or(sim::Seat(0), |seat| seat.0);
        // A minigame's music (the ARE's `MiniGame` `Music`) over the area's.
        let minigame_music = swap
            .level
            .minigame()
            .map(|rules| rules.music.trim().to_string())
            .filter(|music| !music.is_empty());
        let instances = Instances::in_snapshot(&swap.level, current);
        let (bodies, blows) = crate::battle_music::bodies_and_blows(current, &instances);
        let moment = crate::battle_music::Moment {
            tick: current.tick,
            now_ms: current.time_ms,
            bodies: &bodies,
            blows: &blows,
            scripted: current.area_sound.battle.state,
            talking: !current.talk.conversations.is_empty(),
        };
        let standing = swap.level.module().map(|module| current.standings(module));
        let hostile = crate::battle_music::hostility(standing);
        let battle = ambience.fight.step(&moment, hostile);
        // The floors stage plays no music or ambience over its footsteps.
        if swap.level.name() != crate::floor_arena::NAME {
            tracks(
                ambience,
                current,
                battle,
                now,
                &sounding,
                &mut beds,
                minigame_music,
            );
        }
        // In a minigame the camera rides the course away from where the
        // area's sound objects stand: one not positional (the swoop's
        // engine, gears, countdown) is heard in both ears at its volume
        // wherever the camera is (xoreos and reone give such a sound no
        // place).
        let anywhere = current.minigame.is_some();
        // A co-op race: this seat hears its own run's sound objects (M10).
        let run = current
            .minigame
            .as_ref()
            .map(|course| course.run_of(seat.0));
        objects(
            ambience,
            (&instances, swap.level.module().map(|module| &**module)),
            (current, run),
            now,
            anywhere,
            (&mut loops, &mut cues),
        );
        footsteps(
            ambience,
            (&swap.level, &instances),
            current,
            (asked, &landed),
            (&mut loops, &mut cues),
        );
    }
    if ambience.playing != ambience.played {
        for (track, file) in &ambience.played {
            if ambience.playing.get(track) != Some(file) {
                info!(target: "sound", %track, %file, "stops");
            }
        }
        for (track, file) in &ambience.playing {
            if ambience.played.get(track) != Some(file) {
                info!(target: "sound", %track, %file, area = %module.area, "plays");
            }
        }
        ambience.played = ambience.playing.clone();
    }
    if facts.wanted() {
        facts.0.insert(
            "ambience".into(),
            serde_json::json!({
                "area": module.area,
                "silent": silent,
                "playing": ambience.playing,
                "fighting": ambience.fighting,
                "battle": {
                    "on": ambience.fight.on,
                    "stirred": ambience.fight.stirred,
                },
                "clips": ambience.clips.len(),
                "streams": ambience
                    .beds
                    .iter()
                    .map(|(name, stream)| (name, stream.bytes()))
                    .collect::<BTreeMap<_, _>>(),
                "failed": ambience.failed,
                "objects": ambience.turns.len(),
                "cued": ambience.cued,
                "scripted": ambience.scripted,
                "steps": ambience.steps,
                "moves": ambience.moves,
                "rolled": ambience.rolled,
            }),
        );
    }
}

/// The music, the battle music and its stinger, and the ambience.
fn tracks(
    ambience: &mut Ambience,
    current: &Snapshot,
    battle: bool,
    now: f64,
    sounding: &Sounding,
    beds: &mut Beds,
    minigame_music: Option<String>,
) {
    let Some((area, tables)) = &ambience.area else {
        return;
    };
    let scripted = &current.area_sound;
    let tracks = &area.tracks;
    let night = tracks.night;
    let music_row = row(&scripted.music, night, tracks.music_day, tracks.music_night);
    let music_file = minigame_music.or_else(|| tables.music(music_row).map(String::from));
    let battle_row = scripted.battle.day.unwrap_or(tracks.battle);
    for lost in [music_row, battle_row] {
        if !tables.music_listed(lost) && ambience.unlisted.insert(lost) {
            warn!(
                target: "sound",
                "no music: ambientmusic.2da has no row {lost} (a missing or short table)"
            );
        }
    }
    let battle_file = tables.music(battle_row).map(String::from);
    let stingers = tables.stingers(battle_row).to_vec();
    let ambient_file = tables
        .ambient(row(
            &scripted.ambient,
            night,
            tracks.ambient_day,
            tracks.ambient_night,
        ))
        .map(String::from);
    let ambient_volume = match scripted.ambient.volume {
        0 if night => tracks.ambient_night_volume,
        0 => tracks.ambient_day_volume,
        volume => volume - 1,
    };
    let [song, battle_bed, stinger_bed, ambient_bed] = [
        (SONG, Group::Music, 127),
        (BATTLE, Group::Music, 127),
        (STINGER, Group::Stingers, 127),
        (AMBIENT, Group::AreaAmbient, ambient_volume),
    ]
    .map(|(bed, group, volume)| group_bed(bed, tables, group, volume));
    // A kjk rule replaces the area's own `MusicDelay` (issue 291).
    let delay = rules::world::area_music::WAIT_MS as f64;
    let wants: Vec<String> = [&music_file, &battle_file, &ambient_file]
        .into_iter()
        .flatten()
        .chain(&stingers)
        .cloned()
        .collect();
    wanted(ambience, Keep::Bed, wants.iter().map(String::as_str));

    let fight = battle && battle_file.is_some();
    if ambience.fighting && !fight && ambience.stinger.is_none() && !stingers.is_empty() {
        let rng = ambience.rng.get_or_insert(Rng(1));
        let pick = rng.roll_between(0, stingers.len() as i32 - 1) as usize;
        ambience.stinger = Some((stingers[pick].clone(), false));
    }
    if fight {
        ambience.stinger = None;
    }
    ambience.fighting = fight;
    let mut hold = |ambience: &mut Ambience, what: &'static str, file: &str, bed: Bed| {
        if let Some(stream) = ambience.beds.get(file) {
            beds.0.push((named(file), stream.clone(), bed));
            ambience.playing.insert(what, file.to_string());
        }
    };
    if fight && let Some(file) = &battle_file {
        hold(ambience, "battle", file, battle_bed);
        ambience.song = None;
    } else if let Some((stinger, heard)) = ambience.stinger.clone() {
        // The stinger plays through, then the music begins again.
        let sounds = sounding.0.contains(&named(&stinger));
        if heard && !sounds || ambience.failed.contains(&stinger) {
            ambience.stinger = None;
        } else {
            ambience.stinger = Some((stinger.clone(), heard || sounds));
            hold(ambience, "stinger", &stinger, stinger_bed);
        }
    } else if scripted.music.state != sound::STOPPED
        && let Some(file) = &music_file
        && ambience.beds.contains_key(file)
    {
        let name = named(file);
        let heard = sounding.0.contains(&name);
        let started =
            scripted.music.state == sound::PLAYING && ambience.music_state != sound::PLAYING;
        let (turn, plays) = Song::turn(ambience.song.take(), &name, (heard, started), now, delay);
        ambience.song = Some(turn);
        ambience.music_state = scripted.music.state;
        if plays {
            hold(ambience, "music", file, song);
        }
    } else if scripted.music.state == sound::STOPPED {
        ambience.song = None;
        ambience.music_state = sound::STOPPED;
    } else if ambience.song != Some(Song::Entered) {
        // No track (or none read yet): an area's first wait begins once
        // its track is known.
        ambience.song = None;
    }
    if scripted.ambient.state != sound::STOPPED
        && let Some(file) = &ambient_file
    {
        hold(ambience, "ambient", file, ambient_bed);
    }
}

/// Scripts' sounds whose files are read, each once at and with the object
/// that played it (none: heard alike everywhere), a placed sound (`HeardAt`)
/// once at its place, the engine's locked sound
/// as the object's own, a line of a character's sound set as its own (a
/// creature's, `views`, or a player's, its persona's) on its voice; those
/// not read yet asked for, and dropped after `HEARD_WAIT`, as are those
/// that failed.
fn scripted(
    ambience: &mut Ambience,
    (current, views): (&Snapshot, &[crate::viewer::CreatureView]),
    now: f64,
    cues: &mut Cues,
) {
    let file_of = |ambience: &Ambience, sound: &sim::notice::Sound| {
        if let Some(slot) = sound.sound_set_line() {
            let who = match current.get(sim::SimId(sound.object))?.kind {
                Kind::Object(ObjectKind::Creature, index) => views
                    .iter()
                    .find(|view| view.object == index)?
                    .sound_set?
                    .to_string(),
                Kind::Avatar(seat) => {
                    let persona = current.personas.iter().find(|p| p.seat == seat)?;
                    format!("pc{}", persona.portrait)
                }
                _ => return None,
            };
            return Some(format!("{LINE_FILE}{who}/{slot}"));
        }
        let file = sound.resref();
        if file != sim::notice::LOCKED_SOUND {
            return Some(file);
        }
        let Some(Kind::Object(_, index)) = current.get(sim::SimId(sound.object)).map(|e| e.kind)
        else {
            return None;
        };
        match ambience.area.as_ref()?.0.objects.get(index as usize)? {
            Voice::Moves(moves) => moves.locked.clone(),
            _ => None,
        }
    };
    let files: Vec<String> = ambience
        .heard
        .iter()
        .filter_map(|(sound, ..)| file_of(ambience, sound))
        .collect();
    wanted(ambience, Keep::Clip, files.iter().map(String::as_str));
    let waiting = std::mem::take(&mut ambience.heard);
    for (sound, at, came) in waiting {
        let Some(file) = file_of(ambience, &sound) else {
            continue;
        };
        if !ambience.clips.contains_key(&file) {
            if !ambience.failed.contains(&file) && now < came + HEARD_WAIT {
                ambience.heard.push((sound, at, came));
            }
            continue;
        }
        if let Some(at) = at {
            cues.0.push(Cue::at(
                named(&file),
                Vec3::from_array(at) / UNIT,
                current.time_ms as f32,
            ));
            ambience.scripted += 1;
            continue;
        }
        let id = sim::SimId(sound.object);
        let channel = match sound.sound_set_line() {
            Some(_) => audio::Channel::Voice,
            None => audio::Channel::Auto,
        };
        let cue = match current.get(id) {
            Some(entity) => Cue {
                entity: Some(id),
                follows: true,
                channel,
                ..Cue::at(
                    named(&file),
                    Vec3::from_array(entity.position) / UNIT,
                    current.time_ms as f32,
                )
            },
            None => Cue {
                falloff: Falloff::Everywhere,
                ..Cue::at(named(&file), Vec3::ZERO, current.time_ms as f32)
            },
        };
        cues.0.push(cue);
        ambience.scripted += 1;
    }
}

/// The placeable appearances of the placeables play made (`current`'s,
/// `instances`: its table), by object: remains as their `bodybag.2da` row
/// has them, another as its template's (asked for until read); the
/// sounds of each asked for.
fn made_placeables(
    ambience: &mut Ambience,
    (instances, module): (&Instances, Option<&sim::module::ModuleData>),
    current: &Snapshot,
) -> BTreeMap<u32, u16> {
    let mut made = BTreeMap::new();
    let mut ask = Vec::new();
    for entity in &current.entities {
        let Kind::Object(ObjectKind::Placeable, index) = entity.kind else {
            continue;
        };
        let Some(template) = instances.made_from(index).map(|made| &made.template) else {
            continue;
        };
        let appearance = match module.and_then(|module| sim::remains::bag_of(module, template)) {
            Some(bag) => Some(bag.placeable()),
            None => *ambience
                .appearances
                .entry(template.clone())
                .or_insert_with(|| {
                    ask.push(template.clone());
                    None
                }),
        };
        made.extend(appearance.map(|appearance| (index, appearance)));
    }
    if !ask.is_empty()
        && let Some(worker) = &ambience.worker
    {
        let _ = worker.ask.send(Ask::Appearances(ask));
    }
    let files: Vec<String> = ambience.area.as_ref().map_or_else(Vec::new, |(area, _)| {
        made.values()
            .filter_map(
                |appearance| match area.placeables.get(usize::from(*appearance)) {
                    Some(Voice::Moves(moves)) => Some(moves.files().map(String::from)),
                    _ => None,
                },
            )
            .flatten()
            .collect()
    });
    wanted(ambience, Keep::Clip, files.iter().map(String::as_str));
    made
}

/// Sound objects, doors and containers (`instances`: `current`'s table,
/// made in `module`), a sound object as `run` (the viewer's run of a
/// minigame) has it. A placeable play made sounds as its appearance does
/// (`made_placeables`).
fn objects(
    ambience: &mut Ambience,
    (instances, module): (&Instances, Option<&sim::module::ModuleData>),
    (current, run): (&Snapshot, Option<&sim::minigame::Course>),
    now: f64,
    anywhere: bool,
    (loops, cues): (&mut Loops, &mut Cues),
) {
    let made = made_placeables(ambience, (instances, module), current);
    let Some((area, _)) = &ambience.area else {
        return;
    };
    let rng = ambience.rng.get_or_insert(Rng(1));
    for entity in &current.entities {
        let Kind::Object(kind, index) = entity.kind else {
            continue;
        };
        let voice = area.objects.get(index as usize).or_else(|| {
            made.get(&index)
                .and_then(|appearance| area.placeables.get(usize::from(*appearance)))
        });
        let (Some(voice), Some(_)) = (voice, instances.kind(index)) else {
            continue;
        };
        let state = entity.object.unwrap_or(sim::ObjectState::default());
        let state = run.map_or(state, |run| run.voiced(entity.id.0, state));
        let at = Vec3::from_array(entity.position);
        match (kind, voice) {
            (ObjectKind::Sound, Voice::Emitter(emitter)) => {
                let on = match state.state {
                    sound::PLAYING => true,
                    sound::STOPPED => false,
                    _ => emitter.active,
                };
                // A script's fading stop (`SoundObjectFadeAndStop`): its loop
                // goes on, quieter, for the fade, and nothing new sounds.
                let left = if on {
                    if let Some(turn) = ambience.turns.get_mut(&index) {
                        turn.fading = None;
                    }
                    1.0
                } else {
                    let fade = f64::from(state.fade);
                    let left = match ambience.turns.get_mut(&index) {
                        Some(turn) if fade > 0.0 => {
                            turn.done = true;
                            1.0 - (now - *turn.fading.get_or_insert(now)) / fade
                        }
                        _ => 0.0,
                    };
                    if left <= 0.0 {
                        ambience.turns.remove(&index);
                        continue;
                    }
                    left
                };
                let volume = match state.volume {
                    0 => emitter.volume,
                    volume => volume - 1,
                };
                let volume = (f64::from(volume) * left).round() as u8;
                // A speed a script fixed, else the template's variation.
                let fixed = (state.pitch > 0).then(|| f64::from(state.pitch - 1) / 100.0);
                let at = (at + Vec3::Z * emitter.elevation) / UNIT;
                let turn = ambience.turns.entry(index).or_default();
                // A script's start plays it from its first sound at once (a
                // swoop's gear shift, its countdown), not within its first
                // interval as an object sounding as placed does.
                if state.state == sound::PLAYING && turn.plays != Some(state.plays) {
                    *turn = Turn {
                        next: Some(now),
                        plays: Some(state.plays),
                        ..Turn::default()
                    };
                }
                if emit(
                    emitter,
                    (volume, anywhere && !emitter.positional, fixed),
                    at,
                    now,
                    turn,
                    rng,
                    &ambience.clips,
                    loops,
                    cues,
                ) {
                    ambience.cued += 1;
                }
            }
            (ObjectKind::Door | ObjectKind::Placeable, Voice::Moves(moves)) => {
                // A door by its state, open or not; a placeable by its look,
                // as the executable sounds its changes: opened, closed (from
                // open), used (on or off), destroyed.
                let shown = u64::from(state.state);
                let was = ambience.moved.insert(index, shown);
                let file = match (kind, was) {
                    (ObjectKind::Door, Some(0)) if shown != 0 => &moves.opened,
                    (ObjectKind::Door, Some(was)) if was != 0 && shown == 0 => &moves.closed,
                    (ObjectKind::Placeable, Some(was)) if was != shown => {
                        placeable_sound(moves, was as u8, state.state)
                    }
                    _ => &None,
                };
                if let Some(file) = file
                    && ambience.clips.contains_key(file)
                {
                    cues.0
                        .push(Cue::at(named(file), at / UNIT, current.time_ms as f32));
                    *ambience.moves.entry(file.clone()).or_insert(0) += 1;
                }
            }
            _ => {}
        }
    }
}

/// The sound a placeable's look makes changing from `from` to `to`
/// (`sim::look`), as the executable sounds it.
fn placeable_sound(moves: &Moves, from: u8, to: u8) -> &Option<String> {
    match (from, to) {
        (_, look::OPEN) => &moves.opened,
        (look::OPEN, look::CLOSED | look::DEFAULT) => &moves.closed,
        (_, look::ON | look::OFF) | (look::ON | look::OFF, look::DEFAULT) => &moves.used,
        (_, look::DEAD) => &moves.destroyed,
        _ => &None,
    }
}

/// A sound object's sounds this frame: a looping one held, the others cued
/// in turn; whether one was cued.
#[allow(clippy::too_many_arguments)]
fn emit(
    emitter: &Emitter,
    (volume, anywhere, fixed): (u8, bool, Option<f64>),
    at: Vec3,
    now: f64,
    turn: &mut Turn,
    rng: &mut Rng,
    clips: &BTreeMap<String, f64>,
    loops: &mut Loops,
    cues: &mut Cues,
) -> bool {
    if emitter.sounds.is_empty() {
        return false;
    }
    let falloff = if anywhere {
        Falloff::Everywhere
    } else {
        Falloff::Range {
            near: emitter.near / UNIT,
            far: emitter.far.max(emitter.near) / UNIT,
            placed: emitter.positional,
        }
    };
    // One looping sound loops seamlessly; several play in turn.
    if emitter.looping && emitter.sounds.len() == 1 {
        let file = &emitter.sounds[0];
        if clips.contains_key(file) {
            loops.0.push(Hold {
                falloff,
                gain: f32::from(volume) / 127.0,
                rate: fixed.unwrap_or(1.0).max(0.1),
                ..Hold::at(named(file), at)
            });
        }
        return false;
    }
    if turn.done {
        return false;
    }
    let interval = f64::from(emitter.interval_ms);
    let vary = f64::from(emitter.interval_variation_ms);
    // The first sound comes within its first interval, so objects placed
    // together do not all sound at once.
    let next = *turn
        .next
        .get_or_insert_with(|| now + f64::from(rng.random()) * (interval + vary));
    if next > now {
        return false;
    }
    let pick = if emitter.random {
        rng.roll_between(0, emitter.sounds.len() as i32 - 1) as usize
    } else {
        turn.index % emitter.sounds.len()
    };
    let file = &emitter.sounds[pick];
    // Not read yet: its turn waits.
    let Some(seconds) = clips.get(file) else {
        return false;
    };
    turn.index += 1;
    if !emitter.continuous && !emitter.looping && turn.index >= emitter.sounds.len() {
        turn.done = true;
    }
    let varied = |base: f32, by: f32, rng: &mut Rng| base + by * rng.signed_unit();
    let at = if emitter.random_position {
        at + Vec3::new(
            emitter.range[0] * rng.signed_unit(),
            emitter.range[1] * rng.signed_unit(),
            0.0,
        ) / UNIT
    } else {
        at
    };
    cues.0.push(Cue {
        falloff,
        gain: (varied(f32::from(volume), f32::from(emitter.volume_variation), rng) / 127.0)
            .clamp(0.0, 1.0),
        rate: fixed
            .unwrap_or_else(|| f64::from(varied(1.0, emitter.pitch_variation, rng).max(0.1))),
        ..Cue::at(named(file), at, now as f32)
    });
    turn.next =
        Some(now + seconds * 1000.0 + (interval + vary * f64::from(rng.signed_unit())).max(0.0));
    true
}

/// Every character's footsteps this frame (and a rolling droid's sound):
/// the sim's walk's, and those its KOTOR clips' own events ask for
/// (`clip_steps`, by whom and where, units) where the walk steps nothing,
/// standing still (a cutscene's stunt, a script's clip), as the
/// executable's footsteps come from its walk and run clips' events alone;
/// and one for each soft landing in `landed`.
fn footsteps(
    ambience: &mut Ambience,
    (level, instances): (&sim::Level, &Instances),
    current: &Snapshot,
    (clip_steps, landed): (Vec<(sim::SimId, Vec3)>, &BTreeSet<sim::SimId>),
    (loops, cues): (&mut Loops, &mut Cues),
) {
    let Some((area, tables)) = &ambience.area else {
        return;
    };
    let rows: BTreeMap<u32, u8> = current
        .entities
        .iter()
        .filter_map(|entity| {
            Some((
                entity.id.0,
                step_row(entity, current, instances, area, tables)?,
            ))
        })
        .collect();
    // A body the area's GIT does not hold (a companion, a script's
    // creature) may walk with a row it did not read.
    let files: Vec<String> = rows
        .values()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .flat_map(|row| tables.step_files(*row).map(String::from))
        .collect();
    wanted(ambience, Keep::Clip, files.iter().map(String::as_str));
    let Some((_, tables)) = &ambience.area else {
        return;
    };
    let rng = ambience.rng.get_or_insert(Rng(1));
    let steps_of = |entity: &sim::EntityState| tables.steps(*rows.get(&entity.id.0)?);
    let moving = |player: &sim::motion::MotionState| {
        let [vx, vy, _] = player.velocity;
        (vx * vx + vy * vy).sqrt() >= STILL
    };
    let mut stepped = Vec::new();
    for entity in &current.entities {
        let Some(player) = &entity.player else {
            continue;
        };
        let Some(steps) = steps_of(entity) else {
            continue;
        };
        let cycle = player.footstep_cycle;
        let was = ambience.cycles.insert(entity.id.0, cycle).unwrap_or(cycle);
        let moving = moving(player);
        let origin = Vec3::from_array(entity.position) / UNIT;
        if let Some(rolling) = &steps.rolling {
            if moving && ambience.clips.contains_key(rolling) {
                loops.0.push(Hold::at(named(rolling), origin));
                ambience.rolled.insert(rolling.clone());
            }
            continue;
        }
        if landed.contains(&entity.id) {
            stepped.push((entity, steps, origin));
            continue;
        }
        // Quake's rule: a step each time the cycle passes 64 or 192, and
        // none while ducked (crouching or rolling).
        let ducked = player.ducked() || player.flags & sim::motion::motion_flags::ROLLING != 0;
        if !moving
            || ducked
            || cycle == 0
            || (u16::from(was) + 64) & 128 == (u16::from(cycle) + 64) & 128
        {
            continue;
        }
        stepped.push((entity, steps, origin));
    }
    for (id, origin) in clip_steps {
        let Some(entity) = current.get(id) else {
            continue;
        };
        let standing = entity.player.as_ref().is_some_and(|player| !moving(player));
        if let Some(steps) = steps_of(entity).filter(|steps| standing && steps.rolling.is_none()) {
            stepped.push((entity, steps, origin));
        }
    }
    for (entity, steps, origin) in stepped {
        // The level's collision is in metres.
        let at = origin * UNIT;
        let floor = level.collision().trace(
            at.to_array(),
            (at - Vec3::Z * FLOOR * UNIT).to_array(),
            &Extents::POINT,
            Contents::SOLID,
        );
        if floor.fraction >= 1.0 {
            continue;
        }
        // A humanoid's step is JA's for the ground it looks like.
        let humanoid = matches!(rows.get(&entity.id.0), Some(&(SOFT | HARD)));
        let running = entity.player.as_ref().is_some_and(|player| {
            let [vx, vy, _] = player.velocity;
            vx * vx + vy * vy >= RUNNING * RUNNING
        });
        if humanoid
            && let Some(name) =
                crate::ja_footsteps::step(floor.surface.ground, running, rng.roll_between(1, 4))
        {
            cues.0.push(Cue {
                entity: Some(entity.id),
                gain: STEP,
                ..Cue::at(name, origin - Vec3::Z * SOLES, current.time_ms as f32)
            });
            ambience.steps += 1;
            continue;
        }
        let choices: Vec<&String> = tables
            .step_sounds(steps, floor.surface.material)
            .iter()
            .flatten()
            .filter(|file| ambience.clips.contains_key(*file))
            .collect();
        if choices.is_empty() {
            continue;
        }
        let file = choices[rng.roll_between(0, choices.len() as i32 - 1) as usize];
        cues.0.push(Cue {
            entity: Some(entity.id),
            gain: STEP,
            rate: f64::from(1.0 + steps.pitch),
            ..Cue::at(
                named(file),
                origin - Vec3::Z * SOLES,
                current.time_ms as f32,
            )
        });
        ambience.steps += 1;
    }
}

/// The `footstepsounds.2da` row `entity` walks with: its body's appearance's
/// (`footsteptype`), as the executable reads it for every creature, players
/// and companions too; else, a creature of the GIT's, its placement's, and
/// the players' soft steps.
fn step_row(
    entity: &sim::EntityState,
    current: &Snapshot,
    instances: &Instances,
    area: &AreaSound,
    tables: &Tables,
) -> Option<u8> {
    let changed = entity
        .fighter
        .as_ref()
        .and_then(|fighter| fighter.look.appearance);
    let appearance = changed.or_else(|| match entity.kind {
        Kind::Avatar(seat) => sim::persona::dressed(current, seat).map(|(row, _)| row),
        Kind::Object(ObjectKind::Creature, _) => {
            Some(instances.of_entity(entity)?.body?.appearance)
        }
        _ => None,
    });
    if let Some(row) = appearance.and_then(|appearance| tables.walk(appearance)) {
        return Some(row);
    }
    match entity.kind {
        Kind::Avatar(_) => Some(PLAYER_STEPS),
        Kind::Object(ObjectKind::Creature, index) => match area.objects.get(index as usize) {
            Some(Voice::Steps(row)) => Some(*row),
            _ => Some(PLAYER_STEPS),
        },
        _ => None,
    }
}
