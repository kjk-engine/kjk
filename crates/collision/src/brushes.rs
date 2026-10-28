//! A collision world of convex brushes with planes of any slant: ramps,
//! wedges and boxes to trace against before the level collision exists,
//! and for probes that need slopes (`BoxWorld` has only axial faces). Each
//! brush goes through the brush clip every world shares (`brush::clip`,
//! `docs/TRACE-SPEC.md`). A brush lists its six axial bounds too, as JA's
//! compiler adds them (bevels), so a box's corner cannot slip past a
//! slanted face's edge.

use crate::brush::{self, InOrder, Swept, dot};
use crate::{CollisionWorld, Contents, Extents, Plane, Surface, Trace};

/// One convex solid: the points behind all its planes.
#[derive(Debug, Clone, PartialEq)]
pub struct Brush {
    pub planes: Vec<Plane>,
    /// Its bounds, for `point_contents` and a quick reject.
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub contents: Contents,
    pub surface: Surface,
    pub entity: Option<u32>,
}

/// The six faces of the box `min`..`max`, facing out: +X, −X, +Y, −Y, +Z,
/// −Z.
pub(crate) fn axial(min: [f32; 3], max: [f32; 3]) -> [Plane; 6] {
    std::array::from_fn(|face| {
        let axis = face / 2;
        let mut normal = [0.0; 3];
        if face % 2 == 0 {
            normal[axis] = 1.0;
            Plane {
                normal,
                dist: max[axis],
            }
        } else {
            normal[axis] = -1.0;
            Plane {
                normal,
                dist: -min[axis],
            }
        }
    })
}

impl Brush {
    /// An axis-aligned box of level geometry (the same planes, in the same
    /// order, as `Solid`).
    pub fn cuboid(min: [f32; 3], max: [f32; 3]) -> Brush {
        Brush {
            planes: axial(min, max).to_vec(),
            min,
            max,
            contents: Contents::SOLID,
            surface: Surface::default(),
            entity: None,
        }
    }

    /// A ramp over the box `min`..`max`, rising along `axis` (0 is X, 1 is
    /// Y) from `min.z` at the `min` side to `max.z` at the `max` side (or
    /// the other way with `down`): a wedge, its top the slope.
    pub fn ramp(min: [f32; 3], max: [f32; 3], axis: usize, down: bool) -> Brush {
        let mut brush = Brush::cuboid(min, max);
        let run = max[axis] - min[axis];
        let rise = max[2] - min[2];
        let length = (run * run + rise * rise).sqrt();
        // The slope's plane goes through the low edge and the high edge.
        let mut normal = [0.0; 3];
        normal[axis] = if down { rise / length } else { -rise / length };
        normal[2] = run / length;
        let low_edge = if down { max[axis] } else { min[axis] };
        let mut point = [0.0; 3];
        point[axis] = low_edge;
        point[2] = min[2];
        brush.planes.push(Plane {
            normal,
            dist: dot(normal, point),
        });
        brush
    }
}

/// Brushes, traced in the order they were added (the first of equal hits
/// wins).
#[derive(Debug, Clone, Default)]
pub struct BrushWorld {
    pub brushes: Vec<Brush>,
}

impl BrushWorld {
    pub fn new() -> BrushWorld {
        BrushWorld::default()
    }

    pub fn with(mut self, brush: Brush) -> BrushWorld {
        self.brushes.push(brush);
        self
    }
}

impl CollisionWorld for BrushWorld {
    fn trace(&self, start: [f32; 3], end: [f32; 3], extents: &Extents, mask: Contents) -> Trace {
        let swept = Swept::Box {
            mins: extents.mins,
            maxs: extents.maxs,
        };
        let reach = (extents.mins, extents.maxs);
        let mut found = InOrder::new(end);
        for brush in self
            .brushes
            .iter()
            .filter(|b| b.contents.intersects(mask))
            .filter(|b| crate::reaches(start, end, reach, &b.min, &b.max))
        {
            let clip = brush::clip(&brush.planes, start, end, &swept);
            if found.add(clip, start, (brush.contents, brush.surface, brush.entity)) {
                break;
            }
        }
        found.finish(start, end)
    }

    fn point_contents(&self, point: [f32; 3], mask: Contents) -> Contents {
        self.brushes
            .iter()
            .filter(|b| b.contents.intersects(mask))
            .filter(|b| b.planes.iter().all(|p| dot(point, p.normal) <= p.dist))
            .fold(Contents::NONE, |all, b| all | b.contents)
    }
}
