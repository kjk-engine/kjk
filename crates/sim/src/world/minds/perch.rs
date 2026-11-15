//! What a creature does about a target that stands off the walkable floor, on
//! a crate or a piece of furniture (`pace::reach`). It never stands below the
//! target and waits. Once its target has stood there for `reach.after_ms` it
//! strikes upward if its weapon reaches the target from the floor, jumps up,
//! pushes the target off, switches to a ranged weapon, or hides.

use super::{Command, Mind, MotionState, Own, Seen, SimId, Vec3, buttons, math, mind, pace};
use crate::Level;
use crate::motion::Ground;
use crate::walk;
use pace::Reach;

/// What a creature does this step about its target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Plan {
    /// Its target is reachable: it fights as it always has.
    Floor,
    /// It strikes up from the floor beside the perch (at the spot, when the
    /// level has a mesh to find it in).
    Strike(Option<Vec3>),
    /// It jumps onto the perch, running at it from the spot.
    Jump(Vec3),
    /// It stands at the spot and pushes its target off.
    Push(Vec3),
    /// It hides from its target.
    Hide,
}

impl Plan {
    /// Whether the plan is one of a target out of reach (a Force power is
    /// then no longer held back, `pace::power_allowed`).
    pub(super) fn out_of_reach(self) -> bool {
        matches!(self, Plan::Jump(_) | Plan::Push(_) | Plan::Hide)
    }

    /// Where a fighter that strikes upward stands.
    pub(super) fn stand_at(self) -> Option<Vec3> {
        match self {
            Plan::Strike(spot) => spot,
            _ => None,
        }
    }
}

/// What a creature reads of the level and of itself when it plans.
pub(super) struct Facts<'a> {
    pub level: &'a Level,
    /// How far its weapon reaches past its own centre (units).
    pub weapon: f32,
    /// A second weapon set holds a ranged weapon.
    pub ranged_set: bool,
}

/// Whether `target` stands off the walkable floor: on a placeable, or on the
/// level where the navigation mesh has no floor. None while it is in the air.
fn perched(level: &Level, target: &Seen) -> Option<bool> {
    match target.ps.ground? {
        Ground::Entity(entity) => Some(entity & crate::world::BODY == 0),
        Ground::World => Some(!walk::on_mesh(level, target.ps.origin)),
    }
}

/// The plan for this step, after noting where the target stands. A fighter
/// that switched to its ranged weapon switches back (a request in `swaps`)
/// once its target has been on the floor for `reach.swap_back_ms`.
pub(super) fn plan(
    (mind, me): (&mut Mind, SimId),
    (ps, own, target): (&MotionState, &Own, &Seen),
    facts: &Facts,
    now: u64,
    swaps: &mut Vec<SimId>,
) -> Plan {
    if let Some(off) = perched(facts.level, target) {
        mind.pace.note_floor(!off, now);
    }
    if mind.pace.swaps_back(now) {
        swaps.push(me);
        mind.pace.swapped = false;
    }
    let plan = if own.gunner || !mind.pace.target_perched(now) {
        Plan::Floor
    } else {
        perch_plan((mind, me), (ps, own, target), facts, (now, swaps))
    };
    if plan != Plan::Hide {
        mind.pace.hide_at = None;
    }
    plan
}

fn perch_plan(
    (mind, me): (&mut Mind, SimId),
    (ps, own, target): (&MotionState, &Own, &Seen),
    facts: &Facts,
    (now, swaps): (u64, &mut Vec<SimId>),
) -> Plan {
    let spot = walk::below_perch(facts.level, ps.origin, target.ps.origin);
    // Up in the air after its jump it finishes the jump: a walk would carry it
    // through the crate's shape.
    if let Some(spot) =
        spot.filter(|_| ps.ground.is_none() && mind.pace.run_up && now < mind.pace.jump_at)
    {
        return Plan::Jump(spot);
    }
    let gap = [Some(ps.origin), spot]
        .into_iter()
        .flatten()
        .map(|at| math::length(math::sub(target.ps.origin, at)) - facts.weapon)
        .fold(f32::MAX, f32::min);
    let rise = target.ps.origin[2] - ps.origin[2];
    let how = pace::reach(
        false,
        gap,
        rise,
        own.can_jump,
        own.has_force,
        mind.pace.jump_tries,
    );
    match (how, spot) {
        (Reach::Floor, _) => Plan::Floor,
        (Reach::Blade, _) => Plan::Strike(spot),
        (Reach::Jump | Reach::ForceJump, Some(spot)) => Plan::Jump(spot),
        (Reach::Jump | Reach::ForceJump, None) => Plan::Floor,
        (Reach::Not, spot) => match pace::fallback(own.tier, own.knows_push, facts.ranged_set) {
            pace::Fallback::Push => spot.map_or(Plan::Hide, Plan::Push),
            pace::Fallback::Ranged => {
                if !mind.pace.swapped {
                    swaps.push(me);
                    mind.pace.swapped = true;
                }
                Plan::Floor
            }
            pace::Fallback::Hide => Plan::Hide,
        },
    }
}

/// A command that only faces `target`.
fn face(ps: &MotionState, target: &Seen, msec: u32) -> Command {
    let at = mind::aim(mind::eye(ps), mind::eye(&target.ps));
    let (pitch, yaw) = mind::turned(ps, at, pace::TURN_RATE, msec);
    mind::looking(ps, pitch, yaw)
}

/// A command that runs along the route to `goal`, facing the way it goes.
fn route_to(
    mind: &mut Mind,
    ps: &MotionState,
    goal: Vec3,
    (now, msec): (u64, u32),
    (level, doors): (&Level, &dyn walk::Ways),
) -> Command {
    let step = mind.walk_to(goal, 1.0).command(level, ps, now, doors);
    let looks = math::short_to_degrees(step.angles[1].wrapping_add(ps.view_offset[1]));
    let (pitch, yaw) = mind::turned(ps, (0.0, looks), pace::TURN_RATE, msec);
    let (forward, right) = walk::rebased(ps, &step, yaw);
    Command {
        forward,
        right,
        ..mind::looking(ps, pitch, yaw)
    }
}

/// How far (units) a creature backs off from a perch to run at it.
const RUN_UP: f32 = 2.5 * rules::ai::duelling::reach::JUMP_FROM;

/// How far (units) above its target's feet a creature that jumps at the perch
/// lets go of jump: it keeps rising a little after that.
const CLEARANCE: f32 = -24.0;

/// How many units a Force jump takes off farther back for each unit of the
/// perch's height above a plain jump's.
const FORCE_RUN: f32 = 1.1;

/// The share of its run speed a creature must have toward the perch to jump.
const RUNNING: f32 = 0.6;

/// A creature that jumps at the perch at `spot`: it runs at the spot, presses
/// jump within `reach.jump_from` of it with its face to the target, and backs
/// off for another run when it fails.
pub(super) fn jump(
    (mind, ps, target): (&mut Mind, &MotionState, &Seen),
    spot: Vec3,
    (now, msec): (u64, u32),
    (level, doors): (&Level, &dyn walk::Ways),
) -> Command {
    let toward = math::sub(spot, ps.origin);
    let apart = math::flat_length(toward);
    // A Force jump rises for longer before it comes over the perch: it takes
    // off that much farther back.
    let rise =
        (target.ps.origin[2] - ps.origin[2] - rules::ai::duelling::reach::JUMP_RISE).max(0.0);
    let takeoff = rules::ai::duelling::reach::JUMP_FROM + FORCE_RUN * rise;
    let mut leap = face(ps, target, msec);
    if ps.ground.is_none() {
        mind.walker = None;
        leap.forward = i8::MAX;
        // A Force jump goes on rising while jump is held: held only until it
        // is above the perch, so that it comes down on it.
        let above = ps.origin[2] > target.ps.origin[2] + CLEARANCE;
        leap.up = if ps.velocity[2] > 0.0 && !above {
            i8::MAX
        } else {
            0
        };
        return leap;
    }
    let along = math::dot(
        [ps.velocity[0], ps.velocity[1], 0.0],
        math::scale(toward, 1.0 / apart.max(f32::EPSILON)),
    );
    if mind.pace.run_up
        && apart >= takeoff + RUN_UP - rules::ai::duelling::reach::JUMP_FROM - crate::walk::REACHED
        && now >= mind.pace.jump_at
    {
        mind.pace.run_up = false;
    }
    if !mind.pace.run_up && apart <= takeoff {
        if along >= RUNNING * ps.run_speed && mind.pace.try_jump(now) {
            mind.walker = None;
            mind.pace.run_up = true;
            leap.forward = i8::MAX;
            leap.up = i8::MAX;
            return leap;
        }
        mind.pace.run_up = true;
    }
    if mind.pace.run_up {
        let away = math::scale(toward, -1.0 / apart.max(f32::EPSILON));
        let run_up = takeoff + RUN_UP - rules::ai::duelling::reach::JUMP_FROM;
        let back = math::mad(spot, run_up, [away[0], away[1], 0.0]);
        if apart >= run_up - crate::walk::REACHED {
            mind.walker = None;
            return leap;
        }
        return route_to(mind, ps, back, (now, msec), (level, doors));
    }
    route_to(mind, ps, spot, (now, msec), (level, doors))
}

/// A creature at the spot below its target's perch pushes the target off it:
/// it faces the target and asks for a Force Push once it can.
pub(super) fn push(
    (mind, ps, own, target): (&mut Mind, &MotionState, &Own, &Seen),
    spot: Vec3,
    (me, now, msec): (SimId, u64, u32),
    (level, doors): (&Level, &dyn walk::Ways),
) -> Command {
    let arrived = math::flat_distance(ps.origin, spot) <= walk::REACHED;
    let mut command = if arrived {
        mind.walker = None;
        face(ps, target, msec)
    } else {
        route_to(mind, ps, spot, (now, msec), (level, doors))
    };
    // It looks up at its target on the way, so that a Force Push started
    // as it arrives is aimed.
    command.angles[0] = face(ps, target, msec).angles[0];
    let at = mind::aim(mind::eye(ps), mind::eye(&target.ps));
    let facing = crate::combat::ai::THROW_FACING;
    let aligned = mind::turn(at.1, ps.view[1]).abs() <= facing
        && mind::turn(at.0, ps.view[0]).abs() <= facing;
    let in_reach = own
        .push
        .is_some_and(|(_, reach)| math::length(math::sub(target.ps.origin, ps.origin)) <= reach);
    if arrived && aligned && in_reach && now >= mind.pace.power_at {
        mind.pace.push_now = true;
        mind.pace.start_power(
            own.tier == pace::Tier::Boss,
            now,
            &mut pace::Dice::new(now, me),
        );
    }
    command
}

/// A creature that hides: it runs to `hide_at` (chosen by `choose`, again
/// every `reach.look_ms` and when its spot is seen) and waits there facing its
/// target, without attacking.
pub(super) fn hide(
    (mind, ps, target): (&mut Mind, &MotionState, &Seen),
    (now, msec): (u64, u32),
    (level, doors): (&Level, &dyn walk::Ways),
    choose: &dyn Fn() -> Option<Vec3>,
) -> Command {
    if mind.pace.hide_at.is_none() || now >= mind.pace.look_at {
        mind.pace.look_at = now + rules::ai::duelling::reach::LOOK_MS;
        mind.pace.hide_at = choose().or(mind.pace.hide_at);
    }
    let mut command = match mind.pace.hide_at {
        Some(spot) if math::flat_distance(ps.origin, spot) > walk::REACHED => {
            route_to(mind, ps, spot, (now, msec), (level, doors))
        }
        _ => {
            mind.walker = None;
            face(ps, target, msec)
        }
    };
    command.buttons &= !buttons::ATTACK;
    command
}
