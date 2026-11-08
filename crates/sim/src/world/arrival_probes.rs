//! Probes for arrivals over no floor (`kjk sim probes --only arrivals`):
//! a module whose entry lies off its rooms still installs, its seats and
//! companions standing round the walkmesh's closest face (on the face
//! itself where no room is found round it), or at the entry itself where
//! the module has no walkmesh, and the install says so.

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
    let corner = |x: f32, y: f32| ground_point(x * LEDGE_HALF_M, y * LEDGE_HALF_M, 0.0);
    let [a, b, c, d] =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| corner(x, y));
    let triangles = [[a, b, c], [a, c, d]];
    let mut builder = LevelBuilder::new();
    for triangle in triangles {
        builder.triangle(triangle, Tag::level(Contents::SOLID, 1));
    }
    let nav = collision::nav::NavMesh::build(&triangles, crate::walk::CLEARANCE * UNIT);
    let data = (**ground.module().expect("the proving ground is a module")).clone();
    Level::new(
        "ledge",
        Arc::new(builder.build()),
        Arc::new(AnimTable::stand_in()),
    )
    .with_module(data)
    .with_nav(nav)
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
    ]
}
