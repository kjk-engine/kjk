//! The burn and the glow a lit blade leaves where it drags along a wall, as
//! JA's client lays and draws them:
//!
//! - A blade that met the level last time and meets it again, where the
//!   level itself was met (not a door, a placeable or a body), lays a strip
//!   from where it met before to where it meets now: `RADIUS` either side
//!   of the line and past either end, on the wall's plane, laid onto the
//!   level's faces (`fragments`); each point's texture coordinates `0.5`
//!   at the middle and spread along by 0.05 to 0.08 a unit and across by
//!   0.15 to 0.2, drawn afresh for each point.
//! - Laying a strip: the faces near it (in a box round the strip and its
//!   push along the wall's reversed normal) that face it are each cut to
//!   the strip's four edges and to a slab from `IN_FRONT` units before the
//!   strip to `BEHIND` past it; what is left of a face, if a polygon, is a
//!   piece (`fragments`, `keep_front`).
//! - Each piece is two marks: the burn (`gfx/damage/rivetmark`, twice the
//!   texture times the wall) and the glow (`gfx/effects/saberDamageGlow`,
//!   added, red 215–255, green 96–128, blue 0–15), the glow dated
//!   `GLOW_BACK_MS` back so it lives 1.5 s, full for half a second and
//!   darkening to nothing over the last.
//! - Marks live `LIFE_MS`; the client keeps `MOST` of them (both kinds,
//!   and every other mark), dropping the oldest. The burn's last second
//!   fades its vertex alpha, which its shader does not read: it goes at
//!   once (QUIRKS.md Q-0280).
//! - The client lays one strip a frame, from the frame's trace; here
//!   `blade_fx` cuts once a client frame from where the last cut ended,
//!   and a cut over several frames (snapshots further apart) is laid as a
//!   strip a frame.
//!
//! The wall sound and sparks are `blade_fx`'s.

use std::collections::VecDeque;

use asset_ja::fx::Rng;
use bevy::prelude::*;

use crate::scene_fx::{Batch, ShaderMeshes, vertex_colour};

const BURN: &str = "gfx/damage/rivetmark";
const GLOW: &str = "gfx/effects/saberDamageGlow";
/// The shaders the marks are drawn with.
pub const SHADERS: &[&str] = &[BURN, GLOW];
/// How far a strip reaches either side of the blade's line and past its
/// ends (units).
const RADIUS: f32 = 0.65;
/// How far off the wall a strip is drawn (units), as `fx::mark` lifts a
/// decal (JA: `polygonOffset`).
const LIFT: f32 = 0.1;
/// The marks kept (the client's most).
const MOST: usize = 256;
/// A strip's pieces, all their points, and a piece's points, at most
/// (the client's limits).
const MOST_PIECES: usize = 128;
const ALL_POINTS: usize = 384;
const MOST_POINTS: usize = 10;
/// How far in front of the strip and behind it faces are cut (units).
const IN_FRONT: f32 = 32.0;
const BEHIND: f32 = 20.0;
/// How far off a plane a point is still on it, cutting (units).
const ON_PLANE: f32 = 0.5;
/// How long a mark lives, and over how much of its end it fades (ms).
const LIFE_MS: f32 = 10_000.0;
const FADE_MS: f32 = 1000.0;
/// How far back a glow is dated (ms).
const GLOW_BACK_MS: f32 = 8500.0;

/// Where a blade dragged along a wall since the last strip: from, to (units),
/// the wall's normal there, when (the view's ms), and how many client frames
/// it took.
#[derive(Clone, Copy, Debug)]
pub struct Cut {
    pub from: Vec3,
    pub to: Vec3,
    pub normal: Vec3,
    pub ms: f32,
    pub frames: u32,
    /// Seeds the strip's random numbers.
    pub seed: (u32, u32),
}

/// The cuts made since the marks were last drawn.
#[derive(Resource, Default)]
pub struct Cuts(pub Vec<Cut>);

/// The level's faces overlapping a box (units: its least and most
/// corners), each a triangle anticlockwise seen from outside.
pub type Faces<'a> = &'a dyn Fn(Vec3, Vec3) -> Vec<[Vec3; 3]>;

/// One mark: its points and their texture coordinates, whether it is the
/// glow, from when it counts its life (the view's ms), its colour.
struct Mark {
    points: Vec<(Vec3, [f32; 2])>,
    glow: bool,
    from_ms: f32,
    rgb: [u8; 3],
}

/// The marks laid, oldest first.
#[derive(Default)]
pub struct Marks(VecDeque<Mark>);

impl Marks {
    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The strips `cut` leaves on `faces`, a client frame's each.
    pub fn lay(&mut self, cut: &Cut, faces: Faces) {
        let frames = cut.frames.max(1);
        let mut rng = crate::view_rng::ja_seed(cut.seed.0, cut.seed.1);
        for frame in 0..frames {
            let along = |k: u32| cut.from.lerp(cut.to, k as f32 / frames as f32);
            self.strip(
                (along(frame), along(frame + 1), cut.normal),
                (cut.ms, faces),
                &mut rng,
            );
        }
        let over = self.0.len().saturating_sub(MOST);
        self.0.drain(..over);
    }

    /// One strip from `start` to `end` on the plane of `normal`, laid on
    /// `faces`: none when the blade has not moved along it.
    fn strip(
        &mut self,
        (start, end, normal): (Vec3, Vec3, Vec3),
        (ms, faces): (f32, Faces),
        rng: &mut Rng,
    ) {
        let normal = normal.normalize_or(Vec3::Z);
        let Some(along) = (end - start).try_normalize() else {
            return;
        };
        let across = along.cross(normal);
        let (back, out) = (start - along * RADIUS, end + along * RADIUS);
        let corners = [
            back - across * RADIUS,
            out - across * RADIUS,
            out + across * RADIUS,
            back + across * RADIUS,
        ];
        let middle = (start + end) / 2.0;
        for piece in fragments(corners, -normal, faces) {
            let points: Vec<(Vec3, [f32; 2])> = piece
                .into_iter()
                .map(|point| {
                    let delta = point - middle;
                    let s = 0.5 + delta.dot(along) * rng.range(0.05, 0.08);
                    let t = 0.5 + delta.dot(across) * rng.range(0.15, 0.2);
                    (point + normal * LIFT, [s, t])
                })
                .collect();
            let rgb = [(215.0, 255.0), (96.0, 128.0), (0.0, 15.0)]
                .map(|(low, high)| rng.range(low, high) as u8);
            self.0.push_back(Mark {
                points: points.clone(),
                glow: false,
                from_ms: ms,
                rgb: [255; 3],
            });
            self.0.push_back(Mark {
                points,
                glow: true,
                from_ms: ms - GLOW_BACK_MS,
                rgb,
            });
        }
    }

    /// The marks alive at `now` into `batches`, the spent ones dropped.
    pub fn draw(
        &mut self,
        now: f32,
        shaders: &ShaderMeshes,
        batches: &mut std::collections::BTreeMap<String, Batch>,
        to_bevy: impl Fn(Vec3) -> Vec3 + Copy,
    ) {
        self.0.retain(|mark| now <= mark.from_ms + LIFE_MS);
        for mark in &self.0 {
            if now < mark.from_ms {
                continue;
            }
            let shader = if mark.glow { GLOW } else { BURN }.to_ascii_lowercase();
            let Some(blend) = shaders.blend(&shader) else {
                continue;
            };
            // The glow darkens; the burn's alpha would fade, unread.
            let left = mark.from_ms + LIFE_MS - now;
            let fade = if mark.glow {
                (left / FADE_MS).min(1.0)
            } else {
                1.0
            };
            let [r, g, b] = mark.rgb.map(|v| (f32::from(v) * fade) as u8);
            let colour = vertex_colour([r, g, b, 255], blend);
            let batch = batches.entry(shader).or_default();
            // A fan, two of its triangles a quad (the last alone, doubled).
            let last = mark.points.len() - 1;
            for i in (1..last).step_by(2) {
                let at = |k: usize| mark.points[k.min(last)];
                batch.quad([at(0), at(i), at(i + 1), at(i + 2)], colour, to_bevy);
            }
        }
    }
}

/// The pieces of the level's faces a strip lands on: the polygon `corners`
/// (four points, units, anticlockwise seen against `projection`) pushed
/// along `projection` onto `faces`. Each face turned towards the strip is
/// cut to the strip's outline and to a slab from `IN_FRONT` before its
/// plane to `BEHIND` past it; at most `MOST_PIECES` pieces of at most
/// `MOST_POINTS` points, and `ALL_POINTS` points in all.
fn fragments(corners: [Vec3; 4], projection: Vec3, faces: Faces) -> Vec<Vec<Vec3>> {
    let way = projection.normalize_or(Vec3::NEG_Z);
    let (least, most) = corners
        .iter()
        .flat_map(|&p| [p, p + projection, p - way * BEHIND])
        .fold((Vec3::MAX, Vec3::MIN), |(least, most), p| {
            (least.min(p), most.max(p))
        });

    // Planes keeping the side where `point · normal - dist` is positive:
    // inside each edge of the strip, then the slab's near and far sides.
    let mut planes: Vec<(Vec3, f32)> = (0..4)
        .map(|i| {
            let edge = corners[(i + 1) % 4] - corners[i];
            let normal = edge.cross(-projection).normalize_or_zero();
            (normal, normal.dot(corners[i]))
        })
        .collect();
    planes.push((way, way.dot(corners[0]) - IN_FRONT));
    planes.push((-way, -way.dot(corners[0]) - BEHIND));

    let mut pieces: Vec<Vec<Vec3>> = Vec::new();
    let mut taken = 0;
    for [a, b, c] in faces(least, most) {
        let facing = (b - a).cross(c - a).normalize_or_zero();
        if facing.dot(way) > -0.5 {
            continue;
        }
        let mut piece = vec![a, b, c];
        for &(normal, dist) in &planes {
            if piece.is_empty() {
                break;
            }
            piece = keep_front(&piece, normal, dist);
        }
        if piece.len() < 3 || taken + piece.len() > ALL_POINTS {
            continue;
        }
        taken += piece.len();
        piece.truncate(MOST_POINTS);
        pieces.push(piece);
        if pieces.len() == MOST_PIECES {
            break;
        }
    }
    pieces
}

/// The part of `polygon` in front of the plane `normal`, `dist` (where
/// `point · normal - dist` is positive), points within `ON_PLANE` of it
/// counting as on it: nothing when no point is in front, the polygon
/// itself when none is behind, else its points in front or on the plane
/// with a new point wherever an edge crosses from one side to the other.
fn keep_front(polygon: &[Vec3], normal: Vec3, dist: f32) -> Vec<Vec3> {
    let distances: Vec<f32> = polygon.iter().map(|p| p.dot(normal) - dist).collect();
    // 1 in front, -1 behind, 0 on the plane (NaN too).
    let sides: Vec<i8> = distances
        .iter()
        .map(|&d| {
            if d > ON_PLANE {
                1
            } else if d < -ON_PLANE {
                -1
            } else {
                0
            }
        })
        .collect();
    if !sides.contains(&1) {
        return Vec::new();
    }
    if !sides.contains(&-1) {
        return polygon.to_vec();
    }
    let n = polygon.len();
    let mut kept = Vec::with_capacity(n + 2);
    for i in 0..n {
        let j = (i + 1) % n;
        if sides[i] >= 0 {
            kept.push(polygon[i]);
        }
        if sides[i] * sides[j] < 0 {
            let (di, dj) = (distances[i], distances[j]);
            kept.push(polygon[i].lerp(polygon[j], di / (di - dj)));
        }
    }
    kept
}
