//! `kjk combat brawls`: the fights the games stage between creatures, none
//! of them the party's, found from the data and checked in the sim.
//!
//! A module stages such a fight two ways: creatures of factions
//! `repute.2da` makes enemies placed near each other (Taris's gangs), or a
//! script of its own that turns a faction (`ChangeToStandardFaction`,
//! `ChangeFaction`, `SetIsTemporaryEnemy`, `AdjustReputation`,
//! `SetStandardFactionReputation`: the Upper Sewers' kath hounds turned on
//! the rakghouls by the room's trigger). Each module is played once as it
//! loads and once after each of its own such scripts, run for the player
//! as its trigger or conversation would: after `SETTLE_MS` the nearest
//! two living creatures outside the party that are enemies, within
//! `NEAR_M` of each other, are its fight. The player then arrives at the
//! door or waypoint nearest them (a fight starts as the games' do, when
//! the player comes and the creatures perceive anew) and `seconds` are
//! stepped; the blows creatures outside the party land on each other are
//! counted. A fight staged that lands none is a brawl the sim stands idle
//! in.

use std::collections::BTreeMap;
use std::process::ExitCode;

use fmt_odyssey::ResType;
use sim::module::{ObjectKind, Side};
use sim::{Kind, Seat, SimWorld, Snapshot, StepReason, TickInput, TickRate, step};
use vfs::{DirIndex, Discovery, Game};

/// The routines that turn creatures on each other.
const TURNING: [&str; 5] = [
    "ChangeToStandardFaction",
    "ChangeFaction",
    "SetIsTemporaryEnemy",
    "AdjustReputation",
    "SetStandardFactionReputation",
];

/// How long a scenario runs before its fight is looked for (sim ms): its
/// script's delayed commands and the spawn scripts' first round.
const SETTLE_MS: u64 = 3000;

/// Two enemies this near each other (metres) are a fight staged.
const NEAR_M: f32 = 20.0;

/// One scenario of a module: as it loads (`script` none) or after one of
/// its scripts.
struct Outcome {
    module: String,
    script: Option<String>,
    /// The two enemies found (tags) and how far apart (metres).
    pair: Option<(String, String, f32)>,
    /// Enemy pairs within `NEAR_M`.
    pairs: usize,
    arrive: Option<String>,
    /// Blows landed between creatures outside the party, by (attacker,
    /// target) tag.
    blows: BTreeMap<(String, String), usize>,
    deaths: usize,
    /// The creatures that set on an enemy outside the party and stood at it
    /// `IDLE_MS` or more never attacking (no blow landed on one, attack
    /// never held, no round begun: tags).
    idle: Vec<String>,
}

/// How long (sim ms) a creature may fight another outside the party
/// without a blow landed before it counts as idle: two of KOTOR's rounds.
const IDLE_MS: u64 = 6000;

/// `kjk combat brawls`: every module of `game` (or `only`), each scenario
/// stepped `seconds`.
pub fn brawls(discovery: &Discovery, game: &str, only: &[String], seconds: u64) -> ExitCode {
    let installer = match crate::scenes::installer(discovery, game) {
        Ok(installer) => installer,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let Some(key) = Game::from_key(game) else {
        eprintln!("use k1 or tsl");
        return ExitCode::from(2);
    };
    let Some(install) = discovery.get(key) else {
        eprintln!("{game}: not found");
        return ExitCode::FAILURE;
    };
    let index = match DirIndex::build(&install.data_root) {
        Ok(index) => index,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let modules: Vec<String> = match only {
        [] => crate::modules::module_names(&index),
        only => only.to_vec(),
    };
    let mut outcomes = Vec::new();
    let mut refused = Vec::new();
    for module in &modules {
        match module_outcomes(&installer, &index, key, module, seconds) {
            Ok(found) => outcomes.extend(found),
            Err(error) => refused.push((module.clone(), error)),
        }
    }
    let staged: Vec<&Outcome> = outcomes.iter().filter(|o| o.pair.is_some()).collect();
    outln!(
        "combat brawls {game}: {} modules, {} scenarios with a fight staged, {} with idle fighters",
        modules.len(),
        staged.len(),
        staged.iter().filter(|o| !o.idle.is_empty()).count()
    );
    for outcome in &staged {
        let (a, b, apart) = outcome.pair.as_ref().expect("staged");
        outln!(
            "  {:<12} {:<18} {:>3} pairs  {a} x {b} {apart:.1} m  arrive {:<20} blows {:>4}  deaths {:>2}  idle {}",
            outcome.module,
            outcome.script.as_deref().unwrap_or("(as loaded)"),
            outcome.pairs,
            outcome.arrive.as_deref().unwrap_or("(entry)"),
            outcome.blows.values().sum::<usize>(),
            outcome.deaths,
            outcome.idle.join(", ")
        );
    }
    for (module, error) in &refused {
        outln!("  {module}: refused: {error}");
    }
    let rows: Vec<serde_json::Value> = outcomes
        .iter()
        .map(|o| {
            serde_json::json!({
                "module": o.module, "script": o.script,
                "pair": o.pair.as_ref().map(|(a, b, m)| serde_json::json!([a, b, m])),
                "pairs": o.pairs, "arrive": o.arrive, "idle": o.idle,
                "blows": o.blows.iter().map(|((a, b), n)| serde_json::json!([a, b, n])).collect::<Vec<_>>(),
                "deaths": o.deaths,
            })
        })
        .collect();
    crate::runtime::write_facts(serde_json::json!({
        "verb": "combat brawls", "game": game, "seconds": seconds,
        "scenarios": rows,
        "refused": refused.iter().map(|(m, e)| serde_json::json!([m, e])).collect::<Vec<_>>(),
    }));
    ExitCode::SUCCESS
}

/// A module's scenarios: as it loads, and after each of its own scripts
/// that turn factions.
fn module_outcomes(
    installer: &crate::lifecycle::Installer,
    index: &DirIndex,
    game: Game,
    module: &str,
    seconds: u64,
) -> Result<Vec<Outcome>, String> {
    let level = installer.prepare_one(module, None)?.plan.level;
    let own = vfs::resolver::Resolver::open(
        game,
        index,
        Some(module),
        vfs::resolver::TextureQuality::High,
    )
    .map_err(|error| error.to_string())?
    .module_names(ResType::NCS);
    let turning: Vec<String> = level.scripts().map_or_else(Vec::new, |library| {
        own.into_iter()
            .filter(|name| library.may_call(name, &TURNING))
            .collect()
    });
    std::iter::once(None)
        .chain(turning.into_iter().map(Some))
        .map(|script| scenario(&level, module, script, seconds))
        .collect()
}

/// One scenario (see the module documentation).
fn scenario(
    level: &sim::Level,
    module: &str,
    script: Option<String>,
    seconds: u64,
) -> Result<Outcome, String> {
    let scripts: Vec<String> = script.iter().cloned().collect();
    let start = |arrive: Option<&str>| -> Result<SimWorld, String> {
        let mut world = crate::scenes::one_seat(level.clone(), arrive)?;
        crate::by_hand::queue_scripts(&mut world, &scripts)?;
        Ok(world)
    };
    let mut outcome = Outcome {
        module: module.to_string(),
        script,
        pair: None,
        pairs: 0,
        arrive: None,
        blows: BTreeMap::new(),
        deaths: 0,
        idle: Vec::new(),
    };
    let rate = TickRate::default();
    let mut world = start(None)?;
    let standing = |_: &Snapshot| sim::Command::default();
    let settled = run(&mut world, rate, SETTLE_MS, standing, |_, _| {});
    let enemies = enemies_near(&world, &settled);
    outcome.pairs = enemies.len();
    let Some(&(a, b, apart)) = enemies.first() else {
        return Ok(outcome);
    };
    let objects = sim::Instances::in_snapshot(world.level(), &settled);
    let tag = |at: usize| tag_of(&settled.entities[at], &objects);
    let (a_at, b_at) = (settled.entities[a].player, settled.entities[b].player);
    outcome.pair = Some((tag(a), tag(b), apart));
    let middle = match (a_at, b_at) {
        (Some(a), Some(b)) => sim::math::scale(
            sim::math::add(a.origin, b.origin),
            0.5 * collision::UNIT,
        ),
        _ => return Ok(outcome),
    };
    outcome.arrive = nearest_way(level, middle);
    let mut world = start(outcome.arrive.as_deref())?;
    // The player walks up to the fight, as a player would come upon it.
    let mut walker = me(&world.snapshot())
        .and_then(|at| crate::script_verbs::walk_to(level, &at, middle[0], middle[1]).ok());
    let mut counted = std::collections::BTreeSet::new();
    let mut dead = std::collections::BTreeSet::new();
    // Each creature's time at an enemy outside the party (ms), and those
    // that attacked, by id.
    let mut at_foe: BTreeMap<u32, (u64, String)> = BTreeMap::new();
    let mut attacked = std::collections::BTreeSet::new();
    let msec = u64::from(rate.msec(1));
    let walk = |last: &Snapshot| match (walker.as_mut(), me(last)) {
        // It opens the doors on its way.
        (Some(walker), Some(at)) => sim::Command {
            buttons: sim::buttons::USE,
            ..walker.command(level, &at, last.time_ms, &crate::scenes::door_states(last))
        },
        _ => sim::Command::default(),
    };
    run(&mut world, rate, seconds * 1000, walk, |last, world| {
        // A round under way (KOTOR's, between creatures: `combat::rounds`).
        attacked.extend(world.script_state().strikes.iter().map(|strike| strike.by.0));
        let objects = sim::Instances::in_snapshot(level, last);
        for entity in last.entities.iter().filter(|e| outsider(e)) {
            let Some(fighter) = entity.fighter else {
                continue;
            };
            if fighter.dead() {
                dead.insert(entity.id.0);
            }
            let foe = fighter.target.and_then(|target| last.get(target));
            if fighter.last_command.buttons & sim::buttons::ATTACK != 0 {
                attacked.insert(entity.id.0);
            }
            if fighter.alive() && foe.is_some_and(outsider) {
                at_foe
                    .entry(entity.id.0)
                    .or_insert((0, tag_of(entity, &objects)))
                    .0 += msec;
            }
            for event in fighter.events.iter().flatten() {
                let by = event.by.and_then(|by| last.get(by)).filter(|by| outsider(by));
                if let Some(by) = by
                    && counted.insert((entity.id.0, event.sequence))
                {
                    let pair = (
                        tag_of(by, &objects),
                        tag_of(entity, &objects),
                    );
                    *outcome.blows.entry(pair).or_insert(0) += 1;
                    attacked.insert(by.id.0);
                }
            }
        }
    });
    outcome.deaths = dead.len();
    outcome.idle = at_foe
        .into_iter()
        .filter(|(id, (ms, _))| *ms >= IDLE_MS && !attacked.contains(id))
        .map(|(_, (_, tag))| tag)
        .collect();
    Ok(outcome)
}

/// A creature outside the party.
fn outsider(entity: &sim::EntityState) -> bool {
    matches!(entity.kind, Kind::Object(ObjectKind::Creature, _)) && !sim::party_member(entity)
}

/// The player's body in `snapshot`.
fn me(snapshot: &Snapshot) -> Option<sim::motion::MotionState> {
    snapshot
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, Kind::Avatar(_)))
        .and_then(|entity| entity.player)
}

/// Steps `world` for `ms`, the player's command each step by `command`
/// from the last snapshot, calling `each` with every snapshot and the
/// world after it; the last one.
fn run(
    world: &mut SimWorld,
    rate: TickRate,
    ms: u64,
    mut command: impl FnMut(&Snapshot) -> sim::Command,
    mut each: impl FnMut(&Snapshot, &SimWorld),
) -> Snapshot {
    let mut last = world.snapshot();
    let until = rate.time_ms(last.tick) + ms;
    while rate.time_ms(last.tick) < until {
        let tick = last.tick + 1;
        let mut input = TickInput::default();
        input.insert(Seat(0), command(&last));
        last = step(world, tick, &input, rate.msec(tick), StepReason::Authority);
        each(&last, world);
    }
    last
}

/// The pairs of living creatures outside the party that are enemies (either
/// way, by `repute.2da` as scripts left it) within `NEAR_M` of each other,
/// nearest first: their places in `snapshot`'s entities and the metres
/// between them.
fn enemies_near(world: &SimWorld, snapshot: &Snapshot) -> Vec<(usize, usize, f32)> {
    let level = world.level();
    let Some(module) = level.module() else {
        return Vec::new();
    };
    let standing = module.repute.with(&world.script_state().reputation);
    let objects = sim::Instances::in_snapshot(level, snapshot);
    let creatures: Vec<(usize, Side, sim::math::Vec3)> = snapshot
        .entities
        .iter()
        .enumerate()
        .filter(|(_, e)| outsider(e))
        .filter_map(|(at, e)| {
            let fighter = e.fighter.filter(|f| f.alive())?;
            let sheet = objects.of_entity(e)?.sheet.as_ref()?;
            let side = Side {
                id: e.id.0,
                faction: fighter.faction.unwrap_or(sheet.faction),
                party: false,
            };
            Some((at, side, e.player?.origin))
        })
        .collect();
    let mut pairs = Vec::new();
    for (i, (a, side_a, at_a)) in creatures.iter().enumerate() {
        for (b, side_b, at_b) in &creatures[i + 1..] {
            let metres = sim::math::length(sim::math::sub(*at_a, *at_b)) * collision::UNIT;
            let enemies = standing.hostile(side_a.faction, side_b.faction)
                || standing.hostile(side_b.faction, side_a.faction);
            if enemies && metres <= NEAR_M {
                pairs.push((*a, *b, metres));
            }
        }
    }
    pairs.sort_by(|x, y| x.2.total_cmp(&y.2));
    pairs
}

/// The tag of the door or waypoint nearest `at` (metres) that no other
/// door or waypoint has (an arrival takes the first of a tag).
fn nearest_way(level: &sim::Level, at: sim::math::Vec3) -> Option<String> {
    let module = level.module()?;
    let ways = || {
        module
            .objects()
            .filter(|object| matches!(object.kind, ObjectKind::Door | ObjectKind::Waypoint))
            .filter(|object| !object.tag.is_empty())
    };
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for object in ways() {
        *counts.entry(object.tag.to_ascii_lowercase()).or_insert(0) += 1;
    }
    ways()
        .filter(|object| counts.get(&object.tag.to_ascii_lowercase()) == Some(&1))
        .min_by(|a, b| {
            let d = |o: &sim::module::ObjectData| sim::math::length(sim::math::sub(o.position, at));
            d(a).total_cmp(&d(b))
        })
        .map(|object| object.tag.clone())
}

/// A creature's tag (its number where it has none).
fn tag_of(entity: &sim::EntityState, objects: &sim::Instances) -> String {
    match entity.kind {
        Kind::Object(_, index) => objects
            .tag(index)
            .map_or_else(|| format!("#{index}"), str::to_string),
        _ => "player".to_string(),
    }
}
