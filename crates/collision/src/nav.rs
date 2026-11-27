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
//!
//! A level editor project may add links (`Link`): a jump or a drop between
//! two floors the mesh does not join, placed as a pair of points. A link
//! joins the islands of the faces under its ends, and a walker takes one
//! only where no way leads on foot (`sim::walk`), so every route of a mesh
//! is the same with links or without. A mesh with no links writes and
//! digests as it did before links were made.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

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

/// A face narrower than this (metres, seen from above) is a sliver
/// (`Face::sliver`): the width of the column `mark` looks for furniture in.
const SLIVER: f32 = 0.1;

/// A face whose longest edge is longer than this (metres) is looked over
/// for furniture across its area (`Face::spread`), points `SPREAD` apart.
const WIDE: f32 = 2.0;
const SPREAD: f32 = 0.5;
/// The most parts `Face::spread` splits a face's edge into, so a face
/// longer than 64 m is looked over with points further apart. The swoop
/// races' floors have faces up to 4.6 km long. Points half a metre apart
/// made millions of looks for each of them, and the race's module never
/// finished loading.
const SPREAD_PARTS: f32 = 128.0;

/// Where a point is looked for over a footprint (`mark`): a thin column
/// from under the face to a step over it.
const FOOTPRINT: crate::Extents = crate::Extents {
    mins: [-0.05, -0.05, -0.1],
    maxs: [0.05, 0.05, STEP_RISE],
};

/// Where a point is looked for under a placeable's drawn shape (`mark`):
/// a thin column from a hand over the face to a body's height. Low enough
/// for a pot or a heap lower than a step, which the sim does not let a
/// walker step onto; high enough to miss a footprint flat on the floor.
const UNDER_SHAPE: crate::Extents = crate::Extents {
    mins: [-0.05, -0.05, 0.1],
    maxs: [0.05, 0.05, 1.6],
};

/// Shares of a corner's step tried, the whole first (`step_off`).
const ROUND_SHARES: u32 = 8;
/// Shares of the rounding a route is pulled with, the whole first, when
/// rounder corners leave the mesh (`route`).
const PULL_SHARES: u32 = 4;

/// How a link is crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// A jump, either way: up onto a ledge, or across a gap.
    Jump,
    /// A drop, from `from` down to `to` only.
    Drop,
}

/// A way between two floors the mesh does not join (`NavMesh::add_links`):
/// where a walker leaves the floor and where it lands (metres, the feet),
/// and the faces under them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Link {
    pub kind: LinkKind,
    pub from: Vec3,
    pub to: Vec3,
    pub from_face: u32,
    pub to_face: u32,
}

impl Link {
    /// Its ends as a walker crosses it from the island `island`: where it
    /// leaves and where it lands. `None` when it is not crossed from there
    /// (a drop's lower end).
    pub fn crossed_from(&self, mesh: &NavMesh, island: u32) -> Option<(Vec3, Vec3)> {
        let island_of = |face: u32| mesh.faces.get(face as usize).map(|face| face.island);
        if island_of(self.from_face) == Some(island) {
            Some((self.from, self.to))
        } else if self.kind == LinkKind::Jump && island_of(self.to_face) == Some(island) {
            Some((self.to, self.from))
        } else {
            None
        }
    }

    /// The islands of the faces under its two ends, its start's first.
    /// The link keeps those faces from when it was made (`add_links`), so
    /// no face is looked for. `None` when a face is not of `mesh`.
    fn islands(&self, mesh: &NavMesh) -> Option<(u32, u32)> {
        let island_of = |face: u32| mesh.faces.get(face as usize).map(|face| face.island);
        Some((island_of(self.from_face)?, island_of(self.to_face)?))
    }
}

/// How far (metres) under and over a link's end its face is looked for.
const LINK_BELOW: f32 = 0.5;
const LINK_ABOVE: f32 = 0.5;

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
    /// The ways between floors a project adds (`add_links`).
    pub links: Vec<Link>,
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
        NavMesh::build_stepped(triangles, radius, SEAM_RISE)
    }

    /// As `build`, with seams joined up to `seam_rise` metres apart in
    /// height in place of `SEAM_RISE`. A Jedi Academy level's stairs have
    /// no slope between their steps, as KOTOR's walkmeshes have: two steps
    /// meet only as edges along one line, up to a step's height apart
    /// (`player::STEP_HEIGHT`).
    pub fn build_stepped(triangles: &[[Vec3; 3]], radius: f32, seam_rise: f32) -> NavMesh {
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
            links: Vec::new(),
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
        let stitched = mesh.stitch(&boundary, seam_rise);
        mesh.seams = stitched.iter().filter(|seamed| **seamed).count();
        let walls: Vec<Edge> = boundary
            .iter()
            .zip(&stitched)
            .filter(|(_, seamed)| !**seamed)
            .map(|(edge, _)| *edge)
            .collect();
        mesh.index();
        mesh.step_off(&walls, seam_rise);
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
    fn step_off(&mut self, walls: &[Edge], seam_rise: f32) {
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
                    (gap < clear && gap > 1e-4 && away[2].abs() <= seam_rise)
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
    fn stitch(&mut self, boundary: &[Edge], seam_rise: f32) -> Vec<bool> {
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
            if let Some((from, to)) = overlap(a, b, c, d, seam_rise) {
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
    /// many faces were tagged. A placeable's drawn shape
    /// (`Contents::SHAPE`) counts for neither. A face is its placeable's
    /// when its points are mostly (three of the four) under the shape's
    /// non-walkable parts, from a hand over the floor to a body's height
    /// ([`UNDER_SHAPE`]): a swoop bike's shape on
    /// Tatooine's Anchorhead stands out past its footprint, and the K1
    /// chain's walk to the exit stalled against it. Counted as a body's
    /// width round each point, the chairs round Javyyar's cantina tables
    /// on Taris shut the cantina's floor between them.
    pub fn mark(&mut self, level: &crate::Level) -> usize {
        const SHAPE: crate::Contents = crate::Contents::SHAPE;
        let extents = crate::Extents {
            mins: [-self.radius, -self.radius, 0.45],
            maxs: [self.radius, self.radius, 1.6],
        };
        let column = crate::Extents {
            mins: [-SLIVER / 2.0, -SLIVER / 2.0, 0.5],
            maxs: [SLIVER / 2.0, SLIVER / 2.0, 1.6],
        };
        // The farthest any box that an entity is asked about reaches from a
        // point: sideways, down and up. The search for entities near a face
        // is grown by these, so a change to a box changes the search too.
        let boxes = [&extents, &FOOTPRINT, &UNDER_SHAPE];
        let across = (boxes.iter())
            .flat_map(|b| [b.mins[0], b.mins[1], b.maxs[0], b.maxs[1]])
            .fold(0.0_f32, |far, side: f32| far.max(side.abs()));
        let below = (boxes.iter()).fold(0.0_f32, |far, b| far.max(-b.mins[2]));
        let above = (boxes.iter()).fold(0.0_f32, |far, b| far.max(b.maxs[2]));
        for face in &mut self.faces {
            let points = [
                face.centre,
                lerp(face.centre, face.corners[0], 1.0 / 3.0),
                lerp(face.centre, face.corners[1], 1.0 / 3.0),
                lerp(face.centre, face.corners[2], 1.0 / 3.0),
            ];
            // Only the entities with a brush near the face's points can
            // overlap a box at one of them. Asking each entity of the
            // level in turn made a level of 1,000 placeables take minutes
            // to load. The entities are asked lowest first, as before.
            let (mut low, mut high) = ([f32::MAX; 3], [f32::MIN; 3]);
            for point in &points {
                for axis in 0..3 {
                    low[axis] = low[axis].min(point[axis]);
                    high[axis] = high[axis].max(point[axis]);
                }
            }
            let entities = level.entities_near(
                [low[0] - across, low[1] - across, low[2] - below],
                [high[0] + across, high[1] + across, high[2] + above],
            );
            face.entity = entities.iter().copied().find(|&entity| {
                points.iter().any(|&at| {
                    level.entity_overlaps_but(
                        entity,
                        0,
                        at,
                        &extents,
                        crate::Contents::SOLID,
                        SHAPE,
                    )
                }) || points
                    .iter()
                    .filter(|&&at| {
                        level.entity_overlaps_but(
                            entity,
                            0,
                            at,
                            &FOOTPRINT,
                            crate::Contents::NONWALK,
                            SHAPE,
                        )
                    })
                    .count()
                    >= 3
                    || points
                        .iter()
                        .filter(|&&at| {
                            level.entity_overlaps(
                                entity,
                                0,
                                at,
                                &UNDER_SHAPE,
                                crate::Contents::NONWALK,
                            )
                        })
                        .count()
                        >= 3
            });
            // Most of it (three of the four points; over a face wider than
            // `WIDE`, three quarters of its area, `Face::spread`), below a
            // body's height and above a step, inside the level's solid:
            // furniture (a non-walkable walkmesh face's, or drawn geometry
            // no walkmesh face covers, which KOTOR's walker passes through
            // and a body here does not). Not a face a wall's edge merely
            // grazes at its middle (the Sith academy's hall: one such shut a
            // wing in), nor a wide floor face furniture stands on a corner
            // of: Nar Shaddaa's swoop gallery (301NAR) is one 15 m triangle
            // between its east and west, whose four points fell three on
            // the bar's pazaak table, and the way to C9-T9's cage was shut.
            let samples = if face.longest() > WIDE {
                face.spread()
            } else {
                points.to_vec()
            };
            let under = samples
                .iter()
                .filter(|&&at| level.level_overlaps(at, &column, false, crate::Contents::SOLID))
                .count();
            face.covered = face.entity.is_none() && under * 4 >= samples.len() * 3;
        }
        // A sliver, narrower than the column the points are looked over
        // with, has no floor of its own: its points lie on its edges, and
        // the column over them meets whatever wall stands along one, as a
        // corridor's wall beside a 6 mm sliver joining its two rooms'
        // floors (TSL's Peragus mining tunnels, the hallway past the
        // damaged droid). A body only crosses it, between the faces it
        // joins, so where two of them are open it is open too.
        let slivers: Vec<bool> = self.faces.iter().map(Face::sliver).collect();
        let open: Vec<bool> = self
            .faces
            .iter()
            .zip(&slivers)
            .map(|(face, sliver)| !sliver && !face.covered)
            .collect();
        for (face, sliver) in self.faces.iter_mut().zip(&slivers) {
            if *sliver && face.covered {
                let joined = face
                    .portals
                    .iter()
                    .filter(|portal| open[portal.to as usize])
                    .count();
                face.covered = joined < 2;
            }
        }
        self.faces
            .iter()
            .filter(|face| face.entity.is_some() || face.covered)
            .count()
    }

    /// Joins islands where the mesh's faces meet without sharing an edge,
    /// as a Jedi Academy level's floors do. KOTOR's walkmeshes never need
    /// it. Only edges that lead to no other face are looked at, and only
    /// pairs of different islands. Two ways are joined:
    ///
    /// - **Side by side.** An edge of each lies along the other, at most
    ///   `across` metres apart seen from above and `rise` metres apart in
    ///   height, along at least a body's width: a narrow trim or groove
    ///   between two floors left no floor of its own.
    /// - **Resting on a face** (`resting`). An edge lies over another
    ///   face, at most `rise` above or below it, along at least a body's
    ///   width. The level's compiler does not cut a floor where a detail
    ///   brush, such as a stair, stands on it, so the stair's lowest edge
    ///   lies on the floor, and the floor runs on under it.
    ///
    /// Each join is made only where a standing body steps from one to the
    /// other in `level` (`stepped_over`). Gives how many it joined, and
    /// numbers the islands anew.
    pub fn bridge(&mut self, level: &crate::Level, across: f32, rise: f32) -> usize {
        // The mesh's edges: a face's edge that portals to faces on its
        // other side cover less than half of. A level drawn with two faces
        // over one place (a step's tread and a slope over it) shares their
        // edges between faces on the same side, which leaves the edge open.
        let on = |a: Vec3, b: Vec3, p: Vec3| {
            let length = flat_length(a, b).max(1e-6);
            cross_z(sub(b, a), sub(p, a)).abs() / length < 0.01
        };
        let mut edges: Vec<Edge> = Vec::new();
        for (index, face) in self.faces.iter().enumerate() {
            for side in 0..3 {
                let (a, b) = (face.corners[side], face.corners[(side + 1) % 3]);
                let length = flat_length(a, b);
                if length < 2.0 * self.radius {
                    continue;
                }
                let own_side = cross_z(sub(b, a), sub(face.centre, a));
                let covered: f32 = (face.portals.iter())
                    .filter(|portal| on(a, b, portal.a) && on(a, b, portal.b))
                    .filter(|portal| {
                        let theirs =
                            cross_z(sub(b, a), sub(self.faces[portal.to as usize].centre, a));
                        theirs * own_side < 0.0
                    })
                    .map(|portal| flat_length(portal.a, portal.b))
                    .sum();
                if covered < length / 2.0 {
                    edges.push((index as u32, a, b));
                }
            }
        }
        let low = |edge: &Edge| edge.1[0].min(edge.2[0]);
        edges.sort_by(|x, y| low(x).total_cmp(&low(y)).then(x.0.cmp(&y.0)));
        let mut joined = 0;
        for at in 0..edges.len() {
            let (one, a, b) = edges[at];
            let reach = a[0].max(b[0]) + across;
            for &(other, c, d) in &edges[at + 1..] {
                if low(&(other, c, d)) > reach {
                    break;
                }
                if self.faces[one as usize].island == self.faces[other as usize].island {
                    continue;
                }
                let Some((from, to, onto)) = alongside((a, b), (c, d), across, rise, self.radius)
                else {
                    continue;
                };
                // A body crosses at the middle of the stretch the edges
                // share.
                let middle = lerp(from, to, 0.5);
                let there = onto(middle);
                if !stepped_over(level, middle, there, self.radius, rise) {
                    continue;
                }
                self.link(one, other, from, to);
                joined += 1;
            }
        }
        joined += self.resting(level, &edges, rise);
        if joined > 0 {
            self.islands = self.label();
        }
        joined
    }

    /// The second way of `bridge`: each of `edges` that lies over a face of
    /// another island, at most `rise` above or below it at points along it,
    /// joined to that face along the stretch that does. The stretch must be
    /// a body's width long, and a body must step across it from the edge's
    /// face onto the other.
    fn resting(&mut self, level: &crate::Level, edges: &[Edge], rise: f32) -> usize {
        // How high the face is at a point seen from above, where the point
        // lies over it.
        let height_on = |face: &Face, p: Vec3| -> Option<f32> {
            let [a, b, c] = face.corners;
            let area = cross_z(sub(b, a), sub(c, a));
            if area.abs() < 1e-9 {
                return None;
            }
            let u = cross_z(sub(c, b), sub(p, b)) / area;
            let v = cross_z(sub(a, c), sub(p, c)) / area;
            let w = 1.0 - u - v;
            let margin = -1e-4;
            (u >= margin && v >= margin && w >= margin).then(|| u * a[2] + v * b[2] + w * c[2])
        };
        const SAMPLES: usize = 16;
        let mut joins: Vec<(u32, u32, Vec3, Vec3)> = Vec::new();
        for &(own, a, b) in edges {
            let island = self.faces[own as usize].island;
            // The faces near the edge, of other islands.
            let (low, high) = (
                cell(a[0].min(b[0]), a[1].min(b[1])),
                cell(a[0].max(b[0]), a[1].max(b[1])),
            );
            let mut near: Vec<u32> = Vec::new();
            for x in low.0..=high.0 {
                for y in low.1..=high.1 {
                    for &face in self.grid.get(&(x, y)).into_iter().flatten() {
                        if self.faces[face as usize].island != island && !near.contains(&face) {
                            near.push(face);
                        }
                    }
                }
            }
            for other in near {
                let face = &self.faces[other as usize];
                // The share of the edge, from its start, that lies over the
                // face within `rise`.
                let over: Vec<f32> = (0..=SAMPLES)
                    .map(|step| step as f32 / SAMPLES as f32)
                    .filter(|&t| {
                        let p = lerp(a, b, t);
                        height_on(face, p).is_some_and(|z| (z - p[2]).abs() <= rise)
                    })
                    .collect();
                let (Some(&first), Some(&last)) = (over.first(), over.last()) else {
                    continue;
                };
                let (from, to) = (lerp(a, b, first), lerp(a, b, last));
                if flat_length(from, to) < 2.0 * self.radius {
                    continue;
                }
                // A body steps from the edge's face, across the middle of
                // the stretch, onto the other face beyond.
                let middle = lerp(from, to, 0.5);
                let own_centre = self.faces[own as usize].centre;
                let (x, y) = (middle[0] - own_centre[0], middle[1] - own_centre[1]);
                let length = (x * x + y * y).sqrt().max(1e-6);
                let reach = self.radius + 0.1;
                let inside = [
                    middle[0] - x / length * reach,
                    middle[1] - y / length * reach,
                    middle[2],
                ];
                let mut beyond = [
                    middle[0] + x / length * reach,
                    middle[1] + y / length * reach,
                    0.0,
                ];
                let Some(z) = height_on(face, beyond) else {
                    continue;
                };
                beyond[2] = z;
                if !stepped_over(level, inside, beyond, self.radius, rise) {
                    continue;
                }
                joins.push((own, other, from, to));
            }
        }
        let count = joins.len();
        for (own, other, from, to) in joins {
            self.link(own, other, from, to);
        }
        count
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

    /// The point of the mesh nearest `point` (metres, in space) within
    /// `reach` of it seen from above, on a face not under furniture, of
    /// `island` when one is given, whose height there is from `below` under
    /// `point` to `above` over it. The nearer of two alike is the face of
    /// the lower number. Only the faces of the grid's cells within `reach`
    /// are looked at.
    pub fn nearest_point(
        &self,
        point: Vec3,
        (below, above): (f32, f32),
        reach: f32,
        island: Option<u32>,
    ) -> Option<Vec3> {
        let (low, high) = (
            cell(point[0] - reach, point[1] - reach),
            cell(point[0] + reach, point[1] + reach),
        );
        let near: BTreeSet<u32> = (low.0..=high.0)
            .flat_map(|x| (low.1..=high.1).map(move |y| (x, y)))
            .filter_map(|at| self.grid.get(&at))
            .flatten()
            .copied()
            .collect();
        near.into_iter()
            .map(|index| &self.faces[index as usize])
            .filter(|face| !face.covered && island.is_none_or(|island| face.island == island))
            .filter_map(|face| {
                let spot = face.nearest_to(point);
                let rise = spot[2] - point[2];
                (rise <= above && -rise <= below && flat_length(spot, point) <= reach)
                    .then(|| (distance(spot, point), spot))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, spot)| spot)
    }

    /// Adds the links `wanted` (each its kind, where it leaves and where it
    /// lands, metres), each over the faces under its ends. A link with an
    /// end over no face is left out, and so is one whose ends are on one
    /// island already. Returns each one left out, by its place in `wanted`,
    /// with why.
    pub fn add_links(&mut self, wanted: &[(LinkKind, Vec3, Vec3)]) -> Vec<(usize, String)> {
        let mut refused = Vec::new();
        for (at, (kind, from, to)) in wanted.iter().enumerate() {
            let (Some(from_face), Some(to_face)) = (
                self.locate(*from, LINK_BELOW, LINK_ABOVE),
                self.locate(*to, LINK_BELOW, LINK_ABOVE),
            ) else {
                refused.push((at, "an end is over no walkable floor".to_string()));
                continue;
            };
            if self.faces[from_face as usize].island == self.faces[to_face as usize].island {
                refused.push((
                    at,
                    "its ends are joined by floor already, so walkers walk there".to_string(),
                ));
                continue;
            }
            self.links.push(Link {
                kind: *kind,
                from: *from,
                to: *to,
                from_face,
                to_face,
            });
        }
        refused
    }

    /// The first link on the fewest links from the island `from` to the
    /// island `to`, as a walker crosses it (`Link::crossed_from`): where it
    /// leaves and where it lands. The links are tried in their order, so
    /// every machine takes the same. `None` when no links lead there.
    pub fn first_link(&self, from: u32, to: u32) -> Option<(Vec3, Vec3)> {
        if from == to || self.links.is_empty() {
            return None;
        }
        // Each way over a link, in the links' order: the island it leaves,
        // the island it lands on, and where it leaves and lands. A link
        // is crossed from its start, and a jump from its end too
        // (`Link::crossed_from`). The islands are read from the faces the
        // link keeps (`Link::islands`). Looking for the face under each
        // end again, for every link and every island, gave the same
        // islands at many times the work.
        let ways: Vec<(u32, u32, Vec3, Vec3)> = (self.links.iter())
            .flat_map(|link| {
                let ends = link.islands(self);
                let there = ends.map(|(start, end)| (start, end, link.from, link.to));
                let back = ends
                    .filter(|_| link.kind == LinkKind::Jump)
                    .map(|(start, end)| (end, start, link.to, link.from));
                [there, back]
            })
            .flatten()
            .collect();
        // Back from the goal: the islands that reach `to`, by how many
        // links away, then the first link out of `from` one nearer.
        let mut away: BTreeMap<u32, usize> = BTreeMap::new();
        away.insert(to, 0);
        let mut frontier = vec![to];
        let mut steps = 0;
        while !frontier.is_empty() && !away.contains_key(&from) && steps < self.links.len() {
            steps += 1;
            let mut next = Vec::new();
            for &(leaves, lands, ..) in &ways {
                if !away.contains_key(&leaves) && frontier.contains(&lands) {
                    away.insert(leaves, steps);
                    next.push(leaves);
                }
            }
            frontier = next;
        }
        let distance = *away.get(&from)?;
        (ways.iter())
            .find(|(leaves, lands, ..)| *leaves == from && away.get(lands) == Some(&(distance - 1)))
            .map(|&(_, _, leave, land)| (leave, land))
    }

    /// The link a walker crosses from `leave` to `land` (metres, as
    /// `first_link` gives them), if one does.
    pub fn link_between(&self, leave: Vec3, land: Vec3) -> Option<&Link> {
        let same = |a: Vec3, b: Vec3| (0..3).all(|axis| a[axis].to_bits() == b[axis].to_bits());
        self.links.iter().find(|link| {
            same(link.from, leave) && same(link.to, land)
                || link.kind == LinkKind::Jump && same(link.to, leave) && same(link.from, land)
        })
    }

    /// The island of the face under `point` (`locate`), if any.
    pub fn island_at(&self, point: Vec3, below: f32, above: f32) -> Option<u32> {
        self.locate(point, below, above)
            .map(|face| self.faces[face as usize].island)
    }

    /// The middle of the face nearest `point` (metres) in space, not under
    /// furniture: where an arrival far off the mesh stands, on the storey
    /// nearest it rather than a roof over or under it.
    pub fn closest(&self, point: Vec3) -> Option<Vec3> {
        self.faces
            .iter()
            .filter(|face| !face.covered)
            .map(|face| {
                let [dx, dy, dz] = std::array::from_fn(|i| face.centre[i] - point[i]);
                (dx * dx + dy * dy + dz * dz, face.centre)
            })
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

    /// Whether a straight leg from `from` to `to` (metres, `from` over a
    /// face: `route`'s `locate`) keeps to the mesh as a route's legs do
    /// (`cross`): over no hole, no edge without a portal, no floor a body
    /// would step off or up from.
    pub fn keeps_to(&self, from: Vec3, to: Vec3) -> bool {
        self.locate(from, 1.0, 0.5)
            .is_some_and(|face| self.cross(face, from, to).is_ok())
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
            // turned the way back round it. So too both ends of a gate whose
            // corners the apex stands beside neither of (farther from each
            // than it is stepped off its walls): between two blocks' corners
            // the stepped gate lies aslant the bare portal, and a walker on
            // the portal, past the stepped gate, was sent back round one
            // stepped end and then the other (TSL's Peragus hangar bay,
            // 106PER: the start room's way to the lower door zig-zagged, and
            // the walk turned back and forth between two ways). Beside one
            // block's corner (in a doorway), an end at an outside corner
            // still bounds its side, and a gate collapsed to its middle is
            // never passed over, even with the middle behind the apex.
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
                    let beside = |(bare, stepped): (Vec3, Vec3)| {
                        flat_length(apex, bare) <= flat_length(stepped, bare)
                    };
                    let neither = !beside(left_end) && !beside(right_end);
                    (
                        neither || self.inside_corner(left_end),
                        neither || self.inside_corner(right_end),
                    )
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
        // Links follow only where there are any, so a mesh without them
        // writes as before.
        if !self.links.is_empty() {
            out.len(self.links.len());
            for link in &self.links {
                out.u32(u32::from(link.kind == LinkKind::Drop));
                out.vec3(link.from);
                out.vec3(link.to);
                out.u32(link.from_face);
                out.u32(link.to_face);
            }
        }
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
        let mut links = Vec::new();
        if !input.0.is_empty() {
            for _ in 0..input.len(36)? {
                let kind = match input.u32()? {
                    0 => LinkKind::Jump,
                    1 => LinkKind::Drop,
                    _ => return Err(crate::pack::Bad("a link of no known kind")),
                };
                links.push(Link {
                    kind,
                    from: input.vec3()?,
                    to: input.vec3()?,
                    from_face: input.u32()?,
                    to_face: input.u32()?,
                });
            }
        }
        input.finish()?;
        let len = faces.len() as u32;
        let inside = |face: &u32| *face < len;
        if !faces
            .iter()
            .all(|face| face.portals.iter().all(|portal| inside(&portal.to)))
            || !grid.values().all(|list: &Vec<u32>| list.iter().all(inside))
            || !links
                .iter()
                .all(|link| inside(&link.from_face) && inside(&link.to_face))
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
            links,
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
        // Links join the digest only where there are any.
        for link in &self.links {
            hasher.update(b"link");
            hasher.update(&u32::from(link.kind == LinkKind::Drop).to_le_bytes());
            floats(&mut hasher, &link.from);
            floats(&mut hasher, &link.to);
            hasher.update(&link.from_face.to_le_bytes());
            hasher.update(&link.to_face.to_le_bytes());
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

    /// Whether the face is narrower, seen from above, than the column
    /// `mark` looks over its points with (`SLIVER`): twice its area over
    /// its longest edge.
    fn sliver(&self) -> bool {
        let [a, b, c] = self.corners;
        let area = cross_z(sub(b, a), sub(c, a)).abs();
        area < SLIVER * self.longest()
    }

    /// Its longest edge's length, seen from above (metres).
    fn longest(&self) -> f32 {
        let [a, b, c] = self.corners;
        flat_length(a, b)
            .max(flat_length(b, c))
            .max(flat_length(c, a))
    }

    /// Points spread evenly over it, each standing for as much of its area:
    /// the middles of the triangles it splits into, `SPREAD` metres or less
    /// along an edge, and each edge in at most `SPREAD_PARTS` parts.
    fn spread(&self) -> Vec<Vec3> {
        let n = (self.longest() / SPREAD).ceil().clamp(1.0, SPREAD_PARTS) as u32;
        let [a, b, c] = self.corners;
        let at = |(i, j, k): (f32, f32, f32)| -> Vec3 {
            std::array::from_fn(|axis| (a[axis] * i + b[axis] * j + c[axis] * k) / n as f32)
        };
        let mut points = Vec::with_capacity((n * n) as usize);
        for i in 0..n {
            for j in 0..n - i {
                let k = n - 1 - i - j;
                points.push(at((
                    i as f32 + 1.0 / 3.0,
                    j as f32 + 1.0 / 3.0,
                    k as f32 + 1.0 / 3.0,
                )));
                if k > 0 {
                    points.push(at((
                        i as f32 + 2.0 / 3.0,
                        j as f32 + 2.0 / 3.0,
                        k as f32 - 1.0 / 3.0,
                    )));
                }
            }
        }
        points
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

    /// The point of this face nearest `point`, seen from above, at the
    /// face's height there.
    fn nearest_to(&self, point: Vec3) -> Vec3 {
        if let Some(z) = self.height_at(point) {
            return [point[0], point[1], z];
        }
        let [a, b, c] = self.corners;
        [(a, b), (b, c), (c, a)]
            .into_iter()
            .map(|(from, to)| {
                let edge = sub(to, from);
                let length = edge[0] * edge[0] + edge[1] * edge[1];
                let along = sub(point, from);
                let t = ((along[0] * edge[0] + along[1] * edge[1]) / length.max(f32::EPSILON))
                    .clamp(0.0, 1.0);
                lerp(from, to, t)
            })
            .min_by(|p, q| flat_length(*p, point).total_cmp(&flat_length(*q, point)))
            .unwrap_or(self.centre)
    }
}

/// Where two edges lie along each other for `bridge`: nearly the same way
/// (within about 10 degrees), the second at most `across` from the first's
/// line seen from above, sharing a stretch at least a body's width long
/// (`radius` either side), at most `rise` apart in height along it. Gives
/// the stretch on the first edge (as it runs) and a way to find the point
/// of the second edge across from a point of the first.
#[allow(clippy::type_complexity)]
fn alongside(
    (a, b): (Vec3, Vec3),
    (c, d): (Vec3, Vec3),
    across: f32,
    rise: f32,
    radius: f32,
) -> Option<(Vec3, Vec3, impl Fn(Vec3) -> Vec3)> {
    let length = flat_length(a, b);
    let (ab, cd) = (sub(b, a), sub(d, c));
    let other = flat_length(c, d);
    if length <= 0.0 || other <= 0.0 {
        return None;
    }
    // Nearly parallel, either way round.
    if cross_z(ab, cd).abs() > 0.17 * length * other {
        return None;
    }
    let off = |p: Vec3| cross_z(ab, sub(p, a)) / length;
    let (oc, od) = (off(c), off(d));
    if oc.abs() > across || od.abs() > across || oc * od < -1e-6 {
        return None;
    }
    let along = move |p: Vec3| (ab[0] * (p[0] - a[0]) + ab[1] * (p[1] - a[1])) / length;
    let (tc, td) = (along(c), along(d));
    let (start, end) = (tc.min(td).max(0.0), tc.max(td).min(length));
    if end - start < 2.0 * radius {
        return None;
    }
    let (from, to) = (lerp(a, b, start / length), lerp(a, b, end / length));
    // The second edge's point across from a point of the first.
    let onto = move |p: Vec3| {
        let t = ((along(p) - tc) / (td - tc)).clamp(0.0, 1.0);
        lerp(c, d, t)
    };
    let higher = [from, to].map(|p| (onto(p)[2] - p[2]).abs());
    if higher.iter().any(|gap| *gap > rise) {
        return None;
    }
    Some((from, to, onto))
}

/// Whether a standing body steps from the floor at `from` to the floor at
/// `to` (metres) in `level`: a body's box (`radius` either side), from a
/// step (`rise`) over the higher floor to a head's height, moves from a
/// little before `from` to a little past `to` without meeting anything
/// solid. Where the two points are one, the body moves a little either
/// way across the line they lie on.
fn stepped_over(level: &crate::Level, from: Vec3, to: Vec3, radius: f32, rise: f32) -> bool {
    use crate::CollisionWorld;
    let floor = from[2].max(to[2]) + 0.01;
    let body = crate::Extents {
        mins: [-radius + 0.01, -radius + 0.01, rise],
        maxs: [radius - 0.01, radius - 0.01, 1.6],
    };
    let (x, y) = (to[0] - from[0], to[1] - from[1]);
    let length = (x * x + y * y).sqrt();
    if length < 0.01 {
        return false;
    }
    let (x, y) = (x / length, y / length);
    let start = [from[0] - x * 0.1, from[1] - y * 0.1, floor];
    let end = [to[0] + x * 0.1, to[1] + y * 0.1, floor];
    let trace = level.trace(start, end, &body, crate::Contents::PLAYER_SOLID);
    trace.fraction >= 1.0 && !trace.start_solid
}

/// The stretch two segments share when they lie along one line (within
/// `SEAM_OFF` of it), overlap by `SEAM_OVERLAP` or more and are at most
/// `seam_rise` apart in height there: its ends, as `a`–`b` runs.
fn overlap(a: Vec3, b: Vec3, c: Vec3, d: Vec3, seam_rise: f32) -> Option<(Vec3, Vec3)> {
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
    (rise <= seam_rise).then_some((from, to))
}
