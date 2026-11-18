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
//! walks out on the floor rather than being lifted onto its top
//! (`walker_world`, `world::movement`). At a prop shaped as a swoop bike,
//! whose drawn shape stands out past its footprint, a walker and a body
//! thrown by a Force push are stopped by the shape. A body walking a
//! planned route, walking or thrown, passes into the shape and is stopped
//! by the walls round the footprint. A body standing just short of the prop has its shape
//! beside it (`beside_shape`), as the playthrough harness judges a walk the
//! prop holds up. Waiting for the owner's approval before they join
//! an approved test.

use collision::{Contents, Level, LevelBuilder, Masked, Tag, UNIT};

use super::{
    AnimTable, Ground, HALF_WIDTH, MotionState, SOLE_Z, STAND_HEIGHT, beside_shape, move_body,
    walker_world,
};
use crate::math::{self, Vec3};
use crate::probe::{Checks, check};

/// The entity the props belong to.
const PROP: u32 = 7;

/// A floor with a box owned by [`PROP`] on it, from `min` to `max`
/// (metres), with `contents`.
fn floor_with(min: [f32; 3], max: [f32; 3], contents: Contents) -> Level {
    floor_with_boxes(&[(min, max, contents)])
}

/// A floor with boxes owned by [`PROP`] on it, each from its `min` to its
/// `max` (metres), with its contents.
fn floor_with_boxes(boxes: &[([f32; 3], [f32; 3], Contents)]) -> Level {
    let mut builder = LevelBuilder::new();
    builder.aabb(
        [-5.0, -5.0, -1.0],
        [5.0, 5.0, 0.0],
        Tag::level(Contents::SOLID, 0),
    );
    for &(min, max, contents) in boxes {
        builder.aabb(
            min,
            max,
            Tag {
                entity: Some(PROP),
                ..Tag::level(contents, 0)
            },
        );
    }
    builder.build()
}

/// A prop shaped as a swoop bike is: a drawn shape 2 m long, 0.8 m high
/// and 0.6 m wide from x = 1 m, standing out past its footprint, which
/// lies from x = 1.6 m to 2.4 m and has walls round it from under the
/// floor to 3 m up (`Contents::FOOTPRINT_CLIP`, the lower part of the walls
/// `collision_odyssey` stands round a drawn placeable's footprint).
fn bike() -> Level {
    floor_with_boxes(&[
        (
            [1.0, -0.3, 0.0],
            [3.0, 0.3, 0.8],
            Contents(Contents::SHAPE.0 | Contents::NONWALK.0),
        ),
        (
            [BIKE_FOOTPRINT, -0.2, -0.5],
            [2.4, 0.2, 3.0],
            Contents::FOOTPRINT_CLIP,
        ),
    ])
}

/// Where the bike's footprint starts along x (metres).
const BIKE_FOOTPRINT: f32 = 1.6;

/// How a probe's walker moves.
#[derive(Clone, Copy)]
struct Walk {
    /// Where it starts along x (metres; y = 0, on the floor).
    from: f32,
    /// It jumps on the way.
    jump: bool,
    /// It walks a planned route, as `world::movement` steps a routed body.
    routed: bool,
    /// It stands still and is thrown toward x = 4 m from where it starts,
    /// as a Force push at its first level's full strength throws a body.
    pushed: bool,
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
    if how.pushed {
        use crate::combat::force::ja;
        ps.velocity = [ja::PUSH_PULL_STRENGTH, 0.0, ja::SHOVE_LIFT];
    }
    for step in 0..500 {
        let mut command = if how.pushed {
            crate::input::Command::default()
        } else {
            crate::walk::toward(&ps, to)
        };
        if how.jump && (100..140).contains(&step) {
            command.up = 127;
        }
        let world = walker_world(level, how.routed, ps.origin, ps.maxs_z);
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

/// How far (units) before a prop's box a creature that jumps at it takes
/// off (`ai.duelling.reach.jump_from`).
const JUMP_FROM: f32 = rules::ai::duelling::reach::JUMP_FROM;
const JUMP_RISE: f32 = rules::ai::duelling::reach::JUMP_RISE;
/// The steps `highest_plain_jump_top` raises a prop's top in (units).
const STEP: f32 = 2.0;

/// Whether a body running at a footlocker-sized prop `top` units high
/// from 4 m away, pressing jump `JUMP_FROM` units before its box, ends
/// on its top: the way a creature that jumps at a prop does it.
fn jump_lands(top: f32) -> bool {
    let shape = Contents(Contents::SHAPE.0 | Contents::NONWALK.0);
    let level = floor_with([1.0, -0.3, 0.0], [2.2, 0.3, top * UNIT], shape);
    let anims = AnimTable::stand_in();
    let stand = |x: f32| -> Vec3 { [x / UNIT, 0.0, -SOLE_Z + 0.01 / UNIT] };
    let mut ps = MotionState::standing(stand(-3.0), 0.0);
    let (to, edge) = (stand(4.0), 1.0 / UNIT - HALF_WIDTH);
    let mut landed = false;
    for step in 0..400 {
        let mut command = crate::walk::toward(&ps, to);
        command.up = if ps.ground.is_some() && edge - ps.origin[0] <= JUMP_FROM {
            127
        } else {
            0
        };
        move_body(
            &mut ps,
            &command,
            8,
            (step + 1) * 8,
            &Masked::new(&level, Contents::NONE),
            &anims,
            None,
        );
        landed |= ps.ground == Some(Ground::Entity(PROP));
    }
    landed
}

/// The highest top (units, in steps of 2) a plain jump lands on.
fn highest_plain_jump_top() -> f32 {
    (2..=80)
        .map(|step| step as f32 * STEP)
        .rfind(|top| jump_lands(*top))
        .unwrap_or(0.0)
}

pub fn probes() -> Checks {
    let shape = Contents(Contents::SHAPE.0 | Contents::NONWALK.0);
    let locker = floor_with([1.0, -0.3, 0.0], [2.2, 0.3, 0.4], shape);
    let base = floor_with([1.0, -0.8, 0.0], [2.6, 0.8, 0.25], Contents::SHAPE);
    let bike = bike();
    let free = Walk {
        from: -2.0,
        jump: false,
        routed: false,
        pushed: false,
    };
    let routed = Walk {
        routed: true,
        ..free
    };
    let pushed = Walk {
        from: -0.5,
        pushed: true,
        ..free
    };
    // Name, level, walk, and what it must come to: the end's x and soles
    // (metres) and the highest stand on the prop.
    type Want = fn(Vec3, Option<f32>) -> bool;
    let cases: [(&str, &Level, Walk, Want, &str); 9] = [
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
            routed,
            |end, on| end[0] > 3.5 && end[2].abs() < 0.02 && on.is_none(),
            "a body walking a planned route through the footlocker's box",
        ),
        (
            "bike_shape_stops_a_walker",
            &bike,
            free,
            |end, on| end[0] < 1.0 - HALF_WIDTH * UNIT + 0.01 && on.is_none(),
            "a walker run at a swoop bike's drawn shape",
        ),
        (
            "bike_footprint_stops_a_route",
            &bike,
            routed,
            |end, on| {
                end[0] > 1.0 - HALF_WIDTH * UNIT + 0.05
                    && end[0] < BIKE_FOOTPRINT - HALF_WIDTH * UNIT + 0.01
                    && on.is_none()
            },
            "a body walking a planned route at a swoop bike, into its shape and up to the walls round its footprint",
        ),
        (
            "bike_shape_stops_a_pushed_body",
            &bike,
            pushed,
            |end, on| end[0] < 1.0 - HALF_WIDTH * UNIT + 0.01 && on.is_none(),
            "a body thrown by a Force push at a swoop bike",
        ),
        (
            "bike_footprint_stops_a_pushed_route",
            &bike,
            Walk {
                routed: true,
                ..pushed
            },
            |end, on| end[0] < BIKE_FOOTPRINT - HALF_WIDTH * UNIT + 0.01 && on.is_none(),
            "a body walking a planned route, thrown by a Force push at a swoop bike",
        ),
        (
            "prop_shape_walked_out_of",
            &locker,
            Walk { from: 1.6, ..free },
            |end, on| end[0] > 3.5 && end[2].abs() < 0.02 && on.is_none(),
            "a walker standing in the footlocker's box",
        ),
    ];
    // A body standing 0.05 m short of the footlocker's box has its shape
    // beside it, within 0.1 m; one 0.3 m short does not, and neither does
    // one standing in it (`beside_shape`, the playthrough harness's
    // `shape_holds`).
    let beside = {
        let half = HALF_WIDTH * UNIT;
        let at = |x: f32| -> Vec3 { [x / UNIT, 0.0, -SOLE_Z + 0.01 / UNIT] };
        let judged = [1.0 - half - 0.05, 1.0 - half - 0.3, 1.6]
            .map(|x| beside_shape(&locker, at(x), STAND_HEIGHT, 0.1 / UNIT));
        (
            "prop_shape_beside_a_box",
            check(
                judged == [true, false, false],
                format!(
                    "beside the shape 0.05 m short, 0.3 m short and standing in it: {judged:?}"
                ),
            ),
        )
    };
    let mut checks: Checks = cases
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
        .collect();
    checks.push(beside);
    let highest = highest_plain_jump_top();
    checks.push((
        "highest_plain_jump_top",
        check(
            (JUMP_RISE..=JUMP_RISE + STEP).contains(&highest),
            format!("a plain jump taken off {JUMP_FROM} units short of the box lands on a prop up to {highest} units high; the rule `ai.duelling.reach.jump_rise` of {JUMP_RISE} must lie within one step of it, at or below it (a creature in the world lands on one {} units high, `jumps_onto_a_prop`)", JUMP_RISE.floor() - 1.0),
        ),
    ));
    checks
}
