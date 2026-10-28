//! The command queue that scripts drive the runtime with (`kjk --cmds '…'`).
//!
//! Commands run one at a time, and each one finishes only when what it
//! promises is true: a wait has elapsed, a screenshot file exists, a dump is
//! written. A command that cannot keep its promise ends the run with an
//! error instead of letting the next one start.
//!
//! A screenshot also waits until the renderer has no pipelines left to
//! compile: shaders compile in the background, and a frame captured before
//! they finish shows nothing of the scene.

mod script;
mod video;
mod vitals;

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bevy::app::AppExit;
use bevy::prelude::*;
use bevy::render::render_resource::{CachedPipelineState, PipelineCache};
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
use bevy::render::{Render, RenderApp, RenderSystems};

pub use script::{CameraPose, Command, Frames, ParseError, Video, WaitFor, parse};
pub use video::AUDIO;

/// Frames in a row with no pipeline waiting before a screenshot is taken.
const SETTLED_FRAMES: u32 = 3;
/// How long a screenshot waits for pipelines, then for the captured frame.
const PIPELINE_LIMIT: Duration = Duration::from_secs(60);
const CAPTURE_LIMIT: Duration = Duration::from_secs(10);

/// Where a run writes its files (screenshots, dumps).
#[derive(Resource, Clone, Debug)]
pub struct RunFolder(pub PathBuf);

/// What `screenshot` captures: the window, or the image a headless run
/// renders into.
#[derive(Resource, Clone, Debug)]
pub enum CaptureTarget {
    PrimaryWindow,
    Image(Handle<Image>),
}

/// Facts other plugins publish for `dump` (render adapter, mode, …).
#[derive(Resource, Default, Debug)]
pub struct DumpFacts(pub BTreeMap<String, serde_json::Value>);

/// Whether the next frame's `Update` may write a dump: facts costly to
/// publish every frame (the sim's hash) are published only then. True
/// until the queue has run once, so a dump on the first frame has them.
#[derive(Resource, Debug)]
pub struct DumpDue(pub bool);

impl Default for DumpDue {
    fn default() -> DumpDue {
        DumpDue(true)
    }
}

/// Animation time held fixed by `anim <seconds>`; `None` runs with the clock.
#[derive(Resource, Default, Debug)]
pub struct AnimationTime(pub Option<f32>);

/// The game's time the view shows (ms), once there is a game: what `clip`
/// stamps its frames with, on the clock of the run's sound.
#[derive(Resource, Default, Debug)]
pub struct GameClock(pub Option<f64>);

/// A camera placement the scene applies on its next frame, then clears.
#[derive(Resource, Default, Debug)]
pub struct CameraRequest(pub Option<CameraPose>);

/// States other plugins publish for `wait <state>` (`world`, `loading`).
#[derive(Resource, Default, Debug)]
pub struct States(pub BTreeMap<String, bool>);

/// A `module` command's install, for the host to take (`pending`), and
/// why it failed, if it did.
#[derive(Resource, Default, Debug)]
pub struct ModuleRequest {
    pub pending: Option<(String, Option<String>)>,
    pub failed: Option<String>,
}

/// While true, the script waits: a run whose session loads behind its
/// window (`kjk play`) starts its script once the session is in, so a
/// script's frames are the game's, as when the window opened on it.
#[derive(Resource, Default, Debug)]
pub struct Hold(pub bool);

/// Where the console's queue runs: what takes its requests runs after.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct ConsoleQueue;

/// `ui` commands for the game's screens to take, oldest first.
#[derive(Resource, Default, Debug)]
pub struct UiRequests(pub Vec<Vec<String>>);

/// `vfx` commands for the view to play, oldest first.
#[derive(Resource, Default, Debug)]
pub struct VfxRequests(pub Vec<Vec<String>>);

/// `arena` orders for the combat arena to give, oldest first.
#[derive(Resource, Default, Debug)]
pub struct ArenaRequests(pub Vec<Vec<String>>);

/// How long `wait <state>` and `module` wait.
const STATE_LIMIT: Duration = Duration::from_secs(120);

/// The render world's pipeline counts, published every frame.
#[derive(Resource, Clone, Default)]
struct Pipelines {
    waiting: Arc<AtomicUsize>,
    failed: Arc<AtomicUsize>,
    total: Arc<AtomicUsize>,
}

pub struct ConsolePlugin {
    pub commands: Vec<Command>,
}

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        let pipelines = Pipelines::default();
        app.insert_resource(Queue {
            pending: self.commands.iter().cloned().collect(),
            active: None,
            frame: 0,
            started: Instant::now(),
            settled: 0,
            clip: None,
        })
        .insert_resource(pipelines.clone())
        .init_resource::<DumpFacts>()
        .init_resource::<CameraRequest>()
        .init_resource::<AnimationTime>()
        .init_resource::<GameClock>()
        .init_resource::<States>()
        .init_resource::<ModuleRequest>()
        .init_resource::<UiRequests>()
        .init_resource::<VfxRequests>()
        .init_resource::<ArenaRequests>()
        .init_resource::<DumpDue>()
        .init_resource::<Hold>()
        .init_resource::<vitals::FrameTimes>()
        .add_systems(First, vitals::time_frames)
        .add_systems(
            Update,
            (vitals::publish, run_queue, dump_due)
                .chain()
                .in_set(ConsoleQueue)
                .run_if(|hold: Res<Hold>| !hold.0),
        );
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .insert_resource(pipelines)
                .add_systems(Render, count_pipelines.in_set(RenderSystems::Cleanup));
        }
    }
}

fn count_pipelines(cache: Res<PipelineCache>, pipelines: Res<Pipelines>) {
    let failed = cache
        .pipelines()
        .filter(|pipeline| matches!(pipeline.state, CachedPipelineState::Err(_)))
        .count();
    pipelines
        .waiting
        .store(cache.waiting_pipelines().count(), Ordering::Release);
    pipelines.failed.store(failed, Ordering::Release);
    pipelines
        .total
        .store(cache.pipelines().count(), Ordering::Release);
}

#[derive(Resource)]
struct Queue {
    pending: VecDeque<Command>,
    active: Option<Active>,
    frame: u64,
    started: Instant,
    /// Frames in a row with no pipeline waiting.
    settled: u32,
    /// A clip being made: the game's time it started, and each screenshot's
    /// since.
    clip: Option<(f64, Vec<f64>)>,
}

struct Active {
    command: Command,
    since: Instant,
    since_frame: u64,
    /// For a screenshot: the file, and whether it was captured. `None` while
    /// waiting for pipelines.
    captured: Option<(PathBuf, Arc<AtomicBool>)>,
    capture_requested_at: Option<Instant>,
    /// For a video: `ffmpeg` at work.
    making: Option<std::thread::JoinHandle<Result<bool, String>>>,
    /// For a film: its frames so far.
    recording: Option<Recording>,
}

/// A `film`'s frames: when it started, each frame's ms since, and the
/// thread that writes them.
struct Recording {
    started: Instant,
    times: Vec<u128>,
    frames: std::sync::mpsc::Sender<Image>,
    /// Frames written so far, and the size of the first.
    written: Arc<AtomicUsize>,
    size: Arc<std::sync::Mutex<Option<UVec2>>>,
}

impl Recording {
    /// A recording whose frames go to `path` as raw RGB, one after another,
    /// on a thread of its own (encoding a frame takes longer than one).
    fn new(path: PathBuf) -> Recording {
        let (frames, incoming) = std::sync::mpsc::channel::<Image>();
        let written = Arc::new(AtomicUsize::new(0));
        let size = Arc::new(std::sync::Mutex::new(None));
        let (count, first) = (written.clone(), size.clone());
        std::thread::spawn(move || {
            use std::io::Write as _;
            let file = std::fs::File::create(&path).map(std::io::BufWriter::new);
            let mut file = match file {
                Ok(file) => Some(file),
                Err(error) => {
                    error!(target: "console", "{}: {error}", path.display());
                    None
                }
            };
            for image in incoming {
                if let (Some(out), Ok(image)) = (file.as_mut(), image.try_into_dynamic()) {
                    let rgb = image.to_rgb8();
                    first
                        .lock()
                        .expect("the recording's size")
                        .get_or_insert(UVec2::new(rgb.width(), rgb.height()));
                    if let Err(error) = out.write_all(rgb.as_raw()) {
                        error!(target: "console", "{}: {error}", path.display());
                        file = None;
                    }
                }
                count.fetch_add(1, Ordering::Release);
            }
            if let Some(mut out) = file {
                let _ = out.flush();
            }
        });
        Recording {
            started: Instant::now(),
            times: Vec::new(),
            frames,
            written,
            size,
        }
    }
}

/// The screenshot of `target`'s next frame.
fn screenshot(target: &CaptureTarget) -> Screenshot {
    match target {
        CaptureTarget::PrimaryWindow => Screenshot::primary_window(),
        CaptureTarget::Image(handle) => Screenshot::image(handle.clone()),
    }
}

/// After the queue ran: a dump starts next frame when nothing is active
/// and a dump is next.
fn dump_due(queue: Res<Queue>, mut due: ResMut<DumpDue>) {
    due.0 = queue.active.is_none() && matches!(queue.pending.front(), Some(Command::Dump(_)));
}

#[allow(clippy::too_many_arguments)]
fn run_queue(
    mut queue: ResMut<Queue>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
    folder: Res<RunFolder>,
    target: Option<Res<CaptureTarget>>,
    mut facts: ResMut<DumpFacts>,
    mut camera: ResMut<CameraRequest>,
    mut animation: ResMut<AnimationTime>,
    (pipelines, states): (Res<Pipelines>, Res<States>),
    (mut module, mut screens, mut visuals, mut arena): (
        ResMut<ModuleRequest>,
        ResMut<UiRequests>,
        ResMut<VfxRequests>,
        ResMut<ArenaRequests>,
    ),
    clock: Res<GameClock>,
    entities: Query<Entity>,
) {
    queue.frame += 1;
    let frame = queue.frame;
    let waiting = pipelines.waiting.load(Ordering::Acquire);
    let failed = pipelines.failed.load(Ordering::Acquire);
    queue.settled = if waiting == 0 { queue.settled + 1 } else { 0 };
    facts.0.insert(
        "pipelines".into(),
        serde_json::json!({
            "total": pipelines.total.load(Ordering::Acquire),
            "waiting": waiting,
            "failed": failed,
        }),
    );

    if queue.active.is_none() {
        let Some(command) = queue.pending.pop_front() else {
            return;
        };
        info!(target: "console", "start: {command}");
        let active = Active {
            command: command.clone(),
            since: Instant::now(),
            since_frame: frame,
            captured: None,
            capture_requested_at: None,
            making: None,
            recording: None,
        };
        match &command {
            Command::Screenshot(_) | Command::Film(..) if target.is_none() => {
                fail(&mut exit, &command, "nothing renders in this run");
                return;
            }
            Command::Dump(name) => {
                let path = folder.0.join(format!("{name}.json"));
                let dump = serde_json::json!({
                    "frame": frame,
                    "elapsed_ms": queue.started.elapsed().as_millis() as u64,
                    "entities": entities.iter().count(),
                    "facts": facts.0,
                });
                let text = serde_json::to_string_pretty(&dump).expect("json values serialize");
                if let Err(error) = std::fs::write(&path, text) {
                    fail(&mut exit, &command, &format!("{}: {error}", path.display()));
                    return;
                }
            }
            Command::Quit => {
                info!(target: "console", "done: quit");
                exit.write(AppExit::Success);
                return;
            }
            Command::Crash => panic!("crash requested"),
            Command::Camera(pose) => camera.0 = Some(*pose),
            Command::Ui(words) => screens.0.push(words.clone()),
            Command::Vfx(words) => visuals.0.push(words.clone()),
            Command::Arena(words) => arena.0.push(words.clone()),
            Command::Clip => match clock.0 {
                Some(now) => queue.clip = Some((now, Vec::new())),
                None => {
                    fail(&mut exit, &command, "no game runs here to clip");
                    return;
                }
            },
            Command::Video(video) => {
                let mut video = video.clone();
                if let Frames::Clipped(times) = &mut video.frames {
                    *times = queue.clip.take().map_or(Vec::new(), |(_, times)| times);
                }
                let folder = folder.0.clone();
                queue.active = Some(Active {
                    making: Some(std::thread::spawn(move || video::make(&folder, &video))),
                    ..active
                });
                return;
            }
            Command::Anim(seconds) => animation.0 = *seconds,
            Command::Module {
                module: name,
                arrival,
                ..
            } => {
                module.pending = Some((name.clone(), arrival.clone()));
                module.failed = None;
            }
            Command::Wait(_)
            | Command::Screenshot(_)
            | Command::Film(..)
            | Command::Until(_)
            | Command::ClipAt(_) => {}
        }
        queue.active = Some(active);
    }

    let queue = &mut *queue;
    let settled = queue.settled;
    let clipping = queue.clip.as_ref().map(|(start, _)| *start);
    let mut clipped = None;
    let active = queue.active.as_mut().expect("set above");
    let finished = match &active.command {
        Command::Wait(WaitFor::Time(duration)) => active.since.elapsed() >= *duration,
        Command::Wait(WaitFor::Frames(frames)) => frame - active.since_frame >= u64::from(*frames),
        Command::Screenshot(name) => match &active.captured {
            None => {
                if settled >= SETTLED_FRAMES {
                    if failed > 0 {
                        warn!(target: "console", "{failed} render pipelines failed to compile");
                    }
                    let path = folder.0.join(format!("{name}.png"));
                    let done = Arc::new(AtomicBool::new(false));
                    let flag = done.clone();
                    let mut save = save_to_disk(path.clone());
                    let target = target.as_deref().expect("checked at start");
                    commands.spawn(screenshot(target)).observe(
                        move |captured: On<ScreenshotCaptured>| {
                            save(captured);
                            flag.store(true, Ordering::Release);
                        },
                    );
                    active.captured = Some((path, done));
                    active.capture_requested_at = Some(Instant::now());
                    clipped = clock.0;
                } else if active.since.elapsed() > PIPELINE_LIMIT {
                    let command = active.command.clone();
                    fail(
                        &mut exit,
                        &command,
                        &format!("{waiting} render pipelines still compiling after 60 s"),
                    );
                }
                false
            }
            Some((path, done)) => {
                let since = active.capture_requested_at.expect("set with captured");
                if !done.load(Ordering::Acquire) {
                    if since.elapsed() > CAPTURE_LIMIT {
                        let command = active.command.clone();
                        fail(&mut exit, &command, "no frame was captured within 10 s");
                    }
                    false
                } else if !path.is_file() {
                    let command = active.command.clone();
                    fail(
                        &mut exit,
                        &command,
                        &format!("{} was not written", path.display()),
                    );
                    false
                } else {
                    true
                }
            }
        },
        Command::Dump(_)
        | Command::Anim(_)
        | Command::Clip
        | Command::Ui(_)
        | Command::Vfx(_)
        | Command::Arena(_) => true,
        Command::ClipAt(ms) => match (clipping, clock.0) {
            (Some(start), Some(now)) => now >= start + ms,
            _ => true,
        },
        Command::Video(video) => {
            let made = active
                .making
                .take_if(|making| making.is_finished())
                .map(|making| {
                    making
                        .join()
                        .unwrap_or(Err("ffmpeg's thread panicked".into()))
                });
            match made {
                None => false,
                Some(Ok(true)) => true,
                Some(Ok(false)) => {
                    warn!(target: "console", "no ffmpeg: {}'s frames are kept as they are", video.name);
                    true
                }
                Some(Err(why)) => {
                    let command = active.command.clone();
                    fail(&mut exit, &command, &why);
                    false
                }
            }
        }
        Command::Film(name, duration) => {
            let target = target.as_deref().expect("checked at start");
            match &mut active.recording {
                None if settled >= SETTLED_FRAMES => {
                    active.recording = Some(Recording::new(folder.0.join(format!("{name}.rgb"))));
                    false
                }
                None => {
                    if active.since.elapsed() > PIPELINE_LIMIT {
                        let command = active.command.clone();
                        fail(
                            &mut exit,
                            &command,
                            &format!("{waiting} render pipelines still compiling after 60 s"),
                        );
                    }
                    false
                }
                Some(recording) if recording.started.elapsed() < *duration => {
                    let frames = recording.frames.clone();
                    commands.spawn(screenshot(target)).observe(
                        move |captured: On<ScreenshotCaptured>| {
                            let _ = frames.send(captured.image.clone());
                        },
                    );
                    recording
                        .times
                        .push(recording.started.elapsed().as_millis());
                    false
                }
                Some(recording) => {
                    let written = recording.written.load(Ordering::Acquire);
                    if written >= recording.times.len() {
                        let size = recording.size.lock().expect("the recording's size");
                        let size = size.map_or(String::from("0x0"), |s| format!("{}x{}", s.x, s.y));
                        let times: Vec<String> =
                            recording.times.iter().map(u128::to_string).collect();
                        let path = folder.0.join(format!("{name}.txt"));
                        let text = format!("{size}\n{}\n", times.join("\n"));
                        if let Err(error) = std::fs::write(&path, text) {
                            let command = active.command.clone();
                            fail(&mut exit, &command, &format!("{}: {error}", path.display()));
                            return;
                        }
                        true
                    } else {
                        if recording.started.elapsed() > *duration + CAPTURE_LIMIT * 3 {
                            let command = active.command.clone();
                            fail(&mut exit, &command, "frames were not written within 30 s");
                        }
                        false
                    }
                }
            }
        }

        Command::Until(state) => {
            if states.0.get(state).copied().unwrap_or(false) {
                true
            } else if active.since.elapsed() > STATE_LIMIT {
                let command = active.command.clone();
                fail(&mut exit, &command, "not reached within 120 s");
                false
            } else {
                false
            }
        }
        Command::Module { wait, .. } => {
            let wait = *wait;
            if let Some(why) = module.failed.take() {
                let command = active.command.clone();
                fail(&mut exit, &command, &why);
                false
            } else if module.pending.is_none()
                && (!wait || states.0.get("world").copied().unwrap_or(false))
            {
                true
            } else if active.since.elapsed() > STATE_LIMIT {
                let command = active.command.clone();
                fail(&mut exit, &command, "the module was not up within 120 s");
                false
            } else {
                false
            }
        }
        Command::Camera(_) => {
            if camera.0.is_none() {
                true
            } else if frame - active.since_frame > 60 {
                let command = active.command.clone();
                fail(
                    &mut exit,
                    &command,
                    "nothing placed the camera within 60 frames",
                );
                false
            } else {
                false
            }
        }
        Command::Quit | Command::Crash => unreachable!("quit and crash never become active"),
    };
    if let (Some(at), Some((_, times))) = (clipped, &mut queue.clip) {
        times.push(at);
    }
    if finished {
        info!(
            target: "console",
            "done: {} ({} frames, {} ms)",
            active.command,
            frame - active.since_frame,
            active.since.elapsed().as_millis()
        );
        queue.active = None;
    }
}

fn fail(exit: &mut MessageWriter<AppExit>, command: &Command, reason: &str) {
    error!(target: "console", "failed: {command}: {reason}");
    exit.write(AppExit::error());
}
