//! Hosts the sim inside the Bevy app (`kjk sim view`, `kjk play`). The
//! authority world lives in a resource this module keeps private, with the
//! network session's server and the player's own client, joined over a
//! loopback (`solo`, D14). Each frame, the clock turns time into the ticks
//! now due; for each, the client makes the tick's command, the host steps
//! with what the server hands it, and the server keeps the snapshot (its
//! client rebuilds one every few ticks, checked against it). The stepped
//! snapshots go into `PresentedSnapshot`: the only sim state the rest of
//! the app can name. The views draw from it; they could not reach
//! the authority world if they tried.
//!
//! The clock is wall time, or, while an `anim <seconds>` command holds
//! animation time, that time: the sim is stepped up to it, so a scripted
//! run shows the same moment in every run.
//!
//! Module installs (I14): when a step asks for a transition (a player went
//! through a door or trigger that leads out), or a `module` command asks
//! for one, the host stops stepping, shows the loading screen (`Loading`)
//! and plans the module on a thread of its own (the sim's level and the
//! scene: `lifecycle::Installer`). Once it is ready the plan is committed
//! (the world swaps in one call, recorded as an install), the new scene
//! replaces the old (`SceneContent`, `SceneSwap`), and stepping goes on
//! from the tick it stopped at. A transition that fails to plan (its
//! module, arriving at its tag) is not asked for again; the world goes on
//! where it is. A replay installs the modules its recording did, after
//! the same ticks, each checked against the recording's announcement.
//!
//! The reload after the whole party fell (I30 P13, `ReloadRequest`): the
//! host's latest save's module planned as an install, and at the commit the
//! world made from the save (`play::from_save`) in place of the plan's,
//! numbered from the tick it stopped at. Every player follows it as any
//! install, taking the world whole (a keyframe), without leaving. The
//! recording starts again from the reloaded world (the one before goes to
//! `inputs-<tick>.kjkr`): a reload does not replay.

use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use asset_odyssey::texture::TextureIr;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use console::{AnimationTime, DumpDue, DumpFacts, ModuleRequest, RunFolder, States};
use session::Request;
use sim::{
    Accumulator, Announcement, Command, Level, Presented, Recorder, Recording, Save, Seat,
    SimWorld, Snapshot, StepReason, TickInput, TickRate, buttons, step,
};

use crate::lifecycle::{Installer, Prepared};
use crate::play::BodyScene;
use crate::scene::{Headless, SceneContent};
use crate::settings::{Action, KeyMap, MouseFeel};
use crate::solo::Solo;
use net::session::LevelRequest;
use sim::determinism::{Setup, pilot};
use sim::moves::{Script, view_angles};
use sim::script::dialogue::{self, Conversation, Pick};

/// Ticks between the hashes a live session's recording keeps.
const RECORD_EVERY: u64 = 100;

/// Degrees of view per unit of mouse motion (JA's `m_yaw` 0.022 at
/// `sensitivity` 5).
pub(crate) const MOUSE_DEGREES: f32 = 0.11;

/// Who sends seat 0's commands.
#[derive(Clone)]
pub enum Driver {
    /// The pilot for every seat; in a window, WASD overrides seat 0.
    #[cfg_attr(not(feature = "probes"), allow(dead_code))]
    Pilot,
    /// Seat 0 from a `--moves` script, or else the keyboard and mouse in
    /// a window (`kjk play`).
    Player(Option<Script>),
}

/// What installs the modules a session goes to: the planner, the body to
/// draw in each new scene, and the first install, made before the run.
pub struct Lifecycle {
    pub installer: Arc<Installer>,
    pub body: Option<Arc<BodyScene>>,
    pub first: Option<Announcement>,
}

/// What `kjk sim view` and `kjk play` run.
pub struct SimSetup {
    pub setup: Setup,
    pub level: Level,
    pub driver: Driver,
    /// Seat 0's script looks at the nearest creature with this tag, the
    /// living first (`kjk play --aim`).
    pub aim: Option<String>,
    /// Played back instead of live input when present.
    pub recording: Option<Recording>,
    /// A world already made (a module's first install) instead of a new one
    /// from `setup` in `level`.
    pub world: Option<SimWorld>,
    pub lifecycle: Option<Lifecycle>,
    /// Who runs the world: this process alone, this process for players
    /// on a socket too, or another process this one joined.
    pub net: Net,
    /// The camera's seat in a world run here (`OwnSeat`): seat 0, or
    /// another player's in a recording watched (`kjk play --seat`).
    pub seat: Seat,
}

/// The network role of a run (`kjk play`, `kjk host`, `kjk join`).
pub enum Net {
    /// The world here, with the player's own client over a loopback.
    Solo,
    /// The same, and players joining over a socket (`kjk host`).
    Host(Box<crate::solo::Listen>),
    /// A player of another process's world (`kjk join`).
    Join(Box<crate::net_join::Joining>),
}

/// The save to load in place of the world after the party fell (I30 P13;
/// `save_view`): planned as an install, which every player follows.
#[derive(Resource, Default)]
pub struct ReloadRequest(pub Option<String>);

/// The player's own seat: its avatar is the camera's.
#[derive(Resource, Clone, Copy)]
pub struct OwnSeat(pub Seat);

/// The world runs in this process (alone, or hosting): it may be saved
/// here (`save_world`); played `alone`, a save may be loaded during play.
#[derive(Resource, Clone, Copy)]
pub struct WorldHere {
    pub alone: bool,
}

/// The power the player's force key casts, as the HUD shows it.
#[derive(Resource, Default)]
pub struct SelectedPower(pub u8);

/// A joined player's own predictions (only in `kjk join`): its avatar and
/// the camera are drawn from these, ahead of the host's snapshots in
/// `PresentedSnapshot` by the round trip and the remote delay; `fresh` has
/// every prediction made this frame (a rewind's re-steps too), and
/// `correction` (units) is added to the avatar as drawn.
#[derive(Resource)]
pub struct OwnView {
    pub presented: Presented,
    pub fresh: Vec<Snapshot>,
    pub correction: [f32; 3],
}

pub struct SimHostPlugin(Mutex<Option<SimSetup>>);

impl SimHostPlugin {
    pub fn new(setup: SimSetup) -> Self {
        SimHostPlugin(Mutex::new(Some(setup)))
    }

    /// Its systems, waiting for a session the menu starts (`install`).
    pub fn dormant() -> Self {
        SimHostPlugin(Mutex::new(None))
    }
}

/// The player at this machine drives a seat: the mouse is held for it.
#[derive(Resource)]
pub(crate) struct PlayerCursor;

/// The last two snapshots and the blend between them: everything render,
/// UI and audio may know of the sim. `fresh` holds every snapshot stepped
/// this frame, in order, for views that follow each tick. Shared, as
/// `Presented`'s are: a snapshot is too large to copy for each view.
#[derive(Resource)]
pub struct PresentedSnapshot(pub Presented, pub Vec<Arc<Snapshot>>);

/// The moment the views show (ms): between the last two snapshots, by the
/// blend.
pub fn view_ms(presented: &Presented) -> f64 {
    let (previous, current) = (presented.previous(), presented.current());
    previous.time_ms as f64
        + (current.time_ms as f64 - previous.time_ms as f64) * f64::from(presented.alpha())
}

/// How long the host's world has waited for movies and breaks (ms): real
/// time in a window, the held animation time's (`anim`, `record`) while it
/// is held. What is heard and the movies run on the game's time plus this
/// (`shown_ms`), so they go on while the world waits, and a recording's
/// frames (held time) and its sound keep in step.
#[derive(Resource, Default)]
pub struct Waited {
    pub ms: f64,
    /// The held animation time last seen (ms).
    held: Option<f64>,
}

impl Waited {
    /// A frame passed: while the world waits (`waiting`: a movie, a break)
    /// what it waited grows by the frame's time, or the held animation
    /// time's under `anim` and `record`.
    pub fn frame(&mut self, waiting: bool, held_ms: Option<f64>, real_ms: f64) {
        if waiting {
            self.ms += match held_ms {
                Some(ms) => (ms - self.held.unwrap_or(ms)).max(0.0),
                None => real_ms,
            };
        }
        self.held = held_ms;
    }
}

/// The world waits this frame (a movie up, a break): every sound but the
/// beds is paused where it was (`sound`).
#[derive(Resource, Default)]
pub struct Frozen(pub bool);

/// The moment heard and shown (ms): the game's (`view_ms`) plus the time
/// the world waited for movies.
pub fn shown_ms(presented: &Presented, waited: Option<&Waited>) -> f64 {
    view_ms(presented) + waited.map_or(0.0, |waited| waited.ms)
}

/// A module installed and its scene swapped in: views that keep state
/// across frames start over (`generation` counts the swaps), with the
/// body's index in the new scene and the new level. Without a new scene
/// (`scene` false: a joined player starting in the host's world, an
/// install with nothing to draw) what was spawned in the old one stays.
#[derive(Resource)]
pub struct SceneSwap {
    pub generation: u32,
    pub body: Option<usize>,
    pub level: Level,
    pub scene: bool,
}

/// The loading screen's state, for the view that draws it
/// (`lifecycle_view`).
#[derive(Resource, Default)]
pub struct Loading {
    /// A module is being installed.
    pub active: bool,
    pub module: String,
    pub title: Option<String>,
    /// The module's loading image, and how many images were set so far
    /// (the view rebuilds when it changes).
    pub image: Option<TextureIr>,
    pub images: u32,
    /// The image's resref (`ui_view` draws the game's own screen with it).
    pub image_name: Option<String>,
    /// The progress bar's fill (`bluefill`).
    pub bar: Option<TextureIr>,
    /// 0 to 1.
    pub progress: f32,
    /// When the install was committed; the screen fades out after.
    pub committed: Option<Instant>,
    /// When the install started, and whether its screen (image) came.
    pub started: Option<Instant>,
    pub screen: bool,
}

enum Source {
    /// Live input, recorded.
    Live { recorder: Option<Box<Recorder>> },
    /// A recording's ticks, in order, with their own milliseconds, and
    /// its installs after their ticks.
    Replay {
        recording: Box<Recording>,
        next: usize,
        /// The next of `recording.installs` to make.
        install: usize,
        /// Whether the end hash matched, once the recording is used up (or
        /// `false` as soon as an install did not reach its announced state).
        matched: Option<bool>,
    },
}

/// Where the player at the keyboard looks (degrees), from the mouse.
#[derive(Clone, Copy)]
pub(crate) struct Look {
    pub pitch: f32,
    pub yaw: f32,
}

impl Look {
    /// Turned by the mouse's motion this frame, as the player set it. The
    /// sim adds its `view_offset` to the command's angles (a saber move
    /// holding the view, a script's facing or a new module's start move
    /// them), so the pitch is held where the view it gives, `held` (the
    /// player's pitch delta, degrees) added, stays within the sim's limit:
    /// the view reaches straight down and up whatever the delta, and turns
    /// back at once from either end.
    pub(crate) fn turn(&mut self, delta: Vec2, feel: crate::settings::MouseFeel, held: f32) {
        let degrees = MOUSE_DEGREES * feel.scale;
        let up = if feel.invert { -1.0 } else { 1.0 };
        self.yaw -= delta.x * degrees;
        let most = pitch_limit();
        self.pitch = (self.pitch + up * delta.y * degrees).clamp(-most - held, most - held);
    }
}

/// The pitch delta (degrees) the sim adds to `seat`'s commands in
/// `snapshot` (`Look::turn`), none without its player.
pub(crate) fn held_pitch(snapshot: &Snapshot, seat: Seat) -> f32 {
    snapshot
        .own(seat)
        .and_then(|entity| entity.player.as_ref())
        .map_or(0.0, |player| {
            sim::math::short_to_degrees(player.view_offset[0])
        })
}

/// How far a view pitches up or down (degrees): the sim's limit
/// (`motion::view_of`, JA's own pitch limit, about 88°).
pub(crate) fn pitch_limit() -> f32 {
    sim::motion::view_of([i16::MAX, 0, 0], [0; 3])[0]
}

/// What a planning thread sends back.
enum JobMessage {
    Screen(Box<crate::lifecycle::LoadingScreen>),
    Progress(f32),
    Prepared(Box<Result<Prepared, String>>),
}

/// A module being planned.
pub(crate) struct Job {
    pub request: Request,
    receiver: Mutex<Receiver<JobMessage>>,
    pub started: Instant,
    /// A `module` command asked for it.
    commanded: bool,
    /// A replay's install: what the plan and the state after it must be.
    expect: Option<Announcement>,
    /// A save made the world at commit, in place of the install (I30 P13).
    reload: Option<Box<Save>>,
}

impl Job {
    /// Starts planning `request` with `installer` on a thread of its own
    /// (with its scene when `scene`, `delay` late), and shows the loading
    /// screen for it.
    pub(crate) fn spawn(
        installer: Arc<Installer>,
        request: Request,
        scene: bool,
        delay: Duration,
        loading: &mut Loading,
    ) -> Job {
        let (sender, receiver) = channel();
        let job_request = request.clone();
        std::thread::spawn(move || {
            let screen = installer.loading_screen(&job_request.module);
            let _ = sender.send(JobMessage::Screen(Box::new(screen)));
            let progress = sender.clone();
            std::thread::sleep(delay);
            let prepared = installer.prepare_with(&job_request, scene, &move |done| {
                let _ = progress.send(JobMessage::Progress(done));
            });
            let _ = sender.send(JobMessage::Prepared(Box::new(prepared)));
        });
        info!(target: "lifecycle", module = %request.module, arrival = ?request.arrival, "installing");
        *loading = Loading {
            active: true,
            module: request.module.clone(),
            title: None,
            image: None,
            images: loading.images,
            image_name: None,
            bar: None,
            progress: 0.0,
            committed: None,
            started: Some(Instant::now()),
            screen: false,
        };
        Job {
            request,
            receiver: Mutex::new(receiver),
            started: Instant::now(),
            commanded: false,
            expect: None,
            reload: None,
        }
    }

    /// Takes what the planning thread sent (the loading screen's parts);
    /// the plan, once it is done.
    pub(crate) fn poll(&self, loading: &mut Loading) -> Option<Result<Prepared, String>> {
        loop {
            let message = self
                .receiver
                .lock()
                .expect("the job channel lock")
                .try_recv();
            match message {
                Ok(JobMessage::Screen(screen)) => {
                    loading.image = screen.image;
                    loading.image_name = screen.name;
                    loading.bar = screen.bar;
                    loading.images += 1;
                    loading.title = screen.title;
                    loading.screen = true;
                    loading.progress = loading.progress.max(0.1);
                }
                Ok(JobMessage::Progress(progress)) => loading.progress = progress,
                Ok(JobMessage::Prepared(prepared)) => return Some(*prepared),
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    return Some(Err("the planning thread ended without a plan".into()));
                }
            }
        }
    }
}

/// An install done, for dumps.
struct Installed {
    tick: u64,
    module: String,
    arrival: String,
    prepare_ms: f64,
    /// Of `prepare_ms`: opening the archives, the plan, the scene.
    open_ms: f64,
    plan_ms: f64,
    scene_ms: f64,
    /// The area came from the installer's level cache.
    area_cached: bool,
    commit_ms: f64,
    live: usize,
    left_behind: usize,
    hash: String,
}

/// The authority. Private: nothing outside this module can read it.
#[derive(Resource)]
struct Authority {
    world: SimWorld,
    clock: Accumulator,
    source: Source,
    setup: Setup,
    driver: Driver,
    aim: Option<String>,
    look: Look,
    controls: Controls,
    last: Arc<Snapshot>,
    /// Frames seen, frames that stepped no tick, most ticks in one frame.
    frames: u64,
    idle_frames: u64,
    most: usize,
    lifecycle: Option<Lifecycle>,
    job: Option<Job>,
    /// Transitions that failed to plan, as (module, arrival): not asked
    /// for again.
    declined: Vec<(String, Option<String>)>,
    installs: Vec<Installed>,
    generation: u32,
    /// The session: the server, the player's client, the loopback.
    solo: Solo,
    /// A live world's ticks streamed to the run's folder (a replay's are
    /// not).
    flight: Option<crate::flight_writer::Flight>,
}

impl Plugin for SimHostPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Waited>()
            .init_resource::<CameraAim>()
            .init_resource::<ReloadRequest>()
            .init_resource::<crate::save_view::SaveRequest>()
            .init_resource::<crate::save_view::SaveOutcome>()
            .init_resource::<Frozen>()
            .init_resource::<crate::swallow::Swallowed>()
            .add_systems(
                PreUpdate,
                (
                    crate::swallow::hold_up.after(bevy::input::InputSystems),
                    (
                        (advance, save_world, autosave)
                            .chain()
                            .run_if(resource_exists::<Authority>),
                        crate::net_join::advance.run_if(crate::net_join::joined),
                    ),
                )
                    .chain(),
            )
            .add_systems(
                Last,
                (
                    (write_transcript, write_session)
                        .chain()
                        .run_if(resource_exists::<Authority>),
                    crate::net_join::flush.run_if(crate::net_join::joined),
                ),
            )
            .add_systems(Update, grab_cursor.run_if(resource_exists::<PlayerCursor>));
        let setup = self.0.lock().expect("the host's setup lock").take();
        if let Some(setup) = setup
            && let Err(error) = install(app.world_mut(), setup)
        {
            panic!("{error}");
        }
    }
}

/// A session's world and its network role, put into `world`: from here
/// the plugin's systems step it.
pub fn install(app: &mut World, setup: SimSetup) -> Result<(), String> {
    {
        let SimSetup {
            setup,
            level,
            driver,
            aim,
            recording,
            world,
            lifecycle,
            net,
            seat,
        } = setup;
        let player = matches!(driver, Driver::Player(_));
        if player {
            app.insert_resource(PlayerCursor);
        }
        let listen = match net {
            Net::Solo => None,
            Net::Host(listen) => Some(listen),
            Net::Join(joining) => {
                let start = match world {
                    Some(mut world) => world.snapshot(),
                    None => SimWorld::new(setup.world, level.clone())
                        .map(|mut world| world.snapshot())
                        .map_err(|error| format!("the stage's world: {error}"))?,
                };
                crate::net_join::install(app, *joining, level, start, driver, lifecycle);
                return Ok(());
            }
        };
        let rate = recording
            .as_ref()
            .and_then(|recording| TickRate::new(recording.hz))
            .unwrap_or(setup.rate);
        // A replay's world is made (and checked) by whoever read the
        // recording; a live world here, from a seat count the command line
        // bounded.
        let mut world = match world {
            Some(world) => world,
            None => SimWorld::new(setup.world, level.clone())
                .map_err(|error| format!("a new world: {error}"))?,
        };
        let last = world.snapshot();
        let source = match recording {
            Some(recording) => Source::Replay {
                recording: Box::new(recording),
                next: 0,
                install: 0,
                matched: None,
            },
            None => Source::Live {
                recorder: Some(Box::new(
                    Recorder::new(rate.hz(), last.clone(), world.level().id(), RECORD_EVERY)
                        .with_script(world.script_state().clone()),
                )),
            },
        };
        let request = match lifecycle.as_ref().and_then(|l| l.first.clone()) {
            Some(announcement) => LevelRequest::Module(announcement),
            None => LevelRequest::Stage(world.level().id()),
        };
        let alone = listen.is_none();
        let folder = app.resource::<RunFolder>().0.clone();
        let flight = matches!(source, Source::Live { .. }).then(|| {
            crate::flight_writer::Flight::new(
                &folder,
                rate.hz(),
                &last,
                &world.level().id(),
                world.script_state(),
            )
        });
        let solo = Solo::new(world.level(), request, rate, &last, listen, &folder)
            .map_err(|error| format!("the session: {error}"))?;
        let mut states = States::default();
        states.0.insert("world".into(), true);
        states.0.insert("loading".into(), false);
        let last = Arc::new(last);
        app.insert_resource(PresentedSnapshot(
            Presented::shared(last.clone()),
            vec![last.clone()],
        ));
        app.insert_resource(SceneSwap {
            generation: 0,
            body: None,
            level: world.level().clone(),
            scene: false,
        });
        app.get_resource_or_insert_with(States::default)
            .0
            .extend(states.0);
        app.insert_resource(OwnSeat(seat));
        app.insert_resource(WorldHere { alone });
        app.init_resource::<SelectedPower>();
        app.init_resource::<Loading>();
        crate::report::game_started(app);
        app.insert_resource(Authority {
            world,
            clock: Accumulator::new(rate, last.tick),
            source,
            setup: Setup { rate, ..setup },
            driver,
            aim,
            look: Look {
                pitch: 0.0,
                yaw: setup.world.yaw,
            },
            controls: Controls::default(),
            last,
            frames: 0,
            idle_frames: 0,
            most: 0,
            lifecycle,
            job: None,
            declined: Vec::new(),
            installs: Vec::new(),
            generation: 0,
            solo,
            flight,
        });
    }
    Ok(())
}

/// This machine's player's input as play reads it, the host's (`advance`)
/// and a joined player's (`net_join::advance`) alike: the keys (none
/// without a window; none pressed, an idle command that keeps the view,
/// while a screen or a movie is up), the mouse buttons held while the
/// cursor is, and how far the mouse turns the view (not while a movie is
/// up: on the host the world waits for it, `movie_view`). What a movie
/// skip, a conversation or the cursor took reads as up until let go
/// (`swallow`, before both).
#[derive(SystemParam)]
pub(crate) struct PlayerInput<'w, 's> {
    keys: Option<Res<'w, ButtonInput<KeyCode>>>,
    buttons: Option<Res<'w, ButtonInput<MouseButton>>>,
    motion: Option<Res<'w, AccumulatedMouseMotion>>,
    headless: Res<'w, Headless>,
    windows: Query<'w, 's, &'static CursorOptions, With<PrimaryWindow>>,
    screens: Option<Res<'w, crate::ui_view::UiOpen>>,
    showing: Option<Res<'w, crate::movie_view::Showing>>,
    feel: Option<Res<'w, MouseFeel>>,
    idle: Local<'s, ButtonInput<KeyCode>>,
}

/// A frame's `PlayerInput`.
pub(crate) struct Read<'a> {
    pub keys: Option<&'a ButtonInput<KeyCode>>,
    pub triggers: Triggers,
    /// The mouse's motion the view turns by (`Look::turn`), if it turns.
    pub turn: Option<Vec2>,
    pub feel: MouseFeel,
}

impl PlayerInput<'_, '_> {
    /// Whether a movie is up (`movie_view::Showing`).
    pub(crate) fn movie_up(&self) -> bool {
        self.showing.as_ref().is_some_and(|showing| showing.0)
    }

    pub(crate) fn read(&self) -> Read<'_> {
        let headless = self.headless.0;
        let held = self.movie_up() || self.screens.as_ref().is_some_and(|open| open.0);
        let feel = self
            .feel
            .as_deref()
            .map_or_else(MouseFeel::default, |feel| *feel);
        let grabbed = !self.movie_up()
            && self
                .windows
                .iter()
                .any(|cursor| cursor.grab_mode != CursorGrabMode::None);
        Read {
            keys: self
                .keys
                .as_deref()
                .filter(|_| !headless)
                .map(|keys| if held { &*self.idle } else { keys }),
            triggers: Triggers::of(grabbed, self.buttons.as_deref(), feel),
            turn: self
                .motion
                .as_ref()
                .filter(|_| !headless && grabbed)
                .map(|motion| motion.delta),
            feel,
        }
    }
}

/// In a window, the mouse turns the player: the cursor is held and hidden;
/// a screen opening lets it go, the last one closing (or a click) takes it
/// again; a conversation's screen frees it meanwhile (`dialogue_view`).
pub(crate) fn grab_cursor(
    headless: Res<Headless>,
    screens: Option<Res<crate::ui_view::UiOpen>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    replies: Option<Res<crate::dialogue_view::ReplyInput>>,
    mut swallowed: ResMut<crate::swallow::Swallowed>,
    mut was_open: Local<Option<bool>>,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if headless.0 || replies.is_some_and(|input| input.active) {
        return;
    }
    let open = screens.is_some_and(|screens| screens.0);
    let release = open && *was_open != Some(true);
    let clicked = buttons
        .as_ref()
        .is_some_and(|b| b.just_pressed(MouseButton::Left));
    let free = windows
        .iter()
        .any(|cursor| cursor.grab_mode == CursorGrabMode::None);
    let take = !open && (*was_open != Some(false) || clicked);
    if let Some(buttons) = buttons.as_deref().filter(|_| take && clicked && free) {
        swallowed.button(MouseButton::Left, buttons);
    }
    // The last screen closing under a held button (Take All, whose screen
    // closes once the host has emptied the container, frames after the
    // click): the press was the screen's, not play's (`swallow`).
    if let Some(buttons) = buttons
        .as_deref()
        .filter(|_| !open && *was_open == Some(true))
    {
        for button in [MouseButton::Left, MouseButton::Right] {
            swallowed.button(button, buttons);
        }
    }
    *was_open = Some(open);
    for mut cursor in &mut windows {
        if release {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        } else if take {
            cursor.grab_mode = CursorGrabMode::Locked;
            cursor.visible = false;
        }
    }
}

/// Seat 0's command in `kjk sim view`: WASD, space to jump, C to crouch,
/// looking along +Y.
pub(crate) fn keyboard(keys: &ButtonInput<KeyCode>) -> Command {
    let axis = |plus: KeyCode, minus: KeyCode| {
        127 * i8::from(keys.pressed(plus)) - 127 * i8::from(keys.pressed(minus))
    };
    Command {
        angles: view_angles(0.0, 90.0),
        forward: axis(KeyCode::KeyW, KeyCode::KeyS),
        right: axis(KeyCode::KeyD, KeyCode::KeyA),
        up: axis(KeyCode::Space, KeyCode::KeyC),
        ..Command::default()
    }
}

/// What the player chose that outlives a key press (JA's `weapon` and
/// `forcesel`): the hand out, the power the force key casts, and its last
/// answer to a conversation (held, `sim::script::dialogue::answer`).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Controls {
    pub weapon: u8,
    pub power: u8,
    pub reply: u8,
    /// The `--moves` script's last answer byte, and the byte last sent
    /// (`scripted`).
    script_reply: u8,
    sent: u8,
}

impl Controls {
    /// A scripted run's answer byte for a tick: the held one, which the
    /// screens answer into at once (`ui nav`, `ui press`), and the
    /// script's own answers (`reply N`). A tick carries one new answer: a
    /// script's that comes with a screen's not yet sent waits a tick.
    pub(crate) fn scripted(&mut self, script: u8) -> u8 {
        if self.reply == self.sent && script != self.script_reply {
            self.script_reply = script;
            if let Some(pick) = dialogue::pick_of(script) {
                self.reply = dialogue::answer(self.reply, pick);
            }
        }
        self.sent = self.reply;
        self.reply
    }

    /// This frame's choices: in a conversation (`talk`) the answer the
    /// conversation screen took (`picked`, `dialogue_view::ReplyInput`);
    /// else the right or the left hand's weapon out (their keys, 1 and 2
    /// unless rebound). [ and ] step through `choices` (`force::choices` of
    /// the player's sheet), which starts at its first.
    pub(crate) fn update(
        &mut self,
        keys: &ButtonInput<KeyCode>,
        map: &KeyMap,
        choices: &[u8],
        talk: Option<&Conversation>,
        picked: Option<Pick>,
    ) {
        if talk.is_some() {
            if let Some(pick) = picked {
                self.reply = dialogue::answer(self.reply, pick);
            }
        } else {
            if map.pressed_now(keys, Action::RightHand) {
                self.weapon = 0;
            }
            if map.pressed_now(keys, Action::LeftHand) {
                self.weapon = 1;
            }
        }
        let step = i32::from(map.pressed_now(keys, Action::NextPower))
            - i32::from(map.pressed_now(keys, Action::PreviousPower));
        self.power = match choices.iter().position(|row| *row == self.power) {
            _ if choices.is_empty() => 0,
            None => choices[0],
            Some(at) => choices[(at as i32 + step).rem_euclid(choices.len() as i32) as usize],
        };
    }
}

/// The powers `seat`'s player can select in `snapshot`: its body's sheet's
/// (`cast::body_sheet`, `level` the snapshot's).
pub(crate) fn power_choices(snapshot: &Snapshot, seat: Seat, level: &Level) -> Vec<u8> {
    crate::cast::body_sheet(snapshot, seat, level).map_or_else(Vec::new, |(_, sheet)| {
        sim::combat::force::choices(&sheet.powers)
    })
}

/// The line through the middle of the own screen as the last frame drew it
/// (`Command::aim`, set by `play_view` from its camera's ray; none while no
/// camera of the player's looks): every own command carries it, so its
/// shots go where the screen's middle looks.
#[derive(Resource, Default)]
pub struct CameraAim(pub [i8; 3]);

/// The mouse buttons held while the cursor is (the click that takes the
/// cursor does not fire: `swallow`).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Triggers {
    pub attack: bool,
    pub alt: bool,
}

impl Triggers {
    pub(crate) fn of(
        grabbed: bool,
        mouse: Option<&ButtonInput<MouseButton>>,
        feel: crate::settings::MouseFeel,
    ) -> Triggers {
        let held = |button| grabbed && mouse.is_some_and(|mouse| mouse.pressed(button));
        let (attack, alt) = if feel.swap {
            (MouseButton::Right, MouseButton::Left)
        } else {
            (MouseButton::Left, MouseButton::Right)
        };
        Triggers {
            attack: held(attack),
            alt: held(alt),
        }
    }
}

/// The player's command in `kjk play`: the mouse looks, WASD moves, space
/// jumps, C or Ctrl crouches, Shift walks (JA's client sends 64 for a
/// walk); E is the target's (`target_view`: its chosen action, a revive
/// among them), not the use bit; the left mouse button attacks, the right
/// one is the weapon's other mode (a feat special with a blaster, a kick
/// or special with a saber); 1 and 2 pick the hand, L cycles the saber
/// style, F casts the selected power ([ and ] select), G grips, R throws
/// lightning, Q drains, T charges a grenade (thrown on release), 3 uses a
/// medpac, 4 takes a stim, 5 puts up a shield, X turns stealth mode on or
/// off, O lays the best mine carried (H holsters: `holster_view`).
pub(crate) fn player_command(
    keys: &ButtonInput<KeyCode>,
    map: &KeyMap,
    triggers: Triggers,
    look: Look,
    controls: Controls,
) -> Command {
    let held = |action| map.held(keys, action);
    let walk = held(Action::Walk) || keys.pressed(KeyCode::ShiftRight);
    let speed: i8 = if walk { 64 } else { 127 };
    let axis = |plus, minus| speed * i8::from(held(plus)) - speed * i8::from(held(minus));
    let crouch = held(Action::Crouch) || keys.pressed(KeyCode::ControlLeft);
    let pressed = [
        (walk, buttons::WALKING),
        (triggers.attack, buttons::ATTACK),
        (triggers.alt, buttons::ALT_ATTACK),
        (held(Action::SaberStyle), buttons::SABER_STYLE),
        (held(Action::Power), buttons::FORCE_POWER),
        (held(Action::Grip), buttons::FORCE_GRIP),
        (held(Action::Lightning), buttons::FORCE_LIGHTNING),
        (held(Action::Drain), buttons::FORCE_DRAIN),
        (held(Action::Grenade), buttons::GRENADE),
        (held(Action::Medpac), buttons::MEDPAC),
        (held(Action::Shield), buttons::SHIELD),
        (held(Action::Stim), buttons::STIM),
        (held(Action::Stealth), buttons::STEALTH),
        (held(Action::LayMine), buttons::LAY_MINE),
    ];
    Command {
        angles: view_angles(look.pitch, look.yaw),
        forward: axis(Action::Forward, Action::Backward),
        right: axis(Action::Right, Action::Left),
        up: if held(Action::Jump) {
            127
        } else if crouch {
            -127
        } else {
            0
        },
        buttons: pressed
            .iter()
            .filter(|(on, _)| *on)
            .fold(0, |bits, (_, button)| bits | button),
        weapon: controls.weapon,
        force: controls.power,
        reply: controls.reply,
        // The camera's, put in as the command is sent (`CameraAim`).
        aim: [0; 3],
    }
}

/// `command` turned to look at the chest of the creature tagged `tag`
/// nearest seat 0 in `last`, the living first (unchanged with none):
/// scripted fights that hit, and stay on the body when it falls, for
/// filming them.
fn aimed(command: Command, last: &Snapshot, level: &Level, tag: &str) -> Command {
    use sim::combat::mind;
    let Some(me) = last.own(Seat(0)).and_then(|e| e.player) else {
        return command;
    };
    let objects = sim::Instances::in_snapshot(level, last);
    let tagged = |entity: &sim::EntityState| crate::cast::is_tagged(entity, &objects, tag);
    let distance = |them: &sim::motion::MotionState| {
        let d = sim::math::sub(them.origin, me.origin);
        sim::math::dot(d, d)
    };
    let nearest = last
        .entities
        .iter()
        .filter(|e| tagged(e))
        .filter_map(|e| Some((!e.fighter?.alive(), e.player?)))
        .min_by(|(a_dead, a), (b_dead, b)| {
            a_dead.cmp(b_dead).then(distance(a).total_cmp(&distance(b)))
        });
    let Some((_, them)) = nearest else {
        return command;
    };
    let (pitch, yaw) = mind::aim(mind::crosshair(&me), mind::chest(&them));
    Command {
        angles: mind::looking(&me, pitch, yaw).angles,
        ..command
    }
}

impl Authority {
    /// Starts planning `request` on a thread of its own, if the session
    /// installs modules and this transition did not fail before (a command
    /// or a replay asks regardless).
    fn start(
        &mut self,
        request: Request,
        commanded: bool,
        expect: Option<Announcement>,
        loading: &mut Loading,
    ) -> bool {
        let Some(lifecycle) = &self.lifecycle else {
            return false;
        };
        let key = (request.module.clone(), request.arrival.clone());
        if !commanded && expect.is_none() && self.declined.contains(&key) {
            return false;
        }
        let installer = lifecycle.installer.clone();
        let scene = lifecycle.body.is_some();
        info!(
            target: "lifecycle",
            module = %request.module,
            seats = request.seats,
            players = request.players,
            "planning"
        );
        self.solo.installing(&request);
        self.job = Some(Job {
            commanded,
            expect,
            ..Job::spawn(installer, request, scene, Duration::ZERO, loading)
        });
        true
    }

    /// A replay's next install, if it is made after `tick`: planning it
    /// starts (the replay waits for it, as a live session waits for its
    /// installs). A replay with no installer to plan it stops, unmatched.
    fn replay_install(&mut self, tick: u64, loading: &mut Loading) -> bool {
        let Source::Replay {
            recording,
            install,
            next,
            matched,
        } = &mut self.source
        else {
            return false;
        };
        let Some(announcement) = recording.installs.get(*install).filter(|a| a.tick == tick) else {
            return false;
        };
        let announcement = announcement.clone();
        *install += 1;
        if self.lifecycle.is_none() {
            error!(target: "sim", module = %announcement.module, "the replay installs a module, and nothing here plans one");
            *next = recording.ticks.len();
            *matched = Some(false);
            return false;
        }
        self.start(
            Request::of(&announcement),
            false,
            Some(announcement),
            loading,
        )
    }
}

/// A save asked for (`save_view`), made between ticks: the world as it
/// stands and the script VM's state, written with what the screens show of
/// it, unless KOTOR would refuse it now (`saves::refusal`, a module
/// loading, a movie holding the world).
fn save_world(
    mut authority: ResMut<Authority>,
    mut request: ResMut<crate::save_view::SaveRequest>,
    mut outcome: ResMut<crate::save_view::SaveOutcome>,
    made: Option<Res<crate::character::Made>>,
    showing: Option<Res<crate::movie_view::Showing>>,
    own: Option<Res<OwnSeat>>,
    settings: Option<Res<crate::settings::Settings>>,
) {
    let Some(asked) = request.0.take() else {
        return;
    };
    let on_screen = asked.on_screen;
    let authority = &mut *authority;
    let movie = showing.is_some_and(|showing| showing.0);
    let seat = own.map_or(Seat(0), |own| own.0);
    let character = made.and_then(|made| made.0.clone());
    let saved = store(
        authority,
        Slot::Named(asked.slot),
        asked.name,
        asked.area,
        character,
        movie,
        seat,
    )
    .map(|(id, meta, world)| {
        let host = settings.map_or_else(
            || crate::settings::Settings::load().host_id().to_string(),
            |settings| settings.host_id().to_string(),
        );
        send_copies(&mut authority.solo, host, &id, &meta, &world);
        id
    });
    if let Err(why) = &saved {
        warn!(target: "saves", "not saved: {why}");
    }
    outcome.0 = Some(crate::save_view::Saved {
        result: saved,
        on_screen,
    });
}

/// The host's world now as a save's bytes (for a bug report), none
/// without one.
pub(crate) fn current_save(world: &mut World) -> Option<Vec<u8>> {
    let mut authority = world.get_resource_mut::<Authority>()?;
    let snapshot = authority.world.snapshot();
    let save = sim::Save {
        hz: authority.setup.rate.hz(),
        level: authority.world.level().id(),
        script: authority.world.script_state().clone(),
        snapshot,
    };
    Some(save.encode())
}

/// Where a save goes: a slot (a new number without one), or the autosaves.
enum Slot {
    Named(Option<String>),
    Auto,
}

/// The world saved into `slot`, named `name`, in the area called `area`,
/// stamped with this build; its id, meta and bytes. Refused as KOTOR
/// refuses (`saves::refusal` for `seat`), while a module loads and during a
/// `movie`.
fn store(
    authority: &mut Authority,
    slot: Slot,
    name: String,
    area: String,
    character: Option<ui::intent::CharacterDesc>,
    movie: bool,
    seat: Seat,
) -> Result<(String, saves::Meta, Vec<u8>), String> {
    let snapshot = authority.world.snapshot();
    let refused = if authority.job.is_some() {
        Some("You cannot save while an area loads.".to_string())
    } else if movie {
        Some("You cannot save during a movie.".to_string())
    } else {
        saves::refusal(&snapshot, seat, authority.world.level())
    };
    if let Some(why) = refused {
        return Err(why);
    }
    let hash = snapshot.hash();
    let meta = saves::Meta {
        name,
        module: snapshot.place.as_ref().map_or_else(
            || authority.world.level().id().name,
            |place| place.module.clone(),
        ),
        area,
        saved: saves::now(),
        seats: snapshot.seats(),
        character,
        build: Some(env!("KJK_GIT_SHA").into()),
        version: Some(env!("KJK_VERSION").into()),
        formats: Some(saves::Formats {
            save: sim::SAVE_FORMAT,
            snapshot: sim::SNAPSHOT_FORMAT,
        }),
        ..saves::Meta::default()
    };
    let tick = snapshot.tick;
    let save = sim::Save {
        hz: authority.setup.rate.hz(),
        level: authority.world.level().id(),
        script: authority.world.script_state().clone(),
        snapshot,
    };
    let world = save.encode();
    let root = crate::save_view::folder().ok_or("no folder for saves here")?;
    let id = match slot {
        Slot::Named(slot) => saves::write_encoded(&root, slot.as_deref(), &world, &meta)?,
        Slot::Auto => saves::write_auto(&root, &world, &meta)?,
    };
    // The whole save's hash (the VM's state too): a load of it says the
    // same (`play::load`).
    let bytes = blake3::hash(&world);
    info!(target: "saves", id, tick, %hash, save = %bytes, "saved");
    Ok((id, meta, world))
}

/// An autosave owed since a module was installed (`installs` counts them;
/// none counted yet: the first module the game starts in is owed one too):
/// tried each second until nothing refuses it.
#[derive(Default)]
struct AutosaveDue {
    installs: Option<usize>,
    owed: bool,
    tried: Option<Instant>,
}

impl AutosaveDue {
    /// The authority's `installs` seen (`fresh` when it was just made: a
    /// game started or loaded); whether an autosave became owed, which only
    /// a player's game with `eligible` (live, Autosave on) owes.
    fn observe(&mut self, installs: usize, fresh: bool, eligible: bool) -> bool {
        if fresh {
            *self = AutosaveDue::default();
        }
        if Some(installs) == self.installs {
            return false;
        }
        self.installs = Some(installs);
        self.owed = eligible;
        eligible
    }
}

/// The autosave owed at a game's first module and after each module
/// entered, a new game's opening included after another game was played.
#[cfg(feature = "probes")]
pub fn autosave_probes() -> sim::probe::Checks {
    let mut due = AutosaveDue::default();
    let first = due.observe(0, true, true);
    due.owed = false;
    let same = due.observe(0, false, true);
    let again = due.observe(0, true, true);
    due.owed = false;
    let entered = due.observe(1, false, true);
    let off = AutosaveDue::default().observe(0, true, false);
    vec![(
        "autosave_owed_each_game",
        sim::probe::check(
            first && !same && entered && again && !off,
            format!("first {first}, same {same}, entered {entered}, a new game {again}, off {off}"),
        ),
    )]
}

/// The autosave owed after each module installed (KOTOR's rule), once a
/// second until nothing refuses it: a player's game, solo or hosting (the
/// host's world is the authority), live, with Autosave on.
fn autosave(
    mut authority: ResMut<Authority>,
    mut due: Local<AutosaveDue>,
    (player, settings): (
        Option<Res<crate::crash::PlayerRun>>,
        Option<Res<crate::settings::Settings>>,
    ),
    showing: Option<Res<crate::movie_view::Showing>>,
    own: Option<Res<OwnSeat>>,
    made: Option<Res<crate::character::Made>>,
) {
    let eligible = player.is_some_and(|player| player.0)
        && matches!(authority.source, Source::Live { .. })
        && settings
            .is_some_and(|settings| settings.value(crate::settings::AUTOSAVE) == Some("true"));
    if due.observe(authority.installs.len(), authority.is_added(), eligible) {
        info!(target: "saves", installs = authority.installs.len(), "autosave owed");
    }
    let now = Instant::now();
    if !due.owed
        || due
            .tried
            .is_some_and(|tried| now - tried < Duration::from_secs(1))
    {
        return;
    }
    due.tried = Some(now);
    let area = authority
        .world
        .level()
        .module()
        .map_or_else(String::new, |module| module.area_title.clone());
    let movie = showing.is_some_and(|showing| showing.0);
    let seat = own.map_or(Seat(0), |own| own.0);
    let character = made.and_then(|made| made.0.clone());
    match store(
        &mut authority,
        Slot::Auto,
        area.clone(),
        area,
        character,
        movie,
        seat,
    ) {
        Ok(_) => due.owed = false,
        // KOTOR's refusals pass: the autosave waits for them.
        Err(why) if why.starts_with("You cannot save") => {
            debug!(target: "saves", "autosave waits: {why}")
        }
        Err(why) => {
            warn!(target: "saves", "no autosave: {why}");
            due.owed = false;
        }
    }
}

/// The reload of the save `id` starts: its module planned as an install
/// for the session's seats and the players it was planned for; or why not,
/// for the host (the save does not read or decode, a module is loading,
/// this run plans none).
fn reload_start(authority: &mut Authority, id: &str, loading: &mut Loading) -> Result<(), String> {
    let (save, _) =
        crate::save_view::read(id).map_err(|why| format!("The last save does not load: {why}"))?;
    let request = authority
        .lifecycle
        .as_ref()
        .filter(|_| authority.job.is_none())
        .zip(save.snapshot.place.as_ref())
        .map(|(lifecycle, place)| {
            lifecycle
                .installer
                .request(&place.module, None, authority.last.seats())
                .with_players(place.players)
        });
    let request = request.ok_or(
        "The last save does not load now: a module is loading, this run plans none, \
         or the save is not in one.",
    )?;
    info!(target: "saves", id, module = %request.module, "reloading");
    if authority.start(request, true, None, loading)
        && let Some(job) = authority.job.as_mut()
    {
        job.reload = Some(Box::new(save));
    }
    Ok(())
}

/// A save just written goes to every player as a copy (I30 P17), saying
/// this install's `host` id and whose game it is (the character's name,
/// else the host's); a game played alone has none to send it to. A player
/// with too many copies still waiting is left out, and logged.
fn send_copies(solo: &mut Solo, host: String, id: &str, meta: &saves::Meta, world: &[u8]) {
    let character = meta.character.as_ref().map(|made| made.name.trim());
    let received = saves::copies::Received {
        host,
        from: character
            .filter(|name| !name.is_empty())
            .map_or_else(|| solo.name(), str::to_string),
        slot: id.to_string(),
        build: env!("KJK_GIT_SHA").to_string(),
    };
    let copy = saves::copies::pack(meta, world, &received);
    match solo.save_copy(&copy) {
        Ok(sent) => {
            if !sent.to.is_empty() {
                info!(target: "saves", id, bytes = copy.len(), seats = ?sent.to, "copy sent");
            }
            if !sent.left_out.is_empty() {
                warn!(target: "saves", id, seats = ?sent.left_out, "no copy for these players: too many of theirs still on the way");
            }
        }
        Err(why) => warn!(target: "saves", id, "no copy for the players: {why}"),
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn advance(
    time: Res<Time<Real>>,
    held: Res<AnimationTime>,
    input: PlayerInput,
    mut authority: ResMut<Authority>,
    mut presented: ResMut<PresentedSnapshot>,
    mut facts: ResMut<DumpFacts>,
    dump_due: Res<DumpDue>,
    (mut loading, mut states, mut request, mut swap, mut commands, mut selected, mut replies): (
        ResMut<Loading>,
        ResMut<States>,
        ResMut<ModuleRequest>,
        ResMut<SceneSwap>,
        Commands,
        Option<ResMut<SelectedPower>>,
        Option<ResMut<crate::dialogue_view::ReplyInput>>,
    ),
    (map, mut waited, mut autopilot, mut frozen, mut reload, folder): (
        Option<Res<KeyMap>>,
        ResMut<Waited>,
        Option<ResMut<crate::target_view::Autopilot>>,
        ResMut<Frozen>,
        ResMut<ReloadRequest>,
        Res<RunFolder>,
    ),
    (mut outgoing, mut told, mut shown, mut heard, mut ui, camera_aim): (
        Option<ResMut<crate::loot::Outgoing>>,
        Option<ResMut<crate::loot::Told>>,
        Option<ResMut<crate::script_screens::Shown>>,
        Option<ResMut<crate::ambience::Heard>>,
        Option<ResMut<crate::ui_view::Ui>>,
        Res<CameraAim>,
    ),
) {
    let map = map.map_or_else(KeyMap::default, |map| map.clone());
    let authority = &mut *authority;
    let rate = authority.setup.rate;
    // The first frame keeps the starting snapshot for the views.
    if authority.frames > 0 {
        presented.1.clear();
    }
    // A `module` command's install.
    if let Some((module, arrival)) = request.pending.take() {
        match &authority.lifecycle {
            Some(lifecycle) if authority.job.is_none() => {
                let request =
                    lifecycle
                        .installer
                        .request_from(&authority.last, &module, arrival.as_deref());
                authority.start(request, true, None, &mut loading);
            }
            Some(_) => request.failed = Some("a module is being installed already".into()),
            None => request.failed = Some("this run installs no modules".into()),
        }
    }
    // The party fell: the latest save's module planned as an install, the
    // world made from the save at its commit; or why not, over the lost box.
    if let Some(id) = reload.0.take()
        && let Err(why) = reload_start(authority, &id, &mut loading)
    {
        warn!(target: "saves", id, "not reloaded: {why}");
        if let Some(ui) = ui.as_deref_mut() {
            crate::save_view::refuse_on_screen(ui, why);
        }
    }
    // A module being planned: its loading screen, then its commit.
    if let Some(job) = &authority.job
        && let Some(result) = job.poll(&mut loading)
    {
        let mut job = authority.job.take().expect("checked above");
        let prepare_ms = job.started.elapsed().as_secs_f64() * 1000.0;
        let reloaded = job.reload.take();
        let was_reload = reloaded.is_some();
        let committed = result.and_then(|prepared| {
            // A replay's install must be the recorded one, before and
            // after.
            if let Some(expect) = &job.expect {
                prepared
                    .plan
                    .check(expect)
                    .map_err(|mismatch| format!("not the recorded install: {mismatch}"))?;
            }
            let started = Instant::now();
            let announcement = match reloaded {
                Some(save) => {
                    let seats = authority.last.seats();
                    let tick = authority.last.tick;
                    let (world, first) =
                        crate::play::from_save(&save, &prepared.plan.level, seats, Some(tick))?;
                    authority.world = world;
                    info!(target: "saves", tick, "reloaded");
                    first
                }
                None => prepared
                    .plan
                    .commit(&mut authority.world)
                    .map_err(|error| error.to_string())?,
            };
            if let Some(expect) = &job.expect {
                expect
                    .check_after(&authority.world.snapshot())
                    .map_err(|mismatch| format!("replayed install: {mismatch}"))?;
            }
            Ok((
                prepared,
                announcement,
                started.elapsed().as_secs_f64() * 1000.0,
            ))
        });
        match committed {
            Ok((prepared, announcement, commit_ms)) => {
                let after = authority.world.snapshot();
                authority
                    .solo
                    .installed(announcement.clone(), &after, authority.world.level());
                if let Source::Live {
                    recorder: Some(recorder),
                } = &mut authority.source
                {
                    if was_reload {
                        // A reload does not replay: the recording so far
                        // is kept beside, and one starts from here.
                        let before = std::mem::replace(
                            recorder,
                            Box::new(
                                Recorder::new(
                                    rate.hz(),
                                    after.clone(),
                                    authority.world.level().id(),
                                    RECORD_EVERY,
                                )
                                .with_script(authority.world.script_state().clone()),
                            ),
                        );
                        let path = folder.0.join(format!("inputs-{}.kjkr", after.tick));
                        let ended = before.finish_encoded(&authority.last);
                        if let Err(error) = std::fs::write(&path, ended) {
                            error!(target: "sim", "{}: {error}", path.display());
                        }
                    } else {
                        recorder.install(announcement.clone());
                    }
                }
                // The flight recorder's segment for the module entered
                // (a reload's too): it installs nothing.
                if let Some(flight) = &mut authority.flight {
                    flight.start(
                        &after,
                        &authority.world.level().id(),
                        authority.world.script_state(),
                    );
                }
                let census = after.census();
                info!(
                    target: "lifecycle",
                    module = %announcement.module,
                    arrival = %announcement.arrival,
                    tick = after.tick,
                    prepare_ms,
                    commit_ms,
                    live = census.live_total(),
                    left_behind = census.stashed_total(),
                    hash = %announcement.after.short(),
                    "installed"
                );
                authority.installs.push(Installed {
                    tick: after.tick,
                    module: announcement.module.clone(),
                    arrival: announcement.arrival.clone(),
                    prepare_ms,
                    open_ms: prepared.open_ms,
                    plan_ms: prepared.plan_ms,
                    scene_ms: prepared.scene_ms,
                    area_cached: prepared.plan.facts.cached,
                    commit_ms,
                    live: census.live_total(),
                    left_behind: census.stashed_total(),
                    hash: announcement.after.to_string(),
                });
                let after = Arc::new(after);
                presented.0 = Presented::shared(after.clone());
                presented.1 = vec![after.clone()];
                authority.clock = Accumulator::new(rate, after.tick);
                authority.last = after;
                authority.generation += 1;
                let mut body = None;
                let scene = prepared.scene.is_some();
                if let Some(scene) = prepared.scene {
                    let scene = match authority.lifecycle.as_ref().and_then(|l| l.body.clone()) {
                        Some(parts) => {
                            let (scene, index) = crate::play::compose(Some(scene), &parts, None);
                            body = Some(index);
                            scene
                        }
                        None => scene,
                    };
                    commands.insert_resource(SceneContent(Box::new(scene)));
                }
                *swap = SceneSwap {
                    generation: authority.generation,
                    body,
                    level: authority.world.level().clone(),
                    scene,
                };
                loading.progress = 1.0;
                loading.committed = Some(Instant::now());
            }
            Err(error) => {
                error!(target: "lifecycle", module = %job.request.module, "not installed: {error}");
                authority.solo.abandon();
                if was_reload {
                    // The world goes on lost; the box stays for another Load.
                } else if job.commanded {
                    request.failed = Some(error);
                } else if job.expect.is_some() {
                    // The replay cannot go on as recorded.
                    if let Source::Replay {
                        recording,
                        next,
                        matched,
                        ..
                    } = &mut authority.source
                    {
                        *next = recording.ticks.len();
                        *matched = Some(false);
                    }
                } else {
                    authority
                        .declined
                        .push((job.request.module.clone(), job.request.arrival.clone()));
                }
                loading.active = false;
            }
        }
    }
    // A replay's install made before its next tick (its start's, or one
    // that was waiting for this frame).
    if authority.job.is_none() {
        let tick = authority.last.tick;
        authority.replay_install(tick, &mut loading);
    }
    let installing = authority.job.is_some();
    states.0.insert("loading".into(), loading.active);
    states.0.insert("world".into(), !loading.active);
    // This player's actions go to the host before the ticks, so a resume
    // reaches it while a break holds.
    if let Some(outgoing) = outgoing.as_deref_mut() {
        for action in outgoing.0.drain(..) {
            authority.solo.act(action);
        }
    }
    // The world waits while a movie is up, as KOTOR's does, or while a
    // break holds until a resume waits to be stepped (its caller's or the
    // host's ends it, another's is told no: `sim::party`); the players wait
    // with it (`Solo::hold`). A replay steps its recorded ticks.
    let movie_up = input.movie_up();
    let on_break = matches!(authority.source, Source::Live { .. })
        && authority.last.paused_by.is_some()
        && !authority.solo.resume_waiting();
    let waiting = movie_up || on_break;
    authority.solo.hold(waiting);
    frozen.0 = waiting;
    // The session answers its players (the own client joins and loads
    // before the world steps; a host waits for loaders after an install).
    authority.solo.pump();
    let playing = authority.solo.playing();

    // The ticks due: by wall time, or up to the held animation time (less
    // what the world waited); none while a module is being installed (its
    // time is not the sim's), before the player's client plays, or while
    // the world waits (`Waited`). A movie up takes the keys and the mouse
    // too (`PlayerInput`).
    let held_ms = held.0.map(|seconds| f64::from(seconds) * 1000.0);
    waited.frame(waiting, held_ms, time.delta_secs_f64() * 1000.0);
    let due: Vec<(u64, u32)> = if installing || !playing || waiting {
        if !playing || waiting {
            authority.clock = Accumulator::new(rate, authority.last.tick);
        }
        Vec::new()
    } else {
        match held_ms {
            Some(ms) => {
                let target = (ms - waited.ms).max(0.0) as u64;
                let mut due = Vec::new();
                let mut tick = authority.last.tick;
                while rate.time_ms(tick) < target {
                    tick += 1;
                    due.push((tick, rate.msec(tick)));
                }
                authority.clock = Accumulator::new(rate, tick);
                due
            }
            None => authority.clock.advance(time.delta()),
        }
    };
    let Read {
        keys,
        triggers,
        turn,
        feel,
    } = input.read();
    if let (Some(delta), Driver::Player(_)) = (turn, &authority.driver) {
        let held = held_pitch(&authority.last, Seat(0));
        authority.look.turn(delta, feel, held);
    }
    let picked = replies.as_deref_mut().and_then(|input| input.pick.take());
    let scripted = matches!(authority.driver, Driver::Player(Some(_)));
    if let Some(pick) = picked.filter(|_| scripted)
        && authority.last.talk_for(Seat(0)).is_some()
    {
        authority.controls.reply = dialogue::answer(authority.controls.reply, pick);
    }
    if let Some(keys) = &keys {
        let choices = power_choices(&authority.last, Seat(0), authority.world.level());
        let talk = authority.last.talk_for(Seat(0));
        authority
            .controls
            .update(keys, &map, &choices, talk, picked.filter(|_| !scripted));
        if let Some(selected) = selected.as_deref_mut() {
            selected.0 = authority.controls.power;
        }
    }

    if let Some(told) = told.as_deref_mut() {
        told.0.extend(authority.solo.take_notices());
    }
    if let Some(shown) = shown.as_deref_mut() {
        shown.0.extend(authority.solo.take_screens());
    }
    if let Some(heard) = heard.as_deref_mut() {
        heard.0.extend(authority.solo.take_sounds());
    }
    // The snapshots this frame steps are pushed after the first `fresh`.
    let fresh = presented.1.len();
    let mut stepped = 0;
    for (tick, msec) in due {
        let start_ms = rate.time_ms(tick - 1);
        // The player's command for the tick, made on its client: the
        // script's, the keyboard's, the pilot's, or the recording's.
        let pilot_input = matches!(authority.driver, Driver::Pilot)
            .then(|| pilot(authority.setup.pilot, tick, authority.setup.world.seats));
        let own = match (&authority.source, &authority.driver, &keys) {
            (
                Source::Replay {
                    recording, next, ..
                },
                ..,
            ) => recording
                .ticks
                .get(*next)
                .and_then(|recorded| recorded.input.get(Seat(0)).copied()),
            (Source::Live { .. }, Driver::Pilot, Some(keys)) => Some(keyboard(keys)),
            (Source::Live { .. }, Driver::Pilot, None) => pilot_input
                .as_ref()
                .and_then(|input| input.get(Seat(0)).copied()),
            (Source::Live { .. }, Driver::Player(Some(script)), _) => {
                let command = script.command(start_ms);
                // The HUD shows the power the script picks, as the keys'.
                if let Some(selected) = selected.as_deref_mut()
                    && command.force != 0
                {
                    selected.0 = command.force;
                }
                Some(match &authority.aim {
                    Some(tag) => {
                        // A minigame's bot (`race`, `turrets`; the probes'),
                        // else a creature.
                        let level = authority.world.level();
                        #[cfg(feature = "probes")]
                        let steered = crate::minigame_bot::drive(
                            tag,
                            command,
                            (
                                level.minigame().map(|rules| &**rules),
                                authority.last.minigame.as_ref(),
                            ),
                            sim::script::global(
                                &authority.world.script_state().numbers,
                                crate::minigame_bot::GEAR,
                            )
                            .copied(),
                            start_ms,
                            &crate::minigame_bot::area_foes(&authority.last),
                        )
                        .unwrap_or_else(|| aimed(command, &authority.last, level, tag));
                        #[cfg(not(feature = "probes"))]
                        let steered = aimed(command, &authority.last, level, tag);
                        steered
                    }
                    None => command,
                })
            }
            (Source::Live { .. }, Driver::Player(None), Some(keys)) => Some(player_command(
                keys,
                &map,
                triggers,
                authority.look,
                authority.controls,
            )),
            (Source::Live { .. }, Driver::Player(None), None) => None,
        };
        // A walk to a target steers in the player's place (`target_view`),
        // with no keys too (headless: the console's `ui act`).
        let walking = autopilot
            .as_ref()
            .is_some_and(|pilot| pilot.command.is_some());
        let own = match (&authority.source, &authority.driver, own) {
            (Source::Live { .. }, Driver::Player(_), own) if own.is_some() || walking => {
                Some(crate::target_view::steer(
                    autopilot.as_deref_mut(),
                    own.unwrap_or(sim::Command::default()),
                    &mut authority.look,
                ))
            }
            (_, _, own) => own,
        };
        let own = match (&authority.source, own) {
            (Source::Live { .. }, Some(mut command)) if scripted => {
                command.reply = authority.controls.scripted(command.reply);
                Some(command)
            }
            (_, own) => own,
        };
        // The player's shots go where the middle of its screen looks.
        let own = match (&authority.source, &authority.driver, own) {
            (Source::Live { .. }, Driver::Player(_), Some(command)) => Some(Command {
                aim: camera_aim.0,
                ..command
            }),
            (_, _, own) => own,
        };
        authority.solo.command(tick, own);
        let snapshot = match &mut authority.source {
            Source::Live { recorder } => {
                // What the server has (the player's), and the pilot for the
                // seats no client drives.
                let mut input = authority.solo.commands_for(tick, authority.last.paused_by);
                for (seat, command) in pilot_input.iter().flat_map(TickInput::iter) {
                    if !authority.solo.occupied(seat) {
                        input.insert(seat, *command);
                    }
                }
                let snapshot = step(
                    &mut authority.world,
                    tick,
                    &input,
                    msec,
                    StepReason::Authority,
                );
                if let Some(recorder) = recorder {
                    recorder.record(msec, &input, &snapshot);
                }
                if let Some(flight) = &mut authority.flight {
                    flight.tick(msec, &input, &snapshot);
                }
                for (seat, notice) in authority.world.take_notices() {
                    match notice {
                        sim::notice::Notice::Show(screen) => authority.solo.screen(seat, screen),
                        sim::notice::Notice::Sound(sound) => authority.solo.sound(seat, sound),
                        notice => {
                            let text = crate::loot::words(&notice, ui.as_deref_mut());
                            authority.solo.notice(seat, &text);
                        }
                    }
                }
                (input, snapshot)
            }
            Source::Replay {
                recording, next, ..
            } => {
                let Some(recorded) = recording.ticks.get(*next) else {
                    break;
                };
                let snapshot = step(
                    &mut authority.world,
                    tick,
                    &recorded.input,
                    recorded.msec,
                    StepReason::Replay,
                );
                *next += 1;
                (recorded.input.clone(), snapshot)
            }
        };
        let (input, snapshot) = snapshot;
        stepped += 1;
        let transition = snapshot.transition.clone();
        let snapshot = Arc::new(snapshot);
        authority.solo.stepped(&input, &snapshot);
        presented.1.push(snapshot.clone());
        authority.last = snapshot;
        // A replay's install after this tick: the world waits for it.
        if authority.replay_install(tick, &mut loading) {
            authority.clock = Accumulator::new(rate, tick);
            states.0.insert("loading".into(), true);
            states.0.insert("world".into(), false);
            break;
        }
        // A transition asked for: the world waits while the module loads.
        if let Some(transition) = transition
            && matches!(authority.source, Source::Live { .. })
            && let Some(lifecycle) = &authority.lifecycle
        {
            let request = lifecycle.installer.request_from(
                &authority.last,
                &transition.module,
                Some(&transition.tag),
            );
            if authority.start(request, false, None, &mut loading) {
                authority.clock = Accumulator::new(rate, tick);
                states.0.insert("loading".into(), true);
                states.0.insert("world".into(), false);
                break;
            }
        }
        // A break called this tick: the world waits from here.
        if authority.last.paused_by.is_some() && matches!(authority.source, Source::Live { .. }) {
            authority.clock = Accumulator::new(rate, tick);
            break;
        }
    }
    // Where the play is, for a crash's report.
    let (last, module) = (&authority.last, authority.world.level().name());
    crate::crash::note(|crumbs| {
        crumbs.tick = last.tick;
        if crumbs.module != module {
            module.clone_into(&mut crumbs.module);
        }
        let talk = last.talk.conversations.first();
        if crumbs
            .conversation
            .as_ref()
            .map(|(dialog, entry)| (dialog.as_str(), *entry))
            != talk.map(|talk| (talk.dialog.as_str(), talk.entry))
        {
            crumbs.conversation = talk.map(|talk| (talk.dialog.clone(), talk.entry));
        }
    });
    // The views blend the frame's last two snapshots; the rest they read
    // from the list (all of them shared, none copied).
    let (previous, newest) = match &presented.1[fresh..] {
        [] => (None, None),
        [only] => (None, Some(only.clone())),
        [.., previous, newest] => (Some(previous.clone()), Some(newest.clone())),
    };
    if let Some(newest) = newest {
        if let Some(previous) = previous {
            presented.0.push_shared(previous);
        }
        presented.0.push_shared(newest);
    }
    let alpha = match held_ms {
        Some(ms) => {
            let (from, to) = (
                presented.0.previous().time_ms as f32,
                presented.0.current().time_ms as f32,
            );
            if to > from {
                ((ms - waited.ms) as f32 - from) / (to - from)
            } else {
                1.0
            }
        }
        None if authority.job.is_some() => 1.0,
        None => authority.clock.alpha(),
    };
    presented.0.set_alpha(alpha);

    // A replay is over once its ticks and its installs are: its end must be
    // the recording's.
    if authority.job.is_none()
        && let Source::Replay {
            recording,
            next,
            install,
            matched: matched @ None,
        } = &mut authority.source
        && *next == recording.ticks.len()
        && *install == recording.installs.len()
    {
        let same = authority.last.hash() == recording.end;
        info!(target: "sim", tick = authority.last.tick, same, "replay finished");
        *matched = Some(same);
    }
    authority.frames += 1;
    authority.idle_frames += u64::from(stepped == 0);
    authority.most = authority.most.max(stepped);
    let (source, replay) = match &authority.source {
        Source::Live { .. } => ("live", None),
        Source::Replay { matched, .. } => ("replay", *matched),
    };
    let last = &authority.last;
    let census = last.census();
    facts.0.insert(
        "sim".into(),
        serde_json::json!({
            "source": source,
            "hz": authority.setup.rate.hz(),
            "tick": last.tick,
            "time_ms": last.time_ms,
            // Hashing encodes the whole state: only for a dump.
            "hash": dump_due.0.then(|| last.hash().to_string()),
            "entities": last.entities.len(),
            "module": last.place.as_ref().map(|place| place.module.clone()),
            "census": { "live": census.live, "left_behind": census.stashed },
            // The games' scripts run so far, by script (for a dump).
            "script_runs": dump_due.0.then(|| authority.world.script_report().runs.clone()),
            "frames": authority.frames,
            "frames_without_tick": authority.idle_frames,
            "most_ticks_in_a_frame": authority.most,
            "dropped_ms": authority.clock.dropped().as_millis() as u64,
            "replay_matched": replay,
        }),
    );
    authority.solo.sample();
    facts.0.insert("net".into(), authority.solo.facts());

    let first = authority
        .lifecycle
        .as_ref()
        .and_then(|lifecycle| lifecycle.first.as_ref());
    facts.0.insert(
        "lifecycle".into(),
        serde_json::json!({
            "installing": authority.job.as_ref().map(|job| job.request.module.clone()),
            "loading_screen": loading.active,
            "first": first.map(|a| serde_json::json!({
                "module": a.module, "arrival": a.arrival, "hash": a.after.to_string(),
            })),
            "installs": authority.installs.iter().map(|i| serde_json::json!({
                "tick": i.tick,
                "module": i.module,
                "arrival": i.arrival,
                "prepare_ms": i.prepare_ms,
                "open_ms": i.open_ms,
                "plan_ms": i.plan_ms,
                "scene_ms": i.scene_ms,
                "area_cached": i.area_cached,
                "commit_ms": i.commit_ms,
                "live": i.live,
                "left_behind": i.left_behind,
                "hash": i.hash,
            })).collect::<Vec<_>>(),
            "declined": authority.declined.iter().map(|(module, arrival)| serde_json::json!({
                "module": module, "arrival": arrival,
            })).collect::<Vec<_>>(),
            "scene_generation": authority.generation,
        }),
    );
}

/// The conversations' lines as they happen, to `transcript.txt` in the run
/// folder (the authority's own, resolved against the level they were said
/// in).
fn write_transcript(
    authority: Option<Res<Authority>>,
    folder: Res<RunFolder>,
    mut written: Local<usize>,
) {
    let Some(authority) = authority else {
        return;
    };
    let talk = &authority.world.script_report().talk;
    if talk.len() <= *written {
        return;
    }
    let level = authority.world.level();
    let (Some(library), Some(lifecycle)) = (level.scripts(), &authority.lifecycle) else {
        *written = talk.len();
        return;
    };
    let installer = &lifecycle.installer;
    let strref = |strref| installer.text(strref);
    let names = crate::transcript::speakers(&authority.last, level);
    let mut text = String::new();
    let mut tokens = crate::transcript::tokens(&talk[..*written]);
    for line in &talk[*written..] {
        let player = sim::persona::of(&authority.last.personas, sim::Seat(0));
        for written in
            crate::transcript::transcript(&strref, library, &names, (&mut tokens, player), line)
        {
            text.push_str(&written);
            text.push('\n');
        }
    }
    *written = talk.len();
    let path = folder.0.join("transcript.txt");
    let appended = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()));
    if let Err(error) = appended {
        error!(target: "sim", "{}: {error}", path.display());
    }
}

/// When the run ends: a live session's recording (`inputs.kjkr`) and a save
/// of where it stopped (`end.kjkv`) go into the run folder.
fn write_session(
    mut exits: MessageReader<AppExit>,
    mut authority: ResMut<Authority>,
    folder: Res<RunFolder>,
) {
    if exits.read().next().is_none() {
        return;
    }
    let authority = &mut *authority;
    authority.solo.shutdown(&folder.0);
    // The flight recorder's last ticks, waited for on the disk.
    if let Some(mut flight) = authority.flight.take() {
        flight.flush(&authority.last);
    }
    let Source::Live { recorder } = &mut authority.source else {
        return;
    };
    let Some(recorder) = recorder.take() else {
        return;
    };
    let recording = (*recorder).finish_encoded(&authority.last);
    let save = Save {
        hz: authority.setup.rate.hz(),
        level: authority.world.level().id(),
        snapshot: (*authority.last).clone(),
        script: authority.world.script_state().clone(),
    };
    for (name, bytes) in [("inputs.kjkr", recording), ("end.kjkv", save.encode())] {
        let path = folder.0.join(name);
        match std::fs::write(&path, bytes) {
            Ok(()) => info!(target: "sim", "wrote {}", path.display()),
            Err(error) => error!(target: "sim", "{}: {error}", path.display()),
        }
    }
}
