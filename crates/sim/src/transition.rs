//! The area transition the player faces (`docs/UI.md`, "Area
//! transitions"), found render side each frame from the presented
//! snapshot, as KOTOR's client finds the one its area transition panel
//! names: each door and trigger that leads to a module offers itself when
//! the line the player faces, `REACH` long across the floor, meets it and
//! it is in sight of the player's eyes; the nearest wins. A door is met by
//! its pick volume (`target::DOOR_RADIUS`; KOTOR's, its model's box) and
//! must stand ahead; a trigger by its outline, where the line crosses it
//! first. KOTOR asks again every frame: nothing holds.

use crate::instance::Instances;
use crate::math::{self, Vec3};
use crate::module::{Link, ObjectKind};
use crate::target::{DOOR_RADIUS, Looker, in_sight};
use crate::world::Level;
use crate::{Kind, Snapshot};

/// How far ahead of the player (metres, across the floor) a door or
/// trigger is named.
pub const REACH: f32 = rules::targeting::transition::REACH;
/// How high the player's eyes are above its feet: the sight line runs at
/// their height.
const EYES: f32 = rules::targeting::pick::EYES;

/// The door or trigger the player faces that leads out: its number among
/// the area's objects, where the facing line meets it (metres) and where
/// it leads.
#[derive(Debug, Clone, PartialEq)]
pub struct Faced {
    pub index: u32,
    pub kind: ObjectKind,
    pub at: Vec3,
    pub link: Link,
}

/// The transition `looker` faces in `snapshot` (see the module
/// documentation), doors as `states` has them.
pub fn faced(
    level: &Level,
    states: Option<&[u8]>,
    snapshot: &Snapshot,
    looker: &Looker,
) -> Option<Faced> {
    let instances = Instances::in_snapshot(level, snapshot);
    let feet = looker.feet;
    let [fx, fy] = looker.facing;
    let end = [feet[0] + fx * REACH, feet[1] + fy * REACH];
    let eyes = [feet[0], feet[1], feet[2] + EYES];
    snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let Kind::Object(kind, index) = entity.kind else {
                return None;
            };
            let facts = instances.of_entity(entity)?;
            let link = facts.exit()?;
            let [x, y] = match kind {
                ObjectKind::Door => {
                    let at = entity.position;
                    let ahead = (at[0] - feet[0]) * fx + (at[1] - feet[1]) * fy;
                    (ahead > 0.0 && meets_circle([feet[0], feet[1]], end, at, DOOR_RADIUS))
                        .then_some([at[0], at[1]])?
                }
                ObjectKind::Trigger => {
                    let outline: Vec<[f32; 2]> = facts
                        .geometry
                        .iter()
                        .map(|point| [entity.position[0] + point[0], entity.position[1] + point[1]])
                        .collect();
                    first_crossing([feet[0], feet[1]], end, &outline)?
                }
                _ => return None,
            };
            let at = [x, y, eyes[2]];
            in_sight(level, states, eyes, at, index).then(|| Faced {
                index,
                kind,
                at,
                link: link.clone(),
            })
        })
        .min_by(|a, b| {
            let away = |faced: &Faced| math::flat_distance(faced.at, feet);
            away(a).total_cmp(&away(b))
        })
}

/// Whether the segment `from`–`to` passes within `radius` of `centre`
/// (across the floor).
fn meets_circle(from: [f32; 2], to: [f32; 2], centre: Vec3, radius: f32) -> bool {
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let (px, py) = (centre[0] - from[0], centre[1] - from[1]);
    let along = ((px * dx + py * dy) / (dx * dx + dy * dy).max(f32::EPSILON)).clamp(0.0, 1.0);
    let (cx, cy) = (px - dx * along, py - dy * along);
    cx * cx + cy * cy <= radius * radius
}

/// Where the segment `from`–`to` first crosses an edge of the closed
/// `outline`, if it does.
fn first_crossing(from: [f32; 2], to: [f32; 2], outline: &[[f32; 2]]) -> Option<[f32; 2]> {
    let d = [to[0] - from[0], to[1] - from[1]];
    let cross = |a: [f32; 2], b: [f32; 2]| a[0] * b[1] - a[1] * b[0];
    let edges = outline
        .iter()
        .zip(outline.iter().cycle().skip(1))
        .take(outline.len());
    edges
        .filter_map(|(&a, &b)| {
            let e = [b[0] - a[0], b[1] - a[1]];
            let denom = cross(d, e);
            if denom.abs() < f32::EPSILON {
                return None;
            }
            let w = [a[0] - from[0], a[1] - from[1]];
            let t = cross(w, e) / denom;
            let u = cross(w, d) / denom;
            ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
        })
        .min_by(f32::total_cmp)
        .map(|t| [from[0] + d[0] * t, from[1] + d[1] * t])
}
