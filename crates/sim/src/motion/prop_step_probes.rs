//! The probes of a walker at a placeable's drawn shape (`kjk sim probes
//! --only "prop steps"`), no install. A prop the size of a footlocker
//! (1.2 m by 0.6 m, 0.4 m high, lower than a step), solid and `NONWALK`
//! and owned by an entity, as `collision_odyssey::placeable_shape` puts
//! one in. A walker run at it is stopped: `set_down` refuses to set a box
//! lifted by a step down on an entity's non-walkable top, though the box's
//! middle is still over the floor. A jump lands on top. A low base owned
//! by an entity and walkable (a cage's foot) is stepped onto and walked
//! off. Waiting for the owner's approval before they join an approved
//! test.

use collision::{Contents, Level, LevelBuilder, Tag, UNIT};

use super::{AnimTable, Ground, MotionState, SOLE_Z, move_body};
use crate::math::{self, Vec3};
use crate::probe::{Checks, check};

/// The entity the props belong to.
const PROP: u32 = 7;

/// A floor with a box owned by [`PROP`] on it, from `min` to `max`
/// (metres), with `contents`.
fn floor_with(min: [f32; 3], max: [f32; 3], contents: Contents) -> Level {
    let mut builder = LevelBuilder::new();
    builder.aabb(
        [-5.0, -5.0, -1.0],
        [5.0, 5.0, 0.0],
        Tag::level(Contents::SOLID, 0),
    );
    builder.aabb(
        min,
        max,
        Tag {
            entity: Some(PROP),
            ..Tag::level(contents, 0)
        },
    );
    builder.build()
}

/// A walk from x = -2 m toward x = 4 m along y = 0, jumping when `jump`,
/// for 4 s: where the walker ended (metres, soles) and the highest its
/// soles stood on the prop.
fn walk(level: &Level, jump: bool) -> (Vec3, Option<f32>) {
    let anims = AnimTable::stand_in();
    let stand = |x: f32| -> Vec3 { [x / UNIT, 0.0, -SOLE_Z + 0.01 / UNIT] };
    let to = stand(4.0);
    let mut ps = MotionState::standing(stand(-2.0), 0.0);
    let mut on_prop: Option<f32> = None;
    for step in 0..500 {
        let mut command = crate::walk::toward(&ps, to);
        if jump && (100..140).contains(&step) {
            command.up = 127;
        }
        move_body(&mut ps, &command, 8, (step + 1) * 8, level, &anims, None);
        if ps.ground == Some(Ground::Entity(PROP)) {
            let soles = (ps.origin[2] + SOLE_Z) * UNIT;
            on_prop = Some(on_prop.map_or(soles, |high| high.max(soles)));
        }
    }
    (
        math::scale([ps.origin[0], ps.origin[1], ps.origin[2] + SOLE_Z], UNIT),
        on_prop,
    )
}

pub fn probes() -> Checks {
    let locker = floor_with(
        [1.0, -0.3, 0.0],
        [2.2, 0.3, 0.4],
        Contents(Contents::SOLID.0 | Contents::NONWALK.0),
    );
    let (walked, on_top) = walk(&locker, false);
    let (jumped, landed) = walk(&locker, true);
    let base = floor_with([1.0, -0.8, 0.0], [2.6, 0.8, 0.25], Contents::SOLID);
    let (crossed, stood) = walk(&base, false);
    vec![
        (
            "prop_low_top_stops_a_walker",
            check(
                walked[0] < 1.0 && on_top.is_none(),
                format!(
                    "a walker run at a footlocker's box ends at x {:.2} m, its soles {:.2} m up, never on the box",
                    walked[0], walked[2]
                ),
            ),
        ),
        (
            "prop_low_top_takes_a_jump",
            check(
                landed.is_some_and(|soles| (soles - 0.4).abs() < 0.02),
                format!(
                    "a walker jumping at it stands on its top {:?} (m), and ends at x {:.2} m",
                    landed, jumped[0]
                ),
            ),
        ),
        (
            "prop_low_base_walked_over",
            check(
                stood.is_some_and(|soles| (soles - 0.25).abs() < 0.02) && crossed[0] > 3.5,
                format!(
                    "a walker run at a cage's base 0.25 m high stands on it {:?} (m) and ends past it at x {:.2} m",
                    stood, crossed[0]
                ),
            ),
        ),
    ]
}
