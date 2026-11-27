//! A Jedi Academy level cut into rooms by hand, as a KOTOR area is made of
//! rooms.
//!
//! A level starts as one room, the rest ([`RoomCuts::names`]' first). The
//! user places points, and the points make a shape ([`Shape`]). A cut
//! ([`Cut`]) gives everything inside a shape to a room: a new one, or one
//! that exists, so a room with a bend is made of two cuts. Cuts are kept in
//! the order they were made. A place belongs to the room of the last cut
//! that holds it, else to the rest. So a later cut takes from the rooms
//! cut before it.
//!
//! **A shape** is the smallest convex solid round its points. `up` and
//! `down` reach it further: each point is also taken that far above and
//! below itself. Points clicked on a floor make a flat shape with no
//! inside, and a hall under an open sky has no ceiling to click, so a
//! shape is mostly a floor's outline reached up. With both at 0 the shape
//! is exactly the points. `margin` moves every face out, so a wall the
//! points were clicked on is inside.
//!
//! **Triangles are cut at a shape's faces** ([`Cutter::pieces`]). A
//! hangar's floor may be one triangle 50 m long. Given whole to one room,
//! a room's edge would follow the level's triangles and not the shape.
//!
//! Everything here is in metres.

type V = [f64; 3];

fn dot(a: V, b: V) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn sub(a: V, b: V) -> V {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: V, b: V) -> V {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// How far (metres) a point may lie off a plane and still be on it.
const ON_PLANE: f64 = 1e-4;

/// An edge of a solid is shallow when its two faces lie within this many
/// degrees of one plane (`Solid::shallow`).
pub const SHALLOW: f32 = 25.0;

/// The most points a shape takes. Finding a shape's faces tries every
/// three of its points, each raised and lowered too.
pub const MOST_POINTS: usize = 32;

/// What the user places: points, and how far the shape reaches above and
/// below each and out from its faces.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub points: Vec<[f32; 3]>,
    pub up: f32,
    pub down: f32,
    pub margin: f32,
}

/// A convex solid: the planes that bound it, each a normal that faces out
/// and how far the plane lies along it.
#[derive(Debug, Clone, PartialEq)]
pub struct Solid {
    planes: Vec<(V, f64)>,
    min: [f32; 3],
    max: [f32; 3],
    /// Its corners and its edges (pairs of corners), before the margin,
    /// to draw it by.
    pub corners: Vec<[f32; 3]>,
    pub edges: Vec<(usize, usize)>,
    /// Whether each edge is a shallow one: its two faces lie within
    /// [`SHALLOW`] degrees of one plane. Points placed at different heights
    /// give a shape whose top and bottom are not flat but made of
    /// triangles, and the edges between those are shallow. They are edges
    /// of the solid all the same.
    pub shallow: Vec<bool>,
    /// The point of the shape each corner came from: the point itself, or
    /// the point raised or lowered.
    pub corner_points: Vec<usize>,
}

impl Shape {
    /// The points the solid is drawn round: each point, raised by `up` and
    /// lowered by `down`. With each, the point of the shape it came from.
    fn spread(&self) -> (Vec<V>, Vec<usize>) {
        let mut out: Vec<V> = Vec::new();
        let mut from = Vec::new();
        for (index, point) in self.points.iter().enumerate().take(MOST_POINTS) {
            let [x, y, z] = point.map(f64::from);
            for height in [
                z,
                z + f64::from(self.up.max(0.0)),
                z - f64::from(self.down.max(0.0)),
            ] {
                let at = [x, y, height];
                let known = out
                    .iter()
                    .any(|other| (0..3).all(|axis| (other[axis] - at[axis]).abs() <= ON_PLANE));
                if !known {
                    out.push(at);
                    from.push(index);
                }
            }
        }
        (out, from)
    }

    /// The solid the shape makes. `None` when its points have no inside:
    /// fewer than four, or all in one plane.
    pub fn solid(&self) -> Option<Solid> {
        let (points, from) = self.spread();
        let mut planes: Vec<(V, f64)> = Vec::new();
        for i in 0..points.len() {
            for j in i + 1..points.len() {
                for k in j + 1..points.len() {
                    let normal = cross(sub(points[j], points[i]), sub(points[k], points[i]));
                    let length = dot(normal, normal).sqrt();
                    if length < 1e-9 {
                        continue;
                    }
                    let normal = normal.map(|part| part / length);
                    let dist = dot(normal, points[i]);
                    let sides = points.iter().map(|point| dot(normal, *point) - dist);
                    let (mut before, mut behind) = (false, false);
                    for side in sides {
                        before |= side > ON_PLANE;
                        behind |= side < -ON_PLANE;
                    }
                    // A face has every other point behind it, or every
                    // other point before it (it then faces the other way).
                    let face = match (before, behind) {
                        (false, true) => (normal, dist),
                        (true, false) => (normal.map(|part| -part), -dist),
                        // Every point is in this plane: the shape is flat.
                        (false, false) => return None,
                        (true, true) => continue,
                    };
                    let known = planes.iter().any(|(other, at)| {
                        (0..3).all(|axis| (other[axis] - face.0[axis]).abs() <= 1e-6)
                            && (at - face.1).abs() <= ON_PLANE
                    });
                    if !known {
                        planes.push(face);
                    }
                }
            }
        }
        if planes.len() < 4 {
            return None;
        }
        // The corners are the points on three faces or more, and an edge
        // joins two corners on two of the same faces.
        let on = |point: V| -> Vec<usize> {
            (0..planes.len())
                .filter(|&plane| (dot(planes[plane].0, point) - planes[plane].1).abs() <= ON_PLANE)
                .collect()
        };
        let corners: Vec<(V, Vec<usize>, usize)> = (points.iter().zip(&from))
            .map(|(point, from)| (*point, on(*point), *from))
            .filter(|(_, faces, _)| faces.len() >= 3)
            .collect();
        let mut edges = Vec::new();
        let mut shallow = Vec::new();
        let flat = f64::from(SHALLOW).to_radians().cos();
        for a in 0..corners.len() {
            for b in a + 1..corners.len() {
                let shared: Vec<usize> = (corners[a].1.iter())
                    .copied()
                    .filter(|face| corners[b].1.contains(face))
                    .collect();
                if shared.len() >= 2 {
                    edges.push((a, b));
                    shallow.push(match shared[..] {
                        [one, other] => dot(planes[one].0, planes[other].0) > flat,
                        _ => false,
                    });
                }
            }
        }
        let margin = f64::from(self.margin.max(0.0));
        let planes: Vec<(V, f64)> = (planes.into_iter())
            .map(|(normal, dist)| (normal, dist + margin))
            .collect();
        // The box round the solid as the margin leaves it: round its own
        // corners, each where three of its faces meet. The box round the
        // points, widened by the margin, is too small where two faces meet
        // at a sharp angle. Moved out by the margin, they meet further out
        // than the margin. A triangle there lay inside the solid and
        // outside its box, and `Cutter::pieces` left it to the rest of the
        // level while `Cutter::room_at` gave it to the room.
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for (first, a) in planes.iter().enumerate() {
            for (second, b) in planes.iter().enumerate().skip(first + 1) {
                for c in planes.iter().skip(second + 1) {
                    let (bc, ca, ab) = (cross(b.0, c.0), cross(c.0, a.0), cross(a.0, b.0));
                    let turn = dot(a.0, bc);
                    if turn.abs() < 1e-9 {
                        continue;
                    }
                    let corner: V = std::array::from_fn(|axis| {
                        (a.1 * bc[axis] + b.1 * ca[axis] + c.1 * ab[axis]) / turn
                    });
                    // A corner of the solid lies behind every face of it.
                    if planes
                        .iter()
                        .all(|(normal, dist)| dot(*normal, corner) - dist <= ON_PLANE)
                    {
                        for axis in 0..3 {
                            min[axis] = min[axis].min(corner[axis]);
                            max[axis] = max[axis].max(corner[axis]);
                        }
                    }
                }
            }
        }
        // Never smaller than the box round the points with the margin.
        for point in &points {
            for axis in 0..3 {
                min[axis] = min[axis].min(point[axis] - margin);
                max[axis] = max[axis].max(point[axis] + margin);
            }
        }
        Some(Solid {
            planes,
            min: min.map(|part| part as f32),
            max: max.map(|part| part as f32),
            corners: (corners.iter())
                .map(|(corner, ..)| corner.map(|part| part as f32))
                .collect(),
            edges,
            shallow,
            corner_points: corners.iter().map(|(.., from)| *from).collect(),
        })
    }
}

impl Solid {
    /// Whether the point is inside the solid or on its faces.
    pub fn holds(&self, point: [f32; 3]) -> bool {
        let point = point.map(f64::from);
        (self.planes.iter()).all(|(normal, dist)| dot(*normal, point) - dist <= 1e-6)
    }

    /// Whether the box `min`..`max` may reach into the solid: their boxes
    /// meet.
    fn near(&self, min: [f32; 3], max: [f32; 3]) -> bool {
        (0..3).all(|axis| min[axis] <= self.max[axis] && self.min[axis] <= max[axis])
    }
}

/// One cut: everything inside `shape` belongs to room `room`
/// (`RoomCuts::names`).
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    pub room: usize,
    pub shape: Shape,
}

/// A level's rooms: their names, the rest of the level first, and the cuts
/// that made them, in the order they were made.
#[derive(Debug, Clone, PartialEq)]
pub struct RoomCuts {
    pub names: Vec<String>,
    pub cuts: Vec<Cut>,
}

/// The name the rest of a level has until the user gives it another.
pub const REST: &str = "the rest of the level";

impl Default for RoomCuts {
    fn default() -> RoomCuts {
        RoomCuts {
            names: vec![REST.to_string()],
            cuts: Vec::new(),
        }
    }
}

impl RoomCuts {
    /// Takes room `room` out: its cuts go, so what it held falls back to
    /// the rooms cut before it. The rest of the level cannot be taken out.
    pub fn remove(&mut self, room: usize) -> bool {
        if room == 0 || room >= self.names.len() {
            return false;
        }
        self.names.remove(room);
        self.cuts.retain(|cut| cut.room != room);
        for cut in &mut self.cuts {
            if cut.room > room {
                cut.room -= 1;
            }
        }
        true
    }

    /// The cuts made ready to cut with.
    pub fn cutter(&self) -> Cutter {
        Cutter {
            // The last cut decides, so it is asked first.
            solids: (self.cuts.iter().rev())
                .filter_map(|cut| Some((cut.room, cut.shape.solid()?)))
                .collect(),
        }
    }
}

/// A level's cuts as solids, the last made first.
pub struct Cutter {
    solids: Vec<(usize, Solid)>,
}

/// A point of a triangle as shares of its three corners.
type Share = [f64; 3];

/// The whole triangle: its three corners.
const WHOLE: [Share; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

/// A polygon's two parts either side of a plane: behind it (inside) and
/// before it. `side` is each corner's distance before the plane.
fn split(polygon: &[Share], side: &[f64]) -> (Vec<Share>, Vec<Share>) {
    let (mut inside, mut outside) = (Vec::new(), Vec::new());
    for at in 0..polygon.len() {
        let next = (at + 1) % polygon.len();
        let (a, b) = (side[at], side[next]);
        if a <= 0.0 {
            inside.push(polygon[at]);
        }
        if a >= 0.0 {
            outside.push(polygon[at]);
        }
        if (a < 0.0 && b > 0.0) || (a > 0.0 && b < 0.0) {
            let t = a / (a - b);
            let crossing: Share =
                std::array::from_fn(|i| polygon[at][i] + (polygon[next][i] - polygon[at][i]) * t);
            inside.push(crossing);
            outside.push(crossing);
        }
    }
    (inside, outside)
}

impl Cutter {
    /// The room the point `at` is in.
    pub fn room_at(&self, at: [f32; 3]) -> usize {
        (self.solids.iter())
            .find(|(_, solid)| solid.holds(at))
            .map_or(0, |(room, _)| *room)
    }

    /// A triangle as the pieces the cuts make of it: each piece's room and
    /// its three corners, as shares of the triangle's own corners. The
    /// pieces wind as the triangle does. A triangle no cut reaches is one
    /// piece, whole, in the rest of the level.
    pub fn pieces(&self, corners: [[f32; 3]; 3]) -> Vec<(usize, [[f32; 3]; 3])> {
        let whole = |room: usize| vec![(room, WHOLE.map(|share| share.map(|part| part as f32)))];
        let min: [f32; 3] = std::array::from_fn(|axis| {
            corners
                .iter()
                .map(|c| c[axis])
                .fold(f32::INFINITY, f32::min)
        });
        let max: [f32; 3] = std::array::from_fn(|axis| {
            (corners.iter().map(|c| c[axis])).fold(f32::NEG_INFINITY, f32::max)
        });
        if !self.solids.iter().any(|(_, solid)| solid.near(min, max)) {
            return whole(0);
        }
        let corners = corners.map(|corner| corner.map(f64::from));
        let place = |share: &Share| -> V {
            std::array::from_fn(|axis| {
                share[0] * corners[0][axis]
                    + share[1] * corners[1][axis]
                    + share[2] * corners[2][axis]
            })
        };
        // The polygons no cut has taken yet, and the ones a room has.
        let mut free: Vec<Vec<Share>> = vec![WHOLE.to_vec()];
        let mut taken: Vec<(usize, Vec<Share>)> = Vec::new();
        for (room, solid) in &self.solids {
            if !solid.near(min, max) {
                continue;
            }
            let mut left = Vec::new();
            for polygon in free {
                let mut inside = polygon.clone();
                let mut cut_off = Vec::new();
                for (normal, dist) in &solid.planes {
                    let side: Vec<f64> = (inside.iter())
                        .map(|share| dot(*normal, place(share)) - dist)
                        .collect();
                    // A corner a hair off a plane is on it, so no sliver
                    // is cut.
                    let side: Vec<f64> = (side.into_iter())
                        .map(|d| if d.abs() <= 1e-6 { 0.0 } else { d })
                        .collect();
                    if side.iter().all(|d| *d <= 0.0) {
                        continue;
                    }
                    let (behind, before) = split(&inside, &side);
                    if before.len() >= 3 {
                        cut_off.push(before);
                    }
                    inside = behind;
                    if inside.len() < 3 {
                        break;
                    }
                }
                if inside.len() >= 3 {
                    taken.push((*room, inside));
                    left.extend(cut_off);
                } else {
                    // Nothing of it is inside, so it stays in one piece.
                    left.push(polygon);
                }
            }
            free = left;
        }
        if taken.is_empty() {
            return whole(0);
        }
        taken.extend(free.into_iter().map(|polygon| (0, polygon)));
        let mut pieces = Vec::new();
        for (room, polygon) in taken {
            for at in 1..polygon.len() - 1 {
                let triangle = [polygon[0], polygon[at], polygon[at + 1]];
                let [a, b, c] = [
                    place(&triangle[0]),
                    place(&triangle[1]),
                    place(&triangle[2]),
                ];
                let area = cross(sub(b, a), sub(c, a));
                // A sliver with no area draws nothing.
                if dot(area, area) > 1e-16 {
                    pieces.push((room, triangle.map(|share| share.map(|part| part as f32))));
                }
            }
        }
        pieces
    }

    /// The parts of the triangles `triangles` (metres) that lie in room
    /// `room`, each wound as its triangle is, with its triangle's tag
    /// (`tags`, in the triangles' order).
    pub fn room_part<T: Copy>(
        &self,
        triangles: &[[[f32; 3]; 3]],
        tags: &[T],
        room: usize,
    ) -> Vec<([[f32; 3]; 3], T)> {
        let mut kept = Vec::new();
        for (corners, tag) in triangles.iter().zip(tags) {
            for (piece_room, shares) in self.pieces(*corners) {
                if piece_room != room {
                    continue;
                }
                let piece = shares.map(|share| {
                    std::array::from_fn(|axis| {
                        share[0] * corners[0][axis]
                            + share[1] * corners[1][axis]
                            + share[2] * corners[2][axis]
                    })
                });
                kept.push((piece, *tag));
            }
        }
        kept
    }
}
