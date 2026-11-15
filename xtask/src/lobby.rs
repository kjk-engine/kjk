//! `cargo xtask lobby`: the co-op lobby with a host and two guests as three
//! processes on this machine, over 127.0.0.1, all headless and muted.
//!
//! The host opens the lobby on a save (`kjk host --lobby 2`). The first
//! guest joins and marks Ready (`kjk join --lobby-script ready`). The
//! second joins a few seconds later, swaps characters with the first, and
//! marks Ready (`--lobby-script swap`). The host starts the game once both
//! are Ready. The run passes when:
//! - the host started with both guests, and the second guest's companion at
//!   the start is the one the first guest had before the swap;
//! - both guests loaded the save's level;
//! - every snapshot each guest rebuilt hashes as the host stepped it;
//! - the host's dump shows each guest's seat driving a companion, a
//!   different one each.
//!
//! The save must have at least two companions in its party, or enough
//! recruited ones to fill it (a chain checkpoint such as `k1-08`, the Sith
//! base on Taris). Its folder is copied, so the save is never changed. The
//! results go to `kjk-artifacts/runs/<time>-lobby/lobby.md`.
//!
//! ```text
//! cargo xtask lobby --saves DIR --load ID [--port 29191] [--binary PATH]
//! ```

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::duo::{build, copy_tree, hashes, plain, run_folder, spawn, wait_for};
use crate::repo_root;

struct Options {
    saves: PathBuf,
    load: String,
    port: u16,
    binary: Option<PathBuf>,
}

fn options(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        saves: PathBuf::new(),
        load: String::new(),
        port: 29191,
        binary: None,
    };
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(format!("{flag}: needs a value"))?;
        match flag.as_str() {
            "--saves" => options.saves = PathBuf::from(value),
            "--load" => options.load = value.clone(),
            "--port" => options.port = value.parse().map_err(|_| format!("--port {value}"))?,
            "--binary" => options.binary = Some(PathBuf::from(value)),
            other => return Err(format!("{other}: --saves, --load, --port or --binary")),
        }
    }
    if options.load.is_empty() || !options.saves.join(&options.load).is_dir() {
        return Err("--saves DIR --load ID: a folder of saves holding the save ID".into());
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
    let folder = repo_root()
        .join("kjk-artifacts")
        .join("runs")
        .join(format!("{stamp}-lobby"));
    let sides = ["host", "guest-a", "guest-b"];
    for side in sides {
        let saves = folder.join(format!("{side}-saves"));
        std::fs::create_dir_all(&saves).map_err(|error| format!("{}: {error}", saves.display()))?;
    }
    let held = folder.join("host-saves").join(&options.load);
    std::fs::create_dir_all(&held).map_err(|error| format!("{}: {error}", held.display()))?;
    copy_tree(&options.saves.join(&options.load), &held)?;
    println!("lobby: {}", folder.display());
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
        &options.load,
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
        &binary,
        &host_args,
        &[("KJK_SAVES", &host_saves), ("KJK_MUTE", "1")],
        &repo_root(),
        &folder,
        "host",
    )?;
    let mut guests = Vec::new();
    let opened = wait_for(&folder, "host", "lobby open", Duration::from_secs(180));
    let started = opened.and_then(|_| {
        for (name, script, side) in [("Mia", "ready", "guest-a"), ("Kalle", "swap", "guest-b")] {
            let saves = saves_of(side);
            let env = [("KJK_SAVES", saves.as_str()), ("KJK_MUTE", "1")];
            guests.push(spawn(
                &binary,
                &guest_args(name, script, side),
                &env,
                &repo_root(),
                &folder,
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
    let _ = writeln!(
        report,
        "# lobby {stamp}\n\nsave `{}` from `{}`\n",
        options.load,
        options.saves.display()
    );
    if statuses.iter().any(|status| !status.success()) {
        failed.push("a process failed".to_string());
    }
    let joined = lines_with(&folder, "host", "a player joined the lobby");
    let chose = lines_with(&folder, "host", "a player chose");
    let start = lines_with(&folder, "host", "lobby started")
        .pop()
        .unwrap_or(String::new());
    // The first guest's companion as it joined, before the second's swap.
    let before = joined.first().and_then(|line| {
        let rest = &line[line.find("Companion(")? + "Companion(".len()..];
        Some(rest[..rest.find(')')?].to_string())
    });
    let after = companion_of(&start, 2);
    let _ = writeln!(
        report,
        "- joined: {joined:?}\n- chose: {chose:?}\n- started: `{start}`"
    );
    if joined.len() != 2 || before.is_none() || before != after {
        failed.push(format!(
            "the swap (the first guest had {before:?}, the second started with {after:?})"
        ));
    }
    let runs: Vec<PathBuf> = sides
        .iter()
        .map(|side| run_folder(&folder, side))
        .collect::<Result<_, _>>()?;
    for side in &sides[1..] {
        let loading = lines_with(&folder, side, "joined as seat");
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
    let _ = writeln!(
        report,
        "- the guests' bodies in the host's dump: {bodies:?}"
    );
    let seats: Vec<u8> = bodies.iter().map(|(seat, _)| *seat).collect();
    if seats != [1, 2] && seats != [2, 1] || bodies[0].1 == bodies[1].1 {
        failed.push("each guest driving its own companion".to_string());
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
    std::fs::write(folder.join("lobby.md"), &report)
        .map_err(|error| format!("lobby.md: {error}"))?;
    print!("{report}");
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("lobby: {}", failed.join(", ")))
    }
}
