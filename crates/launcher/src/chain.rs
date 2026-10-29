//! `kjk chain`: a game's routes (`data/routes/<game>/`) played through as
//! the chain they make (docs/RUN.md). Each route starts from the checkpoint
//! its header names (`kjk playthrough k1 @NAME data/routes/...`), which one
//! other route's `save NAME` makes, so the routes are a tree: the Endar
//! Spire at its root, the branches (the Sand People's war, Juhani slain,
//! the dark ending) off the main way. Each route is a `kjk playthrough`
//! child with the game's chain flags (`data/routes/<game>/chain.flags`)
//! and its header's. One starts as soon as the route before it has saved
//! its checkpoint (a save is written whole, `write_whole`), so branches
//! play beside the main way, up to `jobs` at once. By default the chain
//! stops at its first block: each route ends at its first blocked goal,
//! and no route starts after one.
//!
//! A run is its own. Its folder holds a copy of the binary every route
//! runs (a rebuild meanwhile changes nothing), the routes as they were when
//! it began, the checkpoints it starts from (`starts/`, the kept ones as
//! they stood then) and those its routes save (`checkpoints/`). The
//! checkpoints of each route met are kept (`kjk-artifacts/checkpoints/
//! <game>`), where a route played alone starts; a blocked route's stay in
//! the run folder.
//!
//! A route's outcome is decided by the binary, the game's data, its start
//! checkpoint's bytes, the route and its flags alone: the sim never reads
//! the clock and draws from a seeded generator (rerun, a route meets or
//! blocks byte for byte the same). So each route met or blocked is cached
//! under a hash of those, with the checkpoints it saved
//! (`kjk-artifacts/chain-cache/<game>`), and an identical rerun is taken
//! from there instead of played.
//!
//! The checkpoints a game's `saves.list` names (`data/routes/<game>/`) are
//! written as saves the window loads (`k1-01`, `k1-02`, …, in the list's
//! order: `write_saves`), so each phase of the game is a Load away. A run
//! in the main checkout writes them; one in a worktree only with `--saves`,
//! as its binary is not the one the owner plays.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::time::{Duration, Instant, SystemTime};

use serde_json::json;
use vfs::{Discovery, Game};

use crate::playthrough::checkpoint;

/// How many binaries' runs the cache keeps (the last used).
const KEPT_BINARIES: usize = 3;

/// Bump when what a cache entry holds or what its key covers changes.
const CACHE_VERSION: &str = "kjk chain cache 2";

/// What a run plays and how.
pub struct Options {
    pub game: String,
    /// The routes asked for (by file name), and, unless `alone`, every
    /// route after them; none, every route.
    pub routes: Vec<String>,
    /// Each route chosen starts from the checkpoint kept, none waiting on
    /// another (`--alone`, `--quick`).
    pub alone: bool,
    pub jobs: usize,
    /// Each route ends at its first blocked goal and no route starts after
    /// one (`--keep-going` turns it off).
    pub fail_fast: bool,
    pub goal_seconds: Option<u64>,
    /// `--mighty` and `--skill NAME=RANK` for every route, beside the chain
    /// flags and its header's.
    pub mighty: bool,
    pub skills: Vec<String>,
    /// Without the game's chain flags (`--bare`).
    pub bare: bool,
    /// Identical runs taken from the cache (`--no-cache` turns it off).
    pub cache: bool,
    /// Each route's command and cached outcome said, nothing played.
    pub dry_run: bool,
    /// The saves list's checkpoints written as saves even from a worktree
    /// (`--saves`; the main checkout always writes them).
    pub saves: bool,
}

/// A route file, as its header plays it.
struct Route {
    name: String,
    path: PathBuf,
    text: String,
    /// A module, or `@NAME` for the checkpoint it starts from.
    start: String,
    /// The header's flags after the route's path (`--god`, `--skill ...`).
    flags: Vec<String>,
    /// The checkpoints its `save` goals make, each with whether it may be
    /// saved again (named twice, or in a `while`): the route starting from
    /// one such waits for this route's end, not its first save.
    saves: BTreeMap<String, bool>,
}

impl Route {
    /// The checkpoint it starts from, when it starts from one.
    fn starts_from(&self) -> Option<&str> {
        self.start.strip_prefix('@')
    }
}

/// How a route's run ended.
#[derive(Clone, Copy, PartialEq)]
enum Ended {
    /// Every goal met (the playthrough's exit 0).
    Met,
    /// Some goal blocked (exit 1).
    Blocked,
    /// The playthrough could not run (another exit, or none).
    Failed,
    /// Not run: the route before it did not save its checkpoint, or the
    /// chain stopped at a block.
    Skipped,
}

impl Ended {
    const ALL: [Ended; 4] = [Ended::Met, Ended::Blocked, Ended::Failed, Ended::Skipped];

    fn word(self) -> &'static str {
        match self {
            Ended::Met => "met",
            Ended::Blocked => "BLOCKED",
            Ended::Failed => "FAILED",
            Ended::Skipped => "skipped",
        }
    }

    fn from_word(word: &str) -> Option<Ended> {
        Ended::ALL.into_iter().find(|ended| ended.word() == word)
    }
}

/// A route's outcome: how it ended, its wall seconds, and its log's
/// `playthrough:` summary line, or why it did not run or ended so.
struct Done {
    ended: Ended,
    seconds: f64,
    summary: String,
    /// Taken from the cache, not played.
    cached: bool,
}

impl Done {
    fn not_run(ended: Ended, summary: String) -> Done {
        Done {
            ended,
            seconds: 0.0,
            summary,
            cached: false,
        }
    }
}

fn routes_folder(game: &str) -> PathBuf {
    Path::new("data/routes").join(game)
}

/// The route files of `game`, in name order.
fn read_routes(game: &str) -> Result<Vec<Route>, String> {
    let folder = routes_folder(game);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&folder)
        .map_err(|error| format!("{}: {error}", folder.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "route"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            read_route(game, path, text)
        })
        .collect()
}

/// A route file's header command and its `save` goals.
fn read_route(game: &str, path: PathBuf, text: String) -> Result<Route, String> {
    let name = path
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let header = format!("kjk playthrough {game} ");
    let words: Vec<&str> = text
        .lines()
        .find_map(|line| line.find(&header).map(|at| &line[at + header.len()..]))
        .ok_or_else(|| format!("{}: no `{}` line in its header", path.display(), header.trim()))?
        .split_whitespace()
        .collect();
    let [start, _path, flags @ ..] = words.as_slice() else {
        return Err(format!("{}: its header names no start and route", path.display()));
    };
    let start = start.to_string();
    let flags = flags.iter().map(|flag| flag.to_string()).collect();
    let saves = crate::playthrough::saves(&text).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(Route {
        name,
        path,
        text,
        start,
        flags,
        saves,
    })
}

/// The flags every route of `game` plays with in a chain
/// (`data/routes/<game>/chain.flags`, `#` starting a comment); none
/// without the file.
fn chain_flags(game: &str) -> Result<Vec<String>, String> {
    let path = routes_folder(game).join("chain.flags");
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(text
            .lines()
            .flat_map(|line| line.split('#').next().unwrap_or("").split_whitespace())
            .map(str::to_string)
            .collect()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

/// Each route's parent: the route whose `save` makes its checkpoint.
fn parents(routes: &[Route]) -> Vec<Option<usize>> {
    routes
        .iter()
        .map(|route| {
            let from = route.starts_from()?;
            routes.iter().position(|other| other.saves.contains_key(from))
        })
        .collect()
}

/// The routes to play: those named and, unless `alone`, every route after
/// them; none named, all.
fn chosen(routes: &[Route], parents: &[Option<usize>], options: &Options) -> Result<BTreeSet<usize>, String> {
    if options.routes.is_empty() {
        return Ok((0..routes.len()).collect());
    }
    let mut chosen = BTreeSet::new();
    for name in &options.routes {
        let index = routes
            .iter()
            .position(|route| route.name == *name)
            .ok_or_else(|| format!("no route {name} in data/routes/{}", options.game))?;
        chosen.insert(index);
    }
    // The routes after them, until no more join.
    if !options.alone {
        loop {
            let more: Vec<usize> = (0..routes.len())
                .filter(|index| !chosen.contains(index))
                .filter(|index| parents[*index].is_some_and(|parent| chosen.contains(&parent)))
                .collect();
            if more.is_empty() {
                break;
            }
            chosen.extend(more);
        }
    }
    Ok(chosen)
}

/// A route's `kjk playthrough` arguments as its header names them (its
/// start `@NAME`, its route file): the game's chain flags, the header's,
/// then those given for every route, each flag once.
fn arguments(route: &Route, chain_flags: &[String], options: &Options) -> Vec<String> {
    let mut flags: Vec<String> = chain_flags.iter().chain(&route.flags).cloned().collect();
    if options.mighty {
        flags.push("--mighty".to_string());
    }
    for skill in &options.skills {
        flags.extend(["--skill".to_string(), skill.clone()]);
    }
    if !options.fail_fast {
        flags.push("--keep-going".to_string());
    }
    if let Some(seconds) = options.goal_seconds {
        flags.extend(["--goal-seconds".to_string(), seconds.to_string()]);
    }
    let mut arguments = vec![
        "playthrough".to_string(),
        options.game.clone(),
        route.start.clone(),
        route.path.display().to_string(),
    ];
    arguments.extend(once_each(&flags));
    arguments
}

/// `flags` with each flag (a `--NAME` and the values after it) where it
/// was first given, its repeats left out.
fn once_each(flags: &[String]) -> Vec<String> {
    let mut kept: Vec<&[String]> = Vec::new();
    let mut rest = flags;
    while !rest.is_empty() {
        let values = rest[1..].iter().take_while(|word| !word.starts_with("--")).count();
        let (flag, after) = rest.split_at(1 + values);
        if !kept.contains(&flag) {
            kept.push(flag);
        }
        rest = after;
    }
    kept.concat()
}

/// What every route's outcome hangs on beside its own: the binary's hash,
/// the game data's fingerprint, and the animation table a playthrough reads
/// from Jedi Academy (its stand-in without it: swings' timing and blades).
struct Inputs {
    binary: String,
    data: String,
    anims: [u8; 32],
}

impl Inputs {
    fn of(discovery: &Discovery, game: &str, binary: &Path) -> Result<Inputs, String> {
        let mut hasher = blake3::Hasher::new();
        std::fs::File::open(binary)
            .and_then(|file| hasher.update_reader(file).map(drop))
            .map_err(|error| format!("{}: {error}", binary.display()))?;
        let game = Game::from_key(game).ok_or("use k1 or tsl")?;
        let install = crate::installs::open(discovery, game)?;
        let print = vfs::fingerprint::fingerprint(
            game,
            &install.index,
            Some(&crate::fingerprint_cache(game)),
        )
        .map_err(|error| format!("the game data's fingerprint: {error}"))?;
        Ok(Inputs {
            binary: hasher.finalize().to_hex().to_string(),
            data: print.digest,
            anims: crate::stage::anims(discovery).0.digest(),
        })
    }

    /// The cache key of a route's run: its start checkpoint's bytes (none
    /// for a module), its route's text and its arguments (`arguments`).
    fn key(&self, start: &[u8], route: &str, arguments: &[String]) -> String {
        let mut hasher = blake3::Hasher::new();
        // A build file a flag names is read by the run: its text counts.
        let builds: Vec<u8> = arguments
            .windows(2)
            .filter(|pair| pair[0] == "--build")
            .flat_map(|pair| std::fs::read(&pair[1]).unwrap_or_else(|_| Vec::new()))
            .collect();
        let arguments = arguments.join("\n");
        for part in [
            CACHE_VERSION.as_bytes(),
            self.binary.as_bytes(),
            self.data.as_bytes(),
            &self.anims,
            start,
            route.as_bytes(),
            arguments.as_bytes(),
            &builds,
        ] {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part);
        }
        hasher.finalize().to_hex().to_string()
    }
}

/// The cache of route runs met or blocked, of one binary: an entry per key
/// with its outcome, its log and the checkpoints it saved.
struct Cache {
    folder: PathBuf,
}

/// An outcome the cache holds.
struct Cached {
    done: Done,
    saves: Vec<String>,
}

impl Cache {
    fn of(game: &str, inputs: &Inputs) -> Cache {
        let folder = crate::runtime::artifacts()
            .join("chain-cache")
            .join(game)
            .join(&inputs.binary[..16]);
        Cache { folder }
    }

    fn entry(&self, key: &str) -> PathBuf {
        self.folder.join(key)
    }

    /// Marks this binary's cache used now, and removes other binaries'
    /// beyond the `KEPT_BINARIES` last used.
    fn mark_used(&self) {
        if write_whole(&self.folder.join("used"), b"").is_err() {
            return;
        }
        let Some(game) = self.folder.parent() else {
            return;
        };
        let Ok(entries) = std::fs::read_dir(game) else {
            return;
        };
        let mut used: Vec<(SystemTime, PathBuf)> = entries
            .filter_map(|entry| {
                let folder = entry.ok()?.path();
                let at = std::fs::metadata(folder.join("used")).ok()?.modified().ok()?;
                Some((at, folder))
            })
            .collect();
        used.sort_by_key(|(at, _)| std::cmp::Reverse(*at));
        for (_, folder) in used.into_iter().skip(KEPT_BINARIES) {
            let _ = std::fs::remove_dir_all(folder);
        }
    }

    fn get(&self, key: &str) -> Option<Cached> {
        let entry = self.entry(key);
        let text = std::fs::read_to_string(entry.join("outcome.json")).ok()?;
        let outcome: serde_json::Value = serde_json::from_str(&text).ok()?;
        let ended = Ended::from_word(outcome["ended"].as_str()?)?;
        Some(Cached {
            done: Done {
                ended,
                seconds: outcome["seconds"].as_f64()?,
                summary: outcome["summary"].as_str()?.to_string(),
                cached: true,
            },
            saves: outcome["saves"]
                .as_array()?
                .iter()
                .filter_map(|save| save.as_str().map(str::to_string))
                .collect(),
        })
    }

    /// A route's run kept under `key`: its outcome, its log and the
    /// checkpoints `saves` in `saved`. Another run keeping the same key
    /// first wins (the two are the same).
    fn put(&self, key: &str, done: &Done, log: &Path, saved: &Path, saves: &[String]) -> std::io::Result<()> {
        let entry = self.entry(key);
        let partial = partial(&entry)?;
        std::fs::create_dir_all(&partial)?;
        place(log, &partial.join("route.log"))?;
        for save in saves {
            let name = format!("{save}.kjkv");
            place(&saved.join(&name), &partial.join(name))?;
        }
        let outcome = json!({
            "ended": done.ended.word(),
            "seconds": done.seconds,
            "summary": done.summary,
            "saves": saves,
        });
        std::fs::write(partial.join("outcome.json"), outcome.to_string())?;
        if std::fs::rename(&partial, &entry).is_err() {
            std::fs::remove_dir_all(&partial)?;
        }
        Ok(())
    }

    /// A cached run's log and checkpoints placed where a run of it would
    /// have left them.
    fn restore(&self, key: &str, cached: &Cached, log: &Path, saved: &Path) -> std::io::Result<()> {
        let entry = self.entry(key);
        place(&entry.join("route.log"), log)?;
        for save in &cached.saves {
            let name = format!("{save}.kjkv");
            place(&entry.join(&name), &saved.join(name))?;
        }
        Ok(())
    }
}

/// Writes `bytes` to `path` whole or not at all: into a file beside it,
/// then renamed over it, so a reader (a chain waiting on a checkpoint)
/// never sees part of it and a file linked elsewhere is never written into.
pub(crate) fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let partial = partial(path)?;
    std::fs::write(&partial, bytes)?;
    std::fs::rename(&partial, path)
}

/// `from` placed at `to` whole, as `write_whole` does: linked where the
/// file system allows (checkpoints are replaced, never written into, so a
/// link shares bytes that never change), else copied.
fn place(from: &Path, to: &Path) -> std::io::Result<()> {
    let partial = partial(to)?;
    let _ = std::fs::remove_file(&partial);
    std::fs::hard_link(from, &partial).or_else(|_| std::fs::copy(from, &partial).map(drop))?;
    std::fs::rename(&partial, to)
}

/// A path beside `path` (this process's own) to make it in first, its
/// folder made.
fn partial(path: &Path) -> std::io::Result<PathBuf> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let mut name = path.file_name().unwrap_or(path.as_os_str()).to_os_string();
    name.push(format!(".{}.partial", std::process::id()));
    Ok(path.with_file_name(name))
}

/// The `playthrough:` summary line of a route's log.
fn summary(log: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(log) else {
        return format!("no log at {}", log.display());
    };
    text.lines()
        .rev()
        .find_map(|line| line.strip_prefix("playthrough: "))
        .unwrap_or("no summary")
        .to_string()
}

/// A chain's plan: the routes, which are played, and how.
struct Plan {
    routes: Vec<Route>,
    parents: Vec<Option<usize>>,
    chosen: BTreeSet<usize>,
    /// Each chosen route's `kjk playthrough` arguments (`arguments`).
    arguments: BTreeMap<usize, Vec<String>>,
}

impl Plan {
    fn new(options: &Options) -> Result<Plan, String> {
        let routes = read_routes(&options.game)?;
        let parents = parents(&routes);
        let chosen = chosen(&routes, &parents, options)?;
        let flags = if options.bare {
            Vec::new()
        } else {
            chain_flags(&options.game)?
        };
        let arguments = chosen
            .iter()
            .map(|&index| (index, arguments(&routes[index], &flags, options)))
            .collect();
        Ok(Plan {
            routes,
            parents,
            chosen,
            arguments,
        })
    }

    /// The route whose save `index` waits on in this run: its parent,
    /// chosen, unless routes play alone.
    fn waits_on(&self, index: usize, options: &Options) -> Option<usize> {
        self.parents[index].filter(|parent| !options.alone && self.chosen.contains(parent))
    }
}

/// Where a run keeps what it plays: the binary, the routes, the
/// checkpoints it starts from and those it saves, and each route's log.
struct Folder {
    root: PathBuf,
    binary: PathBuf,
    starts: PathBuf,
    saved: PathBuf,
}

impl Folder {
    fn new() -> Result<Folder, String> {
        let root = crate::runtime::create_run_folder()
            .map_err(|error| format!("cannot create the run folder: {error}"))?;
        let exe = std::env::current_exe().map_err(|error| error.to_string())?;
        let binary = root.join(exe.file_name().unwrap_or(std::ffi::OsStr::new("kjk")));
        std::fs::copy(&exe, &binary).map_err(|error| format!("{}: {error}", binary.display()))?;
        // A build with Steam loads Steam's library from its own folder
        // (`launcher/build.rs`): the copy needs it beside it to start.
        let beside = exe.parent().unwrap_or(Path::new("."));
        for library in ["libsteam_api.dylib", "steam_api64.dll", "libsteam_api.so"] {
            let from = beside.join(library);
            if from.is_file() {
                let to = root.join(library);
                std::fs::copy(&from, &to).map_err(|error| format!("{}: {error}", to.display()))?;
            }
        }
        Ok(Folder {
            starts: root.join("starts"),
            saved: root.join("checkpoints"),
            binary,
            root,
        })
    }

    fn log(&self, route: &Route) -> PathBuf {
        self.root.join(format!("{}.log", route.name))
    }

    fn route(&self, route: &Route) -> PathBuf {
        self.root.join("routes").join(format!("{}.route", route.name))
    }

    /// The checkpoints `route` saved in this run (`saved`), each that is.
    fn saved_by<'a>(&'a self, route: &'a Route) -> impl Iterator<Item = (&'a String, PathBuf)> + 'a {
        route
            .saves
            .keys()
            .map(|name| (name, self.saved.join(format!("{name}.kjkv"))))
            .filter(|(_, path)| path.exists())
    }

    /// The checkpoints of a route met, kept for the runs after (each where
    /// `checkpoint` finds it).
    fn promote(&self, game: &str, route: &Route) {
        for (name, path) in self.saved_by(route) {
            if let Err(error) = place(&path, &checkpoint(game, name)) {
                eprintln!("{name}: not kept: {error}");
            }
        }
    }

    /// What the run no longer needs let go: the binary, the starts and the
    /// checkpoints kept (a blocked route's stay, to play on from).
    fn close(&self, plan: &Plan, done: &BTreeMap<usize, Done>) {
        let _ = std::fs::remove_file(&self.binary);
        let _ = std::fs::remove_dir_all(&self.starts);
        for (index, _) in done.iter().filter(|(_, d)| d.ended == Ended::Met) {
            for (_, path) in self.saved_by(&plan.routes[*index]) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Whether, and from what, a route can start now.
enum Ready {
    /// From a module (`None`) or this checkpoint.
    Go(Option<PathBuf>),
    /// Its checkpoint is not saved yet.
    Wait,
    /// It cannot start: how it ended.
    Not(Done),
}

/// Whether route `index` can start: from its module, from the checkpoint
/// the route before it saved in this run (as soon as it is there, unless
/// that route may save it again), or, that route not played here, from the
/// checkpoint kept, as it stood when the run began.
fn ready(plan: &Plan, index: usize, options: &Options, folder: &Folder, done: &BTreeMap<usize, Done>) -> Ready {
    let route = &plan.routes[index];
    let Some(from) = route.starts_from() else {
        return Ready::Go(None);
    };
    let name = format!("{from}.kjkv");
    let Some(parent) = plan.waits_on(index, options) else {
        let path = folder.starts.join(name);
        return if path.exists() {
            Ready::Go(Some(path))
        } else {
            Ready::Not(Done::not_run(
                Ended::Skipped,
                format!(
                    "no checkpoint {}: the route before it never met",
                    checkpoint(&options.game, from).display()
                ),
            ))
        };
    };
    let path = folder.saved.join(name);
    let ended = done.contains_key(&parent);
    if path.exists() && (ended || !plan.routes[parent].saves[from]) {
        Ready::Go(Some(path))
    } else if ended {
        Ready::Not(Done::not_run(
            Ended::Skipped,
            format!("{} did not save {from}", plan.routes[parent].name),
        ))
    } else {
        Ready::Wait
    }
}

/// A route's line in the table.
fn report(route: &Route, done: &Done) {
    println!(
        "{:<26} {:<8} {:>7.1} s  {}{}",
        route.name,
        done.ended.word(),
        done.seconds,
        if done.cached { "(cached) " } else { "" },
        done.summary
    );
}

/// The inputs and cache of a run, or why it has none.
fn cache_of(discovery: &Discovery, options: &Options, binary: &Path) -> Option<(Inputs, Cache)> {
    if !options.cache {
        return None;
    }
    match Inputs::of(discovery, &options.game, binary) {
        Ok(inputs) => {
            let cache = Cache::of(&options.game, &inputs);
            Some((inputs, cache))
        }
        Err(error) => {
            println!("no cache this run: {error}");
            None
        }
    }
}

/// `--dry-run`: each route's command, and its cached outcome where its
/// start is known before the chain plays (a module, or a kept checkpoint).
fn dry_run(discovery: &Discovery, options: &Options, plan: &Plan) -> ExitCode {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let cache = cache_of(discovery, options, &exe);
    for &index in &plan.chosen {
        let route = &plan.routes[index];
        let arguments = &plan.arguments[&index];
        let start = match (route.starts_from(), plan.waits_on(index, options)) {
            (_, Some(parent)) => Err(format!("after {}", plan.routes[parent].name)),
            (None, None) => Ok(Vec::new()),
            (Some(from), None) => std::fs::read(checkpoint(&options.game, from))
                .map_err(|_| format!("no checkpoint {from}")),
        };
        let cached = match (start, &cache) {
            (Err(why), _) => why,
            (Ok(_), None) => "to play".to_string(),
            (Ok(start), Some((inputs, cache))) => cache
                .get(&inputs.key(&start, &route.text, arguments))
                .map_or("to play".to_string(), |cached| {
                    format!("cached {}", cached.done.ended.word())
                }),
        };
        println!("{:<26} {cached:<28} kjk {}", route.name, arguments.join(" "));
    }
    ExitCode::SUCCESS
}

/// The checkpoints `game`'s phases are loaded from, in story order, each
/// with what its save is called (`data/routes/<game>/saves.list`: a
/// checkpoint's name and its title on a line, `#` starting a comment);
/// none without the file.
fn saves_list(game: &str) -> Result<Vec<(String, String)>, String> {
    let path = routes_folder(game).join("saves.list");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim())
        .filter(|line| !line.is_empty())
        .map(|line| match line.split_once(char::is_whitespace) {
            Some((name, title)) => Ok((name.to_string(), title.trim().to_string())),
            None => Err(format!("{}: {line}: a checkpoint and its title", path.display())),
        })
        .collect()
}

/// The kept checkpoints `game`'s saves list names, written into the saves'
/// folder as `k1-01`, `k1-02`, … (`saves::checkpoint_id`), titled by the
/// list and dated a second apart so the Load screen shows them in its
/// order. A number whose checkpoint is not kept keeps the save it had;
/// those past the list's end are removed. The numbers written, and the
/// checkpoints missing.
fn write_saves(game: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let list = saves_list(game)?;
    let root = crate::save_view::folder().ok_or("no folder for saves here")?;
    let now = saves::now();
    let (mut written, mut missing) = (Vec::new(), Vec::new());
    for (index, (name, title)) in list.iter().enumerate() {
        let id = saves::checkpoint_id(game, index + 1);
        let Ok(bytes) = std::fs::read(checkpoint(game, name)) else {
            missing.push(name.clone());
            continue;
        };
        let save = sim::Save::decode(&bytes).map_err(|error| format!("{name}: {error}"))?;
        let meta = saves::Meta {
            name: format!("{} {title}", id.to_ascii_uppercase()),
            module: save
                .snapshot
                .place
                .as_ref()
                .map_or_else(|| save.level.name.clone(), |place| place.module.clone()),
            area: title.clone(),
            saved: now.saturating_sub(index as u64),
            seats: save.snapshot.seats(),
            ..saves::Meta::default()
        };
        saves::write_encoded(&root, Some(&id), &bytes, &meta)?;
        written.push(id);
    }
    // Numbers past the list's end, left by a longer list.
    for number in list.len() + 1.. {
        let id = saves::checkpoint_id(game, number);
        if !root.join(&id).exists() {
            break;
        }
        saves::delete(&root, &id)?;
    }
    Ok((written, missing))
}

/// The saves list written (`write_saves`), said in a line.
fn report_saves(game: &str) {
    match write_saves(game) {
        Ok((written, _)) if written.is_empty() => {}
        Ok((written, missing)) => {
            let folder = crate::save_view::folder()
                .map_or_else(String::new, |folder| folder.display().to_string());
            println!(
                "saves: {} to {}{} ({}; kjk --load {}, or the Load screen)",
                written.len(),
                written.last().map_or("", String::as_str),
                if missing.is_empty() {
                    String::new()
                } else {
                    format!(", not kept: {}", missing.join(", "))
                },
                folder,
                written[0],
            );
        }
        Err(error) => eprintln!("saves: not written: {error}"),
    }
}

/// Whether the working folder is a git worktree's, not the main checkout
/// (`.git` a file naming the repository's).
fn in_worktree() -> bool {
    Path::new(".git").is_file()
}

pub fn run(discovery: &Discovery, options: &Options) -> ExitCode {
    let plan = match Plan::new(options) {
        Ok(plan) => plan,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    if options.dry_run {
        return dry_run(discovery, options, &plan);
    }
    let folder = match Folder::new() {
        Ok(folder) => folder,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::from(2);
        }
    };
    let routes = &plan.routes;
    // The routes as they are now, and the kept checkpoints the routes not
    // waiting on another start from, as they stand now.
    for &index in &plan.chosen {
        let route = &routes[index];
        if let Err(error) = write_whole(&folder.route(route), route.text.as_bytes()) {
            eprintln!("{}: {error}", folder.route(route).display());
            return ExitCode::from(2);
        }
        if let (Some(from), None) = (route.starts_from(), plan.waits_on(index, options)) {
            let kept = checkpoint(&options.game, from);
            if kept.exists()
                && let Err(error) = place(&kept, &folder.starts.join(format!("{from}.kjkv")))
            {
                eprintln!("{}: {error}", kept.display());
            }
        }
    }
    let cache = cache_of(discovery, options, &folder.binary);
    if let Some((_, cache)) = &cache {
        cache.mark_used();
    }
    println!(
        "chain {}: {} routes, {} at once{}{}, run folder {}",
        options.game,
        plan.chosen.len(),
        options.jobs,
        if options.alone { ", each alone" } else { "" },
        if options.fail_fast { "" } else { ", keeping going past blocks" },
        folder.root.display()
    );
    let began = Instant::now();
    let mut waiting: Vec<usize> = plan.chosen.iter().copied().collect();
    let mut running: Vec<Playing> = Vec::new();
    let mut done: BTreeMap<usize, Done> = BTreeMap::new();
    // A route ended: its line, its run kept (met or blocked), its
    // checkpoints kept (met).
    let end = |index: usize, outcome: Done, key: Option<&str>, done: &mut BTreeMap<usize, Done>| {
        let route = &routes[index];
        report(route, &outcome);
        if let (Some((_, cache)), Some(key), false, Ended::Met | Ended::Blocked) =
            (&cache, key, outcome.cached, outcome.ended)
        {
            let saves: Vec<String> = folder.saved_by(route).map(|(name, _)| name.clone()).collect();
            if let Err(error) = cache.put(key, &outcome, &folder.log(route), &folder.saved, &saves) {
                eprintln!("{}: not cached: {error}", route.name);
            }
        }
        if outcome.ended == Ended::Met {
            folder.promote(&options.game, route);
        }
        done.insert(index, outcome);
    };
    // Routes ready and not in the cache, waiting for a job: each with its
    // start and its cache key.
    let mut queued: VecDeque<(usize, Option<PathBuf>, Option<String>)> = VecDeque::new();
    while !waiting.is_empty() || !queued.is_empty() || !running.is_empty() {
        for mut playing in running.extract_if(.., |playing| !matches!(playing.child.try_wait(), Ok(None))) {
            let outcome = playing.outcome(&folder.log(&routes[playing.index]));
            end(playing.index, outcome, playing.key.as_deref(), &mut done);
        }
        let stopped = |done: &BTreeMap<usize, Done>| {
            options.fail_fast
                && done
                    .values()
                    .any(|outcome| matches!(outcome.ended, Ended::Blocked | Ended::Failed))
        };
        if stopped(&done) {
            let why = "the chain stopped at its first block (--keep-going plays on)";
            let left = queued.drain(..).map(|(index, _, _)| index);
            for index in waiting.drain(..).chain(left).collect::<Vec<_>>() {
                end(index, Done::not_run(Ended::Skipped, why.to_string()), None, &mut done);
            }
        }
        let mut next = Vec::new();
        for index in waiting.drain(..) {
            // A block this pass (a cached one): the rest wait to be skipped.
            if stopped(&done) {
                next.push(index);
                continue;
            }
            let start = match ready(&plan, index, options, &folder, &done) {
                Ready::Go(start) => start,
                Ready::Wait => {
                    next.push(index);
                    continue;
                }
                Ready::Not(outcome) => {
                    end(index, outcome, None, &mut done);
                    continue;
                }
            };
            let route = &routes[index];
            let arguments = &plan.arguments[&index];
            let start_bytes = match start.as_deref().map(std::fs::read).transpose() {
                // A module's start: no bytes.
                Ok(bytes) => bytes.unwrap_or(Vec::new()),
                Err(error) => {
                    end(index, Done::not_run(Ended::Failed, error.to_string()), None, &mut done);
                    continue;
                }
            };
            let key = cache
                .as_ref()
                .map(|(inputs, _)| inputs.key(&start_bytes, &route.text, arguments));
            if let (Some((_, cache)), Some(key)) = (&cache, &key)
                && let Some(cached) = cache.get(key)
            {
                if let Err(error) = cache.restore(key, &cached, &folder.log(route), &folder.saved) {
                    eprintln!("{}: the cached run not restored ({error}); played", route.name);
                } else {
                    end(index, cached.done, Some(key), &mut done);
                    continue;
                }
            }
            queued.push_back((index, start, key));
        }
        waiting = next;
        while running.len() < options.jobs.max(1)
            && !stopped(&done)
            && let Some((index, start, key)) = queued.pop_front()
        {
            match spawn(&folder, &routes[index], &plan.arguments[&index], start.as_deref()) {
                Ok(child) => running.push(Playing {
                    index,
                    child,
                    began: Instant::now(),
                    key,
                }),
                Err(error) => end(index, Done::not_run(Ended::Failed, error.to_string()), None, &mut done),
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    folder.close(&plan, &done);
    if options.saves || !in_worktree() {
        report_saves(&options.game);
    }
    let count = |ended: Ended| done.values().filter(|d| d.ended == ended).count();
    let cached = done.values().filter(|d| d.cached).count();
    println!(
        "chain {}: {} routes, {} met, {} blocked, {} failed, {} skipped ({cached} from the cache); {:.1} s",
        options.game,
        done.len(),
        count(Ended::Met),
        count(Ended::Blocked),
        count(Ended::Failed),
        count(Ended::Skipped),
        began.elapsed().as_secs_f64()
    );
    let facts = json!({
        "verb": "chain",
        "game": options.game,
        "git": env!("KJK_GIT_SHA"),
        "binary": cache.as_ref().map(|(inputs, _)| &inputs.binary),
        "data": cache.as_ref().map(|(inputs, _)| &inputs.data),
        "seconds": began.elapsed().as_secs_f64(),
        "routes": done.iter().map(|(index, d)| json!({
            "route": routes[*index].name,
            "start": routes[*index].start,
            "command": plan.arguments[index].join(" "),
            "ended": d.ended.word(),
            "cached": d.cached,
            "seconds": d.seconds,
            "summary": d.summary,
        })).collect::<Vec<_>>(),
    });
    let path = folder.root.join("facts.json");
    if let Err(error) = std::fs::write(&path, serde_json::to_string_pretty(&facts).expect("json values serialize")) {
        eprintln!("{}: {error}", path.display());
    }
    if count(Ended::Met) == done.len() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A route playing: its child, when it began, its cache key.
struct Playing {
    index: usize,
    child: Child,
    began: Instant,
    key: Option<String>,
}

impl Playing {
    /// How it ended (its child has exited): by the playthrough's exit code
    /// and the summary in its `log`.
    fn outcome(&mut self, log: &Path) -> Done {
        let (ended, summary) = match self.child.wait().map(|status| status.code()) {
            Ok(Some(0)) => (Ended::Met, summary(log)),
            Ok(Some(1)) => (Ended::Blocked, summary(log)),
            Ok(code) => (Ended::Failed, format!("exit {code:?}; see {}", log.display())),
            Err(error) => (Ended::Failed, error.to_string()),
        };
        Done {
            ended,
            seconds: self.began.elapsed().as_secs_f64(),
            summary,
            cached: false,
        }
    }
}

/// A route's playthrough from the run's binary, its route file and `start`
/// (its checkpoint, unless it starts from a module), saving into the run's
/// checkpoints, its output into its log.
fn spawn(folder: &Folder, route: &Route, arguments: &[String], start: Option<&Path>) -> std::io::Result<Child> {
    let out = std::fs::File::create(folder.log(route))?;
    let mut arguments = arguments.to_vec();
    if let Some(start) = start {
        arguments[2] = format!("@{}", start.display());
    }
    arguments[3] = folder.route(route).display().to_string();
    Command::new(&folder.binary)
        .args(arguments)
        .arg("--save-to")
        .arg(&folder.saved)
        .stdin(Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out)
        .spawn()
}
