//! Convex brushes: a solid as the planes that bound it, and the clip of a
//! swept box or capsule against one.
//!
//! A brush's planes must bound the Minkowski sum of the solid and any box,
//! or a swept box would pass through corners. So besides its faces a brush
//! keeps bevels: the six axial planes of its bounds, and a plane through
//! each edge for each axis (edge × axis) wherever the solid lies wholly
//! behind it. A plane's distance is always the solid's support along its
//! normal, so every plane is a true bound and extra ones cost only time. A
//! triangle is a flat brush: its plane both ways, bounds and edge bevels.
//!
//! [`clip`] is the one clip every world uses (`Level`, [`crate::BoxWorld`],
//! [`crate::BrushWorld`], [`crate::Bodies`]). Each plane, moved out by the
//! swept shape, says whether the path stays clear of it, enters behind it,
//! leaves from behind it, or is not limited by it. A path clear of any
//! plane misses; one that starts behind them all started inside; any other
//! hits where the last entry comes before the first exit, a small gap
//! short of the face, and misses past an edge or a corner where it does
//! not.

use crate::{CONTACT_GAP, Contents, Plane, Surface, Trace};

pub(crate) use crate::vector::{cross, dot, normalize, sub};

/// Two normals closer than this in every component are one plane. A plane
/// dropped for one kept leaves the swept solid loose by the normals'
/// difference times the brush's and the box's size, so the test is tight:
/// a cosine of 0.99999 took a nearly flat floor triangle's face (tilted
/// 1.3 milliradians) for its axial bevel, and its slab stood a millimetre
/// over its own top under a standing box's far side, burying the player who
/// walked onto it (the doorway sill out of Manaan's Sith base's droid room).
const SAME_NORMAL: f32 = 1e-5;

fn same_normal(a: [f32; 3], b: [f32; 3]) -> bool {
    (0..3).all(|i| (a[i] - b[i]).abs() <= SAME_NORMAL)
}

/// The planes of the convex hull of `points`, given its face normals and
/// edges (pairs of point indices), with its bevels. `None` when the points
/// span nothing (fewer than one usable plane).
pub(crate) fn planes(
    points: &[[f32; 3]],
    faces: &[[f32; 3]],
    edges: &[(usize, usize)],
) -> Option<Vec<Plane>> {
    let support = |normal: [f32; 3]| {
        points
            .iter()
            .map(|&p| dot(normal, p))
            .fold(f32::NEG_INFINITY, f32::max)
    };
    let mut out: Vec<Plane> = Vec::new();
    // A normal all but equal to one already kept adds nothing: the kept
    // plane stays at its own support, a true bound. (Taking the nearer of
    // the two supports pulled it into the solid, by up to |p| times the
    // normals' difference: 0.45 m at 100 m from the origin.)
    let mut add = |normal: [f32; 3]| {
        if !out.iter().any(|plane| same_normal(plane.normal, normal)) {
            out.push(Plane {
                normal,
                dist: support(normal),
            });
        }
    };
    // How near a bevel's support must come to the edge to count, grown with
    // the coordinates' size so f32 rounding far from the origin does not
    // drop bevels the same triangle keeps near it. A bevel let in is still
    // at its own support, so a looser test only adds true bounds.
    let scale = points
        .iter()
        .flatten()
        .fold(1.0f32, |largest, &c| largest.max(c.abs()));
    let near = (scale * 8.0 * f32::EPSILON).max(1e-5);
    for &face in faces {
        if let Some(normal) = normalize(face) {
            add(normal);
        }
    }
    for axis in 0..3 {
        for sign in [1.0, -1.0] {
            let mut normal = [0.0; 3];
            normal[axis] = sign;
            add(normal);
        }
    }
    for &(a, b) in edges {
        let edge = sub(points[b], points[a]);
        for axis in 0..3 {
            let mut unit = [0.0; 3];
            unit[axis] = 1.0;
            let Some(normal) = normalize(cross(edge, unit)) else {
                continue;
            };
            // The bevel is a face of the swept solid only where the edge
            // is the solid's support along it.
            for normal in [normal, normal.map(|c| -c)] {
                if support(normal) - dot(normal, points[a]) <= near {
                    add(normal);
                }
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// What sweeps through the planes, set up once per trace.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Swept {
    /// A box: `mins` and `maxs` around the origin.
    Box { mins: [f32; 3], maxs: [f32; 3] },
    /// An upright capsule: spheres of `radius` at ± `offset` (Z) around the
    /// traced point, which is the capsule's centre.
    Capsule { radius: f32, offset: f32 },
    /// An upright cylinder: `radius` across round the traced point, from
    /// `low` to `high` (Z) about it; a box whose footprint is round
    /// (`Shape::Upright`).
    Upright { radius: f32, low: f32, high: f32 },
}

impl Swept {
    /// The cylinder standing in for `extents`, a box centred across: as
    /// tall, and round with its X half-size.
    pub(crate) fn upright(extents: &crate::Extents) -> Swept {
        Swept::Upright {
            radius: (extents.maxs[0] - extents.mins[0]) * 0.5,
            low: extents.mins[2],
            high: extents.maxs[2],
        }
    }

    /// The shape against a solid of `contents`: one not of the level's own
    /// solid (the walls along a walkmesh's outline and round placeables'
    /// footprints, `WALK_CLIP`; the containment audit's `PLAYER_CLIP`) is
    /// met by the box a body round across stands for, as those walls are
    /// set back by its half-width to stand in for KOTOR's point on the
    /// walkmesh.
    pub(crate) fn against(&self, contents: crate::Contents) -> Swept {
        match *self {
            Swept::Upright { radius, low, high }
                if !contents.intersects(crate::Contents::SOLID) =>
            {
                Swept::Box {
                    mins: [-radius, -radius, low],
                    maxs: [radius, radius, high],
                }
            }
            other => other,
        }
    }
}

impl Swept {
    /// How high the shape stands over `plane` with its traced point at
    /// `start` and at `end`. Moving the plane out by the shape's reach
    /// along its normal shrinks the shape to one point, so a height is
    /// that point's distance in front of the moved plane.
    fn heights(&self, plane: &Plane, start: [f32; 3], end: [f32; 3]) -> (f32, f32) {
        let n = plane.normal;
        match *self {
            Swept::Box { mins, maxs } => {
                // The corner furthest against the normal meets the plane
                // first.
                let corner: [f32; 3] =
                    std::array::from_fn(|i| if n[i] < 0.0 { maxs[i] } else { mins[i] });
                let moved = plane.dist - dot(corner, n);
                (dot(start, n) - moved, dot(end, n) - moved)
            }
            Swept::Capsule { radius, offset } => {
                // Of the two sphere centres, the one toward the plane's
                // back (the lower one for a floor) meets it first.
                let z = if n[2] * offset > 0.0 { -offset } else { offset };
                let moved = plane.dist + radius;
                let centre = |p: [f32; 3]| [p[0], p[1], p[2] + z];
                (dot(centre(start), n) - moved, dot(centre(end), n) - moved)
            }
            Swept::Upright { radius, low, high } => {
                // As the box, but across it reaches its radius whichever
                // way the plane faces: a box's corner reaches √2 of its
                // half-size toward a wall at 45°.
                let across = (n[0] * n[0] + n[1] * n[1]).sqrt();
                let z = if n[2] < 0.0 { high } else { low };
                let moved = plane.dist + radius * across - z * n[2];
                (dot(start, n) - moved, dot(end, n) - moved)
            }
        }
    }
}

/// How a sweep meets one brush.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Clip {
    Miss,
    /// Started inside, and got out.
    StartSolid,
    /// Started inside and never got out.
    AllSolid,
    /// Entered at `fraction` (already `CONTACT_GAP` short), through
    /// `plane`.
    Hit {
        fraction: f32,
        plane: Plane,
    },
}

/// What one plane says about the path, from the shape's heights over it at
/// the two ends.
#[derive(Debug, Clone, Copy)]
enum Crossing {
    /// In front all the way: the plane separates the path from the brush.
    Clear,
    /// The plane limits nothing (behind at both ends, or a NaN height).
    Unbounded,
    /// The path starts in front and comes within reach of the plane: it can
    /// be inside from this fraction on.
    Enters(f32),
    /// The path starts behind and ends in front: it can be inside only up
    /// to this fraction.
    Leaves(f32),
}

impl Crossing {
    /// The verdict of a plane the path stands `h0` in front of at its start
    /// and `h1` at its end (negative behind).
    fn of(h0: f32, h1: f32) -> Crossing {
        if h0.is_nan() || h1.is_nan() {
            return Crossing::Unbounded;
        }
        if h0 > 0.0 {
            // Never within the gap of the plane, or not closing on it.
            if h1 >= CONTACT_GAP || h1 >= h0 {
                return Crossing::Clear;
            }
            // Where the path comes to the gap short of the plane; one that
            // starts already within it enters at once.
            let fraction = (h0 - CONTACT_GAP) / (h0 - h1);
            Crossing::Enters(fraction.max(0.0))
        } else if h1 > 0.0 {
            // Counted out to the gap in front, so a start just behind the
            // plane may leave before 0; never after the end.
            let fraction = (h0 + CONTACT_GAP) / (h0 - h1);
            Crossing::Leaves(fraction.min(1.0))
        } else {
            Crossing::Unbounded
        }
    }
}

/// Sweeps `swept` from `start` to `end` against the convex solid behind
/// all of `planes`.
///
/// Each plane bounds the stretch of the path that can be inside: past an
/// entry, or short of an exit. The path is inside where all the bounds
/// overlap, so it hits at the latest entry when that comes strictly before
/// the earliest exit (of equal entries, the first plane's), and misses
/// when the exit comes first (passing an edge or a corner) or any plane is
/// clear of it. A path that starts behind every plane started inside: it
/// gets out if it ends in front of any.
pub(crate) fn clip(planes: &[Plane], start: [f32; 3], end: [f32; 3], swept: &Swept) -> Clip {
    let mut starts_in_front = false;
    let mut ends_in_front = false;
    let mut latest_entry: Option<(f32, &Plane)> = None;
    let mut earliest_exit = 1.0f32;
    for plane in planes {
        let (h0, h1) = swept.heights(plane, start, end);
        // A NaN height is in front of nothing.
        starts_in_front |= h0 > 0.0;
        ends_in_front |= h1 > 0.0;
        match Crossing::of(h0, h1) {
            // The rest of the planes cannot change a miss.
            Crossing::Clear => return Clip::Miss,
            Crossing::Unbounded => {}
            Crossing::Enters(fraction) => {
                if latest_entry.is_none_or(|(latest, _)| fraction > latest) {
                    latest_entry = Some((fraction, plane));
                }
            }
            Crossing::Leaves(fraction) => {
                if fraction < earliest_exit {
                    earliest_exit = fraction;
                }
            }
        }
    }
    if !starts_in_front {
        return if ends_in_front {
            Clip::StartSolid
        } else {
            Clip::AllSolid
        };
    }
    match latest_entry {
        Some((fraction, plane)) if fraction < earliest_exit => Clip::Hit {
            fraction,
            plane: *plane,
        },
        _ => Clip::Miss,
    }
}

/// A list world's trace (`BoxWorld`, `BrushWorld`), its solids tried in
/// order: the nearest hit, of equal ones the first; the first solid the
/// start never gets out of decides it.
pub(crate) struct InOrder {
    trace: Trace,
    decided: bool,
}

impl InOrder {
    pub(crate) fn new(end: [f32; 3]) -> InOrder {
        InOrder {
            trace: Trace::clear(end),
            decided: false,
        }
    }

    /// Takes one solid's clip into account. True once the trace is decided
    /// and the remaining solids need not be tried.
    pub(crate) fn add(
        &mut self,
        clip: Clip,
        start: [f32; 3],
        (contents, surface, entity): (Contents, Surface, Option<u32>),
    ) -> bool {
        let trace = &mut self.trace;
        match clip {
            Clip::Miss => {}
            Clip::StartSolid => trace.start_solid = true,
            Clip::AllSolid => {
                // Nothing was entered, so the plane stays the best hit's.
                trace.start_solid = true;
                trace.all_solid = true;
                trace.fraction = 0.0;
                trace.end = start;
                trace.contents = contents;
                trace.surface = surface;
                trace.entity = entity;
                self.decided = true;
            }
            Clip::Hit { fraction, plane } => {
                if fraction < trace.fraction {
                    trace.fraction = fraction;
                    trace.plane = plane;
                    trace.surface = surface;
                    trace.contents = contents;
                    trace.entity = entity;
                }
            }
        }
        self.decided
    }

    pub(crate) fn finish(self, start: [f32; 3], end: [f32; 3]) -> Trace {
        let mut trace = self.trace;
        if !self.decided && trace.fraction < 1.0 {
            trace.end = std::array::from_fn(|i| start[i] + trace.fraction * (end[i] - start[i]));
        }
        trace
    }
}

/// Whether `point` is inside (or on) the planes.
pub(crate) fn contains(planes: &[Plane], point: [f32; 3]) -> bool {
    planes
        .iter()
        .all(|plane| dot(point, plane.normal) - plane.dist <= 0.0)
}

/// Faces smaller than this (m²) are a bevel's touch, not a face.
const FACE_AREA: f64 = 1e-6;
/// How far outside a plane a corner may be and still be on it (metres).
const ON_PLANE: f64 = 1e-4;

/// The brush's faces: for each plane that bounds it over an area (a bevel
/// touches it only along an edge or at a corner), the polygon there,
/// anticlockwise seen from outside. `min`..`max` are its bounds. A flat
/// brush (a triangle's slab, `TRIANGLE_DEPTH` deep) gives its front only.
pub(crate) fn faces(planes: &[Plane], min: [f32; 3], max: [f32; 3]) -> Vec<Vec<[f32; 3]>> {
    let wide = |v: [f32; 3]| v.map(f64::from);
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let along =
        |p: [f64; 3], v: [f64; 3], t: f64| -> [f64; 3] { std::array::from_fn(|i| p[i] + v[i] * t) };
    let (min, max) = (wide(min), wide(max));
    let centre: [f64; 3] = std::array::from_fn(|i| (min[i] + max[i]) * 0.5);
    let reach = dot(
        std::array::from_fn(|i| max[i] - min[i]),
        std::array::from_fn(|i| max[i] - min[i]),
    )
    .sqrt()
        + 1.0;
    let flat = planes.len() >= 2
        && dot(wide(planes[0].normal), wide(planes[1].normal)) < -0.999
        && f64::from(planes[0].dist + planes[1].dist) <= f64::from(crate::TRIANGLE_DEPTH) * 1.01;
    let kept = if flat { 1 } else { planes.len() };
    let mut out = Vec::new();
    for (i, plane) in planes.iter().enumerate().take(kept) {
        let n = wide(plane.normal);
        let d = f64::from(plane.dist);
        // A square on the plane, larger than the brush, cut by the others.
        let on = along(centre, n, d - dot(n, centre));
        let side = if n[0].abs() < 0.6 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let u = cross(side, n);
        let u_length = dot(u, u).sqrt();
        let u = u.map(|c| c / u_length * reach);
        let v = cross(n, u);
        let mut polygon: Vec<[f64; 3]> = [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]
            .into_iter()
            .map(|(a, b)| std::array::from_fn(|k| on[k] + u[k] * a + v[k] * b))
            .collect();
        for (j, other) in planes.iter().enumerate() {
            if j == i || polygon.len() < 3 {
                continue;
            }
            let (m, e) = (wide(other.normal), f64::from(other.dist));
            let outside = |p: [f64; 3]| dot(m, p) - e;
            let mut cut = Vec::with_capacity(polygon.len() + 1);
            for (k, &p) in polygon.iter().enumerate() {
                let q = polygon[(k + 1) % polygon.len()];
                let (a, b) = (outside(p), outside(q));
                if a <= ON_PLANE {
                    cut.push(p);
                }
                if (a > ON_PLANE) != (b > ON_PLANE) {
                    let t = a / (a - b);
                    cut.push(std::array::from_fn(|c| p[c] + (q[c] - p[c]) * t));
                }
            }
            polygon = cut;
        }
        if polygon.len() < 3 {
            continue;
        }
        let first = polygon[0];
        let area: f64 = polygon
            .windows(2)
            .skip(1)
            .map(|w| {
                let a: [f64; 3] = std::array::from_fn(|k| w[0][k] - first[k]);
                let b: [f64; 3] = std::array::from_fn(|k| w[1][k] - first[k]);
                dot(cross(a, b), n) * 0.5
            })
            .sum();
        if area > FACE_AREA {
            out.push(polygon.iter().map(|p| p.map(|c| c as f32)).collect());
        }
    }
    out
}
