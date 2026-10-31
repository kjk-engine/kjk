//! A body that leans with the floor (`appearance.2da`'s `groundtilt`: T3-M4,
//! the astromechs, the Mark Two and Four droids, banthas, dewbacks and
//! rontos): drawn turned from upright to the slope under its middle, eased
//! as the floor changes, and standing on that slope rather than on the
//! corner of the sim's box (which rests on the slope's high side, its
//! middle over air). View-side only: the sim's box and its movement down
//! the slope are untouched.

use bevy::math::{Quat, Vec3};
use collision::player::WALKABLE_NORMAL_Z;
use collision::{Contents, Extents};

const EASE_MS: f32 = rules::creatures::ground::EASE_MS;
const REACH_M: f32 = rules::creatures::ground::REACH_M;
const LEVEL_NORMAL_Z: f32 = rules::creatures::ground::LEVEL_NORMAL_Z;

/// The walkable floor under a body's middle: its height and its up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floor {
    /// Metres, game space.
    pub z: f32,
    pub normal: Vec3,
}

/// The walkable floor under a body whose box's middle is at `middle` and
/// whose soles are at `soles` (game space, metres), no further than
/// `REACH_M` below the soles, through `level` as the sim traces it with
/// `states` (`sim::Level::with`).
pub fn floor_under(
    level: &sim::Level,
    states: Option<&[u8]>,
    (middle, soles): (Vec3, Vec3),
) -> Option<Floor> {
    let below = Vec3::new(middle.x, middle.y, soles.z - REACH_M);
    let trace = level.with(states, |world| {
        world.trace(
            middle.to_array(),
            below.to_array(),
            &Extents::POINT,
            Contents::SOLID,
        )
    });
    let normal = Vec3::from_array(trace.plane.normal);
    (!trace.start_solid && trace.fraction < 1.0 && normal.z >= WALKABLE_NORMAL_Z).then_some(Floor {
        z: trace.end[2],
        normal,
    })
}

/// How a leaning body stands: how far it leans (its `groundtilt`), its up
/// as last drawn and when (ms).
#[derive(Debug, Clone, Copy)]
pub struct Lean {
    share: f32,
    up: Option<(Vec3, f64)>,
}

impl Lean {
    /// A body leaning `share` of the way with the floor; none for one that
    /// stands upright.
    pub fn new(share: f32) -> Option<Lean> {
        (share > 0.0).then_some(Lean { share, up: None })
    }

    /// Where the body standing at `soles` is drawn at `ms` over `floor`
    /// (`floor_under`) and how it is turned from upright: its up eased
    /// toward the floor's (upright with no floor found), with a time
    /// constant of `EASE_MS`; on a sloping floor it stands on the slope
    /// under its middle. Time going back (a rewind) holds the lean.
    pub fn stand(&mut self, floor: Option<Floor>, soles: Vec3, ms: f64) -> (Vec3, Quat) {
        let wanted = floor.map_or(Vec3::Z, |floor| {
            Vec3::Z.lerp(floor.normal, self.share).normalize_or(Vec3::Z)
        });
        let up = match self.up {
            Some((up, at)) => {
                let elapsed = (ms - at).max(0.0) as f32;
                let eased = 1.0 - (-elapsed / EASE_MS).exp();
                up.lerp(wanted, eased).normalize_or(wanted)
            }
            None => wanted,
        };
        self.up = Some((up, ms));
        let soles = match floor {
            Some(floor) if floor.normal.z < LEVEL_NORMAL_Z => soles.with_z(floor.z),
            _ => soles,
        };
        (soles, Quat::from_rotation_arc(Vec3::Z, up))
    }
}
