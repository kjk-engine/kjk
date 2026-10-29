//! The seats' remote droids in the world (`crate::remote`). A resource, not
//! entities: no system that moves, hurts, perceives or runs scripts for
//! bodies ever sees one.

use bevy_ecs::prelude::*;
use collision::UNIT;

use super::{Avatar, Id, Inputs, LEADER_SEAT, Player, SeatsHeld, lent};
use crate::input::Seat;
use crate::math::scale;
use crate::remote::Remote;

/// The droids flying, by seat.
#[derive(Resource, Default)]
pub(crate) struct Remotes(pub Vec<Remote>);

/// The places this step's released bodies stood, by seat (`drivers::assign`
/// fills it): a droid launches from where its seat's body was.
#[derive(Resource, Default)]
pub(crate) struct Launched(pub Vec<(Seat, [f32; 3])>);

/// Gives each held guest seat that spoke and drives no body a droid, and
/// takes it from one that drives a body again. Runs on the authority, right
/// after `drivers::assign`.
pub(super) fn launch(world: &mut World) {
    let launched = std::mem::take(&mut world.resource_mut::<Launched>().0);
    let playing = world.resource::<super::minigame::Minigame>().0.is_none();
    let held = world.resource::<SeatsHeld>().0;
    let spoke: Vec<Seat> = {
        let inputs = &world.resource::<Inputs>().0;
        (1..held)
            .map(Seat)
            .filter(|seat| inputs.get(*seat).is_some())
            .collect()
    };
    let bodied: Vec<Seat> = world
        .query::<&Avatar>()
        .iter(world)
        .map(|avatar| avatar.0)
        .collect();
    let flies = |seat: &Seat| playing && spoke.contains(seat) && !bodied.contains(seat);
    let leader = leader(world);
    let mut remotes = std::mem::take(&mut world.resource_mut::<Remotes>().0);
    remotes.retain(|remote| flies(&remote.seat));
    for seat in spoke.iter().filter(|seat| flies(seat)) {
        if remotes.iter().any(|remote| remote.seat == *seat) {
            continue;
        }
        let from = launched
            .iter()
            .find(|(of, _)| of == seat)
            .map(|(_, at)| *at);
        let Some((at, yaw)) = from
            .map(|at| (at, leader.map_or(0.0, |(_, yaw)| yaw)))
            .or_else(|| leader.map(behind))
        else {
            continue;
        };
        remotes.push(Remote::new(*seat, [at[0], at[1], at[2] + LIFT], yaw));
    }
    remotes.sort_by_key(|remote| remote.seat);
    world.resource_mut::<Remotes>().0 = remotes;
}

/// How far above a body's origin a droid launches (m).
const LIFT: f32 = rules::remote::launch::LIFT_M;
/// How far behind the leader a droid with no body to launch from appears (m).
const BEHIND: f32 = rules::remote::launch::BEHIND_M;

/// Where the party's leader is (m) and which way it looks (degrees): seat
/// 0's acting body, the companion leading when one does (`lent`).
pub(super) fn leader(world: &mut World) -> Option<([f32; 3], f32)> {
    let lent = lent(&world.resource::<super::scripts::Scripts>().state.party);
    world
        .query::<(&Id, &Player, Option<&Avatar>)>()
        .iter(world)
        .find(|(id, _, avatar)| match lent {
            Some(lent) => id.0 == lent,
            None => avatar.is_some_and(|avatar| avatar.0 == LEADER_SEAT),
        })
        .map(|(_, player, _)| (scale(player.0.origin, UNIT), player.0.view[1]))
}

/// A place `BEHIND` the leader at `at`, looking along `yaw`.
fn behind((at, yaw): ([f32; 3], f32)) -> ([f32; 3], f32) {
    let (sin, cos) = crate::math::sin_cos_deg(yaw);
    ([at[0] - BEHIND * cos, at[1] - BEHIND * sin, at[2]], yaw)
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
