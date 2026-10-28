//! Falling, getting up and reviving (`docs/FALLING.md`). At 0 HP a party
//! member is downed, not dead (`combat::damage::apply`). Each step on
//! the authority, after the step's hits (`falls`), in order:
//!
//! 1. one down that left the party gets up at once;
//! 2. the whole party down is the loss (`Snapshot::lost`; a script's
//!    `EndGame` with its death screen is one too), final until the
//!    world is replaced;
//! 3. the revives chosen in the target block (`Action::Revive`) begin on
//!    their bodies;
//! 4. the revives under way go on, finish or break off;
//! 5. new revives start: a seat's acting body holding use with a downed
//!    party member ahead in reach, the nearest;
//! 6. out of combat (`super::InCombat`), the downed get up with `RISE_HP`.

use bevy_ecs::prelude::*;

use super::{Avatar, Clock, Fighting, Id, Object, Player, PlayerSheet};
use super::{Inputs, Static, containers, in_party, party, sheet_of};
use crate::action::Action;
use crate::combat::fighter::{Fighter, Revive};
use crate::combat::sheet::{races, skills};
use crate::input::{Seat, TickInput, buttons};
use crate::math::{self, scale};
use crate::motion::{self, Anim, AnimTable, MotionState};
use crate::notice::{Notice, Notices};
use crate::snapshot::SimId;

/// When the whole party went down (sim ms): final until the world is
/// replaced (a reload, `Snapshot::lost`).
#[derive(Resource, Default)]
pub(crate) struct Lost(pub Option<u64>);

/// The seats whose use this step is a revive's (bit n for seat n):
/// `interact` uses nothing for them.
#[derive(Resource, Default)]
pub(crate) struct Reviving(pub u64);

/// How long an untrained player holds use to revive (ms).
pub const REVIVE_MS: u32 = rules::downed::revive::MS;
/// What each rank of Treat Injury (Repair, reviving a droid) takes off it.
pub const REVIVE_RANK_MS: u32 = rules::downed::revive::RANK_MS;
/// The shortest revive.
pub const REVIVE_MIN_MS: u32 = rules::downed::revive::LEAST_MS;
/// What the downed get up with, in hundredths.
pub const RISE_HP: i32 = rules::downed::rising::RISE_HP;
/// The least time down before the fight's end gets one up.
pub const DOWN_MIN_MS: u64 = rules::downed::rising::DOWN_MIN_MS;

/// A character's row as `falls` reads it.
type Row<'a> = (
    &'a Id,
    Option<&'a Avatar>,
    Option<&'a Object>,
    Option<&'a PlayerSheet>,
    Mut<'a, Player>,
    Mut<'a, Fighting>,
);

/// See the module documentation. On the authority, after `hurt`.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn falls(
    clock: Res<Clock>,
    inputs: Res<Inputs>,
    level: Res<Static>,
    in_combat: Res<super::InCombat>,
    scripts: Res<super::scripts::Scripts>,
    mut notices: ResMut<Notices>,
    (mut lost, mut reviving): (ResMut<Lost>, ResMut<Reviving>),
    mut actors: Query<(
        &Id,
        Option<&Avatar>,
        Option<&Object>,
        Option<&PlayerSheet>,
        &mut Player,
        &mut Fighting,
    )>,
) {
    reviving.0 = 0;
    let now = clock.time_ms;
    let mut rows: Vec<Row> = actors.iter_mut().collect();
    rows.sort_by_key(|(id, ..)| id.0);
    let party: Vec<usize> = (0..rows.len())
        .filter(|&at| in_party(rows[at].1, &rows[at].5.0))
        .collect();
    let anims = &level.0.anims;
    // One down that left the party gets up at once: only the party is
    // revived and gets up.
    for at in (0..rows.len()).filter(|at| !party.contains(at)) {
        let (.., player, fighting) = &mut rows[at];
        if fighting.0.downed() {
            stand(&mut player.0, &mut fighting.0, anims);
        }
    }
    if let Some(arena) = level.0.arena_rules() {
        arena_falls(arena, &mut rows, &party, now);
        return;
    }
    if lost.0.is_none() && !party.is_empty() && party.iter().all(|&at| !rows[at].5.0.alive()) {
        lost.0 = Some(now);
    }
    if lost.0.is_some() {
        // Nobody revives the lost, and nobody gets up.
        for &at in &party {
            if let Some(fallen) = &mut rows[at].5.0.fallen {
                fallen.revive = None;
            }
        }
        return;
    }
    let seats = acting(&rows, super::lent(&scripts.state.party));
    chosen(&mut rows, &seats, &inputs.0, now, &mut notices.0);
    revive(&mut rows, &seats, anims, now, &mut reviving.0);
    if !in_combat.0 {
        for &at in &party {
            let (.., player, fighting) = &mut rows[at];
            let due = fighting
                .0
                .fallen
                .is_some_and(|fallen| now >= fallen.since_ms + DOWN_MIN_MS);
            if due {
                stand(&mut player.0, &mut fighting.0, anims);
            }
        }
    }
}

/// An arena's falls (`super::arena`): a body under the pit's floor dies
/// there (the party's goes down), and the party's down come back whole at
/// the respawn point once they have lain `respawn_ms`.
fn arena_falls(arena: &super::arena::Rules, rows: &mut [Row], party: &[usize], now: u64) {
    let pit = arena.pit / collision::UNIT;
    for (at, (.., player, fighting)) in rows.iter_mut().enumerate() {
        let fighter = &mut fighting.0;
        if player.0.origin[2] >= pit || !fighter.alive() {
            continue;
        }
        if party.contains(&at) {
            fighter.fall(now);
        } else {
            fighter.hp = 0;
            fighter.life = crate::combat::Life::Dead;
            fighter.fallen = None;
        }
        player.0.flags |= motion::motion_flags::DEAD;
    }
    for &at in party {
        let (_, _, object, sheet, player, fighting) = &mut rows[at];
        let due = fighting
            .0
            .fallen
            .is_some_and(|fallen| now >= fallen.since_ms + arena.respawn_ms);
        if !due {
            continue;
        }
        let sheet = sheet_of(*sheet, *object);
        let fighter = &mut fighting.0;
        fighter.rise(i32::from(sheet.max_hp) * 100);
        fighter.fp = i32::from(sheet.pool()) * 100;
        fighter.force.stop();
        // Standing on the point: its box's origin over the soles.
        let mut origin = scale(arena.respawn, 1.0 / collision::UNIT);
        origin[2] += 0.125 - motion::SOLE_Z;
        player.0 = MotionState::standing(origin, arena.yaw);
    }
}

/// Up again (I30 P11): `RISE_HP`, the fall's pose given up for JA's
/// get-up.
fn stand(player: &mut MotionState, fighter: &mut Fighter, anims: &AnimTable) {
    fighter.rise(RISE_HP);
    player.flags &= !motion::motion_flags::DEAD;
    motion::react(player, anims, Anim::BOTH_GETUP1, false);
}

/// The buttons that break a revive off: any action.
pub const ACTING: u16 = buttons::ATTACK
    | buttons::ALT_ATTACK
    | buttons::FORCE_GRIP
    | buttons::FORCE_POWER
    | buttons::FORCE_LIGHTNING
    | buttons::FORCE_DRAIN
    | buttons::GRENADE
    | buttons::MEDPAC
    | buttons::SHIELD
    | buttons::STIM;

/// Whether `reviver` holds a revive this step, by the command it acted on
/// (`Fighter::last_command`: none while stunned): use held (`USE_HELD`:
/// the target block's hold, or `--moves`' `use`), standing still, no
/// action, not knocked down. `USE` alone holds nothing.
fn holding(reviver: &Fighter) -> bool {
    let command = &reviver.last_command;
    reviver.alive()
        && !reviver.saber.floored()
        && command.buttons & buttons::USE_HELD != 0
        && command.buttons & ACTING == 0
        && (command.forward, command.right, command.up) == (0, 0, 0)
}

/// How far across the floor (metres) `downed` lies ahead of `reviver`,
/// when within reach (`containers::ahead`).
fn ahead(reviver: &MotionState, downed: &MotionState) -> Option<f32> {
    let (forward, _, _) = math::angle_vectors(reviver.view);
    let me = scale(reviver.origin, collision::UNIT);
    let lies = scale(downed.origin, collision::UNIT);
    containers::ahead(me, [forward[0], forward[1]], lies)
}

/// Each seat's acting body (`super::acting_body`: its avatar, the
/// companion it drives, or the companion leading) by its row, in seat
/// order.
fn acting(rows: &[Row], lent: Option<SimId>) -> Vec<(Seat, usize)> {
    let mut seats: Vec<(Seat, usize)> = rows
        .iter()
        .filter_map(|(id, avatar, ..)| {
            let seat = (*avatar)?.0;
            let body = super::acting_body(seat, id.0, lent);
            let at = rows.binary_search_by_key(&body, |(id, ..)| id.0).ok()?;
            Some((seat, at))
        })
        .collect();
    seats.sort();
    seats.dedup_by_key(|(seat, _)| *seat);
    seats
}

/// The revive by the body at row `by` of the downed at row `at`, begun
/// now unless one is under way: as long as the reviver's Treat Injury
/// (Repair, for a droid) has it.
fn begin(rows: &mut [Row], by: usize, at: usize, now: u64) {
    let droid = sheet_of(rows[at].3, rows[at].2).race == races::DROID;
    let skill = if droid {
        skills::REPAIR
    } else {
        skills::TREAT_INJURY
    };
    let rank = sheet_of(rows[by].3, rows[by].2).skill(skill).clamp(0, 255) as u8;
    let needs_ms = revive_ms(rank);
    let reviver = rows[by].0.0;
    if let Some(fallen) = &mut rows[at].5.0.fallen
        && fallen.revive.is_none()
    {
        fallen.revive = Some(Revive {
            by: reviver,
            from_ms: now,
            needs_ms,
        });
    }
}

/// The revives chosen in the target block (`Action::Revive`), in the
/// order sent: each begins on its body, the seat's body letting go of any
/// other it revives, when the body is a downed party member ahead in reach
/// that no other revives; else the seat is told why not.
fn chosen(
    rows: &mut [Row],
    seats: &[(Seat, usize)],
    inputs: &TickInput,
    now: u64,
    notices: &mut Vec<(Seat, Notice)>,
) {
    for (seat, action) in inputs.actions() {
        let Action::Revive(body) = action else {
            continue;
        };
        let Some(&(_, by)) = seats.iter().find(|(own, _)| own == seat) else {
            continue;
        };
        let reviver = rows[by].0.0;
        let at = rows.binary_search_by_key(body, |(id, ..)| id.0).ok();
        let refused = match at {
            Some(at) if rows[at].5.0.downed() => {
                let other = rows[at]
                    .5
                    .0
                    .fallen
                    .and_then(|fallen| fallen.revive)
                    .is_some_and(|revive| revive.by != reviver);
                if other {
                    Some(Notice::CannotRevive)
                } else if !rows[by].5.0.alive() || ahead(&rows[by].4.0, &rows[at].4.0).is_none() {
                    Some(Notice::OutOfReach)
                } else {
                    None
                }
            }
            _ => Some(Notice::CannotRevive),
        };
        match (refused, at) {
            (Some(notice), _) => notices.push((*seat, notice)),
            (None, Some(at)) => {
                for (id, .., fighting) in rows.iter_mut() {
                    if let Some(fallen) = &mut fighting.0.fallen
                        && id.0 != *body
                        && fallen.revive.is_some_and(|revive| revive.by == reviver)
                    {
                        fallen.revive = None;
                    }
                }
                begin(rows, by, at, now);
            }
            (None, None) => {}
        }
    }
}

/// The revives (see the module documentation, 4 and 5).
fn revive(
    rows: &mut [Row],
    seats: &[(Seat, usize)],
    anims: &AnimTable,
    now: u64,
    reviving: &mut u64,
) {
    let seat_of = |by: usize| {
        seats
            .iter()
            .find(|(_, at)| *at == by)
            .map(|(seat, _)| *seat)
    };
    // Under way: on, done or broken off.
    for at in 0..rows.len() {
        let Some(revive) = rows[at].5.0.fallen.and_then(|fallen| fallen.revive) else {
            continue;
        };
        let by = rows.binary_search_by_key(&revive.by, |(id, ..)| id.0).ok();
        let keeps = by.and_then(|by| {
            let reviver = &rows[by];
            // A hit landed since it began (one in the step it began came
            // before it: `hurt` runs first).
            let hurt = reviver
                .5
                .0
                .last_attacker
                .is_some_and(|(_, when)| when > revive.from_ms);
            let seat = seat_of(by)?;
            let on = !hurt && holding(&reviver.5.0) && ahead(&reviver.4.0, &rows[at].4.0).is_some();
            on.then_some(seat)
        });
        let (.., player, fighting) = &mut rows[at];
        match keeps {
            None => {
                if let Some(fallen) = &mut fighting.0.fallen {
                    fallen.revive = None;
                }
            }
            Some(seat) => {
                *reviving |= party::bit(seat);
                if now >= revive.from_ms + u64::from(revive.needs_ms) {
                    stand(&mut player.0, &mut fighting.0, anims);
                }
            }
        }
    }
    // New ones, in seat order: the nearest downed party member ahead.
    for &(seat, by) in seats {
        let bit = party::bit(seat);
        if *reviving & bit != 0 || !holding(&rows[by].5.0) {
            continue;
        }
        let nearest = (0..rows.len())
            .filter(|&at| rows[at].5.0.downed())
            .filter_map(|at| ahead(&rows[by].4.0, &rows[at].4.0).map(|far| (far, at)))
            .min_by(|a, b| a.0.total_cmp(&b.0));
        let Some((_, at)) = nearest else { continue };
        // Its use is spent here, revive or wait: it opens nothing behind.
        *reviving |= bit;
        begin(rows, by, at, now);
    }
}

/// How long a reviver of rank `rank` holds use (ms).
pub fn revive_ms(rank: u8) -> u32 {
    REVIVE_MS
        .saturating_sub(REVIVE_RANK_MS * u32::from(rank))
        .max(REVIVE_MIN_MS)
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
