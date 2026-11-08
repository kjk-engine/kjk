//! Probes for arrivals over no floor (`kjk sim probes --only arrivals`):
//! a module whose entry lies off its rooms still installs, its players
//! standing on the walkmesh's nearest face, or at the entry itself where
//! the module has no walkmesh.

use crate::determinism::{ground_point, proving_ground};
use crate::install::APART;
use crate::math::Vec3;
use crate::probe::Checks;
use crate::reconcile_probes::mesh_ground;
use crate::snapshot::Kind;
use crate::walk::on_mesh;
use crate::world::Level;
use crate::{SimWorld, WorldConfig};

/// Where the entry is moved: 40 m out past the proving ground's walls.
const VOID: (f32, f32) = (-40.0, 0.0);

/// `level` with its module's entry out in the void.
fn entry_in_void(level: Level) -> Level {
    let mut data = (**level.module().expect("the proving ground is a module")).clone();
    data.entry.position = ground_point(VOID.0, VOID.1, 0.0);
    level.with_module(data)
}

/// The seats' origins (units) after `level` is installed for `seats`.
fn arrive(level: &Level, seats: u8) -> Result<Vec<Vec3>, String> {
    let config = WorldConfig {
        seats,
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, Level::arena()).map_err(|error| error.to_string())?;
    let after = world
        .install(level.clone(), None)
        .map_err(|error| error.to_string())?;
    let mut avatars: Vec<_> = after
        .entities
        .iter()
        .filter_map(|entity| match entity.kind {
            Kind::Avatar(seat) => Some((seat, entity.player?.origin)),
            _ => None,
        })
        .collect();
    avatars.sort_by_key(|(seat, _)| *seat);
    Ok(avatars.into_iter().map(|(_, origin)| origin).collect())
}

pub fn probes() -> Checks {
    let on_the_mesh = {
        let level = entry_in_void(mesh_ground());
        arrive(&level, 1).and_then(|origins| {
            let ok = !origins.is_empty() && origins.iter().all(|&origin| on_mesh(&level, origin));
            let detail = format!("{origins:?}");
            if ok { Ok(detail) } else { Err(detail) }
        })
    };
    let at_the_entry = {
        let level = entry_in_void(proving_ground());
        arrive(&level, 2).and_then(|origins| {
            let entry = ground_point(VOID.0, VOID.1, 0.0);
            let near = |origin: &Vec3| {
                let (dx, dy) = (
                    origin[0] * collision::UNIT - entry[0],
                    origin[1] * collision::UNIT - entry[1],
                );
                (dx * dx + dy * dy).sqrt() <= 2.0 * APART * collision::UNIT
            };
            let apart = origins.windows(2).all(|pair| pair[0] != pair[1]);
            let ok = origins.len() == 2 && origins.iter().all(near) && apart;
            let detail = format!("{origins:?}");
            if ok { Ok(detail) } else { Err(detail) }
        })
    };
    vec![
        (
            "an entry over no floor stands on the walkmesh's nearest face",
            on_the_mesh,
        ),
        (
            "an entry with no walkmesh anywhere stands its seats at the entry",
            at_the_entry,
        ),
    ]
}
