//! What creatures perceive, at KOTOR's cadence, and stealth's losses.

use super::*;

/// How often a creature whose AI is active perceives (ms; KOTOR's
/// cadence, drawn each time: the least and the spread), and one whose is
/// not.
pub(super) const PERCEIVE_LEAST_MS: u64 = rules::stealth::perception::ACTIVE_LEAST_MS;
pub(super) const PERCEIVE_SPREAD_MS: u32 = rules::stealth::perception::ACTIVE_SPREAD_MS;
pub(super) const PERCEIVE_IDLE_MS: u64 = rules::stealth::perception::IDLE_MS;

/// Each creature's perception when its moment comes this step (see the
/// module documentation); its notice events, hearing before sight. A
/// creature whose AI is active (living, not held by a conversation)
/// perceives at `PERCEIVE_LEAST_MS` and up to `PERCEIVE_SPREAD_MS` more,
/// another every `PERCEIVE_IDLE_MS` (KOTOR's). A stealthed body is heard
/// only past the listen check and seen only past the spot check
/// (`stealth::listen_score`, `spot_score`), with the rolls each holds for
/// `stealth::ROLL_EVERY_MS`. Sight goes all round, as xoreos has it (range alone) and the
/// Aurora engine's creatures see [verify in K1's exe]; reone gives a 120°
/// field of view, under which the Upper City's interrogation commander,
/// facing his prisoner as the scene places him, never saw the party come
/// in behind him, and his own AI (which picks only enemies it sees) never
/// fought back.
pub(super) fn perceive(
    view: &View,
    state: &mut State,
    bodies: &[Body],
    sight: &dyn Fn(Vec3, Vec3) -> bool,
    walls: &dyn Fn(Vec3, Vec3) -> f32,
    rng: &mut crate::rng::SimRng,
) {
    let now_ms = view.now_ms;
    let held = |id: u32| {
        state
            .talk
            .conversations
            .iter()
            .any(|talk| talk.owner.0 == id || talk.speaker.0 == id)
    };
    let held: Vec<u32> = bodies
        .iter()
        .map(|body| body.id)
        .filter(|id| held(*id))
        .collect();
    let mod_listen = view.module.area_stealth.mod_listen;
    let party: Vec<u32> = view
        .things
        .iter()
        .filter(|thing| thing.character.is_some_and(|c| c.party))
        .map(|thing| thing.id.0)
        .collect();
    for body in bodies.iter().filter(|body| body.alive) {
        let Some(senses) = body.senses else { continue };
        let next = state.perception.get(&body.id).map_or(0, |(next, _)| *next);
        if now_ms < next {
            continue;
        }
        let wait = if held.contains(&body.id) {
            PERCEIVE_IDLE_MS
        } else {
            PERCEIVE_LEAST_MS + u64::from(rng.below(PERCEIVE_SPREAD_MS))
        };
        let mine = rolls(state, rng, body.id, now_ms);
        state.perception.insert(body.id, (now_ms + wait, mine));
        let watcher = crate::stealth::Watcher {
            awareness: body.awareness,
            rolls: mine,
            in_combat: body.in_combat,
            running: body.running,
            still: body.still,
            npc: body.npc,
            facing: body.facing,
            true_seeing: body.traits & crate::combat::gear::traits::TRUE_SEEING != 0,
        };
        let range = if body.in_combat {
            senses.sight.max(view.module.ranges.party_combat_m)
        } else {
            senses.sight
        };
        let mut now = Perceived::default();
        // A blind one's spot and listen checks are skipped: it perceives
        // no one (TSL's `EffectBlind`).
        let others = bodies
            .iter()
            .filter(|other| other.id != body.id && !body.blind);
        for other in others {
            let d = math::sub(other.origin, body.origin);
            let metres = math::length(d) * collision::UNIT;
            let sneaker = other.stealth.map(|stealth| crate::stealth::Sneaker {
                stealth,
                rolls: rolls(state, rng, other.id, now_ms),
                still: other.still,
                player: !other.npc,
                size: other.size,
                silent: other.traits & crate::combat::gear::traits::SILENT != 0,
            });
            if metres <= senses.hearing
                && sneaker.is_none_or(|sneaker| {
                    let wall = walls(body.eye, other.chest);
                    crate::stealth::listen_score(&watcher, &sneaker, metres, mod_listen, wall) > 0
                })
            {
                now.heard.push(other.id);
            }
            if metres <= range
                && sight(body.eye, other.chest)
                && sneaker.is_none_or(|sneaker| {
                    let toward = [d[0] * collision::UNIT, d[1] * collision::UNIT];
                    crate::stealth::spot_score(&watcher, &sneaker, toward) > 0
                })
            {
                now.seen.push(other.id);
            }
        }
        let was = state
            .perceived
            .remove(&body.id)
            .unwrap_or_else(Perceived::default);
        let me = ObjectId(body.id);
        let kinds = [
            (
                &was.heard,
                &now.heard,
                perception::HEARD,
                perception::INAUDIBLE,
            ),
            (&was.seen, &now.seen, perception::SEEN, perception::VANISHED),
        ];
        for (before, after, gained, lost) in kinds {
            let changes = after
                .iter()
                .filter(|id| !before.contains(id))
                .map(|id| (*id, gained))
                .chain(
                    before
                        .iter()
                        .filter(|id| !after.contains(id))
                        .map(|id| (*id, lost)),
                );
            for (id, kind) in changes {
                // A party member newly seen costs the area's stealth XP
                // its loss in `STEALTH_LOSS_MS` (`stealth_loss`).
                if kind == perception::SEEN
                    && foe_of_party(view, state, body.id, id, &party)
                    && state.stealth_xp(view.module).enabled
                {
                    state
                        .stealth_loss
                        .entry(body.id)
                        .or_insert(now_ms + STEALTH_LOSS_MS);
                }
                raise_with(view, state, me, Hook::Notice, Some(ObjectId(id)), kind);
            }
        }
        if now != Perceived::default() {
            state.perceived.insert(body.id, now);
        }
    }
}

/// Whether `observer` seeing `seen` costs stealth XP: `seen` is a party
/// member and `observer` one hostile to it from outside the party (not a
/// companion nor a friendly passer-by).
pub(super) fn foe_of_party(
    view: &View,
    state: &State,
    observer: u32,
    seen: u32,
    party: &[u32],
) -> bool {
    let faction = |id: u32| {
        view.thing(ObjectId(id))
            .and_then(|thing| thing.character)
            .map(|character| character.faction)
    };
    party.contains(&seen)
        && !party.contains(&observer)
        && faction(observer)
            .zip(faction(seen))
            .is_some_and(|(a, b)| view.module.repute.with(&state.reputation).hostile(a, b))
}

/// How long after a creature newly sees a party member the area's stealth
/// XP loses its loss (ms).
pub(super) const STEALTH_LOSS_MS: u64 = rules::stealth::stealth_xp::LOSS_AFTER_MS;

/// The creatures that saw the party: once their time is up, or at once
/// when they turn on a party member, the area's stealth XP loses its loss
/// (while still to be awarded).
pub(super) fn stealth_loss(view: &View, state: &mut State) {
    if state.stealth_loss.is_empty() {
        return;
    }
    let attacking = |id: u32| {
        view.thing(ObjectId(id))
            .and_then(|thing| thing.character)
            .and_then(|character| character.target)
            .and_then(|target| view.thing(target))
            .is_some_and(|target| target.character.is_some_and(|c| c.party))
    };
    let due: Vec<u32> = state
        .stealth_loss
        .iter()
        .filter(|(id, at)| view.now_ms >= **at || attacking(**id))
        .map(|(id, _)| *id)
        .collect();
    for id in due {
        state.stealth_loss.remove(&id);
        let xp = state.stealth_xp(view.module);
        if xp.enabled {
            xp.current = (xp.current - xp.loss).max(0);
        }
    }
}

/// Whether `observer` perceives `target` now (sees or hears it).
pub(crate) fn perceived(state: &State, observer: u32, target: u32) -> bool {
    state.perceived.get(&observer).is_some_and(|perceived| {
        perceived.seen.contains(&target) || perceived.heard.contains(&target)
    })
}

/// The perception rolls `id` holds now, drawn again when due (kept with
/// its next look, `State::perception`).
pub(super) fn rolls(
    state: &mut State,
    rng: &mut crate::rng::SimRng,
    id: u32,
    now_ms: u64,
) -> crate::stealth::Rolls {
    let entry = state.perception.entry(id).or_default();
    if entry.1.due(now_ms) {
        entry.1 = crate::stealth::Rolls::draw(rng, now_ms);
    }
    entry.1
}
