//! Walls along the walkmesh's open edges.
//!
//! KOTOR moves a character only to a point whose walkmesh face is walkable
//! (reone's and xoreos's walkers both refuse a step that ends anywhere
//! else): the outline of the walkable faces stops a walker, whatever is
//! drawn beyond it. Collision built from drawn triangles has no such
//! outline. Where a room's mesh runs on past the walkmesh (terrain, a hull,
//! a backdrop) or the walkmesh goes on in a face that is not walkable, a JA
//! player walks on.
//!
//! So every edge of a walkable face that no walkable face continues gets an
//! upright `WALK_CLIP` slab (it stops walkers' moves only: the camera,
//! shots and sight pass), set back from the edge by the player's half width
//! so that the box's centre, like KOTOR's point, reaches the edge.
//! "Continues" is tested where the player would step: just past the edge,
//! the topmost walkmesh face within a step up and a metre down is walkable
//! (another room's floor at a seam, a lower floor a short drop away).
//! At an edge's end the walls meet as the outline set back would: past a
//! corner the walkable faces turn away from, a wall runs on; short of one
//! they wrap round, it stops, else its end would reach into floor beside
//! the corner that a centre may stand on. How far is the set back times
//! the tangent of half the turn to the outline's next edge (the half width
//! at a right angle, a few centimetres at a slight bend). A flat half
//! width either way had stopped both walls of a slight bend half a box
//! short, leaving a gap a player walked through, or run one on into the
//! floor of a wide turn, where walks stuck. At a vertex where the outline
//! is not one edge on, or goes on along an edge with no wall (a step down
//! to a lower floor), the floor just past the end decides, by the half
//! width.
//!
//! A wall reaches far above its edge ([`HEIGHT`]): drawn terrain often
//! rises over the walkmesh, and a wall only as high as a jump would be
//! buried under a hill a player climbs. A walkable floor overhead stops it,
//! so a lower storey's walls never cut an upper one; only the stretch
//! under that floor (half a metre at a time), not the wall's whole run: a
//! ramp passing over the end of a floor's edge had lowered its whole wall
//! to a metre, which a jump clears. Reaching the sky, the
//! walls also end jumping off the walkmesh (onto furniture in its holes,
//! onto ledges): opening the world up for jumps is a later step.

use std::collections::HashMap;

use collision::clip_file::ClipShape;
use collision::player::HALF_WIDTH;
use collision::{CollisionWorld, Contents, Extents, Level, UNIT};

use crate::Surfaces;

/// How far behind the edge a wall's near face stands: the player's half
/// width (JA's 15 units).
pub const SET_BACK: f32 = HALF_WIDTH * UNIT;
/// How high above the edge a wall reaches by default: past any jump, and
/// past any drawn terrain over the walkmesh a player could climb to jump
/// from. A walkable floor above stops it lower ([`CLEARANCE`]).
pub const HEIGHT: f32 = 100.0;
/// How far above the edge a walkable floor stops a wall: one closer is the
/// floor past the edge that continues it ([`ABOVE`]), not a floor
/// overhead. A ramp rising from beside a floor's edge is over its wall
/// from there on: a metre had let the wall rise through the ramp's first
/// stretch, boxing a walker on the ramp in between it and the ramp's own
/// wall (TSL 002ebo).
pub(crate) const CLEARANCE: f32 = ABOVE;
/// How thick a wall is.
pub const THICKNESS: f32 = 0.1;
/// How far below the edge a wall starts.
pub(crate) const FOOT: f32 = 0.1;
/// The longest stretch of an edge tested as one.
pub(crate) const PIECE: f32 = 0.5;
/// How far past an edge the next face is looked for: just past it, and
/// across a crack between two rooms' walkmeshes.
const PROBES: [f32; 2] = [0.02, 0.15];
/// How far above and below the edge a face past it still continues it.
const ABOVE: f32 = 0.5;
const BELOW: f32 = 1.0;
/// Corners closer than this are the same vertex.
pub(crate) const WELD: f32 = 0.001;

/// A vertex welded to [`WELD`].
type Vertex = [i64; 3];
/// The outline's edges at each vertex: each far vertex, welded and as is.
type Outline = HashMap<Vertex, Vec<(Vertex, [f32; 3])>>;

/// What walls the open edges get.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeWalls {
    /// How high above the edge a wall reaches, metres.
    pub height: f32,
}

/// The walls along the open edges of `walkable` (world-space faces with
/// their materials); `walkmesh` holds every walkmesh face, whatever its
/// material.
pub fn walls(
    walkable: &[([[f32; 3]; 3], u32)],
    walkmesh: &Level,
    surfaces: &Surfaces,
    options: &EdgeWalls,
) -> Vec<ClipShape> {
    let key = |p: [f32; 3]| p.map(|v| (v / WELD).round() as i64);
    // A face with two corners on one vertex has no area: it is no floor,
    // and its doubled edge would read as one a neighbour continues (Taris
    // undercity's has one beside a real edge, which had no wall).
    let walkable: Vec<&([[f32; 3]; 3], u32)> = walkable
        .iter()
        .filter(|(corners, _)| {
            let [a, b, c] = corners.map(key);
            a != b && b != c && c != a
        })
        .collect();
    // Each edge's sides that a walkable face lies on (bit 0 left of it,
    // bit 1 right, from its lower vertex key to its higher): an edge is
    // inside the floor only with floor on both sides. Two faces on one
    // side of it overlap (Dantooine's grove has a sliver over a larger
    // face), and the side beyond is as open as with one.
    let mut shared: HashMap<(Vertex, Vertex), u8> = HashMap::new();
    for (corners, _) in walkable.iter().copied() {
        for k in 0..3 {
            let (p, q, r) = (corners[k], corners[(k + 1) % 3], corners[(k + 2) % 3]);
            let (a, b) = (key(p), key(q));
            let (from, to) = if a <= b { (p, q) } else { (q, p) };
            let cross = (to[0] - from[0]) * (r[1] - from[1]) - (to[1] - from[1]) * (r[0] - from[0]);
            *shared.entry((a.min(b), a.max(b))).or_insert(0) |= if cross > 0.0 { 1 } else { 2 };
        }
    }
    // The outline's edges at each vertex: the far vertex of every edge
    // with floor on one side only.
    let mut outline = Outline::new();
    for (corners, _) in walkable.iter().copied() {
        for k in 0..3 {
            let (p, q) = (corners[k], corners[(k + 1) % 3]);
            let (a, b) = (key(p), key(q));
            if shared.get(&(a.min(b), a.max(b))).copied().unwrap_or(0) == 3 {
                continue;
            }
            for (at, far, far_point) in [(a, b, q), (b, a, p)] {
                let edges = outline.entry(at).or_insert_with(Vec::new);
                if !edges.iter().any(|(k, _)| *k == far) {
                    edges.push((far, far_point));
                }
            }
        }
    }
    // Where the outline goes on from `vertex` past the edge from `came`:
    // the direction of its one other edge there, if it has exactly one.
    let next_edge = |vertex: [f32; 3], came: [f32; 3]| -> Option<[f32; 2]> {
        let (at, from) = (key(vertex), key(came));
        let others: Vec<[f32; 3]> = outline
            .get(&at)?
            .iter()
            .filter(|(far, _)| *far != from)
            .map(|(_, point)| *point)
            .collect();
        let [far] = others.as_slice() else {
            return None;
        };
        let d = [far[0] - vertex[0], far[1] - vertex[1]];
        let length = (d[0] * d[0] + d[1] * d[1]).sqrt();
        (length >= 0.01).then(|| [d[0] / length, d[1] / length])
    };
    let continues = |x: f32, y: f32, z: f32| {
        let hit = walkmesh.trace(
            [x, y, z + ABOVE],
            [x, y, z - BELOW],
            &Extents::POINT,
            Contents::SOLID,
        );
        hit.fraction < 1.0 && surfaces.walkable(u32::from(hit.surface.material))
    };
    let floor_above =
        |p: [f32; 2], low: f32, high: f32| floor_above(walkmesh, surfaces, p, low, high);
    let mut out = Vec::new();
    for (corners, _) in walkable {
        let centre: [f32; 2] =
            std::array::from_fn(|i| (corners[0][i] + corners[1][i] + corners[2][i]) / 3.0);
        for k in 0..3 {
            let (a, b) = (corners[k], corners[(k + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            if shared.get(&(ka.min(kb), ka.max(kb))).copied().unwrap_or(0) == 3 {
                continue;
            }
            let along = [b[0] - a[0], b[1] - a[1]];
            let length = (along[0] * along[0] + along[1] * along[1]).sqrt();
            if length < 0.01 {
                continue;
            }
            let dir = [along[0] / length, along[1] / length];
            let mut out_n = [dir[1], -dir[0]];
            if out_n[0] * (centre[0] - a[0]) + out_n[1] * (centre[1] - a[1]) > 0.0 {
                out_n = [-out_n[0], -out_n[1]];
            }
            let at = |t: f32| -> [f32; 3] { std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t) };
            let open_past = |p: [f32; 3], along: f32, outward: &[f32]| {
                outward.iter().all(|&probe| {
                    !continues(
                        p[0] + dir[0] * along + out_n[0] * probe,
                        p[1] + dir[1] * along + out_n[1] * probe,
                        p[2],
                    )
                })
            };
            let pieces = (length / PIECE).ceil().max(1.0) as usize;
            let open: Vec<bool> = (0..pieces)
                .map(|piece| open_past(at((piece as f32 + 0.5) / pieces as f32), 0.0, &PROBES))
                .collect();
            let mut piece = 0;
            while piece < pieces {
                if !open[piece] {
                    piece += 1;
                    continue;
                }
                let first = piece;
                while piece < pieces && open[piece] {
                    piece += 1;
                }
                let (t0, t1) = (first as f32 / pieces as f32, piece as f32 / pieces as f32);
                let (s, e) = (at(t0), at(t1));
                // At a corner the wall meets the next edge's where the
                // outline set back meets: past the corner where the floor
                // turns away, short of it where the floor wraps round, by
                // the set back times the tangent of half the turn (a
                // slight bend moves the end a little, a right angle by
                // the set back), and half a thickness on so they touch.
                // Where the outline is not one edge on (a vertex several
                // meet at), or that edge has no wall, the floor just past
                // the end decides.
                let end = |at_end: bool, p: [f32; 3], came: [f32; 3], into: [f32; 2]| {
                    if !at_end {
                        return 0.0;
                    }
                    let by_floor_past = || {
                        let past = SET_BACK * (into[0] * dir[0] + into[1] * dir[1]);
                        match open_past(p, past, &PROBES) {
                            true => SET_BACK,
                            false => -SET_BACK,
                        }
                    };
                    let Some(next) = next_edge(p, came) else {
                        return by_floor_past();
                    };
                    // The next edge walled by the corner: no floor goes
                    // on beyond it there (a step down to a lower floor has
                    // no wall to meet).
                    let turn = into[0] * out_n[1] - into[1] * out_n[0];
                    let out_next = if turn > 0.0 {
                        [-next[1], next[0]]
                    } else {
                        [next[1], -next[0]]
                    };
                    let walled = PROBES.iter().all(|&probe| {
                        !continues(
                            p[0] + next[0] * PIECE * 0.5 + out_next[0] * probe,
                            p[1] + next[1] * PIECE * 0.5 + out_next[1] * probe,
                            p[2],
                        )
                    });
                    if !walled {
                        return by_floor_past();
                    }
                    let half_turn = half_turn(into, next);
                    let back = SET_BACK + THICKNESS * 0.5;
                    let wraps = next[0] * out_n[0] + next[1] * out_n[1] > 0.0;
                    let shift = if wraps { -back } else { back } * half_turn;
                    shift + THICKNESS * 0.5
                };
                let before = end(first == 0, a, b, [-dir[0], -dir[1]]);
                let after = end(piece == pieces, b, a, dir);
                if (t1 - t0) * length + before + after <= 0.0 {
                    continue;
                }
                let back = SET_BACK + THICKNESS * 0.5;
                let from = [
                    s[0] - dir[0] * before + out_n[0] * back,
                    s[1] - dir[1] * before + out_n[1] * back,
                ];
                let to = [
                    e[0] + dir[0] * after + out_n[0] * back,
                    e[1] + dir[1] * after + out_n[1] * back,
                ];
                let base = s[2].max(e[2]);
                let samples = ((t1 - t0) * length / PIECE).ceil().max(1.0) as usize;
                let point = |k: usize| -> [f32; 2] {
                    let t = k as f32 / samples as f32;
                    std::array::from_fn(|i| from[i] + (to[i] - from[i]) * t)
                };
                // A floor overhead is looked for from above the edge where
                // the sample stands, not its highest end: a sloped edge's
                // low end has floors nearer over it.
                let tops: Vec<f32> = (0..=samples)
                    .map(|k| {
                        let edge = s[2] + (e[2] - s[2]) * k as f32 / samples as f32;
                        floor_above(point(k), edge + CLEARANCE, base + options.height)
                    })
                    .collect();
                // Each stretch as high as the floor over its ends lets it:
                // a floor overhead (a ramp passing over) lowers the wall
                // under it alone, not the whole run, which a jump would
                // clear beside the ramp.
                let mut k = 0;
                while k < samples {
                    let top = tops[k].min(tops[k + 1]);
                    let first = k;
                    while k + 1 < samples && tops[k + 1].min(tops[k + 2]) == top {
                        k += 1;
                    }
                    k += 1;
                    out.push(ClipShape::Wall {
                        from: point(first),
                        to: point(k),
                        z: [s[2].min(e[2]) - FOOT, top],
                        thickness: THICKNESS,
                    });
                }
            }
        }
    }
    out
}

/// The tangent of half the turn from `into` to `next` (unit directions) at
/// an outline's corner, at most 1 (a right angle's). A turn straight back
/// (a sliver face's corners in a line turn the outline back on itself)
/// is past a right angle too (K1's Korriban academy once had a wall end
/// 9,860 km off from one, across its corridors; one in TSL). As
/// `sqrt((1 - cos) / (1 + cos))`, not `tan(acos(cos) / 2)`: the platform's
/// `acos` and `tan` round their last bits their own way, and these walls
/// are in the level's digest, so a save made on one machine was refused on
/// another.
pub(crate) fn half_turn(into: [f32; 2], next: [f32; 2]) -> f32 {
    let cos = (into[0] * next[0] + into[1] * next[1]).clamp(-1.0, 1.0);
    // Straight back: 2 / 0, infinite, past the right angle.
    ((1.0 - cos) / (1.0 + cos)).sqrt().min(1.0)
}

/// The lowest walkable face of `walkmesh` over `p` from `low` up to `high`,
/// less [`FOOT`] (a floor above, which a wall from below must not cut
/// through), else `high`.
pub(crate) fn floor_above(
    walkmesh: &Level,
    surfaces: &Surfaces,
    p: [f32; 2],
    low: f32,
    high: f32,
) -> f32 {
    let mut z = low;
    while z < high {
        let hit = walkmesh.trace(
            [p[0], p[1], z],
            [p[0], p[1], high],
            &Extents::POINT,
            Contents::SOLID,
        );
        if hit.fraction >= 1.0 {
            break;
        }
        if surfaces.walkable(u32::from(hit.surface.material)) {
            return hit.end[2] - FOOT;
        }
        z = hit.end[2] + 0.02;
    }
    high
}

/// A wall's middle as two upright triangles (to draw it).
pub fn middle(wall: &ClipShape) -> [[[f32; 3]; 3]; 2] {
    let (from, to, z) = match *wall {
        ClipShape::Wall { from, to, z, .. } => (from, to, z),
        ClipShape::Box { min, max } => ([min[0], min[1]], [max[0], max[1]], [min[2], max[2]]),
    };
    let corner = |p: [f32; 2], h: f32| [p[0], p[1], h];
    [
        [corner(from, z[0]), corner(to, z[0]), corner(to, z[1])],
        [corner(from, z[0]), corner(to, z[1]), corner(from, z[1])],
    ]
}
