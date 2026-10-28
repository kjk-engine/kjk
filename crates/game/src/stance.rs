//! A character standing in one of JA's held stances breathes as KOTOR's
//! own characters stand, gently.
//!
//! - **Why:** JA's stances are held poses, one or two frames long
//!   (`BOTH_STAND2` for a saber, `TORSO_WEAPONREADY2`/`3` for a gun, the
//!   dual and staff stances, `BOTH_STAND1` under them); JA's only life at
//!   rest is its idle every few seconds (`sim::combat::idle`). KOTOR's
//!   characters never stand still: they loop `pause1` out of a fight and
//!   their wield's ready stance (`g<wield>r1`) in one (as reone
//!   plays them).
//! - **What:** while both the legs and the torso hold such a pose (or the
//!   torso fires a gun from one), alive, standing, under no KOTOR clip (a
//!   script's, the gait's), the upper body (`NODES`) takes a share
//!   (`AMOUNT`) of the motion of its wield's ready clip, played slower
//!   (`PACE`): each node's turn away from the clip's first frame, over the
//!   JA pose (`Animator::add_over`). The chest also leans slowly from side
//!   to side (`LEAN_DEG`, `LEAN_S`). The legs, hands and fingers keep JA's
//!   pose, so the feet stay planted, the gun keeps its aim and the blade
//!   its grip. A body without its wield's clip takes `pause1`'s.
//! - **Not KOTOR's amount** (the owner's call): the ready clips' whole
//!   motion over JA's stances looked like the shakes (shins 7°, hands 6°).
//! - **Blending:** it eases in and out over `EASE_S`, as the gait's clips
//!   do; each body plays from its own place in the loops, so a squad does
//!   not breathe in step.
//!
//! Render only: the sim's skeleton, which the blades cut with, holds JA's
//! pose.

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use bevy::prelude::Entity;
use fmt_ja::animation_cfg::Sequence;
use sim::combat::sheet::{Blaster, SaberShape, Sheet};
use sim::combat::{Delivery, Weapon};
use sim::motion::Anim;
use vfs::resolver::Resolver;

use crate::acting::{self, Bound};
use crate::scene::Animator;

/// Seconds the motion takes to come in or go out.
const EASE_S: f32 = 0.25;
/// The longest sequence (frames) that is a held pose.
const HELD_FRAMES: i32 = 2;
/// The share of the ready clip's motion shown.
const AMOUNT: f32 = 0.3;
/// How fast the ready clip plays (1: its own pace).
const PACE: f64 = 0.5;
/// The side-to-side lean: how far each way (degrees) and how long one
/// sway, there and back, takes (seconds).
const LEAN_DEG: f64 = 0.8;
const LEAN_S: f64 = 6.5;
/// The nodes that breathe: the spine, the head and the arms above the
/// wrists.
pub const NODES: [&str; 8] = [
    "torso_g",
    "torsoupr_g",
    "neck_g",
    "head_g",
    "lbicep_g",
    "rbicep_g",
    "lforearm_g",
    "rforearm_g",
];
/// The node that leans.
pub const LEANS: &str = "torsoupr_g";

/// Whether a body whose legs and torso play `legs` and `torso` stands in a
/// held stance: both held poses in `sequences` (JA's `animation.cfg`, by
/// upper-case name), or the torso a gun's shot from one (a gunner firing
/// steadily keeps breathing; the gun's aim and kick move the arms).
pub fn holding(sequences: &BTreeMap<String, Sequence>, (legs, torso): (Anim, Anim)) -> bool {
    let held = |anim: Anim| {
        sequences
            .get(anim.name())
            .is_some_and(|sequence| sequence.frame_count <= HELD_FRAMES)
    };
    let shot = matches!(torso, Anim::BOTH_ATTACK2 | Anim::BOTH_ATTACK3);
    held(legs) && (shot || held(torso))
}

/// The game's `baseitems.2da` `weaponwield` by row: 1 a stun baton, 2 a
/// one-handed blade, 3 a double-bladed or two-handed one, 4 a pistol, 5 a
/// rifle, 6 a heavy weapon (none without the game's tables).
#[derive(Default)]
pub struct Wields(Vec<Option<u8>>);

impl Wields {
    /// The column from `resolver`'s `baseitems.2da`.
    pub fn read(resolver: Option<&Resolver>) -> Wields {
        let table = resolver.and_then(|resolver| resolver.two_da("baseitems"));
        Wields(table.map_or_else(Vec::new, |table| {
            (0..table.cells.len())
                .map(|row| u8::try_from(table.int(row, "weaponwield")?).ok())
                .collect()
        }))
    }

    /// Base item `row`'s `weaponwield`.
    fn of(&self, row: u16) -> Option<u8> {
        self.0.get(usize::from(row)).copied().flatten()
    }
}

/// KOTOR's wield number (as reone numbers it, the digit of
/// `g<wield>r1`) of `weapon`, out of `sheet`'s, by its base item's
/// `weaponwield` (`Wields`): 1 a stun baton, 2 a blade (4 with a blade in
/// each hand), 3 a double-bladed or two-handed one, 5 a pistol, 7 a rifle,
/// 9 a heavy weapon, 8 bare hands; by how it is delivered where the table
/// has no row; 6 a pistol in each hand (`Sheet::pistol_pair`, reone's
/// dual-pistol stance). `Sheet::wield_number` gives the melee ones alone (the
/// scenes' rounds).
pub fn wield(weapon: Weapon, sheet: &Sheet, wields: &Wields) -> u8 {
    if sheet.pistol_pair() {
        return 6;
    }
    let blade = |weapon: &Option<Weapon>| {
        weapon.is_some_and(|w| matches!(w.delivery, Delivery::Melee | Delivery::Saber(_)))
    };
    let dual = sheet.weapons.iter().all(blade);
    match (weapon.delivery, wields.of(weapon.base_item)) {
        (Delivery::Unarmed, _) => 8,
        (_, Some(1)) | (Delivery::Baton, None) => 1,
        (_, Some(3)) | (Delivery::Saber(SaberShape::Staff), None) => 3,
        (_, Some(4)) | (Delivery::Blaster(Blaster::Pistol), None) => 5,
        (_, Some(5)) => 7,
        (_, Some(6)) => 9,
        (Delivery::Blaster(_), _) => 7,
        _ if dual => 4,
        _ => 2,
    }
}

/// A body's breathing: its wield number (`wield`), and how much of it
/// shows, as of when (sim ms).
#[derive(Default)]
pub struct Breath {
    pub wield: u8,
    weight: f32,
    last_ms: Option<f64>,
}

/// What a breathing body shows: its ready clip, how far in and how much of
/// its motion; and how far its chest leans (radians, about its forward
/// axis).
pub struct Shown<'a> {
    pub clip: Option<(&'a Animator, f32, f32)>,
    pub lean: f32,
}

impl Breath {
    /// How much of the motion shows (0 to 1).
    pub fn weight(&self) -> f32 {
        self.weight
    }

    /// At `ms`, eases toward breathing (`wanted`) or not, and gives what
    /// shows of it: `bound`'s ready stance of its wield (else its `pause1`)
    /// and the lean, each `phase` (0 to 1) of the way into its loop at 0.
    pub fn follow<'a>(
        &mut self,
        bound: &'a Bound,
        wanted: bool,
        (ms, phase): (f64, f32),
    ) -> Option<Shown<'a>> {
        let seconds = self
            .last_ms
            .map_or(0.0, |last| ((ms - last) / 1000.0) as f32)
            .clamp(0.0, 0.25);
        self.last_ms = Some(ms);
        let target = if wanted { 1.0 } else { 0.0 };
        self.weight = crate::gait::ease(self.weight, target, seconds / EASE_S);
        if self.weight <= 0.0 {
            return None;
        }
        let seconds = ms / 1000.0;
        let phase = f64::from(phase);
        let clip = [acting::READY + u16::from(self.wield), 0]
            .into_iter()
            .find_map(|animation| bound.iter().find(|(a, _)| *a == animation))
            .map(|(_, clip)| {
                let length = f64::from(clip.length());
                let at = (seconds * PACE + phase * length).rem_euclid(length);
                (clip, at as f32, self.weight * AMOUNT)
            });
        let sway = (seconds / LEAN_S + phase).fract() * TAU;
        Some(Shown {
            clip,
            lean: (LEAN_DEG.to_radians() * sway.sin()) as f32 * self.weight,
        })
    }
}

/// A body's place in the loops from its root entity: spread over 0 to 1
/// by the golden ratio.
pub fn phase(root: Entity) -> f32 {
    (root.index_u32() as f32 * 0.618_034).fract()
}
