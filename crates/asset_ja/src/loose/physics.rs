//! The physics values of the loose points (step L2 of
//! `docs/research/2026-11-19-loose-parts-plan.md`). The games' bones carry
//! no physics values, so each kind of garment starts from one default
//! (`DEFAULTS`), and the model's own data adjusts it per point (`values`).
//! Every number to tune is in this file. They are starting values, to be
//! tuned by watching the `robe_acrobatics` bench scene.
//!
//! The units are those of XPBD. A compliance is the inverse of a stiffness,
//! in metres per newton, with point masses in kilograms. A compliance of 0
//! is a rigid constraint.
//!
//! What the data adjusts:
//!
//! - **Distance from the root.** A point's maximum distance from its target
//!   pose grows with its rest distance from the pinned root along the chain
//!   (`reach_share`). A hem point may stray far, and a point near the
//!   shoulder may not. This is how cloth tools paint their maximum distance
//!   (Unreal Chaos Cloth, Alan Wake's jacket).
//! - **Place in the chain.** The bending and the pull to the target pose
//!   are stiffer near the root and softer toward the tips, by the point's
//!   share of the tree's longest chain.
//! - **Mesh carried.** A point's mass is the kind's point mass times its
//!   share of the skin weights that its bones carry, against the mean point
//!   of its tree. The share is held between 0.5 and 2.
//! - **The old hand-keyed animation.** If the clips turned the bones above
//!   a point, the point may stray at least as far as those keys moved it
//!   from the rest pose, up to the kind's limit. The keys showed how far an
//!   artist let that part move.

use super::{Kind, Why};

/// One kind of garment's default values.
#[derive(Debug, Clone, Copy)]
pub struct Defaults {
    /// The mass of a point that carries the tree's mean load (kg).
    pub mass: f32,
    /// How far a point may stray from its target pose, as a share of its
    /// rest distance from the root along the chain.
    pub reach_share: f32,
    /// The most any point of the kind may stray (metres).
    pub max_distance: f32,
    /// The bones keep their length: the link to the parent point.
    pub stretch_compliance: f32,
    /// The bending at a joint, at the tips. It is 4 times stiffer at the
    /// root.
    pub bend_compliance: f32,
    /// The pull toward the target pose, at the tips. It is 4 times stiffer
    /// at the root.
    pub pull_compliance: f32,
    /// The share of a point's speed lost per second.
    pub damping: f32,
    /// The share of the character's world travel that the point feels. At 0
    /// it is carried with the character, and at 1 it is left behind in the
    /// world.
    pub world_travel: f32,
    /// The same for the character's turns.
    pub world_turn: f32,
    /// The radius of the point against the body's capsules (metres).
    pub radius: f32,
    /// The compliance of the links between neighbouring strands that the
    /// skin or a shared root joins. Links that are only near are 4 times
    /// softer.
    pub link_compliance: f32,
}

/// The defaults of each kind of garment.
///
/// - **Cape.** The freest kind. A long cape swings the most and feels the
///   most of a spin or a flip, so it flies out behind an acrobat. Its tip
///   may stray up to 70 cm, a little more than the 40 cm of Alan Wake's
///   jacket, because a cape is longer. It feels 30% of the character's
///   travel and 20% of its turns, the top of Alan Wake's 10 to 30%.
/// - **Robe skirt.** A little calmer than a cape. The hem hangs at knee
///   height and must not wrap over the head in a flip, so its hem may stray
///   up to 45 cm and it feels 25% of the travel and 15% of the turns.
/// - **Flap.** A short piece: a belt strip, a thigh flap or a piece on the
///   chest. It may stray up to 25 cm.
/// - **Sleeve.** A sleeve hangs from a forearm and swings with the arm. It
///   may stray up to 15 cm, and it is pulled back to the arm firmly.
/// - **Hood.** The stiffest cloth. A hood must stay on the head during a
///   flip, so it may stray only 6 cm, it is pulled hard toward its target,
///   it feels only 5% of the travel and turns, and it bends 100 times less
///   than a cape.
/// - **Other.** Armour, such as the Mandalorian's shoulder plates. It barely
///   moves: 3 cm at most, and nearly rigid.
pub const DEFAULTS: [(Kind, Defaults); 6] = [
    (
        Kind::Cape,
        Defaults {
            mass: 0.25,
            reach_share: 0.9,
            max_distance: 0.7,
            stretch_compliance: 0.0,
            bend_compliance: 1e-2,
            pull_compliance: 1e-1,
            damping: 1.0,
            world_travel: 0.3,
            world_turn: 0.2,
            radius: 0.03,
            link_compliance: 1e-3,
        },
    ),
    (
        Kind::RobeSkirt,
        Defaults {
            mass: 0.2,
            reach_share: 0.7,
            max_distance: 0.45,
            stretch_compliance: 0.0,
            bend_compliance: 5e-3,
            pull_compliance: 5e-2,
            damping: 1.5,
            world_travel: 0.25,
            world_turn: 0.15,
            radius: 0.04,
            link_compliance: 5e-4,
        },
    ),
    (
        Kind::Flap,
        Defaults {
            mass: 0.1,
            reach_share: 0.5,
            max_distance: 0.25,
            stretch_compliance: 0.0,
            bend_compliance: 2e-3,
            pull_compliance: 1e-2,
            damping: 2.0,
            world_travel: 0.2,
            world_turn: 0.15,
            radius: 0.025,
            link_compliance: 5e-4,
        },
    ),
    (
        Kind::Sleeve,
        Defaults {
            mass: 0.1,
            reach_share: 0.35,
            max_distance: 0.15,
            stretch_compliance: 0.0,
            bend_compliance: 1e-3,
            pull_compliance: 1e-3,
            damping: 3.0,
            world_travel: 0.15,
            world_turn: 0.1,
            radius: 0.03,
            link_compliance: 5e-4,
        },
    ),
    (
        Kind::Hood,
        Defaults {
            mass: 0.1,
            reach_share: 0.15,
            max_distance: 0.06,
            stretch_compliance: 0.0,
            bend_compliance: 1e-4,
            pull_compliance: 1e-4,
            damping: 4.0,
            world_travel: 0.05,
            world_turn: 0.05,
            radius: 0.03,
            link_compliance: 1e-4,
        },
    ),
    (
        Kind::Other,
        Defaults {
            mass: 0.3,
            reach_share: 0.1,
            max_distance: 0.03,
            stretch_compliance: 0.0,
            bend_compliance: 1e-5,
            pull_compliance: 1e-5,
            damping: 6.0,
            world_travel: 0.05,
            world_turn: 0.05,
            radius: 0.02,
            link_compliance: 1e-5,
        },
    ),
];

/// How much stiffer the bending and the pull are at the root than at the
/// tips.
const ROOT_STIFFER: f32 = 4.0;
/// How far a point's load may move its mass from the kind's point mass.
const LOAD_MIN: f32 = 0.5;
const LOAD_MAX: f32 = 2.0;
/// How much softer a link between strands that are only near is.
const NEAR_SOFTER: f32 = 4.0;

/// The defaults of `kind`.
pub fn defaults(kind: Kind) -> Defaults {
    DEFAULTS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, d)| *d)
        .expect("every kind has defaults")
}

/// The physics values of one point, for the solver.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Physics {
    /// 0 for a pinned root, which follows its animated parent exactly.
    pub inverse_mass: f32,
    /// How far the point may stray from its target pose (metres).
    pub max_distance: f32,
    /// The link to the parent point.
    pub stretch_compliance: f32,
    /// The bending at the parent point: the parent segment against this
    /// one.
    pub bend_compliance: f32,
    /// The pull toward the target pose.
    pub pull_compliance: f32,
    /// The share of the point's speed lost per second.
    pub damping: f32,
    /// The share of the character's world travel and turns that the point
    /// feels.
    pub world_travel: f32,
    pub world_turn: f32,
    /// The point's radius against the body's capsules (metres).
    pub radius: f32,
}

/// What the model's data says about one point.
#[derive(Debug, Clone, Copy)]
pub struct Input {
    pub pinned: bool,
    /// The rest length of the chain from the root to the point (metres).
    pub along: f32,
    /// The longest such length in the point's tree.
    pub longest: f32,
    /// The skin weights the point's bones carry, against the mean point of
    /// its tree.
    pub load: f32,
    /// How far the clips' keys moved the point from its rest pose (metres).
    pub keyed_reach: f32,
}

/// The values of one point of a tree of `kind`.
pub fn values(kind: Kind, input: &Input) -> Physics {
    let d = defaults(kind);
    let share = if input.longest > 0.0 {
        (input.along / input.longest).clamp(0.0, 1.0)
    } else {
        1.0
    };
    let softer_to_tip = (1.0 + (ROOT_STIFFER - 1.0) * share) / ROOT_STIFFER;
    let mass = d.mass * input.load.clamp(LOAD_MIN, LOAD_MAX);
    let max_distance = if input.pinned {
        0.0
    } else {
        (d.reach_share * input.along)
            .max(input.keyed_reach)
            .min(d.max_distance)
    };
    Physics {
        inverse_mass: if input.pinned { 0.0 } else { 1.0 / mass },
        max_distance,
        stretch_compliance: d.stretch_compliance,
        bend_compliance: d.bend_compliance * softer_to_tip,
        pull_compliance: d.pull_compliance * softer_to_tip,
        damping: d.damping,
        world_travel: d.world_travel,
        world_turn: d.world_turn,
        radius: d.radius,
    }
}

/// The compliance of a link of `why` between strands of `kind`.
pub fn link_compliance(kind: Kind, why: Why) -> f32 {
    let d = defaults(kind);
    match why {
        Why::Branch => 0.0,
        Why::Root | Why::Skin => d.link_compliance,
        Why::Near => d.link_compliance * NEAR_SOFTER,
    }
}
