//! The probes of a walker at a placeable's drawn shape (`kjk sim probes
//! --only "prop steps"`), no install. A prop the size of a footlocker
//! (1.2 m by 0.6 m, 0.4 m high, lower than a step), `SHAPE | NONWALK` and
//! owned by an entity, as `collision_odyssey::placeable_shape` puts one in.
//! A walker run at it is stopped: `set_down` refuses to set a box lifted by
//! a step down on an entity's non-walkable top, though the box's middle is
//! still over the floor. A jump lands on top. A low base owned by an
//! entity and walkable (a cage's foot) is stepped onto and walked off. A
//! body walking a planned route passes through the prop, as KOTOR's walker
//! meets only the walkmesh, and so does one whose box stands in it, which
//! walks out on the floor rather than being lifted onto its top (`in_shape`,
//! `world::movement`). Waiting for the owner's approval before they join
//! an approved test.

use collision::{Contents, Level, LevelBuilder, Masked, Tag, UNIT};

use super::{AnimTable, Ground, MotionState, SOLE_Z, in_shape, move_body};
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

/// How a probe's walker moves.
#[derive(Clone, Copy)]
struct Walk {
    /// Where it starts along x (metres; y = 0, on the floor).
    from: f32,
    /// It jumps on the way.
    jump: bool,
    /// It walks a planned route, as `world::movement` steps a routed body.
    routed: bool,
}

/// A walk toward x = 4 m along y = 0 for 4 s, stepped as
/// `world::movement` steps a body: placeables' shapes left out of its
/// traces while it is routed or its box stands in one. Where the walker
/// ended (metres, soles) and the highest its soles stood on the prop.
fn walk(level: &Level, how: Walk) -> (Vec3, Option<f32>) {
    let anims = AnimTable::stand_in();
    let stand = |x: f32| -> Vec3 { [x / UNIT, 0.0, -SOLE_Z + 0.01 / UNIT] };
    let to = stand(4.0);
    let mut ps = MotionState::standing(stand(how.from), 0.0);
    let mut on_prop: Option<f32> = None;
    for step in 0..500 {
        let mut command = crate::walk::toward(&ps, to);
        if how.jump && (100..140).contains(&step) {
            command.up = 127;
        }
        let through = how.routed || in_shape(level, ps.origin, ps.maxs_z);
        let off = if through {
            Contents::SHAPE
        } else {
            Contents::NONE
        };
        let world = Masked::new(level, off);
        move_body(&mut ps, &command, 8, (step + 1) * 8, &world, &anims, None);
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
    let shape = Contents(Contents::SHAPE.0 | Contents::NONWALK.0);
    let locker = floor_with([1.0, -0.3, 0.0], [2.2, 0.3, 0.4], shape);
    let base = floor_with([1.0, -0.8, 0.0], [2.6, 0.8, 0.25], Contents::SHAPE);
    let free = Walk {
        from: -2.0,
        jump: false,
        routed: false,
    };
    // Name, level, walk, and what it must come to: the end's x and soles
    // (metres) and the highest stand on the prop.
    type Want = fn(Vec3, Option<f32>) -> bool;
    let cases: [(&str, &Level, Walk, Want, &str); 5] = [
        (
            "prop_low_top_stops_a_walker",
            &locker,
            free,
            |end, on| end[0] < 1.0 && on.is_none(),
            "a walker run at a footlocker's box",
        ),
        (
            "prop_low_top_takes_a_jump",
            &locker,
            Walk { jump: true, ..free },
            |_, on| on.is_some_and(|soles| (soles - 0.4).abs() < 0.02),
            "a walker jumping at it",
        ),
        (
            "prop_low_base_walked_over",
            &base,
            free,
            |end, on| on.is_some_and(|soles| (soles - 0.25).abs() < 0.02) && end[0] > 3.5,
            "a walker run at a cage's base 0.25 m high",
        ),
        (
            "prop_shape_passed_by_a_route",
            &locker,
            Walk {
                routed: true,
                ..free
            },
            |end, on| end[0] > 3.5 && end[2].abs() < 0.02 && on.is_none(),
            "a body walking a planned route through the footlocker's box",
        ),
        (
            "prop_shape_walked_out_of",
            &locker,
            Walk { from: 1.6, ..free },
            |end, on| end[0] > 3.5 && end[2].abs() < 0.02 && on.is_none(),
            "a walker standing in the footlocker's box",
        ),
    ];
    cases
        .into_iter()
        .map(|(name, level, how, want, what)| {
            let (end, on) = walk(level, how);
            (
                name,
                check(
                    want(end, on),
                    format!(
                        "{what} ends at x {:.2} m with its soles {:.2} m up; its highest stand on the prop {on:?} (m)",
                        end[0], end[2]
                    ),
                ),
            )
        })
        .collect()
}
