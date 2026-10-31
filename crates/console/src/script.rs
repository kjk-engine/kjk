//! The command language: commands separated by `;` or new lines, each a verb
//! and its arguments.

use std::fmt;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Waits for a duration of wall time or a number of rendered frames.
    Wait(WaitFor),
    /// Captures the rendered frame to `<name>.png` in the run folder.
    Screenshot(String),
    /// Captures every rendered frame for a duration of wall time to
    /// `<name>.rgb` (raw RGB, one frame after another), with `<name>.txt`
    /// giving their size (`1280x720`), then each frame's ms since the first:
    /// what a video of them is timed by.
    Film(String, Duration),
    /// Writes the runtime's state to `<name>.json` in the run folder.
    Dump(String),
    /// Puts the camera at a point (game space, metres) looking along a yaw
    /// (degrees about Z from +X) and pitch (degrees, up positive), with an
    /// optional vertical field of view (degrees).
    Camera(CameraPose),
    /// Freezes animation time (scrolling textures, flip-books) at a number
    /// of seconds, or lets it run again (`anim run`), so shots repeat.
    Anim(Option<f32>),
    /// Waits until a state the runtime publishes is true (`wait world`: no
    /// module is loading and the scene is up; `wait loading`: one is).
    Until(String),
    /// Installs a module, arriving at a door or waypoint (or its entry),
    /// and finishes when the world is up in it (`module`), or as soon as
    /// the game takes the request (`travel`: what follows sees the loading
    /// screen).
    Module {
        module: String,
        arrival: Option<String>,
        wait: bool,
    },
    /// A request to the game's screens (`ui open inventory`, `ui press
    /// BTN_OK`, `ui nav down`, `ui type 127.0.0.1`), handed over at once.
    Ui(Vec<String>),
    /// A visual effect for the view to play, as a script's would be (`vfx
    /// 2007 end_trask 9`: `visualeffects.2da` row, the creature's tag,
    /// seconds it lasts, a beam's source's tag), handed over at once.
    Vfx(Vec<String>),
    /// An order in the combat arena, as its keys give it (`arena spawn
    /// g_sithtroop01`, `arena clear`, `arena god`), handed over at once.
    Arena(Vec<String>),
    /// Makes `<name>.mp4` of the screenshots `<name>-0000.png` on, with
    /// the run's sound (`audio.wav`, on the game's clock) from their first
    /// moment, when the run has one. `record` and `clip` end with it.
    Video(Video),
    /// Starts a clip: the game's time now is its start, and every
    /// screenshot until its video notes the game's time it shows.
    Clip,
    /// Waits until the game's time is this many ms past the clip's start.
    ClipAt(f64),
    /// Ends the run with success.
    Quit,
    /// Panics, to prove what a crash leaves in the run's folder.
    Crash,
}

/// A video of `frames` screenshots.
#[derive(Debug, Clone, PartialEq)]
pub struct Video {
    pub name: String,
    pub frames: Frames,
}

/// When a video's frames are, in the game's time.
#[derive(Debug, Clone, PartialEq)]
pub enum Frames {
    /// `count` frames `fps` a second from `from` seconds (`record`).
    Even { from: f32, fps: u32, count: u32 },
    /// The clip's screenshots, each at the game's time it showed (ms;
    /// filled in when the clip ends).
    Clipped(Vec<f64>),
}

const RECORD_USAGE: &str = "record takes a name, from and to (seconds of game time) and fps";
/// `record`'s frames a second when it names none.
const RECORD_FPS: u32 = 30;
const CLIP_USAGE: &str = "clip takes a name, seconds of game time and fps";
/// `clip`'s frames a second when it names none: about what a headless run
/// captures while the game runs.
const CLIP_FPS: u32 = 12;

const CAMERA_USAGE: &str = "camera takes five or six numbers: x y z yaw pitch [fov]";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    /// Vertical field of view in degrees; `None` keeps the current one.
    pub fov: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WaitFor {
    Time(Duration),
    Frames(u32),
}

impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Command::Wait(WaitFor::Time(duration)) => write!(f, "wait {}ms", duration.as_millis()),
            Command::Wait(WaitFor::Frames(frames)) => write!(f, "wait {frames}f"),
            Command::Screenshot(name) => write!(f, "screenshot {name}"),
            Command::Film(name, duration) => {
                write!(f, "film {}ms {name}", duration.as_millis())
            }
            Command::Dump(name) => write!(f, "dump {name}"),
            Command::Camera(pose) => {
                write!(
                    f,
                    "camera {} {} {} {} {}",
                    pose.position[0], pose.position[1], pose.position[2], pose.yaw, pose.pitch
                )?;
                match pose.fov {
                    Some(fov) => write!(f, " {fov}"),
                    None => Ok(()),
                }
            }
            Command::Anim(Some(seconds)) => write!(f, "anim {seconds}"),
            Command::Anim(None) => f.write_str("anim run"),
            Command::Until(state) => write!(f, "wait {state}"),
            Command::Module {
                module,
                arrival,
                wait,
            } => {
                let verb = if *wait { "module" } else { "travel" };
                write!(f, "{verb} {module}")?;
                match arrival {
                    Some(arrival) => write!(f, " {arrival}"),
                    None => Ok(()),
                }
            }
            Command::Ui(words) => write!(f, "ui {}", words.join(" ")),
            Command::Vfx(words) => write!(f, "vfx {}", words.join(" ")),
            Command::Arena(words) => write!(f, "arena {}", words.join(" ")),
            Command::Video(Video {
                name,
                frames: Frames::Even { from, fps, count },
            }) => write!(
                f,
                "video {name} ({count} frames at {fps} fps from {from} s)"
            ),
            Command::Video(Video {
                name,
                frames: Frames::Clipped(_),
            }) => write!(f, "video {name} (clipped)"),
            Command::Clip => f.write_str("clip"),
            Command::ClipAt(ms) => write!(f, "clip at {ms} ms"),
            Command::Quit => f.write_str("quit"),
            Command::Crash => f.write_str("crash"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    /// 1-based position of the command in the script.
    pub index: usize,
    pub text: String,
    pub reason: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "command {} ({:?}): {}",
            self.index, self.text, self.reason
        )
    }
}

impl std::error::Error for ParseError {}

/// Parses a whole script. Names default to a counter per verb so that two
/// screenshots in one script never overwrite each other.
pub fn parse(script: &str) -> Result<Vec<Command>, ParseError> {
    let mut commands = Vec::new();
    let (mut screenshots, mut dumps, mut films) = (0, 0, 0);
    let pieces = script
        .split([';', '\n'])
        .map(str::trim)
        .filter(|piece| !piece.is_empty());
    for (index, text) in pieces.enumerate() {
        let error = |reason: &str| ParseError {
            index: index + 1,
            text: text.to_string(),
            reason: reason.to_string(),
        };
        let mut words = text.split_whitespace();
        let verb = words.next().expect("pieces are not empty");
        let args: Vec<&str> = words.collect();
        let command = match (verb, args.as_slice()) {
            ("wait", [amount]) => match wait_for(amount) {
                Some(wait) => Command::Wait(wait),
                None if amount.chars().all(|c| c.is_ascii_lowercase() || c == '_') => {
                    Command::Until(amount.to_string())
                }
                None => {
                    return Err(error(
                        "expected a duration like 2s or 500ms, frames like 30f, or a state like world",
                    ));
                }
            },
            ("module" | "travel", [module, rest @ ..]) if rest.len() <= 1 => Command::Module {
                module: module.to_ascii_lowercase(),
                arrival: rest.first().map(|arrival| arrival.to_ascii_lowercase()),
                wait: verb == "module",
            },
            ("module" | "travel", _) => {
                return Err(error(&format!("{verb} takes a module and an arrival tag")));
            }
            ("wait", _) => return Err(error("wait takes one argument")),
            ("screenshot", []) => {
                screenshots += 1;
                Command::Screenshot(format!("screenshot-{screenshots}"))
            }
            ("screenshot", [name]) => Command::Screenshot(
                file_stem(name).ok_or_else(|| error("names use letters, digits, '-' and '_'"))?,
            ),
            ("film", [amount, rest @ ..]) if rest.len() <= 1 => {
                let Some(WaitFor::Time(duration)) = wait_for(amount) else {
                    return Err(error("film takes a duration like 3s, then a name"));
                };
                let name = match rest {
                    [name] => file_stem(name)
                        .ok_or_else(|| error("names use letters, digits, '-' and '_'"))?,
                    _ => {
                        films += 1;
                        format!("film-{films}")
                    }
                };
                Command::Film(name, duration)
            }
            ("film", _) => return Err(error("film takes a duration like 3s, then a name")),
            ("dump", []) => {
                dumps += 1;
                Command::Dump(format!("dump-{dumps}"))
            }
            ("dump", [name]) => Command::Dump(
                file_stem(name).ok_or_else(|| error("names use letters, digits, '-' and '_'"))?,
            ),
            ("quit", []) => Command::Quit,
            ("crash", []) => Command::Crash,
            ("ui", words) if !words.is_empty() => {
                Command::Ui(words.iter().map(|word| word.to_string()).collect())
            }
            ("ui", _) => return Err(error("ui takes a request, e.g. ui open main_menu")),
            ("vfx", [row, tag, rest @ ..]) if rest.len() <= 2 => Command::Vfx(
                std::iter::once(row)
                    .chain([tag])
                    .chain(rest)
                    .map(|word| word.to_string())
                    .collect(),
            ),
            ("vfx", _) => {
                return Err(error(
                    "vfx takes a row, a tag, then seconds and a beam's source",
                ));
            }
            ("arena", ["spawn", template]) => {
                Command::Arena(vec!["spawn".into(), template.to_string()])
            }
            ("arena", [order @ ("clear" | "god")]) => Command::Arena(vec![order.to_string()]),
            ("arena", ["down", n]) if n.parse::<u8>().is_ok() => {
                Command::Arena(vec!["down".into(), n.to_string()])
            }
            ("arena", _) => {
                return Err(error("arena takes spawn TEMPLATE, clear, god or down N"));
            }
            ("camera", words) if words.len() == 5 || words.len() == 6 => {
                let numbers: Vec<f32> = words.iter().filter_map(|word| word.parse().ok()).collect();
                if numbers.len() != words.len() || numbers.iter().any(|n: &f32| !n.is_finite()) {
                    return Err(error(CAMERA_USAGE));
                }
                let fov = numbers.get(5).copied();
                if fov.is_some_and(|fov| !(1.0..=170.0).contains(&fov)) {
                    return Err(error("camera: fov is degrees between 1 and 170"));
                }
                Command::Camera(CameraPose {
                    position: [numbers[0], numbers[1], numbers[2]],
                    yaw: numbers[3],
                    pitch: numbers[4],
                    fov,
                })
            }
            ("camera", _) => return Err(error(CAMERA_USAGE)),
            ("anim", ["run"]) => Command::Anim(None),
            ("anim", [seconds]) => Command::Anim(Some(
                seconds
                    .parse::<f32>()
                    .ok()
                    .filter(|s| s.is_finite() && *s >= 0.0)
                    .ok_or_else(|| error("anim takes seconds (0 or more) or `run`"))?,
            )),
            ("anim", _) => return Err(error("anim takes seconds (0 or more) or `run`")),
            ("record", [name, from, to, fps @ ..]) if fps.len() <= 1 => {
                let name = file_stem(name)
                    .ok_or_else(|| error("names use letters, digits, '-' and '_'"))?;
                let seconds = |word: &str| {
                    word.parse::<f32>()
                        .ok()
                        .filter(|s| s.is_finite() && *s >= 0.0)
                };
                let (Some(from), Some(to)) = (seconds(from), seconds(to)) else {
                    return Err(error(RECORD_USAGE));
                };
                let fps = match fps {
                    [] => RECORD_FPS,
                    [fps] => fps
                        .parse()
                        .ok()
                        .filter(|fps| (1..=120).contains(fps))
                        .ok_or_else(|| error("record: fps is 1 to 120"))?,
                    _ => unreachable!("guarded"),
                };
                if to <= from {
                    return Err(error("record: to comes after from"));
                }
                commands.extend(record(name, from, to, fps));
                continue;
            }
            ("record", _) => return Err(error(RECORD_USAGE)),
            ("clip", [name, seconds, fps @ ..]) if fps.len() <= 1 => {
                let name = file_stem(name)
                    .ok_or_else(|| error("names use letters, digits, '-' and '_'"))?;
                let seconds = seconds
                    .parse::<f32>()
                    .ok()
                    .filter(|s| s.is_finite() && *s > 0.0)
                    .ok_or_else(|| error(CLIP_USAGE))?;
                let fps = match fps {
                    [] => CLIP_FPS,
                    [fps] => fps
                        .parse()
                        .ok()
                        .filter(|fps| (1..=60).contains(fps))
                        .ok_or_else(|| error("clip: fps is 1 to 60"))?,
                    _ => unreachable!("guarded"),
                };
                commands.extend(clip(name, seconds, fps));
                continue;
            }
            ("clip", _) => return Err(error(CLIP_USAGE)),
            ("screenshot" | "dump" | "quit" | "crash", _) => {
                return Err(error("too many arguments"));
            }
            _ => return Err(error("unknown command")),
        };
        commands.push(command);
    }
    Ok(commands)
}

/// A recording: game time held at each frame's moment in turn, a
/// screenshot of each, then the video of them.
fn record(name: String, from: f32, to: f32, fps: u32) -> Vec<Command> {
    let frames = ((to - from) * fps as f32).round().max(1.0) as u32;
    let mut commands: Vec<Command> = (0..frames)
        .flat_map(|frame| {
            [
                Command::Anim(Some(from + frame as f32 / fps as f32)),
                Command::Screenshot(format!("{name}-{frame:04}")),
            ]
        })
        .collect();
    commands.push(Command::Video(Video {
        name,
        frames: Frames::Even {
            from,
            fps,
            count: frames,
        },
    }));
    commands
}

/// A clip: the game running on, a screenshot when each frame's moment has
/// come (later when capturing falls behind), then the video of them.
fn clip(name: String, seconds: f32, fps: u32) -> Vec<Command> {
    let frames = (seconds * fps as f32).round().max(1.0) as u32;
    let mut commands = vec![Command::Clip];
    for frame in 0..frames {
        commands.push(Command::ClipAt(f64::from(frame) * 1000.0 / f64::from(fps)));
        commands.push(Command::Screenshot(format!("{name}-{frame:04}")));
    }
    commands.push(Command::Video(Video {
        name,
        frames: Frames::Clipped(Vec::new()),
    }));
    commands
}

fn wait_for(amount: &str) -> Option<WaitFor> {
    let number = |suffix: &str| amount.strip_suffix(suffix)?.parse::<f64>().ok();
    if let Some(frames) = amount.strip_suffix('f') {
        return frames.parse().ok().map(WaitFor::Frames);
    }
    let seconds = number("ms").map(|ms| ms / 1000.0).or_else(|| number("s"))?;
    (seconds.is_finite() && seconds >= 0.0).then(|| WaitFor::Time(Duration::from_secs_f64(seconds)))
}

fn file_stem(name: &str) -> Option<String> {
    let ok = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| name.to_string())
}
