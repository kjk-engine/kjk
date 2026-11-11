//! `kjk anim audit --clips`: a gif of each of the worst failing rows. Each
//! row's case is run again and filmed at every audited 25 ms step around the
//! row's moment, from the duelist's side, with the sim's and JA's blades
//! drawn beside the drawn one (`--sim-blades`).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::cases::{self, Rules, STEP_MS};
use super::report::{self, CLIPS, Record, Row};
use super::{Frame, io, stepped};

/// A clip starts this long before its row's moment (ms), rounded down to a
/// step of the case's run.
pub const BEFORE_MS: u64 = 500;
/// A clip ends this long after its row's moment (ms).
pub const AFTER_MS: u64 = 1000;
/// `film` films this many of the worst failing rows.
const FILMED: usize = 20;
/// The gif's frames each second. Each frame is one `STEP_MS` step of the
/// game, so the gif plays at a quarter of the game's speed.
const GIF_FPS: u32 = 10;
/// The gif's width in pixels. Its height keeps the cropped frames' shape.
const GIF_WIDTH: u32 = 640;
/// The name the clip's screenshots take, before their number
/// (`clip-0000.png`).
const FRAME_NAME: &str = "clip";
/// The camera's vertical field of view (degrees).
const FOV: f32 = 40.0;
/// A headless run's frame is this many times as wide as it is high.
const ASPECT: f32 = 16.0 / 9.0;
/// The part of each frame the gif keeps, from and to as shares of its
/// width, over its whole height. The middle is clear of the HUD's corners
/// and of the arena's debug panel, which no command hides.
const CROP: [f32; 2] = [0.2125, 0.7875];
/// The share of the kept part's half height and half width that the
/// duelist and its blades may reach from its middle.
const FILL: f32 = 0.85;
/// The camera comes no nearer than this to the duelist's track (metres).
const NEAREST: f32 = 1.0;
/// The camera follows the pelvis averaged over this many frames on either
/// side, so that the gait's sway does not shake the view.
const SMOOTH: usize = 4;
/// The camera's turns from straight beside the duelist (degrees), tried in
/// order on its right side and then its left, until one sees the duelist
/// past every arena box.
const TURNS: [f32; 5] = [0.0, -30.0, 30.0, -60.0, 60.0];

/// Whether `ffmpeg` runs, which the clips need. The audit asks before it
/// plays any case, so a missing `ffmpeg` stops the run at once.
pub fn need_ffmpeg() -> Result<(), String> {
    let version = Command::new("ffmpeg")
        .arg("-version")
        .stdin(Stdio::null())
        .output();
    if version.is_ok_and(|output| output.status.success()) {
        Ok(())
    } else {
        Err("--clips needs ffmpeg, and it does not run".to_string())
    }
}

/// The gifs of the `FILMED` worst failing rows under MDA's rules in
/// `records`, into `out/clips`, which is emptied first so that the report
/// links no clip of an earlier run. A clip that fails is told and left out.
pub fn film(exe: &Path, out: &Path, records: &[Record]) -> Result<(), String> {
    let clips = out.join(CLIPS);
    let _ = std::fs::remove_dir_all(&clips);
    std::fs::create_dir_all(&clips).map_err(|error| io(&clips, error))?;
    let rows: Vec<(String, Row)> = report::failing(records, Rules::Mda)
        .into_iter()
        .enumerate()
        .take(FILMED)
        .map(|(rank, row)| (row.clip(rank), row))
        .collect();
    eprintln!("anim audit: filming {} clips", rows.len());
    let made = cases::at_once(&rows, |(clip, row)| {
        let made = film_row(exe, out, row, clip);
        match &made {
            Ok(()) => eprintln!("anim audit: clip {clip}"),
            Err(why) => eprintln!("anim audit: clip {clip} failed: {why}"),
        }
        made.is_ok()
    });
    eprintln!(
        "anim audit: {} of {} clips made in {}",
        made.iter().filter(|made| **made).count(),
        rows.len(),
        clips.display()
    );
    Ok(())
}

/// The gif `clip` of `row`, into `out/clips`. The run's own files are in
/// `out/runs/clip-<name>`. The case's audited frames place the camera. The
/// run is made once more when its frames cannot all be found. The frames
/// are taken out of the run's folder in order, made into the gif, and
/// removed. They are kept when `ffmpeg` fails.
fn film_row(exe: &Path, out: &Path, row: &Row, clip: &str) -> Result<(), String> {
    let stem = clip.strip_suffix(".gif").unwrap_or(clip);
    let dir = out.join("runs").join(format!("clip-{stem}"));
    std::fs::create_dir_all(&dir).map_err(|error| io(&dir, error))?;
    let moment = cases::step_floor(row.worst.ms.max(0.0) as u64);
    let from = cases::step_floor(moment.saturating_sub(BEFORE_MS));
    let to = moment + AFTER_MS;
    let log = value_of(&row.record.args, "--audit-log")
        .ok_or("the case's arguments have no --audit-log")?;
    let text = std::fs::read_to_string(log).map_err(|error| io(Path::new(log), error))?;
    let audited = super::parse(&text).map_err(|error| io(Path::new(log), error))?;
    let window: Vec<Frame> = stepped(audited)
        .into_iter()
        .filter(|frame| (from as f64 - 1.0..=to as f64 + 1.0).contains(&frame.ms))
        .collect();
    let blocks: Vec<Block> = kjk_game::arena::sight_blocks().collect();
    let Framing { shots, hidden } =
        framing(&window, &blocks).ok_or("the case drew no frame in the clip")?;
    if hidden > 0 {
        let looked: usize = window.iter().map(|frame| seen(frame).count()).sum();
        eprintln!(
            "anim audit: clip {clip}: from every side tried, an arena box hides some of the \
             pelvis, head, hands and blade tips. The best side hides {hidden} of {looked}."
        );
    }
    let count = ((to - from) / STEP_MS + 1) as usize;
    let mut cmds = cases::steps(to, |ms| {
        if ms < from {
            return Vec::new();
        }
        let number = (ms - from) / STEP_MS;
        vec![
            shots[nearest(&window, ms)].command(),
            format!("screenshot {FRAME_NAME}-{number:04}"),
        ]
    });
    cmds.push("quit".to_string());
    let mut args = with_value(&row.record.args, "--cmds", cmds.join(";"));
    args = with_value(
        &args,
        "--audit-log",
        dir.join("frames.jsonl").display().to_string(),
    );
    args.push("--sim-blades".to_string());
    let stderr = dir.join("stderr.log");
    let mut taken = Err(String::new());
    for attempt in 0..2 {
        cases::run_kjk(exe, &args, &row.record.env, &stderr)?;
        taken = screenshots(&stderr, count);
        match &taken {
            Ok(_) => break,
            Err(why) if attempt == 0 => {
                eprintln!("anim audit: clip {clip}: {why}. It runs once more.");
            }
            Err(_) => {}
        }
    }
    let taken = taken?;
    let frames = dir.join("frames");
    let _ = std::fs::remove_dir_all(&frames);
    std::fs::create_dir_all(&frames).map_err(|error| io(&frames, error))?;
    for (number, shot) in taken.iter().enumerate() {
        let frame = frames.join(format!("frame-{number:04}.png"));
        // The run's folder may be on another disk, where a rename fails.
        if std::fs::rename(shot, &frame).is_err() {
            std::fs::copy(shot, &frame).map_err(|error| io(shot, error))?;
        }
    }
    gif(&frames, &out.join(CLIPS).join(clip))
        .map_err(|why| format!("{why} (its frames are kept in {})", frames.display()))?;
    let _ = std::fs::remove_dir_all(&frames);
    Ok(())
}

/// The `count` screenshots of a run whose error output is in `stderr`, in
/// order, from the run's folder that the error output names. A run folder
/// named relative to the working folder is taken from the audit's own
/// working folder, which the run shares.
fn screenshots(stderr: &Path, count: usize) -> Result<Vec<PathBuf>, String> {
    let log = std::fs::read_to_string(stderr).map_err(|error| io(stderr, error))?;
    let run = crate::tour::run_folder(&log).ok_or("the run named no run folder")?;
    let run = std::path::absolute(&run).map_err(|error| io(&run, error))?;
    let prefix = format!("{FRAME_NAME}-");
    let mut shots: Vec<PathBuf> = std::fs::read_dir(&run)
        .map_err(|error| io(&run, error))?
        .filter_map(|entry| Some(entry.ok()?.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix) && name.ends_with(".png"))
        })
        .collect();
    shots.sort();
    if shots.len() == count {
        Ok(shots)
    } else {
        Err(format!(
            "{} has {} of the clip's {count} frames",
            run.display(),
            shots.len()
        ))
    }
}

/// A box that hides what is behind it (min, max corners, metres).
type Block = ([f32; 3], [f32; 3]);

/// Where the camera stands for one frame, and the way it looks (degrees
/// from the x axis). It looks level.
struct Shot {
    at: [f32; 3],
    yaw: f32,
}

impl Shot {
    /// The console command that places the camera.
    fn command(&self) -> String {
        let [x, y, z] = self.at;
        format!("camera {x:.3} {y:.3} {z:.3} {:.2} 0 {FOV}", self.yaw)
    }
}

/// The index of the frame of `frames` drawn nearest to `ms`.
fn nearest(frames: &[Frame], ms: u64) -> usize {
    let ms = ms as f64;
    (0..frames.len())
        .min_by(|&a, &b| {
            (frames[a].ms - ms)
                .abs()
                .total_cmp(&(frames[b].ms - ms).abs())
        })
        .unwrap_or(0)
}

/// Every place of `frame` the clip must show: its nodes, and both ends of
/// each blade as drawn, as JA's skeleton holds it, and as the sim has it.
fn points(frame: &Frame) -> impl Iterator<Item = [f32; 3]> + '_ {
    frame
        .nodes
        .values()
        .copied()
        .chain(frame.blades.iter().flat_map(|blade| {
            [blade.drawn, blade.reference, blade.sim]
                .into_iter()
                .flatten()
        }))
}

/// The places of `frame` the camera must see past the arena's boxes: the
/// pelvis, the head, the hands and the drawn blades' tips.
fn seen(frame: &Frame) -> impl Iterator<Item = [f32; 3]> + '_ {
    ["pelvis_g", "headhook", "rhand_g", "lhand_g"]
        .into_iter()
        .filter_map(|node| frame.nodes.get(node).copied())
        .chain(frame.blades.iter().map(|blade| blade.drawn[1]))
}

/// The camera of each of `frames`, and how often one of `blocks` hides the
/// duelist from it. The camera looks level from beside the duelist
/// (its shoulders' line), at the height halfway between the lowest and the
/// highest place of the clip. It follows the smoothed pelvis, and it stands
/// as near as it can while every place of every frame stays inside `FILL`
/// of the kept part of the frame. The first of `TURNS` on either side that
/// sees past every box is taken. When none does, the one that a box hides
/// least often is taken. None without a frame.
fn framing(frames: &[Frame], blocks: &[Block]) -> Option<Framing> {
    if frames.is_empty() {
        return None;
    }
    let pelvis: Vec<[f32; 2]> = frames
        .iter()
        .map(|frame| {
            let at = frame
                .nodes
                .get("pelvis_g")
                .copied()
                .unwrap_or_else(|| mean(points(frame)));
            [at[0], at[1]]
        })
        .collect();
    let track: Vec<[f32; 2]> = (0..frames.len())
        .map(|index| {
            let near =
                &pelvis[index.saturating_sub(SMOOTH)..(index + SMOOTH + 1).min(frames.len())];
            let [x, y] = near
                .iter()
                .fold([0.0, 0.0], |sum, at| [sum[0] + at[0], sum[1] + at[1]]);
            [x / near.len() as f32, y / near.len() as f32]
        })
        .collect();
    let (low, high) = frames
        .iter()
        .flat_map(points)
        .fold((f32::MAX, f32::MIN), |(low, high), at| {
            (low.min(at[2]), high.max(at[2]))
        });
    let middle = (low + high) / 2.0;
    // The duelist's right, from the left shoulder to the right one.
    let right = frames
        .iter()
        .filter_map(|frame| Some((frame.nodes.get("lbicep_g")?, frame.nodes.get("rbicep_g")?)))
        .fold([0.0, 0.0], |sum, (left, right)| {
            [sum[0] + right[0] - left[0], sum[1] + right[1] - left[1]]
        });
    let right = unit(right).unwrap_or([1.0, 0.0]);
    let half_high = FILL * (FOV.to_radians() / 2.0).tan();
    let half_wide = half_high * ASPECT * (CROP[1] - CROP[0]);
    let candidates = [1.0, -1.0].into_iter().flat_map(|side: f32| {
        TURNS.into_iter().map(move |turn| {
            let (sin, cos) = turn.to_radians().sin_cos();
            let [x, y] = right;
            [side * (x * cos - y * sin), side * (x * sin + y * cos)]
        })
    });
    let shoot = |toward: [f32; 2]| {
        let across = [-toward[1], toward[0]];
        let mut distance = NEAREST;
        for (frame, centre) in frames.iter().zip(&track) {
            for at in points(frame) {
                let offset = [at[0] - centre[0], at[1] - centre[1]];
                let along = offset[0] * toward[0] + offset[1] * toward[1];
                let aside = (offset[0] * across[0] + offset[1] * across[1]).abs();
                let up = (at[2] - middle).abs();
                distance = distance
                    .max(along + aside / half_wide)
                    .max(along + up / half_high);
            }
        }
        let yaw = (-toward[1]).atan2(-toward[0]).to_degrees();
        let shots: Vec<Shot> = track
            .iter()
            .map(|centre| Shot {
                at: [
                    centre[0] + toward[0] * distance,
                    centre[1] + toward[1] * distance,
                    middle,
                ],
                yaw,
            })
            .collect();
        // A place inside a box (a blade through it) is hidden from every
        // side, so that box does not count against any.
        let hidden = frames
            .iter()
            .zip(&shots)
            .flat_map(|(frame, shot)| seen(frame).map(move |at| (shot.at, at)))
            .filter(|(from, at)| {
                blocks
                    .iter()
                    .any(|block| !inside(*at, block) && crosses(*from, *at, block))
            })
            .count();
        Framing { shots, hidden }
    };
    let mut best: Option<Framing> = None;
    for toward in candidates {
        let framing = shoot(toward);
        if framing.hidden == 0 {
            return Some(framing);
        }
        if best
            .as_ref()
            .is_none_or(|best| framing.hidden < best.hidden)
        {
            best = Some(framing);
        }
    }
    best
}

/// The cameras of a clip's frames, and how many of the places the camera
/// must see (`seen`) a box hides from it.
struct Framing {
    shots: Vec<Shot>,
    hidden: usize,
}

/// Whether `at` is inside `block`.
fn inside(at: [f32; 3], (min, max): &Block) -> bool {
    (0..3).all(|axis| min[axis] <= at[axis] && at[axis] <= max[axis])
}

/// The mean of `places`, or the origin when there is none.
fn mean(places: impl Iterator<Item = [f32; 3]>) -> [f32; 3] {
    let (sum, count) = places.fold(([0.0; 3], 0), |(sum, count), at| {
        ([sum[0] + at[0], sum[1] + at[1], sum[2] + at[2]], count + 1)
    });
    if count == 0 {
        sum
    } else {
        sum.map(|value| value / count as f32)
    }
}

/// `vector` made one long, or none when it has no length.
fn unit([x, y]: [f32; 2]) -> Option<[f32; 2]> {
    let length = x.hypot(y);
    (length > 1e-6).then(|| [x / length, y / length])
}

/// Whether the straight line from `from` to `to` passes through `block`.
fn crosses(from: [f32; 3], to: [f32; 3], (min, max): &Block) -> bool {
    let (mut enter, mut leave) = (0.0_f32, 1.0_f32);
    for axis in 0..3 {
        let step = to[axis] - from[axis];
        if step.abs() < 1e-6 {
            if from[axis] < min[axis] || from[axis] > max[axis] {
                return false;
            }
            continue;
        }
        let a = (min[axis] - from[axis]) / step;
        let b = (max[axis] - from[axis]) / step;
        enter = enter.max(a.min(b));
        leave = leave.min(a.max(b));
        if enter > leave {
            return false;
        }
    }
    true
}

/// The value after `flag` in `args`.
fn value_of<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    let at = args.iter().position(|arg| arg == flag)?;
    args.get(at + 1).map(String::as_str)
}

/// `args` with the value after `flag` set to `value`, or with both added at
/// the end when `flag` is not among them.
fn with_value(args: &[String], flag: &str, value: String) -> Vec<String> {
    let mut args = args.to_vec();
    match args.iter().position(|arg| arg == flag) {
        Some(at) if at + 1 < args.len() => args[at + 1] = value,
        _ => args.extend([flag.to_string(), value]),
    }
    args
}

/// The gif `gif` of the numbered frames in `frames` (`frame-0000.png`),
/// cropped to `CROP`, at `GIF_FPS` and `GIF_WIDTH`, with a palette made
/// from the frames.
fn gif(frames: &Path, gif: &Path) -> Result<(), String> {
    let [left, right] = CROP;
    let filter = format!(
        "crop=iw*{}:ih:iw*{left}:0,scale={GIF_WIDTH}:-2:flags=lanczos,\
         split[a][b];[a]palettegen[p];[b][p]paletteuse",
        right - left
    );
    let output = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-framerate"])
        .arg(GIF_FPS.to_string())
        .arg("-i")
        .arg(frames.join("frame-%04d.png"))
        .args(["-vf", &filter, "-loop", "0"])
        .arg(gif)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("ffmpeg: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "ffmpeg: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}
