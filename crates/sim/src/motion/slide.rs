//! The slide: the box swept through the step's time, turned along every
//! surface it meets, and the climbing slide that tries the same sweep a
//! step higher when a low ledge blocks it.
//!
//! Two velocities travel through a sweep: the motion, which the box moves
//! by (with gravity, the average over the step), and the kept velocity,
//! which it ends the step with. Both bend along the same planes.

use collision::Contents;

use super::{Hit, Move, STEP_HEIGHT, WALKABLE_NORMAL_Z, events};
use crate::math::{Vec3, add, cross, dot, mad, normalize, scale, sub};

/// A velocity slid along a plane is pushed this much past it (or held this
/// much short when it already leaves it), so the next trace does not graze
/// the plane.
const PUSH_OFF: f32 = 1.001;
/// How far into a plane a motion may point and still count as leaving it.
const CLEAR: f32 = 0.1;
/// The most planes one sweep turns along, and the most traces it makes.
const PLANE_LIMIT: usize = 5;
const TRACE_LIMIT: usize = 4;

/// How far (units) a refused step looks on for the floor beyond a low
/// step's bevel (`Move::stride`), and how high a step it may be (a raised
/// floor's, not a ledge's or a console's base).
const STRIDE: f32 = rules::movement::fine::STRIDE;
const STRIDE_RISE: f32 = rules::movement::fine::STRIDE_RISE;

/// The motion and kept velocity of one sweep.
#[derive(Clone, Copy)]
struct Pair {
    motion: Vec3,
    kept: Vec3,
}

impl Move<'_, '_> {
    /// `v` with its part into the plane of `normal` taken out (and a hair
    /// more); on the ground, a wall leaves `v`'s climb or fall alone.
    pub(super) fn along_plane(&self, v: Vec3, normal: Vec3) -> Vec3 {
        let into = dot(v, normal);
        let push = if into < 0.0 {
            into * PUSH_OFF
        } else {
            into / PUSH_OFF
        };
        let mut out: Vec3 = std::array::from_fn(|i| v[i] - normal[i] * push);
        if self.ps.ground.is_some() && normal[2] < WALKABLE_NORMAL_Z {
            out[2] = v[2];
        }
        out
    }

    /// Both velocities slid along `normal`.
    fn slide_pair(&self, pair: Pair, normal: Vec3) -> Pair {
        Pair {
            motion: self.along_plane(pair.motion, normal),
            kept: self.along_plane(pair.kept, normal),
        }
    }

    /// Turns `pair` along the `planes` it heads into; `None` when a corner
    /// of three planes stops it.
    fn bend(&self, planes: &[Vec3], pair: Pair) -> Option<Pair> {
        let goes_into = |v: Vec3, plane: Vec3| dot(v, plane) < CLEAR;
        let Some(first) = planes.iter().position(|&p| goes_into(pair.motion, p)) else {
            return Some(pair);
        };
        let first_plane = planes[first];
        let mut bent = self.slide_pair(pair, first_plane);
        for (second, &plane) in planes.iter().enumerate() {
            if second == first || !goes_into(bent.motion, plane) {
                continue;
            }
            bent = self.slide_pair(bent, plane);
            if dot(bent.motion, first_plane) >= 0.0 {
                continue;
            }
            // Pressed into both: along the crease where they meet.
            let mut crease = cross(first_plane, plane);
            normalize(&mut crease);
            bent = Pair {
                motion: scale(crease, dot(crease, pair.motion)),
                kept: scale(crease, dot(crease, pair.kept)),
            };
            let cornered = planes
                .iter()
                .enumerate()
                .any(|(third, &p)| third != first && third != second && goes_into(bent.motion, p));
            if cornered {
                return None;
            }
        }
        Some(bent)
    }

    /// One sweep of the step's time, with or without gravity; true when
    /// something blocked it.
    fn slide(&mut self, gravity: bool) -> bool {
        let velocity = self.ps.velocity;
        let mut pair = Pair {
            motion: velocity,
            kept: velocity,
        };
        let mut landing_hold = velocity;
        if gravity {
            pair.kept[2] = velocity[2] - self.ps.gravity * self.seconds;
            pair.motion[2] = (velocity[2] + pair.kept[2]) * 0.5;
            landing_hold[2] = pair.kept[2];
            if let Some(plane) = self.ground_plane {
                pair.motion = self.along_plane(pair.motion, plane);
            }
        }

        let mut planes: Vec<Vec3> = Vec::with_capacity(PLANE_LIMIT);
        if let Some(plane) = self.ground_plane {
            planes.push(plane);
        }
        let mut heading = pair.motion;
        normalize(&mut heading);
        planes.push(heading);

        let mut remaining = self.seconds;
        let mut blocked = false;
        for _ in 0..TRACE_LIMIT {
            let origin = self.ps.origin;
            let hit = self.trace(
                origin,
                mad(origin, remaining, pair.motion),
                self.mins,
                self.maxs,
            );
            if hit.all_solid {
                self.ps.velocity = [pair.motion[0], pair.motion[1], 0.0];
                return true;
            }
            if hit.fraction > 0.0 {
                self.ps.origin = hit.end;
            }
            if hit.fraction == 1.0 {
                break;
            }
            self.touch(hit.ground);
            blocked = true;
            remaining -= remaining * hit.fraction;
            if planes.len() >= PLANE_LIMIT {
                self.ps.velocity = [0.0; 3];
                return true;
            }
            if planes.contains(&hit.normal) {
                // The same plane again: nudge off it.
                pair.motion = add(hit.normal, pair.motion);
                continue;
            }
            planes.push(hit.normal);
            match self.bend(&planes, pair) {
                Some(bent) => pair = bent,
                None => {
                    self.ps.velocity = [0.0; 3];
                    return true;
                }
            }
        }

        self.ps.velocity = if self.ps.hold_ms != 0 {
            landing_hold
        } else if gravity {
            pair.kept
        } else {
            pair.motion
        };
        blocked
    }

    /// A sweep that, blocked, is tried again lifted by a step and set back
    /// down: how the box climbs stairs and low ledges.
    pub(super) fn climbing_slide(&mut self, gravity: bool) {
        let start = self.ps.origin;
        let start_velocity = self.ps.velocity;
        if !self.slide(gravity) {
            return;
        }

        let below = [start[0], start[1], start[2] - STEP_HEIGHT];
        if self.ps.velocity[2] > 0.0 {
            let floor = self.trace(start, below, self.mins, self.maxs);
            if floor.fraction == 1.0 || floor.normal[2] < WALKABLE_NORMAL_Z {
                // Rising with no floor under the start: a blocked jump.
                return;
            }
        }

        let plain = (self.ps.origin, self.ps.velocity);
        let above = [start[0], start[1], start[2] + STEP_HEIGHT];
        let lift = self.trace(start, above, self.mins, self.maxs);
        if lift.all_solid {
            return;
        }
        let rise = lift.end[2] - start[2];
        self.ps.origin = lift.end;
        self.ps.velocity = start_velocity;
        self.slide(gravity);

        if !self.set_down(rise, plain.0) {
            (self.ps.origin, self.ps.velocity) = plain;
            self.stride(lift.end, start, start_velocity);
        }
        if self.ps.origin[2] - start[2] > 2.0 {
            self.ps.events |= events::STEP;
        }
    }

    /// Lowers the lifted box by `rise` onto what it stepped up to; false
    /// when there is nothing fit to stand on (a non-walkable top, or a climb
    /// too steep from where the plain sweep, at `plain_origin`, stopped).
    fn set_down(&mut self, rise: f32, plain_origin: Vec3) -> bool {
        let o = self.ps.origin;
        let hit: Hit = self.trace(o, [o[0], o[1], o[2] - rise], self.mins, self.maxs);
        if hit.all_solid {
            return false;
        }
        let short = hit.fraction < 1.0;
        if short && hit.contents.intersects(Contents::NONWALK) && self.middle_on_nonwalk(hit.end) {
            return false;
        }
        if hit.normal[2] < WALKABLE_NORMAL_Z {
            let mut climb = sub(hit.end, plain_origin);
            normalize(&mut climb);
            if climb[2] > 1.0 - WALKABLE_NORMAL_Z {
                return false;
            }
        }
        self.ps.origin = hit.end;
        if short {
            self.ps.velocity = self.along_plane(self.ps.velocity, hit.normal);
        }
        true
    }

    /// A step refused as too steep a climb that a frame's short move only
    /// made look so (JA judges the climb from where the plain slide stopped
    /// to the raised move's landing, a couple of units on at 8 ms: a low
    /// step's bevel reads as a wall): from `top` (a step over `start`), on
    /// `STRIDE` units along the move and down onto walkable floor no more
    /// than `STRIDE_RISE` over `start`, taken if it lands so (Hrakert station's
    /// raised floors, an 8-unit step with a 54-degree bevel).
    fn stride(&mut self, top: Vec3, start: Vec3, velocity: Vec3) {
        let mut way = [velocity[0], velocity[1], 0.0];
        if dot(way, way) < 1.0 {
            return;
        }
        normalize(&mut way);
        let ahead = mad(top, STRIDE, way);
        let across = self.trace(top, ahead, self.mins, self.maxs);
        if across.all_solid || across.fraction < 1.0 {
            return;
        }
        let mut below = ahead;
        below[2] = start[2] - 1.0;
        let down = self.trace(ahead, below, self.mins, self.maxs);
        // The level's own floor, under the middle too: never a placeable's
        // top (a large locker on Hrakert station, stepped onto off the
        // walkmesh), as KOTOR's walker stands only on its walkmesh.
        let walkable = down.fraction < 1.0
            && !down.all_solid
            && down.ground == super::Ground::World
            && down.normal[2] >= WALKABLE_NORMAL_Z
            && !down.contents.intersects(Contents::NONWALK)
            && !self.middle_on_nonwalk(down.end)
            && down.end[2] > start[2] + 1.0
            && down.end[2] <= start[2] + STRIDE_RISE;
        if walkable {
            self.ps.origin = down.end;
            self.ps.velocity = self.along_plane(velocity, down.normal);
        }
    }

    /// Whether the ground under the middle of a box standing at `at` (within
    /// a step down from its soles) is `NONWALK`.
    fn middle_on_nonwalk(&self, at: Vec3) -> bool {
        let soles = [at[0], at[1], at[2] + self.mins[2] + 1.0];
        let mut below = soles;
        below[2] -= STEP_HEIGHT;
        let hit = self.trace(soles, below, [0.0; 3], [0.0; 3]);
        hit.fraction < 1.0 && hit.contents.intersects(Contents::NONWALK)
    }
}
