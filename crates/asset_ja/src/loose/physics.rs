//! The physics values of the loose points. The games' bones carry no
//! physics values. Each kind of garment therefore starts from one set of
//! defaults (`DEFAULTS`), and the model's own data adjusts it for each
//! point. Every number to tune is in this file. They are starting values,
//! to be tuned by watching the `robe_acrobatics` bench scene.
//!
//! The values follow the input of the robe and cape solver. A compliance is
//! the inverse of a stiffness, in metres per newton, and 0 is a rigid
//! constraint. A point's freedom is its inverse mass, from 0 to 1.
//!
//! The data adjusts the defaults in four ways.
//!
//! - **Distance from the root.** A point's maximum distance from its target
//!   pose grows with its rest distance from the pinned root along the chain
//!   (`reach_share`). A hem point may stray far, and a point near the
//!   shoulder may not. Cloth tools paint their maximum distance the same
//!   way (Unreal Chaos Cloth, Alan Wake's jacket).
//! - **Place in the chain.** The bending and the pull toward the target pose
//!   are stiffer near the root and softer toward the tips. They follow the
//!   point's share of the tree's longest chain.
//! - **Mesh carried.** A point that carries more skin than the mean point of
//!   its tree is heavier, so it is less free. The share is held at 2 or
//!   less, so the freedom is held between 0.5 and 1.
//! - **The old hand-keyed animation.** If the clips turned the bones above a
//!   point, the point may stray at least as far as those keys moved it from
//!   the hang pose, up to the kind's limit. The keys show how far an artist
//!   let that part move.
//!
//! The radius here is the garment's thickness around the point. The solver
//! adds it to each body capsule. It is not the point's own fit to the
//! capsules, which the collision step works out.

use super::{Cause, Kind};

/// One kind of garment's default values.
#[derive(Debug, Clone, Copy)]
pub struct Defaults {
    /// How far a point may stray from its target pose, as a share of its
    /// rest distance from the root along the chain.
    pub reach_share: f32,
    /// The most that any point of the kind may stray (metres).
    pub max_distance: f32,
    /// The compliance of the link to the parent point. At 0 the bones keep
    /// their length.
    pub stretch: f32,
    /// The compliance of the bend at a joint, at the tips. It is 4 times
    /// stiffer at the root.
    pub bend: f32,
    /// The compliance of the pull toward the target pose, at the tips. It is
    /// 4 times stiffer at the root.
    pub drive: f32,
    /// The share of a point's speed that it loses each second.
    pub damping: f32,
    /// The share of the character's movement through the world that a point
    /// feels. At 0 the point is carried with the character. At 1 it is left
    /// behind in the world.
    pub inertia: f32,
    /// The garment's thickness around a point (metres).
    pub radius: f32,
    /// The compliance of a link between neighbouring strands that the skin
    /// or a shared root joins. A link between strands that are only near,
    /// or only next to each other around the pelvis, is 4 times softer.
    pub link: f32,
}

/// The defaults of each kind of garment.
///
/// - **Cape.** The freest kind. A long cape swings the most and feels the
///   most of a spin or a flip, so it flies out behind an acrobat. Its tip
///   may stray further than Alan Wake's jacket, because a cape is longer.
///   It feels the top of Alan Wake's 10 to 30% of world motion.
/// - **Robe skirt.** A little calmer than a cape. The hem hangs at knee
///   height, and it must not wrap over the head in a flip.
/// - **Flap.** A short piece, such as a belt strip, a thigh flap or a piece
///   on the chest.
/// - **Sleeve.** A sleeve hangs from a forearm and swings with the arm. It
///   is pulled back to the arm firmly.
/// - **Hood.** The stiffest cloth. A hood must stay on the head during a
///   flip. It may stray only a few centimetres, it is pulled hard toward
///   its target, it feels almost nothing of the travel, and it bends 100
///   times less than a cape.
/// - **Other.** Armour, such as the Mandalorian's shoulder plates. It barely
///   moves and is nearly rigid.
pub const DEFAULTS: [(Kind, Defaults); 6] = [
    (
        Kind::Cape,
        Defaults {
            reach_share: 0.9,
            max_distance: 0.7,
            stretch: 0.0,
            bend: 2e-3,
            drive: 2e-2,
            damping: 1.0,
            inertia: 0.3,
            radius: 0.03,
            link: 2e-4,
        },
    ),
    (
        Kind::RobeSkirt,
        Defaults {
            reach_share: 0.7,
            max_distance: 0.45,
            stretch: 0.0,
            bend: 1e-3,
            drive: 1e-2,
            damping: 1.5,
            inertia: 0.25,
            radius: 0.04,
            link: 1e-4,
        },
    ),
    (
        Kind::Flap,
        Defaults {
            reach_share: 0.5,
            max_distance: 0.25,
            stretch: 0.0,
            bend: 4e-4,
            drive: 2e-3,
            damping: 2.0,
            inertia: 0.2,
            radius: 0.025,
            link: 1e-4,
        },
    ),
    (
        Kind::Sleeve,
        Defaults {
            reach_share: 0.35,
            max_distance: 0.15,
            stretch: 0.0,
            bend: 2e-4,
            drive: 2e-4,
            damping: 3.0,
            inertia: 0.15,
            radius: 0.03,
            link: 1e-4,
        },
    ),
    (
        Kind::Hood,
        Defaults {
            reach_share: 0.15,
            max_distance: 0.06,
            stretch: 0.0,
            bend: 2e-5,
            drive: 2e-5,
            damping: 4.0,
            inertia: 0.05,
            radius: 0.03,
            link: 2e-5,
        },
    ),
    (
        Kind::Other,
        Defaults {
            reach_share: 0.1,
            max_distance: 0.03,
            stretch: 0.0,
            bend: 2e-6,
            drive: 2e-6,
            damping: 6.0,
            inertia: 0.05,
            radius: 0.02,
            link: 2e-6,
        },
    ),
];

/// How much stiffer the bending and the pull are at the root than at the
/// tips.
const ROOT_STIFFER: f32 = 4.0;
/// The most that a point's share of its tree's mean load counts. A point
/// with this share has half the freedom of a light one.
const LOAD_MAX: f32 = 2.0;
/// How much softer a link between strands that are only near, or only next
/// to each other around the pelvis, is.
const NEAR_SOFTER: f32 = 4.0;

pub fn defaults(kind: Kind) -> Defaults {
    DEFAULTS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, d)| *d)
        .expect("every kind has defaults")
}

/// The physics values of one point, for the solver. The fields and their
/// ranges are those of the solver's own point.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Physics {
    /// The point's inverse mass, from 0 to 1. A pinned root has 0.
    pub freedom: f32,
    /// How far the point may stray from its target pose (metres). A pinned
    /// root has 0.
    pub max_distance: f32,
    /// The compliance of the link to the parent point.
    pub stretch: f32,
    /// The compliance of the bend at the parent point, which keeps this
    /// point near its rest distance from its grandparent.
    pub bend: f32,
    /// The compliance of the pull toward the target pose.
    pub drive: Option<f32>,
    /// The share of the point's speed that it loses each second.
    pub damping: f32,
    /// The share of the character's movement through the world that the
    /// point feels, from 0 to 1.
    pub inertia: f32,
    /// The garment's thickness around the point (metres).
    pub radius: f32,
}

/// What the model's data says about one point.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Input {
    pub pinned: bool,
    /// The rest length of the chain from the root to the point (metres).
    pub along: f32,
    /// The longest such length in the point's tree.
    pub longest: f32,
    /// The skin weights that the point's bones carry, against the mean point
    /// of its tree.
    pub load: f32,
    /// How far the clips' keys moved the point from its hang pose (metres).
    pub keyed_reach: f32,
}

/// The values of one point of a tree of `kind`.
pub(crate) fn values(kind: Kind, input: &Input) -> Physics {
    let d = defaults(kind);
    let share = if input.longest > 0.0 {
        (input.along / input.longest).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let softer_to_tip = (1.0 + (ROOT_STIFFER - 1.0) * share) / ROOT_STIFFER;
    if input.pinned {
        return Physics {
            freedom: 0.0,
            max_distance: 0.0,
            stretch: d.stretch,
            bend: d.bend * softer_to_tip,
            drive: Some(d.drive * softer_to_tip),
            damping: d.damping,
            inertia: d.inertia,
            radius: d.radius,
        };
    }
    Physics {
        freedom: 1.0 / input.load.clamp(1.0, LOAD_MAX),
        max_distance: (d.reach_share * input.along)
            .max(input.keyed_reach)
            .min(d.max_distance),
        stretch: d.stretch,
        bend: d.bend * softer_to_tip,
        drive: Some(d.drive * softer_to_tip),
        damping: d.damping,
        inertia: d.inertia,
        radius: d.radius,
    }
}

/// The compliance of a link of `cause` between strands of `kind`.
pub(crate) fn link_compliance(kind: Kind, cause: Cause) -> f32 {
    let d = defaults(kind);
    match cause {
        Cause::Branch => 0.0,
        Cause::Root | Cause::Skin => d.link,
        Cause::Near | Cause::Around => d.link * NEAR_SOFTER,
    }
}
