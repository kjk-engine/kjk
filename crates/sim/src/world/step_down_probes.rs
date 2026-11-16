//! The probe of a character stepping down off a raised floor its walk's
//! goal is not on (`kjk sim probes --only step down`; `walk::down_to`, the
//! playthrough's driver), no install: a ledge 1 m over a floor, a wall
//! along its north edge, the goal on the floor far north. The character
//! steps down where it can walk straight over to, off the open south
//! side, not towards the floor beyond the wall (Dantooine's Enclave
//! sublevel: the Exile up on the ledge round the dung pit, held against
//! the ledge's wall).

use std::sync::Arc;

use collision::{Contents, LevelBuilder, Tag, UNIT};

use crate::math::Vec3;
use crate::motion::AnimTable;
use crate::probe::{Checks, check};
use crate::world::Level;

pub fn probes() -> Checks {
    vec![("steps_down_where_it_can_walk", steps_down())]
}

/// The floor's cells (metres): its extent and a cell's side.
const FLOOR_X: [f32; 2] = [-5.0, 5.0];
const FLOOR_Y: [f32; 2] = [-4.0, 7.0];
const CELL: f32 = 0.5;
/// The ledge's half side and height (metres), round the origin.
const LEDGE_HALF: f32 = 1.0;
const LEDGE_HIGH: f32 = 1.0;
/// The wall along the ledge's north edge (metres): from y to y, its height.
const WALL_Y: [f32; 2] = [1.0, 1.4];
const WALL_HIGH: f32 = 3.0;
/// Where the character stands on the ledge and where its goal is
/// (metres, soles).
const ON_LEDGE: [f32; 2] = [0.0, 0.5];
const GOAL: [f32; 2] = [0.0, 6.5];

/// The two triangles of the cell at `(x, y)` (its south-west corner),
/// `z` high.
fn cell(x: f32, y: f32, z: f32) -> [[Vec3; 3]; 2] {
    let [a, b, c, d] =
        [(x, y), (x + CELL, y), (x + CELL, y + CELL), (x, y + CELL)].map(|(x, y)| [x, y, z]);
    [[a, b, c], [a, c, d]]
}

/// The cells of `x` by `y` (metres) at `z`, but those `keep` turns down.
fn cells(x: [f32; 2], y: [f32; 2], z: f32, keep: impl Fn(f32, f32) -> bool) -> Vec<[Vec3; 3]> {
    let count = |[from, to]: [f32; 2]| ((to - from) / CELL) as usize;
    (0..count(x))
        .flat_map(|i| (0..count(y)).map(move |j| (i, j)))
        .map(|(i, j)| (x[0] + i as f32 * CELL, y[0] + j as f32 * CELL))
        .filter(|&(cx, cy)| keep(cx, cy))
        .flat_map(|(cx, cy)| cell(cx, cy, z))
        .collect()
}

fn level() -> Level {
    let side = -LEDGE_HALF..LEDGE_HALF;
    let under_ledge = |x: f32, y: f32| side.contains(&x) && side.contains(&y);
    let mut triangles = cells(FLOOR_X, FLOOR_Y, 0.0, |x, y| !under_ledge(x, y));
    let ledge = [-LEDGE_HALF, LEDGE_HALF];
    triangles.extend(cells(ledge, ledge, LEDGE_HIGH, |_, _| true));
    let mut builder = LevelBuilder::new();
    let solid = Tag::level(Contents::SOLID, 1);
    for &triangle in &triangles {
        builder.triangle(triangle, solid);
    }
    builder.aabb(
        [-LEDGE_HALF, -LEDGE_HALF, 0.0],
        [LEDGE_HALF, LEDGE_HALF, LEDGE_HIGH],
        solid,
    );
    builder.aabb(
        [FLOOR_X[0], WALL_Y[0], 0.0],
        [FLOOR_X[1], WALL_Y[1], WALL_HIGH],
        solid,
    );
    let nav = collision::nav::NavMesh::build(&triangles, crate::walk::CLEARANCE * UNIT);
    Level::new(
        "step_down",
        Arc::new(builder.build()),
        Arc::new(AnimTable::stand_in()),
    )
    .with_nav(nav)
}

/// The origin (units) of a character standing at `feet` (metres).
fn origin_at([x, y, z]: Vec3) -> Vec3 {
    [x / UNIT, y / UNIT, z / UNIT - crate::motion::SOLE_Z]
}

fn steps_down() -> Result<String, String> {
    let level = level();
    let from = origin_at([ON_LEDGE[0], ON_LEDGE[1], LEDGE_HIGH]);
    let goal = origin_at([GOAL[0], GOAL[1], 0.0]);
    let spot = crate::walk::down_to(&level, from, goal).ok_or("no spot to step down to")?;
    let (y, passes) = (spot[1] * UNIT, level.passes(from, spot));
    check(
        passes && y < WALL_Y[0],
        format!(
            "steps down towards y {y:.2} m (the wall at {WALL_Y:?}), a straight walk there: {passes}"
        ),
    )
}
