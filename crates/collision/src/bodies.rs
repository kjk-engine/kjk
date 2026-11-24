//! The actors' bodies over a level, traced together: what a player's move,
//! a bolt's flight and a blade's sweep run into. As JA's server trace does
//! (`SV_Trace`): the world first, then every body the swept box reaches, in
//! the list's order; the nearest hit wins, a tie keeps what was found
//! first (the level's), a body the trace starts in marks it `start_solid`,
//! and one it never leaves decides it. A body is an axis-aligned box,
//! clipped by the brush clip every world shares (`docs/TRACE-SPEC.md`);
//! a body moving round across (`Shape::Upright`) meets it round across as
//! well (`brush::clip_round`), as KOTOR's creatures meet by their personal
//! space.
//!
//! The list is the caller's, rebuilt each step from the sim's actors in id
//! order, so the result depends on nothing but the state.

use crate::brush::{self, Clip, Swept};
use crate::{CollisionWorld, Contents, Extents, Level, Shape, Surface, Trace};

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

    /// `trace`, as a shot or a Force power's line is traced: one that starts
    /// inside a body and leaves it meets that body where it starts, as JA's
    /// missiles take the entity they start in as the one they hit
    /// (`G_RunMissile`). A body met by its model's box (a wraid's, a
    /// rancor's) is wider than the room it keeps round it, so one standing
    /// close to it can have its eyes or its gun inside that box.
    pub fn shot(&self, start: [f32; 3], end: [f32; 3], extents: &Extents, mask: Contents) -> Trace {
        let met = self.trace(start, end, extents, mask);
        if !met.start_solid || met.entity.is_some() {
            return met;
        }
        let inside = self.trace(start, start, extents, mask);
        if inside.entity.is_some() { inside } else { met }
    }

    fn solid(&self, body: &Body, mask: Contents) -> bool {
        body.contents.intersects(mask) && !self.skip.contains(&Some(body.entity))
    }
}

impl Bodies<'_> {
    /// The bodies `extents` meets swept from `start` to `end`, after the
    /// world's `trace`: as a box, a box square to the axes; `round`, an
    /// upright cylinder as wide as the box across, met by one.
    fn bodies_after(
        &self,
        mut trace: Trace,
        start: [f32; 3],
        end: [f32; 3],
        extents: &Extents,
        mask: Contents,
        round: bool,
    ) -> Trace {
        if trace.all_solid {
            return trace;
        }
        let swept = match round {
            true => Swept::upright(extents),
            false => Swept::Box {
                mins: extents.mins,
                maxs: extents.maxs,
            },
        };
        let reach = (extents.mins, extents.maxs);
        let mut moved = false;
        for body in self.bodies.iter().filter(|body| self.solid(body, mask)) {
            if !crate::reaches(start, end, reach, &body.min, &body.max) {
                continue;
            }
            let clip = match round {
                true => brush::clip_round(
                    [
                        (body.min[0] + body.max[0]) * 0.5,
                        (body.min[1] + body.max[1]) * 0.5,
                    ],
                    (body.max[0] - body.min[0]) * 0.5,
                    (body.min[2], body.max[2]),
                    start,
                    end,
                    &swept,
                ),
                false => brush::clip(
                    &crate::brushes::axial(body.min, body.max),
                    start,
                    end,
                    &swept,
                ),
            };
            match clip {
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
}

impl CollisionWorld for Bodies<'_> {
    fn trace(&self, start: [f32; 3], end: [f32; 3], extents: &Extents, mask: Contents) -> Trace {
        let trace = self.world.trace(start, end, extents, mask);
        self.bodies_after(trace, start, end, extents, mask, false)
    }

    fn sweep(&self, start: [f32; 3], end: [f32; 3], shape: &Shape, mask: Contents) -> Trace {
        match shape {
            Shape::Box(extents) => self.trace(start, end, extents, mask),
            Shape::Upright(extents) => {
                let trace = self.world.sweep(start, end, shape, mask);
                self.bodies_after(trace, start, end, extents, mask, true)
            }
            Shape::Capsule(capsule) => self.trace(start, end, &capsule.bounds(), mask),
        }
    }

    fn fits(&self, at: [f32; 3], shape: &Shape, mask: Contents) -> bool {
        let (extents, round) = match shape {
            Shape::Box(extents) => (extents, false),
            Shape::Upright(extents) => (extents, true),
            Shape::Capsule(_) => return !self.sweep(at, at, shape, mask).all_solid,
        };
        self.world.fits(at, shape, mask)
            && !self
                .bodies_after(Trace::clear(at), at, at, extents, mask, round)
                .all_solid
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

/// A world traced with some contents left out of every mask, and others
/// put in their place. A body walking a planned route traces with
/// `Contents::SHAPE` swapped for `Contents::FOOTPRINT_CLIP`
/// (`Contents::ROUTED_SOLID`). It passes through placeables' drawn shapes
/// and is stopped by the walls round their footprints.
pub struct Masked<'a> {
    world: &'a dyn CollisionWorld,
    off: Contents,
    on: Contents,
}

impl<'a> Masked<'a> {
    pub fn new(world: &'a dyn CollisionWorld, off: Contents) -> Masked<'a> {
        Masked {
            world,
            off,
            on: Contents::NONE,
        }
    }

    /// A world in which every mask that holds `off` holds `on` instead.
    pub fn swapping(world: &'a dyn CollisionWorld, off: Contents, on: Contents) -> Masked<'a> {
        Masked { world, off, on }
    }

    /// The world a body walking a planned route traces
    /// (`Contents::ROUTED_SOLID`).
    pub fn routed(world: &'a dyn CollisionWorld) -> Masked<'a> {
        Masked::swapping(world, Contents::SHAPE, Contents::FOOTPRINT_CLIP)
    }

    fn mask(&self, mask: Contents) -> Contents {
        let on = if mask.intersects(self.off) {
            self.on
        } else {
            Contents::NONE
        };
        Contents((mask.0 & !self.off.0) | on.0)
    }
}

impl CollisionWorld for Masked<'_> {
    fn trace(&self, start: [f32; 3], end: [f32; 3], extents: &Extents, mask: Contents) -> Trace {
        self.world.trace(start, end, extents, self.mask(mask))
    }

    fn sweep(&self, start: [f32; 3], end: [f32; 3], shape: &Shape, mask: Contents) -> Trace {
        self.world.sweep(start, end, shape, self.mask(mask))
    }

    fn fits(&self, at: [f32; 3], shape: &Shape, mask: Contents) -> bool {
        self.world.fits(at, shape, self.mask(mask))
    }

    fn point_contents(&self, point: [f32; 3], mask: Contents) -> Contents {
        self.world.point_contents(point, self.mask(mask))
    }

    fn level(&self) -> Option<&Level> {
        self.world.level()
    }
}
