//! Walls round the tops of tall structures over the walkmesh.
//!
//! KOTOR moves a character only over its walkmesh's walkable faces, so the
//! top of anything that stands high over that floor (the wall tops and the
//! door frames round Tatooine's docking bay, the Ebon Hawk's hull) is a
//! place no KOTOR character ever stands. The edge walls ([`crate::edges`])
//! stand along the walkmesh's outline and keep a jumping body inside it,
//! but these tops lie inside the outline, over the floor itself. A Force
//! jump from the floor, or from a crate, reached them, and from there a
//! player ran along the tops of the area's walls.
//!
//! So the area is looked down on in columns half a metre wide. In each
//! column the player's box is let down to the first thing it rests on. It
//! starts 12.5 m over the highest walkmesh face under its footprint, higher
//! than a rank 3 Force jump carries a body. Where the box does not fit
//! there (inside a ship's hull), it starts a metre lower, and so on.
//!
//! Under each part of the box's footprint (its middle and its four
//! corners), the look goes down from where the box rests, through
//! walkmesh faces that are not walkable (the top's own face, a ledge of
//! the same wall), to the first walkable face, else to the lowest face. A
//! column is a high top when that face lies more than the audit's perch
//! height ([`collision::PERCH`], 2.5 m) under the box for some part
//! of the footprint, and no part has a face within that height. A box on
//! the floor, on a crate or on a console is no high top. Nor is a column
//! with a walkable floor beside it, within a half width of its sides,
//! from 2.5 m under the top to 0.35 m over it: a box on that floor (a
//! catwalk past a ledge) reaches over the column's side, and a wall there
//! would stop it before the floor's edge.
//!
//! Each side of a high top's column gets a wall from just under the top:
//! - beside a column that is no high top, up to the sky or to just under
//!   the lowest walkable floor over it, looked for across a band a half
//!   width either side of the side, so a floor ending near the side still
//!   stops the wall;
//! - beside a higher top, up to that top, so that a body on a ledge under
//!   an overhang does not step across onto the lower top beside it;
//! - beside a top about as high (within a step), none.
//!
//! Sides one after another along a line make one wall while their feet,
//! and their tops, stay within 0.25 m of each other. A wall runs from the
//! lowest foot to the lowest top of its sides, so it is never more than
//! 0.25 m lower than any of them. A wall stops a walker's moves only
//! (`WALK_CLIP`), as the edge walls do. The camera, shots and sight pass through it. A body on the
//! floor under a top never meets one, since each wall starts more than
//! 2.15 m (the perch height, less a column's foot and the merge) over the
//! walkmesh, higher than a standing body.

use collision::clip_file::ClipShape;
use collision::player::{HALF_WIDTH, SOLE_Z, STAND_HEIGHT};
use collision::{CollisionWorld, Contents, Extents, Level, PERCH, UNIT};

use crate::Surfaces;
use crate::edges::{FOOT, HEIGHT, PIECE, SET_BACK, THICKNESS};

/// How high over the highest walkmesh face under a column the box is let
/// down from: over the highest a rank 3 Force jump carries a body (the
/// audit's 12.44 m). A top higher than that is reached by no jump.
const REACH: f32 = 12.5;
/// How far over the spot the box rests on a walkmesh face may lie and
/// still be at that spot (the containment audit's in-bounds height).
const AT: f32 = 0.35;
/// How much higher a top beside another must stand to need a wall between
/// them: a step.
const STEP: f32 = collision::player::STEP_HEIGHT * UNIT;
/// How far apart the highest and the lowest foot, and the highest and the
/// lowest top, of the sides that make one wall may be.
pub(crate) const MERGE: f32 = 0.25;
/// How far below a face the look down goes on.
const PAST: f32 = 0.02;
/// How far below the lowest walkmesh face the looks down reach.
const DEPTH: f32 = 1000.0;

/// What looks down at the walkmesh: a few millimetres wide, so it does not
/// slip through the seam between two faces (the columns' middles fall on
/// a grid that many walkmeshes' corners lie on too).
const GRAIN: Extents = Extents {
    mins: [-0.005, -0.005, 0.0],
    maxs: [0.005, 0.005, 0.0],
};

/// What lies under one point of the box's footprint.
#[derive(Clone, Copy, PartialEq)]
enum Under {
    /// A walkmesh face within [`PERCH`] under the spot: the floor, or a low
    /// perch.
    Near,
    /// The nearest walkmesh face under the spot lies farther down.
    Far,
    /// No walkmesh face.
    Other,
}

/// The walls round the high tops over the walkable faces `walkable` of
/// `walkmesh` (every walkmesh face of the area), with `solid` what a body
/// rests on (the rooms, the doors and the placeables' shapes).
/// `surfaces` tells which walkmesh faces are walkable.
pub fn walls(
    walkable: &[([[f32; 3]; 3], u32)],
    walkmesh: &Level,
    solid: &Level,
    surfaces: &Surfaces,
) -> Vec<ClipShape> {
    let Some(bounds) = bounds(walkable) else {
        return Vec::new();
    };
    let (low, high) = (bounds[0][2] - DEPTH, bounds[1][2] + DEPTH);
    // The player's standing box.
    let player = Extents {
        mins: [-HALF_WIDTH * UNIT, -HALF_WIDTH * UNIT, SOLE_Z * UNIT],
        maxs: [HALF_WIDTH * UNIT, HALF_WIDTH * UNIT, STAND_HEIGHT * UNIT],
    };
    let rests_on = Contents(Contents::SOLID.0 | Contents::SHAPE.0);
    let face_under = |x: f32, y: f32, from: f32| {
        if from <= low {
            return None;
        }
        let hit = walkmesh.trace([x, y, from], [x, y, low], &GRAIN, Contents::SOLID);
        (hit.fraction < 1.0).then(|| {
            (
                hit.end[2],
                surfaces.walkable(u32::from(hit.surface.material)),
            )
        })
    };
    // What lies under one point of the footprint of a box resting at `spot`:
    // the look goes down through faces that are not walkable to the first
    // walkable one, else to the lowest. Once a face lies farther down than
    // the perch height, no face below it can make the spot a perch.
    let under = |x: f32, y: f32, spot: f32| {
        let mut from = spot + AT;
        let mut lowest = None;
        while let Some((z, walkable)) = face_under(x, y, from) {
            lowest = Some(z);
            if walkable || spot - z > PERCH {
                break;
            }
            from = z - PAST;
        }
        match lowest {
            None => Under::Other,
            Some(z) if spot - z <= PERCH => Under::Near,
            Some(_) => Under::Far,
        }
    };
    // Whether a walkable face lies under (x, y) from `high` down to `low`.
    let walkable_between = |x: f32, y: f32, high: f32, low: f32| {
        let mut from = high;
        while let Some((z, walkable)) = face_under(x, y, from) {
            if z < low {
                return false;
            }
            if walkable {
                return true;
            }
            from = z - PAST;
        }
        false
    };
    let half = player.maxs[0];
    let footprint = [
        (0.0, 0.0),
        (-half, -half),
        (half, -half),
        (-half, half),
        (half, half),
    ];
    // Round the column, as far as a box on a walkable floor beside it
    // reaches over the column's side: half a column and a half width.
    let reach = PIECE * 0.5 + SET_BACK;
    let ring = [
        (-reach, 0.0),
        (reach, 0.0),
        (0.0, -reach),
        (0.0, reach),
        (-reach, -reach),
        (reach, -reach),
        (-reach, reach),
        (reach, reach),
    ];
    // Where the box rests in the column at (x, y), when that is a high top.
    // The column's floor is the highest walkmesh face under its footprint:
    // a mast stands in a hole of the walkmesh, with no face under its
    // middle.
    let top = |x: f32, y: f32| -> Option<f32> {
        let floor = footprint
            .iter()
            .filter_map(|(dx, dy)| face_under(x + dx, y + dy, high))
            .map(|(z, _)| z)
            .reduce(f32::max)?;
        // From above the highest jump, or, where the box does not fit
        // there (inside a ship's hull), from the first height a metre at a
        // time lower down where it does.
        let end = [x, y, floor - 1.0];
        let mut from = floor + REACH;
        let hit = loop {
            let hit = solid.trace([x, y, from], end, &player, rests_on);
            if !hit.start_solid {
                break hit;
            }
            from -= 1.0;
            if from < floor + PERCH {
                return None;
            }
        };
        if hit.fraction >= 1.0 {
            return None;
        }
        let spot = hit.end[2] + player.mins[2];
        let found = footprint.map(|(dx, dy)| under(x + dx, y + dy, spot));
        if found.contains(&Under::Near) || !found.contains(&Under::Far) {
            return None;
        }
        // A walkable floor beside the column about as high as the top (a
        // catwalk past a ledge) makes the top a perch beside that floor: a
        // wall on the column's side would stand in the way of a box on the
        // floor.
        let beside = ring
            .iter()
            .any(|(dx, dy)| walkable_between(x + dx, y + dy, spot + AT, spot - PERCH));
        (!beside).then_some(spot)
    };
    // The lowest walkable floor over a side from `low` up to `high`, less
    // [`FOOT`], else `high`: looked for across a band as wide as a box
    // reaches either side of the side's line, so a floor that ends near the
    // line still stops the wall under it.
    let floor_over = |from: [f32; 2], to: [f32; 2], low: f32, high: f32| {
        let length = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        let across = [-(to[1] - from[1]) / length, (to[0] - from[0]) / length];
        let mut lowest = high;
        for t in [0.0, 0.5, 1.0] {
            for out in [-SET_BACK, 0.0, SET_BACK] {
                let p = [
                    from[0] + (to[0] - from[0]) * t + across[0] * out,
                    from[1] + (to[1] - from[1]) * t + across[1] * out,
                ];
                let mut z = low;
                while z < lowest {
                    let hit = walkmesh.trace(
                        [p[0], p[1], z],
                        [p[0], p[1], lowest],
                        &GRAIN,
                        Contents::SOLID,
                    );
                    if hit.fraction >= 1.0 {
                        break;
                    }
                    if surfaces.walkable(u32::from(hit.surface.material)) {
                        lowest = lowest.min(hit.end[2] - FOOT);
                        break;
                    }
                    z = hit.end[2] + PAST;
                }
            }
        }
        lowest
    };

    let cell = PIECE;
    let origin = [
        (bounds[0][0] / cell).floor() * cell,
        (bounds[0][1] / cell).floor() * cell,
    ];
    let columns = [
        ((bounds[1][0] - origin[0]) / cell).ceil() as usize,
        ((bounds[1][1] - origin[1]) / cell).ceil() as usize,
    ];
    let corner = |i: usize, j: usize| [origin[0] + i as f32 * cell, origin[1] + j as f32 * cell];
    let tops: Vec<Option<f32>> = (0..columns[1])
        .flat_map(|j| (0..columns[0]).map(move |i| (i, j)))
        .map(|(i, j)| {
            let [x, y] = corner(i, j);
            top(x + cell * 0.5, y + cell * 0.5)
        })
        .collect();
    let high_top = |i: isize, j: isize| -> Option<f32> {
        if i < 0 || j < 0 || i as usize >= columns[0] || j as usize >= columns[1] {
            return None;
        }
        tops[j as usize * columns[0] + i as usize]
    };

    // The ends of a stretch of the grid's lines: which way the line runs (0
    // along y, 1 along x), the line, and the first and last corners along it.
    let ends = |way: u8, line: usize, first: usize, last: usize| match way {
        0 => (corner(line, first), corner(line, last)),
        _ => (corner(first, line), corner(last, line)),
    };
    // Each wall a side of a column needs: which way its line runs (0 along
    // y, 1 along x), the line, where along it the side starts, and its
    // heights.
    let mut sides: Vec<(u8, usize, usize, [f32; 2])> = Vec::new();
    for (index, spot) in tops.iter().enumerate() {
        let Some(spot) = *spot else {
            continue;
        };
        let (i, j) = (index % columns[0], index / columns[0]);
        let (si, sj) = (i as isize, j as isize);
        for (beside, side) in [
            ((si - 1, sj), (0, i, j)),
            ((si + 1, sj), (0, i + 1, j)),
            ((si, sj - 1), (1, j, i)),
            ((si, sj + 1), (1, j + 1, i)),
        ] {
            // Beside a column that is no high top, the wall goes up to the
            // sky. Beside a higher top, it goes up to that top: a body under
            // the higher top (on a ledge beneath an overhang) does not step
            // across onto this one. Beside a top about as high, there is no
            // wall.
            let roof = match high_top(beside.0, beside.1) {
                None => {
                    let (from, to) = ends(side.0, side.1, side.2, side.2 + 1);
                    floor_over(from, to, spot + AT, spot + HEIGHT)
                }
                Some(higher) if higher - spot > STEP => higher,
                Some(_) => continue,
            };
            sides.push((side.0, side.1, side.2, [spot - FOOT, roof]));
        }
    }
    // Sides one after another along a line, with heights close together,
    // make one wall ([`merge`]).
    sides.sort_by_key(|a| (a.0, a.1, a.2));
    merge(sides)
        .into_iter()
        .map(|(way, line, first, last, [feet, tops])| {
            // From the lowest foot to the lowest top: no side's top is
            // raised into a floor overhead.
            let (from, to) = ends(way, line, first, last);
            ClipShape::Wall {
                from,
                to,
                z: [feet[0], tops[0]],
                thickness: THICKNESS,
            }
        })
        .collect()
}

/// A run of sides along one grid line: which way the line runs, the line,
/// the first side and the one past the last, and the lowest and highest
/// foot and top of its sides.
pub(crate) type Run = (u8, usize, usize, usize, [[f32; 2]; 2]);

/// Sides one after another along a line (sorted by the line, then along
/// it), with heights `[foot, top]`, made into runs. A side joins the run
/// before it while the run's feet and its tops each stay within [`MERGE`]
/// of each other, so a wall along a sloped top follows the slope in
/// pieces and never drifts far below or above any of its sides.
pub(crate) fn merge(sides: Vec<(u8, usize, usize, [f32; 2])>) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (way, line, at, z) in sides {
        if let Some(run) = runs.last_mut()
            && run.0 == way
            && run.1 == line
            && run.3 == at
        {
            let feet = [run.4[0][0].min(z[0]), run.4[0][1].max(z[0])];
            let tops = [run.4[1][0].min(z[1]), run.4[1][1].max(z[1])];
            if feet[1] - feet[0] <= MERGE && tops[1] - tops[0] <= MERGE {
                run.3 = at + 1;
                run.4 = [feet, tops];
                continue;
            }
        }
        runs.push((way, line, at, at + 1, [[z[0], z[0]], [z[1], z[1]]]));
    }
    runs
}

/// The box round the walkable faces, if there are any.
fn bounds(walkable: &[([[f32; 3]; 3], u32)]) -> Option<[[f32; 3]; 2]> {
    let mut corners = walkable
        .iter()
        .flat_map(|(corners, _)| corners.iter().copied());
    let first = corners.next()?;
    Some(corners.fold([first, first], |[min, max], p| {
        [
            std::array::from_fn(|i| min[i].min(p[i])),
            std::array::from_fn(|i| max[i].max(p[i])),
        ]
    }))
}
