//! The probes of a body put where KOTOR's walker stands but JA's box is
//! buried (`kjk sim probes --only tucked`; `movement::tucked`), no install:
//! a floor and a chair, its seat and back a box over the floor beside the
//! body's middle (TSL's Jedi Council, put at their chairs to sit, #277):
//! - a body put at the chair, its box all in the chair, its middle on the
//!   floor, is tucked: it stands there;
//! - one clear of the chair is not: move_body moves it as before;
//! - one whose middle is in the chair too is not: the walker has no room;
//! - one over no floor within a step is not.

use collision::{BoxWorld, Solid, UNIT};

use super::movement::tucked;
use crate::motion::{HALF_WIDTH, SOLE_Z, STAND_HEIGHT, STEP_HEIGHT};
use crate::probe::{Checks, check};

pub fn probes() -> Checks {
    vec![
        ("tucked_at_a_chair", at_chair()),
        ("clear_body_not_tucked", clear()),
        ("walker_in_the_chair_not_tucked", walker_in_chair()),
        ("no_floor_not_tucked", floorless()),
    ]
}

/// How far (metres) the chair's near face stands from the body's middle:
/// inside the box's half width, clear of the walker's point.
const CHAIR_FROM: f32 = 0.1;

/// A floor at z 0 with a chair (a box 0.6 m wide, from `CHAIR_FROM` along
/// +X and up to a body's height) beside the origin.
fn chamber() -> BoxWorld {
    let top = (STAND_HEIGHT - SOLE_Z) * UNIT;
    BoxWorld::floor(4.0, 0.0, 1.0).with(Solid::level(
        [CHAIR_FROM, -0.3, 0.0],
        [CHAIR_FROM + 0.6, 0.3, top],
    ))
}

/// The origin (units) of a body standing with its soles at `(x, y, z)`
/// metres, held over its floor as a body put down standing is
/// (`walk::LIFT`).
fn origin([x, y, z]: [f32; 3]) -> [f32; 3] {
    let lift = rules::walking::walker::LIFT;
    [x / UNIT, y / UNIT, z / UNIT - SOLE_Z + lift]
}

fn is_tucked(soles: [f32; 3]) -> bool {
    tucked(&chamber(), origin(soles), STAND_HEIGHT)
}

fn at_chair() -> Result<String, String> {
    let buried = is_tucked([0.0, 0.0, 0.0]);
    check(
        buried,
        format!(
            "a body {CHAIR_FROM} m from a chair (box half width {HALF_WIDTH} units) is tucked: {buried}"
        ),
    )
}

fn clear() -> Result<String, String> {
    let tucked = is_tucked([-1.0, 0.0, 0.0]);
    check(
        !tucked,
        format!("a body 1 m clear of the chair is not tucked: {tucked}"),
    )
}

fn walker_in_chair() -> Result<String, String> {
    let tucked = is_tucked([CHAIR_FROM + 0.3, 0.0, 0.0]);
    check(
        !tucked,
        format!("a body whose middle is in the chair is not tucked: {tucked}"),
    )
}

fn floorless() -> Result<String, String> {
    let high = 2.0 * STEP_HEIGHT * UNIT;
    let tucked = is_tucked([0.0, 0.0, high]);
    check(
        !tucked,
        format!("a body {high} m over the floor is not tucked: {tucked}"),
    )
}
