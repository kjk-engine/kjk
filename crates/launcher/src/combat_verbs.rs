//! `kjk combat …`: combat's evidence without a window (I23). `roster` lists
//! a module's creatures as combat reads them (sheet, faction, weapons,
//! whether they are hostile to the player); `fight` installs a module and
//! lets the sim fight it out for a while, the player's seat driven by the
//! same stand-in mind the creatures use (`sim::combat::mind`), then replays
//! the recording to the same end. Each writes `facts.json`.

use std::process::ExitCode;

use sim::combat::sheet::factions;
use sim::combat::{Delivery, Fighter, mind};
use sim::module::ObjectKind;
use sim::{Kind, Recorder, Seat, Snapshot, StepReason, TickInput, TickRate, step};
use vfs::Discovery;

fn weapon_name(delivery: Option<Delivery>) -> String {
    delivery.map_or_else(|| "fists".to_string(), |delivery| format!("{delivery:?}"))
}

/// `kjk combat roster`: every creature of a module as combat reads it.
pub fn roster(discovery: &Discovery, game: &str, module: &str) -> ExitCode {
    let plan = match crate::scenes::installer(discovery, game)
        .and_then(|installer| installer.prepare(&installer.request(module, None, 1), false))
    {
        Ok(prepared) => prepared.plan,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let data = plan.level.module().expect("a plan has its module");
    outln!(
        "combat roster {module}: {} creatures, {} with a sheet read",
        data.count(ObjectKind::Creature),
        plan.facts.sheets
    );
    let mut rows = Vec::new();
    for (index, object) in data.objects().enumerate() {
        if object.kind != ObjectKind::Creature {
            continue;
        }
        let Some(sheet) = object.sheet else {
            outln!(
                "  {index:>3} {:<22} no sheet ({})",
                object.tag,
                object.template
            );
            continue;
        };
        // How the creature regards the player (its row, the player's
        // column): what makes it attack.
        let hostile = data.repute.hostile(sheet.faction, factions::PLAYER);
        let weapons: Vec<String> = sheet
            .weapons
            .iter()
            .map(|weapon| weapon_name(weapon.map(|w| w.delivery)))
            .collect();
        outln!(
            "  {index:>3} {:<22} {:<16} level {:>2} HP {:>3} FP {:>3} def {:>2} faction {:>2}{} {}",
            object.tag,
            object.template,
            sheet.level,
            sheet.max_hp,
            sheet.max_fp,
            sheet.defence(),
            sheet.faction,
            if hostile { " HOSTILE" } else { "" },
            weapons.join(" / ")
        );
        // What its items guard against, and its weapons' race bonuses.
        let guards: Vec<String> = (0..sim::combat::sheet::DAMAGE_TYPES)
            .filter_map(|row| {
                let (immunity, resist) = (sheet.guards.immunity[row], sheet.guards.resist[row]);
                ((immunity, resist) != (0, 0))
                    .then(|| format!("type {row}: immunity {immunity}%, resist {resist}"))
            })
            .collect();
        let versus: Vec<String> = sheet
            .weapons
            .iter()
            .flatten()
            .filter_map(|weapon| weapon.versus)
            .map(|versus| {
                let roll = versus.damage;
                format!(
                    "{:+} to hit, {}d{}+{} type {} vs {:?}",
                    versus.attack,
                    roll.dice,
                    roll.die,
                    roll.bonus,
                    versus.damage_type,
                    versus.against
                )
            })
            .collect();
        if !guards.is_empty() || !versus.is_empty() {
            outln!(
                "      race {}; guards: {}; versus: {}",
                sheet.race,
                if guards.is_empty() {
                    "none".to_string()
                } else {
                    guards.join(", ")
                },
                if versus.is_empty() {
                    "none".to_string()
                } else {
                    versus.join(", ")
                }
            );
        }
        rows.push(serde_json::json!({
            "index": index, "tag": object.tag, "template": object.template,
            "level": sheet.level, "max_hp": sheet.max_hp, "max_fp": sheet.max_fp,
            "defence": sheet.defence(), "faction": sheet.faction, "hostile": hostile,
            "weapons": weapons, "flags": sheet.flags, "race": sheet.race,
            "guards": guards, "versus": versus,
        }));
    }
    crate::runtime::write_facts(serde_json::json!({
        "verb": "combat roster", "game": game, "module": module, "creatures": rows,
    }));
    ExitCode::SUCCESS
}

/// What happened in a fight, from the snapshots' event rings.
#[derive(Default)]
struct Tally {
    /// (attacker, target) -> (hits, damage in hundredths).
    pairs: std::collections::BTreeMap<(String, String), (usize, i64)>,
    hits: usize,
    damage_centi: i64,
    by_outcome: std::collections::BTreeMap<String, usize>,
    deaths: Vec<(u64, String)>,
    /// Party members downed (I30 P11), each the first time.
    downs: Vec<(u64, String)>,
    /// Each character's saber or melee attacks as they started (I24).
    swings: std::collections::BTreeMap<String, Vec<&'static str>>,
    /// Each character's saber move in the last snapshot, by id.
    moves: std::collections::BTreeMap<u32, sim::combat::saber::Move>,
}

/// Seat 0's command: the stand-in mind against the nearest hostile it can
/// reach in `last`.
fn player_command(last: &Snapshot) -> sim::Command {
    let Some(me) = last
        .entities
        .iter()
        .find(|e| matches!(e.kind, Kind::Avatar(Seat(0))))
    else {
        return sim::Command::default();
    };
    let (Some(player), Some(fighter)) = (me.player, me.fighter) else {
        return sim::Command::default();
    };
    let target = last
        .entities
        .iter()
        .filter(|e| matches!(e.kind, Kind::Object(ObjectKind::Creature, _)))
        .filter(|e| e.fighter.as_ref().is_some_and(Fighter::alive))
        .filter(|e| e.fighter.is_some_and(|f| f.target == Some(me.id)))
        .filter_map(|e| Some((e.id, e.player?)))
        .min_by(|a, b| {
            let d = |p: &sim::motion::MotionState| {
                sim::math::length(sim::math::sub(p.origin, player.origin))
            };
            d(&a.1).total_cmp(&d(&b.1))
        });
    match target {
        Some((_, other)) if fighter.alive() => {
            let crosshair = mind::crosshair(&player);
            mind::engage(
                me.id,
                &player,
                crosshair,
                mind::chest(&other),
                mind::Attack::Ranged,
                last.time_ms,
                false,
            )
        }
        _ => mind::looking(&player, player.view[0], player.view[1]),
    }
}

/// `kjk combat fight`: installs `module` for one player arriving at
/// `arrive` (creatures tagged `hostile` and `allies` turned so first, the
/// module's `scripts` queued for the player's first step), steps `seconds`
/// of fighting (the player shooting back at whoever targets it), tallies
/// what the event rings recorded, and replays the recording.
pub fn fight(
    discovery: &Discovery,
    game: &str,
    module: &str,
    arrive: Option<&str>,
    seconds: u64,
    (hostile, allies): (&[String], &[String]),
    scripts: &[String],
) -> ExitCode {
    let installed = crate::scenes::installed(discovery, game, module, arrive, hostile, allies)
        .and_then(|mut world| {
            crate::by_hand::queue_scripts(&mut world, scripts)?;
            Ok(world)
        });
    let mut world = match installed {
        Ok(world) => world,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    let rate = TickRate::default();
    let mut last = world.snapshot();
    let mut recorder = Recorder::new(rate.hz(), last.clone(), world.level().id(), 100)
        .with_script(world.script_state().clone());
    let mut tally = Tally::default();
    // Events already counted, by (character, sequence number).
    let mut seen = std::collections::BTreeSet::new();
    let started = std::time::Instant::now();
    while rate.time_ms(last.tick) < seconds * 1000 {
        let tick = last.tick + 1;
        let mut input = TickInput::default();
        input.insert(Seat(0), player_command(&last));
        last = step(
            &mut world,
            tick,
            &input,
            rate.msec(tick),
            StepReason::Authority,
        );
        recorder.record(rate.msec(tick), &input, &last);
        let objects = sim::Instances::in_snapshot(world.level(), &last);
        for entity in &last.entities {
            let Some(fighter) = entity.fighter else {
                continue;
            };
            for event in fighter.events.iter().flatten() {
                if !seen.insert((entity.id.0, event.sequence)) {
                    continue;
                }
                tally.hits += 1;
                tally.damage_centi += i64::from(event.damage);
                let by = event
                    .by
                    .and_then(|by| last.get(by))
                    .map_or_else(|| "world".to_string(), |by| name(by, &objects));
                let pair = tally
                    .pairs
                    .entry((by, name(entity, &objects)))
                    .or_insert((0, 0));
                pair.0 += 1;
                pair.1 += i64::from(event.damage);
                *tally
                    .by_outcome
                    .entry(format!("{:?}", event.result))
                    .or_insert(0) += 1;
            }
            for (fallen, list) in [
                (fighter.dead(), &mut tally.deaths),
                (fighter.downed(), &mut tally.downs),
            ] {
                if fallen && !list.iter().any(|(_, who)| *who == label(entity)) {
                    list.push((last.time_ms, label(entity)));
                }
            }
            let swing = fighter.saber.current_move;
            if tally.moves.insert(entity.id.0, swing) != Some(swing) && swing.attack() {
                tally
                    .swings
                    .entry(name(entity, &objects))
                    .or_default()
                    .push(swing.name());
            }
        }
    }
    let step_seconds = started.elapsed().as_secs_f64();
    let recording = recorder.finish(&last);
    let replayed = recording.play(world.level());
    let same = replayed
        .as_ref()
        .is_ok_and(|end| end.hash() == recording.end);
    let alive = |kind: fn(&Kind) -> bool| {
        last.entities
            .iter()
            .filter(|e| kind(&e.kind) && e.fighter.is_some_and(|f| f.alive()))
            .count()
    };
    let player = last
        .entities
        .iter()
        .find(|e| matches!(e.kind, Kind::Avatar(Seat(0))))
        .and_then(|e| e.fighter);
    outln!(
        "combat fight {module}: {seconds} s ({} ticks) stepped in {step_seconds:.2} s",
        last.tick
    );
    let listed = |list: &[(u64, String)]| {
        list.iter()
            .map(|(ms, who)| format!("{who} at {ms} ms"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    outln!(
        "  hits {} ({:?}), {:.2} HP dealt; deaths: {}; downed: {}",
        tally.hits,
        tally.by_outcome,
        tally.damage_centi as f64 / 100.0,
        listed(&tally.deaths),
        listed(&tally.downs)
    );
    outln!(
        "  the player ends with {:.2} HP; creatures alive: {}; bolts in flight: {}",
        player.map_or(0.0, |f| f.hp as f64 / 100.0),
        alive(|kind| matches!(kind, Kind::Object(ObjectKind::Creature, _))),
        last.entities
            .iter()
            .filter(|e| e.kind == Kind::Missile)
            .count()
    );
    for ((by, to), (hits, damage)) in &tally.pairs {
        outln!(
            "    {by:>24} -> {to:<24} {hits:>4} hits {:>7.2} HP",
            *damage as f64 / 100.0
        );
    }
    for (who, swings) in &tally.swings {
        let shown: Vec<&str> = swings.iter().take(8).copied().collect();
        outln!(
            "    {who:>24} swung {} times: {}{}",
            swings.len(),
            shown.join(", "),
            if swings.len() > shown.len() {
                ", …"
            } else {
                ""
            }
        );
    }
    outln!(
        "  recording ({} ticks) replayed: {}",
        recording.ticks.len(),
        if same { "same" } else { "DIFFERENT" }
    );
    crate::runtime::write_facts(serde_json::json!({
        "verb": "combat fight", "game": game, "module": module, "arrive": arrive,
        "hostile": hostile, "allies": allies, "run": scripts,
        "seconds": seconds, "ticks": last.tick, "step_seconds": step_seconds,
        "hits": tally.hits, "damage_hp": tally.damage_centi as f64 / 100.0,
        "outcomes": tally.by_outcome,
        "deaths": tally.deaths.iter().map(|(ms, who)| serde_json::json!({"ms": ms, "who": who})).collect::<Vec<_>>(),
        "downs": tally.downs.iter().map(|(ms, who)| serde_json::json!({"ms": ms, "who": who})).collect::<Vec<_>>(),
        "player_hp": player.map(|f| f.hp as f64 / 100.0),
        "swings": tally.swings,
        "replay_same": same, "end": recording.end.to_string(),
    }));
    if same {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// A character by its tag (a creature) or seat.
fn name(entity: &sim::EntityState, objects: &sim::Instances) -> String {
    match entity.kind {
        Kind::Object(_, index) => objects
            .tag(index)
            .map_or_else(|| format!("#{index}"), |tag| format!("{tag}#{index}")),
        _ => label(entity),
    }
}

fn label(entity: &sim::EntityState) -> String {
    match entity.kind {
        Kind::Avatar(seat) => format!("player {}", seat.0),
        Kind::Object(_, index) => format!("creature #{index}"),
        _ => format!("entity {}", entity.id.0),
    }
}
