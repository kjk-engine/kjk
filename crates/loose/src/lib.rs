//! Loose parts: robe skirts, capes and other garments that hang from a
//! character's body, moved by an XPBD solver on points. This is step L3
//! and L4 of `docs/research/2026-11-19-loose-parts-plan.md`.
//!
//! The solver runs in the view only. Nothing here reaches the sim, so it
//! need not be deterministic across machines. It does give the same result
//! for the same inputs on one machine, because it steps by the game time it
//! is given, in fixed substeps, and never by a frame's wall time.
//!
//! - [`Rig`] is the solver's input, made once per body model. It is plain
//!   data: the points with their tree and their values, and the links
//!   between neighbouring chains.
//! - [`Pose`] is what the solver reads each frame: the character's root in
//!   the world, the animated frame each root point hangs on, and the
//!   animated turn of each point's bone.
//! - [`Solver`] keeps the points' state and steps them. [`Solver::turns`]
//!   turns the solved points back into bone rotations.
//! - [`body`] poses the 14 ragdoll capsules from the animated skeleton and
//!   gives each point the capsules it collides with.
//!
//! Every position the solver keeps is in the character's root space. The
//! root is the frame that the character's movement through the world moves:
//! in the game, the model's root, placed by the cast. A flip or a roll that
//! the animation plays turns the body within that frame, so the points feel
//! all of it. The root's own movement through the world is felt only by a
//! share that each point sets ([`Point::inertia`]).

pub mod body;
#[cfg(feature = "probes")]
pub mod probes;
mod solver;

pub use body::{Capsule, Collision, Contact};
pub use ragdoll::Frame;
pub use solver::{Pose, Report, Settings, Solver};

use glam::Vec3;

/// One point of a loose tree. A point is the joint of a loose bone, or the
/// tip after the last bone of a chain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// The point this one hangs from. A point without a parent is a root.
    /// A root hangs on a node of the body that the animation moves (its
    /// attach node, given each frame in [`Pose::attach`]). A parent always
    /// comes before its children in [`Rig::points`].
    pub parent: Option<usize>,
    /// Where the point sits in the frame of the bone it hangs from. For a
    /// point under another point, that is the parent point's bone. For a
    /// root, it is the attach node. This is the bone's local translation,
    /// which never changes.
    pub offset: Vec3,
    /// Where the point is in the rest pose, in model space. The rest
    /// lengths of the bends, the links and the tethers are measured here.
    pub at: Vec3,
    /// A pinned point follows its target pose exactly. Every root must be
    /// pinned.
    pub pinned: bool,
    /// How freely the point moves, from 0 to 1. This is its inverse mass.
    /// A point with 0 follows its target pose as if it were pinned.
    pub freedom: f32,
    /// How far the point may ever be from its target pose (metres).
    pub max_distance: f32,
    /// The compliance of the distance to its parent (metres per newton).
    /// 0 keeps the length exact.
    pub stretch: f32,
    /// The compliance of the bend at its parent (metres per newton). It
    /// keeps the distance from this point to its grandparent near its rest
    /// length. 0 keeps the chain as straight as it is at rest.
    pub bend: f32,
    /// The compliance of a pull toward its target pose (metres per newton).
    /// None means no pull. The point then only hangs and swings, within
    /// its maximum distance.
    pub drive: Option<f32>,
    /// The share of its speed that the point loses each second.
    pub damping: f32,
    /// The share of the root's movement through the world that the point
    /// feels, from 0 to 1. At 0 the point is carried along with the
    /// character and feels none of it. At 1 it stays where it was in the
    /// world and the character moves away under it.
    pub inertia: f32,
    /// How far the point keeps from the surface of a capsule (metres). It is
    /// the thickness of the garment around the point. [`body::contacts`]
    /// adds it to each capsule's radius.
    pub radius: f32,
}

/// A distance kept between two points of neighbouring chains, so that a
/// robe does not split apart. The rest length is the distance between the
/// two points at rest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Link {
    pub a: usize,
    pub b: usize,
    /// Metres per newton. 0 keeps the distance exact.
    pub compliance: f32,
}

/// The loose points of one body and the links between them: the solver's
/// input, made once per body model.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rig {
    pub points: Vec<Point>,
    pub links: Vec<Link>,
}

impl Rig {
    /// Whether the rig can be solved. Each parent must come before its
    /// children, every root must be pinned, every link must join two
    /// different points, and every value must be a finite number in its
    /// range.
    pub fn check(&self) -> Result<(), String> {
        for (index, point) in self.points.iter().enumerate() {
            match point.parent {
                Some(parent) if parent >= index => {
                    return Err(format!("point {index} comes before its parent {parent}"));
                }
                None if !point.pinned => {
                    return Err(format!("point {index} is a root and is not pinned"));
                }
                _ => {}
            }
            let values = [
                point.freedom,
                point.max_distance,
                point.stretch,
                point.bend,
                point.drive.unwrap_or(0.0),
                point.damping,
                point.inertia,
                point.radius,
            ];
            if values
                .iter()
                .any(|value| !value.is_finite() || *value < 0.0)
                || !point.offset.is_finite()
                || !point.at.is_finite()
            {
                return Err(format!(
                    "point {index} has a value that is not a finite number of at least 0"
                ));
            }
            if point.freedom > 1.0 || point.inertia > 1.0 {
                return Err(format!("point {index} has a freedom or an inertia above 1"));
            }
        }
        for link in &self.links {
            let count = self.points.len();
            if link.a == link.b || link.a >= count || link.b >= count {
                return Err(format!(
                    "the link from {} to {} does not join two points",
                    link.a, link.b
                ));
            }
            if !link.compliance.is_finite() || link.compliance < 0.0 {
                return Err(format!(
                    "the link from {} to {} has a bad compliance",
                    link.a, link.b
                ));
            }
        }
        Ok(())
    }

    /// The pinned point that each point hangs from, at the top of its tree.
    pub fn roots(&self) -> Vec<usize> {
        let mut roots: Vec<usize> = Vec::with_capacity(self.points.len());
        for (index, point) in self.points.iter().enumerate() {
            let root = match point.parent {
                Some(parent) if !point.pinned => roots[parent],
                _ => index,
            };
            roots.push(root);
        }
        roots
    }
}
