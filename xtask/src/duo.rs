//! `cargo xtask duo`: a host and a player as two processes on this
//! machine, over 127.0.0.1, both headless, each with its own `KJK_NETEM`,
//! and the evidence of the session they played:
//! - every snapshot the player rebuilt hashes as the host stepped it (the
//!   host's `hashes.txt` against the player's `rebuilt.txt`);
//! - the host's recording (`inputs.kjkr`, what it applied) replays to its
//!   end (`kjk sim replay`);
//! - the modules installed on the way (epochs) and that the player
//!   committed each one to the announced state (its dump's net block);
//! - both sides draw every player, at the same places once all stand still
//!   (their dumps' rosters), with a screenshot from each;
//! - the net feel of both sides (`kjk net feel`) in `net-feel.md`. With
//!   `KJK_PERF=1` each run's `trace.json` has the counters too;
//! - the players each install was planned for, as each side planned it
//!   (the host's `planning` lines, the player's `the host installs`; I30
//!   P21).
//!
//! - what each seat took in the fight (the replay's hits, blocks and the
//!   blows held for its grace, I25);
//! - the host's saves the player kept a copy of, byte for byte (I30 P17):
//!   each side keeps its saves in its own folder beside the runs
//!   (`host-saves`, `player-saves`; `KJK_SAVES`), the host's starting as a
//!   copy of `--host-saves` when given;
//! - the host's reloads after the party fell (I30 P13), whose installs the
//!   player takes as a keyframe.
//!
//! By default the host stands in `end_m01aa` at `wp_player01` for 12 s (the
//! player joins meanwhile), then runs through the door to `end_m01ab`
//! holding use and back; the player stands. On `--stage duel` the two are
//! Jedi facing off on a floor (the host's seat looking along +X, the
//! player's back at it) and fight: the host after 8 s, when the player has
//! joined, each to its own rhythm of swings each way, rests and a style
//! change. The results go to `kjk-artifacts/runs/<time>-duo/duo.md`,
//! beside the two runs' folders.
//!
//! `--preset exit` is the ready check (I30 P16): two players in `end_m01ab`
//! at `from_01a`; after Carth's call the host steps into the door back to
//! `end_m01aa` and stands there, the player says it is ready (`ui ready`,
//! a few times), and the run passes when the check went and the player
//! committed the install. Options given beside it win.
//!
//! `--preset race` and `--preset turret` are a minigame in co-op
//! (`docs/MINIGAMES.md`): Taris's swoop race cold, the
//! host's run played by the race bot (`--aim race`) and the player's by
//! presses and strafes; the Ebon Hawk's turret, the host's gun swept
//! without firing and the player's swept back and forth firing. Each side dumps
//! mid-minigame (`host-mid`, `player-mid`), and the run passes when each
//! drew its own seat's run (the race, the other's swoop a ghost) or its own
//! gun on the one hull (the turret).
//!
//! ```text
//! cargo xtask duo [--stage end_m01aa|duel|course] [--arrive wp_player01|entry]
//!                 [--seconds 26] [--netem 'delay=50ms,jitter=5ms,loss=2%,burst=3']
//!                 [--moves-host SCRIPT] [--moves-join SCRIPT] [--port 29171]
//!                 [--binary PATH]   # another kjk than this tree's, unbuilt
//!                 [--plan-delay S]  # the player plans each module S s late
//!                 [--connect ::1]   # the address the player dials (127.0.0.1)
//!                 [--class soldier|jedi]
//!                 [--guests avatars|companions]  # what the player plays
//!                 [--follow TAG]    # the host's party from the start (`kjk host --follow`)
//!                 [--sim-blades on]  # both draw the sim's blades (`kjk play`)
//!                 [--cmds-host CMDS] [--cmds-join CMDS]  # in place of the
//!                                    # shots and dumps; they must still
//!                                    # `dump host` and `dump player`
//!                 [--host-args 'ARGS']  # more for `kjk host`, e.g.
//!                                    # '--follow end_trask --xp 6000'
//!                 [--film S]        # each side films S s from its shot on
//!                 [--camera 'X Y Z YAW PITCH']  # both sides' camera, pinned
//!                 [--host-saves DIR]  # the host's saves start as these
//!                                    # (e.g. with --host-args '--load 0001')
//!                 [--preset exit|race|turret]   # (above)
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::repo_root;

/// The host's walk on `end_m01aa`: standing while the player joins, then
/// through the door to `end_m01ab` holding use, and back.
const DOOR_WALK: &str =
    "12000ms; 500ms yaw 0; 2500ms w use; 2000ms; 700ms turn 180; 2500ms w use; 1000ms";

/// The duel's rounds: swings, pressing in or standing, with rests between
/// (a rest blocks by state) and a style change; each seat keeps its yaw,
/// so neither strafes out of the other's reach.
const DUEL_ROUND: &str = "600ms attack w; 700ms; 600ms attack; 500ms; 600ms attack w; 900ms; \
     300ms style; 400ms; 600ms attack; 800ms";

/// The host's duel (seat 0, looking along +X): waiting for the player, the
/// saber lit, six rounds.
fn duel_host() -> String {
    format!(
        "8000ms yaw 0; 200ms attack; 1000ms; {}",
        [DUEL_ROUND; 6].join("; ")
    )
}

/// The player's duel (seat 1, facing back along −X): lit at once, then
/// rounds offset from the host's.
fn duel_join() -> String {
    format!(
        "500ms yaw 180; 200ms attack; 1300ms; {}",
        [DUEL_ROUND; 6].join("; ")
    )
}

struct Options {
    stage: String,
    arrive: Option<String>,
    seconds: u64,
    netem: String,
    moves_host: String,
    moves_join: String,
    port: u16,
    binary: Option<PathBuf>,
    plan_delay: Option<String>,
    connect: String,
    /// The players' sheet (`kjk host --class`).
    class: String,
    sim_blades: bool,
    cmds_host: Option<String>,
    cmds_join: Option<String>,
    /// Extra arguments for `kjk host`, as one string.
    host_args: Vec<String>,
    /// Seconds each side films (`film`) after its screenshot.
    film: u64,
    /// Both sides' camera, pinned (`camera`), if given.
    camera: Option<String>,
    /// What the player plays (`kjk host --guests`): `avatars` (a player of
    /// its own, the default) or `companions` (the host's party member).
    guests: String,
    /// A creature of the module in the host's party from the start.
    follow: Option<String>,
    /// Saves the host's folder starts with.
    host_saves: Option<PathBuf>,
    /// `--preset exit`: the verdict also wants the ready check gone through.
    exit: bool,
    /// `--preset remote`: the verdict also wants the guest's droid drawn on
    /// both sides.
    remote: bool,
    /// `--preset race` or `turret`: the verdict also wants each side's own
    /// run or gun (the minigame's co-op).
    minigame: Option<&'static str>,
}

/// `--preset exit`'s options (see the module documentation).
const EXIT_PRESET: [(&str, &str); 7] = [
    ("stage", "end_m01ab"),
    ("arrive", "from_01a"),
    ("guests", "avatars"),
    ("seconds", "40"),
    ("moves-host", "16s; 300ms yaw 180; 300ms w; 40s"),
    ("moves-join", "60s"),
    (
        "cmds-join",
        "wait 8s; screenshot player-8s; wait 4s; ui ready; wait 3s; ui ready; wait 3s; \
         ui ready; wait 3s; ui ready; wait 3s; screenshot player-ready; wait 10s; dump player; \
         wait 8s; quit",
    ),
];

/// `--preset remote`'s options: end_m01ab's start (as `--preset exit`),
/// Trask not brought along: past Carth's call over the comm, the guest with no companion to drive flying a remote droid: up,
/// ahead, turning, each button once (the dart has no one hurt to reach).
const REMOTE_PRESET: [(&str, &str); 8] = [
    ("stage", "end_m01ab"),
    ("arrive", "from_01a"),
    ("guests", "companions"),
    ("seconds", "40"),
    ("moves-host", "60s"),
    (
        "moves-join",
        "16s yaw 90; 300ms jump yaw 90; 1200ms w yaw 90; 300ms attack yaw 90; 1s yaw 90; 300ms alt yaw 90; 1s yaw 90; 300ms medpac yaw 90; 30s yaw 90",
    ),
    (
        "cmds-host",
        "wait 11s; screenshot host-11s; wait 10s; screenshot host-remote; dump host-remote; \
         wait 23s; dump host; quit",
    ),
    (
        "cmds-join",
        "wait 8s; screenshot player-8s; wait 9s; screenshot player-remote; wait 1300ms; \
         screenshot player-kit; wait 1500ms; screenshot player-blast; dump player-remote; \
         wait 10s; dump player; wait 8s; quit",
    ),
];

/// The player's race: past the countdown, fire pressed every half second
/// (the gear shifts as soon as it may), drifting right and left, for some
/// 50 s; then still, so both sides see it stand where it is in the pits.
fn race_join() -> String {
    format!(
        "5300ms; {}; 60s",
        ["150ms attack d; 350ms d; 150ms attack a; 350ms a"; 50].join("; ")
    )
}

/// The player's gun: swept a half turn each way, firing, again and again.
fn turret_join() -> String {
    format!(
        "2s; {}",
        ["3s attack turn 90; 6s attack turn -180; 3s attack turn 90"; 8].join("; ")
    )
}

/// What each side runs in a minigame preset: a dump and a shot mid-way,
/// a film, and the usual end dump.
fn minigame_cmds(side: &str, mid: u64, seconds: u64) -> String {
    format!(
        "wait {mid}s; screenshot {side}-mid; dump {side}-mid; film 6s {side}; wait {}s; dump {side}; {}quit",
        seconds.saturating_sub(mid + 6),
        if side == "player" { "wait 8s; " } else { "" }
    )
}

/// `--preset race` and `turret`'s options (see the module documentation).
fn minigame_preset(name: &str) -> Vec<(&'static str, String)> {
    // Each side's moves, how long, and when each dumps (s). The turret's
    // host sweeps its gun without firing: its bot would down the fighters
    // in some 5 s, before the player is there.
    let (stage, host_args, (host_moves, join), seconds, mid) = match name {
        "race" => (
            "tar_m03mg",
            "--aim race --global TAR_SWOOP_ACCEL=7",
            ("120s".to_string(), race_join()),
            70,
            (20, 17),
        ),
        _ => (
            "m12ab",
            "--global K_KOTOR_MASTER=10",
            (
                format!("1s; {}", ["8s turn 40; 8s turn -40"; 8].join("; ")),
                turret_join(),
            ),
            40,
            (16, 13),
        ),
    };
    vec![
        ("stage", stage.to_string()),
        ("arrive", "entry".to_string()),
        ("guests", "avatars".to_string()),
        ("seconds", seconds.to_string()),
        ("host-args", host_args.to_string()),
        ("moves-host", host_moves),
        ("moves-join", join),
        ("cmds-host", minigame_cmds("host", mid.0, seconds)),
        ("cmds-join", minigame_cmds("player", mid.1, seconds)),
    ]
}

fn options(args: &[String]) -> Result<Options, String> {
    let mut named: BTreeMap<&str, String> = BTreeMap::new();
    let mut at = 0;
    while at < args.len() {
        let key = args[at]
            .strip_prefix("--")
            .ok_or(format!("{}: expected --option value", args[at]))?;
        let value = args.get(at + 1).ok_or(format!("--{key}: needs a value"))?;
        named.insert(key, value.clone());
        at += 2;
    }
    let preset = named.remove("preset");
    let remote = preset.as_deref() == Some("remote");
    let (exit, minigame) = match preset.as_deref() {
        None | Some("remote") => (false, None),
        Some("exit") => (true, None),
        Some("race") => (false, Some("race")),
        Some("turret") => (false, Some("turret")),
        Some(other) => return Err(format!("--preset {other}: exit, race, turret or remote")),
    };
    for (key, value) in (exit.then_some(EXIT_PRESET.as_slice()))
        .or(remote.then_some(REMOTE_PRESET.as_slice()))
        .unwrap_or(&[])
    {
        named.entry(key).or_insert_with(|| value.to_string());
    }
    for (key, value) in minigame.map_or_else(Vec::new, minigame_preset) {
        named.entry(key).or_insert(value);
    }
    let stage = named
        .remove("stage")
        .unwrap_or_else(|| "end_m01aa".to_string());
    let module = !matches!(stage.as_str(), "course" | "duel");
    let duel = stage == "duel";
    let options = Options {
        arrive: match named.remove("arrive").as_deref() {
            // The module's own entry (the bunk room on end_m01aa).
            Some("entry") => None,
            Some(tag) => Some(tag.to_string()),
            None => module.then(|| "wp_player01".to_string()),
        },
        follow: named.remove("follow"),
        guests: match named.remove("guests").as_deref() {
            None | Some("avatars") => "avatars".to_string(),
            Some("companions") => "companions".to_string(),
            Some(other) => return Err(format!("--guests {other}: avatars or companions")),
        },
        seconds: named
            .remove("seconds")
            .map_or(Ok(26), |s| s.parse().map_err(|_| "--seconds: a number"))?,
        netem: named
            .remove("netem")
            .unwrap_or_else(|| "delay=50ms,jitter=5ms,loss=2%,burst=3".to_string()),
        moves_host: named.remove("moves-host").unwrap_or_else(|| {
            if duel {
                duel_host()
            } else if module {
                DOOR_WALK.to_string()
            } else {
                "course".to_string()
            }
        }),
        moves_join: named.remove("moves-join").unwrap_or_else(|| {
            if duel {
                duel_join()
            } else {
                "60000ms".to_string()
            }
        }),
        port: named
            .remove("port")
            .map_or(Ok(29171), |s| s.parse().map_err(|_| "--port: a number"))?,
        binary: named.remove("binary").map(PathBuf::from),
        plan_delay: named.remove("plan-delay"),
        connect: named
            .remove("connect")
            .unwrap_or_else(|| "127.0.0.1".to_string()),
        class: named
            .remove("class")
            .unwrap_or_else(|| if duel { "jedi" } else { "soldier" }.to_string()),
        sim_blades: match named.remove("sim-blades").as_deref() {
            None | Some("off") => false,
            Some("on") => true,
            Some(other) => return Err(format!("--sim-blades {other}: on or off")),
        },
        cmds_host: named.remove("cmds-host"),
        cmds_join: named.remove("cmds-join"),
        host_args: named.remove("host-args").map_or_else(Vec::new, |args| {
            args.split_whitespace().map(str::to_string).collect()
        }),
        film: named
            .remove("film")
            .map_or(Ok(0), |s| s.parse().map_err(|_| "--film: seconds"))?,
        camera: named.remove("camera"),
        host_saves: named.remove("host-saves").map(PathBuf::from),
        exit,
        remote,
        minigame,
        stage,
    };
    if let Some(key) = named.keys().next() {
        return Err(format!("--{key}: not a duo option"));
    }
    Ok(options)
}

/// The launcher, built in release (where cargo puts it).
pub(crate) fn build() -> Result<PathBuf, String> {
    let status = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .current_dir(repo_root())
        .args(["build", "--release", "-p", "launcher"])
        .status()
        .map_err(|error| format!("cargo build: {error}"))?;
    if !status.success() {
        return Err("cargo build failed".into());
    }
    let target = std::env::var("CARGO_TARGET_DIR")
        .map_or_else(|_| repo_root().join("target"), PathBuf::from);
    let binary = target
        .join("release")
        .join(if cfg!(windows) { "kjk.exe" } else { "kjk" });
    binary
        .exists()
        .then_some(binary.clone())
        .ok_or(format!("{}: not built", binary.display()))
}

/// A process run in `cwd` with `env` added, its output in
/// `folder/<name>.log`.
pub(crate) fn spawn(
    binary: &Path,
    args: &[String],
    env: &[(&str, &str)],
    cwd: &Path,
    folder: &Path,
    name: &str,
) -> Result<Child, String> {
    let log = std::fs::File::create(folder.join(format!("{name}.log")))
        .map_err(|error| format!("{name}.log: {error}"))?;
    let errors = log
        .try_clone()
        .map_err(|error| format!("{name}.log: {error}"))?;
    Command::new(binary)
        .current_dir(cwd)
        .args(args)
        .envs(env.iter().copied())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(errors))
        .spawn()
        .map_err(|error| format!("{name}: {error}"))
}

/// Waits until `folder/<name>.log` has a whole line containing `text`;
/// that line, without its colours. A line still being written is not
/// read: its end may not be there yet.
pub(crate) fn wait_for(
    folder: &Path,
    name: &str,
    text: &str,
    limit: Duration,
) -> Result<String, String> {
    let started = Instant::now();
    loop {
        let log =
            std::fs::read_to_string(folder.join(format!("{name}.log"))).unwrap_or(String::new());
        let written = &log[..log.rfind('\n').map_or(0, |end| end + 1)];
        if let Some(line) = written.lines().map(plain).find(|line| line.contains(text)) {
            return Ok(line);
        }
        if started.elapsed() > limit {
            return Err(format!("{name}: no `{text}` in {} s", limit.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// The run folder a process's log names.
fn run_folder(folder: &Path, name: &str) -> Result<PathBuf, String> {
    let line = wait_for(folder, name, "run folder:", Duration::from_secs(60))?;
    let path = line
        .split("run folder:")
        .nth(1)
        .map(str::trim)
        .ok_or(format!("{name}: no run folder"))?;
    Ok(repo_root().join(path))
}

/// `from`'s files and folders copied into `to`.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    let entries =
        std::fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))?;
    for entry in entries.flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|error| format!("{}: {error}", target.display()))?;
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)
                .map_err(|error| format!("{}: {error}", target.display()))?;
        }
    }
    Ok(())
}

/// The worlds (`save.kjkv`) of the saves in `root`, by id.
fn worlds(root: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let world = std::fs::read(entry.path().join("save.kjkv")).ok()?;
            Some((entry.file_name().to_str()?.to_string(), world))
        })
        .collect()
}

/// The player's copies (`r…`) whose world is byte for byte the host's save
/// they name (`"slot"` in their meta), as (copy, slot); and how many saves
/// the host wrote in the run (new or changed since `before`).
fn copies(
    host: &Path,
    before: &BTreeMap<String, Vec<u8>>,
    player: &Path,
) -> (Vec<(String, String)>, usize) {
    let hosts = worlds(host);
    let written = hosts
        .iter()
        .filter(|(id, world)| before.get(*id) != Some(world))
        .count();
    let kept = worlds(player)
        .into_iter()
        .filter(|(id, _)| id.starts_with('r'))
        .filter_map(|(id, world)| {
            let meta = std::fs::read_to_string(player.join(&id).join("meta.json")).ok()?;
            let meta: serde_json::Value = serde_json::from_str(&meta).ok()?;
            let slot = meta.get("received")?.get("slot")?.as_str()?.to_string();
            (hosts.get(&slot) == Some(&world)).then_some((id, slot))
        })
        .collect();
    (kept, written)
}

/// Lines of "epoch tick hash".
fn hashes(path: &Path) -> Result<Vec<(String, String, String)>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(text
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some((
                words.next()?.to_string(),
                words.next()?.to_string(),
                words.next()?.to_string(),
            ))
        })
        .collect())
}

/// The players a dump's roster draws: each seat's soles (metres), from
/// the rows' `"soles_m": [x, y, z]` before their `"who": "player N"`.
fn players(json: &str) -> BTreeMap<u8, [f64; 3]> {
    const WHO: &str = "\"who\": \"player ";
    const SOLES: &str = "\"soles_m\": [";
    let mut found = BTreeMap::new();
    let mut from = 0;
    while let Some(at) = json[from..].find(WHO) {
        let at = from + at;
        from = at + 1;
        let seat: String = json[at + WHO.len()..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        let Some(soles) = json[..at].rfind(SOLES) else {
            continue;
        };
        let numbers: Vec<f64> = json[soles + SOLES.len()..at]
            .split(']')
            .next()
            .unwrap_or("")
            .split(',')
            .filter_map(|n| n.trim().parse().ok())
            .collect();
        if let (Ok(seat), [x, y, z]) = (seat.parse(), numbers.as_slice()) {
            found.insert(seat, [*x, *y, *z]);
        }
    }
    found
}

/// The roster's players with a character of their own (`"who": "player N"`,
/// not a driven companion's `"player N (tag)"`).
fn own_players(json: &str) -> usize {
    json.split("\"who\": \"player ")
        .skip(1)
        .filter(|rest| rest.split('"').next().is_some_and(|who| !who.contains('(')))
        .count()
}

/// A number the dump's JSON has after `"key": ` (first occurrence).
fn number_after(json: &str, key: &str) -> Option<f64> {
    let at = json.find(&format!("\"{key}\": "))? + key.len() + 4;
    let digits: String = json[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse().ok()
}

/// The lines of `log` that hold `marker`, each from the marker on.
fn logged(log: &str, marker: &str) -> Vec<String> {
    log.lines()
        .map(plain)
        .filter_map(|line| Some(format!("{marker}{}", line.split_once(marker)?.1)))
        .collect()
}

/// `lines` as a Markdown list under a report line.
fn listed(lines: &[String]) -> String {
    lines.iter().map(|line| format!("\n  - `{line}`")).collect()
}

/// `line` without its terminal colours (`ESC [ … m`).
pub(crate) fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            chars.by_ref().find(|&c| c == 'm');
        } else {
            out.push(c);
        }
    }
    out
}

/// The `film` command for `seconds` (none for 0), named `name`.
fn filming(seconds: u64, name: &str) -> String {
    if seconds == 0 {
        String::new()
    } else {
        format!("film {seconds}s {name}; ")
    }
}

/// The `camera` command pinning the view at `pose`, if given.
fn pinned(pose: Option<&str>) -> String {
    pose.map_or(String::new(), |pose| format!("wait world; camera {pose}; "))
}

/// The round trip `netem` makes (its `delay=` both ways), in ms.
fn path_rtt_ms(netem: &str) -> Option<f64> {
    let delay = netem
        .split(',')
        .find_map(|part| part.trim().strip_prefix("delay="))?;
    let ms: f64 = delay.strip_suffix("ms")?.parse().ok()?;
    Some(2.0 * ms)
}

pub fn run(args: &[String]) -> Result<(), String> {
    let options = options(args)?;
    let binary = match &options.binary {
        Some(binary) => binary.clone(),
        None => build()?,
    };
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let folder = repo_root()
        .join("kjk-artifacts")
        .join("runs")
        .join(format!("{stamp}-duo"));
    std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
    println!("duo: {}", folder.display());
    let (host_saves, player_saves) = (folder.join("host-saves"), folder.join("player-saves"));
    for saves in [&host_saves, &player_saves] {
        std::fs::create_dir_all(saves).map_err(|error| format!("{}: {error}", saves.display()))?;
    }
    if let Some(from) = &options.host_saves {
        copy_tree(from, &host_saves)?;
    }
    let saves_before = worlds(&host_saves);
    let saves_env = |saves: &Path| saves.to_string_lossy().into_owned();
    let (host_saves_env, player_saves_env) = (saves_env(&host_saves), saves_env(&player_saves));

    let mut host_args: Vec<String> = ["host", "--stage", &options.stage, "--port"]
        .iter()
        .map(ToString::to_string)
        .collect();
    host_args.push(options.port.to_string());
    host_args.extend(["--class".to_string(), options.class.clone()]);
    // Two players, as the duo has always been, each playing as told.
    host_args
        .extend(["--seats", "2", "--guests", options.guests.as_str()].map(ToString::to_string));
    if let Some(arrive) = &options.arrive {
        host_args.extend(["--arrive".to_string(), arrive.clone()]);
    }
    if let Some(tag) = &options.follow {
        host_args.extend(["--follow".to_string(), tag.clone()]);
    }
    host_args.extend([
        "--moves".to_string(),
        options.moves_host.clone(),
        "--headless".to_string(),
        "--cmds".to_string(),
        options.cmds_host.clone().unwrap_or_else(|| {
            format!(
                "{}wait 11s; screenshot host-11s; {}wait {}s; dump host; quit",
                pinned(options.camera.as_deref()),
                filming(options.film, "host"),
                options.seconds.saturating_sub(5 + options.film)
            )
        }),
    ]);
    if options.sim_blades {
        host_args.push("--sim-blades".to_string());
    }
    host_args.extend(options.host_args.iter().cloned());
    let mut host = spawn(
        &binary,
        &host_args,
        &[
            ("KJK_NETEM", &format!("{},seed=1", options.netem)),
            ("KJK_SAVES", &host_saves_env),
        ],
        &repo_root(),
        &folder,
        "host",
    )?;
    // The host says it listens once its stage is loaded.
    let listening = wait_for(&folder, "host", "listening on", Duration::from_secs(120));
    let hosted = listening.and_then(|_| {
        let mut join_args = vec![
            "join".to_string(),
            match options.connect.parse::<std::net::IpAddr>() {
                Ok(std::net::IpAddr::V6(ip)) => format!("[{ip}]:{}", options.port),
                _ => format!("{}:{}", options.connect, options.port),
            },
            "--moves".to_string(),
            options.moves_join.clone(),
            "--headless".to_string(),
            "--cmds".to_string(),
            options.cmds_join.clone().unwrap_or_else(|| {
                // It stays past the host's dump: a guest gone is a body
                // the host no longer draws as its seat's.
                format!(
                    "{}wait 8s; screenshot player-8s; {}wait {}s; dump player; wait 8s; quit",
                    pinned(options.camera.as_deref()),
                    filming(options.film, "player"),
                    options.seconds.saturating_sub(8 + options.film)
                )
            }),
        ];
        if options.sim_blades {
            join_args.push("--sim-blades".to_string());
        }
        if let Some(delay) = &options.plan_delay {
            join_args.extend(["--plan-delay".to_string(), delay.clone()]);
        }
        spawn(
            &binary,
            &join_args,
            &[
                ("KJK_NETEM", &format!("{},seed=2", options.netem)),
                ("KJK_SAVES", &player_saves_env),
            ],
            &repo_root(),
            &folder,
            "player",
        )
    });
    let mut player = match hosted {
        Ok(player) => player,
        Err(error) => {
            let _ = host.kill();
            return Err(error);
        }
    };
    let player_status = player.wait().map_err(|error| error.to_string())?;
    let host_status = host.wait().map_err(|error| error.to_string())?;
    let host_run = run_folder(&folder, "host")?;
    let player_run = run_folder(&folder, "player")?;

    let mut report = String::new();
    let mut failed = Vec::new();
    let _ = writeln!(report, "# duo {stamp}\n");
    let _ = writeln!(
        report,
        "stage {} {}, {} s, netem `{}` both ways (seeds 1 and 2), the player dialing {}\n\nhost: `{}` ({})\nplayer: `{}` ({})\n",
        options.stage,
        options.arrive.as_deref().unwrap_or(""),
        options.seconds,
        options.netem,
        options.connect,
        host_run.display(),
        host_status,
        player_run.display(),
        player_status
    );
    if !host_status.success() || !player_status.success() {
        failed.push("a process failed".to_string());
    }

    // Rebuilt against stepped.
    let stepped = hashes(&host_run.join("hashes.txt"))?;
    let rebuilt = hashes(&player_run.join("rebuilt.txt"))?;
    let known: BTreeMap<(String, String), BTreeSet<String>> =
        stepped
            .iter()
            .fold(BTreeMap::new(), |mut known, (e, t, h)| {
                known
                    .entry((e.clone(), t.clone()))
                    .or_default()
                    .insert(h.clone());
                known
            });
    let wrong = rebuilt
        .iter()
        .filter(|(e, t, h)| {
            known
                .get(&(e.clone(), t.clone()))
                .is_none_or(|hashes| !hashes.contains(h))
        })
        .count();
    let epochs: BTreeSet<&String> = stepped.iter().map(|(e, _, _)| e).collect();
    let player_epochs: BTreeSet<&String> = rebuilt.iter().map(|(e, _, _)| e).collect();
    let _ = writeln!(
        report,
        "- rebuilt: {} snapshots on the player, {wrong} unlike the host's {} stepped ({} epochs on the host, {} seen by the player)",
        rebuilt.len(),
        stepped.len(),
        epochs.len(),
        player_epochs.len()
    );
    if rebuilt.is_empty() || wrong > 0 {
        failed.push("rebuilt snapshots".to_string());
    }

    // The host's recording.
    let replay = Command::new(&binary)
        .current_dir(repo_root())
        .args(["sim", "replay"])
        .arg(host_run.join("inputs.kjkr"))
        .output()
        .map_err(|error| format!("kjk sim replay: {error}"))?;
    let replay_text = String::from_utf8_lossy(&replay.stdout).to_string()
        + &String::from_utf8_lossy(&replay.stderr);
    let replayed = replay.status.success() && replay_text.contains("as recorded");
    let _ = writeln!(
        report,
        "- the host's recording replays to its end: {replayed} ({})",
        replay_text
            .lines()
            .find(|line| line.contains("ends at") || line.contains("replay"))
            .unwrap_or("")
            .trim()
    );
    if !replayed {
        failed.push("replay".to_string());
    }
    for line in replay_text
        .lines()
        .filter(|line| line.trim_start().starts_with("seat "))
    {
        let _ = writeln!(report, "  - {}", line.trim());
    }

    // Installs.
    let player_dump =
        std::fs::read_to_string(player_run.join("player.json")).unwrap_or(String::new());
    let agreed = number_after(&player_dump, "installs_agreed").unwrap_or(0.0) as u64;
    let differed = number_after(&player_dump, "installs_differed").unwrap_or(0.0) as u64;
    let installs = epochs.len().saturating_sub(1) as u64;
    // A reload's install the player cannot make itself: it takes the
    // host's world whole (a keyframe).
    let host_log = std::fs::read_to_string(folder.join("host.log")).unwrap_or(String::new());
    let host_lines: Vec<String> = host_log.lines().map(plain).collect();
    let count = |text: &str| host_lines.iter().filter(|line| line.contains(text)).count();
    let reloads = count("saves: reloaded") as u64;
    let _ = writeln!(
        report,
        "- installs: {installs} on the host, {reloads} of them reloads; the player committed {agreed} to the announced state, {differed} differed (a reload's does)"
    );
    if agreed + reloads != installs || differed != reloads {
        failed.push("installs".to_string());
    }
    // The host's saves kept on the player (I30 P17): every one sent while
    // the player was there.
    let (kept, written) = copies(&host_saves, &saves_before, &player_saves);
    let sent = count("saves: copy sent");
    let _ = writeln!(
        report,
        "- save copies: the host wrote {written} save(s) and sent {sent} while the player was there; the player kept {} byte for byte{}",
        kept.len(),
        kept.iter()
            .map(|(copy, slot)| format!("; {copy} from {slot}"))
            .collect::<String>()
    );
    if kept.len() < sent {
        failed.push("save copies".to_string());
    }
    // The installs' counts (I30 P21), as each side planned them.
    for (side, text) in [("host", "planning"), ("player", "the host installs")] {
        let log =
            std::fs::read_to_string(folder.join(format!("{side}.log"))).unwrap_or(String::new());
        for line in log.lines().filter(|line| line.contains(text)) {
            let _ = writeln!(report, "- planned ({side}): {}", plain(line));
        }
    }
    // Every player drawn on both sides, standing where the other side has
    // it once all stand still.
    let host_dump = std::fs::read_to_string(host_run.join("host.json")).unwrap_or(String::new());
    let (on_host, on_player) = (players(&host_dump), players(&player_dump));
    let apart = on_host
        .iter()
        .filter_map(|(seat, a)| {
            let b = on_player.get(seat)?;
            Some((0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f64>().sqrt())
        })
        .fold(0.0, f64::max);
    let seats = |drawn: &BTreeMap<u8, [f64; 3]>| {
        drawn
            .iter()
            .map(|(seat, [x, y, z])| format!("player {seat} at ({x:.2}, {y:.2}, {z:.2})"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let _ = writeln!(
        report,
        "- players drawn on the host: {}; on the player: {}; at most {apart:.3} m apart (shots `host-11s.png`, `player-8s.png`)",
        seats(&on_host),
        seats(&on_player)
    );
    // A guest flying a droid has no body: only the host is drawn.
    let bodies = if options.remote { 1 } else { 2 };
    if on_host.len() < bodies || on_host.keys().ne(on_player.keys()) || apart > 0.05 {
        failed.push("players drawn".to_string());
    }
    // A guest playing the host's party member is drawn as its seat's player
    // on the host (`cast::Cast::roster`).
    if options.guests == "companions" && !options.remote {
        let driven = host_dump.contains("\"who\": \"player 1 (");
        let _ = writeln!(
            report,
            "- seat 1 drives a party member on the host: {driven}"
        );
        if !driven {
            failed.push("seat 1's body".to_string());
        }
    }
    // A seat's own figure only for a seat with a character of its own: a
    // guest who drives a companion or watches draws none (a stray at the
    // spawn otherwise). A dump with no roster (the party lost, nobody to
    // follow) has nothing to weigh it by.
    for (side, dump) in [("host", &host_dump), ("player", &player_dump)] {
        if dump.contains("\"roster\": null") {
            continue;
        }
        let shown = number_after(dump, "seat_bodies_shown").unwrap_or(-1.0);
        let own = own_players(dump) as f64;
        let _ = writeln!(
            report,
            "- seat figures shown on the {side}: {shown}, for {own} players of their own"
        );
        if shown != own {
            failed.push(format!("seat figures on the {side}"));
        }
    }
    // The fight as each side drew it: blows and shots that landed, and
    // those turned (blocks, clashes, deflections), from the hits' events.
    let drew = |dump: &str| {
        ["blade_hits_seen", "blade_blocks_seen", "bolt_blocks_seen"]
            .map(|key| number_after(dump, key).map_or("-".to_string(), |n| n.to_string()))
            .join(" / ")
    };
    let _ = writeln!(
        report,
        "- blade hits / blade blocks / bolts turned drawn: the host {}, the player {}",
        drew(&host_dump),
        drew(&player_dump)
    );
    // The level cache, and the load barrier: the host resuming without a
    // player still loading (`--plan-delay`).
    let cached = host_dump.matches("\"area_cached\": true").count();
    let built = host_dump.matches("\"area_cached\": false").count();
    let late = logged(&host_log, "the world resumes");
    let _ = writeln!(
        report,
        "- the host's installs: {cached} took their area from the level cache, {built} built it; resumed without a loading player {} time(s){}",
        late.len(),
        listed(&late)
    );
    // The party's calls on the host, as its log has them (`party_calls`).
    let calls = logged(&host_log, "calls: ");
    let _ = writeln!(
        report,
        "- the party's calls on the host: {}{}",
        calls.len(),
        listed(&calls)
    );
    // The exit preset: the ready check went, and the player took the
    // install it led to.
    if options.exit {
        let went = calls
            .iter()
            .any(|call| call.contains("ready check to") && call.contains(" went "));
        let _ = writeln!(
            report,
            "- the ready check went: {went}; the player committed {agreed} install(s)"
        );
        if !went || agreed < 1 {
            failed.push("ready check".to_string());
        }
    }
    // The remote preset: both sides drew the guest's droid, with its model,
    // and its shock bolt and Sonic Blast went off (their waits set).
    if options.remote {
        let drawn = |dump: &str| {
            let remotes = dump.find("\"remotes\"").map_or("", |at| &dump[at..]);
            let model =
                number_after(remotes, "seat") == Some(1.0) && !remotes.contains("\"model\": null");
            let fired = remotes
                .find("\"ready_ms\"")
                .map(|at| &remotes[at..])
                .and_then(|ready| {
                    let open = ready.find('[')?;
                    let close = ready.find(']')?;
                    let waits: Vec<f64> = ready[open + 1..close]
                        .split(',')
                        .filter_map(|n| n.trim().parse().ok())
                        .collect();
                    Some(waits.len() == 3 && waits[0] > 0.0 && waits[1] > 0.0)
                })
                .unwrap_or(false);
            (model, fired)
        };
        let mid = |run: &Path, name: &str| {
            std::fs::read_to_string(run.join(format!("{name}.json"))).unwrap_or(String::new())
        };
        let (host, player) = (
            drawn(&mid(&host_run, "host-remote")),
            drawn(&mid(&player_run, "player-remote")),
        );
        let _ = writeln!(
            report,
            "- the guest's droid (drawn with its model, bolt and blast fired): the host {host:?}, the player {player:?} (shots `host-remote.png`, `player-remote.png`, `player-kit.png`)"
        );
        if !(host.0 && player.0 && player.1) {
            failed.push("the remote droid".to_string());
        }
    }
    // A minigame preset: each side drew its own seat's run or gun mid-way.
    if let Some(minigame) = options.minigame {
        let mid = |run: &Path, side: &str| {
            let dump = std::fs::read_to_string(run.join(format!("{side}-mid.json")))
                .unwrap_or(String::new());
            let course = dump.find("\"minigame\"").map_or("", |at| &dump[at..]);
            ["seat", "runs", "gunners", "ghosts_shown"].map(|key| number_after(course, key))
        };
        let (host_mid, player_mid) = (mid(&host_run, "host"), mid(&player_run, "player"));
        let shown = |seen: [Option<f64>; 4]| {
            format!(
                "seat {:?}, runs {:?}, gunners {:?}, ghosts {:?}",
                seen[0], seen[1], seen[2], seen[3]
            )
        };
        let own = match minigame {
            "race" => {
                host_mid[0] == Some(0.0)
                    && player_mid[0] == Some(1.0)
                    && host_mid[1] == Some(1.0)
                    && player_mid[1] == Some(1.0)
            }
            _ => host_mid[2] == Some(1.0) && player_mid[2] == Some(1.0),
        };
        let _ = writeln!(
            report,
            "- the {minigame} in co-op: the host drew {}; the player {} (shots `host-mid.png`, `player-mid.png`)",
            shown(host_mid),
            shown(player_mid)
        );
        if !own {
            failed.push(format!("the {minigame}'s co-op"));
        }
    }
    // The round trip each side reads, against the path's.
    let reads = |dump: &str| {
        number_after(dump, "rtt_ms").map_or("-".to_string(), |ms| format!("{ms:.1} ms"))
    };
    let _ = writeln!(
        report,
        "- round trip: the host reads {}, the player {} (the path: {})",
        reads(&host_dump),
        reads(&player_dump),
        path_rtt_ms(&options.netem).map_or("-".to_string(), |ms| format!(
            "{ms:.0} ms of netem delay, jitter on top"
        ))
    );
    for key in [
        "mismatches",
        "compared",
        "rebuilt",
        "commands",
        "bytes_down",
        "bytes_up",
    ] {
        let _ = writeln!(
            report,
            "- player {key}: {}",
            number_after(&player_dump, key).map_or("-".to_string(), |n| n.to_string())
        );
    }
    // The net feel of both sides.
    let feel = Command::new(&binary)
        .current_dir(repo_root())
        .args(["net", "feel"])
        .arg(host_run.join("net.json"))
        .arg(player_run.join("net.json"))
        .output()
        .map_err(|error| format!("kjk net feel: {error}"))?;
    if feel.status.success() {
        std::fs::write(folder.join("net-feel.md"), &feel.stdout)
            .map_err(|error| format!("net-feel.md: {error}"))?;
        let _ = writeln!(report, "- the net feel: `net-feel.md`");
    } else {
        failed.push("net feel".to_string());
    }
    let verdict = if failed.is_empty() {
        "passed".to_string()
    } else {
        format!("FAILED: {}", failed.join(", "))
    };
    let _ = writeln!(report, "\n{verdict}");
    std::fs::write(folder.join("duo.md"), &report).map_err(|error| format!("duo.md: {error}"))?;
    print!("{report}");
    if failed.is_empty() {
        Ok(())
    } else {
        Err(verdict)
    }
}
