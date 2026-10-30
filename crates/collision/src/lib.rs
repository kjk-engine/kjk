//! Collision queries the sim runs: box and capsule traces over KOTOR's
//! levels, answering as JA's movement expects.
//!
//! A trace sweeps an axis-aligned box from `start` to `end` and stops at
//! the first solid it touches, as JA's movement expects: how far it got
//! (`fraction`), where (`end`), the plane it hit, whether it started inside
//! a solid or never left one, and what it hit. A point is a box of zero
//! size.
//!
//! Space is the sim's: KOTOR's world, metres, +Z up. JA's distances (such
//! as the 0.125 units a trace stops short of a face) are converted with
//! [`UNIT`], an inch.
//!
//! Collision runs inside the sim, so it keeps the sim's determinism rules
//! (`docs/SIM.md`): floats only through `+ - * /`, `sqrt`, comparisons and
//! exact bit operations, and anything order-dependent visited in a fixed
//! order, so every platform gets the same trace.
//!
//! - [`CollisionWorld`] is what movement, sabers and the AI call.
//! - [`BoxWorld`] is a stand-in made of boxes, for work that needs traces
//!   before the level collision is built (I12), and for probes.
//! - [`BrushWorld`] is the same with convex brushes of any slant (ramps),
//!   for probes that need slopes.
//! - [`Bodies`] puts the actors' boxes over any of them.
//! - [`Level`] is a level's collision: convex brushes (triangles, boxes,
//!   clip brushes) in a bounding-volume tree, with entity-owned brushes
//!   that switch with the entity's state (doors, placeables).
//! - [`audit`] finds where a player can get that the level's walkmesh
//!   never meant them to; [`clip_file`] reads the clip brushes that close
//!   it off; [`trig`] is the portable trigonometry the build and the sim
//!   use.

#[cfg(feature = "probes")]
pub mod audit;
mod bodies;
mod boxes;
mod brush;
mod brushes;
pub mod clip_file;
mod digester;
mod level;
pub mod nav;
pub mod pack;
#[cfg(feature = "probes")]
pub mod probes;
pub mod trig;
pub mod vector;

pub use bodies::{Bodies, Body};
pub use boxes::{BoxWorld, Solid};
pub use brushes::{Brush, BrushWorld};
pub use digester::Digester;
pub use level::{
    ALWAYS, BrushView, DropIndex, GONE, Level, LevelBuilder, LevelStats, Stated, TRIANGLE_DEPTH,
    Tag,
};

/// A JA unit (an inch) in metres.
pub const UNIT: f32 = 0.0254;

/// JA's player movement numbers, in units: the one definition the sim's
/// movement, the nav graph, the edge walls and the containment audit all
/// read; each the rulebook's (`data/rules/movement.toml`).
pub mod player {
    pub const STEP_HEIGHT: f32 = rules::movement::body::STEP_HEIGHT;
    pub const GRAVITY: f32 = rules::movement::air::GRAVITY;
    pub const BASE_SPEED: f32 = rules::movement::ground::BASE_SPEED;
    /// The movement box: this far either side, from `SOLE_Z` below the
    /// origin to `STAND_HEIGHT` above it standing.
    pub const HALF_WIDTH: f32 = rules::movement::body::HALF_WIDTH;
    pub const SOLE_Z: f32 = rules::movement::body::SOLE_Z;
    pub const STAND_HEIGHT: f32 = rules::movement::body::STAND_HEIGHT;
    pub const WALKABLE_NORMAL_Z: f32 = rules::movement::ground::WALKABLE_NORMAL_Z;
}

/// How far a trace stops short of the plane it hits, so the next trace
/// does not start inside it (0.125 units).
pub const CONTACT_GAP: f32 = 0.125 * UNIT;

/// What a volume is made of, as bits; a trace stops only at the contents
/// in its mask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Contents(pub u32);

impl Contents {
    pub const NONE: Contents = Contents(0);
    /// Level geometry.
    pub const SOLID: Contents = Contents(1);
    /// Blocks players and creatures only: the invisible walls the
    /// containment audit adds where KOTOR's levels were not built for
    /// jumping.
    pub const PLAYER_CLIP: Contents = Contents(1 << 1);
    /// Creatures' bodies.
    pub const BODY: Contents = Contents(1 << 2);
    /// Blocks a walker's own moves and nothing else (not the camera, shots
    /// or sight): the walls along a KOTOR walkmesh's open edges.
    pub const WALK_CLIP: Contents = Contents(1 << 3);
    /// With `SOLID`: a surface KOTOR's walker never stands on (a walkmesh
    /// face whose material is not walkable: a table's top, a console). A
    /// step up does not land on it (`sim::motion`).
    pub const NONWALK: Contents = Contents(1 << 4);

    /// What stops a player or creature (`MASK_PLAYERSOLID`, and the walkmesh
    /// edge walls).
    pub const PLAYER_SOLID: Contents = Contents(1 | (1 << 1) | (1 << 2) | (1 << 3));

    pub fn intersects(self, other: Contents) -> bool {
        self.0 & other.0 != 0
    }
}

impl std::ops::BitOr for Contents {
    type Output = Contents;
    fn bitor(self, other: Contents) -> Contents {
        Contents(self.0 | other.0)
    }
}

/// The surface a trace hit: KOTOR's walkmesh material (a row of
/// `surfacemat.2da`; 0 when the solid has none, as a box does).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Surface {
    pub material: u16,
}

/// A plane: points `p` with `normal · p = dist`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Plane {
    pub normal: [f32; 3],
    pub dist: f32,
}

/// The swept box: `mins` and `maxs` around the origin being traced.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Extents {
    pub mins: [f32; 3],
    pub maxs: [f32; 3],
}

impl Extents {
    /// A point: a box of zero size.
    pub const POINT: Extents = Extents {
        mins: [0.0; 3],
        maxs: [0.0; 3],
    };
}

/// An upright capsule: a segment along Z swept by a sphere (the shape JA's
/// movement can sweep instead of a box). `center` is from the traced
/// origin.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Capsule {
    pub center: [f32; 3],
    pub radius: f32,
    /// From the centre to the top (and bottom), sphere included.
    pub half_height: f32,
}

impl Capsule {
    /// The capsule that stands in for a box: centred in it, as tall, and
    /// as wide as the smaller of its X half-size and its half height.
    pub fn from_extents(extents: &Extents) -> Capsule {
        let half: [f32; 3] = std::array::from_fn(|i| (extents.maxs[i] - extents.mins[i]) * 0.5);
        Capsule {
            center: std::array::from_fn(|i| (extents.mins[i] + extents.maxs[i]) * 0.5),
            radius: if half[0] > half[2] { half[2] } else { half[0] },
            half_height: half[2],
        }
    }

    /// The box around it.
    pub fn bounds(&self) -> Extents {
        let (r, h, c) = (self.radius, self.half_height, self.center);
        Extents {
            mins: [c[0] - r, c[1] - r, c[2] - h],
            maxs: [c[0] + r, c[1] + r, c[2] + h],
        }
    }
}

/// What a sweep moves: a box (a point is a box of zero size) or a capsule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Shape {
    Box(Extents),
    Capsule(Capsule),
    /// A box centred across, its footprint round (an upright cylinder of
    /// its X half-size, as tall): a character's body against the level.
    /// KOTOR tests a creature's move by its personal space across the way
    /// it goes, whichever way that is (TSL's
    /// CSWSCreature::WalkUpdateLocationTestDistance hands
    /// CSWSArea::NoNonWalkPolysDetailed the appearance's `perspace`, and
    /// CSWCollisionMesh::NoNonWalkPolys sweeps the path widened by it), so
    /// a body as wide as a square box toward an axis is no wider toward a
    /// diagonal wall. Against an axis-aligned face, a box's. The walls
    /// standing in for KOTOR's point on the walkmesh (its outline's and
    /// placeables' footprints', set back by the box's half-width) it meets
    /// as the box.
    Upright(Extents),
}

/// The outcome of a trace.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trace {
    /// How much of the way from start to end the box got, 0 to 1.
    pub fraction: f32,
    /// Where the box's origin stopped.
    pub end: [f32; 3],
    /// The plane it stopped against (meaningless when `fraction` is 1).
    pub plane: Plane,
    /// The box started inside a solid.
    pub start_solid: bool,
    /// The box never left a solid (`fraction` is then 0).
    pub all_solid: bool,
    /// What it hit, and of what.
    pub surface: Surface,
    pub contents: Contents,
    /// The entity it hit, if not the level (doors, placeables, bodies).
    pub entity: Option<u32>,
}

impl Trace {
    /// A trace that went all the way.
    pub fn clear(end: [f32; 3]) -> Trace {
        Trace {
            fraction: 1.0,
            end,
            plane: Plane::default(),
            start_solid: false,
            all_solid: false,
            surface: Surface::default(),
            contents: Contents::NONE,
            entity: None,
        }
    }

    pub fn hit(&self) -> bool {
        self.fraction < 1.0 || self.start_solid
    }
}

/// Whether the box swept from `start` to `end` (`mins`, `maxs` round its
/// origin) overlaps a solid's bounds. The game skips every brush it does
/// not (QUIRKS.md Q-0020), so a trace ending in front of a face, nearer
/// than [`CONTACT_GAP`] but not touching it, is not pulled back.
pub(crate) fn reaches(
    start: [f32; 3],
    end: [f32; 3],
    reach: ([f32; 3], [f32; 3]),
    min: &[f32; 3],
    max: &[f32; 3],
) -> bool {
    let (mins, maxs) = reach;
    (0..3).all(|i| {
        let (low, high) = if start[i] < end[i] {
            (start[i] + mins[i], end[i] + maxs[i])
        } else {
            (end[i] + mins[i], start[i] + maxs[i])
        };
        low <= max[i] && high >= min[i]
    })
}

/// The collision the sim traces against.
pub trait CollisionWorld {
    /// Sweeps `extents` from `start` to `end`, stopping at contents in
    /// `mask`.
    fn trace(&self, start: [f32; 3], end: [f32; 3], extents: &Extents, mask: Contents) -> Trace;

    /// Sweeps `shape` from `start` to `end`. A world that has no capsule
    /// or cylinder clip of its own ([`BoxWorld`], whose solids are all
    /// axis-aligned) sweeps the shape's box.
    fn sweep(&self, start: [f32; 3], end: [f32; 3], shape: &Shape, mask: Contents) -> Trace {
        match shape {
            Shape::Box(extents) | Shape::Upright(extents) => self.trace(start, end, extents, mask),
            Shape::Capsule(capsule) => self.trace(start, end, &capsule.bounds(), mask),
        }
    }

    /// The contents at `point`, among those in `mask`.
    fn point_contents(&self, point: [f32; 3], mask: Contents) -> Contents;

    /// The built level behind this world, if it is one: whoever keeps the
    /// entities' states (the sim) traces it `with_states` of its own.
    fn level(&self) -> Option<&Level> {
        None
    }
}
