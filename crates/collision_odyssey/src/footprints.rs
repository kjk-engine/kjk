//! Walls round placeables' footprints.
//!
//! KOTOR's walker never steps to a point over a non-walkable face of a
//! placeable's walkmesh (`<model>.pwk`), as it never steps off the room's
//! walkable faces. Most placeables' walkmeshes are only that: a footprint
//! lying flat on the floor (a swoop bike's, a desk's, a cage's ring), which
//! a JA box walks over as if it were floor. So the outline of a walkmesh's
//! non-walkable faces that face up gets upright walls, set back into the
//! footprint by the player's half width so that the box's centre, like
//! KOTOR's point, reaches the outline and no further: the edge walls'
//! rule ([`crate::edges`]) turned inward. At a corner of the outline the
//! walls meet where its two edges set back meet (short of a corner the
//! footprint turns in at, past one it turns out at, by at most the set
//! back), and half a thickness on so they touch; where the outline is not
//! one edge on, the wall ends at its edge's end. A corner sharper than a
//! right angle (a swoop bike's nose) also gets a wall down its middle,
//! from where its walls meet to the set back short of its point: walls set
//! back alone would meet far inside it, and a centre would reach in that
//! far. A footprint too small for any wall gets a post at its middle.
//!
//! They rise from a little under the footprint to past any jump, or up to
//! a walkable floor overhead, as the edge walls do.

use std::collections::{BTreeMap, HashMap};

use collision::Level;
use collision::clip_file::ClipShape;

use crate::Surfaces;
use crate::edges::{CLEARANCE, EdgeWalls, FOOT, PIECE, SET_BACK, THICKNESS, WELD, floor_above};

type Vertex = [i64; 3];

/// An edge of the outline seen from one of its ends.
#[derive(Debug, Clone, Copy)]
struct Spoke {
    /// The far end.
    far: Vertex,
    /// The way to it.
    towards: [f32; 2],
    /// The edge's normal into the footprint.
    in_n: [f32; 2],
    /// The edge's length.
    length: f32,
}

/// The walls round the footprint `faces` (world space: a walkmesh's
/// non-walkable faces that face up); `walkmesh` is the rooms' walkmesh,
/// for floors overhead.
pub fn walls(
    faces: &[[[f32; 3]; 3]],
    walkmesh: &Level,
    surfaces: &Surfaces,
    options: &EdgeWalls,
) -> Vec<ClipShape> {
    let key = |p: [f32; 3]| -> Vertex { p.map(|v| (v / WELD).round() as i64) };
    let faces: Vec<&[[f32; 3]; 3]> = faces
        .iter()
        .filter(|corners| {
            let [a, b, c] = corners.map(key);
            a != b && b != c && c != a
        })
        .collect();
    let Some(bottom) = faces
        .iter()
        .flat_map(|corners| corners.iter().map(|p| p[2]))
        .reduce(f32::min)
    else {
        return Vec::new();
    };
    // Each edge's sides that a face lies on (bit 0 left of it, bit 1
    // right, from its lower vertex key to its higher): an edge with
    // footprint on both sides is inside it.
    let mut sides: HashMap<(Vertex, Vertex), u8> = HashMap::new();
    for corners in faces.iter().copied() {
        for k in 0..3 {
            let (p, q, r) = (corners[k], corners[(k + 1) % 3], corners[(k + 2) % 3]);
            let (a, b) = (key(p), key(q));
            let (from, to) = if a <= b { (p, q) } else { (q, p) };
            let cross = (to[0] - from[0]) * (r[1] - from[1]) - (to[1] - from[1]) * (r[0] - from[0]);
            *sides.entry((a.min(b), a.max(b))).or_insert(0) |= if cross > 0.0 { 1 } else { 2 };
        }
    }
    let outline_edge =
        |a: Vertex, b: Vertex| sides.get(&(a.min(b), a.max(b))).copied().unwrap_or(0) != 3;
    // The outline's edges at each vertex: the far vertex, the direction to
    // it, and the edge's inward normal.
    let mut outline: BTreeMap<Vertex, ([f32; 3], Vec<Spoke>)> = BTreeMap::new();
    let mut edges = Vec::new();
    for corners in faces.iter().copied() {
        let centre: [f32; 2] =
            std::array::from_fn(|i| (corners[0][i] + corners[1][i] + corners[2][i]) / 3.0);
        for k in 0..3 {
            let (a, b) = (corners[k], corners[(k + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            if !outline_edge(ka, kb) {
                continue;
            }
            let along = [b[0] - a[0], b[1] - a[1]];
            let length = (along[0] * along[0] + along[1] * along[1]).sqrt();
            if length < 0.01 {
                continue;
            }
            let dir = [along[0] / length, along[1] / length];
            let mut in_n = [dir[1], -dir[0]];
            if in_n[0] * (centre[0] - a[0]) + in_n[1] * (centre[1] - a[1]) < 0.0 {
                in_n = [-in_n[0], -in_n[1]];
            }
            for (at, point, far, towards) in [(ka, a, kb, dir), (kb, b, ka, [-dir[0], -dir[1]])] {
                let (_, spokes) = outline.entry(at).or_insert_with(|| (point, Vec::new()));
                if !spokes.iter().any(|spoke| spoke.far == far) {
                    spokes.push(Spoke {
                        far,
                        towards,
                        in_n,
                        length,
                    });
                }
            }
            edges.push((a, b, dir, length, in_n));
        }
    }
    let back = SET_BACK + THICKNESS * 0.5;
    // The other edge of the outline at `at` than the one to `away`, if it
    // has exactly one.
    let next = |at: [f32; 3], away: [f32; 3]| -> Option<Spoke> {
        let from = key(away);
        let (_, spokes) = outline.get(&key(at))?;
        let others: Vec<&Spoke> = spokes.iter().filter(|spoke| spoke.far != from).collect();
        let [next] = others.as_slice() else {
            return None;
        };
        Some(**next)
    };
    // How far the wall along an edge runs on past its end `at` (back along
    // the edge from `away`, `out` pointing past the end): to where the next
    // edge's wall, set back as far, meets it; less than nothing short of a
    // corner the footprint turns in at.
    let run_on = |at: [f32; 3], away: [f32; 3], out: [f32; 2], in_n: [f32; 2]| -> f32 {
        let Some(next) = next(at, away) else {
            return 0.0;
        };
        let across = out[0] * next.in_n[0] + out[1] * next.in_n[1];
        if across.abs() < 1e-3 {
            return 0.0;
        }
        let meet = back * (1.0 - (in_n[0] * next.in_n[0] + in_n[1] * next.in_n[1])) / across;
        meet.min(back) + THICKNESS * 0.5
    };
    let top = |from: [f32; 2], to: [f32; 2], edge: f32| {
        let length = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        let samples = (length / PIECE).ceil().max(1.0) as usize;
        (0..=samples)
            .map(|k| {
                let t = k as f32 / samples as f32;
                let p = std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t);
                floor_above(
                    walkmesh,
                    surfaces,
                    p,
                    edge + CLEARANCE,
                    edge + options.height,
                )
            })
            .fold(f32::INFINITY, f32::min)
    };
    let mut out = Vec::new();
    // Over the footprint, seen from above.
    let over = |p: [f32; 2]| faces.iter().any(|corners| inside(p, corners));
    // A wall's stretches over the footprint: where a part narrower than a
    // box (a droid's arm, a bike's nose) leaves a wall set back past its
    // far edge, that stretch is left out, else it stood out beside the
    // footprint and boxed a walker into the pocket between two (Manaan's
    // broken droid).
    let mut wall = |from: [f32; 2], to: [f32; 2], edge: f32| {
        let length = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
        let samples = (length / SAMPLE).ceil().max(1.0) as usize;
        let at = |k: usize| -> [f32; 2] {
            let t = k as f32 / samples as f32;
            std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t)
        };
        let kept: Vec<bool> = (0..=samples).map(|k| over(at(k))).collect();
        let mut k = 0;
        while k <= samples {
            if !kept[k] {
                k += 1;
                continue;
            }
            let first = k;
            while k < samples && kept[k + 1] {
                k += 1;
            }
            let (a, b) = (at(first), at(k));
            if first < k || samples == 0 {
                out.push(ClipShape::Wall {
                    from: a,
                    to: b,
                    z: [bottom - FOOT, top(a, b, edge)],
                    thickness: THICKNESS,
                });
            }
            k += 1;
        }
    };
    for (a, b, dir, length, in_n) in edges {
        let before = run_on(a, b, [-dir[0], -dir[1]], in_n);
        let after = run_on(b, a, dir, in_n);
        if length + before + after <= 0.0 {
            continue;
        }
        let from = [
            a[0] - dir[0] * before + in_n[0] * back,
            a[1] - dir[1] * before + in_n[1] * back,
        ];
        let to = [
            b[0] + dir[0] * after + in_n[0] * back,
            b[1] + dir[1] * after + in_n[1] * back,
        ];
        wall(from, to, a[2].max(b[2]));
    }
    // A wall down the middle of each corner sharper than a right angle.
    for (point, spokes) in outline.values() {
        let [one, two] = spokes.as_slice() else {
            continue;
        };
        let cos =
            (one.towards[0] * two.towards[0] + one.towards[1] * two.towards[1]).clamp(-1.0, 1.0);
        let middle = [
            one.towards[0] + two.towards[0],
            one.towards[1] + two.towards[1],
        ];
        let length = (middle[0] * middle[0] + middle[1] * middle[1]).sqrt();
        // Sharper than a right angle, and the footprint lies inside it.
        if cos <= 0.0 || length < 1e-3 || middle[0] * one.in_n[0] + middle[1] * one.in_n[1] <= 0.0 {
            continue;
        }
        let middle = [middle[0] / length, middle[1] / length];
        let half_sin = ((1.0 - cos) * 0.5).sqrt();
        // No farther in than the nearer end of its edges, seen along it.
        let reach = one.length.min(two.length) * ((1.0 + cos) * 0.5).sqrt();
        let meet = (back / half_sin.max(1e-3)).min(reach);
        if meet <= SET_BACK {
            continue;
        }
        let along = |by: f32| [point[0] + middle[0] * by, point[1] + middle[1] * by];
        wall(along(meet), along(SET_BACK), point[2]);
    }
    // A footprint too small for walls set back inside it (a jar's, a
    // crystal's) gets a post at its middle, which stops a box about as far
    // out as its outline would a point.
    if out.is_empty() {
        let mut area = 0.0;
        let mut middle = [0.0f32; 2];
        for corners in &faces {
            let [a, b, c] = **corners;
            let twice = ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs();
            area += twice;
            for (i, m) in middle.iter_mut().enumerate() {
                *m += twice * (a[i] + b[i] + c[i]) / 3.0;
            }
        }
        if area > 0.0 {
            let middle = middle.map(|m| m / area);
            let half = THICKNESS * 0.5;
            let top = top(middle, middle, bottom);
            out.push(ClipShape::Box {
                min: [middle[0] - half, middle[1] - half, bottom - FOOT],
                max: [middle[0] + half, middle[1] + half, top],
            });
        }
    }
    out
}

/// How far above or below a floor face a footprint still stands on it.
const ON_FLOOR: f32 = 0.5;
/// How far apart a wall is looked at for whether it stands over its
/// footprint (metres).
const SAMPLE: f32 = 0.05;

/// Whether `p` lies within a triangle's outline seen from above.
fn inside(p: [f32; 2], corners: &[[f32; 3]; 3]) -> bool {
    let side =
        |a: [f32; 3], b: [f32; 3]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let [a, b, c] = *corners;
    let (d0, d1, d2) = (side(a, b), side(b, c), side(c, a));
    (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
}

/// A piece of a cut face smaller than this (square metres) is dropped.
const SLIVER: f32 = 1e-4;

/// The floor faces `floor` cut along the outlines of the footprints'
/// faces (`footprints`, world space) that stand on them, seen from above:
/// each face's part under a footprint and its parts beside it become faces
/// of their own, at the face's height. A navigation mesh baked from them
/// marks the faces under a footprint (`collision::nav::NavMesh::mark`) and
/// routes round it, as KOTOR's walker goes round a placeable's walkmesh;
/// uncut, a street's face a footprint stood on in part was either shut
/// whole or crossed straight through the footprint.
pub fn cut(floor: &[[[f32; 3]; 3]], footprints: &[[[f32; 3]; 3]]) -> Vec<[[f32; 3]; 3]> {
    let bounds = |points: &[[f32; 3]]| {
        points.iter().fold(
            ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]),
            |(lo, hi), p| {
                (
                    std::array::from_fn(|i| lo[i].min(p[i])),
                    std::array::from_fn(|i| hi[i].max(p[i])),
                )
            },
        )
    };
    let prints: Vec<_> = footprints
        .iter()
        .map(|corners| (corners, bounds(corners)))
        .collect();
    let mut out = Vec::with_capacity(floor.len());
    for face in floor {
        let (lo, hi) = bounds(face);
        let over: Vec<&[[f32; 3]; 3]> = prints
            .iter()
            .filter(|(_, (plo, phi))| {
                (0..2).all(|i| plo[i] < hi[i] && phi[i] > lo[i])
                    && plo[2] < hi[2] + ON_FLOOR
                    && phi[2] > lo[2] - ON_FLOOR
            })
            .map(|(corners, _)| *corners)
            .collect();
        if over.is_empty() {
            out.push(*face);
            continue;
        }
        let mut pieces: Vec<Vec<[f32; 2]>> = vec![face.iter().map(|p| [p[0], p[1]]).collect()];
        for print in over {
            let print: [[f32; 2]; 3] = print.map(|p| [p[0], p[1]]);
            pieces = pieces
                .into_iter()
                .flat_map(|piece| split(piece, &print))
                .collect();
        }
        let height = plane_height(face);
        for piece in pieces {
            for k in 1..piece.len().saturating_sub(1) {
                let tri = [piece[0], piece[k], piece[k + 1]];
                if area(&tri) >= SLIVER {
                    out.push(tri.map(|p| [p[0], p[1], height(p)]));
                }
            }
        }
    }
    out
}

/// A convex piece split by a triangle: its part inside, and the parts
/// outside each edge in turn (convex, none overlapping).
fn split(piece: Vec<[f32; 2]>, triangle: &[[f32; 2]; 3]) -> Vec<Vec<[f32; 2]>> {
    // Counter-clockwise, so "inside" is left of each edge.
    let mut t = *triangle;
    if (t[1][0] - t[0][0]) * (t[2][1] - t[0][1]) - (t[1][1] - t[0][1]) * (t[2][0] - t[0][0]) < 0.0 {
        t.swap(1, 2);
    }
    let mut out = Vec::new();
    let mut rest = piece;
    for k in 0..3 {
        let (a, b) = (t[k], t[(k + 1) % 3]);
        let side = |p: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
        let outside = clip(&rest, &|p| -side(p));
        if outside.len() >= 3 && polygon_area(&outside) >= SLIVER {
            out.push(outside);
        }
        rest = clip(&rest, &side);
        if rest.len() < 3 {
            return out;
        }
    }
    out.push(rest);
    out
}

/// The part of a convex polygon where `side` is not negative.
fn clip(polygon: &[[f32; 2]], side: &dyn Fn([f32; 2]) -> f32) -> Vec<[f32; 2]> {
    let mut out = Vec::new();
    for k in 0..polygon.len() {
        let (p, q) = (polygon[k], polygon[(k + 1) % polygon.len()]);
        let (sp, sq) = (side(p), side(q));
        if sp >= 0.0 {
            out.push(p);
        }
        if (sp >= 0.0) != (sq >= 0.0) {
            let t = sp / (sp - sq);
            out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
        }
    }
    out
}

fn area(tri: &[[f32; 2]; 3]) -> f32 {
    polygon_area(tri)
}

fn polygon_area(polygon: &[[f32; 2]]) -> f32 {
    let mut twice = 0.0;
    for k in 0..polygon.len() {
        let (p, q) = (polygon[k], polygon[(k + 1) % polygon.len()]);
        twice += p[0] * q[1] - q[0] * p[1];
    }
    (twice * 0.5).abs()
}

/// The height of a face's plane over a point (its corners' mean where the
/// face stands upright).
fn plane_height(face: &[[f32; 3]; 3]) -> impl Fn([f32; 2]) -> f32 {
    let [a, b, c] = *face;
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let mean = (a[2] + b[2] + c[2]) / 3.0;
    move |p: [f32; 2]| {
        if n[2].abs() < 1e-6 {
            mean
        } else {
            a[2] - (n[0] * (p[0] - a[0]) + n[1] * (p[1] - a[1])) / n[2]
        }
    }
}
