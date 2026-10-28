//! A navigation mesh from a level's walkable faces (I27): where characters
//! can walk, which faces lead into which, and routes across them.
//!
//! Built from the walkmesh's walkable triangles (world space, metres),
//! but those steeper than move_body walks (KOTOR's floors go steeper than
//! JA's): corners closer than a millimetre are one vertex; two faces sharing an
//! edge are neighbours; a boundary edge lying along another's (KOTOR's
//! rooms meet edge to edge, not always vertex to vertex) is a portal over
//! the stretch they share. A route is A* over faces, from face centre to
//! face centre in whole millimetres, then pulled tight through the portals
//! (the funnel). A corner on the mesh's edge is held a body's half-width
//! off the walls that meet there (along their bisector, far enough to clear
//! each); a vertex inside the mesh needs no room and gets none.
//!
//! Everything is whole-millimetre keys, sorted maps and + − × ÷ and sqrt,
//! so every machine bakes the same mesh (its `digest`) and finds the same
//! routes. A face a door or placeable stands on (in its closed, in-place
//! state: `mark`) carries that entity, and a route passes it only where the
//! caller says it may (an open or openable door).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use crate::player::{STEP_HEIGHT, WALKABLE_NORMAL_Z};
use crate::vector::sub;

type Vec3 = [f32; 3];

/// A face's edge as the face winds it: the face, from, to.
type Edge = (u32, Vec3, Vec3);

/// Corners this close (metres) are one vertex.
const WELD: f32 = 0.001;
/// A seam: boundary edges this close to one line (metres), sharing at
/// least `SEAM_OVERLAP` of it, at most `SEAM_RISE` apart in height.
const SEAM_OFF: f32 = 0.02;
const SEAM_OVERLAP: f32 = 0.05;
const SEAM_RISE: f32 = 0.3;
/// The lookup grid's cell (metres).
const CELL: f32 = 2.0;

/// A face's way into a neighbour: which, and the stretch of edge between
/// them (its two ends, as the face's winding meets them), with where a
/// body turns round each end (stepped off the walls there, if the end is
/// on the mesh's edge; the end itself inside the mesh).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Portal {
    pub to: u32,
    pub a: Vec3,
    pub b: Vec3,
    pub a_round: Vec3,
    pub b_round: Vec3,
}

/// One walkable face.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub corners: [Vec3; 3],
    pub centre: Vec3,
    pub portals: Vec<Portal>,
    /// Its island: faces reach each other only within one.
    pub island: u32,
    /// The door or placeable standing on it, if any (`mark`).
    pub entity: Option<u32>,
    /// Furniture over it, the level's own (`mark`): a table's top, a
    /// console, a prop drawn without a walkmesh. A body cannot stand there,
    /// so routes go round it.
    pub covered: bool,
}

/// move_body's step (18 units, metres): a steep face no taller is stepped over.
const STEP_RISE: f32 = STEP_HEIGHT * crate::UNIT;

/// Where a point is looked for over a footprint (`mark`): a thin column
/// from under the face to a step over it.
const FOOTPRINT: crate::Extents = crate::Extents {
    mins: [-0.05, -0.05, -0.1],
    maxs: [0.05, 0.05, STEP_RISE],
};

/// Shares of a corner's step tried, the whole first (`step_off`).
const ROUND_SHARES: u32 = 8;
/// Shares of the rounding a route is pulled with, the whole first, when
/// rounder corners leave the mesh (`route`).
const PULL_SHARES: u32 = 4;

/// See the module documentation.
#[derive(Debug, Clone, PartialEq)]
pub struct NavMesh {
    pub faces: Vec<Face>,
    /// Faces whose extent in x and y touches each grid cell, by cell.
    grid: BTreeMap<(i32, i32), Vec<u32>>,
    /// Portals from shared edges and from seams.
    pub shared: usize,
    pub seams: usize,
    pub islands: usize,
    /// Walkable faces left out as steeper than move_body walks.
    pub steep: usize,
    /// The body's half-width it was baked for (metres).
    pub radius: f32,
}

fn cross_z(a: Vec3, b: Vec3) -> f32 {
    a[0] * b[1] - a[1] * b[0]
}

/// How far `apex` stands from the stretch `(left, right)`, seen from above.
fn off_stretch(apex: Vec3, (left, right): (Vec3, Vec3)) -> f32 {
    let (across, from) = (sub(left, right), sub(apex, right));
    let along = ((across[0] * from[0] + across[1] * from[1])
        / (across[0] * across[0] + across[1] * across[1]).max(f32::EPSILON))
    .clamp(0.0, 1.0);
    flat_length(apex, lerp(right, left, along))
}

/// Whether `apex` stands past the gate `(left, right)`: on its far side as
/// it is walked (left on the left), and between its ends seen across it.
fn past(apex: Vec3, (left, right): (Vec3, Vec3)) -> bool {
    let (across, from) = (sub(left, right), sub(apex, right));
    let along = (across[0] * from[0] + across[1] * from[1])
        / (across[0] * across[0] + across[1] * across[1]).max(f32::EPSILON);
    cross_z(across, from) < 0.0 && (0.0..=1.0).contains(&along)
}

/// How far (metres) from a portal's end `NavMesh::inside_corner` looks for
/// the floor, in how many looks evenly round it (5.625° apart), how many in
/// a row off the floor make it an inside corner (nominally the floor round
/// it within 191.25°, a way bending round it by 11.25° at most; the looks
/// quantize that to about 186° to 197°: Malak's door frame in the
/// Leviathan's hangar, meeting the wall beyond it at about 190°, has 30 in a
/// row), and how far under or over the end (metres) its floor is, as a
/// stepped end is kept on the mesh (`step_off`).
const INSIDE_REACH: f32 = 0.02;
const INSIDE_LOOKS: usize = 64;
const INSIDE_OFF: usize = 30;
const INSIDE_FLOOR: f32 = 0.5;

/// The distance between two points seen from above.
pub fn flat_length(a: Vec3, b: Vec3) -> f32 {
    let d = sub(a, b);
    (d[0] * d[0] + d[1] * d[1]).sqrt()
}

fn distance(a: Vec3, b: Vec3) -> f32 {
    let d = sub(a, b);
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

fn lerp(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    std::array::from_fn(|axis| a[axis] + (b[axis] - a[axis]) * t)
}

fn key(p: Vec3) -> [i32; 3] {
    p.map(|value| (value / WELD).round_ties_even() as i32)
}

fn cell(x: f32, y: f32) -> (i32, i32) {
    ((x / CELL).floor() as i32, (y / CELL).floor() as i32)
}

impl NavMesh {
    /// Bakes the mesh from walkable triangles (world space, metres) for
    /// bodies `radius` metres wide either side.
    pub fn build(triangles: &[[Vec3; 3]], radius: f32) -> NavMesh {
        // Welded vertices, faces wound counter-clockwise seen from above,
        // without the flat and the repeated.
        let mut vertices: BTreeMap<[i32; 3], u32> = BTreeMap::new();
        let mut points: Vec<Vec3> = Vec::new();
        let mut seen = BTreeMap::new();
        let mut faces: Vec<[u32; 3]> = Vec::new();
        let mut steep = 0;
        for triangle in triangles {
            let mut ids = triangle.map(|p| {
                let next = points.len() as u32;
                *vertices.entry(key(p)).or_insert_with(|| {
                    points.push(p);
                    next
                })
            });
            if ids[0] == ids[1] || ids[1] == ids[2] || ids[0] == ids[2] {
                continue;
            }
            let [a, b, c] = ids.map(|id| points[id as usize]);
            let turn = cross_z(sub(b, a), sub(c, a));
            if turn.abs() < 1e-6 {
                continue;
            }
            // Steeper than JA's walkable slope: KOTOR calls it floor, but
            // move_body slides off it.
            let (u, v) = (sub(b, a), sub(c, a));
            let normal = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], turn];
            let length =
                (normal[0] * normal[0] + normal[1] * normal[1] + normal[2] * normal[2]).sqrt();
            // One no taller than a step (debris, a bump in a lair's floor)
            // is walked over as a step: kept, or the floor beyond it is cut
            // off.
            let rise = [a[2], b[2], c[2]].iter().fold(f32::MIN, |m, z| m.max(*z))
                - [a[2], b[2], c[2]].iter().fold(f32::MAX, |m, z| m.min(*z));
            if turn.abs() < WALKABLE_NORMAL_Z * length && rise > STEP_RISE {
                steep += 1;
                continue;
            }
            if turn < 0.0 {
                ids.swap(1, 2);
            }
            let mut sorted = ids;
            sorted.sort_unstable();
            if seen.insert(sorted, ()).is_none() {
                faces.push(ids);
            }
        }
        let mut mesh = NavMesh {
            faces: faces
                .iter()
                .map(|ids| {
                    let corners = ids.map(|id| points[id as usize]);
                    let centre = std::array::from_fn(|axis| {
                        (corners[0][axis] + corners[1][axis] + corners[2][axis]) / 3.0
                    });
                    Face {
                        corners,
                        centre,
                        portals: Vec::new(),
                        island: 0,
                        entity: None,
                        covered: false,
                    }
                })
                .collect(),
            grid: BTreeMap::new(),
            shared: 0,
            seams: 0,
            islands: 0,
            steep,
            radius,
        };
        // Shared edges: every pair of faces on one edge, each edge kept as
        // its face winds it.
        let mut edges: BTreeMap<(u32, u32), Vec<Edge>> = BTreeMap::new();
        for (face, ids) in faces.iter().enumerate() {
            for side in 0..3 {
                let (a, b) = (ids[side], ids[(side + 1) % 3]);
                edges.entry((a.min(b), a.max(b))).or_default().push((
                    face as u32,
                    points[a as usize],
                    points[b as usize],
                ));
            }
        }
        let mut boundary = Vec::new();
        for sharing in edges.values() {
            if let [only] = sharing.as_slice() {
                boundary.push(*only);
                continue;
            }
            for (at, &(one, a, b)) in sharing.iter().enumerate() {
                for &(other, ..) in &sharing[at + 1..] {
                    mesh.link(one, other, a, b);
                    mesh.shared += 1;
                }
            }
        }
        let stitched = mesh.stitch(&boundary);
        mesh.seams = stitched.iter().filter(|seamed| **seamed).count();
        let walls: Vec<Edge> = boundary
            .iter()
            .zip(&stitched)
            .filter(|(_, seamed)| !**seamed)
            .map(|(edge, _)| *edge)
            .collect();
        mesh.index();
        mesh.step_off(&walls);
        mesh.islands = mesh.label();
        mesh
    }

    /// Links two faces through the stretch `a`–`b` (as `one` winds it; the
    /// other, beside it, winds it the other way), each way.
    fn link(&mut self, one: u32, other: u32, a: Vec3, b: Vec3) {
        self.faces[one as usize].portals.push(Portal {
            to: other,
            a,
            b,
            a_round: a,
            b_round: b,
        });
        self.faces[other as usize].portals.push(Portal {
            to: one,
            a: b,
            b: a,
            a_round: b,
            b_round: a,
        });
    }

    /// Where a body turns round each portal end: one on the mesh's edge (on
    /// walls: edges no face shares) stepped along the sum of the walls'
    /// inward normals, far enough to clear each wall by the body's reach;
    /// one inside the mesh but nearer a wall than that (a floor's vertex
    /// beside a wall's corner) stepped straight away from the nearest wall
    /// until it clears it. The farthest of `ROUND_SHARES` shares of that
    /// which stays on the mesh (a gap between two props), not at all if
    /// none does. The body is a box `radius` wide either side along
    /// the axes, as JA's is (and `mark`'s): across a wall that runs aslant
    /// it reaches further than across one along an axis (√2 as far at
    /// 45°), and a disc's clearance let a route along Manaan's slanted
    /// corridor walls graze them.
    fn step_off(&mut self, walls: &[Edge]) {
        let reach = |n: Vec3| self.radius * (n[0].abs() + n[1].abs());
        let mut normals: BTreeMap<[i32; 3], Vec<Vec3>> = BTreeMap::new();
        for &(_, a, b) in walls {
            let d = sub(b, a);
            let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
            if length < 1e-6 {
                continue;
            }
            // A face winds counter-clockwise: its inside is on the left.
            let inward = [-d[1] / length, d[0] / length, 0.0];
            for end in [a, b] {
                normals.entry(key(end)).or_default().push(inward);
            }
        }
        let outs: BTreeMap<[i32; 3], Vec3> = normals
            .into_iter()
            .filter_map(|(at, normals)| {
                let sum = normals
                    .iter()
                    .fold([0.0; 3], |sum, n| [sum[0] + n[0], sum[1] + n[1], 0.0]);
                let length = (sum[0] * sum[0] + sum[1] * sum[1]).sqrt();
                if length < 1e-6 {
                    return None;
                }
                let way = [sum[0] / length, sum[1] / length, 0.0];
                let step = normals
                    .iter()
                    .map(|n| reach(*n) / (way[0] * n[0] + way[1] * n[1]).max(0.3))
                    .fold(0.0f32, f32::max);
                Some((at, [way[0] * step, way[1] * step, 0.0]))
            })
            .collect();
        // The walls by grid cell, for the nearest to a point (within the
        // body's farthest reach, its corner's).
        let farthest = self.radius * std::f32::consts::SQRT_2;
        let mut near: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        for (at, &(_, a, b)) in walls.iter().enumerate() {
            let (low, high) = (
                cell(a[0].min(b[0]) - farthest, a[1].min(b[1]) - farthest),
                cell(a[0].max(b[0]) + farthest, a[1].max(b[1]) + farthest),
            );
            for x in low.0..=high.0 {
                for y in low.1..=high.1 {
                    near.entry((x, y)).or_default().push(at);
                }
            }
        }
        // How far `p` stands off the nearest wall within the body's reach
        // and a step in height, and the step away from it that clears it.
        let off_wall = |p: Vec3| -> Option<(f32, Vec3)> {
            near.get(&cell(p[0], p[1]))?
                .iter()
                .filter_map(|&at| {
                    let (_, a, b) = walls[at];
                    let d = sub(b, a);
                    let length = d[0] * d[0] + d[1] * d[1];
                    let t = if length < 1e-12 {
                        0.0
                    } else {
                        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length).clamp(0.0, 1.0)
                    };
                    let q = lerp(a, b, t);
                    let away = sub(p, q);
                    let gap = (away[0] * away[0] + away[1] * away[1]).sqrt();
                    let way = [away[0] / gap, away[1] / gap, 0.0];
                    let clear = reach(way);
                    (gap < clear && gap > 1e-4 && away[2].abs() <= SEAM_RISE)
                        .then(|| (gap, [way[0] * (clear - gap), way[1] * (clear - gap), 0.0]))
                })
                .min_by(|x, y| x.0.total_cmp(&y.0))
        };
        let round = |p: Vec3| {
            let out = match outs.get(&key(p)) {
                Some(out) => *out,
                None => match off_wall(p) {
                    Some((_, out)) => out,
                    None => return p,
                },
            };
            (1..=ROUND_SHARES)
                .rev()
                .map(|share| share as f32 / ROUND_SHARES as f32)
                .map(|share| [p[0] + out[0] * share, p[1] + out[1] * share, p[2]])
                .find(|q| self.locate(*q, 0.5, 0.5).is_some())
                .unwrap_or(p)
        };
        let rounds: Vec<Vec<(Vec3, Vec3)>> = self
            .faces
            .iter()
            .map(|face| {
                face.portals
                    .iter()
                    .map(|portal| (round(portal.a), round(portal.b)))
                    .collect()
            })
            .collect();
        for (face, rounds) in self.faces.iter_mut().zip(rounds) {
            for (portal, (a, b)) in face.portals.iter_mut().zip(rounds) {
                (portal.a_round, portal.b_round) = (a, b);
            }
        }
    }

    /// Joins boundary edges that lie along each other (seams between
    /// rooms), over the stretch they share: which of them joined.
    fn stitch(&mut self, boundary: &[Edge]) -> Vec<bool> {
        let mut cells: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        for (at, (_, a, b)) in boundary.iter().enumerate() {
            let (low, high) = (
                cell(a[0].min(b[0]), a[1].min(b[1])),
                cell(a[0].max(b[0]), a[1].max(b[1])),
            );
            for x in low.0..=high.0 {
                for y in low.1..=high.1 {
                    cells.entry((x, y)).or_default().push(at);
                }
            }
        }
        let mut pairs = BTreeMap::new();
        for near in cells.values() {
            for (i, &one) in near.iter().enumerate() {
                for &other in &near[i + 1..] {
                    pairs.insert((one.min(other), one.max(other)), ());
                }
            }
        }
        let mut stitched = vec![false; boundary.len()];
        for (one, other) in pairs.into_keys() {
            let (face, a, b) = boundary[one];
            let (theirs, c, d) = boundary[other];
            if face == theirs {
                continue;
            }
            if let Some((from, to)) = overlap(a, b, c, d) {
                self.link(face, theirs, from, to);
                stitched[one] = true;
                stitched[other] = true;
            }
        }
        stitched
    }

    fn index(&mut self) {
        for (at, face) in self.faces.iter().enumerate() {
            let xs = face.corners.map(|p| p[0]);
            let ys = face.corners.map(|p| p[1]);
            let low = cell(xs[0].min(xs[1]).min(xs[2]), ys[0].min(ys[1]).min(ys[2]));
            let high = cell(xs[0].max(xs[1]).max(xs[2]), ys[0].max(ys[1]).max(ys[2]));
            for x in low.0..=high.0 {
                for y in low.1..=high.1 {
                    self.grid.entry((x, y)).or_default().push(at as u32);
                }
            }
        }
    }

    /// Tags each face a body standing on it would be inside an entity of
    /// `level` in its state 0 (a door closed, a placeable in place): the
    /// face's centre and the points a third of the way from it to each
    /// corner, a body `radius` wide either side, from a step over the face
    /// to a head's height; or whose points are mostly (three of the four)
    /// over an entity's footprint (its non-walkable faces flat on the
    /// floor, which walk-clip walls ring: a placeable's), where KOTOR's
    /// walker may not stand. Not a face a footprint merely reaches into: a
    /// street's floor face beside a junked swoop had shut the street. How
    /// many faces were tagged.
    pub fn mark(&mut self, level: &crate::Level) -> usize {
        let extents = crate::Extents {
            mins: [-self.radius, -self.radius, 0.45],
            maxs: [self.radius, self.radius, 1.6],
        };
        let column = crate::Extents {
            mins: [-0.05, -0.05, 0.5],
            maxs: [0.05, 0.05, 1.6],
        };
        let entities: Vec<u32> = level.entities().collect();
        let mut tagged = 0;
        for face in &mut self.faces {
            let points = [
                face.centre,
                lerp(face.centre, face.corners[0], 1.0 / 3.0),
                lerp(face.centre, face.corners[1], 1.0 / 3.0),
                lerp(face.centre, face.corners[2], 1.0 / 3.0),
            ];
            face.entity = entities.iter().copied().find(|&entity| {
                points.iter().any(|&at| {
                    level.entity_overlaps(entity, 0, at, &extents, crate::Contents::SOLID)
                }) || points
                    .iter()
                    .filter(|&&at| {
                        level.entity_overlaps(entity, 0, at, &FOOTPRINT, crate::Contents::NONWALK)
                    })
                    .count()
                    >= 3
            });
            // Most of it (three of the four points), below a body's height
            // and above a step, inside the level's solid: furniture (a
            // non-walkable walkmesh face's, or drawn geometry no walkmesh
            // face covers, which KOTOR's walker passes through and a body
            // here does not). Not a face a wall's edge merely grazes at its
            // middle (the Sith academy's hall: one such shut a wing in).
            let under = points
                .iter()
                .filter(|&&at| level.level_overlaps(at, &column, crate::Contents::SOLID))
                .count();
            face.covered = face.entity.is_none() && under >= 3;
            tagged += usize::from(face.entity.is_some() || face.covered);
        }
        tagged
    }

    /// Numbers the islands; how many.
    fn label(&mut self) -> usize {
        let mut island = vec![u32::MAX; self.faces.len()];
        let mut count = 0;
        for start in 0..self.faces.len() {
            if island[start] != u32::MAX {
                continue;
            }
            let mut stack = vec![start];
            island[start] = count;
            while let Some(face) = stack.pop() {
                for portal in &self.faces[face].portals {
                    let to = portal.to as usize;
                    if island[to] == u32::MAX {
                        island[to] = count;
                        stack.push(to);
                    }
                }
            }
            count += 1;
        }
        for (face, island) in self.faces.iter_mut().zip(island) {
            face.island = island;
        }
        count as usize
    }

    /// The face under `point` (metres): the one whose extent holds it in x
    /// and y and whose surface there is nearest in height, within `below`
    /// under it and `above` over it.
    pub fn locate(&self, point: Vec3, below: f32, above: f32) -> Option<u32> {
        let near = self.grid.get(&cell(point[0], point[1]))?;
        near.iter()
            .filter_map(|&face| {
                let z = self.faces[face as usize].height_at(point)?;
                let rise = z - point[2];
                (rise <= above && -rise <= below).then_some((rise.abs(), face))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
            .map(|(_, face)| face)
    }

    /// The middle of the face nearest `point` (metres, seen from above)
    /// within `reach` of it and from `below` under it to `above` over it,
    /// not under furniture: where one standing off the mesh (on a ledge a
    /// blow put it on) steps back to.
    pub fn nearest(&self, point: Vec3, below: f32, above: f32, reach: f32) -> Option<Vec3> {
        self.faces
            .iter()
            .filter(|face| !face.covered)
            .filter(|face| {
                let rise = face.centre[2] - point[2];
                rise <= above && -rise <= below
            })
            .map(|face| (flat_length(face.centre, point), face.centre))
            .filter(|(d, _)| *d <= reach)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, centre)| centre)
    }

    /// The mesh's height under `point` (`locate`'s face there).
    pub fn height(&self, point: Vec3, below: f32, above: f32) -> Option<f32> {
        let face = self.locate(point, below, above)?;
        self.faces[face as usize].height_at(point)
    }

    /// A route from `from` to `to` (metres, each over a face: `locate`)
    /// for a body of the mesh's radius, or pulled against the bare edges
    /// (`tight`), over faces with no entity or one `passable` lets by: the
    /// corners to walk through, the goal last. `None` when they are on
    /// different islands, off the mesh, or no way leads there.
    pub fn route(
        &self,
        from: Vec3,
        to: Vec3,
        tight: bool,
        passable: &dyn Fn(u32) -> bool,
    ) -> Option<Vec<Vec3>> {
        self.route_round(from, to, tight, passable, &|_| false)
    }

    /// `route`, keeping off the faces `walled` names (by number) but the
    /// goal's.
    pub fn route_round(
        &self,
        from: Vec3,
        to: Vec3,
        tight: bool,
        passable: &dyn Fn(u32) -> bool,
        walled: &dyn Fn(u32) -> bool,
    ) -> Option<Vec<Vec3>> {
        let start = self.locate(from, 1.0, 0.5)?;
        let goal = self.locate(to, 1.0, 0.5)?;
        if self.faces[start as usize].island != self.faces[goal as usize].island {
            return None;
        }
        let faces = self.search(start, goal, to, passable, walled)?;
        if tight {
            return Some(self.pull(from, to, &faces, 0.0));
        }
        Some(self.pull_along(from, (start, to), &faces))
    }

    /// The corners from `from` (over face `start`) to `to` over the
    /// corridor of `faces` (from `search`) whose every leg keeps to the
    /// mesh, crossing from face to face by portals (`crosses`): a leg over a
    /// hole, or from a platform to the floor under it across the
    /// walkmesh's edge (Malak's run to a captive Jedi on the Star Forge,
    /// stopped by the walk-clip wall there), is no way. The corners are
    /// rounded less, by quarters, down to the bare ends' line; where even
    /// that leaves the mesh, the corners before the leg that leaves are
    /// kept, the way goes on to the middle of the farthest portal along the
    /// corridor, out of a face that leg crossed, that a straight leg
    /// reaches, and is pulled on from there.
    fn pull_along(&self, from: Vec3, (start, to): (u32, Vec3), faces: &[u32]) -> Vec<Vec3> {
        let mut corners = Vec::new();
        let (mut apex, mut over, mut first) = (from, start, 0);
        // At most one split a face of the corridor.
        for _ in 0..faces.len() {
            let rest = &faces[first..];
            let mut tight = None;
            for share in (0..=PULL_SHARES).rev() {
                let pulled = self.pull(apex, to, rest, share as f32 / PULL_SHARES as f32);
                match self.keeps(over, apex, &pulled) {
                    Ok(()) => {
                        corners.extend(pulled);
                        return corners;
                    }
                    Err(left) if share == 0 => tight = Some((pulled, left)),
                    Err(_) => {}
                }
            }
            let (tight, (kept, from_face)) = tight.expect("share 0 tried");
            // The leaving leg's start, and the corridor's faces it crossed.
            let leg_from = if kept == 0 { apex } else { tight[kept - 1] };
            let crossed = self.crossed(from_face, leg_from, tight[kept]);
            let reached: Vec<usize> = crossed
                .iter()
                .filter_map(|face| rest.iter().position(|f| f == face))
                .collect();
            let nearest = reached.iter().copied().min().unwrap_or(0);
            let farthest = reached.iter().copied().max().unwrap_or(0);
            // The farthest portal along the corridor out of a face it
            // crossed whose middle a straight leg from its start reaches.
            let split = (nearest..=farthest).rev().find_map(|at| {
                let at = first + at;
                let next = *faces.get(at + 1)?;
                let portal = self.faces[faces[at] as usize]
                    .portals
                    .iter()
                    .find(|portal| portal.to == next)?;
                let middle = lerp(portal.a_round, portal.b_round, 0.5);
                self.cross(from_face, leg_from, middle).ok()?;
                Some((at, middle))
            });
            let Some((at, middle)) = split else {
                corners.extend(tight);
                return corners;
            };
            corners.extend_from_slice(&tight[..kept]);
            corners.push(middle);
            (apex, over, first) = (middle, faces[at + 1], at + 1);
        }
        corners.extend(self.pull(apex, to, &faces[first..], 0.0));
        corners
    }

    /// Whether the legs from `from` (over face `face`) through `corners`
    /// keep to the mesh (`cross`); else how many corners come before the
    /// first leg that leaves it, and the face that leg starts over.
    fn keeps(&self, mut face: u32, from: Vec3, corners: &[Vec3]) -> Result<(), (usize, u32)> {
        let mut last = from;
        for (kept, &corner) in corners.iter().enumerate() {
            let start = face;
            face = self.cross(face, last, corner).map_err(|_| (kept, start))?;
            last = corner;
        }
        Ok(())
    }

    /// The faces the leg from `p` (over `face`) toward `q` crosses before
    /// it ends or leaves the mesh (`cross`), in order.
    fn crossed(&self, face: u32, p: Vec3, q: Vec3) -> Vec<u32> {
        let mut faces = Vec::new();
        let _ = self.walk(face, p, q, &mut |face| faces.push(face));
        faces
    }

    /// The face the leg from `p` (over `face`) to `q` ends over, seen from
    /// above, walking it from face to face across the edges it leaves each
    /// by: each such edge must be a portal to the next face; `Err` with
    /// the face where it leaves the mesh (an edge with no portal there).
    fn cross(&self, face: u32, p: Vec3, q: Vec3) -> Result<u32, u32> {
        self.walk(face, p, q, &mut |_| {})
    }

    /// `cross`, telling `each` every face it walks over. Through a corner
    /// many faces share, a face once met there is not gone back to.
    fn walk(&self, mut face: u32, p: Vec3, q: Vec3, each: &mut dyn FnMut(u32)) -> Result<u32, u32> {
        let (mut at, mut met) = (-1.0, Vec::new());
        for _ in 0..self.faces.len() {
            each(face);
            let Some((next, t)) = self.exit(face, p, q, &met)? else {
                return Ok(face);
            };
            if t > at + f32::EPSILON {
                (at, met) = (t, Vec::new());
            }
            met.push(face);
            face = next;
        }
        Err(face)
    }

    /// The leg from `p` to `q` through face `face`: `Ok(None)` when it ends
    /// over it, `Ok(Some(next))` when it leaves it by a portal into `next`
    /// with the floor there as high as a body standing on it is found
    /// (`route`'s `locate`: within a metre under the leg, half a metre
    /// over; a straight leg from a platform down a ramp and up again is no
    /// way), `Err(face)` when it leaves it otherwise: by an edge with no
    /// portal there, or into a face that does not reach the crossing (two
    /// rooms' floors a gap apart, the walk-clip walls along each room's
    /// edges standing in it).
    fn exit(&self, face: u32, p: Vec3, q: Vec3, met: &[u32]) -> Result<Option<(u32, f32)>, u32> {
        let here = &self.faces[face as usize];
        // It ends over the face: inside it seen from above, at its height
        // (a face's floor under a platform the leg ends on is not it).
        if here
            .height_at(q)
            .is_some_and(|floor| (-LEG_BELOW..=LEG_ABOVE).contains(&(floor - q[2])))
        {
            return Ok(None);
        }
        // Where along the leg it goes out over the face's edges (the line
        // clipped to the triangle): the nearest of those it moves out over.
        let mut out: Option<f32> = None;
        for i in 0..3 {
            let (a, b) = (here.corners[i], here.corners[(i + 1) % 3]);
            let side = |x: Vec3| cross_z(sub(b, a), sub(x, a));
            let (sp, sq) = (side(p), side(q));
            if sp > sq {
                let t = sp / (sp - sq);
                out = Some(out.map_or(t, |best| best.min(t)));
            }
        }
        let Some(t) = out else {
            return Ok(None);
        };
        let t = t.clamp(0.0, 1.0);
        let at = lerp(p, q, t);
        let beyond = lerp(
            p,
            q,
            (t + ON_PORTAL / flat_length(p, q).max(ON_PORTAL)).min(1.0),
        );
        let next = self.through(face, (at, beyond), met).ok_or(face)?;
        let floor = self.faces[next as usize].height_at(at).ok_or(face)?;
        let rise = floor - at[2];
        if rise > LEG_ABOVE || -rise > LEG_BELOW {
            return Err(face);
        }
        Ok(Some((next, t)))
    }

    /// The face a leg leaving face `face` at `at` goes on over (`beyond`, a
    /// hair further): a neighbour by a portal through `at`, or, at a corner
    /// many faces share, one round that corner reached by such portals; not
    /// one of those it `met` there already.
    fn through(&self, face: u32, (at, beyond): (Vec3, Vec3), met: &[u32]) -> Option<u32> {
        let mut round = vec![face];
        round.extend_from_slice(met);
        let mut next = 0;
        while next < round.len() && round.len() <= FAN + met.len() {
            let here = &self.faces[round[next] as usize];
            next += 1;
            for portal in here.portals.iter() {
                if !on_stretch(portal.a, portal.b, at) || round.contains(&portal.to) {
                    continue;
                }
                if self.faces[portal.to as usize].holds(beyond) {
                    return Some(portal.to);
                }
                round.push(portal.to);
            }
        }
        None
    }

    /// A* from face to face (costs in whole millimetres between centres,
    /// ties to the lower face number) over passable faces (the start's and
    /// the goal's always: one stands in a doorway, or is walked up to in
    /// one, as a guard by an exit), none `walled`: the faces crossed, in
    /// order.
    fn search(
        &self,
        start: u32,
        goal: u32,
        to: Vec3,
        passable: &dyn Fn(u32) -> bool,
        walled: &dyn Fn(u32) -> bool,
    ) -> Option<Vec<u32>> {
        let millimetres = |metres: f32| (metres * 1000.0).round_ties_even() as u64;
        let mut best = vec![u64::MAX; self.faces.len()];
        let mut came = vec![u32::MAX; self.faces.len()];
        let mut open = BinaryHeap::new();
        best[start as usize] = 0;
        open.push(Reverse((
            millimetres(distance(self.faces[start as usize].centre, to)),
            start,
        )));
        while let Some(Reverse((_, face))) = open.pop() {
            if face == goal {
                let mut faces = vec![goal];
                while *faces.last().expect("not empty") != start {
                    faces.push(came[*faces.last().expect("not empty") as usize]);
                }
                faces.reverse();
                return Some(faces);
            }
            let here = &self.faces[face as usize];
            let cost = best[face as usize];
            for portal in &here.portals {
                let next = portal.to as usize;
                let there = &self.faces[next];
                if portal.to != goal
                    && (there.covered
                        || there.entity.is_some_and(|entity| !passable(entity))
                        || walled(portal.to))
                {
                    continue;
                }
                let step = millimetres(distance(here.centre, self.faces[next].centre)).max(1);
                if cost + step < best[next] {
                    best[next] = cost + step;
                    came[next] = face;
                    let guess = millimetres(distance(self.faces[next].centre, to));
                    open.push(Reverse((cost + step + guess, portal.to)));
                }
            }
        }
        None
    }

    /// What shuts in the faces a walker at `from` reaches (`search`'s rule):
    /// each face next to them it may not cross, with its centre and what
    /// stands there (`Some` entity, or `None` for the level's furniture), in
    /// face order. Empty when off the mesh.
    pub fn frontier(&self, from: Vec3, passable: &dyn Fn(u32) -> bool) -> Vec<(Vec3, Option<u32>)> {
        let Some(start) = self.locate(from, 1.0, 0.5) else {
            return Vec::new();
        };
        let blocked = |face: &Face| face.covered || face.entity.is_some_and(|e| !passable(e));
        let mut seen = vec![false; self.faces.len()];
        let mut shut = BTreeMap::new();
        let mut stack = vec![start];
        seen[start as usize] = true;
        while let Some(face) = stack.pop() {
            for portal in &self.faces[face as usize].portals {
                let next = &self.faces[portal.to as usize];
                if blocked(next) {
                    shut.insert(portal.to, (next.centre, next.entity));
                } else if !seen[portal.to as usize] {
                    seen[portal.to as usize] = true;
                    stack.push(portal.to);
                }
            }
        }
        shut.into_values().collect()
    }

    /// The funnel over the portals between `faces`, seen from above,
    /// turning round the portals' ends where a body turns (`round` of the
    /// way from the bare ends, 0, to the rounded, 1): the corners of the
    /// tight line from `from` to `to`.
    fn pull(&self, from: Vec3, to: Vec3, faces: &[u32], round: f32) -> Vec<Vec3> {
        // Each portal as (left, right) walking forward, and its ends as
        // (bare, stepped), left and right.
        let mut gates = Vec::with_capacity(faces.len());
        let mut ends = Vec::with_capacity(faces.len());
        for pair in faces.windows(2) {
            let portal = self.faces[pair[0] as usize]
                .portals
                .iter()
                .find(|portal| portal.to == pair[1])
                .expect("a route crosses portals");
            // The face winds counter-clockwise, so walking out through
            // `a`–`b`, `b` is on the left. Ends on the walls step off them;
            // where that steps them past each other (a passage narrower
            // than a body, or a short edge both of whose ends step the same
            // way off one wall) it is passed between the stepped ends, off
            // the walls as they are.
            let (left, right) = (
                lerp(portal.b, portal.b_round, round),
                lerp(portal.a, portal.a_round, round),
            );
            let across = sub(portal.b, portal.a);
            let spread = sub(left, right);
            let gate = if spread[0] * across[0] + spread[1] * across[1] > 1e-4 {
                (left, right)
            } else {
                let middle = lerp(left, right, 0.5);
                (middle, middle)
            };
            gates.push(gate);
            ends.push(((portal.b, portal.b_round), (portal.a, portal.a_round)));
        }
        gates.push((to, to));
        let mut corners = Vec::new();
        let (mut apex, mut left, mut right) = (from, from, from);
        let (mut left_at, mut right_at) = (0usize, 0usize);
        // A side the funnel leaves open since the apex: a gate's end it
        // dropped (below), which bounds nothing until another gate's end
        // on that side does.
        let (mut left_open, mut right_open) = (false, false);
        let mut at = 0;
        while at < gates.len() {
            let (next_left, next_right) = gates[at];
            // At every apex (the start, and each corner the funnel turns
            // at), a gate next on from it that it stands past already (on
            // the stepped gate's far side as it is walked, and between its
            // stepped ends seen across it) while within a body's width of the
            // bare portal drops its ends at an inside corner of the floor
            // (`inside_corner`), which a way bends round little if at all:
            // stepped off the walls there, such an end can lie past a walker
            // standing that near the portal, and taken as the funnel's side it
            // turned the way back round it. An end at an outside corner (one
            // a way bends round) still bounds its side, and a gate collapsed
            // to its middle is never passed over, even with the middle behind
            // the apex.
            let (drop_left, drop_right) = ends
                .get(at)
                .filter(|((left_bare, _), (right_bare, _))| {
                    left == apex
                        && right == apex
                        && next_left != next_right
                        && past(apex, (next_left, next_right))
                        && off_stretch(apex, (*left_bare, *right_bare)) < self.radius
                })
                .map_or((false, false), |&(left_end, right_end)| {
                    (self.inside_corner(left_end), self.inside_corner(right_end))
                });
            if drop_left && drop_right {
                at += 1;
                continue;
            }
            left_open |= drop_left;
            right_open |= drop_right;
            // The right side narrows (the new right is left of it) unless
            // it would cross the left: then the left is a corner.
            if !drop_right && cross_z(sub(right, apex), sub(next_right, apex)) >= 0.0 {
                if apex == right
                    || left_open
                    || cross_z(sub(left, apex), sub(next_right, apex)) < 0.0
                {
                    right = next_right;
                    right_at = at;
                    right_open = false;
                } else {
                    corners.push(left);
                    (apex, right, right_at) = (left, left, left_at);
                    (left_open, right_open) = (false, false);
                    at = left_at + 1;
                    continue;
                }
            }
            // The left side likewise.
            if !drop_left && cross_z(sub(left, apex), sub(next_left, apex)) <= 0.0 {
                if apex == left
                    || right_open
                    || cross_z(sub(right, apex), sub(next_left, apex)) > 0.0
                {
                    left = next_left;
                    left_at = at;
                    left_open = false;
                } else {
                    corners.push(right);
                    (apex, left, left_at) = (right, right, right_at);
                    (left_open, right_open) = (false, false);
                    at = right_at + 1;
                    continue;
                }
            }
            at += 1;
        }
        if corners.last() != Some(&to) {
            corners.push(to);
        }
        // A gate's end on the apex adds the same corner twice.
        corners.dedup();
        corners
    }

    /// Whether a portal's end (`bare`, and where a body turns round it,
    /// `stepped`) is an inside corner of the floor, one a way bends round
    /// little if at all: an end stepped off the walls, round which the floor
    /// a hair away lies within `INSIDE_OFF` looks of a straight angle (the
    /// looks off it `INSIDE_OFF` in a row at least). Not an end inside the
    /// mesh, nor one not stepped.
    fn inside_corner(&self, (bare, stepped): (Vec3, Vec3)) -> bool {
        if flat_length(stepped, bare) < INSIDE_REACH {
            return false;
        }
        let off: Vec<bool> = (0..INSIDE_LOOKS)
            .map(|look| {
                let (sin, cos) =
                    crate::trig::sin_cos_deg((look as f32 + 0.5) * 360.0 / INSIDE_LOOKS as f32);
                let near = [
                    bare[0] + cos * INSIDE_REACH,
                    bare[1] + sin * INSIDE_REACH,
                    bare[2],
                ];
                self.locate(near, INSIDE_FLOOR, INSIDE_FLOOR).is_none()
            })
            .collect();
        // The longest run of looks off the floor, round the end.
        let (mut run, mut longest) = (0, 0);
        for &out in off.iter().chain(off.iter()) {
            run = if out { run + 1 } else { 0 };
            longest = longest.max(run);
        }
        longest.min(INSIDE_LOOKS) >= INSIDE_OFF
    }

    /// The mesh as bytes for the store (`crate::pack`): read back, the same
    /// mesh.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = crate::pack::Out::default();
        out.0.extend(b"KJKN");
        out.u32(NavMesh::FORMAT);
        out.len(self.faces.len());
        for face in &self.faces {
            for corner in face.corners {
                out.vec3(corner);
            }
            out.vec3(face.centre);
            out.u32(face.island);
            out.u32(face.entity.unwrap_or(u32::MAX));
            out.u32(u32::from(face.covered));
            out.len(face.portals.len());
            for portal in &face.portals {
                out.u32(portal.to);
                for point in [portal.a, portal.b, portal.a_round, portal.b_round] {
                    out.vec3(point);
                }
            }
        }
        out.len(self.grid.len());
        for ((x, y), faces) in &self.grid {
            out.u32(*x as u32);
            out.u32(*y as u32);
            out.len(faces.len());
            for face in faces {
                out.u32(*face);
            }
        }
        for count in [self.shared, self.seams, self.islands, self.steep] {
            out.len(count);
        }
        out.f32(self.radius);
        out.0
    }

    /// Bump when `encode`'s bytes change.
    const FORMAT: u32 = 2;

    /// A mesh `encode` wrote.
    pub fn decode(bytes: &[u8]) -> Result<NavMesh, crate::pack::Bad> {
        let mut input = crate::pack::In(bytes);
        input.magic(b"KJKN", NavMesh::FORMAT)?;
        let mut faces = Vec::with_capacity(input.len(52)?);
        for _ in 0..faces.capacity() {
            let corners = [input.vec3()?, input.vec3()?, input.vec3()?];
            let centre = input.vec3()?;
            let island = input.u32()?;
            let entity = Some(input.u32()?).filter(|entity| *entity != u32::MAX);
            let covered = input.u32()? != 0;
            let portals = (0..input.len(52)?)
                .map(|_| {
                    Ok(Portal {
                        to: input.u32()?,
                        a: input.vec3()?,
                        b: input.vec3()?,
                        a_round: input.vec3()?,
                        b_round: input.vec3()?,
                    })
                })
                .collect::<Result<_, crate::pack::Bad>>()?;
            faces.push(Face {
                corners,
                centre,
                portals,
                island,
                entity,
                covered,
            });
        }
        let mut grid = BTreeMap::new();
        for _ in 0..input.len(12)? {
            let cell = (input.i32()?, input.i32()?);
            let list = (0..input.len(4)?)
                .map(|_| input.u32())
                .collect::<Result<_, _>>()?;
            grid.insert(cell, list);
        }
        let mut count = || input.u32().map(|count| count as usize);
        let (shared, seams, islands, steep) = (count()?, count()?, count()?, count()?);
        let radius = input.f32()?;
        input.finish()?;
        let len = faces.len() as u32;
        let inside = |face: &u32| *face < len;
        if !faces
            .iter()
            .all(|face| face.portals.iter().all(|portal| inside(&portal.to)))
            || !grid.values().all(|list: &Vec<u32>| list.iter().all(inside))
        {
            return Err(crate::pack::Bad("a face index past the mesh"));
        }
        Ok(NavMesh {
            faces,
            grid,
            shared,
            seams,
            islands,
            steep,
            radius,
        })
    }

    /// BLAKE3 of the faces and their portals: two machines with the same
    /// digest route the same.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"KJKN");
        let floats = |hasher: &mut blake3::Hasher, values: &[f32]| {
            for value in values {
                hasher.update(&value.to_bits().to_le_bytes());
            }
        };
        for face in &self.faces {
            for corner in &face.corners {
                floats(&mut hasher, corner);
            }
            hasher.update(&face.entity.unwrap_or(u32::MAX).to_le_bytes());
            hasher.update(&(face.portals.len() as u32).to_le_bytes());
            for portal in &face.portals {
                hasher.update(&portal.to.to_le_bytes());
                for point in [portal.a, portal.b, portal.a_round, portal.b_round] {
                    floats(&mut hasher, &point);
                }
            }
        }
        *hasher.finalize().as_bytes()
    }

    /// Its size in faces and portals.
    pub fn portals(&self) -> usize {
        self.faces.iter().map(|face| face.portals.len()).sum()
    }
}

/// How far under and over a leg (metres) the floor may lie where it
/// crosses a portal (`exit`): `locate`'s band, as `route` finds a body's
/// face.
const LEG_BELOW: f32 = 1.0;
const LEG_ABOVE: f32 = 0.5;

/// How many faces round a corner a leg through it is looked for in
/// (`through`).
const FAN: usize = 16;

/// How near (metres, seen from above) a point must lie to a portal's
/// stretch to cross by it.
const ON_PORTAL: f32 = 1e-3;

/// Whether `at` lies on the stretch from `a` to `b`, seen from above
/// (within `ON_PORTAL` of it).
fn on_stretch(a: Vec3, b: Vec3, at: Vec3) -> bool {
    let length = flat_length(a, b);
    if length < f32::EPSILON {
        return flat_length(a, at) <= ON_PORTAL;
    }
    let ab = sub(b, a);
    let ap = sub(at, a);
    let along = (ab[0] * ap[0] + ab[1] * ap[1]) / length;
    let off = cross_z(ab, ap).abs() / length;
    off <= ON_PORTAL && (-ON_PORTAL..=length + ON_PORTAL).contains(&along)
}

impl Face {
    /// Whether `point`'s x and y fall inside the face (edges included, a
    /// hair beyond), seen from above.
    fn holds(&self, point: Vec3) -> bool {
        self.height_at(point).is_some()
    }

    /// The face's height at `point`'s x and y, if they fall inside it
    /// (edges included, a hair beyond).
    fn height_at(&self, point: Vec3) -> Option<f32> {
        let [a, b, c] = self.corners;
        let area = cross_z(sub(b, a), sub(c, a));
        let u = cross_z(sub(c, b), sub(point, b)) / area;
        let v = cross_z(sub(a, c), sub(point, c)) / area;
        let w = 1.0 - u - v;
        let slack = -1e-4;
        (u >= slack && v >= slack && w >= slack).then(|| u * a[2] + v * b[2] + w * c[2])
    }
}

/// The stretch two segments share when they lie along one line (within
/// `SEAM_OFF` of it), overlap by `SEAM_OVERLAP` or more and are at most
/// `SEAM_RISE` apart in height there: its ends, as `a`–`b` runs.
fn overlap(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> Option<(Vec3, Vec3)> {
    let length = flat_length(a, b);
    if length < SEAM_OVERLAP {
        return None;
    }
    let along = |p: Vec3| {
        let ab = sub(b, a);
        let ap = sub(p, a);
        (ab[0] * ap[0] + ab[1] * ap[1]) / length
    };
    let off = |p: Vec3| cross_z(sub(b, a), sub(p, a)).abs() / length;
    if off(c) > SEAM_OFF || off(d) > SEAM_OFF {
        return None;
    }
    let (tc, td) = (along(c), along(d));
    let (low, high) = (tc.min(td).max(0.0), tc.max(td).min(length));
    if high - low < SEAM_OVERLAP {
        return None;
    }
    let (from, to) = (lerp(a, b, low / length), lerp(a, b, high / length));
    // Their heights at the shared stretch's ends.
    let height = |p: Vec3| {
        let t = (along(p) - tc) / (td - tc);
        c[2] + (d[2] - c[2]) * t
    };
    let rise = (height(from) - from[2])
        .abs()
        .max((height(to) - to[2]).abs());
    (rise <= SEAM_RISE).then_some((from, to))
}
