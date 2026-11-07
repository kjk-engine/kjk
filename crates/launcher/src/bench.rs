//! `kjk bench`: the frame rate of fixed scenes, measured the same way each
//! time. Each scene is a `kjk play` (or `kjk --load`) in a window of a fixed
//! size with v-sync off, its settings from a preset, standing still where
//! it arrives (or a fixed script); after a warm-up its dumps every few
//! seconds give the frames' times (`console`'s vitals), and the mean frame
//! time comes from the frames counted between the first and last dump, so
//! an older `kjk` (`--exe`, a release's) is measured alike. The run folders
//! are deleted once read; each scene's last frame is kept as a picture
//! beside the results, to see that what is drawn did not change.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::scenes::Report;

/// A scene: its name, how the game is started, and the commands run once
/// the world is up, before the warm-up.
pub struct Scene {
    pub name: &'static str,
    pub why: &'static str,
    pub args: &'static [&'static str],
    pub setup: &'static str,
}

/// The scenes, from the testers' reports and the systems each stresses
/// (the saves are the chain's, copied to a folder of the bench's own).
pub const SCENES: &[Scene] = &[
    Scene {
        name: "peragus_hangar",
        why: "TSL 106per: shadows, see-through windows",
        args: &["--load", "tsl-13"],
        setup: "",
    },
    Scene {
        name: "dune_sea",
        why: "K1 tat_m18aa: open terrain, far draw, placeables",
        args: &["play", "--stage", "tat_m18aa"],
        setup: "",
    },
    Scene {
        name: "star_map_cave",
        why: "K1 tat_m18ac: the krayt dragon's cave, by the Star Map",
        args: &["play", "--stage", "tat_m18ac", "--arrive", "tat_ambush_spawns"],
        setup: "",
    },
    Scene {
        name: "undercity",
        why: "K1 tar_m04aa: many NPCs",
        args: &["play", "--stage", "tar_m04aa"],
        setup: "",
    },
    Scene {
        name: "cantina",
        why: "K1 tar_m03ae: crowd and lights",
        args: &["play", "--stage", "tar_m03ae"],
        setup: "",
    },
    Scene {
        name: "endar_spire",
        why: "K1 end_m01aa: small interiors",
        args: &["play", "--stage", "end_m01aa"],
        setup: "",
    },
    Scene {
        name: "harbinger",
        why: "TSL 151har: corridors, wall lights",
        args: &["--load", "tsl-16"],
        setup: "",
    },
    Scene {
        name: "arena_fight",
        why: "arena@end_m01ab, 8 foes: combat lights, blades, particles",
        args: &["play", "--stage", "arena@end_m01ab"],
        setup: "arena god; arena spawn g_darkjedi01; arena spawn g_darkjedi03; \
                arena spawn g_sithtroop01; arena spawn g_sithtroop05; arena spawn g_darkjedi01; \
                arena spawn g_sithtroop01; arena spawn g_mandalor01; arena spawn g_sithtroop05; \
                wait 3s",
    },
    Scene {
        name: "dantooine",
        why: "K1 danm14aa: grass, open sky",
        args: &["play", "--stage", "danm14aa"],
        setup: "",
    },
    // Not run unless named (`--scenes`): areas a profile-guided build was
    // not trained on, to check its gain holds beyond the scenes above.
    Scene {
        name: "telos",
        why: "TSL 201tel, held out: Citadel Station",
        args: &["--load", "tsl-15"],
        setup: "",
    },
    Scene {
        name: "kashyyyk",
        why: "K1 kas_m22aa, held out: the Great Walkway",
        args: &["play", "--stage", "kas_m22aa"],
        setup: "",
    },
    Scene {
        name: "upper_city",
        why: "K1 tar_m02af: the first open hub",
        args: &["play", "--stage", "tar_m02af"],
        setup: "",
    },
];

/// Scenes run only when named.
const HELD_OUT: [&str; 2] = ["telos", "kashyyyk"];

/// The settings a preset puts over the defaults (`settings.rs` keys).
fn preset(name: &str) -> Option<Value> {
    let common = json!({
        "display.mode": "window",
        "display.size": "1920x1080",
        "display.vsync": "false",
        "game.autosave": "false",
        "feedback.tutorials": "false",
    });
    let extra = match name {
        "default" => json!({}),
        "low" => json!({
            "display.msaa": "1",
            "display.shadows": "false",
            "display.soft_shadows": "false",
        }),
        "high" => json!({
            "display.msaa": "4",
            "display.shadows": "true",
            "display.soft_shadows": "true",
        }),
        _ => return None,
    };
    let mut all = common.as_object().cloned().unwrap_or_else(serde_json::Map::new);
    all.extend(extra.as_object().cloned().unwrap_or_else(serde_json::Map::new));
    Some(Value::Object(all))
}

/// What one scene measured.
struct Measured {
    frames: u64,
    seconds: f64,
    windows: Vec<Value>,
}

impl Measured {
    fn mean_ms(&self) -> f64 {
        self.seconds * 1000.0 / self.frames.max(1) as f64
    }

    /// The windows' `key` (`frame_ms`, `main_ms`, …) `stat`, averaged.
    fn average(&self, key: &str, stat: &str) -> Option<f64> {
        let values: Vec<f64> = self
            .windows
            .iter()
            .filter_map(|window| window[key][stat].as_f64())
            .collect();
        (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
    }

    fn worst(&self, key: &str, stat: &str) -> Option<f64> {
        self.windows
            .iter()
            .filter_map(|window| window[key][stat].as_f64())
            .reduce(f64::max)
    }
}

pub struct BenchOptions {
    pub exe: Option<PathBuf>,
    pub preset: String,
    pub scenes: Vec<String>,
    pub warmup: u32,
    pub seconds: u32,
    pub window: u32,
    pub label: String,
    pub out: Option<PathBuf>,
    pub trace: bool,
}

fn script(scene: &Scene, options: &BenchOptions) -> String {
    let mut commands = vec!["wait world".to_string()];
    if !scene.setup.is_empty() {
        commands.push(scene.setup.to_string());
    }
    commands.push(format!("wait {}s", options.warmup));
    let windows = (options.seconds / options.window.max(1)).max(1);
    commands.push("dump bench-0".into());
    for window in 1..=windows {
        commands.push(format!("wait {}s", options.window.max(1)));
        commands.push(format!("dump bench-{window}"));
    }
    commands.push("screenshot bench".into());
    commands.push("quit".into());
    commands.join("; ")
}

/// The saves the scenes load, copied from the player's saves folder to
/// the bench's own (`--load` never touches the player's).
fn saves_folder(root: &Path) -> Result<PathBuf, String> {
    let theirs = saves::folder(|key| std::env::var(key).ok()).ok_or("no saves folder")?;
    let ours = root.join("saves");
    for scene in SCENES {
        if let ["--load", id] = scene.args {
            let from = theirs.join(id);
            let to = ours.join(id);
            if to.is_dir() || !from.is_dir() {
                continue;
            }
            std::fs::create_dir_all(&to).map_err(|error| error.to_string())?;
            for entry in std::fs::read_dir(&from).map_err(|error| error.to_string())? {
                let entry = entry.map_err(|error| error.to_string())?;
                if entry.path().is_file() {
                    std::fs::copy(entry.path(), to.join(entry.file_name()))
                        .map_err(|error| error.to_string())?;
                }
            }
        }
    }
    Ok(ours)
}

fn run_scene(
    scene: &Scene,
    options: &BenchOptions,
    exe: &Path,
    settings: &Path,
    saves: &Path,
    pictures: &Path,
) -> Result<Measured, String> {
    let commands = script(scene, options);
    let mut command = Command::new(exe);
    command
        .args(scene.args)
        .args(["--cmds", &commands])
        .env("KJK_SETTINGS", settings)
        .env("KJK_SAVES", saves)
        .env("KJK_MUTE", "1")
        .env("KJK_MOVIES", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if options.trace {
        command.env("KJK_PERF", "1");
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let limit = Duration::from_secs(u64::from(options.warmup + options.seconds) + 240);
    let started = Instant::now();
    // Both pipes read on threads, so a full one never stalls the game.
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let mut stderr = child.stderr.take().ok_or("no stderr")?;
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut text);
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::Read::read_to_string(&mut stderr, &mut text);
        text
    });
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break Some(status);
        }
        if started.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let text = out.join().unwrap_or_else(|_| String::new()) + &err.join().unwrap_or_else(|_| String::new());
    let folder = text
        .lines()
        .find_map(|line| line.strip_prefix("run folder: "))
        .map(|folder| PathBuf::from(folder.trim()))
        .ok_or_else(|| format!("named no run folder (exit {status:?})"))?;
    let read = |name: &str| -> Option<Value> {
        let bytes = std::fs::read(folder.join(format!("{name}.json"))).ok()?;
        serde_json::from_slice(&bytes).ok()
    };
    let mut dumps = Vec::new();
    for index in 0.. {
        match read(&format!("bench-{index}")) {
            Some(dump) => dumps.push(dump),
            None => break,
        }
    }
    let picture = folder.join("bench.png");
    if picture.is_file() {
        let _ = std::fs::copy(
            &picture,
            pictures.join(format!("{}-{}.png", scene.name, options.preset)),
        );
    }
    if options.trace {
        let trace = folder.join("trace.json");
        if trace.is_file() {
            let kept = pictures.join(format!("{}-{}.trace.json", scene.name, options.preset));
            let _ = std::fs::rename(&trace, &kept);
        }
    }
    let log = std::fs::read_to_string(folder.join("kjk.log")).unwrap_or_else(|_| String::new());
    let _ = std::fs::remove_dir_all(&folder);
    if dumps.len() < 2 {
        let tail: Vec<&str> = log.lines().rev().take(8).collect();
        return Err(format!(
            "{} dumps (exit {status:?}); log ends: {}",
            dumps.len(),
            tail.into_iter().rev().collect::<Vec<_>>().join(" | ")
        ));
    }
    let (first, last) = (&dumps[0], &dumps[dumps.len() - 1]);
    let frames = last["frame"].as_u64().unwrap_or(0) - first["frame"].as_u64().unwrap_or(0);
    let ms = last["elapsed_ms"].as_u64().unwrap_or(0) - first["elapsed_ms"].as_u64().unwrap_or(0);
    let windows = dumps[1..]
        .iter()
        .map(|dump| dump["facts"]["vitals"].clone())
        .collect();
    Ok(Measured {
        frames,
        seconds: ms as f64 / 1000.0,
        windows,
    })
}

pub fn bench(options: BenchOptions) -> ExitCode {
    let mut report = Report::default();
    let Some(settings_values) = preset(&options.preset) else {
        report.check(false, format!("{}: a preset (default, low, high)", options.preset));
        return report.finish(json!({}));
    };
    let exe = match &options.exe {
        Some(exe) => exe.clone(),
        None => match std::env::current_exe() {
            Ok(exe) => exe,
            Err(error) => {
                report.check(false, error.to_string());
                return report.finish(json!({}));
            }
        },
    };
    let root = PathBuf::from("kjk-artifacts").join("bench");
    let out_folder = root.join(&options.label);
    let prepared = std::fs::create_dir_all(&out_folder)
        .map_err(|error| error.to_string())
        .and_then(|()| saves_folder(&root));
    let saves = match prepared {
        Ok(saves) => saves,
        Err(error) => {
            report.check(false, error);
            return report.finish(json!({}));
        }
    };
    let settings = std::env::temp_dir().join(format!("kjk-bench-{}.json", std::process::id()));
    if let Err(error) = std::fs::write(&settings, settings_values.to_string()) {
        report.check(false, format!("{}: {error}", settings.display()));
        return report.finish(json!({}));
    }
    let chosen: Vec<&Scene> = SCENES
        .iter()
        .filter(|scene| match options.scenes.is_empty() {
            true => !HELD_OUT.contains(&scene.name),
            false => options.scenes.iter().any(|name| name == scene.name),
        })
        .collect();
    let out = options
        .out
        .clone()
        .unwrap_or_else(|| out_folder.join("results.jsonl"));
    outln!(
        "{} on {} ({} preset), {} s after {} s warm-up:",
        options.label,
        exe.display(),
        options.preset,
        options.seconds,
        options.warmup
    );
    outln!(
        "  {:<16} {:>7} {:>8} {:>7} {:>7} {:>7} {:>8} {:>8} {:>7} {:>6} {:>9}",
        "scene",
        "fps",
        "mean ms",
        "p50",
        "p95",
        "p99",
        "main ms",
        "rend ms",
        "graph",
        "views",
        "draws o/t"
    );
    let mut rows = Vec::new();
    for scene in chosen {
        let measured = run_scene(
            scene,
            &options,
            &exe,
            &settings,
            &saves,
            &out_folder,
        );
        let row = match measured {
            Ok(measured) => {
                let mean = measured.mean_ms();
                let fmt = |value: Option<f64>| value.map_or("-".to_string(), |v| format!("{v:.2}"));
                outln!(
                    "  {:<16} {:>7.1} {:>8.2} {:>7} {:>7} {:>7} {:>8} {:>8} {:>7} {:>6} {:>9}",
                    scene.name,
                    1000.0 / mean,
                    mean,
                    fmt(measured.average("frame_ms", "p50")),
                    fmt(measured.average("frame_ms", "p95")),
                    fmt(measured.worst("frame_ms", "p99")),
                    fmt(measured.average("main_ms", "mean")),
                    fmt(measured.average("render_ms", "mean")),
                    fmt(measured.average("graph_ms", "mean")),
                    fmt(measured.average("views", "mean")),
                    format!(
                        "{:.0}/{:.0}",
                        measured.average("opaque_draws", "mean").unwrap_or(0.0),
                        measured.average("transparent_draws", "mean").unwrap_or(0.0)
                    ),
                );
                json!({
                    "label": options.label,
                    "preset": options.preset,
                    "scene": scene.name,
                    "why": scene.why,
                    "frames": measured.frames,
                    "seconds": measured.seconds,
                    "fps": 1000.0 / mean,
                    "mean_ms": mean,
                    "p50_ms": measured.average("frame_ms", "p50"),
                    "p95_ms": measured.average("frame_ms", "p95"),
                    "p99_ms": measured.worst("frame_ms", "p99"),
                    "max_ms": measured.worst("frame_ms", "max"),
                    "main_ms": measured.average("main_ms", "mean"),
                    "main_p95_ms": measured.average("main_ms", "p95"),
                    "render_ms": measured.average("render_ms", "mean"),
                    "render_p95_ms": measured.average("render_ms", "p95"),
                    "graph_ms": measured.average("graph_ms", "mean"),
                    "views": measured.average("views", "mean"),
                    "opaque_draws": measured.average("opaque_draws", "mean"),
                    "transparent_draws": measured.average("transparent_draws", "mean"),
                    "cameras": measured.windows.last().map(|window| window["cameras"].clone()),
                })
            }
            Err(error) => {
                outln!("  {:<16} failed: {error}", scene.name);
                report.check(false, format!("{}: {error}", scene.name));
                json!({ "label": options.label, "preset": options.preset, "scene": scene.name, "error": error })
            }
        };
        let line = row.to_string() + "\n";
        let appended = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&out)
            .and_then(|mut file| std::io::Write::write_all(&mut file, line.as_bytes()));
        if let Err(error) = appended {
            outln!("  {}: {error}", out.display());
        }
        rows.push(row);
    }
    let _ = std::fs::remove_file(&settings);
    report.finish(json!({ "label": options.label, "preset": options.preset, "rows": rows }))
}
