//! `cargo xtask lobby`: the co-op lobby with a host and two guests as three
//! processes on this machine, over 127.0.0.1, all headless and muted.
//!
//! The host opens the lobby on a save (`kjk host --lobby 2`), and starts
//! the game once both guests are Ready. The first guest joins and marks
//! Ready (`kjk join --lobby-script ready`). The second joins a few seconds
//! later. Two cases run, each on a save of its own:
//! - **swap** (`--load`): the second guest swaps characters with the first,
//!   then marks Ready (`--lobby-script swap`). The save needs two
//!   companions in its party, or enough recruited ones to fill it (a chain
//!   checkpoint such as `k1-08`, the Sith base on Taris).
//! - **droid** (`--droid`): the party has fewer characters than players (a
//!   checkpoint such as `k1-02`, Taris with Carth alone), so the second
//!   guest's slot shows "Remote droid". It marks Ready, and flies a droid.
//!
//! A case passes when:
//! - the host started with both guests, and both loaded the save's level;
//! - every snapshot each guest rebuilt hashes as the host stepped it;
//! - the party in the host's world is the lobby's party: as many companions
//!   follow the main character as the lobby put in it;
//! - swap: the second guest's companion at the start is the one the first
//!   had before the swap, and each guest drives a different companion;
//! - droid: the second guest's seat starts as a droid and flies one in the
//!   host's world, and the first drives its companion.
//!
//! A third check, **sections** (`--tsl-checkpoints`, `--k1-checkpoints`),
//! opens the lobby screen on a copy of each chain checkpoint saved inside a
//! part of the story where the game makes the player play another character
//! (`sections`), and checks what it shows (`run_sections`).
//!
//! The saves' folders are copied, so the saves are never changed. The
//! results go to `kjk-artifacts/runs/<time>-lobby/lobby.md`.
//!
//! ```text
//! cargo xtask lobby --saves DIR [--load ID] [--droid ID] [--port 29191] [--binary PATH]
//!                   [--tsl-checkpoints DIR] [--k1-checkpoints DIR]
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::duo::{build, copy_tree, hashes, plain, run_folder, spawn, wait_for};
use crate::repo_root;

/// A case of the run (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Case {
    Swap,
    Droid,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Case::Swap => "swap",
            Case::Droid => "droid",
        }
    }

    /// The second guest's lobby script (`kjk join --lobby-script`).
    fn second(self) -> &'static str {
        match self {
            Case::Swap => "swap",
            Case::Droid => "ready",
        }
    }
}

struct Options {
    saves: PathBuf,
    cases: Vec<(Case, String)>,
    port: u16,
    binary: Option<PathBuf>,
    /// The folders of the KOTOR 2 and KOTOR chain checkpoints the sections
    /// check reads (`--tsl-checkpoints`, `--k1-checkpoints`).
    checkpoints: [Option<PathBuf>; 2],
}

/// A part of the story where the game makes the player play another
/// character, saved in a chain checkpoint: the checkpoint, its game, the
/// character the first seat plays, the companions the lobby may offer (the
/// section's own party), and the name of the main character, who must not
/// be offered.
struct Section {
    checkpoint: &'static str,
    game: &'static str,
    hero: &'static str,
    offered: &'static [u64],
    main: &'static str,
}

/// A KOTOR 2 section: its main character is named "Exile" in the chain's
/// checkpoints. KOTOR's chain checkpoints name theirs nothing.
fn section(
    checkpoint: &'static str,
    game: &'static str,
    hero: &'static str,
    offered: &'static [u64],
) -> Section {
    Section {
        checkpoint,
        game,
        hero,
        offered,
        main: if game == "tsl" { "Exile" } else { "" },
    }
}

/// Every chain checkpoint saved inside such a part of the story, found by
/// reading each one's party table (2026-11-16). KOTOR 2's party numbers:
/// 0 Atton, 1 Bao-Dur (the remote on Malachor uses his number), 6 Kreia,
/// 7 the prologue's droid and Mira, 8 T3-M4, 9 Visas Marr. KOTOR's: 3 HK-47.
fn sections() -> Vec<Section> {
    vec![
        section("prologue_cargo", "tsl", "T3-M4", &[]),
        section("prologue_garage", "tsl", "T3-M4", &[7]),
        section("prologue_hull", "tsl", "T3-M4", &[7]),
        section("prologue_hyperdrive", "tsl", "T3-M4", &[7]),
        section("prologue_inside", "tsl", "T3-M4", &[7]),
        section("telos_military_base", "tsl", "Bao-Dur", &[6]),
        section("nar_shaddaa_t3", "tsl", "T3-M4", &[]),
        section("nar_shaddaa_yacht", "tsl", "Atton", &[]),
        section("nar_shaddaa_bridge", "tsl", "Mira", &[8]),
        section("dxun_sith_tomb", "tsl", "Visas", &[0, 1]),
        section("at_wave2", "tsl", "Visas", &[0, 1]),
        section("malachor_remote", "tsl", "Remote", &[]),
        section("lev_detention", "k1", "HK-47", &[]),
        section("lev_icebreaker", "k1", "HK-47", &[]),
        section("lev_key", "k1", "HK-47", &[]),
    ]
}

fn options(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        saves: PathBuf::new(),
        cases: Vec::new(),
        port: 29191,
        binary: None,
        checkpoints: [None, None],
    };
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag}: needs a value"))?;
        match flag.as_str() {
            "--saves" => options.saves = PathBuf::from(value),
            "--load" => options.cases.push((Case::Swap, value.clone())),
            "--droid" => options.cases.push((Case::Droid, value.clone())),
            "--port" => options.port = value.parse().map_err(|_| format!("--port {value}"))?,
            "--binary" => options.binary = Some(PathBuf::from(value)),
            "--tsl-checkpoints" => options.checkpoints[0] = Some(PathBuf::from(value)),
            "--k1-checkpoints" => options.checkpoints[1] = Some(PathBuf::from(value)),
            other => {
                return Err(format!(
                    "{other}: --saves, --load, --droid, --port, --binary, --tsl-checkpoints or --k1-checkpoints"
                ));
            }
        }
    }
    let missing = (options.cases.iter()).find(|(_, id)| !options.saves.join(id).is_dir());
    let sections = options.checkpoints.iter().any(Option::is_some);
    if (options.cases.is_empty() && !sections) || missing.is_some() {
        return Err(
            "--saves DIR with --load ID and/or --droid ID (saves in DIR), or --tsl-checkpoints DIR and/or --k1-checkpoints DIR".into(),
        );
    }
    Ok(options)
}

/// The lines of `name`'s log that hold `text`, without their colours.
fn lines_with(folder: &Path, name: &str, text: &str) -> Vec<String> {
    let log = std::fs::read_to_string(folder.join(format!("{name}.log"))).unwrap_or(String::new());
    log.lines()
        .map(plain)
        .filter(|line| line.contains(text))
        .collect()
}

/// The companion a slot plays in a `seats = [(0, Hero), (1, Companion(7)), …]`
/// list as the host logs it.
fn companion_of(seats: &str, slot: usize) -> Option<String> {
    let marker = format!("({slot}, Companion(");
    let rest = &seats[seats.find(&marker)? + marker.len()..];
    Some(rest[..rest.find(')')?].to_string())
}

/// The seat each guest's body has in the host's dump, by its roster's
/// `"who": "player N (tag)"`, as (seat, tag).
fn driven(dump: &str) -> Vec<(u8, String)> {
    let json: serde_json::Value = serde_json::from_str(dump).unwrap_or(serde_json::Value::Null);
    let roster = json["facts"]["characters"]["roster"]
        .as_array()
        .cloned()
        .unwrap_or_else(Vec::new);
    roster
        .iter()
        .filter_map(|row| {
            let who = row["who"].as_str()?.strip_prefix("player ")?;
            let (seat, tag) = who.split_once(" (")?;
            Some((seat.parse().ok()?, tag.trim_end_matches(')').to_string()))
        })
        .filter(|(seat, _)| *seat > 0)
        .collect()
}

/// The companions the lobby put in the party, as the host's `lobby
/// started: party [0, 7], …` line lists them.
fn party_of(start: &str) -> usize {
    start
        .split("party [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .map_or(0, |list| {
            list.split(',').filter(|npc| !npc.trim().is_empty()).count()
        })
}

/// The companions following the main character in the host's dump, and the
/// seats that fly a droid.
fn followers_and_droids(dump: &str) -> (usize, Vec<u64>) {
    let json: serde_json::Value = serde_json::from_str(dump).unwrap_or(serde_json::Value::Null);
    let members = json["facts"]["loot"]["members"]
        .as_array()
        .map_or(0, |members| {
            members.iter().filter(|member| member["id"] != 0).count()
        });
    let droids = json["facts"]["remotes"]["in_snapshot"]
        .as_array()
        .map_or_else(Vec::new, |seats| {
            seats.iter().filter_map(serde_json::Value::as_u64).collect()
        });
    (members, droids)
}

/// Snapshots `guest` rebuilt and how many of them are unlike the host's.
fn unlike(host: &[(String, String, String)], guest: &[(String, String, String)]) -> usize {
    (guest.iter())
        .filter(|(epoch, tick, hash)| {
            !(host.iter()).any(|(e, t, h)| e == epoch && t == tick && h == hash)
        })
        .count()
}

pub fn run(args: &[String]) -> Result<(), String> {
    let options = options(args)?;
    let binary = match &options.binary {
        Some(binary) => binary.clone(),
        None => build()?,
    };
    let stamp = (SystemTime::now().duration_since(UNIX_EPOCH)).map_or(0, |since| since.as_secs());
    let root = repo_root()
        .join("kjk-artifacts")
        .join("runs")
        .join(format!("{stamp}-lobby"));
    println!("lobby: {}", root.display());
    let mut report = format!(
        "# lobby {stamp}\n\nsaves from `{}`\n",
        options.saves.display()
    );
    let mut failed = Vec::new();
    for (case, id) in &options.cases {
        let folder = root.join(case.name());
        let (told, failures) = run_case(&binary, &options, *case, id, &folder)?;
        let _ = writeln!(report, "\n## {} on `{id}`\n\n{told}", case.name());
        failed.extend(
            failures
                .into_iter()
                .map(|failure| format!("{}: {failure}", case.name())),
        );
    }
    if options.checkpoints.iter().any(Option::is_some) {
        let (told, failures) = run_sections(&binary, &options, &root.join("sections"))?;
        let _ = writeln!(report, "\n## sections\n\n{told}");
        failed.extend(
            failures
                .into_iter()
                .map(|failure| format!("sections: {failure}")),
        );
    }
    let _ = writeln!(
        report,
        "\n{}",
        if failed.is_empty() {
            "passed".to_string()
        } else {
            format!("failed: {}", failed.join(", "))
        }
    );
    std::fs::create_dir_all(&root).map_err(|error| format!("{}: {error}", root.display()))?;
    std::fs::write(root.join("lobby.md"), &report).map_err(|error| format!("lobby.md: {error}"))?;
    print!("{report}");
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("lobby: {}", failed.join(", ")))
    }
}

/// One case on save `id`, its processes' logs and saves in `folder`: what
/// it found, and what failed.
fn run_case(
    binary: &Path,
    options: &Options,
    case: Case,
    id: &str,
    folder: &Path,
) -> Result<(String, Vec<String>), String> {
    let sides = ["host", "guest-a", "guest-b"];
    for side in sides {
        let saves = folder.join(format!("{side}-saves"));
        std::fs::create_dir_all(&saves).map_err(|error| format!("{}: {error}", saves.display()))?;
    }
    let held = folder.join("host-saves").join(id);
    std::fs::create_dir_all(&held).map_err(|error| format!("{}: {error}", held.display()))?;
    copy_tree(&options.saves.join(id), &held)?;
    let saves_of = |side: &str| {
        folder
            .join(format!("{side}-saves"))
            .to_string_lossy()
            .into_owned()
    };
    let address = format!("127.0.0.1:{}", options.port);
    let host_args = [
        "host",
        "--load",
        id,
        "--lobby",
        "2",
        "--port",
        &options.port.to_string(),
        "--headless",
        "--cmds",
        "wait 20s; dump host; wait 2s; quit",
    ]
    .map(String::from);
    let guest_args = |name: &str, script: &str, side: &str| {
        [
            "join",
            &address,
            "--name",
            name,
            "--lobby-script",
            script,
            "--headless",
            "--cmds",
            &format!("wait 18s; dump {side}; wait 20s; quit"),
        ]
        .map(String::from)
    };
    let host_saves = saves_of("host");
    let mut host = spawn(
        binary,
        &host_args,
        &[("KJK_SAVES", &host_saves), ("KJK_MUTE", "1")],
        &repo_root(),
        folder,
        "host",
    )?;
    let mut guests = Vec::new();
    let opened = wait_for(folder, "host", "lobby open", Duration::from_secs(180));
    let started = opened.and_then(|_| {
        for (name, script, side) in [
            ("Mia", "ready", "guest-a"),
            ("Kalle", case.second(), "guest-b"),
        ] {
            let saves = saves_of(side);
            let env = [("KJK_SAVES", saves.as_str()), ("KJK_MUTE", "1")];
            guests.push(spawn(
                binary,
                &guest_args(name, script, side),
                &env,
                &repo_root(),
                folder,
                side,
            )?);
            std::thread::sleep(Duration::from_secs(4));
        }
        Ok(())
    });
    if let Err(error) = started {
        let _ = host.kill();
        for guest in &mut guests {
            let _ = guest.kill();
        }
        return Err(error);
    }
    let mut statuses = vec![host.wait().map_err(|error| error.to_string())?];
    for guest in &mut guests {
        statuses.push(guest.wait().map_err(|error| error.to_string())?);
    }

    let mut report = String::new();
    let mut failed = Vec::new();
    if statuses.iter().any(|status| !status.success()) {
        failed.push("a process failed".to_string());
    }
    let joined = lines_with(folder, "host", "a player joined the lobby");
    let chose = lines_with(folder, "host", "a player chose");
    let start = lines_with(folder, "host", "lobby started")
        .pop()
        .unwrap_or(String::new());
    let _ = writeln!(
        report,
        "- joined: {joined:?}\n- chose: {chose:?}\n- started: `{start}`"
    );
    if joined.len() != 2 || start.is_empty() {
        failed.push("both guests in the lobby at the start".to_string());
    }
    let runs: Vec<PathBuf> = sides
        .iter()
        .map(|side| run_folder(folder, side))
        .collect::<Result<_, _>>()?;
    for side in &sides[1..] {
        let loading = lines_with(folder, side, "joined as seat");
        let _ = writeln!(report, "- {side}: {loading:?}");
        if loading.is_empty() {
            failed.push(format!("{side} did not load the level"));
        }
    }
    let stepped = hashes(&runs[0].join("hashes.txt"))?;
    for (side, run) in sides[1..].iter().zip(&runs[1..]) {
        let rebuilt = hashes(&run.join("rebuilt.txt"))?;
        let wrong = unlike(&stepped, &rebuilt);
        let _ = writeln!(
            report,
            "- {side}: {} snapshots rebuilt, {wrong} unlike the host's {}",
            rebuilt.len(),
            stepped.len()
        );
        if rebuilt.is_empty() || wrong > 0 {
            failed.push(format!("{side}'s rebuilt snapshots"));
        }
    }
    let dump = std::fs::read_to_string(runs[0].join("host.json")).unwrap_or(String::new());
    let bodies = driven(&dump);
    let (followers, droids) = followers_and_droids(&dump);
    let party = party_of(&start);
    let _ = writeln!(
        report,
        "- the host's dump: the guests' bodies {bodies:?}, {followers} companion(s) following (the lobby's party has {party}), droids for seats {droids:?}"
    );
    if followers != party {
        failed.push(format!(
            "the party in the world ({followers}, the lobby's {party})"
        ));
    }
    let seats: Vec<u8> = bodies.iter().map(|(seat, _)| *seat).collect();
    match case {
        Case::Swap => {
            // The first guest's companion as it joined, before the swap.
            let before = joined.first().and_then(|line| {
                let rest = &line[line.find("Companion(")? + "Companion(".len()..];
                Some(rest[..rest.find(')')?].to_string())
            });
            let after = companion_of(&start, 2);
            if before.is_none() || before != after {
                failed.push(format!(
                    "the swap (the first guest had {before:?}, the second started with {after:?})"
                ));
            }
            let distinct = bodies.len() == 2 && bodies[0].1 != bodies[1].1;
            if !(seats == [1, 2] || seats == [2, 1]) || !distinct {
                failed.push("each guest driving its own companion".to_string());
            }
        }
        Case::Droid => {
            if !start.contains("(2, Droid)") || droids != [2] || seats != [1] {
                failed.push(
                    "the second guest flying a droid, the first driving its companion".to_string(),
                );
            }
        }
    }
    Ok((report, failed))
}

/// The lobby opened on a copy of every section's checkpoint
/// (`kjk gui screen GAME lobby --step "lobby checkpoint PATH"`, with two
/// guests): what it shows, and what failed. A section passes when the story's
/// character in the main character's place is the section's character with a
/// portrait, a class and a level; the main character is not offered; exactly
/// one slot plays the story's character; and the companions offered are the
/// section's own party, so a guest with none left flies a remote droid.
fn run_sections(
    binary: &Path,
    options: &Options,
    folder: &Path,
) -> Result<(String, Vec<String>), String> {
    let saves = folder.join("saves");
    std::fs::create_dir_all(&saves).map_err(|error| format!("{}: {error}", saves.display()))?;
    let (mut report, mut failed) = (String::new(), Vec::new());
    for section in &sections() {
        let dir = match section.game {
            "tsl" => &options.checkpoints[0],
            _ => &options.checkpoints[1],
        };
        let Some(dir) = dir else {
            continue;
        };
        let path = dir.join(format!("{}.kjkv", section.checkpoint));
        if !path.is_file() {
            failed.push(format!("{} is missing", path.display()));
            continue;
        }
        let output = std::process::Command::new(binary)
            .args(["gui", "screen", section.game, "lobby", "--mute", "--out"])
            .arg(folder)
            .args(["--file", &format!("{}.png", section.checkpoint), "--step"])
            .arg(format!("lobby checkpoint {}", path.display()))
            .env("KJK_SAVES", &saves)
            .env("KJK_MUTE", "1")
            .output()
            .map_err(|error| format!("{}: {error}", binary.display()))?;
        let text = String::from_utf8_lossy(&output.stdout);
        let facts: serde_json::Value = (text.lines())
            .find_map(|line| line.split_once("lobby facts ").map(|(_, json)| json))
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or(serde_json::Value::Null);
        let problems = section_problems(section, &facts);
        let _ = writeln!(
            report,
            "- `{}`: {} ({}, level {}, portrait {}), companions offered {:?}, slots {}{}",
            section.checkpoint,
            facts["hero"],
            facts["hero_class"],
            facts["hero_level"],
            facts["hero_portrait"],
            offered(&facts),
            facts["slots"],
            if problems.is_empty() {
                String::new()
            } else {
                format!(": {}", problems.join(", "))
            },
        );
        failed.extend(
            (problems.into_iter()).map(|problem| format!("{}: {problem}", section.checkpoint)),
        );
    }
    Ok((report, failed))
}

/// The party numbers of the companions a lobby's facts offer.
fn offered(facts: &serde_json::Value) -> Vec<u64> {
    (facts["companions"].as_array().into_iter().flatten())
        .filter(|companion| companion["choice"] != "Unavailable")
        .filter_map(|companion| companion["npc"].as_u64())
        .collect()
}

/// What a section's lobby got wrong (see `run_sections`).
fn section_problems(section: &Section, facts: &serde_json::Value) -> Vec<String> {
    if facts.is_null() {
        return vec!["no lobby facts".into()];
    }
    let text = |key: &str| facts[key].as_str().unwrap_or("").to_string();
    let mut problems = Vec::new();
    if text("hero") != section.hero {
        problems.push(format!(
            "the story's character is {:?}, not {}",
            text("hero"),
            section.hero
        ));
    }
    if text("hero_portrait").is_empty() || text("hero_class").is_empty() {
        problems.push("the story's character has no portrait or no class".into());
    }
    if facts["hero_level"].as_u64().unwrap_or(0) == 0 {
        problems.push("the story's character has no level".into());
    }
    let names: Vec<String> = (facts["companions"].as_array().into_iter().flatten())
        .filter_map(|companion| Some(companion["name"].as_str()?.to_string()))
        .collect();
    if names
        .iter()
        .any(|name| name == section.hero || (!section.main.is_empty() && name == section.main))
    {
        problems.push(format!(
            "the companions list the story's or the main character: {names:?}"
        ));
    }
    let slots: Vec<String> = (facts["slots"].as_array().into_iter().flatten())
        .filter_map(|slot| Some(slot.as_str()?.to_string()))
        .collect();
    if slots.iter().filter(|slot| *slot == "Hero").count() != 1 {
        problems.push(format!(
            "not exactly one slot plays the story's character: {slots:?}"
        ));
    }
    if offered(facts) != section.offered {
        problems.push(format!(
            "companions offered {:?}, the section's party {:?}",
            offered(facts),
            section.offered
        ));
    }
    // Each guest plays an offered companion while one is left, else a droid.
    let droids = slots.iter().filter(|slot| *slot == "Droid").count();
    let guests = slots.len().saturating_sub(1);
    if droids != guests.saturating_sub(section.offered.len()) {
        problems.push(format!(
            "{droids} guests fly droids with {} companions offered",
            section.offered.len()
        ));
    }
    problems
}
