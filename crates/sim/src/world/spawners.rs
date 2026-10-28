//! The step's UTE encounters (`crate::spawner`), on the authority, beside
//! the triggers (`world::scripts`), before the scripts run. First the
//! deaths and goings of the creatures encounters made, each counted once
//! (`RemoveFromActiveCreatureCount`: the last of an encounter's exhausts it
//! and signals its `OnExhausted`). Then each encounter of the area, in the
//! module's order, as the executables' `CSWSEncounter::AIUpdate` runs it:
//! - one creature of its spawn list made (`SpawnCreatureFromSpawnList`):
//!   TSL deals its spawn points round, K1 takes the one farthest from
//!   whoever last entered; placed at a safe spot near the point (`spawner::SAFE_REACH_M`),
//!   else at the encounter's own place or the first safe spot of the rings
//!   round it, else not yet (the entry waits for the next step); facing the
//!   point's way;
//! - its reset, its `ResetTime` up since it exhausted;
//! - its heartbeat, every `HEARTBEAT_AFTER_MS` while active, the first skipped;
//! - characters coming into and leaving its outline, alive or dead
//!   (`OnEntered`, `OnExit`, active or not): one coming in is its last
//!   entered and may start it (`spawner::spawn_if_appropriate`).

use script_ncs::vm::ObjectId;

use super::scripts::raise;
use crate::module::ObjectKind;
use crate::rng::SimRng;
use crate::script::{Hook, Library, Out, State, View};
use crate::spawner::{self, Body, Ground, Link, Point};
use crate::{Level, math};

/// The executables' heartbeat test: more than this since the last stamp (ms).
const HEARTBEAT_AFTER_MS: u64 = rules::encounters::running::HEARTBEAT_AFTER_MS;

/// A body without a personal space of its own is as wide as the movement
/// box (metres either side).
const BODY_RADIUS_M: f32 = collision::player::HALF_WIDTH * collision::UNIT;

/// The step's encounters (see the module documentation).
pub(super) fn step(
    view: &View,
    level: &Level,
    library: &Library,
    state: &mut State,
    out: &mut Out,
    rng: &mut SimRng,
) {
    count_losses(view, state, out);
    let encounters: Vec<ObjectId> = view
        .things
        .iter()
        .filter(|thing| thing.kind == Some(ObjectKind::Encounter))
        .map(|thing| thing.id)
        .filter(|id| spawner::of(view, *id).is_some_and(|(facts, _)| !facts.geometry.is_empty()))
        .collect();
    for id in encounters {
        make_one(view, level, library, state, out, id);
        reset(view, state, rng, id);
        heartbeat(view, state, id);
        comings(view, state, rng, id);
    }
}

/// The deaths and goings of encounters' creatures (here, their encounter
/// standing here): each counted once, the last of an encounter's
/// exhausting it; one gone forgotten.
fn count_losses(view: &View, state: &mut State, out: &Out) {
    let here = |id: u32| view.thing(ObjectId(id)).is_some();
    let made = |id: u32| out.made.iter().any(|made| made.id.0 == id);
    let mut lost = Vec::new();
    let mut gone = Vec::new();
    for (&creature, link) in &mut state.encounter_creatures {
        if !here(link.encounter) || made(creature) {
            continue;
        }
        let body = view.thing(ObjectId(creature));
        let dead = body.is_none_or(|thing| thing.hp.is_some_and(|(hp, _)| hp <= 0));
        if dead && !link.counted {
            link.counted = true;
            lost.push(link.encounter);
        }
        if body.is_none() {
            gone.push(creature);
        }
    }
    for creature in gone {
        state.encounter_creatures.remove(&creature);
    }
    for encounter in lost {
        let id = ObjectId(encounter);
        let Some((_, facts)) = spawner::of(view, id) else {
            continue;
        };
        if spawner::lost_one(state, encounter, facts.reset, view.now_ms) {
            raise(view, state, id, Hook::Exhausted, None);
        }
    }
}

/// The spawn point the next creature takes and its facing (degrees), if
/// the encounter has points: TSL's round (`spawner::Cursor`), K1's farthest
/// from whoever last entered (the first of equals). Without one who entered
/// standing here as a creature, the point is the world's origin facing +X,
/// the round still moving on (QUIRKS.md Q-0262).
fn point(
    view: &View,
    state: &mut State,
    id: ObjectId,
    points: &[Point],
) -> Option<([f32; 3], f32)> {
    if points.is_empty() {
        return None;
    }
    let spawn = state.encounters.get_mut(&id.0)?;
    let entered = spawn
        .last_entered
        .and_then(|who| view.thing(ObjectId(who)))
        .filter(|thing| thing.character.is_some())
        .map(|thing| thing.position);
    let chosen = if spawner::tsl(view.module) {
        let cursor = &mut spawn.cursor;
        if cursor.per_point == 0 {
            cursor.per_point = spawn.list.len() as i32 / points.len() as i32;
        }
        if cursor.count == cursor.per_point {
            cursor.index += 1;
            cursor.count = 0;
            if cursor.index >= points.len() as i32 {
                cursor.index = 0;
            }
        }
        cursor.count += 1;
        entered.and_then(|_| points.get(cursor.index as usize))
    } else {
        entered.and_then(|from| {
            let far = |point: &Point| math::length(math::sub(point.at, from));
            points
                .iter()
                .fold(None, |best: Option<&Point>, point| match best {
                    Some(best) if far(point) <= far(best) => Some(best),
                    _ => Some(point),
                })
        })
    };
    Some(chosen.map_or(([0.0; 3], 0.0), |point| (point.at, point.yaw())))
}

/// One creature of encounter `id`'s spawn list made, its first: linked to
/// the encounter (`GetIsEncounterCreature`), one more of its alive. A
/// template the library lacks, or no place for it, keeps the entry for the
/// next step, as the executables retry it each update.
fn make_one(
    view: &View,
    level: &Level,
    library: &Library,
    state: &mut State,
    out: &mut Out,
    id: ObjectId,
) {
    let Some((facts, encounter)) = spawner::of(view, id) else {
        return;
    };
    let Some(template) = spawner::spawn_of(state, id, encounter)
        .list
        .first()
        .map(|(template, _)| template.clone())
    else {
        return;
    };
    let point = point(view, state, id, &encounter.points);
    let Some(made) = library.creature(&template) else {
        return;
    };
    let radius = if made.personal_space > 0.0 {
        made.personal_space
    } else {
        BODY_RADIUS_M
    };
    let sole = crate::motion::SOLE_Z * collision::UNIT;
    let bodies = view
        .things
        .iter()
        .filter(|thing| thing.character.is_some())
        .map(|thing| Body {
            soles: [
                thing.position[0],
                thing.position[1],
                thing.position[2] + sole,
            ],
            radius: view
                .facts(thing.id)
                .map(|facts| facts.personal_space)
                .filter(|space| *space > 0.0)
                .unwrap_or(BODY_RADIUS_M),
        })
        .chain(out.made.iter().map(|made| {
            Body {
                soles: made.at,
                radius: Some(made.facts.personal_space)
                    .filter(|space| *space > 0.0)
                    .unwrap_or(BODY_RADIUS_M),
            }
        }))
        .collect();
    let ground = Ground {
        nav: level.nav().map(|nav| &**nav),
        bodies,
        sees: view.sees,
    };
    let place = point
        .and_then(|(at, _)| ground.safe_location(at, radius))
        .or_else(|| ground.out_of_sight(facts.position, radius));
    let Some(place) = place else {
        return;
    };
    // Without a point the creature keeps the way it was made facing
    // [verify: the executables' new creature's own].
    let yaw = point.map_or(0.0, |(_, yaw)| yaw);
    let Some(creature) = crate::script::make(library, view, state, out, &template, place, yaw)
    else {
        return;
    };
    state.encounter_creatures.insert(
        creature.0,
        Link {
            encounter: id.0,
            counted: false,
        },
    );
    let spawn = spawner::spawn_of(state, id, encounter);
    spawn.number_spawned += 1;
    spawn.list.remove(0);
}

/// Encounter `id`'s reset: with `SpawnOption` 1 and `Reset`, its
/// `ResetTime` (or a game day) up since it exhausted, while resets remain
/// (`Respawns` -1: always), it counts one, turns active and starts at once
/// if a qualifying creature stands in it; K1's pool is emptied after.
fn reset(view: &View, state: &mut State, rng: &mut SimRng, id: ObjectId) {
    let Some((_, encounter)) = spawner::of(view, id) else {
        return;
    };
    let spawn = spawner::spawn_of(state, id, encounter);
    let Some(since) = spawn.last_spawn_ms else {
        return;
    };
    if encounter.spawn_option != 1 || !encounter.reset {
        return;
    }
    let elapsed = view.now_ms.saturating_sub(since);
    let wait = u64::try_from(encounter.reset_time_s).map_or(u64::MAX, |seconds| seconds * 1000);
    let per_hour = u64::from(match view.module.minutes_per_hour {
        0 => rules::scripts::clock::MINUTES_PER_HOUR,
        minutes => minutes,
    });
    let day = 24 * per_hour * 60_000;
    if elapsed < wait && elapsed < day {
        return;
    }
    if spawn.current_spawns >= spawn.respawns && spawn.respawns != -1 {
        return;
    }
    spawn.current_spawns += 1;
    spawn.last_spawn_ms = None;
    spawn.active = true;
    spawner::spawn_if_appropriate(view, state, rng, id, None);
    if let Some(spawn) = state.encounters.get_mut(&id.0) {
        spawn.pool = 0.0;
    }
}

/// Encounter `id`'s heartbeat while active: more than `HEARTBEAT_AFTER_MS` since its last
/// stamp runs `OnHeartbeat` (not the first time, with no stamp yet) and
/// stamps it again.
fn heartbeat(view: &View, state: &mut State, id: ObjectId) {
    let Some((_, encounter)) = spawner::of(view, id) else {
        return;
    };
    let spawn = spawner::spawn_of(state, id, encounter);
    let since = spawn.heartbeat_ms;
    if !spawn.active || view.now_ms.saturating_sub(since.unwrap_or(0)) <= HEARTBEAT_AFTER_MS {
        return;
    }
    spawn.heartbeat_ms = Some(view.now_ms);
    if since.is_some() {
        raise(view, state, id, Hook::Heartbeat, None);
    }
}

/// Characters coming into and leaving encounter `id`'s outline, alive or
/// dead: one coming in is its last entered, may start it, and runs its
/// `OnEntered`; one leaving its last left, and runs its `OnExit`.
fn comings(view: &View, state: &mut State, rng: &mut SimRng, id: ObjectId) {
    let Some((facts, encounter)) = spawner::of(view, id) else {
        return;
    };
    let inside: Vec<u32> = view
        .things
        .iter()
        .filter(|thing| thing.character.is_some() && spawner::inside(facts, thing.position))
        .map(|thing| thing.id.0)
        .collect();
    let before = state.occupants.get(&id.0).cloned().unwrap_or_else(Vec::new);
    for &who in inside.iter().filter(|who| !before.contains(who)) {
        spawner::spawn_of(state, id, encounter).last_entered = Some(who);
        spawner::spawn_if_appropriate(view, state, rng, id, Some(ObjectId(who)));
        raise(view, state, id, Hook::Enter, Some(ObjectId(who)));
    }
    for &who in before.iter().filter(|who| !inside.contains(who)) {
        spawner::spawn_of(state, id, encounter).last_left = Some(who);
        raise(view, state, id, Hook::Exit, Some(ObjectId(who)));
    }
    if inside.is_empty() {
        state.occupants.remove(&id.0);
    } else {
        state.occupants.insert(id.0, inside);
    }
}
