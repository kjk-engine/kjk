//! Walking a route (I27): from a character where it stands to a place,
//! over the level's navigation mesh (`collision::nav`), and the command
//! that walks it there, as a player's hands would (C14): look at the next
//! corner, run.
//!
//! Routes are worked out again whenever they are needed (a route across
//! the Endar Spire's corridors takes about 12 µs), so a walk keeps only
//! where it is going and the corner it is making for (`Walker`).

use std::sync::Arc;

use collision::UNIT;
use rules::walking::pace;

use crate::combat::mind;
use crate::input::Command;
use crate::math::{self, Vec3};
use crate::module::{ObjectData, ObjectKind, TRIGGER_REACH_M, inside_outline};
use crate::motion::{self, Anim, MotionState};
use crate::script::{MoveRates, STUCK_STALLS};
use crate::snapshot::{ObjectState, SimId};
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};
use crate::world::Level;

/// How far a route keeps a body's centre off the walls where it turns
/// (units): JA's half-width and a hand more, as KOTOR's walkmeshes run on
/// under the walls' feet. The navigation mesh is baked for it.
pub const CLEARANCE: f32 = rules::walking::route::CLEARANCE;

/// How far (metres) over or under a scripted move's goal its floor is
/// looked for (`on_floor`).
const GOAL_COLUMN_M: f32 = rules::walking::searches::GOAL_COLUMN_M;

/// A character this close to a corner (units, seen from above) has
/// reached it; so has one this close to the way on from it.
pub const REACHED: f32 = rules::walking::route::REACHED;

/// The navigation mesh's face under a point (metres): its floor within
/// `FLOOR_BELOW_M` under it or `FLOOR_ABOVE_M` over it.
fn face_under(nav: &collision::nav::NavMesh, point: Vec3) -> Option<u32> {
    nav.locate(point, FLOOR_BELOW_M, FLOOR_ABOVE_M)
}

/// How far (metres) under and over a point its floor on the mesh is
/// looked for (`face_under`).
const FLOOR_BELOW_M: f32 = rules::walking::searches::FLOOR_BELOW_M;
const FLOOR_ABOVE_M: f32 = rules::walking::searches::FLOOR_ABOVE_M;

/// A standing character's soles (metres) from its origin (units).
fn soles(origin: Vec3) -> Vec3 {
    let [x, y, z] = origin;
    math::scale([x, y, z + motion::SOLE_Z], UNIT)
}

/// Whether a walker may go where the level's entity `index` (the module's
/// placement of that number) stands in `state`: through a door that is
/// open or opens (a walker opens it, as a player does with use), a door
/// that leads out of the module too: its link is its use's, a player's
/// click (as reone has it), while a walker opens it as any
/// other (K1's cantinas keep the NPCs' way out, `p_exit` and the trigger
/// that takes them out of play, past their door to the Lower City); never
/// through a locked door or a placeable; always where one out of play stood.
pub fn passable(level: &Level, index: u32, state: ObjectState) -> bool {
    // Out of play (destroyed): nothing stands there.
    if state.state == collision::GONE {
        return true;
    }
    level.placed().get(index as usize).is_some_and(|object| {
        object.kind == ObjectKind::Door && (state.state != 0 || state.opens(object.opens))
    })
}

/// The corners of a route from a character standing at `from` to one
/// standing at `to` (origins, units), where a standing character's origin
/// would be at each; the goal last; through the objects `state` says
/// (`passable` with each one's state). `None` without a navigation mesh,
/// off it, or when no way leads there.
pub fn route(
    level: &Level,
    from: Vec3,
    to: Vec3,
    state: &dyn Fn(u32) -> ObjectState,
) -> Option<Vec<Vec3>> {
    route_round(level, from, to, state, &|_| false)
}

/// Whether a way leads a walker standing at `from` (its origin, units) to
/// `to` through the objects `state` says: a route from where it stands, or,
/// off the navigation mesh, from where it steps back on to it (`back_on`)
/// when that is within `REACHED`, as its walk goes (`Walker::plan`); one
/// farther off has no way (a Vulkar patrol droid told otherwise walked on
/// and on and never came to its fight). A level with no mesh is open.
pub fn leads(level: &Level, from: Vec3, to: Vec3, state: &dyn Fn(u32) -> ObjectState) -> bool {
    level.nav().is_none()
        || route(level, from, to, state).is_some()
        || back_on(level, from)
            .filter(|on| math::flat_distance(*on, from) <= REACHED)
            .is_some_and(|on| route(level, on, to, state).is_some())
}

/// `route`, keeping off the faces of the navigation mesh `walled` names (by
/// number) but the goal's: the floor under a trigger a walk goes round
/// (`faces_under`).
pub fn route_round(
    level: &Level,
    from: Vec3,
    to: Vec3,
    state: &dyn Fn(u32) -> ObjectState,
    walled: &dyn Fn(u32) -> bool,
) -> Option<Vec<Vec3>> {
    let nav = level.nav()?;
    // A walker a step into the doorway of a door it may not pass routes from
    // the nearest floor outside the door's faces, on its own side (one by
    // Korriban's sealed door at the way in to Dreshdae found every way shut;
    // letting it through the door's faces sent another across a closed door
    // by Hrakert's sub).
    let start = soles(from);
    // So too a walker standing at a placeable's footprint, its middle at the
    // outline over a face under it (the walls stop it there; Dantooine's
    // landspeeder in the enclave's courtyard shut every way out).
    let shut = face_under(nav, start)
        .and_then(|face| nav.faces[face as usize].entity)
        .filter(|index| {
            level.placed().get(*index as usize).is_some_and(|object| {
                object.kind == ObjectKind::Placeable
                    || object.kind == ObjectKind::Door && !passable(level, *index, state(*index))
            })
        });
    // The floor round it, nearest first (a face once each), from which a
    // way leads: the doorway's side the goal is on. Where none does, from
    // where it stands (the search lets a walker off its own face): a door's
    // face can reach 3 m into a room, as the Unknown World Elders' entrance
    // does, all its floor near the player the doorway's.
    let mut starts = shut.map_or_else(Vec::new, |door| outside(nav, start, door));
    starts.push(start);
    let corners = starts.into_iter().find_map(|start| {
        nav.route_round(
            start,
            soles(to),
            false,
            &|index| passable(level, index, state(index)),
            walled,
        )
    })?;
    let last = corners.len().saturating_sub(1);
    Some(
        corners
            .into_iter()
            .enumerate()
            .map(|(at, corner)| {
                let [x, y, z] = math::scale(corner, 1.0 / UNIT);
                let origin = [x, y, z - motion::SOLE_Z];
                if at == last {
                    origin
                } else {
                    clear(level, origin)
                }
            })
            .collect(),
    )
}

/// Whether each face of the navigation mesh (by number) lies under
/// `trigger`'s outline, seen from above, any part of it: a corner in the
/// outline, a point of the outline in the face, or their edges crossing;
/// on the trigger's floor (a corner within `TRIGGER_REACH_M` of its points'
/// heights). None without a mesh.
pub fn faces_under(level: &Level, trigger: &ObjectData) -> Vec<bool> {
    let Some(nav) = level.nav() else {
        return Vec::new();
    };
    if trigger.geometry.len() < 3 {
        return vec![false; nav.faces.len()];
    }
    let at = trigger.position;
    let outline: Vec<[f32; 2]> = trigger
        .geometry
        .iter()
        .map(|p| [at[0] + p[0], at[1] + p[1]])
        .collect();
    let (low, high) = trigger
        .geometry
        .iter()
        .fold((f32::MAX, f32::MIN), |(low, high), p| {
            (low.min(at[2] + p[2]), high.max(at[2] + p[2]))
        });
    let inside =
        |_: usize, p: [f32; 2]| inside_outline(&trigger.geometry, [p[0] - at[0], p[1] - at[1]]);
    faces_under_outlines(
        nav,
        &[outline],
        &inside,
        (low - TRIGGER_REACH_M, high + TRIGGER_REACH_M),
    )
}

/// The faces of the navigation mesh under the collision cells of a
/// placeable play made (`ObjectData::geometry`, pairs of corners in its own
/// frame, turned by its facing and stood at `at`, metres), by number: what a
/// walk plans round, as the mesh was baked before play made it.
pub fn faces_under_made(level: &Level, facts: &ObjectData, at: Vec3) -> Vec<bool> {
    level
        .nav()
        .map_or_else(Vec::new, |nav| made_faces(nav, facts, at))
}

/// The faces of the navigation mesh under the placeables play made
/// (`faces_under_made`, all of them at once), worked out again only when
/// the set of them changes: one made, moved or gone, or another level.
/// Derived from what the world shows (each made placeable's number, place
/// and facts), so a client works out the same as the authority.
#[derive(Debug, Default, Clone)]
pub struct MadeWalls {
    /// Each made placeable's number and place (bits), after the level's
    /// placements count and mesh size.
    key: Vec<(u32, [u32; 3])>,
    faces: Arc<[bool]>,
}

impl MadeWalls {
    /// The faces under the placeables `made` names (each one's number and
    /// where it stands, metres) in `level`, their facts asked of `facts`
    /// only when the set changed; empty when none stands on a face.
    pub fn faces(
        &mut self,
        level: &Level,
        made: &[(u32, Vec3)],
        facts: &dyn Fn(u32) -> Option<Arc<ObjectData>>,
    ) -> &Arc<[bool]> {
        let size = level.nav().map_or(0, |nav| nav.faces.len() as u32);
        let same = self.key.first() == Some(&(u32::MAX, [level.placed().len() as u32, size, 0]))
            && self.key.len() == made.len() + 1
            && self.key[1..]
                .iter()
                .zip(made)
                .all(|(key, (index, at))| *key == (*index, at.map(f32::to_bits)));
        if !same {
            let mut under: Vec<bool> = Vec::new();
            for (index, at) in made {
                let Some(data) = facts(*index) else {
                    continue;
                };
                let faces = faces_under_made(level, &data, *at);
                under.resize(faces.len().max(under.len()), false);
                for (wall, face) in under.iter_mut().zip(faces) {
                    *wall |= face;
                }
            }
            self.faces = if under.contains(&true) {
                under.into()
            } else {
                Arc::new([])
            };
            self.key = std::iter::once((u32::MAX, [level.placed().len() as u32, size, 0]))
                .chain(
                    made.iter()
                        .map(|(index, at)| (*index, at.map(f32::to_bits))),
                )
                .collect();
        }
        &self.faces
    }

    /// `faces` for the placeables play made that `snapshot` shows in
    /// `level`: what a view or the playthrough, which hold snapshots, plan
    /// round as the sim's walks do.
    pub fn of_snapshot(
        &mut self,
        level: &Level,
        snapshot: &crate::snapshot::Snapshot,
    ) -> &Arc<[bool]> {
        let placed = level.placed().len() as u32;
        let made: Vec<(u32, Vec3)> = snapshot
            .entities
            .iter()
            .filter_map(|entity| match entity.kind {
                crate::snapshot::Kind::Object(ObjectKind::Placeable, index) if index >= placed => {
                    Some((index, entity.position))
                }
                _ => None,
            })
            .collect();
        let objects = std::cell::OnceCell::new();
        let facts = |index: u32| {
            objects
                .get_or_init(|| crate::instance::Instances::in_snapshot(level, snapshot))
                .facts(index)
                .cloned()
        };
        self.faces(level, &made, &facts)
    }
}

/// What a walk plans through: the doors (each object's state by its
/// placement, `passable`) and the faces it keeps off (`route_round`): the
/// faces under the placeables play made (`MadeWalls`).
pub trait Ways {
    fn door(&self, index: u32) -> ObjectState;
    fn walls(&self, _face: u32) -> bool {
        false
    }
}

/// Doors alone: no face kept off.
impl<F: Fn(u32) -> ObjectState> Ways for F {
    fn door(&self, index: u32) -> ObjectState {
        self(index)
    }
}

/// The doors as `doors` says, off the faces `walled` names (by number).
pub struct Walled<'a> {
    pub doors: &'a dyn Fn(u32) -> ObjectState,
    pub walled: &'a [bool],
}

impl Ways for Walled<'_> {
    fn door(&self, index: u32) -> ObjectState {
        (self.doors)(index)
    }

    fn walls(&self, face: u32) -> bool {
        self.walled.get(face as usize).copied().unwrap_or(false)
    }
}

/// `route` through `ways`: off the faces it keeps off where a way round
/// them leads, else through them.
pub fn route_ways(level: &Level, from: Vec3, to: Vec3, ways: &dyn Ways) -> Option<Vec<Vec3>> {
    route_round(level, from, to, &|index| ways.door(index), &|face| {
        ways.walls(face)
    })
    .or_else(|| route(level, from, to, &|index| ways.door(index)))
}

/// `faces_under_made` on `nav`.
pub fn made_faces(nav: &collision::nav::NavMesh, facts: &ObjectData, at: Vec3) -> Vec<bool> {
    let (sin, cos) = math::sin_cos_deg(facts.yaw);
    let place = |x: f32, y: f32| [at[0] + x * cos - y * sin, at[1] + x * sin + y * cos];
    let outlines: Vec<Vec<[f32; 2]>> = facts
        .geometry
        .as_chunks::<2>()
        .0
        .iter()
        .map(|&[least, most]| {
            // The box round the turned cell, as play makes its collision
            // (`world::made_bodies`).
            let turned = [
                place(least[0], least[1]),
                place(most[0], least[1]),
                place(most[0], most[1]),
                place(least[0], most[1]),
            ];
            let (low, high) =
                turned
                    .iter()
                    .fold(([f32::MAX; 2], [f32::MIN; 2]), |(low, high), p| {
                        (
                            [low[0].min(p[0]), low[1].min(p[1])],
                            [high[0].max(p[0]), high[1].max(p[1])],
                        )
                    });
            vec![low, [high[0], low[1]], high, [low[0], high[1]]]
        })
        .collect();
    let cells: Vec<Vec<[f32; 3]>> = outlines
        .iter()
        .map(|outline| outline.iter().map(|p| [p[0], p[1], 0.0]).collect())
        .collect();
    let inside = |cell: usize, p: [f32; 2]| inside_outline(&cells[cell], p);
    faces_under_outlines(
        nav,
        &outlines,
        &inside,
        (at[2] - FLOOR_BELOW_M, at[2] + FLOOR_BELOW_M),
    )
}

/// The faces of `nav` with a corner within `low..=high` (metres) that any of
/// `outlines` (closed, seen from above, metres; `inside` says whether a
/// point is in the one numbered) overlaps.
fn faces_under_outlines(
    nav: &collision::nav::NavMesh,
    outlines: &[Vec<[f32; 2]>],
    inside: &dyn Fn(usize, [f32; 2]) -> bool,
    (low, high): (f32, f32),
) -> Vec<bool> {
    let edges = |points: &[[f32; 2]]| -> Vec<([f32; 2], [f32; 2])> {
        (0..points.len())
            .map(|k| (points[k], points[(k + 1) % points.len()]))
            .collect()
    };
    nav.faces
        .iter()
        .map(|face| {
            if !face.corners.iter().any(|c| c[2] >= low && c[2] <= high) {
                return false;
            }
            let corners = face.corners.map(|c| [c[0], c[1]]);
            outlines.iter().enumerate().any(|(cell, outline)| {
                corners.iter().any(|c| inside(cell, *c))
                    || outline.iter().any(|p| in_triangle(&corners, *p))
                    || edges(&corners).iter().any(|(a, b)| {
                        edges(outline)
                            .iter()
                            .any(|(c, d)| segments_cross(*a, *b, *c, *d))
                    })
            })
        })
        .collect()
}

/// Whether `p` lies in the triangle `corners`, seen from above.
fn in_triangle(corners: &[[f32; 2]; 3], p: [f32; 2]) -> bool {
    let side =
        |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let [a, b, c] = *corners;
    let sides = [side(a, b), side(b, c), side(c, a)];
    sides.iter().all(|s| *s >= 0.0) || sides.iter().all(|s| *s <= 0.0)
}

/// Whether the segments `a`–`b` and `c`–`d` cross, seen from above.
fn segments_cross(a: [f32; 2], b: [f32; 2], c: [f32; 2], d: [f32; 2]) -> bool {
    let turn = |p: [f32; 2], q: [f32; 2], r: [f32; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let (d1, d2) = (turn(c, d, a), turn(c, d, b));
    let (d3, d4) = (turn(a, b, c), turn(a, b, d));
    (d1 > 0.0) != (d2 > 0.0) && (d3 > 0.0) != (d4 > 0.0)
}

/// The points (metres, rings of `DOORWAY_STEP` out to `DOORWAY_REACH` round
/// `at`, nearest first) over faces of the mesh that are not door (or
/// placeable) `door`'s, the first over each face.
fn outside(nav: &collision::nav::NavMesh, at: Vec3, door: u32) -> Vec<Vec3> {
    let mut faces = std::collections::BTreeSet::new();
    (1..=(DOORWAY_REACH / DOORWAY_STEP) as u32)
        .flat_map(|ring| {
            let far = ring as f32 * DOORWAY_STEP;
            (0..16).map(move |k| {
                let (sin, cos) = math::sin_cos_deg(k as f32 * 22.5);
                [at[0] + cos * far, at[1] + sin * far, at[2]]
            })
        })
        .filter(|p| {
            face_under(nav, *p)
                .filter(|face| nav.faces[*face as usize].entity != Some(door))
                .is_some_and(|face| faces.insert(face))
        })
        .collect()
}

/// How far (metres) out of a doorway `outside` looks, and in what steps.
const DOORWAY_REACH: f32 = rules::walking::searches::DOORWAY_REACH;
const DOORWAY_STEP: f32 = rules::walking::searches::DOORWAY_STEP;

/// Where a character's origin (units) would be to stand at `to` on the
/// floor: `to` itself when it is over the mesh, else over the floor above
/// or below it nearest in height (within `GOAL_COLUMN_M`), as KOTOR puts a
/// scripted mover on its walkmesh by x and y (Dantooine's Council waypoints
/// lie under the enclave's floor: Bastila and Carth never walked to it).
pub fn on_floor(level: &Level, to: Vec3) -> Vec3 {
    let Some(nav) = level.nav() else {
        return to;
    };
    let goal = soles(to);
    if face_under(nav, goal).is_some() {
        return to;
    }
    match nav.height(goal, GOAL_COLUMN_M, GOAL_COLUMN_M) {
        Some(height) => [to[0], to[1], height / UNIT - motion::SOLE_Z],
        None => to,
    }
}

/// Where a character standing at `from` (its origin, units) off the
/// navigation mesh (on a ledge a blow or a jump put it on) steps back to:
/// the origin over the nearest face's middle within `BACK_ON_M`, none when
/// it is on the mesh.
pub fn back_on(level: &Level, from: Vec3) -> Option<Vec3> {
    if on_mesh(level, from) {
        return None;
    }
    let nav = level.nav()?;
    let [x, y, z] = nav.nearest(soles(from), BACK_ON_M, BACK_ON_M, BACK_ON_M)?;
    Some(math::scale([x, y, z], 1.0 / UNIT)).map(|[x, y, z]| [x, y, z - motion::SOLE_Z])
}

/// Where a character standing at `from` (its origin, units) on an island
/// of the navigation mesh its `goal` (an origin, units) is not on steps
/// down to: the origin over the middle of the face of the goal's island,
/// within `BACK_ON_M` of it, nearest the goal, as one lifted onto a raised
/// piece of floor no way leads from (a bed's top its steps let a body up
/// onto) steps off on the goal's side; none when both stand on one island,
/// or either is off the mesh.
pub fn down_to(level: &Level, from: Vec3, goal: Vec3) -> Option<Vec3> {
    let nav = level.nav()?;
    let (here, there) = (soles(from), soles(goal));
    let island = |point: Vec3| face_under(nav, point).map(|face| nav.faces[face as usize].island);
    let goal_island = island(there)?;
    if island(here)? == goal_island {
        return None;
    }
    let [x, y, z] = nav
        .faces
        .iter()
        .filter(|face| face.island == goal_island && !face.covered)
        .filter(|face| collision::nav::flat_length(face.centre, here) <= BACK_ON_M)
        .min_by(|a, b| {
            collision::nav::flat_length(a.centre, there)
                .total_cmp(&collision::nav::flat_length(b.centre, there))
        })?
        .centre;
    Some([x / UNIT, y / UNIT, z / UNIT - motion::SOLE_Z])
}

/// How far (metres) one off the navigation mesh looks for it (`back_on`).
const BACK_ON_M: f32 = rules::walking::searches::BACK_ON_M;

/// Whether a character standing at `from` (its origin, units) is on the
/// navigation mesh: off it, no route leads anywhere from where it stands,
/// and `back_on` finds none to step back to beyond `BACK_ON_M`. A level
/// with no mesh has nothing to be off.
pub fn on_mesh(level: &Level, from: Vec3) -> bool {
    level
        .nav()
        .is_none_or(|nav| face_under(nav, soles(from)).is_some())
}

/// What shuts in the faces a character standing at `from` (its origin,
/// units) reaches, through the objects `state` says: each face's centre
/// (metres) and the entity standing there (`None`: the level's furniture).
pub fn frontier(
    level: &Level,
    from: Vec3,
    state: &dyn Fn(u32) -> ObjectState,
) -> Vec<(Vec3, Option<u32>)> {
    level.nav().map_or(Vec::new(), |nav| {
        nav.frontier(soles(from), &|index| passable(level, index, state(index)))
    })
}

/// How far (units) a corner a body does not fit at is moved to one it
/// does, in rings of eight.
const CLEAR_RINGS: [f32; 4] = rules::walking::searches::CLEAR_RINGS;

/// `corner`, or, where a body standing there would be inside the level's
/// solid (a pillar or pipe drawn over walkable floor, which KOTOR's walker
/// passes through and move_body does not), the nearest spot within
/// `CLEAR_RINGS` where it fits and the mesh goes on; else as it is.
fn clear(level: &Level, corner: Vec3) -> Vec3 {
    if level.fits_level(corner) {
        return corner;
    }
    let on_mesh = |at: Vec3| {
        level
            .nav()
            .is_some_and(|nav| face_under(nav, soles(at)).is_some())
    };
    CLEAR_RINGS
        .iter()
        .flat_map(|&far| {
            (0..8).map(move |k| {
                let (sin, cos) = math::sin_cos_deg(45.0 * k as f32);
                [corner[0] + cos * far, corner[1] + sin * far, corner[2]]
            })
        })
        .find(|&at| level.fits_level(at) && on_mesh(at))
        .unwrap_or(corner)
}

/// Why `route` found no way (for reports): the start or the goal off the
/// mesh, on different islands (each face's, the mesh's count), or every
/// way shut (doors, exits).
pub fn why_none(level: &Level, from: Vec3, to: Vec3) -> String {
    let Some(nav) = level.nav() else {
        return "no navigation mesh".into();
    };
    let face = |at: Vec3| face_under(nav, soles(at));
    match (face(from), face(to)) {
        (None, _) => match back_on(level, from) {
            Some([x, y, z]) => format!(
                "the start is off the mesh (back on at ({:.2}, {:.2}, {:.2}))",
                x * UNIT,
                y * UNIT,
                z * UNIT
            ),
            None => format!("the start is off the mesh (no face within {BACK_ON_M} m)"),
        },
        (_, None) => "the goal is off the mesh".into(),
        (Some(a), Some(b)) => {
            let (a, b) = (nav.faces[a as usize].island, nav.faces[b as usize].island);
            if a == b {
                format!("island {a}: every way shut")
            } else {
                format!("the start on island {a}, the goal on {b}")
            }
        }
    }
}

/// The first corner of `corners` (from `route`) a character at `at` has
/// neither reached nor passed, if any; with `skip`, also one it is within
/// a body and a half of (a corner a wall keeps it from, or a body standing
/// on it: Anchorhead's commoner on the way to the gate). Past one is on the
/// way on from it already: within as near of the stretch to the next
/// corner, beyond the corner (a route from beside a corner may turn back
/// round it first). One it only stands beside, short of the way on, is
/// still to be gone round.
fn next_corner(corners: &[Vec3], at: Vec3, skip: bool, fresh: bool) -> Option<Vec3> {
    let near = if skip {
        rules::walking::route::SKIP_PASSES * PASS
    } else {
        REACHED
    };
    // A new walk (`fresh`, its clock not yet started, so planned as
    // stalled) never passes over its goal: it made for nothing and gave up
    // a scripted step of a metre or so at once (Dantooine's ancient droid
    // scene, the player's). A chase planned anew as its quarry moves still
    // passes over a goal it stands beside.
    let last = corners.len().saturating_sub(1);
    let done = |index: usize| {
        let corner = corners[index];
        let near = if fresh && index == last {
            REACHED
        } else {
            near
        };
        if math::flat_distance(corner, at) <= near {
            return true;
        }
        corners.get(index + 1).is_some_and(|next| {
            let way = math::sub(*next, corner);
            let from = math::sub(at, corner);
            let length = way[0] * way[0] + way[1] * way[1];
            let t = (way[0] * from[0] + way[1] * from[1]) / length.max(f32::EPSILON);
            t > 0.0 && math::flat_distance(from, math::scale(way, t.min(1.0))) <= near
        })
    };
    (0..corners.len())
        .find(|&index| !done(index))
        .map(|index| corners[index])
}

/// The command that runs `player` at `corner`, looking level at it.
pub fn toward(player: &MotionState, corner: Vec3) -> Command {
    let (_, yaw) = mind::aim(player.origin, corner);
    Command {
        forward: 127,
        ..mind::looking(player, 0.0, yaw)
    }
}

/// Whether `player` may move by its keys `forward`, `right` without
/// walking into a wall or off a drop: JA probes a
/// share of the keys' worth of units along the way and stops short of
/// anything within a share of it, or a drop of more than a few steps
/// (`rules::walking::route::PROBE`, `DROP_STEPS`); here the spot that far
/// along must hold a standing body and lie over the navigation mesh within
/// such a drop. Other bodies are not looked at [verify].
pub fn way_clear(level: &Level, player: &MotionState, forward: i8, right: i8) -> bool {
    if forward == 0 && right == 0 {
        return true;
    }
    let (ahead, side, _) = math::angle_vectors([0.0, player.view[1], 0.0]);
    let [share, looked] = rules::walking::route::PROBE;
    let along = |key: i8| f32::from(key) * share * looked;
    let at = math::mad(
        math::mad(player.origin, along(forward), ahead),
        along(right),
        side,
    );
    stands(level, at)
}

/// Whether a standing body fits at `at` (an origin, units) over the
/// navigation mesh within `rules::walking::route::DROP_STEPS` steps' drop.
pub fn stands(level: &Level, at: Vec3) -> bool {
    let drop = (rules::walking::route::DROP_STEPS * motion::STEP_HEIGHT + 1.0) * UNIT;
    level.fits_level(at)
        && level
            .nav()
            .is_none_or(|nav| nav.locate(soles(at), drop, 0.5).is_some())
}

/// Whether a character standing at `from` can walk straight to `to`
/// (origins, units; `Jedi_ClearPathToSpot`): a standing body fits, over the
/// navigation mesh within a drop (`stands`), at every `STRAIGHT_STEP` along
/// the line.
pub fn straight(level: &Level, from: Vec3, to: Vec3) -> bool {
    let way = math::sub(to, from);
    let length = (way[0] * way[0] + way[1] * way[1]).sqrt();
    let steps = (length / STRAIGHT_STEP).ceil().max(1.0) as u32;
    (1..=steps).all(|step| stands(level, math::mad(from, step as f32 / steps as f32, way)))
}

/// How far apart `straight` looks along its line (units).
const STRAIGHT_STEP: f32 = rules::walking::searches::STRAIGHT_STEP;

/// The keys (forward, right) that move a character looking along `yaw`
/// (degrees) toward `dir` (flat), each the share of a full key the way has
/// along it (`G_UcmdMoveForDir`).
pub fn keys_toward(yaw: f32, dir: Vec3) -> (i8, i8) {
    let (ahead, side, _) = math::angle_vectors([0.0, yaw, 0.0]);
    let mut dir = [dir[0], dir[1], 0.0];
    if math::normalize(&mut dir) <= f32::EPSILON {
        return (0, 0);
    }
    let key = |axis: Vec3| (math::dot(axis, dir) * 127.0).round().clamp(-127.0, 127.0) as i8;
    (key(ahead), key(side))
}

/// The keys that make `command` (a walker's: it looks where it goes) move
/// `player` the same way while it looks along `yaw` instead.
pub fn rebased(player: &MotionState, command: &Command, yaw: f32) -> (i8, i8) {
    let looks = math::short_to_degrees(command.angles[1].wrapping_add(player.view_offset[1]));
    let (ahead, side, _) = math::angle_vectors([0.0, looks, 0.0]);
    let way = math::mad(
        math::scale(ahead, f32::from(command.forward)),
        f32::from(command.right),
        side,
    );
    keys_toward(yaw, way)
}

/// How long a walker may go without getting closer to its waypoint
/// before it plans again (ms).
pub const STALL_MS: u64 = rules::walking::route::STALL_MS;

/// How long a stalled walker steps aside (ms).
pub const SIDESTEP_MS: u64 = rules::walking::route::SIDESTEP_MS;

/// How long a walker that found no way stands before it looks again (ms):
/// KOTOR's pathfinding wait, put before the move when `PlotPath` fails, of
/// 2 s and (rand() % 100) / 50 s more (TSL's
/// `CSWSCreature::AIActionMoveToPoint`, `AddPathfindingWaitActionToFront`;
/// its `HandlePathfindError` only logs, so it waits and looks on and on).
/// Its roll is the walk's own: the step's time and the goal.
fn pathfind_wait_ms(now_ms: u64, goal: Vec3) -> u64 {
    let stream = (u64::from(goal[0].to_bits()) << 32) | u64::from(goal[1].to_bits());
    let steps = crate::rng::SimRng::new(now_ms, stream).below(100);
    rules::walking::walker::PATHFIND_WAIT_MS
        + u64::from(steps) * rules::walking::walker::PATHFIND_WAIT_STEP_MS
}

/// How far apart two standing bodies' centres pass (units): a body's width
/// and a hand.
const PASS: f32 = 2.0 * motion::HALF_WIDTH + rules::walking::route::PASS_MARGIN;
/// How far beside a body (in `PASS`es) a walker goes round it: the nearer,
/// else the wider when a crowd takes both.
const SIDES: [f32; 2] = rules::walking::route::SIDES;

/// A walk under way: where to, the corner it is making for, the nearest
/// it has been to that corner and since when, how often it has stalled on
/// the way there and until when it steps aside, and the bodies KOTOR's
/// walker met on it (`kotor_step`). A walker keeps to its
/// waypoint until it reaches it (then plans the next from where it
/// stands) or stalls (then plans again, passing over a corner it cannot
/// get near, and every other time steps aside, to the right and the left
/// by turns, off what its box catches on; KOTOR's walkers never jump):
/// planning every step instead would let two near-equal ways take turns
/// and the walker dither between them. A body standing in its way it goes
/// round (`around`). Fixed-size, so a mind can keep it in a snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Walker {
    pub goal: Vec3,
    pub waypoint: Option<Vec3>,
    pub best: f32,
    pub since_ms: u64,
    pub stalls: u8,
    pub aside_until_ms: u64,
    /// Making for a body's side (`around`), not a corner of the route.
    pub rounding: bool,
    /// How many bodies KOTOR's walker has bumped into on this walk
    /// (`STUCK_STALLS` once it gives up), and the last: who, and where the
    /// walker stood (units, from above).
    pub bumps: u8,
    pub bumped: Option<(SimId, [f32; 2])>,
}

impl Walker {
    pub fn new(goal: Vec3) -> Walker {
        Walker {
            goal,
            waypoint: None,
            best: f32::MAX,
            since_ms: 0,
            stalls: 0,
            aside_until_ms: 0,
            rounding: false,
            bumps: 0,
            bumped: None,
        }
    }

    /// Whether `player` stands at the goal (within `REACHED` twice over).
    pub fn arrived(&self, player: &MotionState) -> bool {
        math::flat_distance(self.goal, player.origin) <= rules::walking::route::ARRIVED * REACHED
    }

    /// Whether a body at `at` (its origin) has reached the corner it makes
    /// for short of the goal, where the walk plans on.
    fn turns_at(&self, at: Vec3) -> bool {
        math::flat_distance(self.goal, at) > rules::walking::route::ARRIVED * REACHED
            && self
                .waypoint
                .is_some_and(|corner| math::flat_distance(corner, at) <= REACHED)
    }

    /// Whether the walk can come no nearer its goal: it stalled again and
    /// again, or KOTOR's walker gave it up (`kotor_step`).
    pub fn stuck(&self) -> bool {
        self.stalls >= STUCK_STALLS || self.bumps >= STUCK_STALLS
    }

    /// A new move on the walk: its bumps counted afresh, a walk given up on
    /// them taken up again.
    pub fn count_bumps_afresh(&mut self) {
        self.bumps = 0;
        self.bumped = None;
    }

    /// A new move on the walk: a walk of its own, its bumps and stalls
    /// counted afresh, so a move sent again to the spot a walk stalled out
    /// short of is walked, not given up at once (Taris's North Apartments,
    /// the raid's commander sent on and on to the same mark).
    pub fn begin_anew(&mut self) {
        self.count_bumps_afresh();
        self.stalls = 0;
        self.rounding = false;
    }

    /// Given up, as KOTOR's walker gives up a walk.
    pub fn give_up(&mut self) {
        self.bumps = STUCK_STALLS;
    }

    /// KOTOR's walker carried `player` on: it never steps aside, and its
    /// headway is kept as it gets nearer its corner (`command`'s own rule,
    /// taken before the stall is judged), so a walk going nowhere, whatever
    /// moves it, still stalls.
    pub fn keep_headway(&mut self, now_ms: u64, player: &MotionState) {
        self.aside_until_ms = 0;
        if let Some(corner) = self.waypoint {
            let distance = math::flat_distance(corner, player.origin);
            if distance < self.best - rules::walking::route::HEADWAY {
                self.best = distance;
                self.since_ms = now_ms;
            }
        }
    }

    /// This step's command for `player` at sim time `now_ms`, planning
    /// through the objects `state` says (`route`) when it must; standing
    /// still at the goal or when no way leads there.
    pub fn command(
        &mut self,
        level: &Level,
        player: &MotionState,
        now_ms: u64,
        ways: &dyn Ways,
    ) -> Command {
        self.command_round(
            level,
            player,
            now_ms,
            &|index| ways.door(index),
            &|face| ways.walls(face),
            true,
        )
    }

    /// `command` for a walk a player steers (the playthrough harness's), not
    /// KOTOR's mover: one that finds no way looks again the next step, with
    /// no pathfinding wait.
    pub fn command_eager(
        &mut self,
        level: &Level,
        player: &MotionState,
        now_ms: u64,
        ways: &dyn Ways,
    ) -> Command {
        self.command_round(
            level,
            player,
            now_ms,
            &|index| ways.door(index),
            &|face| ways.walls(face),
            false,
        )
    }

    /// `command`, planning off the faces `walled` names (`route_round`);
    /// with `waits`, a walk that finds no way waits KOTOR's pathfinding wait
    /// before it looks again (`pathfind_wait_ms`).
    pub fn command_round(
        &mut self,
        level: &Level,
        player: &MotionState,
        now_ms: u64,
        state: &dyn Fn(u32) -> ObjectState,
        walled: &dyn Fn(u32) -> bool,
        waits: bool,
    ) -> Command {
        let idle = mind::looking(player, player.view[0], player.view[1]);
        if self.arrived(player) {
            self.waypoint = None;
            return idle;
        }
        let flat = |corner: Vec3| math::flat_distance(corner, player.origin);
        let stalled = now_ms >= self.since_ms + STALL_MS;
        let reached = self.turns_at(player.origin);
        // A corner of its way reached is headway; the spot beside a body it
        // rounds is not (Freyyr, boxed in by the party, went round the player
        // and back into Jolee and round again, never stuck, his scene's walk
        // never given up).
        if reached && !self.rounding {
            self.stalls = 0;
        }
        if stalled && self.waypoint.is_some() {
            // At most 127: the top bit of its byte in a snapshot is `rounding`.
            self.stalls = self.stalls.saturating_add(1).min(127);
            if self.stalls % 2 == 1 {
                self.aside_until_ms = now_ms + SIDESTEP_MS;
            }
        }
        // No way found: it stands out KOTOR's pathfinding wait, then looks
        // again, its walk kept (`pathfind_wait_ms`), so a door a scene opens
        // a moment later is walked through (a_903kreia's d_entrance).
        let waiting = self.waypoint.is_none() && now_ms < self.since_ms;
        if !waiting && (self.waypoint.is_none() || reached || stalled) {
            self.plan(level, player.origin, state, walled, stalled);
            self.since_ms = match self.waypoint {
                None if waits => now_ms + pathfind_wait_ms(now_ms, self.goal),
                _ => now_ms,
            };
        }
        let Some(corner) = self.waypoint else {
            return idle;
        };
        let distance = flat(corner);
        if distance < self.best - rules::walking::route::HEADWAY {
            self.best = distance;
            self.since_ms = now_ms;
        }
        let mut command = toward(player, corner);
        if now_ms < self.aside_until_ms {
            command.right = if self.stalls % 4 == 1 { 127 } else { -127 };
        }
        command
    }

    /// The corner to make for next from `at` (`route_round` through the
    /// objects `state` says, off the faces `walled` names; with `skip`, past
    /// one it stands beside, `next_corner`), or back onto the mesh; its
    /// headway counted afresh.
    fn plan(
        &mut self,
        level: &Level,
        at: Vec3,
        state: &dyn Fn(u32) -> ObjectState,
        walled: &dyn Fn(u32) -> bool,
        skip: bool,
    ) {
        self.rounding = false;
        // Off the mesh where the mesh lies under it (a floor the bake left
        // out, beside a wall): the way is planned from there, as from where
        // it stands; stepping back on to its own spot plans it nowhere, and
        // the walk turned on the spot step after step (Taris's Twi'lek put
        // against the South Apartments' wall by the raid).
        let back = back_on(level, at);
        let under = back.filter(|on| math::flat_distance(*on, at) <= REACHED);
        self.waypoint = route_round(level, at, self.goal, state, walled)
            .or_else(|| route_round(level, under?, self.goal, state, walled))
            .and_then(|corners| next_corner(&corners, at, skip, self.since_ms == 0))
            .or(back.filter(|_| under.is_none()));
        self.best = f32::MAX;
    }

    /// After a stall, a body standing in the way to the corner (`bodies`:
    /// the others' origins, units) is gone round: the walker makes for the
    /// side of it a body fits on and the navigation mesh holds, the one
    /// nearer the corner, and plans on from there. Not one at the walk's
    /// goal (whom it follows or fights), nor again while it makes for a
    /// side of that body. The navigation mesh knows only the level; KOTOR's
    /// walkers step round a creature in their way.
    pub fn around(&mut self, level: &Level, player: &MotionState, bodies: &[Vec3]) {
        let Some(corner) = self.waypoint.filter(|_| self.stalls > 0) else {
            return;
        };
        let at = player.origin;
        let way = [corner[0] - at[0], corner[1] - at[1]];
        let length = (way[0] * way[0] + way[1] * way[1]).sqrt();
        if length < 1.0 {
            return;
        }
        let dir = [way[0] / length, way[1] / length];
        // The nearest body ahead, within a pass of the way.
        let blocking = bodies
            .iter()
            .filter_map(|body| {
                let rel = [body[0] - at[0], body[1] - at[1]];
                let ahead = rel[0] * dir[0] + rel[1] * dir[1];
                let side = rel[0] * dir[1] - rel[1] * dir[0];
                let at_goal = math::flat_distance(*body, self.goal) < PASS;
                (ahead > 0.0
                    && ahead < (length + PASS).min(rules::walking::route::AHEAD_PASSES * PASS)
                    && side.abs() < PASS
                    && !at_goal)
                    .then_some((ahead, *body))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        // Not while already making for a side of one.
        let Some((_, body)) = blocking.filter(|_| !self.rounding) else {
            return;
        };
        if let Some(beside) = beside(level, at, corner, body, bodies) {
            self.round(beside);
        }
    }

    /// Making for `beside`, a side of a body in its way, and planning on
    /// from there.
    fn round(&mut self, beside: Vec3) {
        self.waypoint = Some(beside);
        self.rounding = true;
        self.best = f32::MAX;
    }

    /// KOTOR's walker standing as `player` (its personal space `space`,
    /// metres) running into `met`, the others standing as `bodies` (`met`
    /// among them): one it may push it puts aside (`pushed_aside`) and walks
    /// on, giving where; any other it bumps into (`bump`).
    pub fn meet(
        &mut self,
        level: &Level,
        player: &MotionState,
        space: f32,
        met: &Met,
        bodies: &[Met],
    ) -> Option<Vec3> {
        let toward = self.heading();
        let pushed = met.push.and_then(|theirs| {
            let rest: Vec<Vec3> = bodies
                .iter()
                .filter(|other| other.who != met.who)
                .map(|other| other.at)
                .chain(std::iter::once(player.origin))
                .collect();
            pushed_aside(level, player.origin, toward, met.at, space + theirs, &rest)
        });
        if pushed.is_none() {
            let all: Vec<Vec3> = bodies.iter().map(|other| other.at).collect();
            self.bump(level, player, met.who, met.at, &all);
        }
        pushed
    }

    /// Where the walk heads past the bodies it meets: the corner it makes
    /// for, or its goal while it goes round a body or has no corner.
    pub fn heading(&self) -> Vec3 {
        self.waypoint
            .filter(|_| !self.rounding)
            .unwrap_or(self.goal)
    }

    /// A walk whose `command` moves it at `speed_mps` none (a body that
    /// cannot walk) is given up [verify: what KOTOR's walker does then].
    pub fn give_up_unable(&mut self, command: &Command, speed_mps: f32) {
        if (command.forward != 0 || command.right != 0) && speed_mps <= 0.0 {
            self.give_up();
        }
    }

    /// Whether a walker standing at `at` waits behind `met` rather than
    /// bumping into it: the body walks on (`Met::going`) away along this
    /// walk's own way, so the way clears [verify: KOTOR's walker behind a
    /// creature walking ahead of it was not read; counted as bumps, the
    /// second of two walked out of a door together gave its walk up behind
    /// the first and stood until its forced move's timeout jumped it there,
    /// Taris's Rodians leaving Javyar's Cantina].
    pub fn waits_behind(&self, at: Vec3, met: &Met) -> bool {
        let Some(going) = met.going else {
            return false;
        };
        let toward = self.heading();
        let mine = [toward[0] - at[0], toward[1] - at[1]];
        let theirs = [going[0] - met.at[0], going[1] - met.at[1]];
        mine[0] * theirs[0] + mine[1] * theirs[1] > 0.0
    }

    /// Whether `met` is one this walk does not bump into: the body at its
    /// goal (whom it follows or fights, `PASS` from it), or the one it is
    /// going round.
    pub fn passes(&self, met: &Met) -> bool {
        math::flat_distance(met.at, self.goal) < PASS
            || (self.rounding && self.bumped.is_some_and(|(was, _)| was == met.who))
    }

    /// A bump into `body` standing at `at` (its origin) by KOTOR's walker
    /// standing as `player`, the others standing at `bodies`: the walk goes
    /// round it (`beside`), or gives up on its 6th bump, on meeting the
    /// same body where it met it last, or with no side to go round by.
    fn bump(
        &mut self,
        level: &Level,
        player: &MotionState,
        body: SimId,
        at: Vec3,
        bodies: &[Vec3],
    ) {
        let spot = [player.origin[0], player.origin[1]];
        let again = self.bumped.is_some_and(|(was, there)| {
            was == body
                && math::flat_length([there[0] - spot[0], there[1] - spot[1], 0.0]) <= REACHED
        });
        self.bumps = self.bumps.saturating_add(1).min(STUCK_STALLS);
        self.bumped = Some((body, spot));
        let corner = self
            .waypoint
            .filter(|_| !self.rounding)
            .unwrap_or(self.goal);
        let side = (!again && self.bumps < STUCK_STALLS)
            .then(|| beside(level, player.origin, corner, at, bodies))
            .flatten();
        match side {
            Some(side) => self.round(side),
            None => self.give_up(),
        }
    }
}

/// The side of a body standing at `body` (its origin, units) in the way
/// of one at `at` making for `corner` that it goes round by: a spot a body
/// fits on (`Level::fits`, standing on the mesh's floor there as KOTOR's
/// walker holds it, `LIFT` over it) over the navigation mesh, clear of
/// every body at `bodies`, beside the body across the way, the nearer the
/// corner; the wider when a crowd takes both near ones. `None` when none
/// is. Tested at the walker's own height, a floor a few millimetres
/// higher beside it (Chuundar's wooden throne room) had put every side in
/// the floor, and the player stood pressed into Freyyr for good.
fn beside(level: &Level, at: Vec3, corner: Vec3, body: Vec3, bodies: &[Vec3]) -> Option<Vec3> {
    // Across the way to the corner, or to the body when it stands on it.
    let way = [corner[0] - at[0], corner[1] - at[1]];
    let way = if math::flat_length([way[0], way[1], 0.0]) < 1.0 {
        [body[0] - at[0], body[1] - at[1]]
    } else {
        way
    };
    let length = math::flat_length([way[0], way[1], 0.0]).max(f32::EPSILON);
    let across = [-way[1] / length, way[0] / length];
    let flat_to = |p: Vec3| ((p[0] - corner[0]).powi(2) + (p[1] - corner[1]).powi(2)).sqrt();
    // A side clear of every body (in a crowd, a listener to Gorton Colu's
    // speech beside the one ahead): the nearer, else one wider.
    let clear = |p: &Vec3| {
        bodies
            .iter()
            .all(|b| math::flat_length([b[0] - p[0], b[1] - p[1], 0.0]) >= PASS)
    };
    SIDES.into_iter().find_map(|wide| {
        [1.0, -1.0]
            .map(|side| {
                [
                    body[0] + across[0] * side * PASS * wide,
                    body[1] + across[1] * side * PASS * wide,
                    at[2],
                ]
            })
            .into_iter()
            .filter_map(|p| match level.nav() {
                None => Some(p),
                Some(nav) => {
                    nav.locate(soles(p), 0.5, 0.5)?;
                    let floor = nav.height(soles(p), FLOOR_BELOW_M, FLOOR_ABOVE_M)?;
                    Some([p[0], p[1], floor / UNIT + LIFT - motion::SOLE_Z])
                }
            })
            .filter(|p| clear(p) && level.fits(*p))
            .min_by(|a, b| flat_to(*a).total_cmp(&flat_to(*b)))
    })
}

/// A body a walker may run into: who, where it stands (its origin, units),
/// where KOTOR's walker pushes it aside rather than bumping into it (it is
/// idle, not hostile to the walker, and neither is a player), its personal
/// space (metres), and where a walk of its own not given up carries it
/// (its `Walker::heading`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Met {
    pub who: SimId,
    pub at: Vec3,
    pub push: Option<f32>,
    pub going: Option<Vec3>,
}

/// How far past the two bodies' personal spaces (metres) KOTOR puts a
/// creature a walker pushes aside, out from the walker's way.
const PUSH_MARGIN_M: f32 = rules::walking::walker::PUSH_MARGIN_M;

/// The rings (units) round the first spot where a pushed creature's place
/// is looked for when that one is taken, eight ways round each.
const PUSH_RINGS: [f32; 4] = rules::walking::walker::PUSH_RINGS;

/// Where KOTOR's walker standing at `at`, making for `toward`, puts a
/// creature standing at `body` (origins, units) it pushes aside: out from
/// the nearest point of its way, on the side the creature stands (the
/// right when on the line), `spaces` (the two personal spaces, metres) and
/// a margin from the way, never nearer than the bodies' widths; else the
/// first free spot round that one. A spot a body stands on over the
/// navigation mesh (`stands`), clear of the others at `bodies`, out of the
/// walker's way and walked to straight from where the creature stood, its
/// feet on the mesh. `None` when there is none: the walker bumps into it.
/// The creature is put there at once, as KOTOR's bump sets its position
/// rather than queueing it a move.
pub fn pushed_aside(
    level: &Level,
    at: Vec3,
    toward: Vec3,
    body: Vec3,
    spaces: f32,
    bodies: &[Vec3],
) -> Option<Vec3> {
    let way = [toward[0] - at[0], toward[1] - at[1]];
    let length = math::flat_length([way[0], way[1], 0.0]).max(f32::EPSILON);
    let dir = [way[0] / length, way[1] / length];
    let off_way = |p: Vec3| {
        let along = ((p[0] - at[0]) * dir[0] + (p[1] - at[1]) * dir[1]).clamp(0.0, length);
        [
            p[0] - (at[0] + dir[0] * along),
            p[1] - (at[1] + dir[1] * along),
        ]
    };
    let out = off_way(body);
    let off = math::flat_length([out[0], out[1], 0.0]);
    let out = if off < 1e-3 {
        [dir[1], -dir[0]]
    } else {
        [out[0] / off, out[1] / off]
    };
    let apart = ((spaces + PUSH_MARGIN_M) / UNIT).max(CONTACT);
    let near = [body[0] - out[0] * off, body[1] - out[1] * off];
    let first = [near[0] + out[0] * apart, near[1] + out[1] * apart, body[2]];
    let facing = math::atan2_deg(out[1], out[0]);
    let rings = PUSH_RINGS.into_iter().flat_map(|ring| {
        (0..8).map(move |k| {
            let (sin, cos) = math::sin_cos_deg(facing + k as f32 * 45.0);
            [first[0] + ring * cos, first[1] + ring * sin, first[2]]
        })
    });
    let nav = level.nav()?;
    std::iter::once(first)
        .chain(rings)
        .filter(|p| {
            let off = off_way(*p);
            math::flat_length([off[0], off[1], 0.0]) >= CONTACT
                && bodies.iter().all(|other| box_apart(*p, *other) >= CONTACT)
                && stands(level, *p)
                && straight(level, body, *p)
        })
        .find_map(|p| {
            let height = nav.height(soles(p), FLOOR_BELOW_M, FLOOR_ABOVE_M)?;
            Some([p[0], p[1], height / UNIT + LIFT - motion::SOLE_Z])
        })
}

/// What a step of KOTOR's walker did (`kotor_step`): where the body stands
/// after it (its origin, units), its velocity (units a second: the step
/// over the time, none once it stands), its yaw (degrees: the step's way),
/// its legs' animation (a walk, a run, or a stand when it arrives, meets a
/// body or a shut door), the body it bumped into, the shut door (the
/// level's entity) it stopped at, which a creature opens as one move_body walks
/// runs into a door, and the body it pushed aside and where to (`Met`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stepped {
    pub origin: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub legs: Anim,
    pub bumped: Option<SimId>,
    pub door: Option<u32>,
    pub pushed: Option<(SimId, Vec3)>,
}

/// How near (units, along either axis) two bodies' origins come before
/// their boxes touch: their widths.
const CONTACT: f32 = 2.0 * motion::HALF_WIDTH;

/// How far apart two bodies' boxes stand (units): their origins' farther
/// apart along x or y, as the boxes are square to the axes.
fn box_apart(a: Vec3, b: Vec3) -> f32 {
    (a[0] - b[0]).abs().max((a[1] - b[1]).abs())
}

/// How far over its floor (units) KOTOR's walker holds a body's soles, as
/// a body is put down standing, so move_body's ground trace finds it on the
/// floor rather than in it.
const LIFT: f32 = rules::walking::walker::LIFT;

/// One step of KOTOR's walker for a body standing as `player` on `route`,
/// at `speed_mps` (`kotor_speed`; 0 when its command does not walk) for
/// `msec`, running or walking, its personal space `space` (metres), the
/// other bodies standing as `bodies` and the objects as `state` says:
/// toward the walk's corner (its goal once it is there), never past it; its
/// feet on the navigation mesh (`NavMesh::height`, no collision traced);
/// its yaw snapped to the step, as KOTOR's mover has no turn rate. A body
/// it would walk into (not one it passes, `Walker::passes`) it pushes aside
/// and walks on, or it stops: a bump, which the walk goes round or gives up
/// on (`Walker::meet`). A shut door's
/// doorway (a door's face of the mesh, the door closed) stops it short of
/// the panel until the door opens, and gives the walk up where the door
/// will not open for it (`passable` by `state`, which shuts every closed
/// door to the held party). A walk with no route there (its walker
/// found none) stands until its walker looks again (KOTOR's pathfinding
/// wait, `pathfind_wait_ms`). `None` where the mesh has no floor ahead (a face too steep for
/// move_body is left out of it), and the caller steps it through move_body.
#[allow(clippy::too_many_arguments)]
pub fn kotor_step(
    level: &Level,
    player: &MotionState,
    route: &mut Walker,
    speed_mps: f32,
    msec: u32,
    run: bool,
    space: f32,
    bodies: &[Met],
    ways: &dyn Ways,
) -> Option<Stepped> {
    let at = player.origin;
    let stand = |bumped: Option<SimId>, door: Option<u32>| Stepped {
        origin: at,
        velocity: [0.0; 3],
        yaw: player.view[1],
        legs: Anim::BOTH_STAND1,
        bumped,
        door,
        pushed: None,
    };
    let arrived = route.arrived(player);
    // No way found (`Walker::command_round` looks again after KOTOR's
    // pathfinding wait): it stands, its walk kept.
    if route.waypoint.is_none() && !arrived {
        return Some(stand(None, None));
    }
    let target = match route.waypoint {
        Some(corner) if !arrived => corner,
        _ => route.goal,
    };
    let way = [target[0] - at[0], target[1] - at[1]];
    let far = math::flat_length([way[0], way[1], 0.0]);
    if route.stuck() || speed_mps <= 0.0 || far <= f32::EPSILON {
        return Some(stand(None, None));
    }
    let dir = [way[0] / far, way[1] / far];
    let reach = speed_mps / UNIT * msec as f32 / 1000.0;
    let advance = reach.min(far);
    let (x, y) = (at[0] + dir[0] * advance, at[1] + dir[1] * advance);
    let nav = level.nav()?;
    let height = nav.height(soles([x, y, at[2]]), FLOOR_BELOW_M, FLOOR_ABOVE_M)?;
    let origin = [x, y, height / UNIT + LIFT - motion::SOLE_Z];
    let blocking = bodies
        .iter()
        .filter(|met| {
            let near = box_apart(origin, met.at);
            near < CONTACT && near < box_apart(at, met.at) && !route.passes(met)
        })
        .min_by(|a, b| math::flat_distance(at, a.at).total_cmp(&math::flat_distance(at, b.at)));
    // A body standing on the goal (whom it follows or fights, or one on the
    // place it walks to) it is not walked into: one it may push it puts
    // aside, any other ends the walk touching it, as near as it comes, as
    // KOTOR's mover gives up short of where it cannot reach.
    let on_goal = || {
        bodies.iter().find(|met| {
            let near = box_apart(origin, met.at);
            near < CONTACT
                && near < box_apart(at, met.at)
                && math::flat_distance(met.at, route.goal) < PASS
        })
    };
    let pushed = match blocking {
        Some(met) if route.waits_behind(at, met) => return Some(stand(None, None)),
        Some(met) => match route.meet(level, player, space, met, bodies) {
            Some(spot) => Some((met.who, spot)),
            None => return Some(stand(Some(met.who), None)),
        },
        None => match on_goal() {
            Some(met) if met.push.is_some() => {
                match route.meet(level, player, space, met, bodies) {
                    Some(spot) => Some((met.who, spot)),
                    None => return Some(stand(Some(met.who), None)),
                }
            }
            Some(_) => {
                route.give_up();
                return Some(stand(None, None));
            }
            None => None,
        },
    };
    // A door shut across the step: a door's face of the mesh (where a body
    // would be inside its closed panels) that it is not on already, the
    // door closed.
    let door_of =
        |p: Vec3| face_under(nav, soles(p)).and_then(|face| nav.faces[face as usize].entity);
    let shut = door_of(origin)
        .filter(|index| door_of(at) != Some(*index) && ways.door(*index).state == 0)
        .filter(|index| {
            level
                .placed()
                .get(*index as usize)
                .is_some_and(|object| object.kind == ObjectKind::Door)
        });
    if let Some(door) = shut {
        // One it may not open (locked, scenery, or the held party's: `state`
        // shuts it, `opening_none`) is no way: the walk is given up.
        if !passable(level, door, ways.door(door)) {
            route.give_up();
        }
        return Some(stand(None, shut));
    }
    // Arrived: it stands on the goal, still. At a corner short of it, the
    // walk plans on from there at once, as `Walker::command` would next step
    // (a corner reached is headway), so a client predicting it, which runs
    // no minds, goes on round the route as the authority does.
    let arrived = advance >= far && target == route.goal;
    if route.turns_at(origin) {
        if !route.rounding {
            route.stalls = 0;
        }
        route.plan(
            level,
            origin,
            &|index| ways.door(index),
            &|face| ways.walls(face),
            false,
        );
    }
    let seconds = msec as f32 / 1000.0;
    Some(Stepped {
        origin,
        velocity: if arrived {
            [0.0; 3]
        } else {
            math::scale(math::sub(origin, at), 1.0 / seconds)
        },
        yaw: math::atan2_deg(dir[1], dir[0]),
        legs: match (arrived, run) {
            (true, _) => Anim::BOTH_STAND1,
            (false, true) => Anim::BOTH_RUN1,
            (false, false) => Anim::BOTH_WALK1,
        },
        bumped: None,
        door: None,
        pushed,
    })
}

/// The objects as `state` says them for a mover that opens no door (a
/// party member while the world is held): a closed door shut to it, as
/// locked (`passable`), so its route goes round, or nowhere.
pub fn opening_none(state: ObjectState) -> ObjectState {
    if state.state == 0 {
        ObjectState {
            lock: crate::snapshot::lock::LOCKED,
            ..state
        }
    } else {
        state
    }
}

/// A body's walk and run rates (m/s) on KOTOR's walker: a player's the
/// table's `PLAYER` row (it has no template), a creature's its own row
/// (`module::Body`); one read as nothing (a row the table lacks) that is
/// not `NOMOVE` the `DEFAULT` row's, as no creature is left frozen by
/// missing data. `None` where none is known: the body steps through move_body.
pub fn rates(
    body: Option<&crate::module::Body>,
    player: bool,
    table: MoveRates,
) -> Option<(f32, f32)> {
    match body {
        _ if player => table.player,
        Some(body) if body.walk_mps > 0.0 || body.run_mps > 0.0 || body.immobile() => {
            Some((body.walk_mps, body.run_mps))
        }
        _ => table.default,
    }
}

/// KOTOR's speed (m/s) for `rates` (`rates`) at `multiplier`, the body's
/// speed as its effects and powers set it (clamped, `rules::walking::pace`),
/// running or walking: too slow a walk is none, a run has a floor. An
/// immobile body (both rates none) moves not at all [verify: the exe marks
/// rate 1 apart, and what a walk of one does was not read].
pub fn kotor_speed((walk, run): (f32, f32), multiplier: f32, running: bool) -> f32 {
    let multiplier = multiplier.clamp(pace::LEAST_MULTIPLIER, pace::MOST_MULTIPLIER);
    match (walk, run) {
        (0.0, 0.0) => 0.0,
        _ if running => (run * multiplier).max(pace::LEAST_RUN),
        _ => Some(walk * multiplier)
            .filter(|walk| *walk >= pace::LEAST_WALK)
            .unwrap_or(0.0),
    }
}

/// How long (ms) a body leaving a fight takes to slow from JA's pace to
/// KOTOR's (`paced`).
pub const PACE_EASE_MS: u64 = rules::walking::pace::EASE_MS;

/// The base speed (units/s) the step of the body standing as `ps` moves
/// at so move_body's speed for `command`'s keys (base speed times the longest
/// key over 127) is KOTOR's pace (`speed_mps`, `kotor_speed`), or `keep`
/// (0 to 1) of the way from it back to JA's for those keys (a body easing
/// out of a fight). Faster than JA's run where KOTOR's is; the keys and
/// their way are kept.
pub fn paced(command: &Command, ps: &MotionState, speed_mps: f32, keep: f32) -> f32 {
    let longest = command
        .forward
        .unsigned_abs()
        .max(command.right.unsigned_abs());
    if longest == 0 {
        return ps.run_speed;
    }
    let asked = ps.run_speed * f32::from(longest) / 127.0;
    let speed = speed_mps / UNIT;
    let allowed = speed + (asked - speed) * keep.clamp(0.0, 1.0);
    allowed * 127.0 / f32::from(longest)
}

/// How much of JA's pace a body that left its fight `since_ms` ago keeps
/// (`paced`): all of it at once, none `PACE_EASE_MS` after, falling evenly.
pub fn pace_kept(since_ms: u64) -> f32 {
    1.0 - since_ms.min(PACE_EASE_MS) as f32 / PACE_EASE_MS as f32
}

impl Walker {
    pub(crate) const BLANK: Walker = Walker {
        goal: [0.0; 3],
        waypoint: None,
        best: 0.0,
        since_ms: 0,
        stalls: 0,
        aside_until_ms: 0,
        rounding: false,
        bumps: 0,
        bumped: None,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        for value in self.goal {
            out.f32(value);
        }
        write_fixed(out, self.waypoint.as_ref(), &[0.0; 3], |p, out| {
            for value in p {
                out.f32(*value);
            }
        });
        out.f32(self.best);
        out.u64(self.since_ms);
        // Whether it is rounding a body in the stalls' top bit.
        out.u8(self.stalls.min(127) | u8::from(self.rounding) << 7);
        out.u64(self.aside_until_ms);
        out.u8(self.bumps);
        write_fixed(
            out,
            self.bumped.as_ref(),
            &(SimId(0), [0.0; 2]),
            |(who, at), out| {
                out.u32(who.0);
                out.f32(at[0]);
                out.f32(at[1]);
            },
        );
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Walker, DecodeError> {
        let goal = [input.f32()?, input.f32()?, input.f32()?];
        let waypoint = read_fixed(
            input,
            &[0.0; 3],
            |input| Ok([input.f32()?, input.f32()?, input.f32()?]),
            "waypoint flag",
        )?;
        let (best, since_ms, stalls) = (input.f32()?, input.u64()?, input.u8()?);
        let aside_until_ms = input.u64()?;
        let bumps = input.u8()?;
        let bumped = read_fixed(
            input,
            &(SimId(0), [0.0; 2]),
            |input| Ok((SimId(input.u32()?), [input.f32()?, input.f32()?])),
            "bumped flag",
        )?;
        Ok(Walker {
            goal,
            waypoint,
            best,
            since_ms,
            stalls: stalls & 127,
            aside_until_ms,
            rounding: stalls & 128 != 0,
            bumps,
            bumped,
        })
    }
}
