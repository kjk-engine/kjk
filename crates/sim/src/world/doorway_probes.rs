//! The probe of a route through a doorway whose walkable floor runs on under
//! the wall beside it (`kjk sim probes --only doorway`; `walk::fit_legs`),
//! no install: two rooms joined by a doorway, its floor wider than its
//! opening, the jamb on one side standing over the floor. A route from one
//! room to the other along the jamb's side keeps every leg one a standing
//! body walks straight along (Nar Shaddaa's refugee quad: the upper level's
//! doors, routed at their far ends, held the Exile against the wall).

use std::sync::Arc;

use collision::{Contents, LevelBuilder, Tag, UNIT};

use crate::math::Vec3;
use crate::motion::AnimTable;
use crate::probe::{Checks, check};
use crate::world::Level;

pub fn probes() -> Checks {
    vec![("doorway_legs_pass_the_jamb", legs_pass())]
}

/// A cell's side (metres) and the rooms' floor: from x to x, from y to y.
const CELL: f32 = 0.5;
const ROOM_Y: [f32; 2] = [-2.0, 2.0];
const WEST_X: [f32; 2] = [-4.0, -0.5];
const EAST_X: [f32; 2] = [0.5, 4.0];
/// The doorway's floor (metres): through the wall, from y to y.
const DOOR_X: [f32; 2] = [-0.5, 0.5];
const DOOR_FLOOR_Y: [f32; 2] = [-1.0, 1.5];
/// The jambs (metres, from y to y): the south one at the floor's edge, the
/// north one standing a metre over the doorway's floor.
const SOUTH_JAMB_Y: [f32; 2] = [-2.0, -1.0];
const NORTH_JAMB_Y: [f32; 2] = [0.5, 2.0];
const WALL_HIGH: f32 = 3.0;
/// Where the walk starts and ends (metres, soles), along the north side.
const START: [f32; 2] = [-3.0, 1.5];
const GOAL: [f32; 2] = [3.0, 1.5];

/// The two triangles of each cell of `x` by `y` (metres) on the floor.
fn cells(x: [f32; 2], y: [f32; 2]) -> Vec<[Vec3; 3]> {
    let count = |[from, to]: [f32; 2]| ((to - from) / CELL) as usize;
    (0..count(x))
        .flat_map(|i| (0..count(y)).map(move |j| (i, j)))
        .flat_map(|(i, j)| {
            let (cx, cy) = (x[0] + i as f32 * CELL, y[0] + j as f32 * CELL);
            let [a, b, c, d] = [
                (cx, cy),
                (cx + CELL, cy),
                (cx + CELL, cy + CELL),
                (cx, cy + CELL),
            ]
            .map(|(x, y)| [x, y, 0.0]);
            [[a, b, c], [a, c, d]]
        })
        .collect()
}

fn level() -> Level {
    let triangles: Vec<[Vec3; 3]> = [(WEST_X, ROOM_Y), (EAST_X, ROOM_Y), (DOOR_X, DOOR_FLOOR_Y)]
        .into_iter()
        .flat_map(|(x, y)| cells(x, y))
        .collect();
    let mut builder = LevelBuilder::new();
    let solid = Tag::level(Contents::SOLID, 1);
    for &triangle in &triangles {
        builder.triangle(triangle, solid);
    }
    for [low, high] in [SOUTH_JAMB_Y, NORTH_JAMB_Y] {
        builder.aabb([DOOR_X[0], low, 0.0], [DOOR_X[1], high, WALL_HIGH], solid);
    }
    let nav = collision::nav::NavMesh::build(&triangles, crate::walk::CLEARANCE * UNIT);
    Level::new(
        "doorway",
        Arc::new(builder.build()),
        Arc::new(AnimTable::stand_in()),
    )
    .with_nav(nav)
}

/// The origin (units) of a character standing at `(x, y)` (metres) on the
/// floor.
fn origin_at([x, y]: [f32; 2]) -> Vec3 {
    [x / UNIT, y / UNIT, -crate::motion::SOLE_Z]
}

fn legs_pass() -> Result<String, String> {
    let level = level();
    let from = origin_at(START);
    let corners = crate::walk::route(&level, from, origin_at(GOAL), &|_| {
        crate::snapshot::ObjectState::default()
    })
    .ok_or("no route through the doorway")?;
    let points: Vec<Vec3> = std::iter::once(from)
        .chain(corners.iter().copied())
        .collect();
    let blocked = points
        .windows(2)
        .filter(|leg| !level.leg_fits_level(leg[0], leg[1]))
        .count();
    let shown: Vec<String> = corners
        .iter()
        .map(|c| format!("({:.2}, {:.2})", c[0] * UNIT, c[1] * UNIT))
        .collect();
    check(
        blocked == 0,
        format!(
            "{} corners {}, {blocked} legs the body does not pass (the jamb from y {})",
            corners.len(),
            shown.join(" "),
            NORTH_JAMB_Y[0]
        ),
    )
}
