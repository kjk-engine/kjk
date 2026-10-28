//! `kjk sweep`: the whole-game check. Every module of K1 and TSL (or of one
//! game): whether its module builds, its scripts stepped headless (gaps and
//! faults, `sweep_verbs`), a frame rendered from its entry (a child `kjk
//! module` run, as `kjk tour` does, with the scene's facts dumped), and its
//! containment audit (`collision_audit::survey`). One run folder holds a
//! gallery page ranked worst first, the bug lists of each area
//! (`bugs.md`), a contact sheet per game and `facts.json`: I34's
//! per-module dashboard, started early.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use asset_odyssey::module::ModuleIr;
use serde_json::{Value, json};
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

use crate::collision_audit::{Survey, survey};
use crate::modules::ResolverSource;
use crate::sweep_verbs::{Swept, coverage, rank, rank_gaps, sweep_module};

/// A render that takes longer than this is killed and counted as failed.
const RENDER_TIMEOUT: Duration = Duration::from_secs(180);
/// The audit map's longer side, pixels.
const MAP_SIZE: u32 = 720;
/// A frame with more of its pixels this dark is reported near-black.
const BLACK_FRACTION: f32 = 0.95;
/// How many of the slowest audits are listed.
const SLOWEST: usize = 15;
/// How `Row::problems` scores, for the page and the bug lists.
const SCORING: &str = "a module that does not build 1000, whose scripts do not install 500, a failed render or audit 300 or 200, script faults 100 and up, a near-black frame 80, escapes on foot 50 and up, by jump 20 and up, missing assets, then one per script gap and per script named but missing";
/// What the bug lists' missing textures are.
const TEXTURES_NOTE: &str = "Textures a module's models name that no layer of the install holds (or that do not decode), and what is drawn where each is named (`viewer::stand_in`).";

pub struct Options {
    pub games: Vec<Game>,
    pub seconds: u64,
    pub only: Option<String>,
    pub size: (u32, u32),
    pub jobs: usize,
    pub scripts: bool,
    pub render: bool,
    pub audit: bool,
}

/// A frame rendered from the module's entry: the scene's facts as the
/// child dumped them, and how much of the frame is black.
struct Rendered {
    scene: Value,
    black: f32,
}

impl Rendered {
    fn list(&self, key: &str) -> Vec<String> {
        self.scene[key]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_else(Vec::new)
    }

    /// What the scene drew where it names a missing texture.
    fn stand_in(&self, texture: &str) -> String {
        self.scene["missing_texture_stand_ins"][texture]
            .as_str()
            .unwrap_or("not reported")
            .to_string()
    }
}

/// What the sweep found in one module.
struct Row {
    game: Game,
    module: String,
    built: Result<(), String>,
    scripts: Option<Result<Swept, String>>,
    render: Option<Result<Rendered, String>>,
    audit: Option<Result<Survey, String>>,
    /// Building, rendering and auditing it.
    seconds: f64,
}

/// The areas a problem belongs to, for the gallery's filters.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Area {
    Refusal,
    Render,
    Scripts,
    Containment,
    Assets,
}

impl Area {
    const ALL: [Area; 5] = [
        Area::Refusal,
        Area::Render,
        Area::Scripts,
        Area::Containment,
        Area::Assets,
    ];

    fn key(self) -> &'static str {
        match self {
            Area::Refusal => "refusal",
            Area::Render => "render",
            Area::Scripts => "scripts",
            Area::Containment => "containment",
            Area::Assets => "assets",
        }
    }
}

struct Problem {
    area: Area,
    score: usize,
    text: String,
}

impl Row {
    fn id(&self) -> String {
        format!("{}-{}", self.game.key(), self.module)
    }

    /// Everything wrong with the module, worst first. A module that does
    /// not build or install outranks any that does; then failures to
    /// render or audit, script faults, ways out of bounds on foot or with
    /// a plain jump, a black frame, missing assets, and last the script
    /// gaps it reaches.
    fn problems(&self) -> Vec<Problem> {
        let mut found = Vec::new();
        let mut add = |area, score, text: String| found.push(Problem { area, score, text });
        if let Err(why) = &self.built {
            add(Area::Refusal, 1000, format!("module refused: {why}"));
        }
        match &self.scripts {
            Some(Err(why)) => add(Area::Refusal, 500, format!("scripts: {why}")),
            Some(Ok(swept)) => {
                if !swept.faults.is_empty() {
                    let distinct = distinct(&swept.faults);
                    add(
                        Area::Scripts,
                        100 + 10 * distinct.len().min(10),
                        format!(
                            "{} script faults: {}",
                            swept.faults.len(),
                            distinct.join("; ")
                        ),
                    );
                }
                if !swept.gaps.is_empty() {
                    add(
                        Area::Scripts,
                        swept.gaps.len().min(30),
                        format!("{} script gaps reached", swept.gaps.len()),
                    );
                }
                if !swept.missing.is_empty() {
                    let names: Vec<String> = swept.missing.keys().cloned().collect();
                    add(
                        Area::Scripts,
                        names.len().min(30),
                        format!(
                            "{} scripts named but missing (nothing runs): {}",
                            names.len(),
                            brief(&names, 6)
                        ),
                    );
                }
            }
            None => {}
        }
        match &self.render {
            Some(Err(why)) => add(Area::Render, 300, format!("render failed: {why}")),
            Some(Ok(rendered)) => {
                if rendered.black > BLACK_FRACTION {
                    add(
                        Area::Render,
                        80,
                        format!(
                            "near-black frame ({:.0}% of pixels)",
                            rendered.black * 100.0
                        ),
                    );
                }
                for (key, what, score) in [
                    ("missing_models", "models missing", 40),
                    ("creature_refusals", "creatures refused", 30),
                    ("missing_textures", "textures missing", 20),
                    ("missing_clips", "clips missing", 10),
                ] {
                    let names = distinct(&rendered.list(key));
                    if !names.is_empty() {
                        add(
                            Area::Assets,
                            score + names.len().min(20),
                            format!("{} {what}: {}", names.len(), brief(&names, 6)),
                        );
                    }
                }
                // Rooms only seen (no walkable face in the game's own
                // walkmesh) are not a fault; a walkmesh we cannot read is.
                let unread = rendered.list("unread_floors");
                if !unread.is_empty() {
                    add(
                        Area::Render,
                        5,
                        format!(
                            "{} room walkmeshes unread: {}",
                            unread.len(),
                            brief(&unread, 6)
                        ),
                    );
                }
            }
            None => {}
        }
        match &self.audit {
            Some(Err(why)) => add(Area::Refusal, 200, format!("audit failed: {why}")),
            Some(Ok(audit)) => {
                for (level, score, each) in [(0, 50, 5), (1, 20, 2)] {
                    let Some(reach) = audit.reaches.get(level) else {
                        continue;
                    };
                    if reach.escapes > 0 {
                        add(
                            Area::Containment,
                            score + each * reach.escapes.min(20),
                            format!("{} escapes by {}", reach.escapes, reach.name),
                        );
                    }
                }
                if audit.voids > 0 {
                    add(
                        Area::Containment,
                        10,
                        format!("{} leaks into the void", audit.voids),
                    );
                }
            }
            None => {}
        }
        found.sort_by_key(|p| std::cmp::Reverse(p.score));
        found
    }

    /// Its scripts' sweep, when they stepped.
    fn swept(&self) -> Option<&Swept> {
        self.scripts
            .as_ref()
            .and_then(|scripts| scripts.as_ref().ok())
    }

    /// The sum of its problems' scores: what ranks it.
    fn score(&self) -> usize {
        self.problems().iter().map(|p| p.score).sum()
    }

    /// Nothing wrong but the script gaps it reaches and the scripts it
    /// names but lacks.
    fn clean(&self) -> bool {
        self.problems()
            .iter()
            .all(|p| p.area == Area::Scripts && p.score < 100)
    }

    fn json(&self) -> Value {
        let problems = self.problems();
        json!({
            "game": self.game.key(),
            "module": self.module,
            "score": self.score(),
            "problems": problems.iter().map(|p| json!({
                "area": p.area.key(), "score": p.score, "text": p.text,
            })).collect::<Vec<_>>(),
            "seconds": self.seconds,
            "built": self.built.as_ref().err(),
            "scripts": self.scripts.as_ref().map(|scripts| match scripts {
                Ok(one) => json!({
                    "runs": one.runs,
                    "instructions": one.instructions,
                    "gaps": one.gaps,
                    "faults": one.faults,
                    "missing": one.missing,
                    "ms": one.ms,
                }),
                Err(why) => json!({ "error": why }),
            }),
            "render": self.render.as_ref().map(|render| match render {
                Ok(rendered) => json!({
                    "black": rendered.black,
                    "scene": rendered.scene,
                }),
                Err(why) => json!({ "error": why }),
            }),
            "audit": self.audit.as_ref().map(|audit| match audit {
                Ok(survey) => survey.json(),
                Err(why) => json!({ "error": why }),
            }),
        })
    }
}

/// The distinct entries of `items`, in first-seen order.
fn distinct(items: &[String]) -> Vec<String> {
    let mut seen = Vec::new();
    for item in items {
        if !seen.contains(item) {
            seen.push(item.clone());
        }
    }
    seen
}

fn brief(names: &[String], most: usize) -> String {
    let shown = names[..names.len().min(most)].join(", ");
    if names.len() > most {
        format!("{shown}, ...")
    } else {
        shown
    }
}

/// Whether the module builds, opening its own resolver (as `kjk tour`).
fn build(index: &DirIndex, game: Game, module: &str) -> Result<ModuleIr, String> {
    let resolver = Resolver::open(game, index, Some(module), TextureQuality::High)
        .map_err(|error| error.to_string())?;
    ModuleIr::build(module, &ResolverSource(&resolver)).map_err(|refusal| format!("{refusal:?}"))
}

/// Starts the child that renders `module` from its entry, its output to
/// `log`.
fn spawn_render(
    game: Game,
    module: &ModuleIr,
    size: (u32, u32),
    log: &Path,
) -> Result<std::process::Child, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let log = std::fs::File::create(log).map_err(|error| format!("{}: {error}", log.display()))?;
    let script = format!(
        "anim 0; {}; screenshot shot; dump; quit",
        crate::tour::entry_camera(module)
    );
    Command::new(exe)
        .args(["module", game.key(), &module.name, "--headless", "--size"])
        .arg(format!("{}x{}", size.0, size.1))
        .arg("--cmds")
        .arg(script)
        .stdout(Stdio::null())
        .stderr(log.try_clone().map_err(|error| error.to_string())?)
        .spawn()
        .map_err(|error| error.to_string())
}

/// Waits for the render child (spawned at `spawned`), then takes its shot
/// into `shot` and its scene facts, and removes the run folder it made.
fn finish_render(
    (mut child, spawned): (std::process::Child, Instant),
    log: &Path,
    shot: &Path,
) -> Result<Rendered, String> {
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break Some(status);
        }
        if spawned.elapsed() > RENDER_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let text = std::fs::read_to_string(log).unwrap_or_else(|_| String::new());
    let folder = crate::tour::run_folder(&text);
    let taken = folder.as_ref().map(|folder| {
        (
            std::fs::copy(folder.join("shot.png"), shot),
            std::fs::read_to_string(folder.join("dump-1.json")),
        )
    });
    // The child's folder holds only what was just taken from it.
    if let Some(folder) = folder.filter(|folder| folder.starts_with(crate::runtime::runs())) {
        let _ = std::fs::remove_dir_all(folder);
    }
    let Some(status) = status else {
        return Err(format!("timed out after {} s", RENDER_TIMEOUT.as_secs()));
    };
    let tail = || {
        let lines: Vec<&str> = text.lines().rev().take(3).collect();
        format!("exit {:?}: {}", status.code(), lines.join(" | "))
    };
    let (taken, dump) = taken.ok_or_else(tail)?;
    taken.map_err(|_| format!("no shot ({})", tail()))?;
    let dump: Value = dump
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .ok_or_else(|| format!("no scene facts ({})", tail()))?;
    Ok(Rendered {
        scene: dump["facts"]["scene"].clone(),
        black: black(shot),
    })
}

/// The fraction of the frame's pixels that are (nearly) black.
fn black(shot: &Path) -> f32 {
    let Some(image) = crate::tour::load(shot) else {
        return 1.0;
    };
    let dark = image
        .pixels()
        .filter(|pixel| pixel.0.iter().all(|&c| c < 12))
        .count();
    dark as f32 / (image.width() * image.height()).max(1) as f32
}

/// The worker's part of one module: build it, then render it while its
/// audit runs.
fn survey_module(
    discovery: &Discovery,
    index: &DirIndex,
    (game, module): (Game, &str),
    folder: &Path,
    options: &Options,
) -> Row {
    let id = format!("{}-{module}", game.key());
    let started = Instant::now();
    let built = std::panic::catch_unwind(AssertUnwindSafe(|| build(index, game, module)))
        .unwrap_or_else(|_| Err("panicked".into()));
    let mut row = Row {
        game,
        module: module.to_string(),
        built: built.as_ref().map(|_| ()).map_err(Clone::clone),
        scripts: None,
        render: None,
        audit: None,
        seconds: 0.0,
    };
    let Ok(ir) = built else {
        return row;
    };
    let log = folder.join("logs").join(format!("{id}.log"));
    let child = options
        .render
        .then(|| spawn_render(game, &ir, options.size, &log).map(|child| (child, Instant::now())));
    if options.audit {
        let surveyed = std::panic::catch_unwind(AssertUnwindSafe(|| {
            survey(discovery, game.key(), module, MAP_SIZE)
        }))
        .unwrap_or_else(|_| Err("panicked".into()));
        row.audit = Some(surveyed.and_then(|(survey, map)| {
            let path = folder.join("audits").join(format!("{id}.png"));
            map.save(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            Ok(survey)
        }));
    }
    row.render = child.map(|child| {
        let shot = folder.join("shots").join(format!("{id}.png"));
        child.and_then(|child| finish_render(child, &log, &shot))
    });
    row.seconds = started.elapsed().as_secs_f64();
    row
}

/// `kjk sweep`.
pub fn sweep(discovery: &Discovery, options: &Options) -> ExitCode {
    let folder = match crate::runtime::create_run_folder() {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("cannot create the run folder: {error}");
            return ExitCode::FAILURE;
        }
    };
    for sub in ["shots", "audits", "logs"] {
        if let Err(error) = std::fs::create_dir_all(folder.join(sub)) {
            eprintln!("{}: {error}", folder.join(sub).display());
            return ExitCode::FAILURE;
        }
    }
    let started = Instant::now();
    let mut rows: Vec<Row> = Vec::new();
    for &game in &options.games {
        // A game whose install is missing leaves the others' report.
        match sweep_game(discovery, game, &folder, options) {
            Ok(done) => rows.extend(done),
            Err(error) => eprintln!("{}: {error}; left out", game.key()),
        }
    }
    if rows.is_empty() {
        eprintln!("no modules swept");
        return ExitCode::FAILURE;
    }
    let seconds = started.elapsed().as_secs_f64();
    let report = Report::new(&rows, options, seconds);
    report.print();
    let writes = [
        ("index.html", report.gallery()),
        ("bugs.md", report.bugs()),
        (
            "facts.json",
            serde_json::to_string_pretty(&report.facts()).expect("json values serialize"),
        ),
    ];
    for (name, text) in writes {
        if let Err(error) = std::fs::write(folder.join(name), text) {
            eprintln!("{name}: {error}");
        }
    }
    let games = if options.render {
        options.games.as_slice()
    } else {
        &[]
    };
    for &game in games {
        let shots: Vec<PathBuf> = report
            .rows(game)
            .map(|row| folder.join("shots").join(format!("{}.png", row.id())))
            .collect();
        if shots.is_empty() {
            continue;
        }
        let columns = (shots.len() as f32).sqrt().ceil() as u32;
        let cell = (240, 240 * options.size.1 / options.size.0.max(1));
        let path = folder.join(format!("sheet-{}.png", game.key()));
        if let Err(error) = crate::tour::sheet(&shots, cell, columns).save(&path) {
            eprintln!("{}: {error}", path.display());
        }
    }
    outln!("sweep: {}", folder.join("index.html").display());
    ExitCode::SUCCESS
}

/// One game: the workers build, render and audit each module while this
/// thread steps their scripts.
fn sweep_game(
    discovery: &Discovery,
    game: Game,
    folder: &Path,
    options: &Options,
) -> Result<Vec<Row>, String> {
    let install = discovery
        .get(game)
        .ok_or(format!("not found (set {})", game.env_var()))?;
    let index = DirIndex::build(&install.data_root).map_err(|error| error.to_string())?;
    let modules: Vec<String> = Resolver::modules(&index)
        .into_iter()
        .filter(|module| {
            options
                .only
                .as_deref()
                .is_none_or(|only| module.starts_with(only))
        })
        .collect();
    outln!(
        "{}: {} modules, {} workers",
        game.key(),
        modules.len(),
        options.jobs
    );
    let next = AtomicUsize::new(0);
    let done: Mutex<BTreeMap<String, Row>> = Mutex::new(BTreeMap::new());
    let mut scripts: BTreeMap<String, Result<Swept, String>> = BTreeMap::new();
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.max(1) {
            scope.spawn(|| {
                while let Some(module) = modules.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let row = survey_module(discovery, &index, (game, module), folder, options);
                    let line = row
                        .problems()
                        .first()
                        .map_or("clean".to_string(), |p| p.text.clone());
                    outln!("  {} {module:<12} {line}", game.key());
                    done.lock()
                        .expect("no worker panics holding it")
                        .insert(module.clone(), row);
                }
            });
        }
        if options.scripts {
            let anims = std::sync::Arc::new(sim::motion::AnimTable::stand_in());
            let installer = crate::lifecycle::Installer::new(discovery, game, anims);
            let stepping = coverage(options.seconds);
            for module in &modules {
                let swept = match &installer {
                    Ok(installer) => sweep_module(installer, module, &stepping),
                    Err(error) => Err(format!("no installer: {error}")),
                };
                scripts.insert(module.clone(), swept);
            }
        }
    });
    let mut done = done.into_inner().expect("no worker panics holding it");
    for (module, swept) in scripts {
        if let Some(row) = done.get_mut(&module) {
            row.scripts = Some(swept);
        }
    }
    Ok(done.into_values().collect())
}

/// The sweep's findings, gathered for the page, the bug lists and the
/// facts.
struct Report<'a> {
    /// Worst first.
    ranked: Vec<&'a Row>,
    options: &'a Options,
    seconds: f64,
}

/// A name and the modules (`game-module`) it is found in.
type Found = BTreeMap<String, Vec<String>>;

impl<'a> Report<'a> {
    fn new(rows: &'a [Row], options: &'a Options, seconds: f64) -> Report<'a> {
        let mut ranked: Vec<&Row> = rows.iter().collect();
        ranked.sort_by_cached_key(|row| {
            (
                std::cmp::Reverse(row.score()),
                row.game.key(),
                row.module.clone(),
            )
        });
        Report {
            ranked,
            options,
            seconds,
        }
    }

    fn rows(&self, game: Game) -> impl Iterator<Item = &&'a Row> {
        self.ranked.iter().filter(move |row| row.game == game)
    }

    /// The games' keys, joined by `separator`.
    fn games(&self, separator: &str) -> String {
        self.options
            .games
            .iter()
            .map(|g| g.key())
            .collect::<Vec<_>>()
            .join(separator)
    }

    /// Modules that refuse, or fail a stage: (module, why).
    fn failures(&self) -> Vec<(String, String)> {
        self.ranked
            .iter()
            .flat_map(|row| {
                row.problems()
                    .into_iter()
                    .filter(|p| {
                        p.area == Area::Refusal || (p.area == Area::Render && p.score >= 300)
                    })
                    .map(|p| (row.id(), p.text))
            })
            .collect()
    }

    /// Script faults by message.
    fn faults(&self) -> Found {
        let mut found = Found::new();
        for row in &self.ranked {
            if let Some(swept) = row.swept() {
                for fault in distinct(&swept.faults) {
                    found.entry(fault).or_default().push(row.id());
                }
            }
        }
        found
    }

    /// Scripts named but missing (nothing runs), by name.
    fn missing(&self) -> Found {
        let mut found = Found::new();
        for row in &self.ranked {
            if let Some(swept) = row.swept() {
                for script in swept.missing.keys() {
                    found.entry(script.clone()).or_default().push(row.id());
                }
            }
        }
        found
    }

    /// Missing assets and refused creatures by name, per kind (textures
    /// apart: `missing_textures`).
    fn assets(&self) -> Vec<(&'static str, Found)> {
        [
            ("missing_models", "Missing models"),
            ("creature_refusals", "Refused creatures"),
            ("missing_clips", "Missing clips"),
        ]
        .into_iter()
        .map(|(key, title)| (title, self.found(key, |name, _| name.to_string())))
        .collect()
    }

    /// Missing textures by (name, what was drawn in their place).
    fn missing_textures(&self) -> BTreeMap<(String, String), Vec<String>> {
        self.found("missing_textures", |name, rendered| {
            (name.to_string(), rendered.stand_in(name))
        })
    }

    /// The modules each entry of the scenes' `key` list appears in, keyed
    /// by `name` of the entry.
    fn found<K: Ord>(
        &self,
        key: &str,
        name: impl Fn(&str, &Rendered) -> K,
    ) -> BTreeMap<K, Vec<String>> {
        let mut found: BTreeMap<K, Vec<String>> = BTreeMap::new();
        for row in &self.ranked {
            if let Some(Ok(rendered)) = &row.render {
                for entry in distinct(&rendered.list(key)) {
                    found
                        .entry(name(&entry, rendered))
                        .or_default()
                        .push(row.id());
                }
            }
        }
        found
    }

    /// Modules that can be left on foot or with a plain jump, most escapes
    /// first: (module, walk, jump, void leaks).
    fn containment(&self) -> Vec<(String, usize, usize, usize)> {
        let mut found: Vec<(String, usize, usize, usize)> = self
            .ranked
            .iter()
            .filter_map(|row| match &row.audit {
                Some(Ok(audit)) => {
                    let level = |n: usize| audit.reaches.get(n).map_or(0, |r| r.escapes);
                    Some((row.id(), level(0), level(1), audit.voids))
                }
                _ => None,
            })
            .filter(|(_, walk, jump, voids)| walk + jump + voids > 0)
            .collect();
        found.sort_by_key(|f| std::cmp::Reverse((f.1, f.2, f.3)));
        found
    }

    /// The audits that took longest, slowest first: (module, seconds).
    fn slowest(&self) -> Vec<(String, f64)> {
        let mut found: Vec<(String, f64)> = self
            .ranked
            .iter()
            .filter_map(|row| match &row.audit {
                Some(Ok(audit)) => Some((row.id(), audit.seconds)),
                _ => None,
            })
            .collect();
        found.sort_by(|a, b| b.1.total_cmp(&a.1));
        found.truncate(SLOWEST);
        found
    }

    /// Script gaps across the games: (routine, modules per game, calls).
    fn gaps(&self) -> Vec<(String, Vec<usize>, usize)> {
        let mut merged: BTreeMap<String, (Vec<usize>, usize)> = BTreeMap::new();
        for (index, &game) in self.options.games.iter().enumerate() {
            for (name, (calls, modules)) in rank_gaps(self.rows(game).filter_map(|row| row.swept()))
            {
                let entry = merged
                    .entry(name.to_string())
                    .or_insert((vec![0; self.options.games.len()], 0));
                entry.0[index] = modules;
                entry.1 += calls;
            }
        }
        let mut ranked: Vec<(String, Vec<usize>, usize)> = merged
            .into_iter()
            .map(|(name, (modules, calls))| (name, modules, calls))
            .collect();
        ranked.sort_by(|a, b| {
            let total = |modules: &[usize]| modules.iter().sum::<usize>();
            total(&b.1)
                .cmp(&total(&a.1))
                .then(b.2.cmp(&a.2))
                .then(a.0.cmp(&b.0))
        });
        ranked
    }

    /// Per game: modules, built, rendered, with script faults, escapable on
    /// foot or by jump, clean, distinct gaps, distinct missing scripts.
    fn tallies(&self) -> Vec<(Game, [usize; 8])> {
        self.options
            .games
            .iter()
            .map(|&game| {
                let rows: Vec<&&Row> = self.rows(game).collect();
                let count = |test: &dyn Fn(&Row) -> bool| rows.iter().filter(|r| test(r)).count();
                let swept = || rows.iter().filter_map(|row| row.swept());
                let gaps = rank_gaps(swept()).len();
                let missing = rank(swept(), |one| &one.missing).len();
                (
                    game,
                    [
                        rows.len(),
                        count(&|r| r.built.is_ok()),
                        count(&|r| matches!(r.render, Some(Ok(_)))),
                        count(&|r| r.swept().is_some_and(|s| !s.faults.is_empty())),
                        count(&|r| matches!(&r.audit, Some(Ok(a)) if a.escapes(2) > 0)),
                        count(&|r| r.clean()),
                        gaps,
                        missing,
                    ],
                )
            })
            .collect()
    }

    fn print(&self) {
        for (
            game,
            [
                modules,
                built,
                rendered,
                faulted,
                escaping,
                clean,
                gaps,
                missing,
            ],
        ) in self.tallies()
        {
            outln!(
                "{}: {modules} modules, {built} build, {rendered} rendered, {faulted} with script faults, {escaping} escapable on foot or by jump, {clean} without problems beyond script gaps and missing scripts; {gaps} distinct gaps, {missing} missing scripts",
                game.key()
            );
        }
        for (module, why) in self.failures() {
            outln!("  FAILED {module}: {why}");
        }
        outln!("({:.0} s)", self.seconds);
    }

    fn facts(&self) -> Value {
        json!({
            "verb": "sweep",
            "git": env!("KJK_GIT_SHA"),
            "games": self.options.games.iter().map(|g| g.key()).collect::<Vec<_>>(),
            "seconds_stepped": self.options.seconds,
            "seconds": self.seconds,
            "modules": self.ranked.iter().map(|row| row.json()).collect::<Vec<_>>(),
            "gaps": self.gaps().iter().map(|(name, modules, calls)| json!({
                "routine": name, "modules": modules, "calls": calls,
            })).collect::<Vec<_>>(),
            "missing_scripts": self.missing(),
        })
    }

    /// The bug lists, in Markdown, one section per area.
    fn bugs(&self) -> String {
        let mut text = format!(
            "# Whole-game sweep\n\n{} at {}, {} s of scripts a module, {:.0} s in all. Modules are scored: {SCORING}.\n",
            self.games(" and "),
            env!("KJK_GIT_SHA"),
            self.options.seconds,
            self.seconds
        );
        let _ = writeln!(text, "\n## Refusals and failed stages\n");
        for (module, why) in self.failures() {
            let _ = writeln!(text, "- `{module}`: {why}");
        }
        let _ = writeln!(text, "\n## Script faults\n");
        for (fault, modules) in self.faults() {
            let _ = writeln!(text, "- {fault}: {}", modules.join(", "));
        }
        let _ = writeln!(
            text,
            "\n## Script gaps, by modules reaching them\n\n| routine | {} | calls |\n|---|{}---|",
            self.games(" | "),
            "---|".repeat(self.options.games.len())
        );
        for (name, modules, calls) in self.gaps() {
            let counts: Vec<String> = modules.iter().map(usize::to_string).collect();
            let _ = writeln!(text, "| {name} | {} | {calls} |", counts.join(" | "));
        }
        let _ = writeln!(text, "\n## Scripts named but missing (nothing runs)\n");
        for (script, modules) in self.missing() {
            let _ = writeln!(text, "- `{script}`: {}", modules.join(", "));
        }
        let _ = writeln!(
            text,
            "\n## Containment: escapes on foot and by jump\n\n| module | walk | jump | void leaks |\n|---|---|---|---|"
        );
        for (module, walk, jump, voids) in self.containment() {
            let _ = writeln!(text, "| {module} | {walk} | {jump} | {voids} |");
        }
        let _ = writeln!(
            text,
            "\n## Slowest audits\n\n| module | seconds |\n|---|---|"
        );
        for (module, seconds) in self.slowest() {
            let _ = writeln!(text, "| {module} | {seconds:.0} |");
        }
        for (title, found) in self.assets() {
            let _ = writeln!(text, "\n## {title}\n");
            for (name, modules) in found {
                let _ = writeln!(text, "- `{name}`: {}", modules.join(", "));
            }
        }
        let _ = writeln!(
            text,
            "\n## Missing textures\n\n{TEXTURES_NOTE}\n\n| texture | in its place | modules |\n|---|---|---|"
        );
        for ((name, stand_in), modules) in self.missing_textures() {
            let _ = writeln!(text, "| `{name}` | {stand_in} | {} |", modules.join(", "));
        }
        text
    }

    /// The gallery page: tallies, every module worst first with its frame
    /// and audit map, filterable by game and area, and the bug lists.
    fn gallery(&self) -> String {
        let mut cards = String::new();
        for row in &self.ranked {
            let problems = row.problems();
            let score = row.score();
            let mut areas: Vec<&str> = problems.iter().map(|p| p.area.key()).collect();
            areas.sort_unstable();
            areas.dedup();
            if row.clean() {
                areas.push("clean");
            }
            let id = row.id();
            let shot = match &row.render {
                Some(Ok(_)) => format!(
                    r#"<a href="shots/{id}.png"><img loading="lazy" src="shots/{id}.png" alt=""></a>"#
                ),
                Some(Err(why)) => format!(r#"<div class="none">{}</div>"#, html(why)),
                None => r#"<div class="none">not rendered</div>"#.into(),
            };
            let map = match &row.audit {
                Some(Ok(_)) => format!(
                    r#"<a href="audits/{id}.png"><img loading="lazy" class="map" src="audits/{id}.png" alt="audit map"></a>"#
                ),
                _ => String::new(),
            };
            let chips: String = problems
                .iter()
                .map(|p| format!(r#"<li class="{}">{}</li>"#, p.area.key(), html(&p.text)))
                .collect();
            let _ = write!(
                cards,
                r#"<article data-game="{game}" data-areas="{areas}"><div class="frames">{shot}{map}</div><h3><span class="game">{game}</span> {module}<span class="score">{score}</span></h3><ul>{chips}</ul></article>"#,
                game = row.game.key(),
                areas = areas.join(" "),
                module = html(&row.module),
            );
        }
        let tallies: String = self
            .tallies()
            .iter()
            .map(|(game, [modules, built, rendered, faulted, escaping, clean, gaps, missing])| {
                format!(
                    r#"<div class="tally"><h2>{}</h2><dl><dt>modules</dt><dd>{modules}</dd><dt>build</dt><dd>{built}</dd><dt>rendered</dt><dd>{rendered}</dd><dt>script faults</dt><dd>{faulted}</dd><dt>escapable</dt><dd>{escaping}</dd><dt>clean</dt><dd>{clean}</dd><dt>distinct gaps</dt><dd>{gaps}</dd><dt>missing scripts</dt><dd>{missing}</dd></dl></div>"#,
                    game.key()
                )
            })
            .collect();
        let games: String = self
            .options
            .games
            .iter()
            .map(|g| format!(r#"<button data-game="{0}">{0}</button>"#, g.key()))
            .collect();
        let areas: String = Area::ALL
            .iter()
            .map(|a| a.key())
            .chain(["clean"])
            .map(|a| format!(r#"<button data-area="{a}">{a}</button>"#))
            .collect();
        let lists = self.lists_html();
        format!(
            r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Whole-game sweep</title>
<style>{STYLE}</style></head><body>
<header><h1>Whole-game sweep</h1><p>{games_named} at <code>{git}</code>, {seconds} s of scripts a module, {total:.0} s in all. Modules worst first; the number is the module's score ({SCORING}).</p></header>
<section class="tallies">{tallies}</section>
<nav><span>game</span><button data-game="" class="on">all</button>{games}<span>area</span><button data-area="" class="on">all</button>{areas}</nav>
<section class="grid">{cards}</section>
{lists}
<script>{SCRIPT}</script>
</body></html>
"#,
            games_named = self.games(" and "),
            git = env!("KJK_GIT_SHA"),
            seconds = self.options.seconds,
            total = self.seconds,
        )
    }

    /// The bug lists as tables.
    fn lists_html(&self) -> String {
        let mut out = String::new();
        let table = |out: &mut String, title: &str, head: &[&str], rows: Vec<Vec<String>>| {
            let _ = write!(
                out,
                "<section class=\"list\"><h2>{} <small>{}</small></h2><div class=\"scroll\"><table><thead><tr>",
                html(title),
                rows.len()
            );
            for cell in head {
                let _ = write!(out, "<th>{}</th>", html(cell));
            }
            out.push_str("</tr></thead><tbody>");
            for row in rows {
                out.push_str("<tr>");
                for cell in row {
                    let _ = write!(out, "<td>{}</td>", html(&cell));
                }
                out.push_str("</tr>");
            }
            out.push_str("</tbody></table></div></section>");
        };
        let found = |found: Found| -> Vec<Vec<String>> {
            let mut rows: Vec<(String, Vec<String>)> = found.into_iter().collect();
            rows.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
            rows.into_iter()
                .map(|(name, modules)| vec![name, modules.len().to_string(), modules.join(", ")])
                .collect()
        };
        table(
            &mut out,
            "Refusals and failed stages",
            &["module", "why"],
            self.failures()
                .into_iter()
                .map(|(module, why)| vec![module, why])
                .collect(),
        );
        table(
            &mut out,
            "Script faults",
            &["fault", "modules", "where"],
            found(self.faults()),
        );
        let mut head = vec!["routine"];
        head.extend(self.options.games.iter().map(|g| g.key()));
        head.push("calls");
        table(
            &mut out,
            "Script gaps, by modules reaching them",
            &head,
            self.gaps()
                .into_iter()
                .map(|(name, modules, calls)| {
                    let mut cells = vec![name];
                    cells.extend(modules.iter().map(usize::to_string));
                    cells.push(calls.to_string());
                    cells
                })
                .collect(),
        );
        table(
            &mut out,
            "Scripts named but missing (nothing runs)",
            &["script", "modules", "where"],
            found(self.missing()),
        );
        table(
            &mut out,
            "Containment: escapes on foot and by jump",
            &["module", "walk", "jump", "void leaks"],
            self.containment()
                .into_iter()
                .map(|(module, walk, jump, voids)| {
                    vec![
                        module,
                        walk.to_string(),
                        jump.to_string(),
                        voids.to_string(),
                    ]
                })
                .collect(),
        );
        table(
            &mut out,
            "Slowest audits",
            &["module", "seconds"],
            self.slowest()
                .into_iter()
                .map(|(module, seconds)| vec![module, format!("{seconds:.0}")])
                .collect(),
        );
        for (title, assets) in self.assets() {
            table(
                &mut out,
                title,
                &["name", "modules", "where"],
                found(assets),
            );
        }
        let mut textures: Vec<_> = self.missing_textures().into_iter().collect();
        textures.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
        table(
            &mut out,
            "Missing textures",
            &["name", "in its place", "modules", "where"],
            textures
                .into_iter()
                .map(|((name, stand_in), modules)| {
                    vec![
                        name,
                        stand_in,
                        modules.len().to_string(),
                        modules.join(", "),
                    ]
                })
                .collect(),
        );
        out
    }
}

fn html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const STYLE: &str = r#"
:root { --bg: #f6f6f4; --card: #fff; --ink: #1d1d1f; --dim: #6b6b70; --line: #deded9;
  --refusal: #b3261e; --render: #8a4fbf; --scripts: #a15c00; --containment: #0b6aa2; --assets: #2f7d4f; }
@media (prefers-color-scheme: dark) { :root:not([data-theme="light"]) { --bg: #151517; --card: #1f1f22; --ink: #ececee;
  --dim: #9a9aa2; --line: #33333a; --refusal: #ff7a70; --render: #c69cf0; --scripts: #f0a64a; --containment: #6cc0f0; --assets: #74c98f; } }
:root[data-theme="dark"] { --bg: #151517; --card: #1f1f22; --ink: #ececee; --dim: #9a9aa2; --line: #33333a;
  --refusal: #ff7a70; --render: #c69cf0; --scripts: #f0a64a; --containment: #6cc0f0; --assets: #74c98f; }
* { box-sizing: border-box; }
body { margin: 0; padding: 16px; background: var(--bg); color: var(--ink); font: 14px/1.45 system-ui, sans-serif; }
header p { color: var(--dim); max-width: 70em; }
h1 { margin: 0 0 4px; font-size: 22px; }
.tallies { display: flex; flex-wrap: wrap; gap: 12px; margin: 12px 0; }
.tally { background: var(--card); border: 1px solid var(--line); border-radius: 8px; padding: 8px 14px; }
.tally h2 { margin: 0 0 4px; font-size: 15px; text-transform: uppercase; }
.tally dl { display: grid; grid-template-columns: repeat(7, auto); gap: 0 14px; margin: 0; }
.tally dt { color: var(--dim); font-size: 12px; grid-row: 1; }
.tally dd { margin: 0; font-size: 18px; font-variant-numeric: tabular-nums; grid-row: 2; }
nav { position: sticky; top: 0; background: var(--bg); padding: 8px 0; display: flex; flex-wrap: wrap; gap: 6px; align-items: center; z-index: 1; }
nav span { color: var(--dim); margin-left: 8px; }
nav button { font: inherit; border: 1px solid var(--line); background: var(--card); color: var(--ink); border-radius: 14px; padding: 2px 10px; cursor: pointer; }
nav button.on { background: var(--ink); color: var(--bg); }
.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 340px), 1fr)); gap: 12px; }
article { background: var(--card); border: 1px solid var(--line); border-radius: 8px; overflow: hidden; }
article[hidden] { display: none; }
.frames { display: grid; grid-template-columns: 1fr 34%; grid-template-rows: 100%; background: #000; aspect-ratio: 16 / 6.2; }
.frames > a { display: block; min-width: 0; min-height: 0; overflow: hidden; }
.frames img { width: 100%; height: 100%; object-fit: cover; display: block; }
.frames img.map { object-fit: contain; background: #2d2d37; }
.none { color: #f99; padding: 10px; font-size: 12px; overflow: hidden; height: 100%; }
article h3 { margin: 6px 10px 2px; font-size: 14px; display: flex; gap: 6px; align-items: baseline; }
.game { color: var(--dim); font-weight: 400; text-transform: uppercase; font-size: 11px; }
.score { margin-left: auto; color: var(--dim); font-variant-numeric: tabular-nums; font-weight: 400; }
article ul { margin: 0 10px 8px; padding: 0; list-style: none; font-size: 12px; }
article li { border-left: 3px solid; padding-left: 6px; margin-top: 3px; overflow-wrap: anywhere; }
li.refusal { border-color: var(--refusal); } li.render { border-color: var(--render); }
li.scripts { border-color: var(--scripts); } li.containment { border-color: var(--containment); } li.assets { border-color: var(--assets); }
.list h2 { font-size: 16px; margin: 28px 0 6px; } .list small { color: var(--dim); font-weight: 400; }
.scroll { max-height: 420px; overflow: auto; border: 1px solid var(--line); border-radius: 8px; background: var(--card); }
table { border-collapse: collapse; width: 100%; font-size: 12px; }
th, td { text-align: left; padding: 3px 8px; border-bottom: 1px solid var(--line); vertical-align: top; overflow-wrap: anywhere; }
th { position: sticky; top: 0; background: var(--card); }
"#;

const SCRIPT: &str = r#"
const filter = { game: "", area: "" };
function apply() {
  for (const card of document.querySelectorAll("article")) {
    const game = !filter.game || card.dataset.game === filter.game;
    const area = !filter.area || card.dataset.areas.split(" ").includes(filter.area);
    card.hidden = !(game && area);
  }
}
for (const button of document.querySelectorAll("nav button")) {
  const key = "game" in button.dataset ? "game" : "area";
  button.addEventListener("click", () => {
    filter[key] = button.dataset[key];
    for (const other of document.querySelectorAll(`nav button[data-${key}]`)) other.classList.toggle("on", other === button);
    apply();
  });
}
"#;
