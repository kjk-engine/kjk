//! Probes for arrivals over no floor (`kjk sim probes --only arrivals`):
//! a module whose entry lies off its rooms still installs, its seats and
//! companions standing round the walkmesh's closest face (on the face
//! itself where no room is found round it), or at the entry itself where
//! the module has no walkmesh, and the install says so. A jump to the
//! floor under a seat drawn over the walkmesh stands there.

use std::sync::Arc;

use collision::{Contents, LevelBuilder, Tag, UNIT};

use crate::determinism::{ground_point, proving_ground};
use crate::install::APART;
use crate::math::Vec3;
use crate::motion::AnimTable;
use crate::probe::Checks;
use crate::reconcile_probes::mesh_ground;
use crate::snapshot::{Kind, Snapshot};
use crate::world::{Guests, Level, SimWorld, WorldConfig};

/// Where the entry is moved: 40 m out past the proving ground's walls.
const VOID: (f32, f32) = (-40.0, 0.0);
/// The half width (metres) of `ledge`'s floor and walkmesh: room for one.
const LEDGE_HALF_M: f32 = 0.1;

/// `level`'s module, under a name of its own, with its entry out in the
/// void.
fn entry_in_void(level: Level) -> Level {
    let mut data = (**level.module().expect("the proving ground is a module")).clone();
    data.name = "probe_void".into();
    data.entry.position = ground_point(VOID.0, VOID.1, 0.0);
    level.with_module(data)
}

/// The proving ground's module on a floor and walkmesh `LEDGE_HALF_M`
/// either way of its middle, the only floor there is, its entry out in
/// the void: no room round the face for more than one.
fn ledge() -> Level {
    let ground = entry_in_void(proving_ground());
    let data = (**ground.module().expect("the proving ground is a module")).clone();
    floors("ledge", &square(LEDGE_HALF_M, 0.0), &[]).with_module(data)
}

/// The half width (metres) of `under_a_seat`'s walkable floor, and of the
/// seat drawn over its middle, and the seat's height over the floor.
const SEAT_FLOOR_HALF_M: f32 = 2.0;
const SEAT_HALF_M: f32 = 0.3;
const SEAT_HEIGHT_M: f32 = 0.7;

/// A square of two triangles `half` metres either way of the proving
/// ground's middle, `z` metres over its floor.
fn square(half: f32, z: f32) -> [[Vec3; 3]; 2] {
    let corner = |x: f32, y: f32| ground_point(x * half, y * half, z);
    let [a, b, c, d] =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| corner(x, y));
    [[a, b, c], [a, c, d]]
}

/// A level named `name` whose collision is the squares `walkable` (in its
/// walkmesh too) and `drawn` (in its collision alone).
fn floors(name: &str, walkable: &[[Vec3; 3]], drawn: &[[Vec3; 3]]) -> Level {
    let mut builder = LevelBuilder::new();
    for &triangle in walkable {
        builder.triangle(triangle, Tag::level(Contents::SOLID, 1));
    }
    for &triangle in drawn {
        builder.triangle(triangle, Tag::level(Contents::SOLID, 0));
    }
    let nav = collision::nav::NavMesh::build(walkable, crate::walk::CLEARANCE * UNIT);
    Level::new(
        name,
        Arc::new(builder.build()),
        Arc::new(AnimTable::stand_in()),
    )
    .with_nav(nav)
}

/// A walkable floor with a seat drawn over its middle, as TSL's Ebon Hawk
/// cockpits draw the pilot's chair over the walkmesh's floor. The seat is
/// in the level's collision but not in its walkmesh.
fn under_a_seat() -> Level {
    floors(
        "under_a_seat",
        &square(SEAT_FLOOR_HALF_M, 0.0),
        &square(SEAT_HALF_M, SEAT_HEIGHT_M),
    )
}

/// The half widths (metres) of `under_a_pad`'s ground and pad, and the
/// pad's height over the ground.
const PAD_GROUND_HALF_M: f32 = 10.0;
const PAD_HALF_M: f32 = 4.0;
const PAD_HEIGHT_M: f32 = 2.5;

/// A walkable landing pad over a ground drawn under it, the module's entry
/// on the ground under the pad's middle. Mission to Sleheyron gives its
/// landing pad this arrival. The ground is in the level's collision but not
/// in its walkmesh.
fn under_a_pad() -> Level {
    let level = floors(
        "under_a_pad",
        &square(PAD_HALF_M, PAD_HEIGHT_M),
        &square(PAD_GROUND_HALF_M, 0.0),
    );
    let mut data = (**proving_ground()
        .module()
        .expect("the proving ground is a module"))
    .clone();
    data.name = "probe_pad".into();
    data.entry.position = ground_point(0.0, 0.0, 0.0);
    level.with_module(data)
}

/// An entry under the walkable pad stands the player on the pad, not on
/// the ground drawn under it: KOTOR stands an arrival on its walkmesh.
fn arrival_under_a_pad() -> Result<String, String> {
    let level = under_a_pad();
    let mut world = SimWorld::new(WorldConfig::one_seat(), Level::arena())
        .map_err(|error| error.to_string())?;
    let after = world
        .install(level, None)
        .map_err(|error| error.to_string())?;
    let seat = *seats(&after).first().ok_or("no seat")?;
    let soles = (seat[2] + crate::motion::SOLE_Z) * UNIT;
    let pad = ground_point(0.0, 0.0, PAD_HEIGHT_M)[2];
    told(
        (soles - pad).abs() <= 0.05,
        format!(
            "soles {:.2} m over the ground, the pad {PAD_HEIGHT_M:.2} m",
            soles - ground_point(0.0, 0.0, 0.0)[2]
        ),
    )
}

/// A jump to the floor under a drawn seat stands the jumper there, on the
/// floor: the executables test a place's room on the walkmesh alone
/// (issue 479, Atton in the Ebon Hawk's pilot chair).
fn jump_under_a_seat() -> Result<String, String> {
    let level = under_a_seat();
    let mark = ground_point(0.0, 0.0, 0.0);
    let space = crate::world::strikes::humanoid_space(0.0);
    let place = level
        .with(None, |world| {
            crate::install::jump_place(
                &level,
                world,
                [mark[0], mark[1], mark[2] + 1.0],
                &[],
                (space, crate::install::JUMP),
            )
        })
        .ok_or("no place found")?;
    let soles = (place[2] + crate::motion::SOLE_Z) * UNIT;
    let (off, rise) = (flat_m(place, mark), soles - mark[2]);
    told(
        off <= 0.05 && rise.abs() <= 0.05,
        format!("{off:.2} m from the mark, soles {rise:.2} m over the floor"),
    )
}

/// `level` installed over `world`: the snapshot after and the line the
/// install gave for its fallback.
fn install(mut world: SimWorld, level: &Level) -> Result<(Snapshot, String), String> {
    let after = world
        .install(level.clone(), None)
        .map_err(|error| error.to_string())?;
    let said = world
        .arrival_fallback()
        .ok_or("no fallback said")?
        .to_string();
    Ok((after, said))
}

/// `level` installed for `seats` players.
fn arrive(level: &Level, seats: u8) -> Result<(Snapshot, String), String> {
    let config = WorldConfig {
        seats,
        ..WorldConfig::one_seat()
    };
    let world = SimWorld::new(config, Level::arena()).map_err(|error| error.to_string())?;
    install(world, level)
}

/// The seats' origins (units), by seat.
fn seats(after: &Snapshot) -> Vec<Vec3> {
    let mut avatars: Vec<_> = after
        .entities
        .iter()
        .filter_map(|entity| match entity.kind {
            Kind::Avatar(seat) => Some((seat, entity.player?.origin)),
            _ => None,
        })
        .collect();
    avatars.sort_by_key(|(seat, _)| *seat);
    avatars.into_iter().map(|(_, origin)| origin).collect()
}

/// How far (metres, seen from above) `origin` (units) is from `at`.
fn flat_m(origin: Vec3, at: Vec3) -> f32 {
    let (dx, dy) = (origin[0] * UNIT - at[0], origin[1] * UNIT - at[1]);
    (dx * dx + dy * dy).sqrt()
}

/// Ok with `detail` when `ok`, else an error with it.
fn told(ok: bool, detail: String) -> Result<String, String> {
    if ok { Ok(detail) } else { Err(detail) }
}

pub fn probes() -> Checks {
    let mesh = entry_in_void(mesh_ground());
    let on_the_mesh = arrive(&mesh, 1).and_then(|(after, said)| {
        let origins = seats(&after);
        let ok = !origins.is_empty()
            && origins
                .iter()
                .all(|&origin| crate::walk::on_mesh(&mesh, origin))
            && said.contains("round the walkmesh's closest face");
        told(ok, format!("{origins:?}; {said}"))
    });
    let ledge = ledge();
    let middle = ground_point(0.0, 0.0, 0.0);
    let crowded = arrive(&ledge, 3).and_then(|(after, said)| {
        let origins = seats(&after);
        let ok = origins.len() == 3
            && origins
                .iter()
                .all(|&origin| flat_m(origin, middle) <= 3.0 * APART * UNIT)
            && said.contains("crowded on the walkmesh's closest face");
        told(ok, format!("{origins:?}; {said}"))
    });
    let companions =
        super::drivers::probes::party_world(Guests::Avatars, 1).and_then(|(world, _, ids)| {
            let (after, said) = install(world, &mesh)?;
            let seat = *seats(&after).first().ok_or("no seat")?;
            let mates: Vec<Vec3> = ids
                .iter()
                .filter_map(|id| after.entities.iter().find(|entity| entity.id == *id))
                .filter_map(|entity| entity.player.map(|player| player.origin))
                .collect();
            let near = mates.iter().all(|&mate| {
                flat_m(mate, [seat[0] * UNIT, seat[1] * UNIT, 0.0])
                    <= rules::scripts::jumps::PARTY_M
            });
            let unstacked = mates.len() == ids.len()
                && mates.iter().all(|mate| *mate != seat)
                && mates.windows(2).all(|pair| pair[0] != pair[1]);
            told(
                near && unstacked,
                format!("seat {seat:?}, companions {mates:?}; {said}"),
            )
        });
    let bare = entry_in_void(proving_ground());
    let entry = ground_point(VOID.0, VOID.1, 0.0);
    let at_the_entry = arrive(&bare, 2).and_then(|(after, said)| {
        let origins = seats(&after);
        let ok = origins.len() == 2
            && origins
                .iter()
                .all(|&origin| flat_m(origin, entry) <= 2.0 * APART * UNIT)
            && origins[0] != origins[1]
            && said.contains("no walkmesh");
        told(ok, format!("{origins:?}; {said}"))
    });
    vec![
        (
            "an entry over no floor stands on the walkmesh's closest face",
            on_the_mesh,
        ),
        (
            "seats with no room round the closest face crowd onto it",
            crowded,
        ),
        (
            "companions gather round the party's moved arrival, apart",
            companions,
        ),
        (
            "an entry with no walkmesh anywhere stands its seats at the entry",
            at_the_entry,
        ),
        (
            "a jump to the floor under a drawn seat stands there",
            jump_under_a_seat(),
        ),
        (
            "an entry under a walkable pad stands on the pad",
            arrival_under_a_pad(),
        ),
    ]
}
