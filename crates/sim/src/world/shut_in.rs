//! Bodies a door shut on. A script's `ActionCloseDoor` closes a door at
//! once, whoever stands still in its doorway (`scripts::apply`), and a save
//! made before this rule can hold a body inside a shut door. A door's
//! closed shape is a shell of panels, and a body caught between its front
//! and its back cannot walk out of it, so it is put on the free floor just
//! outside the door instead: on the side its party stands (the nearest
//! other living body), else the other, as KOTOR's safe-location search
//! puts an arriving companion (`install::jump_place`, `install::PARTY`).
//! This is checked on the first step a world runs (an area just installed
//! or a save just loaded) and on every step a door closed.

use super::*;

/// Whether this step must look for bodies inside a shut door: a door
/// closed this step, or the world was just made. Not in the snapshot: a
/// world made from one looks once, and finds no one shut in unless the
/// snapshot already held them so.
#[derive(Resource)]
pub(super) struct DoorShut(pub bool);

impl Default for DoorShut {
    fn default() -> Self {
        Self(true)
    }
}

/// A body's soles (metres) from its box origin (units).
fn soles(origin: [f32; 3]) -> [f32; 3] {
    scale([origin[0], origin[1], origin[2] + motion::SOLE_Z], UNIT)
}

/// Where a line from `from` across door `index` (along `way`, a flat unit
/// vector) meets the door's panel, within the first ring of the
/// safe-location search: how far (metres) its near face and its far face
/// are. None when the line meets something else first, or nothing.
fn panel(
    world: &dyn CollisionWorld,
    index: u32,
    from: [f32; 3],
    way: [f32; 2],
) -> Option<[f32; 2]> {
    let reach = rules::scripts::jumps::RING_M;
    let along = |by: f32| [from[0] + way[0] * by, from[1] + way[1] * by, from[2]];
    let hits = |a, b| {
        let trace = world.trace(a, b, &Extents::POINT, Contents::PLAYER_SOLID);
        (trace.fraction < 1.0 && trace.entity == Some(index)).then_some(trace.fraction)
    };
    let near = hits(from, along(reach))? * reach;
    // Back from a ring past the near face: the far face.
    let past = near + reach;
    let far = hits(along(past), from).map_or(near, |fraction| past * (1.0 - fraction));
    Some([near, far])
}

/// Moves every living body inside a shut door out of it.
pub(super) fn free_shut_in(
    level: Res<Static>,
    mut shut: ResMut<DoorShut>,
    closed: Res<scripts::Closed>,
    objects: Query<(&Id, &Object)>,
    mut players: Query<(&Id, &mut Player)>,
    fighters: Query<(&Id, &Fighting)>,
) {
    let look = std::mem::take(&mut shut.0) || !closed.0.is_empty();
    if !look || level.0.collision.level().is_none() {
        return;
    }
    let doors: Vec<(u32, &ObjectData)> = objects
        .iter()
        .filter(|(_, object)| object.kind == ObjectKind::Door && object.state.state == 0)
        .filter_map(|(_, object)| {
            let facts = level.0.placed().get(object.index as usize)?;
            Some((object.index, facts.as_ref()))
        })
        .collect();
    if doors.is_empty() {
        return;
    }
    let alive = |id: SimId| {
        fighters
            .iter()
            .any(|(of, fighting)| of.0 == id && fighting.0.alive())
    };
    let states = level.0.states(
        objects
            .iter()
            .map(|(_, object)| (object.index, &object.state)),
    );
    // A door's panel lies along its bearing: the flat way across it.
    let across = |door: &ObjectData| {
        let (sin, cos) = math::sin_cos_deg(door.yaw);
        [-sin, cos]
    };
    // Where a line across the door from the middle of a body at `origin`
    // meets it, each way, when it meets it both ways: the body is inside.
    let walls = |world: &dyn CollisionWorld, (index, door): (u32, &ObjectData), origin, maxs_z| {
        let [x, y, z] = soles(origin);
        let middle = [x, y, z + maxs_z * UNIT / 2.0];
        let [ax, ay] = across(door);
        let ahead = panel(world, index, middle, [ax, ay])?;
        let behind = panel(world, index, middle, [-ax, -ay])?;
        Some((ahead, behind))
    };
    // Each living body (box origin, units), and the shut door it stands
    // inside with how far its panels are each way, if any.
    let bodies: Vec<(
        SimId,
        [f32; 3],
        Option<(u32, &ObjectData, [f32; 2], [f32; 2])>,
    )> = level.0.with(states.as_deref(), |world| {
        players
            .iter()
            .filter(|(id, _)| alive(id.0))
            .map(|(id, player)| {
                let ps = &player.0;
                let inside = doors
                    .iter()
                    .filter(|(index, _)| near_door(&level.0, *index, ps.origin, ps.maxs_z, 0.0))
                    .find_map(|&(index, door)| {
                        let (ahead, behind) = walls(world, (index, door), ps.origin, ps.maxs_z)?;
                        Some((index, door, ahead, behind))
                    });
                (id.0, ps.origin, inside)
            })
            .collect()
    });
    if bodies.iter().all(|(_, _, inside)| inside.is_none()) {
        return;
    }
    let space = |id: SimId| {
        let own = objects
            .iter()
            .find(|(of, _)| of.0 == id)
            .and_then(|(_, object)| object.facts.as_ref())
            .map_or(0.0, |facts| facts.personal_space);
        strikes::humanoid_space(own)
    };
    for &(who, origin, inside) in &bodies {
        let Some((index, door, ahead, behind)) = inside else {
            continue;
        };
        let at = soles(origin);
        let way = across(door);
        let side_of =
            |p: [f32; 3]| (p[0] - door.position[0]) * way[0] + (p[1] - door.position[1]) * way[1];
        let far = |p: &[f32; 3]| (p[0] - at[0]).powi(2) + (p[1] - at[1]).powi(2);
        let nearest = bodies
            .iter()
            .filter(|(other, _, shut)| *other != who && shut.is_none())
            .map(|(_, other, _)| soles(*other))
            .min_by(|a, b| far(a).total_cmp(&far(b)));
        let party_ahead = side_of(nearest.unwrap_or(at)) >= 0.0;
        let mine = space(who);
        let sides = match party_ahead {
            true => [(1.0, ahead), (-1.0, behind)],
            false => [(-1.0, behind), (1.0, ahead)],
        };
        let taken: Vec<([f32; 3], f32)> = bodies
            .iter()
            .filter(|(other, _, _)| *other != who)
            .map(|(other, origin, _)| (*origin, (mine + space(*other)) / UNIT))
            .collect();
        let maxs_z = players
            .iter()
            .find(|(id, _)| id.0 == who)
            .map_or(motion::STAND_HEIGHT, |(_, player)| player.0.maxs_z);
        // Out from the panel's far face, a personal space at a time, as far
        // as a party arrival looks: the first place there with room for the
        // whole body and a floor under it, on that side of the door. Then
        // the other side; then the safe-location search's nearest place
        // anywhere outside the door.
        let free = level.0.with(states.as_deref(), |world| {
            let outside = |place: [f32; 3]| walls(world, (index, door), place, maxs_z).is_none();
            let steps = (rules::scripts::jumps::PARTY_M / mine) as usize;
            let beside = sides.into_iter().find_map(|(sign, [_, far])| {
                (1..=steps).find_map(|step| {
                    let by = sign * (far + mine * step as f32);
                    let spot = crate::module::Spot {
                        position: [at[0] + way[0] * by, at[1] + way[1] * by, at[2]],
                        yaw: door.yaw,
                    };
                    let place = crate::install::spots(world, &spot, 1, &taken, None, 0)?[0];
                    (side_of(soles(place)) * sign > 0.0 && outside(place)).then_some(place)
                })
            });
            beside.or_else(|| {
                let reach = (mine, crate::install::PARTY);
                crate::install::jump_place(&level.0, world, at, &taken, reach)
                    .filter(|place| outside(*place))
            })
        });
        if let Some(free) = free
            && let Some((_, mut player)) = players.iter_mut().find(|(id, _)| id.0 == who)
        {
            let ps = &mut player.0;
            ps.origin = free;
            ps.velocity = [0.0; 3];
        }
    }
}
