//! `kjk combat ai`: the minds' evidence without a window (I27). A module is
//! installed for one player with creatures turned hostile and others made
//! the player's companions (`Mind::companion`); the player walks toward
//! the nearest hostile it has a way to (a `sim::walk::Walker`, holding use
//! through doors), shooting back at whoever has it in sight; everyone else
//! is the minds'. Every change of a mind's task, target, waypoint or held
//! power is logged (the decision dump, `decisions.tsv` in the run folder;
//! the first lines printed), with checks: companions follow and fight,
//! stay near their leader, the hostiles engage, and the recording replays.
//! Writes `facts.json`.

use std::fmt::Write as _;
use std::process::ExitCode;

use sim::combat::ai::{Mind, Task};
use sim::combat::{Fighter, mind};
use sim::module::ObjectKind;
use sim::{Instances, Kind, Recorder, Seat, SimWorld, StepReason, TickInput, TickRate, step};
use vfs::Discovery;

/// What a mind showed at one step, compared from step to step.
#[derive(PartialEq)]
struct Shown {
    task: Task,
    target: Option<u32>,
    hold: u16,
    walking: bool,
}

fn shown(fighter: &Fighter, now: u64) -> Shown {
    Shown {
        task: fighter.mind.task,
        target: fighter.target.map(|id| id.0),
        hold: if fighter.mind.hold_until > now {
            fighter.mind.hold
        } else {
            0
        },
        walking: fighter.mind.walker.is_some(),
    }
}

/// A character's name for the log: its tag and index, or its seat.
fn name(objects: &Instances, entity: &sim::EntityState) -> String {
    match entity.kind {
        Kind::Object(_, index) => objects
            .tag(index)
            .map_or_else(|| format!("#{index}"), |tag| format!("{tag}#{index}")),
        Kind::Avatar(seat) => format!("player {}", seat.0),
        _ => format!("entity {}", entity.id.0),
    }
}

/// `kjk combat ai`.
pub fn ai(
    discovery: &Discovery,
    game: &str,
    module: &str,
    arrive: Option<&str>,
    seconds: u64,
    hostile: &[String],
    companions: &[String],
) -> ExitCode {
    let mut world =
        match crate::scenes::installed(discovery, game, module, arrive, hostile, companions) {
            Ok(world) => world,
            Err(error) => {
                eprintln!("{error}");
                return ExitCode::FAILURE;
            }
        };
    let level = world.level().clone();
    // A creature with one of `tags`, among `objects` (its snapshot's).
    let tagged = |objects: &Instances, entity: &sim::EntityState, tags: &[String]| {
        objects.of_entity(entity).is_some_and(|object| {
            object.kind == ObjectKind::Creature
                && tags.iter().any(|tag| tag.eq_ignore_ascii_case(&object.tag))
        })
    };
    // Companions follow seat 0 and start beside it, as a party arrives.
    let mut start = world.snapshot();
    let me = crate::scenes::seat0(&start);
    let leader = start.entities[me].player.expect("a player");
    // Standing room over the navigation mesh with a way to the leader.
    let free = |at: [f32; 3]| {
        let half = sim::motion::HALF_WIDTH * collision::UNIT;
        let extents = collision::Extents {
            mins: [-half, -half, sim::motion::SOLE_Z * collision::UNIT],
            maxs: [half, half, 40.0 * collision::UNIT],
        };
        let room = !level
            .with(None, |world| {
                let at = sim::math::scale(at, collision::UNIT);
                world.trace(at, at, &extents, collision::Contents::SOLID)
            })
            .start_solid;
        let routed =
            sim::walk::route(&level, leader.origin, at, &|_| sim::ObjectState::default()).is_some();
        room && routed
    };
    // Two rings round the player, behind it first.
    let mut spots = [80.0f32, 140.0]
        .into_iter()
        .flat_map(|reach| (0..12u8).map(move |step| (reach, 180.0 + f32::from(step) * 30.0)))
        .map(|(reach, turn)| {
            let (sin, cos) = sim::math::sin_cos_deg(leader.view[1] + turn);
            let [x, y, z] = leader.origin;
            [x + reach * cos, y + reach * sin, z]
        })
        .filter(|at| free(*at));
    let mut placed = 0usize;
    let objects = Instances::in_snapshot(&level, &start);
    for entity in &mut start.entities {
        if tagged(&objects, entity, companions)
            && let Some(fighter) = entity.fighter.as_mut()
        {
            fighter.mind = Mind::companion(Seat(0));
            if let Some(spot) = spots.next() {
                entity.player = Some(sim::motion::MotionState::standing(spot, leader.view[1]));
                placed += 1;
            }
        }
    }
    outln!("  {placed} companion(s) placed beside the player");
    let world_start = start.clone();
    let mut world = SimWorld::from_snapshot(&start, level.clone()).expect("the scene starts");
    let states_of = crate::scenes::door_states;
    // The player's goal: the nearest hostile it has a way to.
    let player0 = start.entities[me].player.expect("a player");
    let goal = start
        .entities
        .iter()
        .filter(|entity| tagged(&objects, entity, hostile))
        .filter_map(|entity| {
            let at = entity.player?.origin;
            let route = sim::walk::route(&level, player0.origin, at, &states_of(&start))?;
            let mut length = 0.0;
            let mut from = player0.origin;
            for corner in &route {
                length += collision::nav::flat_length(from, *corner);
                from = *corner;
            }
            Some((length, at))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, at)| at);
    let rate = TickRate::default();
    let mut recorder = Recorder::new(rate.hz(), start.clone(), level.id(), 100);
    let mut walk = goal.map(sim::walk::Walker::new);
    let mut last = start;
    let mut previous: Vec<Option<Shown>> = last.entities.iter().map(|_| None).collect();
    let mut log = String::new();
    let mut printed = 0;
    let (mut casts, mut companion_hits, mut hostile_engaged, mut farthest) =
        (0usize, 0usize, 0usize, 0.0f32);
    let (mut followed, mut engaged) = (false, false);
    let mut seen_events: std::collections::BTreeSet<(u32, u16)> = std::collections::BTreeSet::new();
    while rate.time_ms(last.tick) < world_start.time_ms + seconds * 1000 {
        let tick = last.tick + 1;
        let player = last.entities[me].player.expect("a player");
        // Shoot back at whoever has the player in sight; else walk on.
        let threat = last
            .entities
            .iter()
            .filter(|entity| {
                entity
                    .fighter
                    .is_some_and(|f| f.alive() && f.target == Some(last.entities[me].id))
            })
            .filter_map(|entity| entity.player)
            .min_by(|a, b| {
                let d = |p: &sim::motion::MotionState| {
                    sim::math::length(sim::math::sub(p.origin, player.origin))
                };
                d(a).total_cmp(&d(b))
            });
        let command = match (threat, walk.as_mut()) {
            (Some(other), _) => mind::engage(
                last.entities[me].id,
                &player,
                mind::crosshair(&player),
                mind::chest(&other),
                mind::Attack::Ranged,
                last.time_ms,
                false,
            ),
            (None, Some(walker)) => {
                let mut command = walker.command(&level, &player, last.time_ms, &states_of(&last));
                command.buttons |= sim::buttons::USE;
                command
            }
            (None, None) => mind::looking(&player, player.view[0], player.view[1]),
        };
        let mut input = TickInput::default();
        input.insert(Seat(0), command);
        let msec = rate.msec(tick);
        last = step(&mut world, tick, &input, msec, StepReason::Authority);
        recorder.record(msec, &input, &last);
        let leader = last.entities[me].player.expect("a player").origin;
        let objects = Instances::in_snapshot(&level, &last);
        for (at, entity) in last.entities.iter().enumerate() {
            let (Some(fighter), Kind::Object(ObjectKind::Creature, _)) =
                (entity.fighter, entity.kind)
            else {
                continue;
            };
            if !fighter.alive() {
                continue;
            }
            let now_shown = shown(&fighter, last.time_ms);
            if previous.get(at).and_then(Option::as_ref) != Some(&now_shown) {
                let target = now_shown
                    .target
                    .and_then(|id| last.get(sim::SimId(id)))
                    .map_or_else(|| "-".to_string(), |other| name(&objects, other));
                let line = format!(
                    "{:.2}\t{}\t{:?}\t{}\t{}\t{}",
                    f64::from((last.time_ms - world_start.time_ms) as u32) / 1000.0,
                    name(&objects, entity),
                    now_shown.task,
                    target,
                    if now_shown.walking { "walking" } else { "-" },
                    if now_shown.hold != 0 {
                        format!("holds {:#x}", now_shown.hold)
                    } else {
                        "-".into()
                    }
                );
                if printed < 40 {
                    outln!("    {line}");
                    printed += 1;
                }
                let _ = writeln!(log, "{line}");
                if at < previous.len() {
                    previous[at] = Some(now_shown);
                }
            }
            // Casts: its pool fell, or it pressed the power button.
            if fighter.last_command.buttons & sim::buttons::FORCE_POWER != 0 {
                casts += 1;
            }
            let companion = fighter.mind.leader.is_some();
            if companion {
                if let Some(player) = entity.player
                    && last.time_ms >= world_start.time_ms + 3000
                {
                    let apart = collision::nav::flat_length(player.origin, leader);
                    farthest = farthest.max(apart);
                }
                followed |= fighter.mind.task == Task::Follow;
                engaged |= fighter.mind.task == Task::Engage;
            } else if tagged(&objects, entity, hostile) && fighter.mind.task == Task::Engage {
                hostile_engaged += 1;
            }
        }
        // Hits companions dealt, from the event rings.
        for entity in &last.entities {
            let Some(fighter) = entity.fighter else {
                continue;
            };
            for event in fighter.events.iter().flatten() {
                if !seen_events.insert((entity.id.0, event.sequence)) {
                    continue;
                }
                let by_companion = event
                    .by
                    .and_then(|by| last.get(by))
                    .and_then(|by| by.fighter)
                    .is_some_and(|by| by.mind.leader.is_some());
                if by_companion && event.damage > 0 {
                    companion_hits += 1;
                }
            }
        }
    }
    let objects = Instances::in_snapshot(&level, &last);
    for entity in last
        .entities
        .iter()
        .filter(|entity| tagged(&objects, entity, companions))
    {
        let (Some(player), Some(fighter)) = (entity.player, entity.fighter) else {
            continue;
        };
        let nearest = last
            .entities
            .iter()
            .filter(|other| {
                tagged(&objects, other, hostile) && other.fighter.is_some_and(|f| f.alive())
            })
            .filter_map(|other| other.player)
            .map(|other| sim::math::length(sim::math::sub(other.origin, player.origin)))
            .min_by(f32::total_cmp);
        outln!(
            "  {} ends at {:?} ({:?}, {:.1} HP); {}",
            name(&objects, entity),
            player.origin.map(f32::round),
            fighter.mind.task,
            f64::from(fighter.hp) / 100.0,
            nearest.map_or_else(
                || "no hostile left alive".to_string(),
                |d| format!("the nearest living hostile {d:.0} units away")
            )
        );
    }
    let recording = recorder.finish(&last);
    let replayed = recording
        .play(&level)
        .is_ok_and(|end| end.hash() == recording.end);
    let alive_hostiles = last
        .entities
        .iter()
        .filter(|entity| {
            tagged(&objects, entity, hostile) && entity.fighter.is_some_and(|f| f.alive())
        })
        .count();
    outln!(
        "combat ai {module}: {seconds} s; companions {:?} followed {followed}, engaged {engaged}, landed {companion_hits} hits, strayed at most {:.0} units from the player; hostiles engaged {hostile_engaged} steps, {alive_hostiles} of them alive at the end; {casts} power presses; replayed: {}",
        companions,
        farthest,
        if replayed { "same" } else { "DIFFERENT" }
    );
    let folder = crate::collision_verbs::write_facts(serde_json::json!({
        "verb": "combat ai", "game": game, "module": module, "seconds": seconds,
        "hostile": hostile, "companions": companions,
        "followed": followed, "engaged": engaged, "companion_hits": companion_hits,
        "farthest": farthest, "hostile_engaged_steps": hostile_engaged,
        "hostiles_alive": alive_hostiles, "casts": casts, "replayed": replayed,
    }));
    if let Some(folder) = folder {
        let _ = std::fs::write(
            folder.join("decisions.tsv"),
            format!("seconds\tcharacter\ttask\ttarget\twalk\tholds\n{log}"),
        );
    }
    let pass = replayed
        && (companions.is_empty()
            || (followed && engaged && farthest <= sim::combat::ai::LEASH + 256.0));
    if pass {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
