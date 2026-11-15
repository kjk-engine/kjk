//! The sides of targeted blocks (the targeted blocks spec,
//! `docs/superpowers/specs/2026-11-15-targeted-blocks-design.md`). With
//! targeted blocks on, a perfect block needs the player's crosshair on the
//! side of the attacker that the swing comes from: left, right or above.
//!
//! The block rule and the reticule both take the crosshair line from
//! `crosshair_line`, so the lit mark and the rule always agree.

use super::unblockable;
use crate::combat::mind;
use crate::combat::saber::{Move, Quad};
use crate::math::{self, Vec3};
use crate::motion::MotionState;
use rules::duel::targeted;

/// The side a swing comes from, or the side of the attacker the crosshair
/// is on, as the blocker sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Above,
}

/// The crosshair line of `player` (where it starts, and its unit way): the
/// line its blaster shots follow (`world::firing`), before the blaster's
/// kick. It starts at `mind::crosshair`, which the command's `aim` moves to
/// the camera's line, and runs along the view.
pub fn crosshair_line(player: &MotionState) -> (Vec3, Vec3) {
    let (way, ..) = math::angle_vectors(player.view);
    (mind::crosshair(player), way)
}

/// The side of the attacker's swing as the blocker sees it. None: no side.
///
/// A start or a transition is read as the attack it leads into, which
/// begins where it ends, so the side is known from the start of the swing.
/// The top quadrant is above. The other quadrants on the attacker's left or
/// right give a direction along the attacker's right vector. That direction
/// is the blocker's right when its dot product with the blocker's right
/// vector is above `targeted::SIDE_ON`, and the blocker's left when it is
/// below minus that. The bottom quadrant, an attacker standing side-on, and
/// a move that only a perfect block stops have no side.
pub fn swing_side(mv: Move, attacker_yaw: f32, blocker_yaw: f32) -> Option<Side> {
    let mv = if mv.start() || mv.transition() {
        mv.data().chain_attack
    } else {
        mv
    };
    if unblockable(mv) {
        return None;
    }
    let toward_right = match mv.data().start {
        Quad::T => return Some(Side::Above),
        Quad::B => return None,
        Quad::TL | Quad::L | Quad::BL => -1.0,
        Quad::TR | Quad::R | Quad::BR => 1.0,
    };
    let right_of = |yaw: f32| math::angle_vectors([0.0, yaw, 0.0]).1;
    let along = toward_right * math::dot(right_of(attacker_yaw), right_of(blocker_yaw));
    if along > targeted::SIDE_ON {
        Some(Side::Right)
    } else if along < -targeted::SIDE_ON {
        Some(Side::Left)
    } else {
        None
    }
}

/// The side of the attacker that the crosshair line is on. None: no side.
///
/// The offsets are the yaw and the pitch between `line_way` and the way
/// from `line_from` to `chest`.
pub fn crosshair_side(line_from: Vec3, line_way: Vec3, chest: Vec3) -> Option<Side> {
    let to = math::sub(chest, line_from);
    let mut ahead = [line_way[0], line_way[1], 0.0];
    math::normalize(&mut ahead);
    let left = [-ahead[1], ahead[0], 0.0];
    let rise = |way: Vec3| math::atan2_deg(way[2], math::length([way[0], way[1], 0.0]));
    side_of_offsets(
        math::atan2_deg(math::dot(to, left), math::dot(to, ahead)),
        rise(line_way) - rise(to),
    )
}

/// The side of a crosshair `right_deg` to the right of the chest and
/// `up_deg` above it (degrees, negative for left and below).
///
/// Both offsets under `targeted::DEAD_DEG` put the crosshair on the
/// attacker, with no side. A crosshair above the chest whose pitch offset is
/// at least its yaw offset is above. Otherwise a yaw offset of at least
/// `targeted::DEAD_DEG` is left or right, and a crosshair below the chest
/// has no side.
pub fn side_of_offsets(right_deg: f32, up_deg: f32) -> Option<Side> {
    let (yaw, pitch) = (right_deg.abs(), up_deg.abs());
    if yaw < targeted::DEAD_DEG && pitch < targeted::DEAD_DEG {
        None
    } else if up_deg > 0.0 && pitch >= yaw {
        Some(Side::Above)
    } else if yaw >= targeted::DEAD_DEG {
        Some(if right_deg > 0.0 {
            Side::Right
        } else {
            Side::Left
        })
    } else {
        None
    }
}

/// The attacker's chest: `targeted::CHEST_PERCENT` of its eye height above
/// its origin.
pub fn chest(origin: Vec3, eye_height: f32) -> Vec3 {
    let [x, y, z] = origin;
    [
        x,
        y,
        z + eye_height * targeted::CHEST_PERCENT as f32 / 100.0,
    ]
}

/// The side of the swing of `attacker`, playing `mv`, and the side of
/// `attacker` that the crosshair of `blocker` is on, in that order. The
/// block rule and the reticule both read the sides here.
pub fn sides(
    blocker: &MotionState,
    attacker: &MotionState,
    mv: Move,
) -> (Option<Side>, Option<Side>) {
    let (from, way) = crosshair_line(blocker);
    (
        swing_side(mv, attacker.view[1], blocker.view[1]),
        crosshair_side(from, way, chest(attacker.origin, attacker.eye_height)),
    )
}

/// Whether a fighter playing `mv` has a swing under way: an attack, or the
/// start or the transition that leads into one.
pub fn swinging(mv: Move) -> bool {
    mv.attack() || mv.start() || mv.transition()
}

/// One hostile fighter the reticule offers as a possible threat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Near {
    /// How far the fighter stands from the player (units).
    pub distance: f32,
    /// Whether the fighter stands in front of the player (`duel::in_front`).
    pub in_front: bool,
    /// When the fighter's swing began, while one is under way.
    pub swing_began: Option<u64>,
}

/// The index of the threat among `near`. None: there is no threat.
///
/// Only a fighter in front of the player and within `threat_range` can be
/// the threat. Among those whose swing is under way, the one whose swing
/// began first is the threat. With none swinging, the nearest is.
pub fn threat(near: &[Near]) -> Option<usize> {
    let range = threat_range();
    (near.iter().enumerate())
        .filter(|(_, near)| near.in_front && near.distance <= range)
        .min_by(|(_, a), (_, b)| {
            let began = |near: &Near| (near.swing_began.is_none(), near.swing_began);
            began(a)
                .cmp(&began(b))
                .then(a.distance.total_cmp(&b.distance))
        })
        .map(|(index, _)| index)
}

/// Whether a block in the open window is perfect: the swing has no side, or
/// the crosshair is on the swing's side.
pub fn aimed(swing: Option<Side>, crosshair: Option<Side>) -> bool {
    swing.is_none() || swing == crosshair
}

/// The perfect block window (ms) of a player with targeted blocks on:
/// `targeted::WINDOW_PERCENT` of the normal window `normal_ms`.
pub fn window_ms(normal_ms: u64) -> u64 {
    normal_ms * targeted::WINDOW_PERCENT / 100
}

/// How far away (units) a hostile fighter can be and still be the threat
/// whose swing side the reticule shows (`targeted::THREAT_M`).
pub fn threat_range() -> f32 {
    targeted::THREAT_M / collision::UNIT
}
