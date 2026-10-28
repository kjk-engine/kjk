//! `kjk tour <file>`: render a list of viewpoints headless, one run per
//! module (this program started again as a child), gather the shots into a
//! folder and a contact sheet, and compare each with the previous run of the
//! same tour.
//!
//! The tour file is ours (names, modules, coordinates); see
//! `tours/showcase.tour` for the format.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::time::{SystemTime, UNIX_EPOCH};

use asset_odyssey::module::ModuleIr;
use image::{Rgb, RgbImage};
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

use crate::modules::ResolverSource;

const EYE_HEIGHT: f32 = crate::cast::HEAD_M;
const DEFAULT_PITCH: f32 = -5.0;
/// A tour shot's field of view where its line gives none: degrees across,
/// wider than the play camera's, so a stop shows a room more than a face.
const DEFAULT_FOV: f32 = 75.0;

#[derive(Debug, Clone)]
enum Place {
    /// Yaw offset from the entry direction, pitch, fov.
    Entry(f32, f32, f32),
    /// x, y, z, yaw, pitch, fov.
    At([f32; 6]),
}

#[derive(Debug, Clone)]
struct Stop {
    name: String,
    game: String,
    module: String,
    place: Place,
}

fn parse(text: &str) -> Result<Vec<Stop>, String> {
    let mut stops = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        let fail = |why: &str| format!("line {}: {why}: {line}", number + 1);
        if words.len() < 4 {
            return Err(fail("expected <name> <game> <module> entry|at ..."));
        }
        let numbers: Vec<f32> = words[4..]
            .iter()
            .map(|word| word.parse::<f32>().map_err(|_| fail("not a number")))
            .collect::<Result<_, _>>()?;
        let place = match (words[3], numbers.len()) {
            ("entry", 0..=3) => Place::Entry(
                numbers.first().copied().unwrap_or(0.0),
                numbers.get(1).copied().unwrap_or(DEFAULT_PITCH),
                numbers.get(2).copied().unwrap_or(DEFAULT_FOV),
            ),
            ("at", 5 | 6) => Place::At([
                numbers[0],
                numbers[1],
                numbers[2],
                numbers[3],
                numbers[4],
                numbers.get(5).copied().unwrap_or(DEFAULT_FOV),
            ]),
            _ => {
                return Err(fail(
                    "`entry [yaw] [pitch] [fov]` or `at x y z yaw pitch [fov]`",
                ));
            }
        };
        if !words[0]
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(fail("names use letters, digits, '-' and '_'"));
        }
        if stops.iter().any(|stop: &Stop| stop.name == words[0]) {
            return Err(fail("the name is used twice"));
        }
        stops.push(Stop {
            name: words[0].to_string(),
            game: words[1].to_ascii_lowercase(),
            module: words[2].to_ascii_lowercase(),
            place,
        });
    }
    Ok(stops)
}

/// The camera command for a stop; `entry` needs the module's entry point.
fn camera(discovery: &Discovery, stop: &Stop) -> Result<String, String> {
    let [x, y, z, yaw, pitch, fov] = match stop.place {
        Place::At(pose) => pose,
        Place::Entry(offset, pitch, fov) => {
            let game = Game::from_key(&stop.game).ok_or("use k1 or tsl")?;
            let install = discovery
                .get(game)
                .ok_or(format!("{}: not found", game.key()))?;
            let index = DirIndex::build(&install.data_root).map_err(|error| error.to_string())?;
            let resolver = Resolver::open(game, &index, Some(&stop.module), TextureQuality::High)
                .map_err(|error| error.to_string())?;
            let module = ModuleIr::build(&stop.module, &ResolverSource(&resolver))
                .map_err(|refusal| format!("{refusal:?}"))?;
            entry_pose(&module, offset, pitch, fov)
        }
    };
    Ok(format!("camera {x} {y} {z} {yaw} {pitch} {fov}"))
}

/// Standing at the module's entry at eye height, facing the entry direction
/// turned by `offset` degrees: x, y, z, yaw, pitch, fov.
fn entry_pose(module: &ModuleIr, offset: f32, pitch: f32, fov: f32) -> [f32; 6] {
    let [px, py, pz] = module.entry_position;
    let [dx, dy] = module.entry_direction;
    let yaw = dy.atan2(dx).to_degrees() + offset;
    [px, py, pz + EYE_HEIGHT, yaw, pitch, fov]
}

/// The camera command of a tour's plain `entry` stop.
pub fn entry_camera(module: &ModuleIr) -> String {
    let [x, y, z, yaw, pitch, fov] = entry_pose(module, 0.0, DEFAULT_PITCH, DEFAULT_FOV);
    format!("camera {x} {y} {z} {yaw} {pitch} {fov}")
}

/// The run folder a child `kjk` run made, from the line it logs.
pub fn run_folder(log: &str) -> Option<PathBuf> {
    log.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        let at = lower.find("run folder: ")?;
        Some(PathBuf::from(line[at + "run folder: ".len()..].trim()))
    })
}

fn stamp() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    format!("{seconds}")
}

/// Every stop's shot (or why it has none), keyed by stop name.
type Shots = BTreeMap<String, Result<PathBuf, String>>;

/// One child run per module, in file order.
fn render(discovery: &Discovery, stops: &[Stop], size: (u32, u32)) -> Shots {
    let mut shots = Shots::new();
    let mut modules: Vec<(String, String)> = Vec::new();
    for stop in stops {
        let key = (stop.game.clone(), stop.module.clone());
        if !modules.contains(&key) {
            modules.push(key);
        }
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            for stop in stops {
                shots.insert(stop.name.clone(), Err(error.to_string()));
            }
            return shots;
        }
    };
    for (game, module) in modules {
        let here: Vec<&Stop> = stops
            .iter()
            .filter(|stop| stop.game == game && stop.module == module)
            .collect();
        // Animations held at 0 s so every run gives the same shot.
        let mut script = vec!["anim 0".to_string()];
        for stop in &here {
            match camera(discovery, stop) {
                Ok(command) => {
                    script.push(command);
                    script.push(format!("screenshot {}", stop.name));
                }
                Err(error) => {
                    shots.insert(stop.name.clone(), Err(error));
                }
            }
        }
        if script.len() == 1 {
            continue;
        }
        script.push("quit".into());
        outln!("tour: {game} {module}: {} stops", here.len());
        let output = Command::new(&exe)
            .args(["module", &game, &module, "--headless", "--size"])
            .arg(format!("{}x{}", size.0, size.1))
            .arg("--cmds")
            .arg(script.join("; "))
            .output();
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                for stop in &here {
                    shots.insert(stop.name.clone(), Err(error.to_string()));
                }
                continue;
            }
        };
        let stderr = String::from_utf8_lossy(&output.stderr);
        let folder = run_folder(&stderr);
        for stop in &here {
            if shots.contains_key(&stop.name) {
                continue;
            }
            let shot = folder
                .as_ref()
                .map(|folder| folder.join(format!("{}.png", stop.name)))
                .filter(|path| path.is_file())
                .ok_or_else(|| {
                    let tail: Vec<&str> = stderr.lines().rev().take(3).collect();
                    format!(
                        "no shot (exit {:?}): {}",
                        output.status.code(),
                        tail.join(" | ")
                    )
                });
            shots.insert(stop.name.clone(), shot);
        }
    }
    shots
}

pub fn load(path: &Path) -> Option<RgbImage> {
    Some(image::open(path).ok()?.to_rgb8())
}

/// Shots in a grid, `columns` wide, each scaled to `cell`; a shot that
/// does not load is a dark red cell.
pub fn sheet(shots: &[PathBuf], cell: (u32, u32), columns: u32) -> RgbImage {
    let rows = (shots.len() as u32).div_ceil(columns);
    let mut sheet = RgbImage::from_pixel(cell.0 * columns, cell.1 * rows, Rgb([20, 20, 24]));
    for (index, shot) in shots.iter().enumerate() {
        let (x, y) = (
            index as u32 % columns * cell.0,
            index as u32 / columns * cell.1,
        );
        let tile = load(shot)
            .map(|shot| {
                image::imageops::resize(&shot, cell.0 - 4, cell.1 - 4, image::imageops::Triangle)
            })
            .unwrap_or_else(|| RgbImage::from_pixel(cell.0 - 4, cell.1 - 4, Rgb([90, 10, 10])));
        image::imageops::replace(&mut sheet, &tile, i64::from(x + 2), i64::from(y + 2));
    }
    sheet
}

/// Changed pixels and the largest channel difference, when both exist and
/// are the same size.
fn compare(a: &Path, b: &Path) -> Option<(usize, u8)> {
    let (a, b) = (load(a)?, load(b)?);
    if a.dimensions() != b.dimensions() {
        return None;
    }
    let mut changed = 0;
    let mut most = 0;
    for (p, q) in a.pixels().zip(b.pixels()) {
        let diff = (0..3).map(|c| p[c].abs_diff(q[c])).max().unwrap_or(0);
        if diff > 0 {
            changed += 1;
            most = most.max(diff);
        }
    }
    Some((changed, most))
}

pub fn run(discovery: &Discovery, file: &Path, size: (u32, u32), only: Option<&str>) -> ExitCode {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("{}: {error}", file.display());
            return ExitCode::FAILURE;
        }
    };
    let mut stops = match parse(&text) {
        Ok(stops) => stops,
        Err(error) => {
            eprintln!("{}: {error}", file.display());
            return ExitCode::from(2);
        }
    };
    if let Some(only) = only {
        stops.retain(|stop| stop.name == only || stop.module == only);
    }
    let tour = file
        .file_stem()
        .map_or("tour".into(), |stem| stem.to_string_lossy().into_owned());
    let base = crate::runtime::artifacts().join("tours").join(&tour);
    let previous = std::fs::read_dir(&base).ok().and_then(|entries| {
        entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .max()
    });
    let folder = base.join(stamp());
    if let Err(error) = std::fs::create_dir_all(&folder) {
        eprintln!("{}: {error}", folder.display());
        return ExitCode::FAILURE;
    }
    let shots = render(discovery, &stops, size);
    let mut report = Vec::new();
    let mut failed = 0;
    for stop in &stops {
        let line = match &shots[&stop.name] {
            Ok(shot) => {
                let target = folder.join(format!("{}.png", stop.name));
                if let Err(error) = std::fs::copy(shot, &target) {
                    failed += 1;
                    format!("{}: copy failed: {error}", stop.name)
                } else {
                    let before = previous
                        .as_ref()
                        .map(|previous| previous.join(format!("{}.png", stop.name)));
                    match before
                        .as_deref()
                        .and_then(|before| compare(before, &target))
                    {
                        Some((0, _)) => format!("{}: unchanged", stop.name),
                        Some((changed, most)) => {
                            format!("{}: {changed} pixels changed, up to {most}", stop.name)
                        }
                        None => format!("{}: new", stop.name),
                    }
                }
            }
            Err(error) => {
                failed += 1;
                format!("{}: FAILED {error}", stop.name)
            }
        };
        outln!("  {line}");
        report.push(line);
    }
    let columns = (stops.len() as f32).sqrt().ceil().max(1.0) as u32;
    let cell = (320, 320 * size.1 / size.0.max(1));
    let shots: Vec<PathBuf> = stops
        .iter()
        .map(|stop| folder.join(format!("{}.png", stop.name)))
        .collect();
    if let Err(error) = sheet(&shots, cell, columns).save(folder.join("sheet.png")) {
        eprintln!("sheet: {error}");
    }
    let legend: Vec<String> = stops
        .iter()
        .enumerate()
        .map(|(index, stop)| {
            format!(
                "row {} column {}: {} ({} {})",
                index as u32 / columns + 1,
                index as u32 % columns + 1,
                stop.name,
                stop.game,
                stop.module
            )
        })
        .collect();
    let compared = previous.map_or("nothing (first run)".into(), |p| p.display().to_string());
    let text = format!(
        "tour {tour}: {} stops, {failed} failed; compared with {compared}\n\n{}\n\nsheet.png:\n{}\n",
        stops.len(),
        report.join("\n"),
        legend.join("\n")
    );
    if let Err(error) = std::fs::write(folder.join("report.txt"), text) {
        eprintln!("report: {error}");
    }
    outln!(
        "tour {tour}: {} stops, {failed} failed, in {}",
        stops.len(),
        folder.display()
    );
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
