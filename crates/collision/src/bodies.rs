//! The actors' bodies over a level, traced together: what a player's move,
//! a bolt's flight and a blade's sweep run into. As JA's server trace does
//! (`SV_Trace`): the world first, then every body the swept box reaches, in
//! the list's order; the nearest hit wins, a tie keeps what was found
//! first (the level's), a body the trace starts in marks it `start_solid`,
//! and one it never leaves decides it. A body is an axis-aligned box,
//! clipped by the brush clip every world shares (`docs/TRACE-SPEC.md`).
//!
//! The list is the caller's, rebuilt each step from the sim's actors in id
//! order, so the result depends on nothing but the state.

use crate::brush::{self, Clip, Swept};
use crate::{CollisionWorld, Contents, Extents, Level, Surface, Trace};

/// One actor's box in the world (metres).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    /// The actor it belongs to (the sim's id).
    pub entity: u32,
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// `Contents::BODY` for a living actor; what else a body is made of
    /// (a corpse that bolts pass through: none) is the caller's.
    pub contents: Contents,
}

/// A world with bodies in it.
pub struct Bodies<'a> {
    world: &'a dyn CollisionWorld,
    bodies: &'a [Body],
    /// Bodies a trace passes through: the tracer's own, a bolt's owner.
    skip: [Option<u32>; 2],
}

impl<'a> Bodies<'a> {
    pub fn new(world: &'a dyn CollisionWorld, bodies: &'a [Body]) -> Bodies<'a> {
        Bodies {
            world,
            bodies,
            skip: [None; 2],
        }
    }

    /// The same, passing through `entity`'s body (at most two are skipped;
    /// a third call replaces the second).
    pub fn skipping(self, entity: u32) -> Bodies<'a> {
        let skip = match self.skip {
            [None, _] => [Some(entity), None],
            [first, _] => [first, Some(entity)],
        };
        Bodies { skip, ..self }
    }

    fn solid(&self, body: &Body, mask: Contents) -> bool {
        body.contents.intersects(mask) && !self.skip.contains(&Some(body.entity))
    }
}

impl CollisionWorld for Bodies<'_> {
    fn trace(&self, start: [f32; 3], end: [f32; 3], extents: &Extents, mask: Contents) -> Trace {
        let mut trace = self.world.trace(start, end, extents, mask);
        if trace.all_solid {
            return trace;
        }
        let swept = Swept::Box {
            mins: extents.mins,
            maxs: extents.maxs,
        };
        let reach = (extents.mins, extents.maxs);
        let mut moved = false;
        for body in self.bodies.iter().filter(|body| self.solid(body, mask)) {
            if !crate::reaches(start, end, reach, &body.min, &body.max) {
                continue;
            }
            let planes = crate::brushes::axial(body.min, body.max);
            match brush::clip(&planes, start, end, &swept) {
                Clip::Miss => {}
                Clip::StartSolid => trace.start_solid = true,
                Clip::AllSolid => {
                    trace.start_solid = true;
                    trace.all_solid = true;
                    trace.fraction = 0.0;
                    trace.end = start;
                    trace.contents = body.contents;
                    trace.surface = Surface::default();
                    trace.entity = Some(body.entity);
                    return trace;
                }
                Clip::Hit { fraction, plane } => {
                    if fraction < trace.fraction {
                        trace.fraction = fraction;
                        trace.plane = plane;
                        trace.contents = body.contents;
                        trace.surface = Surface::default();
                        trace.entity = Some(body.entity);
                        moved = true;
                    }
                }
            }
        }
        if moved {
            trace.end = std::array::from_fn(|i| start[i] + trace.fraction * (end[i] - start[i]));
        }
        trace
    }

    fn point_contents(&self, point: [f32; 3], mask: Contents) -> Contents {
        self.bodies
            .iter()
            .filter(|body| self.solid(body, mask))
            .filter(|body| (0..3).all(|i| point[i] >= body.min[i] && point[i] <= body.max[i]))
            .fold(self.world.point_contents(point, mask), |all, body| {
                all | body.contents
            })
    }

    fn level(&self) -> Option<&Level> {
        self.world.level()
    }
}
