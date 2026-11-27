//! Commands that start, load or join a game, and list the saves and the
//! mods: warp, sandbox, saves, load, mods, host and join.

use super::Ran;
use crate::{Arg, ArgKind, Effect, Facts, Group, Kind, Parsed, Place, Refusal, Spec, Start};

pub static SPECS: &[Spec] = &[
    Spec {
        name: "warp",
        aliases: &[],
        args: &[
            Arg::new("MODULE", ArgKind::Word),
            Arg::optional("ARRIVAL", ArgKind::Word),
        ],
        help: "Loads a module, arriving at a door or a waypoint when one is given. At the main menu it starts a new game there with a quick character. The game is marked as Cheats used.",
        example: "warp tar_m02aa",
        group: Group::Session,
        place: Place::Anywhere,
        kind: Kind::Cheat,
        run: warp,
    },
    Spec {
        name: "sandbox",
        aliases: &[],
        args: &[],
        help: "Opens the Debug Mode map.",
        example: "sandbox",
        group: Group::Session,
        place: Place::Menu,
        kind: Kind::Session,
        run: sandbox,
    },
    Spec {
        name: "saves",
        aliases: &[],
        args: &[],
        help: "Lists the saves with their numbers, the latest first.",
        example: "saves",
        group: Group::Session,
        place: Place::Anywhere,
        kind: Kind::Local,
        run: saves,
    },
    Spec {
        name: "load",
        aliases: &[],
        args: &[Arg::new("NUMBER", ArgKind::Word)],
        help: "Loads the save with this number. Type saves to see the numbers.",
        example: "load 0003",
        group: Group::Session,
        place: Place::Anywhere,
        kind: Kind::Session,
        run: load,
    },
    Spec {
        name: "mods",
        aliases: &[],
        args: &[],
        help: "Lists the override folders and the Steam Workshop items the game reads, in the order they apply.",
        example: "mods",
        group: Group::Session,
        place: Place::Anywhere,
        kind: Kind::Local,
        run: mods,
    },
    Spec {
        name: "host",
        aliases: &[],
        args: &[Arg::optional("NUMBER", ArgKind::Word)],
        help: "Opens a co-op lobby for a new game, or for the save with this number.",
        example: "host 0003",
        group: Group::Session,
        place: Place::Menu,
        kind: Kind::Session,
        run: host,
    },
    Spec {
        name: "join",
        aliases: &[],
        args: &[
            Arg::new("ADDRESS", ArgKind::Word),
            Arg::optional("NAME", ArgKind::Word),
        ],
        help: "Joins the co-op game hosted at this address, with your player name when one is given.",
        example: "join 192.168.1.20",
        group: Group::Session,
        place: Place::Menu,
        kind: Kind::Session,
        run: join,
    },
];

/// Why a command that changes the session for every player cannot run
/// now: in the lobby, while an area loads, or for a player who is not the
/// host.
fn host_refusal(facts: &Facts) -> Option<Refusal> {
    if facts.lobby {
        return Some(Refusal::new("Leave the lobby before using this command."));
    }
    if !facts.loaded && !facts.menu {
        return Some(Refusal::new("Wait for the area to finish loading."));
    }
    (facts.loaded && !facts.host).then(|| Refusal::new("Only the host can use this command."))
}

/// The effect `start`, and the line that says it began.
fn started(start: Start, said: String) -> Ran {
    Ok(vec![Effect::Start(start), Effect::Print(said)])
}

fn warp(parsed: &Parsed, facts: &Facts) -> Ran {
    if let Some(refusal) = host_refusal(facts) {
        return Err(refusal);
    }
    let module = parsed.word(0).unwrap_or("").to_ascii_lowercase();
    let arrival = parsed.word(1).map(str::to_string);
    let said = format!("Loading {module}.");
    started(Start::Warp { module, arrival }, said)
}

fn sandbox(_: &Parsed, _: &Facts) -> Ran {
    started(Start::Sandbox, "Loading the Debug Mode map.".into())
}

fn saves(_: &Parsed, _: &Facts) -> Ran {
    Ok(vec![Effect::Saves])
}

fn load(parsed: &Parsed, facts: &Facts) -> Ran {
    if let Some(refusal) = host_refusal(facts) {
        return Err(refusal);
    }
    let id = parsed.word(0).unwrap_or("").to_string();
    let said = format!("Loading save {id}.");
    started(Start::Load(id), said)
}

fn mods(_: &Parsed, _: &Facts) -> Ran {
    Ok(vec![Effect::Mods])
}

fn host(parsed: &Parsed, _: &Facts) -> Ran {
    let save = parsed.word(0).map(str::to_string);
    let said = match &save {
        Some(id) => format!("Opening a co-op lobby for save {id}."),
        None => "Opening a co-op lobby for a new game.".into(),
    };
    started(Start::Host(save), said)
}

fn join(parsed: &Parsed, _: &Facts) -> Ran {
    let address = parsed.word(0).unwrap_or("").to_string();
    let name = parsed.word(1).map(str::to_string);
    let said = format!("Connecting to {address}.");
    started(Start::Join { address, name }, said)
}
