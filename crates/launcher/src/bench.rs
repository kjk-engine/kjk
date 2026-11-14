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
///
/// An arena's foes are called in after its arrival cutscene ends, which
/// takes about 20 seconds from the world's arrival.
pub const SCENES: &[Scene] = &[
    Scene {
        name: "main_menu",
        why: "K1's main menu: Malak in smoke and mist, drawn into a view of its own",
        args: &[],
        setup: "",
    },
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
        setup: "wait 20s; arena god; arena spawn g_darkjedi01; arena spawn g_darkjedi03; \
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
    // The lighting plan's scenes (docs/LIGHTING-PLAN.md) also run only
    // when named. The plan measures them together with `peragus_hangar`.
    Scene {
        name: "ebon_hawk",
        why: "TSL 003ebo, lighting: lamps and characters, no fog",
        args: &["--load", "tsl-14"],
        setup: "",
    },
    Scene {
        name: "dark_fight",
        why: "arena@lev_m40ac, 8 foes, lighting: fight lights in a dark hall",
        args: &["play", "--stage", "arena@lev_m40ac"],
        setup: "wait 20s; arena god; arena spawn g_darkjedi01; arena spawn g_darkjedi03; \
                arena spawn g_sithtroop01; arena spawn g_sithtroop05; arena spawn g_darkjedi01; \
                arena spawn g_sithtroop01; arena spawn g_mandalor01; arena spawn g_sithtroop05; \
                wait 3s",
    },
];

/// Scenes run only when named.
const HELD_OUT: [&str; 4] = ["telos", "kashyyyk", "ebon_hawk", "dark_fight"];

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
            "display.shadows": "off",
        }),
        "high" => json!({
            "display.msaa": "4",
            "display.shadows": "high",
        }),
        // The shadow settings alone, over MSAA 4. The lighting plan measures
        // each of its steps against these three.
        "shadows-off" => json!({
            "display.msaa": "4",
            "display.shadows": "off",
        }),
        "shadows-hard" => json!({
            "display.msaa": "4",
            "display.shadows": "low",
        }),
        "shadows-soft" => json!({
            "display.msaa": "4",
            "display.shadows": "high",
        }),
        // Shadows at Ultra, and at High for comparison, both over TAA, as
        // Ultra needs TAA or DLSS. On a card without ray queries Ultra
        // draws as High does.
        "shadows-ultra" => json!({
            "display.msaa": "taa",
            "display.shadows": "ultra",
        }),
        "shadows-high-taa" => json!({
            "display.msaa": "taa",
            "display.shadows": "high",
        }),
        // The Lighting setting alone, over MSAA 4 and Shadows at High. The
        // two pictures are compared with `kjk compare-pictures`.
        "classic" => json!({
            "display.msaa": "4",
            "display.shadows": "high",
            "display.lighting": "classic",
        }),
        "enhanced" => json!({
            "display.msaa": "4",
            "display.shadows": "high",
            "display.lighting": "enhanced",
        }),
        // The Effects Quality setting alone, over MSAA 4 and Shadows at
        // High. The two pictures are compared with `kjk compare-pictures`.
        "effects-high" => json!({
            "display.msaa": "4",
            "display.shadows": "high",
            "display.effects": "high",
        }),
        "effects-low" => json!({
            "display.msaa": "4",
            "display.shadows": "high",
            "display.effects": "low",
        }),
        // The anti-aliasing setting alone: TAA at the window's size, kjk's
        // upscaler at its four sizes, and DLSS at a half of the side where
        // the build and the card have it. The lighting plan's step 6a
        // compares the opaque pass's time at each size.
        "taa" => json!({ "display.msaa": "taa" }),
        "taa-quality" => json!({ "display.msaa": "taa_quality" }),
        "taa-balanced" => json!({ "display.msaa": "taa_balanced" }),
        "taa-performance" => json!({ "display.msaa": "taa_performance" }),
        "taa-ultra-performance" => json!({ "display.msaa": "taa_ultra_performance" }),
        "dlss-performance" => json!({ "display.msaa": "dlss_performance" }),
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

    /// Each GPU pass's mean per frame (`vitals.gpu`, `--gpu`), averaged
    /// over the windows. `group` is `ms` or `fragments`.
    fn gpu(&self, group: &str) -> serde_json::Map<String, Value> {
        let mut passes: Vec<String> = self
            .windows
            .iter()
            .filter_map(|window| window["gpu"][group].as_object())
            .flat_map(|passes| passes.keys().cloned())
            .collect();
        passes.sort();
        passes.dedup();
        passes
            .into_iter()
            .filter_map(|pass| {
                let values: Vec<f64> = self
                    .windows
                    .iter()
                    .filter_map(|window| window["gpu"][group][&pass]["mean"].as_f64())
                    .collect();
                (!values.is_empty())
                    .then(|| (pass, json!(values.iter().sum::<f64>() / values.len() as f64)))
            })
            .collect()
    }

    /// The GPU ms of every top-level pass timed, added up per frame.
    fn gpu_spans(&self) -> Option<f64> {
        let values: Vec<f64> = self
            .windows
            .iter()
            .filter_map(|window| window["gpu"]["spans_ms"]["mean"].as_f64())
            .collect();
        (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
    }

    /// The shadow tiles' facts (`game::shadow_tiles`), each number
    /// averaged over the windows. `cpu_us` holds each step's CPU time.
    fn tiles(&self) -> Option<Value> {
        let averaged = |path: &[&str]| -> Option<f64> {
            let values: Vec<f64> = self
                .windows
                .iter()
                .filter_map(|window| {
                    let mut value = &window["shadow_tiles"];
                    for key in path {
                        value = &value[*key];
                    }
                    value.as_f64()
                })
                .collect();
            (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
        };
        averaged(&["draws"])?;
        let mut cpu = serde_json::Map::new();
        for step in ["gather", "extract", "prepare", "draw"] {
            cpu.insert(step.into(), json!(averaged(&["cpu_us", step])));
        }
        Some(json!({
            "tiles": averaged(&["tiles"]),
            "draws": averaged(&["draws"]),
            "calls": averaged(&["calls"]),
            "kept_redraws": averaged(&["kept_redraws"]),
            "cpu_us": cpu,
        }))
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
    pub gpu: bool,
    /// `WIDTHxHEIGHT` in place of the presets' size.
    pub size: Option<String>,
    pub headless: bool,
}

fn script(scene: &Scene, options: &BenchOptions) -> String {
    // A scene with no arguments is the main menu, which has no world to
    // wait for: its warm-up covers its view's loading.
    let mut commands: Vec<String> = (!scene.args.is_empty())
        .then(|| "wait world".to_string())
        .into_iter()
        .collect();
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
        .args(options.headless.then_some("--headless"))
        // A headless run's frame takes its size from `--size`, a window's
        // from the settings.
        .args(
            options
                .size
                .as_deref()
                .filter(|_| options.headless)
                .map(|size| ["--size", size])
                .into_iter()
                .flatten(),
        )
        .args(["--cmds", &commands])
        .env("KJK_SETTINGS", settings)
        .env("KJK_SAVES", saves)
        .env("KJK_MUTE", "1")
        .env("KJK_MOVIES", "0")
        .env("KJK_UNCAPPED", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if options.trace {
        command.env("KJK_PERF", "1");
    }
    if options.gpu {
        command.env("KJK_GPU_TIMES", "1");
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
    // Each window's vitals, with the shadow tiles' counts and CPU times
    // (`game::shadow_tiles`) when it drew any.
    let windows = dumps[1..]
        .iter()
        .map(|dump| {
            let mut window = dump["facts"]["vitals"].clone();
            if let Some(tiles) = dump["facts"].get("shadow_tiles") {
                window["shadow_tiles"] = tiles.clone();
            }
            for key in ["view_particles", "level_particles"] {
                if let Some(particles) = dump["facts"].get(key) {
                    window[key] = particles.clone();
                }
            }
            window
        })
        .collect();
    Ok(Measured {
        frames,
        seconds: ms as f64 / 1000.0,
        windows,
    })
}

pub fn bench(options: BenchOptions) -> ExitCode {
    let mut report = Report::default();
    let Some(mut settings_values) = preset(&options.preset) else {
        report.check(
            false,
            format!(
                "{}: a preset (default, low, high, shadows-off, shadows-hard, shadows-soft, shadows-ultra, shadows-high-taa, classic, enhanced, effects-high, effects-low, taa, taa-quality, taa-balanced, taa-performance, taa-ultra-performance, dlss-performance)",
                options.preset
            ),
        );
        return report.finish(json!({}));
    };
    if let Some(size) = &options.size {
        settings_values["display.size"] = json!(size);
    }
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
                let gpu_ms = measured.gpu("ms");
                if options.gpu {
                    let ms = |pass: &str| {
                        gpu_ms
                            .get(pass)
                            .and_then(Value::as_f64)
                            .map_or("-".to_string(), |v| format!("{v:.2}"))
                    };
                    outln!(
                        "  {:<16} gpu ms: shadows {}, opaque {}, transparent {}, roll-off {}, all spans {}",
                        "",
                        ms("shadows"),
                        ms("main_opaque_pass_3d"),
                        ms("main_transparent_pass_3d"),
                        ms("roll_off"),
                        measured
                            .gpu_spans()
                            .map_or("-".to_string(), |v| format!("{v:.2}")),
                    );
                    if gpu_ms.contains_key("taa_upscale") {
                        outln!(
                            "  {:<16} gpu ms: upscale {}, sharpen {}",
                            "",
                            ms("taa_upscale"),
                            ms("taa_upscale_sharpen"),
                        );
                    }
                }
                let tiles = measured.tiles();
                if let Some(tiles) = &tiles {
                    let us = |step: &str| {
                        tiles["cpu_us"][step]
                            .as_f64()
                            .map_or("-".to_string(), |v| format!("{v:.0}"))
                    };
                    let count = |key: &str| {
                        tiles[key]
                            .as_f64()
                            .map_or("-".to_string(), |v| format!("{v:.0}"))
                    };
                    outln!(
                        "  {:<16} tiles {}, draws {}, calls {}; cpu us: gather {}, extract {}, prepare {}, draw {}",
                        "",
                        count("tiles"),
                        count("draws"),
                        count("calls"),
                        us("gather"),
                        us("extract"),
                        us("prepare"),
                        us("draw"),
                    );
                }
                // The particles of the screens' 3D views (`gui_scene`'s
                // `view_particles`) and of the level (`effects_quality`'s
                // `level_particles`), in the last window.
                let last_window = |key: &str| {
                    measured
                        .windows
                        .last()
                        .and_then(|window| window[key].as_array().cloned())
                        .unwrap_or_else(Vec::new)
                };
                let (particles, level_particles) =
                    (last_window("view_particles"), last_window("level_particles"));
                for (name, views) in [("view", &particles), ("level", &level_particles)] {
                    for view in views {
                        outln!(
                            "  {:<16} {} particles: {} quads cover {} pixels of {} ({:.1} layers deep), {} samples a pixel; effects {}, {} pixels shaded",
                            "",
                            name,
                            view["quads"],
                            view["pixels"],
                            view["view_pixels"],
                            view["pixels"].as_f64().unwrap_or(0.0)
                                / view["view_pixels"].as_f64().unwrap_or(1.0).max(1.0),
                            view["samples"],
                            view["effects"].as_str().unwrap_or("high"),
                            view["shaded"]
                                .as_f64()
                                .unwrap_or(view["pixels"].as_f64().unwrap_or(0.0)),
                        );
                    }
                }
                json!({
                    "label": options.label,
                    "view_particles": particles,
                    "level_particles": level_particles,
                    "preset": options.preset,
                    "scene": scene.name,
                    "why": scene.why,
                    "size": options.size,
                    "headless": options.headless,
                    "shadow_tiles": tiles,
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
                    "gpu_spans_ms": measured.gpu_spans(),
                    "gpu_ms": gpu_ms,
                    "gpu_fragments": measured.gpu("fragments"),
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

/// A channel of a pixel differing by more than this many steps of 255
/// counts as changed. Two runs of one build differ by a step or two where
/// a blend rounds.
const CHANGED_STEPS: u8 = 2;

/// What `kjk compare-pictures` leaves out of its count.
pub struct CompareOptions {
    /// A second picture taken the same way as the first. Only the pixels
    /// that are exactly the same in both are compared. Those are the
    /// pixels where nothing moved between the two runs.
    pub still: Option<PathBuf>,
    /// Only the channels whose value in the first picture is at most this
    /// are compared.
    pub up_to: u8,
    /// Where the difference picture is written.
    pub out: Option<PathBuf>,
}

/// `kjk compare-pictures`: how far two pictures of one size differ. It
/// prints the largest and the mean difference of a channel, in steps of
/// 255, and the share of pixels that changed. The difference picture shows
/// each pixel's largest channel difference in grey, 16 times as bright, so
/// one step is visible. A pixel left out of the count is black in it.
pub fn compare_pictures(first: &Path, second: &Path, options: &CompareOptions) -> ExitCode {
    let mut report = Report::default();
    let open = |path: &Path| {
        image::open(path)
            .map(|picture| picture.to_rgb8())
            .map_err(|error| format!("{}: {error}", path.display()))
    };
    let still = options.still.as_deref().map(open).transpose();
    let (a, b, still) = match (open(first), open(second), still) {
        (Ok(a), Ok(b), Ok(still)) => (a, b, still),
        (Err(error), ..) | (_, Err(error), _) | (.., Err(error)) => {
            report.check(false, error);
            return report.finish(json!({}));
        }
    };
    let sizes_differ = a.dimensions() != b.dimensions()
        || still.as_ref().is_some_and(|still| still.dimensions() != a.dimensions());
    if sizes_differ {
        report.check(false, "the pictures differ in size".to_string());
        return report.finish(json!({}));
    }
    let (width, height) = a.dimensions();
    let mut shown = image::GrayImage::new(width, height);
    let mut largest = 0u8;
    let mut largest_at = (0, 0);
    let (mut sum, mut channels) = (0u64, 0u64);
    let (mut changed, mut compared) = (0u64, 0u64);
    for (x, y, pixel) in a.enumerate_pixels() {
        if still.as_ref().is_some_and(|still| still.get_pixel(x, y) != pixel) {
            continue;
        }
        compared += 1;
        let other = b.get_pixel(x, y);
        let mut worst = 0u8;
        for channel in 0..3 {
            if pixel[channel] > options.up_to {
                continue;
            }
            let difference = pixel[channel].abs_diff(other[channel]);
            sum += u64::from(difference);
            channels += 1;
            worst = worst.max(difference);
        }
        if worst > largest {
            largest = worst;
            largest_at = (x, y);
        }
        if worst > CHANGED_STEPS {
            changed += 1;
        }
        shown.put_pixel(x, y, image::Luma([worst.saturating_mul(16)]));
    }
    let pixels = u64::from(width) * u64::from(height);
    let mean = sum as f64 / channels.max(1) as f64;
    let share = changed as f64 / compared.max(1) as f64;
    outln!("{} against {}:", first.display(), second.display());
    if still.is_some() {
        outln!(
            "  pixels where nothing moved: {compared} of {pixels} ({:.1}%)",
            compared as f64 / pixels as f64 * 100.0
        );
    }
    if options.up_to < u8::MAX {
        outln!("  only channels at or below {} of 255 are compared", options.up_to);
    }
    outln!(
        "  largest difference: {largest} of 255, at pixel {}, {}",
        largest_at.0,
        largest_at.1
    );
    outln!("  mean difference: {mean:.3} of 255");
    outln!(
        "  pixels differing by more than {CHANGED_STEPS}: {changed} of {compared} ({:.3}%)",
        share * 100.0
    );
    if let Some(out) = &options.out {
        match shown.save(out) {
            Ok(()) => outln!("  difference picture: {}", out.display()),
            Err(error) => report.check(false, format!("{}: {error}", out.display())),
        }
    }
    report.finish(json!({
        "largest": largest,
        "largest_at": [largest_at.0, largest_at.1],
        "mean": mean,
        "changed": changed,
        "compared": compared,
        "pixels": pixels,
    }))
}
