//! The probes of bodies met where their models are drawn (`kjk sim probes
//! --only bulks`; `world::body`, `module::Bulk`), no install: a hovering
//! droid's body (K1's probe droid's, 1.4 m to 3.0 m over its soles) and a
//! rancor's (5 m across, 7.6 m tall) against JA's player box:
//! - a bolt at the droid's middle meets it, one at a man's chest height
//!   under it passes below, as it would not through JA's box;
//! - a bolt at the rancor's head height meets it, over JA's box's top;
//! - a body turned a quarter turns its box with it;
//! - what others aim at (`mind::chest_of`) is the droid's middle.

use collision::{BoxWorld, CollisionWorld, Contents, Extents, UNIT};

use super::body;
use crate::combat::mind;
use crate::math::{Vec3, scale};
use crate::module::Bulk;
use crate::motion::MotionState;
use crate::probe::{Checks, check};
use crate::snapshot::SimId;

/// The probe droid's and the rancor's boxes, round K1's census
/// (`kjk combat bodies k1`).
const DROID: Bulk = Bulk {
    min: [-0.7, -0.7, 1.4],
    max: [0.7, 0.7, 3.0],
};
const RANCOR: Bulk = Bulk {
    min: [-2.5, -1.5, 0.0],
    max: [2.5, 3.0, 7.6],
};

pub fn probes() -> Checks {
    vec![
        ("hovering_droid_met_where_drawn", droid()),
        ("rancor_met_overhead", rancor()),
        ("bulk_turns_with_the_body", turned()),
        ("aimed_at_the_drawn_middle", aimed()),
    ]
}

/// A body standing with its soles on the floor at the origin, facing +Y.
fn standing() -> MotionState {
    MotionState::standing([0.0, 0.0, -crate::motion::SOLE_Z], 90.0)
}

/// Whether a bolt (a point) from 6 m off along -X at `height` metres over
/// the soles meets the body `bulk` makes of `standing` (`None`: JA's box).
fn met_at(bulk: Option<&Bulk>, height: f32) -> bool {
    let target = body(SimId(1), &standing(), bulk);
    let world = BoxWorld::new();
    let bodies = [target];
    let trace = collision::Bodies::new(&world, &bodies).trace(
        [-6.0, 0.0, height],
        [6.0, 0.0, height],
        &Extents::POINT,
        Contents::BODY,
    );
    trace.fraction < 1.0
}

fn droid() -> Result<String, String> {
    let middle = (DROID.min[2] + DROID.max[2]) * 0.5;
    let man = 1.1;
    let (drawn, under, ja_under) = (
        met_at(Some(&DROID), middle),
        met_at(Some(&DROID), man),
        met_at(None, man),
    );
    check(
        drawn && !under && ja_under,
        format!(
            "at its middle ({middle:.1} m) met {drawn}; at {man} m under it met {under} \
             (JA's box: {ja_under})"
        ),
    )
}

fn rancor() -> Result<String, String> {
    let head = 6.5;
    let (drawn, ja) = (met_at(Some(&RANCOR), head), met_at(None, head));
    check(
        drawn && !ja,
        format!("at {head} m met {drawn} (JA's box: {ja})"),
    )
}

fn turned() -> Result<String, String> {
    let mut quarter = standing();
    quarter.view[1] = 0.0;
    let ahead = body(SimId(1), &standing(), Some(&RANCOR));
    let side = body(SimId(1), &quarter, Some(&RANCOR));
    let size = |b: &collision::Body, axis: usize| b.max[axis] - b.min[axis];
    let swapped = (size(&ahead, 0) - size(&side, 1)).abs() < 1e-3
        && (size(&ahead, 1) - size(&side, 0)).abs() < 1e-3;
    // Facing +Y its 3 m ahead lies along +Y; facing +X, along +X.
    let reaches = (ahead.max[1] - 3.0).abs() < 1e-3 && (side.max[0] - 3.0).abs() < 1e-3;
    check(
        swapped && reaches,
        format!(
            "facing +Y {:?}..{:?}; facing +X {:?}..{:?}",
            ahead.min, ahead.max, side.min, side.max
        ),
    )
}

fn aimed() -> Result<String, String> {
    let ps = standing();
    let soles = ps.origin[2] + crate::motion::SOLE_Z;
    let at: Vec3 = mind::chest_of(&ps, Some(&DROID));
    let height = (at[2] - soles) * UNIT;
    let man: Vec3 = scale(mind::chest(&ps), UNIT);
    check(
        (height - 2.2).abs() < 1e-3,
        format!(
            "the droid is aimed at {height:.2} m over its soles (a man at {:.2})",
            man[2] - soles * UNIT
        ),
    )
}
