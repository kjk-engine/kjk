//! Active ragdolls: a character's body as capsules joined at its bones,
//! driven toward an animation by muscles that tire, then left to fall as it
//! will. View-side only: nothing here reaches the sim, so it need not be
//! deterministic.
//!
//! - [`Template`]: which nodes of a skeleton become bodies, the capsules
//!   that fill them (sized from the skin) and the joints between them, by a
//!   [`Plan`] ([`humanoid::PLAN`] for KOTOR's humanoid skeleton) or fitted
//!   to any skeleton alone (`Template::auto`: beasts, droids), of a
//!   [`Material`].
//! - [`World`]: the bodies against the level's faces, stepped in fixed
//!   steps. Each ragdoll follows targets (the animation, world space) with
//!   two drives: `hold`, which pulls every body toward its target as a
//!   puppet's strings would, and `tone`, the joints' muscles, which turn each
//!   body toward its target relative to its parent and can be overpowered.
//!   A [`Muscles`] schedule lets both go as the death plays out.

mod auto;
mod body;
pub mod cut;
pub mod humanoid;
#[cfg(feature = "probes")]
pub mod probes;
mod world;

pub use body::{
    Group, Joint, JointKind, JointPlan, Material, Node, Part, PartPlan, Plan, Shape, Skeleton,
    Solid, Template, Way,
};
pub use world::{Id, Muscles, World};

use glam::{Quat, Vec3};

/// A rigid placement: rotate, then move.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    pub rotation: Quat,
    pub translation: Vec3,
}

impl Frame {
    pub const IDENTITY: Frame = Frame {
        rotation: Quat::IDENTITY,
        translation: Vec3::ZERO,
    };

    pub fn new(translation: Vec3, rotation: Quat) -> Frame {
        Frame {
            rotation,
            translation,
        }
    }

    /// `self` then `local` (`local` given in `self`'s space).
    pub fn then(self, local: Frame) -> Frame {
        Frame {
            rotation: (self.rotation * local.rotation).normalize(),
            translation: self.translation + self.rotation * local.translation,
        }
    }

    pub fn inverse(self) -> Frame {
        let rotation = self.rotation.inverse();
        Frame {
            rotation,
            translation: -(rotation * self.translation),
        }
    }

    pub fn point(self, p: Vec3) -> Vec3 {
        self.translation + self.rotation * p
    }
}
