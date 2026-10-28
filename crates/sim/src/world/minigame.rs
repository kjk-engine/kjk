//! The step's minigame (`crate::minigame`): first in the step, the course
//! moves by each seat's command (a race's run each, the turret's gunners),
//! the hooks it raises queue for this
//! step's scripts (on the authority), and every seat's player stands parked
//! while it plays: its command keeps only where it looks.
//!
//! Two rules TSL's executable keeps outside the course:
//! - a TSL race begins with the global `FORFEIT_SWOOPRACE` false (its
//!   `Go`), which the races' heartbeats set once `GBL_QUIT_SWOOP` holds;
//!   the first seat's `Action::Forfeit` (the pause screen's quit) sets
//!   that;
//! - in the module `107PER` (Peragus' hangar, a turret with no fighters;
//!   the executable names it, so this does too), each of the turret's bolts
//!   stops on the area's walls, or meets the area's creatures (not the
//!   party's) and placeables: the one it meets hears a user-defined event,
//!   the bolt's damage its number (`Struck`, raised in `world::scripts`).

use bevy_ecs::prelude::*;
use collision::{Bodies, Body, CollisionWorld, Extents};

use super::scripts::Scripts;
use super::{
    Avatar, BODY, Clock, Fighting, Id, Inputs, LEADER_SEAT, Object, Player, Static, StepReason,
};
use crate::action::Action;
use crate::input::Command;
use crate::minigame::{Course, Kind, Rules, WORLD_TURRET};
use crate::module::ObjectKind;
use crate::snapshot::SimId;

/// The minigame in play (`Snapshot::minigame`): the install starts it, a
/// snapshot restores it, the step and the scripts move it.
#[derive(Resource, Default)]
pub(crate) struct Minigame(pub Option<crate::minigame::Course>);

/// What the turret's bolts met in `WORLD_TURRET` this step: the thing and
/// the bolt's damage, for its user-defined event. Authority only.
#[derive(Resource, Default)]
pub(crate) struct Struck(pub Vec<(SimId, i32)>);

/// The global a TSL race begins false (TSL's executable's `Go`).
const FORFEIT: &str = "FORFEIT_SWOOPRACE";
/// The global its pause screen's quit sets (`Action::Forfeit`).
const QUIT: &str = "GBL_QUIT_SWOOP";

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn steer(
    clock: Res<Clock>,
    level: Res<Static>,
    mut inputs: ResMut<Inputs>,
    mut game: ResMut<Minigame>,
    mut scripts: ResMut<Scripts>,
    mut struck: ResMut<Struck>,
    objects: Query<(&Id, &Object)>,
    doors: Query<&Object>,
    creatures: Query<(&Id, Option<&Avatar>, &Player, &Fighting)>,
) {
    struck.0.clear();
    let (Some(course), Some(rules)) = (game.0.as_mut(), level.0.minigame()) else {
        return;
    };
    if !course.fits(rules) {
        return;
    }
    let authority = clock.reason != StepReason::Predict;
    if authority && rules.tsl && rules.kind == Kind::Swoop {
        let booleans = &mut scripts.state.booleans;
        if !course.started {
            crate::script::set_global(booleans, FORFEIT, false);
        }
        let quits = inputs
            .0
            .actions()
            .iter()
            .any(|(seat, action)| *seat == LEADER_SEAT && *action == Action::Forfeit);
        if quits {
            crate::script::set_global(booleans, QUIT, true);
        }
    }
    // Each seat's command; one seat alone, not the first, steers the
    // course's player (in co-op each seat keeps its own).
    let mut commands: Vec<(u8, Command)> = inputs
        .0
        .iter()
        .map(|(seat, command)| (seat.0, *command))
        .collect();
    let alone = course.runs.is_empty() && course.gunners.is_empty();
    if let ([only], true) = (commands.as_mut_slice(), alone) {
        only.0 = LEADER_SEAT.0;
    }
    let raised = course.step(rules, &commands, clock.msec, clock.tick);
    if world_turret(&level.0, rules) {
        let states = super::door_states(&level.0, &doors);
        let area = Area {
            bodies: area_bodies(&objects, &creatures),
            placeables: objects
                .iter()
                .filter(|(_, object)| object.kind == ObjectKind::Placeable)
                .map(|(id, object)| (object.index, id.0))
                .collect(),
        };
        let met = meet_world(course, (&level.0, states.as_deref(), &area), clock.msec);
        if authority {
            struck.0 = met;
        }
    }
    if authority {
        let queued = raised
            .into_iter()
            .filter_map(|raised| crate::minigame::pending(rules, raised));
        scripts.state.pending.extend(queued);
    }
    // What steers the course moves no body; an answer to a conversation
    // stays the conversation's (the scripts may start one while it plays:
    // Peragus' turret ends with Atton's `turend`, whose reply loads the
    // boarded Ebon Hawk).
    let parked: Vec<_> = inputs
        .0
        .iter()
        .map(|(seat, command)| {
            (
                seat,
                Command {
                    angles: command.angles,
                    reply: command.reply,
                    ..Command::default()
                },
            )
        })
        .collect();
    for (seat, command) in parked {
        inputs.0.insert(seat, command);
    }
}

/// Whether `level`'s minigame is the turret whose bolts meet the area
/// (`WORLD_TURRET`).
fn world_turret(level: &super::Level, rules: &Rules) -> bool {
    rules.kind == Kind::Turret
        && level
            .module()
            .is_some_and(|module| module.name.eq_ignore_ascii_case(WORLD_TURRET))
}

/// What a bolt may meet in the area (metres, in id order): each living
/// creature not of the party (its body), each placeable (its pick volume).
fn area_bodies(
    objects: &Query<(&Id, &Object)>,
    creatures: &Query<(&Id, Option<&Avatar>, &Player, &Fighting)>,
) -> Vec<Body> {
    let mut all: Vec<Body> = creatures
        .iter()
        .filter(|(_, avatar, _, fighting)| {
            fighting.0.alive() && !super::in_party(*avatar, &fighting.0)
        })
        .map(|(id, _, player, _)| super::body(id.0, &player.0))
        .chain(super::objects::placeable_bodies(
            objects
                .iter()
                .filter(|(_, object)| object.kind == ObjectKind::Placeable),
        ))
        .collect();
    all.sort_by_key(|body| body.entity);
    all
}

/// What the turret's bolts may meet in the area: the bodies
/// (`area_bodies`), and the placeables by their number among the level's
/// entities (one with brushes of its own meets a bolt on them).
struct Area {
    bodies: Vec<Body>,
    placeables: Vec<(u32, SimId)>,
}

/// Each of `course`'s bolts this step (`msec` long) traced from where it
/// was to where it is: one that meets a wall goes; one that meets a thing
/// of `area` goes and is told of (the thing, the bolt's damage).
fn meet_world(
    course: &mut Course,
    (level, states, area): (&super::Level, Option<&[u8]>, &Area),
    msec: u32,
) -> Vec<(SimId, i32)> {
    let now = course.time_ms;
    let mut met = Vec::new();
    course.bullets.retain(|bullet| {
        let from = bullet.at(now.saturating_sub(u64::from(msec)).max(bullet.born));
        let to = bullet.at(now);
        let hit = level.with(states, |world| {
            Bodies::new(world, &area.bodies).trace(from, to, &Extents::POINT, super::SHOT)
        });
        if !hit.hit() {
            return true;
        }
        let thing = match hit.entity {
            Some(entity) if entity & BODY != 0 => Some(SimId(entity & !BODY)),
            Some(index) => area
                .placeables
                .iter()
                .find(|(own, _)| *own == index)
                .map(|(_, id)| *id),
            None => None,
        };
        if let Some(thing) = thing {
            met.push((thing, bullet.damage));
        }
        false
    });
    met
}
