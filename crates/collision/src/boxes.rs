//! A collision world made of axis-aligned boxes: flat floors, walls, steps
//! and ledges to trace against before the level collision exists, and for
//! probes. Each box is a brush of its six axial planes, clipped by the
//! brush clip every world shares (`brush::clip`, `docs/TRACE-SPEC.md`).

use crate::brush::{self, InOrder, Swept};
use crate::{CollisionWorld, Contents, Extents, Plane, Surface, Trace};

/// One solid box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Solid {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub contents: Contents,
    pub surface: Surface,
    /// The entity it belongs to, if not the level.
    pub entity: Option<u32>,
}

impl Solid {
    /// A box of level geometry.
    pub fn level(min: [f32; 3], max: [f32; 3]) -> Solid {
        Solid {
            min,
            max,
            contents: Contents::SOLID,
            surface: Surface::default(),
            entity: None,
        }
    }

    /// Its six planes, facing out: +X, −X, +Y, −Y, +Z, −Z.
    fn planes(&self) -> [Plane; 6] {
        crate::brushes::axial(self.min, self.max)
    }
}

/// Boxes, traced in the order they were added (the first of equal hits
/// wins, so the result never depends on anything but the list).
#[derive(Debug, Clone, Default)]
pub struct BoxWorld {
    pub solids: Vec<Solid>,
}

impl BoxWorld {
    pub fn new() -> BoxWorld {
        BoxWorld::default()
    }

    /// A floor: the box under `z` over the square `half` metres round the
    /// origin, `depth` deep.
    pub fn floor(half: f32, z: f32, depth: f32) -> BoxWorld {
        BoxWorld {
            solids: vec![Solid::level([-half, -half, z - depth], [half, half, z])],
        }
    }

    pub fn with(mut self, solid: Solid) -> BoxWorld {
        self.solids.push(solid);
        self
    }
}

impl CollisionWorld for BoxWorld {
    fn trace(&self, start: [f32; 3], end: [f32; 3], extents: &Extents, mask: Contents) -> Trace {
        let swept = Swept::Box {
            mins: extents.mins,
            maxs: extents.maxs,
        };
        let reach = (extents.mins, extents.maxs);
        let mut found = InOrder::new(end);
        for solid in self
            .solids
            .iter()
            .filter(|s| s.contents.intersects(mask))
            .filter(|s| crate::reaches(start, end, reach, &s.min, &s.max))
        {
            let clip = brush::clip(&solid.planes(), start, end, &swept);
            if found.add(clip, start, (solid.contents, solid.surface, solid.entity)) {
                break;
            }
        }
        found.finish(start, end)
    }

    fn point_contents(&self, point: [f32; 3], mask: Contents) -> Contents {
        self.solids
            .iter()
            .filter(|s| s.contents.intersects(mask))
            .filter(|s| (0..3).all(|i| point[i] >= s.min[i] && point[i] <= s.max[i]))
            .fold(Contents::NONE, |all, s| all | s.contents)
    }
}
