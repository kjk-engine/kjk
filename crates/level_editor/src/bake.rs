//! Baking the fitted lights from the editor (`docs/LEVEL-EDIT.md`, "Bakes"):
//! what `tools/light-kinds/bake.sh` does, with the editor's own scene in
//! place of a launch of the game.
//!
//! The editor saves, then writes the lights of the scene it draws to a
//! dump (`fitted_lights::dump_lights`), as the game writes them for
//! `KJK_FITTED_DUMP`. `kjk detail-bake --gpu` bakes from it on a thread of
//! its own while the editor keeps working, set as `data/lights/bake.toml`
//! sets the module:
//!
//! - a module with a levels file bakes once, held to that file;
//! - a module with a share bakes twice: once to measure each room's level,
//!   then held to the share of it (`level_edit::levels::levels_from_log`).
//!
//! The bake lands where the scripts put theirs,
//! `target/light-kinds/game/bake-<module>/bake/<label>`, with its dump,
//! levels and logs beside it, so the Baked view lists it with theirs.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

/// A bake running.
pub struct BakeJob {
    pub label: String,
    pub started: std::time::Instant,
    /// What it is doing now.
    pub status: String,
    steps: Mutex<Receiver<Step>>,
}

/// What a bake's thread says.
enum Step {
    Status(String),
    /// The baked lightmaps' folder (`.../<area>/b`), or why there is none.
    Done(Result<PathBuf, String>),
}

/// What a bake needs, gathered before its thread starts.
pub struct Plan {
    /// The `kjk` program, which `detail-bake` is a verb of.
    pub program: PathBuf,
    /// The project, passed as `--project` so the bake reads its module.
    pub project: String,
    pub module: String,
    pub game: String,
    /// The light data folder, which the module's levels file is relative
    /// to.
    pub light_data: PathBuf,
    pub entry: level_edit::levels::BakeEntry,
    /// The level each room aims at as the project has changed them, when
    /// it has: the bake is held to these, not to the light data's.
    pub own_levels: Option<std::collections::BTreeMap<String, f32>>,
    /// The area's scratch folder (`target/light-kinds/game/bake-<module>`).
    pub folder: PathBuf,
    pub label: String,
    /// The dump of the scene's lights, written already.
    pub dump: PathBuf,
}

/// The folder of a module's bakes in the kjk checkout at `root`.
pub fn folder_of(root: &Path, module: &str) -> PathBuf {
    root.join("target")
        .join("light-kinds")
        .join("game")
        .join(format!("bake-{module}"))
}

/// A label for a bake made now: the month, day and time, as `bake.sh`
/// makes them, with the seconds, and `ed` for the editor.
pub fn label_now() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    // The day's number since 1970 and the time of day, in UTC.
    let (days, of_day) = (seconds / 86_400, seconds % 86_400);
    let (_, month_day) = civil(days);
    format!(
        "{month_day}-{:02}{:02}{:02}-ed",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

/// The month and day (`MMDD`) of the day `days` after 1 January 1970, with
/// its year.
fn civil(days: u64) -> (i64, String) {
    // Howard Hinnant's civil-from-days, for the proleptic Gregorian
    // calendar.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, format!("{month:02}{day:02}"))
}

impl BakeJob {
    /// Starts `plan`'s bake on a thread of its own.
    pub fn start(plan: Plan) -> BakeJob {
        let (send, steps) = channel();
        let label = plan.label.clone();
        std::thread::Builder::new()
            .name("bake".into())
            .spawn(move || {
                let done = run(&plan, &send);
                let _ = send.send(Step::Done(done));
            })
            .expect("a thread starts");
        BakeJob {
            label,
            started: std::time::Instant::now(),
            status: "starting".into(),
            steps: Mutex::new(steps),
        }
    }

    /// What the thread said since: its new status, and at its end the bake
    /// (`Some`).
    pub fn poll(&mut self) -> Option<Result<PathBuf, String>> {
        use std::sync::mpsc::TryRecvError;
        let Ok(steps) = self.steps.lock() else {
            return Some(Err("the bake's thread failed".into()));
        };
        loop {
            match steps.try_recv() {
                Ok(Step::Status(status)) => self.status = status,
                Ok(Step::Done(done)) => return Some(done),
                Err(TryRecvError::Empty) => return None,
                // The thread is gone and sent no end: the bake is over, so
                // another can start.
                Err(TryRecvError::Disconnected) => {
                    return Some(Err("the bake's thread ended without a result".into()));
                }
            }
        }
    }
}

/// One `kjk detail-bake --gpu` into `out`, with its output in `log`.
fn detail_bake(
    plan: &Plan,
    levels: Option<&Path>,
    weigh: bool,
    out: &Path,
    log: &Path,
) -> Result<String, String> {
    let mut bake = std::process::Command::new(&plan.program);
    bake.args([
        "--project",
        &plan.project,
        "detail-bake",
        &plan.module,
        "--game",
        &plan.game,
    ]);
    bake.arg("--lights").arg(&plan.dump).arg("--gpu");
    if let Some(sun) = &plan.entry.sun_args {
        bake.args(["--sun", sun]);
    }
    if let Some(skylights) = &plan.entry.skylights {
        bake.args(["--skylights", skylights]);
    }
    if levels.is_some() {
        if !plan.entry.even {
            bake.arg("--no-even");
        }
        if weigh {
            bake.arg("--weigh");
        }
        // The module's other flags, as `bake.sh` passes them.
        if plan.entry.fill_stored {
            bake.arg("--fill-stored");
        }
        if let Some(least) = plan.entry.least {
            bake.arg("--least").arg(least.to_string());
        }
        if let Some(most) = plan.entry.most {
            bake.arg("--most").arg(most.to_string());
        }
        if let Some(floor) = plan.entry.floor {
            bake.arg("--floor").arg(floor.to_string());
        }
        if plan.entry.every_room {
            bake.arg("--every-room");
        }
        if plan.entry.stored_backdrop {
            bake.arg("--stored-backdrop");
        }
    }
    if let Some(levels) = levels {
        bake.arg("--levels").arg(levels);
    }
    bake.arg("--out").arg(out);
    let output = crate::child::output(bake)
        .map_err(|error| format!("kjk detail-bake did not start: {error}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::write(log, &text);
    if !output.status.success() {
        let tail: Vec<&str> = text.lines().rev().take(4).collect();
        return Err(format!(
            "kjk detail-bake failed ({}): {}",
            log.display(),
            tail.into_iter().rev().collect::<Vec<_>>().join(" / ")
        ));
    }
    Ok(text)
}

/// The bake (`bake.sh`'s two ways), telling `send` how far it is.
fn run(plan: &Plan, send: &Sender<Step>) -> Result<PathBuf, String> {
    let say = |status: String| {
        let _ = send.send(Step::Status(status));
    };
    let bakes = plan.folder.join("bake");
    std::fs::create_dir_all(&bakes).map_err(|error| format!("{}: {error}", bakes.display()))?;
    let label = &plan.label;
    let levels_path = plan.folder.join(format!("levels-{label}.txt"));
    let weigh = plan.entry.of == "lit";
    match (&plan.entry.levels, plan.entry.share) {
        _ if plan.own_levels.is_some() => {
            let levels = (plan.own_levels.clone()).unwrap_or_else(std::collections::BTreeMap::new);
            let text = level_edit::levels::write_levels(
                None,
                &levels,
                &format!(
                    "The level each room of {} aims at, as the project {} has them.",
                    plan.module, plan.project
                ),
            );
            std::fs::write(&levels_path, text)
                .map_err(|error| format!("{}: {error}", levels_path.display()))?;
            say(format!(
                "baking, {} rooms held to the project's levels",
                levels.len()
            ));
        }
        (Some(file), _) => {
            // Each level of the file times the entry's `scale`, as
            // `bake.sh` writes them.
            let source = plan.light_data.join(file);
            let scale = plan.entry.scale.unwrap_or(1.0);
            let text = std::fs::read_to_string(&source)
                .map_err(|error| format!("{}: {error}", source.display()))?;
            let mut scaled = String::new();
            for line in text.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                let level = match parts.as_slice() {
                    [_, level] if !line.starts_with('#') => level.parse::<f32>().ok(),
                    _ => None,
                };
                match level {
                    Some(level) => {
                        scaled.push_str(&format!("{} {:.4}\n", parts[0], level * scale));
                    }
                    None => {
                        scaled.push_str(line);
                        scaled.push('\n');
                    }
                }
            }
            std::fs::write(&levels_path, scaled)
                .map_err(|error| format!("{}: {error}", levels_path.display()))?;
            say(format!("baking, held to {file}"));
        }
        (None, Some(share)) => {
            say("measuring each room (the first of two bakes)".into());
            let first = bakes.join(format!("{label}-first"));
            let log = detail_bake(
                plan,
                None,
                false,
                &first,
                &plan.folder.join(format!("first-{label}.log")),
            )?;
            let levels = level_edit::levels::levels_from_log(
                &log,
                &plan.entry.of,
                share,
                plan.entry.ceiling,
            );
            if levels.is_empty() {
                return Err(format!(
                    "the first bake gave no {} room levels",
                    plan.entry.of
                ));
            }
            let text = level_edit::levels::write_levels(
                None,
                &levels,
                &format!(
                    "{share} of the {} level of each room of module {}, and at most {}.",
                    plan.entry.of,
                    plan.module,
                    plan.entry
                        .ceiling
                        .map_or_else(|| "none".to_string(), |most| most.to_string())
                ),
            );
            std::fs::write(&levels_path, text)
                .map_err(|error| format!("{}: {error}", levels_path.display()))?;
            say(format!(
                "baking, {} rooms held to their levels",
                levels.len()
            ));
        }
        (None, None) => {
            return Err(format!(
                "data/lights/bake.toml gives {} no level",
                plan.module
            ));
        }
    }
    let out = bakes.join(label);
    let log = detail_bake(
        plan,
        Some(&levels_path),
        weigh,
        &out,
        &plan.folder.join(format!("bake-{label}.log")),
    )?;
    let mean = log
        .lines()
        .find(|line| line.contains("mean stored level"))
        .map(str::trim)
        .unwrap_or("");
    say(format!("baked {mean}"));
    // The bake writes one folder, its area's, with set `b` in it.
    let area = std::fs::read_dir(&out)
        .map_err(|error| format!("{}: {error}", out.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("b").is_dir())
        .ok_or_else(|| format!("{} holds no baked set b", out.display()))?;
    let set = area.join("b");
    // A level whose entry asks for dark surfaces with no lightmap gets
    // `unlit.txt` in its set, as `bake.sh` writes it.
    if plan.entry.dark_unlit {
        let note = set.join(level_edit::lighting::UNLIT);
        std::fs::write(&note, "dark\n").map_err(|error| format!("{}: {error}", note.display()))?;
    }
    Ok(set)
}
