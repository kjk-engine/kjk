//! The probes of a body's width against the level (`kjk sim probes body`):
//! a body moves as a box round across (`collision::Shape::Upright`), as
//! KOTOR tests a creature's move by its personal space across its way
//! whichever way it goes (TSL's CSWSCreature::WalkUpdateLocationTestDistance
//! hands CSWSArea::NoNonWalkPolysDetailed the appearance's `perspace`). A
//! channel 0.9 m across running at 45 degrees, wider than a body (0.76 m)
//! and narrower than JA's square box measured across it (1.08 m), is run
//! through; Khoonda's office joins its hall by such a channel.

use collision::{Contents, Extents, Level, LevelBuilder, Shape, Tag, UNIT};

use super::{AnimTable, HALF_WIDTH, MotionState, SOLE_Z, STAND_HEIGHT, move_body};
use crate::math::{self, Vec3};
use crate::probe::{Checks, check};

/// How far across the channel is between its walls (metres).
const ACROSS_M: f32 = 0.9;

/// A wall 0.3 m thick and 2 m high along the diagonal x = y, its near face
/// `offset` metres from that line (positive: on the side +x, −y).
fn wall(builder: &mut LevelBuilder, offset: f32) {
    let t = std::f32::consts::FRAC_1_SQRT_2;
    let (along, out) = (
        [t, t, 0.0],
        [t * offset.signum(), -t * offset.signum(), 0.0],
    );
    let at = |s: f32, o: f32, z: f32| -> [f32; 3] {
        [along[0] * s + out[0] * o, along[1] * s + out[1] * o, z]
    };
    let near = offset.abs();
    let far = near + 0.3;
    let points: Vec<[f32; 3]> = [(-2.0, near), (2.0, near), (2.0, far), (-2.0, far)]
        .into_iter()
        .flat_map(|(s, o)| [at(s, o, 0.0), at(s, o, 2.0)])
        .collect();
    let back = out.map(|c| -c);
    let faces = [
        [0.0, 0.0, 1.0],
        [0.0, 0.0, -1.0],
        along,
        along.map(|c| -c),
        out,
        back,
    ];
    let edges = [
        (0, 2),
        (2, 4),
        (4, 6),
        (6, 0),
        (1, 3),
        (3, 5),
        (5, 7),
        (7, 1),
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7),
    ];
    builder.convex(&points, &faces, &edges, Tag::level(Contents::SOLID, 0));
}

/// A floor with the diagonal channel on it.
fn channel() -> Level {
    let mut builder = LevelBuilder::new();
    builder.aabb(
        [-5.0, -5.0, -1.0],
        [5.0, 5.0, 0.0],
        Tag::level(Contents::SOLID, 0),
    );
    wall(&mut builder, ACROSS_M * 0.5);
    wall(&mut builder, -ACROSS_M * 0.5);
    builder.build()
}

pub fn probes() -> Checks {
    let level = channel();
    let mut checks: Checks = Vec::new();
    // Standing on the floor, 3.5 m short of the channel's middle along it.
    let stand = |x: f32| -> Vec3 { [x / UNIT, x / UNIT, -SOLE_Z + 0.25] };
    let (from, to) = (stand(-2.5), stand(2.5));
    let body = Extents {
        mins: [-HALF_WIDTH * UNIT, -HALF_WIDTH * UNIT, SOLE_Z * UNIT],
        maxs: [HALF_WIDTH * UNIT, HALF_WIDTH * UNIT, STAND_HEIGHT * UNIT],
    };
    let through = |shape: &Shape| {
        let metres = |v: Vec3| math::scale(v, UNIT);
        collision::CollisionWorld::sweep(&level, metres(from), metres(to), shape, Contents::SOLID)
    };
    let (as_box, upright) = (through(&Shape::Box(body)), through(&Shape::Upright(body)));
    checks.push((
        "body_channel_shapes",
        check(
            as_box.fraction < 1.0 && upright.fraction >= 1.0,
            format!(
                "along a channel {ACROSS_M} m across at 45 degrees, JA's square box stops ({:.2} of the way) and the body round across passes ({:.2})",
                as_box.fraction, upright.fraction
            ),
        ),
    ));
    // A body run through it by move_body, as a walker runs one: it comes
    // out the other side.
    let anims = AnimTable::stand_in();
    let mut ps = MotionState::standing(from, 45.0);
    let mut time_ms = 0;
    for _ in 0..500 {
        time_ms += 8;
        let command = crate::walk::toward(&ps, to);
        move_body(&mut ps, &command, 8, time_ms, &level, &anims, None);
        if math::flat_distance(ps.origin, to) < 12.0 {
            break;
        }
    }
    let left = math::flat_distance(ps.origin, to) * UNIT;
    checks.push((
        "body_runs_a_diagonal_channel",
        check(
            left < 0.35,
            format!(
                "run along the channel by move_body, the body comes out the other side ({left:.2} m short of the spot 3.5 m past its middle, after {:.1} s)",
                time_ms as f32 / 1000.0
            ),
        ),
    ));
    checks.push(("body_slides_by_its_round", slides_by_its_round(&level)));
    checks
}

/// A seat's player's step along a body it stands inside is weighed by the
/// body as `move_body` moves it, round across (`world::slides_clear`):
/// standing in the diagonal channel's middle, where JA's
/// square box's corners stand in both walls, a step along the channel is
/// clear and one 6 units into a wall is not. Weighed as the box, the level
/// counted as holding it already, the step into the wall was weighed by the
/// bodies alone and taken; buried, the player stood held for good
/// (Kashyyyk's Upper Shadowlands, between two tach in the pass to the
/// repulsor field).
fn slides_by_its_round(level: &Level) -> Result<String, String> {
    let middle: Vec3 = [0.0, 0.0, -SOLE_Z + 0.25];
    let height = STAND_HEIGHT;
    let half = HALF_WIDTH * UNIT;
    let square = Extents {
        mins: [-half, -half, SOLE_Z * UNIT],
        maxs: [half, half, height * UNIT],
    };
    let metres = math::scale(middle, UNIT);
    let boxed = collision::CollisionWorld::trace(level, metres, metres, &square, Contents::SOLID)
        .start_solid;
    let t = std::f32::consts::FRAC_1_SQRT_2;
    let step = |along: f32, out: f32| -> Vec3 {
        [
            middle[0] + t * (along + out),
            middle[1] + t * (along - out),
            middle[2],
        ]
    };
    let slides = |to: Vec3| crate::world::slides_clear(level, &[], 0, (middle, to), height);
    let (along, into) = (slides(step(10.0, 0.0)), slides(step(0.0, 6.0)));
    check(
        boxed && along && !into,
        format!(
            "in the channel's middle (the square box held by the walls: {boxed}): a step along it clear {along}, 6 units into a wall clear {into}"
        ),
    )
}
