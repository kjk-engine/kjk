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
//! walls also end jumping off the walkmesh (onto ledges, out of doors).
//!
//! Furniture indoors is the exception ([`furniture_top`]): a table or a
//! stack of crates standing in a hole of the walkmesh has a solid top, and
//! a wall to the sky in front of it stopped every jump onto it. Where the
//! rooms' solid stands at the wall's own line, with a level top more than
//! a step over the edge and under a roof, the wall ends at that top, is
//! open for the height of a standing player and a plain jump ([`SLOT`]),
//! and goes on above. A walker is still stopped (the wall and the
//! furniture are both higher than a step); a jump meets the furniture as
//! it is drawn, and lands on top where it is low enough; a Force jump
//! from the floor still meets the wall higher up, and does not cross the
//! furniture and the room's own wall behind it out of the level. Furniture
//! in a hole of the floor, with floor all round it ([`holes`]), counts in a
//! room that is not indoors too. A top a plain jump reaches is also found
//! across a low strip before it, and counts as closed behind when solid
//! runs on under the way back with no drop (a console along a wall).

use std::collections::{HashMap, HashSet};

use collision::clip_file::ClipShape;
use collision::player::{HALF_WIDTH, JUMP_HEIGHT, SOLE_Z, STAND_HEIGHT, STEP_HEIGHT};
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
/// How much higher than a step over the edge furniture's top must stand:
/// the wall in front of it, [`UNDER_TOP`] lower, is then still more than a
/// step high, and stops a walker.
const OVER_STEP: f32 = 0.05;
/// How far under the furniture's top the wall ends, so a box crossing the
/// top does not catch on the wall's.
const UNDER_TOP: f32 = 0.01;
/// How much room the wall leaves open over furniture's top: a standing
/// player's height and a plain jump's rise. Above it the wall goes on, so
/// a Force jump from the floor does not cross the furniture and the room's
/// own wall behind it.
pub(crate) const SLOT: f32 = (STAND_HEIGHT - SOLE_Z + JUMP_HEIGHT) * UNIT;
/// The least a furniture top's normal points up: about 25 degrees off
/// level. The feet of the Sith base's corridor walls on Taris slope 43
/// degrees (0.73), which JA still walks on: they are no furniture, and a
/// body let onto one is shoved against the wall above it
/// (`COLLISION.md`'s known issue).
const LEVEL_TOP: f32 = 0.9;
/// How far apart the heights are at which room to stand is looked for
/// over the furniture.
const RISE: f32 = 0.125;
/// Furniture the probe cannot rise this far from its top is under a roof
/// (the containment audit's measure of indoors).
const ROOF: f32 = 10.0;
/// How much of a room's walkable floor is under a roof for the room to be
/// indoors. The enclave's courtyard on Dantooine has eaves over a plinth
/// in one corner, with open ground beside it.
const INDOORS: f32 = 0.9;
/// A top higher than this over the edge is not looked for: the wall in
/// front of taller furniture stays whole.
const TALLEST: f32 = 4.0;
/// How far behind furniture a wall up to the roof, or walkable floor
/// again, is looked for.
const BACK: f32 = 4.0;
/// How high over the edge a top may stand for the rules made for what a
/// plain jump reaches (a strip before the furniture, a shelf behind it): a
/// step and a plain jump's rise, 1.27 m. Taller furniture is reached only
/// by a Force jump. On K1's cutscene modules stunt_07, stunt_16, stunt_34
/// and stunt_35, such a top 2.7 m up was a way out of the level.
const PLAIN_REACH: f32 = (STEP_HEIGHT + JUMP_HEIGHT) * UNIT;
/// How far behind the wall's line furniture is looked for across a low
/// strip (floor lights, a row of benches lower than a step): its front is
/// then less than a player's box width (0.76 m) behind the wall's back
/// face, so no box drops into the gap between them. The Sith base's
/// barracks table on Taris stands 0.81 m behind its edge.
const STRIP: f32 = 0.8;

/// What lies one way behind furniture.
#[derive(Clone, Copy, PartialEq)]
enum Behind {
    /// Walkable floor again.
    Floor,
    /// A wall from the furniture's top to the roof.
    Wall,
    /// Neither, as far as was looked.
    Open,
    /// Neither, but solid under the way all along, no lower than a metre
    /// under the edge: a shelf or a console a box walks along, with no drop.
    Shelf,
}

/// The ways behind furniture that must be closed: straight back, and 45
/// degrees to either side (along and across the way back).
const BEHIND: [(f32, f32); 3] = [
    (1.0, 0.0),
    (
        std::f32::consts::FRAC_1_SQRT_2,
        std::f32::consts::FRAC_1_SQRT_2,
    ),
    (
        std::f32::consts::FRAC_1_SQRT_2,
        -std::f32::consts::FRAC_1_SQRT_2,
    ),
];

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

/// What the walls end at furniture's tops by ([`furniture_top`]).
#[derive(Clone, Copy)]
pub struct Furniture<'a> {
    /// The rooms' solid: their drawn triangles and walkmesh faces.
    pub rooms: &'a Level,
    /// For each walkable face, whether its room is indoors
    /// ([`indoors`]): only an indoor room's walls end at furniture.
    pub indoors: &'a [bool],
}

/// Whether a room is indoors: `roofed` of its `floor` (areas of walkable
/// floor) is under a roof, [`INDOORS`] of it or more.
pub fn indoors(roofed: f32, floor: f32) -> bool {
    floor > 0.0 && roofed >= INDOORS * floor
}

/// Whether `rooms`' solid roofs a walkable face: a point over its middle
/// cannot rise [`ROOF`].
pub fn roofed(rooms: &Level, face: &[[f32; 3]; 3]) -> bool {
    let middle: [f32; 3] = std::array::from_fn(|i| (face[0][i] + face[1][i] + face[2][i]) / 3.0);
    let from = [middle[0], middle[1], middle[2] + FOOT];
    let to = [middle[0], middle[1], middle[2] + ROOF];
    rooms
        .trace(from, to, &Extents::POINT, Contents::SOLID)
        .fraction
        < 1.0
}

/// The walls along the open edges of `walkable` (world-space faces with
/// their materials); `walkmesh` holds every walkmesh face, whatever its
/// material. With `furniture`, a wall of an indoor room ends at the top of
/// furniture standing in it ([`furniture_top`]).
pub fn walls(
    walkable: &[([[f32; 3]; 3], u32)],
    walkmesh: &Level,
    furniture: Option<Furniture>,
    surfaces: &Surfaces,
    options: &EdgeWalls,
) -> Vec<ClipShape> {
    let key = |p: [f32; 3]| p.map(|v| (v / WELD).round() as i64);
    // A face with two corners on one vertex has no area: it is no floor,
    // and its doubled edge would read as one a neighbour continues (Taris
    // undercity's has one beside a real edge, which had no wall).
    let kept: Vec<usize> = (0..walkable.len())
        .filter(|&face| {
            let [a, b, c] = walkable[face].0.map(key);
            a != b && b != c && c != a
        })
        .collect();
    let walkable: Vec<&([[f32; 3]; 3], u32)> = kept.iter().map(|&face| &walkable[face]).collect();
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
    // The vertices of loops round holes in the floor: what stands in one
    // is furniture with floor all round, indoors or not.
    let holes = holes(&walkable, &outline, &shared, key);
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
    let floor_under = |p: [f32; 2], high: f32, low: f32| {
        let hit = walkmesh.trace(
            [p[0], p[1], high],
            [p[0], p[1], low],
            &Extents::POINT,
            Contents::SOLID,
        );
        hit.fraction < 1.0 && surfaces.walkable(u32::from(hit.surface.material))
    };
    let mut out = Vec::new();
    for (face, (corners, _)) in walkable.into_iter().enumerate() {
        let indoors = furniture
            .is_some_and(|furniture| furniture.indoors.get(kept[face]).copied().unwrap_or(false));
        let centre: [f32; 2] =
            std::array::from_fn(|i| (corners[0][i] + corners[1][i] + corners[2][i]) / 3.0);
        for k in 0..3 {
            let (a, b) = (corners[k], corners[(k + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            if shared.get(&(ka.min(kb), ka.max(kb))).copied().unwrap_or(0) == 3 {
                continue;
            }
            // The rooms' solid, for an edge of an indoor room or round a
            // hole in the floor.
            let rooms = furniture
                .filter(|_| indoors || (holes.contains(&ka) && holes.contains(&kb)))
                .map(|furniture| furniture.rooms);
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
                // Furniture's top at each sample of the wall's line, and
                // in a gap of one sample between a piece of furniture and
                // another, or solid standing as high as it.
                let edge_at = |k: usize| s[2] + (e[2] - s[2]) * k as f32 / samples as f32;
                let found: Vec<Option<f32>> = (0..=samples)
                    .map(|k| furniture_top(rooms?, &floor_under, point(k), out_n, edge_at(k)))
                    .collect();
                let stands = |k: usize, top: f32| {
                    rooms.is_some_and(|rooms| stands_over(rooms, point(k), top))
                };
                let perches: Vec<Option<f32>> = (0..=samples)
                    .map(|k| {
                        found[k].or_else(|| {
                            let (one, two) = (k.checked_sub(1)?, k + 1);
                            let top = match (found[one], *found.get(two)?) {
                                (Some(one), Some(two)) => one.min(two),
                                (Some(top), None) => stands(two, top).then_some(top)?,
                                (None, Some(top)) => stands(one, top).then_some(top)?,
                                (None, None) => return None,
                            };
                            gap_top(rooms?, point(k), out_n, edge_at(k), top)
                        })
                    })
                    .collect();
                // Each stretch as high as the floor over its ends lets it:
                // a floor overhead (a ramp passing over) lowers the wall
                // under it alone, not the whole run, which a jump would
                // clear beside the ramp. Furniture at both its ends lowers
                // it to the higher of the two tops; furniture at one, and
                // solid standing as high at the other, to the furniture's.
                let stretch = |k: usize| -> f32 {
                    let top = tops[k].min(tops[k + 1]);
                    match (perches[k], perches[k + 1]) {
                        (Some(one), Some(two)) => top.min(one.max(two)),
                        (Some(one), None) if stands(k + 1, one) => top.min(one),
                        (None, Some(two)) if stands(k, two) => top.min(two),
                        _ => top,
                    }
                };
                let mut k = 0;
                while k < samples {
                    let top = stretch(k);
                    let first = k;
                    while k + 1 < samples && stretch(k + 1) == top {
                        k += 1;
                    }
                    k += 1;
                    out.push(ClipShape::Wall {
                        from: point(first),
                        to: point(k),
                        z: [s[2].min(e[2]) - FOOT, top],
                        thickness: THICKNESS,
                    });
                    // Over furniture the wall goes on above the room a
                    // jump onto it takes.
                    let whole = tops[first..=k]
                        .iter()
                        .copied()
                        .fold(f32::INFINITY, f32::min);
                    if top + SLOT < whole {
                        out.push(ClipShape::Wall {
                            from: point(first),
                            to: point(k),
                            z: [top + SLOT, whole],
                            thickness: THICKNESS,
                        });
                    }
                }
            }
        }
    }
    out
}

/// The welded vertices of the outline's loops that go round a hole in the
/// walkable floor (the Sith base's reception desk on Taris, with floor on
/// every side). Each such loop is closed, every vertex of it on two of its
/// edges, and turns clockwise with the floor on its left. The walls of the
/// loop go up to the sky past the furniture's top, so a Force jump from the
/// top stays inside them; the room's floor need not be indoors
/// ([`indoors`]), though the top still needs a roof over it. A loop with a
/// vertex where more edges meet (a seam between rooms) is not counted.
fn holes(
    walkable: &[&([[f32; 3]; 3], u32)],
    outline: &Outline,
    shared: &HashMap<(Vertex, Vertex), u8>,
    key: impl Fn([f32; 3]) -> Vertex,
) -> HashSet<Vertex> {
    fn root(parent: &mut HashMap<Vertex, Vertex>, v: Vertex) -> Vertex {
        let mut r = v;
        while let Some(&up) = parent.get(&r) {
            if up == r {
                break;
            }
            r = up;
        }
        parent.insert(v, r);
        r
    }
    // Each outline edge with the floor on its left: its ends and twice the
    // signed area it adds to its loop.
    let mut parent: HashMap<Vertex, Vertex> = HashMap::new();
    let mut edges = Vec::new();
    for (corners, _) in walkable.iter().copied() {
        let [p0, p1, p2] = *corners;
        let ccw = (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]) > 0.0;
        for k in 0..3 {
            let (p, q) = (corners[k], corners[(k + 1) % 3]);
            let (a, b) = (key(p), key(q));
            if shared.get(&(a.min(b), a.max(b))).copied().unwrap_or(0) == 3 {
                continue;
            }
            let (p, q) = if ccw { (p, q) } else { (q, p) };
            let cross = f64::from(p[0]) * f64::from(q[1]) - f64::from(q[0]) * f64::from(p[1]);
            edges.push((a, b, cross));
            parent.entry(a).or_insert(a);
            parent.entry(b).or_insert(b);
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            if ra != rb {
                parent.insert(ra, rb);
            }
        }
    }
    let mut area: HashMap<Vertex, f64> = HashMap::new();
    let mut simple: HashMap<Vertex, bool> = HashMap::new();
    for &(a, b, cross) in &edges {
        let r = root(&mut parent, a);
        *area.entry(r).or_insert(0.0) += cross;
        let two = [a, b]
            .iter()
            .all(|v| outline.get(v).is_some_and(|edges| edges.len() == 2));
        *simple.entry(r).or_insert(true) &= two;
    }
    let vertices: Vec<Vertex> = parent.keys().copied().collect();
    vertices
        .into_iter()
        .filter(|&v| {
            let r = root(&mut parent, v);
            simple.get(&r).copied().unwrap_or(false) && area.get(&r).copied().unwrap_or(0.0) < 0.0
        })
        .collect()
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

/// The furniture probe: a column as wide as a wall is thick and as tall as
/// a standing player, its origin at its foot.
fn column() -> Extents {
    let half = THICKNESS * 0.5;
    Extents {
        mins: [-half, -half, 0.0],
        maxs: [half, half, (STAND_HEIGHT - SOLE_Z) * UNIT],
    }
}

/// How high `rooms`' solid stands a column at a time from an edge to the
/// wall line `to_wall` behind it at `p`: where a walker's soles are as he
/// nears the wall. Each column is let down from `top`; one that does not
/// fit there (solid taller than the top) counts as `top`.
fn approach(
    rooms: &Level,
    p: [f32; 2],
    behind: [f32; 2],
    to_wall: f32,
    edge: f32,
    top: f32,
) -> Vec<f32> {
    let column = column();
    let mut heights = Vec::new();
    let mut out = THICKNESS * 0.5;
    while out + THICKNESS <= to_wall {
        let q = [
            p[0] - behind[0] * (to_wall - out),
            p[1] - behind[1] * (to_wall - out),
        ];
        let hit = rooms.trace(
            [q[0], q[1], top + UNDER_TOP],
            [q[0], q[1], edge - FOOT],
            &column,
            Contents::SOLID,
        );
        heights.push(match hit.start_solid {
            true => top,
            false => hit.end[2],
        });
        out += THICKNESS;
    }
    heights
}

/// Where the wall behind an edge ends for furniture standing in it: just
/// under the top of `rooms`' solid at `p` (a point of the wall's middle
/// line) or a wall's thickness `behind` it (the unit direction away from
/// the floor), or, across a strip with no solid over a step, up to
/// [`STRIP`] behind it for a top a plain jump reaches ([`PLAIN_REACH`]; and
/// two columns on, past a bevel at the furniture's edge), when that solid
/// - reaches more than a step over `edge`, and rises by more than a step
///   at once somewhere between the edge and the wall (lower, or sloping up
///   from the floor as the foot of a room's wall does, a walker would
///   step or walk up onto it, and the wall stays whole to stop him);
/// - has a level top ([`LEVEL_TOP`]), not a slope;
/// - has room over it for a player to stand: the top is the lowest height
///   over the edge with that room (a duct close over a crate leaves none,
///   and the wall goes on up to the duct's top);
/// - is under a roof, in a room that is indoors ([`indoors`]; out of doors
///   a rock or a bank at the walkmesh's edge can be a way out of the
///   level, and its wall stays);
/// - is closed behind, straight back and 45 degrees to either side: within
///   [`BACK`] the probe comes over walkable floor again (`floor`: whether
///   the walkmesh's topmost face under a point, between two heights, is
///   walkable), or is stopped both at the top's height and just under the
///   roof (a room's wall), or, for a top a plain jump reaches, has solid
///   under it all the way, no lower than a metre under the edge (a shelf:
///   a console along the Sith base's corridors on Taris). Floor to both
///   sides is enough (a table 4 m long, met from its end). A rock under a
///   tree on Dantooine has a roof and open ground behind it, and was a way
///   out of the level. On Peragus (TSL 105per) a rail 0.55 m high stands
///   over a drop of 10 m under a sloped roof, which a probe from higher up
///   had taken for solid under the way;
/// - has no drop behind it: a body going on a half width from the top
///   meets more solid, or has some under it no lower than a metre under
///   the edge (a parapet over a pit keeps its wall).
pub(crate) fn furniture_top(
    rooms: &Level,
    floor: &dyn Fn([f32; 2], f32, f32) -> bool,
    p: [f32; 2],
    behind: [f32; 2],
    edge: f32,
) -> Option<f32> {
    let column = column();
    let step = STEP_HEIGHT * UNIT + OVER_STEP;
    let low = edge + step;
    let at = |p: [f32; 2], z: f32| [p[0], p[1], z];
    let fits = |p: [f32; 2], z: f32| {
        !rooms
            .trace(at(p, z), at(p, z), &column, Contents::SOLID)
            .start_solid
    };
    let behind_by = |back: f32| [p[0] + behind[0] * back, p[1] + behind[1] * back];
    // The wall's line and a wall's thickness behind it; across a strip with
    // no solid over a step, the first column with some and the two after.
    let mut backs = vec![0.0, THICKNESS];
    let mut back = 2.0 * THICKNESS;
    while back <= STRIP + 0.5 * THICKNESS && backs.iter().all(|&b| fits(behind_by(b), low)) {
        if !fits(behind_by(back), low) {
            backs.extend([back, back + THICKNESS, back + 2.0 * THICKNESS]);
            break;
        }
        back += THICKNESS;
    }
    backs.into_iter().find_map(|back| {
        let p = behind_by(back);
        // No solid over a step high here: no furniture.
        if fits(p, low) {
            return None;
        }
        let mut soles = low + RISE;
        while !fits(p, soles) {
            soles += RISE;
            if soles > edge + TALLEST {
                return None;
            }
        }
        let down = rooms.trace(at(p, soles), at(p, low), &column, Contents::SOLID);
        if down.plane.normal[2] < LEVEL_TOP {
            return None;
        }
        let top = down.end[2];
        let plain = top - edge <= PLAIN_REACH;
        if back > 1.5 * THICKNESS && !plain {
            return None;
        }
        // A rise no walker steps up, somewhere from the edge to here. With
        // none (the sloped foot of a wall) a walker would be standing as
        // high as the top when he came to the wall.
        let to_wall = SET_BACK + THICKNESS * 0.5 + back;
        let mut before = edge;
        let mut rises = false;
        for here in approach(rooms, p, behind, to_wall, edge, top)
            .into_iter()
            .chain([top])
        {
            rises |= here - before > step;
            before = here;
        }
        if !rises {
            return None;
        }
        let roof = rooms.trace(at(p, top), at(p, top + ROOF), &column, Contents::SOLID);
        if roof.fraction >= 1.0 {
            return None;
        }
        let reach = HALF_WIDTH * UNIT;
        let past = [p[0] + behind[0] * reach, p[1] + behind[1] * reach];
        let on = rooms.trace(at(p, top), at(past, top), &column, Contents::SOLID);
        if on.fraction >= 1.0 {
            let under = rooms.trace(
                at(past, top),
                at(past, edge - BELOW),
                &column,
                Contents::SOLID,
            );
            if under.fraction >= 1.0 {
                return None;
            }
        }
        // Closed behind: a wall from the top to the roof, or walkable
        // floor again, straight back and to either side of it; or floor
        // to both sides (a long table met from its end).
        let under_roof = roof.end[2];
        let behind_it = |&(along, across): &(f32, f32)| {
            let dir = [
                behind[0] * along - behind[1] * across,
                behind[1] * along + behind[0] * across,
            ];
            let far = [p[0] + dir[0] * BACK, p[1] + dir[1] * BACK];
            let low = rooms.trace(at(p, top), at(far, top), &column, Contents::SOLID);
            let mut out = PIECE * 0.5;
            while out <= low.fraction * BACK {
                let q = [p[0] + dir[0] * out, p[1] + dir[1] * out];
                if floor(q, top, edge - BELOW) {
                    return Behind::Floor;
                }
                out += PIECE * 0.5;
            }
            let high = rooms.trace(
                at(p, under_roof),
                at(far, under_roof),
                &column,
                Contents::SOLID,
            );
            if low.fraction < 1.0 && high.fraction < 1.0 {
                return Behind::Wall;
            }
            // Solid under the way all along, from just over the top: from
            // higher, a roof over a drop read as solid.
            let shelf = plain && {
                let mut out = PIECE * 0.5;
                let mut solid = true;
                while solid && out <= BACK {
                    let q = [p[0] + dir[0] * out, p[1] + dir[1] * out];
                    solid = rooms
                        .trace(
                            at(q, top + RISE),
                            at(q, edge - BELOW),
                            &column,
                            Contents::SOLID,
                        )
                        .fraction
                        < 1.0;
                    out += PIECE * 0.5;
                }
                solid
            };
            match shelf {
                true => Behind::Shelf,
                false => Behind::Open,
            }
        };
        let [back, one, two] = BEHIND.each_ref().map(behind_it);
        let closed = (one == Behind::Floor && two == Behind::Floor)
            || ![back, one, two].contains(&Behind::Open);
        closed.then_some(top - UNDER_TOP)
    })
}

/// Whether `rooms`' solid at `p` on a wall's line stands as high as
/// `top`, a piece of furniture's beside it: a player standing at that
/// height does not fit there (a taller stack, a tilted crate, a duct
/// overhead), so no body crosses the wall's line there whatever the wall.
fn stands_over(rooms: &Level, p: [f32; 2], top: f32) -> bool {
    let at = [p[0], p[1], top + 2.0 * UNDER_TOP];
    rooms.trace(at, at, &column(), Contents::SOLID).start_solid
}

/// Where the wall ends in a gap beside a piece of furniture: at `top`, the
/// furniture's (the lower's, between two pieces), when a walker nearing
/// the wall at `p` stands more than a step under it. The gap is one sample
/// of a wall wide, under a metre, with furniture or solid as high on both
/// sides: the player's box, 0.76 m, rests on what stands either side of
/// it, and a whole wall standing in the gap stopped a jump onto the
/// furniture (a crate 0.9 m wide between a taller stack and such a gap, in
/// the Sith base on Taris).
pub(crate) fn gap_top(
    rooms: &Level,
    p: [f32; 2],
    behind: [f32; 2],
    edge: f32,
    top: f32,
) -> Option<f32> {
    let step = STEP_HEIGHT * UNIT + OVER_STEP;
    let to_wall = SET_BACK + THICKNESS * 0.5;
    approach(rooms, p, behind, to_wall, edge, top)
        .into_iter()
        .chain([edge])
        .all(|soles| top - soles > step)
        .then_some(top)
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
