//! Loose parts: robe skirts, capes and other garments that hang from a
//! character's body, moved by an XPBD solver on points. This is the solver
//! and the body collision of https://github.com/kjk-engine/roadmap/issues/5.
//!
//! The solver runs in the view only. Nothing here reaches the sim, so it
//! need not be deterministic across machines. It does give the same result
//! for the same inputs on one machine. It steps by the game time it is
//! given, in fixed substeps, and never by a frame's wall time.
//!
//! - [`Rig`] is the solver's input, made once per body model. It is plain
//!   data. It lists the points with their values, and the links between
//!   neighbouring chains.
//! - [`Skeleton`] is the body's skeleton at rest, and [`Body`] is its
//!   capsules, both made once per body model.
//! - [`Solver::step`] reads each frame's animated skeleton and returns the
//!   new turns of the loose bones.
//!
//! Every position the solver keeps is in the space of one node of the
//! skeleton, its anchor ([`Skeleton::anchor`]). In the game and the viewers
//! the anchor is the pelvis. The anchor's movement through the world is
//! felt only by a share that each point sets ([`Point::inertia`]). That
//! holds whether the movement comes from the node the cast places in the
//! world or from the animation inside the skeleton. A flip, a spin or a
//! roll of a Jedi Academy move turns the bones and not the placed node, and
//! the cast can move the facing between the two in one frame. Neither
//! changes what the points feel, because the pelvis turns the same way in
//! the world either way.

mod body;
#[cfg(feature = "probes")]
pub mod probes;
mod solver;
mod trace;

pub use body::Body;
pub use ragdoll::Frame;
pub use solver::{Settings, Solver, Step, View};

use glam::Vec3;

/// Where a point is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Place {
    /// The joint of a skeleton node: a loose bone. The point hangs from the
    /// point of the node's parent. When the parent node has no point, this
    /// point is a root. A root is pinned to its parent node, which the
    /// animation moves.
    Joint(usize),
    /// The tip after the last bone of a chain. It hangs from the joint point
    /// `point`, at `offset` in the frame of that point's bone.
    Tip { point: usize, offset: Vec3 },
}

/// One point of a loose tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub place: Place,
    /// How freely the point moves, from 0 to 1. This is its inverse mass. A
    /// point with 0 follows its target pose as if it were pinned.
    pub freedom: f32,
    /// How far the point may ever be from its target pose (metres).
    pub max_distance: f32,
    /// The compliance of the distance to its parent (metres per newton). At
    /// 0 the length never changes.
    pub stretch: f32,
    /// The compliance of the bend at its parent (metres per newton). The
    /// bend keeps the distance from this point to its grandparent near its
    /// rest length. A child of a root has no bend and swings freely.
    pub bend: f32,
    /// The compliance of a pull toward its target pose (metres per newton).
    /// With none, the point only hangs and swings, within its maximum
    /// distance.
    pub drive: Option<f32>,
    /// The share of its speed that the point loses each second.
    pub damping: f32,
    /// The share of the anchor's movement through the world that the
    /// point feels, from 0 to 1. At 0 the point is carried along with the
    /// character and feels none of it. At 1 it stays where it was in the
    /// world, and the character moves away under it.
    pub inertia: f32,
    /// How thick the garment is around the point (metres). The point keeps
    /// this far from the surface of a capsule.
    pub thickness: f32,
    /// The share of the way, from 0 to 1, that the bone to the point turns
    /// from its animated pose toward straight down to make the point's
    /// target. At 0 the target is the animated pose. At 1 the bone's target
    /// hangs straight down, as cloth would with nothing under it.
    pub hang: f32,
    /// The skeleton nodes whose bones move the same garment vertices as the
    /// point's bone. The point collides with the capsules of these nodes and
    /// of the capsules near them. When the list is empty, the point may meet
    /// any capsule.
    pub reach: Vec<usize>,
}

/// A distance kept between two points of neighbouring chains, so that a
/// robe does not split apart. The rest length is the distance between the
/// two points at rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Link {
    pub a: usize,
    pub b: usize,
    /// Metres per newton.
    pub compliance: f32,
}

/// The loose points of one body and the links between them: the solver's
/// input, made once per body model. A point's parent comes before it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rig {
    pub points: Vec<Point>,
    pub links: Vec<Link>,
}

/// A body's skeleton in its rest pose. Each node's parent comes before it.
#[derive(Debug, Clone, PartialEq)]
pub struct Skeleton {
    pub parents: Vec<Option<usize>>,
    /// Each node's rest frame in its parent's frame.
    pub rest: Vec<Frame>,
    /// The node whose movement through the world the points feel only in
    /// part, and in whose space they are solved.
    pub anchor: usize,
}
