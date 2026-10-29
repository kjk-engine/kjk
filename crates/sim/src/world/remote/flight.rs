//! How a droid flies (`remote.flight` in the rulebook): where its seat
//! looks, hovering between its floor bounds, through nothing a player can't
//! pass, leashed to the party; behind the leader again on Use or when lost.
//! While the world holds (`super::holds`) it waits hidden where it is, and
//! comes back behind the leader as the hold ends.

use bevy_ecs::prelude::*;
use collision::{Contents, Extents, UNIT};

use super::super::{
    Avatar, Clock, Fighting, Id, Inputs, Object, Player, Static, door_states, in_party,
};
use super::{Remotes, lead, placed_behind};
use crate::Level;
use crate::input::buttons;
use crate::math::{scale, short_to_degrees, sin_cos_deg};

const SPEED_M: f32 = rules::remote::flight::SPEED_M;
const CLIMB_M: f32 = rules::remote::flight::CLIMB_M;
const RESPONSE: f32 = rules::remote::flight::RESPONSE;
pub(crate) const FLOOR_MIN_M: f32 = rules::remote::flight::FLOOR_MIN_M;
pub(crate) const FLOOR_MAX_M: f32 = rules::remote::flight::FLOOR_MAX_M;
const FLOOR_SEARCH_M: f32 = rules::remote::flight::FLOOR_SEARCH_M;
const HALF_SIZE_M: f32 = rules::remote::flight::HALF_SIZE_M;
pub(crate) const LEASH_M: f32 = rules::remote::flight::LEASH_M;

const BOX: Extents = Extents {
    mins: [-HALF_SIZE_M; 3],
    maxs: [HALF_SIZE_M; 3],
};

/// The floor's height beneath `at` (m), within the floor search.
pub(crate) fn floor(level: &Level, at: [f32; 3]) -> Option<f32> {
    floor_with(level, None, at)
}

fn floor_with(level: &Level, states: Option<&[u8]>, at: [f32; 3]) -> Option<f32> {
    let below = [at[0], at[1], at[2] - FLOOR_SEARCH_M];
    let trace = level.with(states, |world| {
        world.trace(at, below, &Extents::POINT, Contents::SOLID)
    });
    (!trace.start_solid && trace.fraction < 1.0).then_some(trace.end[2])
}

/// As far from `from` toward `to` (m) as a droid's box goes before what is
/// solid (doors as `states` has them): `from` itself if it starts inside.
pub(crate) fn cleared(
    level: &Level,
    states: Option<&[u8]>,
    from: [f32; 3],
    to: [f32; 3],
) -> [f32; 3] {
    let trace = level.with(states, |world| world.trace(from, to, &BOX, Contents::SOLID));
    if trace.start_solid { from } else { trace.end }
}

/// Behind the leader at `lead` (`placed_behind`), but on the leader's side
/// of any wall or door there: traced from over the leader to that spot.
pub(crate) fn behind(
    level: &Level,
    states: Option<&[u8]>,
    lead: ([f32; 3], f32),
) -> ([f32; 3], f32) {
    let (spot, yaw) = placed_behind(lead);
    (cleared(level, states, super::lifted(lead.0), spot), yaw)
}

/// Whether a droid's box at `at` (m) is inside the level.
pub(crate) fn inside(level: &Level, at: [f32; 3]) -> bool {
    inside_with(level, None, at)
}

fn inside_with(level: &Level, states: Option<&[u8]>, at: [f32; 3]) -> bool {
    level.with(states, |world| {
        world.trace(at, at, &BOX, Contents::SOLID).start_solid
    })
}

/// Moves each droid by its seat's command. Runs on every step, so the
/// views predict it.
#[allow(clippy::too_many_arguments)]
pub(in crate::world) fn fly(
    clock: Res<Clock>,
    (held, module): (Res<super::super::scene::Held>, Res<super::super::Module>),
    inputs: Res<Inputs>,
    level: Res<Static>,
    objects: Query<&Object>,
    bodies: Query<(&Id, &Player, &Fighting, Option<&Avatar>)>,
    scripts: Res<super::super::scripts::Scripts>,
    mut remotes: ResMut<Remotes>,
) {
    if remotes.0.is_empty() {
        return;
    }
    let level = &level.0;
    let states = door_states(level, &objects);
    let states = states.as_deref();
    let dt = clock.msec as f32 / 1000.0;
    let leader = lead(
        super::super::lent(&scripts.state.party),
        bodies
            .iter()
            .map(|(id, player, _, avatar)| (id.0, &player.0, avatar.map(|a| a.0))),
    )
    .map(|(_, at, yaw)| (at, yaw));
    // The standing party members (m), the leash's ends.
    let party: Vec<[f32; 3]> = bodies
        .iter()
        .filter(|(_, _, fighting, avatar)| in_party(*avatar, &fighting.0) && fighting.0.alive())
        .map(|(_, player, ..)| scale(player.0.origin, UNIT))
        .collect();
    let holds = super::holds(&held, module.fade.as_ref());
    for remote in &mut remotes.0 {
        let back = |remote: &mut crate::remote::Remote| {
            if let Some((at, yaw)) = leader.map(|lead| behind(level, states, lead)) {
                remote.at = at;
                remote.yaw = yaw;
                remote.velocity = [0.0; 3];
            }
        };
        if holds || remote.hidden {
            remote.hidden = holds;
            remote.velocity = [0.0; 3];
            if !holds {
                back(remote);
            }
            continue;
        }
        let Some(command) = inputs.0.get(remote.seat) else {
            continue;
        };
        if command.buttons & buttons::USE != 0 {
            back(remote);
            continue;
        }
        remote.yaw = short_to_degrees(command.angles[1]);
        let (sin, cos) = sin_cos_deg(remote.yaw);
        let [ahead, right, up] =
            [command.forward, command.right, command.up].map(|axis| f32::from(axis) / 127.0);
        let wanted = [
            (ahead * cos + right * sin) * SPEED_M,
            (ahead * sin - right * cos) * SPEED_M,
            up * CLIMB_M,
        ];
        let closing = (RESPONSE * dt).min(1.0);
        for (v, want) in remote.velocity.iter_mut().zip(wanted) {
            *v += (want - *v) * closing;
        }
        let mut next: [f32; 3] = std::array::from_fn(|i| remote.at[i] + remote.velocity[i] * dt);
        let lost = match floor_with(level, states, next) {
            Some(floor) => {
                // Under a low ceiling it hovers a box's height below it, so
                // it never scrapes along it.
                let low = floor + FLOOR_MIN_M;
                let (from, to) = (
                    [next[0], next[1], low],
                    [next[0], next[1], floor + FLOOR_MAX_M + HALF_SIZE_M],
                );
                let up = level.with(states, |world| {
                    world.trace(from, to, &Extents::POINT, Contents::SOLID)
                });
                let high = (up.end[2] - 2.0 * HALF_SIZE_M).max(low);
                next[2] = next[2].clamp(low, high.min(floor + FLOOR_MAX_M));
                false
            }
            None => true,
        };
        let next = leashed(&party, next);
        let trace = level.with(states, |world| {
            world.trace(remote.at, next, &BOX, Contents::SOLID)
        });
        if lost || trace.start_solid {
            back(remote);
            continue;
        }
        if trace.fraction >= 1.0 {
            remote.at = trace.end;
            continue;
        }
        // Stopped: it slides, each axis on its own, losing only the speed
        // into what stopped it.
        for axis in 0..3 {
            let mut to = remote.at;
            to[axis] = next[axis];
            let part = level.with(states, |world| {
                world.trace(remote.at, to, &BOX, Contents::SOLID)
            });
            if !part.start_solid {
                remote.at = part.end;
            }
            if part.fraction < 1.0 {
                remote.velocity[axis] = 0.0;
            }
        }
    }
}

/// `at`, pulled to within `LEASH_M` of the nearest of `party` (m).
pub(crate) fn leashed(party: &[[f32; 3]], at: [f32; 3]) -> [f32; 3] {
    let apart = |of: &[f32; 3]| (0..3).map(|i| (of[i] - at[i]).powi(2)).sum::<f32>();
    let Some(anchor) = party
        .iter()
        .copied()
        .min_by(|a, b| apart(a).total_cmp(&apart(b)))
    else {
        return at;
    };
    let far = apart(&anchor).sqrt();
    if far <= LEASH_M {
        return at;
    }
    std::array::from_fn(|i| anchor[i] + (at[i] - anchor[i]) * LEASH_M / far)
}
