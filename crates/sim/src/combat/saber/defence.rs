//! Blocking with the blade: whether a holder is ready to block and the
//! thing comes from inside its arc (a defence level's arc, narrower
//! against blades and thrown sabers than against bolts), which of the five
//! parries meets a point, and what a blocked bolt does (destroyed at
//! defence 1, sent on roughly where the holder looks at 0 and 2, back at
//! its shooter at 3 and above), with the cooldown before the body blocks
//! another.

use crate::math::{self, Vec3};
use crate::motion::{Anim, MotionState};
use crate::rng::SimRng;

use super::power;
use super::{Blocked, Change, Hand, Move, SaberState};
use rules::sabers::bolts;
use rules::sabers::defence;

/// What is being blocked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Against {
    Bolt,
    Blade,
    Thrown,
}

/// A blade holder as blocking reads it.
#[derive(Debug, Clone, Copy)]
pub struct Guard<'a> {
    pub saber: &'a SaberState,
    pub ps: &'a MotionState,
    pub weapon_wait_ms: i32,
    /// Its defence level against this (0 to 3, the deflect property
    /// already applied for bolts).
    pub level: u8,
    /// Attack held on its last command.
    pub attack_held: bool,
    /// A Force hand held out (JA's hand extends).
    pub hand_out: bool,
}

/// Ready and facing it: the holder blocks something arriving at `point`.
pub fn can_block(guard: &Guard, point: Vec3, against: Against, now: u64) -> bool {
    ready(guard, now)
        && in_arc(
            guard.ps.origin,
            guard.ps.view[1],
            guard.level,
            point,
            against,
        )
}

/// How closely the flat way to a point must line up with the facing for a
/// defence level to block it; no level, no arc.
fn arc_threshold(level: u8, against: Against) -> Option<f32> {
    let arc = match level {
        0 => return None,
        1 => defence::ARC[0],
        2 => defence::ARC[1],
        _ => defence::ARC[2],
    };
    Some(match against {
        Against::Bolt => arc,
        Against::Blade | Against::Thrown => arc - defence::AGAINST_BLADES,
    })
}

/// `point`, seen from above, lies inside the arc a holder at `origin`
/// facing `yaw` covers at defence `level`.
pub fn in_arc(origin: Vec3, yaw: f32, level: u8, point: Vec3, against: Against) -> bool {
    let Some(threshold) = arc_threshold(level, against) else {
        return false;
    };
    let (forward, _, _) = math::angle_vectors([0.0, yaw, 0.0]);
    math::dot(flat_way(origin, point), forward) > threshold
}

/// The unit way from `from` to `to` across the floor.
fn flat_way(from: Vec3, to: Vec3) -> Vec3 {
    let mut way = math::sub(to, from);
    way[2] = 0.0;
    math::normalize(&mut way);
    way
}

/// The blade is free to block: not attacking or about to, not swinging
/// an animation of its own, lit and in hand, the weapon not being raised,
/// the move one that blocks, and the holder neither on the bolt cooldown,
/// holding a hand out nor knocked down.
pub fn ready(guard: &Guard, now: u64) -> bool {
    let saber = guard.saber;
    let current = saber.current_move;
    if current.attack() {
        return false;
    }
    let swinging = guard
        .ps
        .torso
        .anim
        .between(Anim::BOTH_A1_T__B_, Anim::BOTH_H1_S1_BR)
        && saber.blocked == Blocked::None
        && !matches!(current, Move::READY | Move::NONE)
        && !current.parry()
        && !current.reflect();
    if swinging {
        return false;
    }
    let held_back = current.broken()
        || !saber.on
        || saber.thrown.is_some()
        || saber.change == Change::Raising
        || guard.attack_held
        || power::attacking(current, saber.blocked, guard.weapon_wait_ms)
        || (current != Move::READY && saber.blocking == super::Block::No)
        || saber.bolt_block_until >= now
        || guard.hand_out
        || saber.hand == Hand::Knockdown;
    !held_back
}

/// The parry that meets `point`: by where it is from the eye, high or low
/// and to which side (the sides open wider below the eye than above it);
/// against a bolt, the same parry's shot form.
pub fn parry_for(ps: &MotionState, point: Vec3, bolt: bool) -> Blocked {
    let eye = [ps.origin[0], ps.origin[1], ps.origin[2] + ps.eye_height];
    let (_, right, _) = math::angle_vectors([0.0, ps.view[1], 0.0]);
    let side = math::dot(right, flat_way(eye, point));
    let height = point[2] - eye[2];
    let edge = if height > 0.0 {
        defence::PARRY_EDGE[0]
    } else {
        defence::PARRY_EDGE[1]
    };
    let melee = if height > -defence::PARRY_LOW {
        if side > edge {
            Blocked::UpperRight
        } else if side < -edge {
            Blocked::UpperLeft
        } else {
            Blocked::Top
        }
    } else if side >= 0.0 {
        Blocked::LowerRight
    } else {
        Blocked::LowerLeft
    };
    if !bolt {
        return melee;
    }
    match melee {
        Blocked::UpperRight => Blocked::UpperRightShot,
        Blocked::UpperLeft => Blocked::UpperLeftShot,
        Blocked::LowerRight => Blocked::LowerRightShot,
        Blocked::LowerLeft => Blocked::LowerLeftShot,
        _ => Blocked::TopShot,
    }
}

/// What a blocked bolt does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BoltFate {
    /// Destroyed (defence 1).
    Absorbed,
    /// Sent on along this unit direction, now the defender's.
    Turned(Vec3),
}

/// The level a holder blocks bolts at: its defence, raised a level for
/// every whole stretch of the deflect property, from none to the top.
pub fn bolt_level(defence: u8, deflect: i8) -> u8 {
    let level = i32::from(defence) + i32::from(deflect) / bolts::DEFLECT_PER_LEVEL;
    level.clamp(0, 3) as u8
}

/// A level lower while the holder rises or backs away.
pub fn bolt_level_now(ps: &MotionState, back: bool, level: u8) -> u8 {
    if ps.velocity[2] > 0.0 || back {
        level.saturating_sub(1)
    } else {
        level
    }
}

/// A bolt blocked at `at` by a holder at `level`, shot from `shooter`:
/// where it goes, and when the holder's bolt cooldown ends (0 at the top
/// level, which has none).
pub fn bolt(
    ps: &MotionState,
    level: u8,
    at: Vec3,
    shooter: Vec3,
    rng: &mut SimRng,
    now: u64,
) -> (BoltFate, u64) {
    let fate = match level {
        1 => BoltFate::Absorbed,
        0 | 2 => {
            let (forward, _, _) = math::angle_vectors(ps.view);
            BoltFate::Turned(scatter(forward, bolts::DEFLECT_SPREAD, rng))
        }
        _ => {
            let mut back = math::sub(shooter, at);
            math::normalize(&mut back);
            BoltFate::Turned(scatter(back, bolts::REFLECT_SPREAD, rng))
        }
    };
    let until = if level >= 3 {
        0
    } else {
        now + bolts::COOLDOWN_MS - bolts::COOLDOWN_PER_LEVEL * u64::from(level)
    };
    (fate, until)
}

/// `way` with each component nudged by up to `spread` either side, in
/// order, then made unit again.
fn scatter(mut way: Vec3, spread: f32, rng: &mut SimRng) -> Vec3 {
    for component in &mut way {
        *component += rng.range(-spread, spread);
    }
    math::normalize(&mut way);
    way
}
