//! The Force jump: while jump stays held after leaving the ground, a body
//! with the power keeps rising toward its level's height, paying Force
//! points as it goes, and turns its jump into a flip or a directional leap.

use super::anim::{Parts, is_leap, is_special_move, play_flags};
use super::{Anim, JUMP_SPEED, Move, motion_flags};
use crate::combat::saber::Kit;
use crate::math;

/// Heights above the start by level (0: a normal jump), and strengths by
/// level from 1.
const HEIGHT: [f32; 4] = rules::force::jump::HEIGHTS;
const STRENGTH: [f32; 3] = rules::force::jump::STRENGTHS;

/// The kit's Force jump level, 0 to 3: anyone with a Force pool jumps at
/// level 1 at least.
fn jump_level(kit: &Kit) -> u8 {
    let least = u8::from(kit.max_fp > 0);
    kit.force_jump.max(least).min(3)
}

/// The leap's animation as it starts, from the keys or, at level 2 and up,
/// from which way the body already moves against its flat facing.
fn leap_anim(
    forward: i8,
    right: i8,
    legs: Anim,
    level: u8,
    yaw: f32,
    velocity: [f32; 3],
) -> Option<Anim> {
    let flipping = matches!(
        legs,
        Anim::BOTH_FLIP_F | Anim::BOTH_FLIP_B | Anim::BOTH_FLIP_R | Anim::BOTH_FLIP_L
    );
    if (forward != 0 || right != 0) && !flipping {
        return Some(if forward > 0 {
            Anim::BOTH_FLIP_F
        } else if forward < 0 {
            Anim::BOTH_FLIP_B
        } else if right > 0 {
            Anim::BOTH_FLIP_R
        } else {
            Anim::BOTH_FLIP_L
        });
    }
    if level < 2 {
        return None;
    }
    let (ahead, side, _) = math::angle_vectors([0.0, yaw, 0.0]);
    let along = math::dot(ahead, velocity);
    let across = math::dot(side, velocity);
    if across.abs() > along.abs() * 1.5 {
        if across > 150.0 {
            Some(Anim::BOTH_FORCEJUMPRIGHT1)
        } else if across < -150.0 {
            Some(Anim::BOTH_FORCEJUMPLEFT1)
        } else {
            None
        }
    } else if along > 150.0 {
        Some(Anim::BOTH_FORCEJUMP1)
    } else if along < -150.0 {
        Some(Anim::BOTH_FORCEJUMPBACK1)
    } else {
        None
    }
}

/// A directional leap's landing pose, once its animation has run out.
fn landing_pose(legs: Anim) -> Option<Anim> {
    match legs {
        Anim::BOTH_FORCEJUMP1 => Some(Anim::BOTH_FORCELAND1),
        Anim::BOTH_FORCEJUMPBACK1 => Some(Anim::BOTH_FORCELANDBACK1),
        Anim::BOTH_FORCEJUMPLEFT1 => Some(Anim::BOTH_FORCELANDLEFT1),
        Anim::BOTH_FORCEJUMPRIGHT1 => Some(Anim::BOTH_FORCELANDRIGHT1),
        _ => None,
    }
}

impl Move<'_, '_> {
    /// The Force jump's part of the jump check: true when it took the step
    /// (the body is rising on the Force), so no plain jump starts.
    pub(super) fn force_jump(&mut self) -> bool {
        let (cost_above, cost_points) = (
            rules::force::jump::COST_ABOVE,
            rules::force::jump::COST_POINTS,
        );

        let Some(arms) = self.arms.as_deref_mut() else {
            return false;
        };
        let level = jump_level(arms.kit);
        let max_fp = i32::from(arms.kit.max_fp);
        let fighter = &mut *arms.fighter;
        let now = self.time_ms;
        let ps = &mut *self.ps;

        if ps.ground.is_some() || ps.origin[2] < ps.takeoff_z {
            fighter.force.jumping = false;
        }
        if fighter.force.jumping && fighter.force.jump_paid_at < now {
            let rate = cost_above
                .iter()
                .zip(cost_points)
                .find(|&(&above, _)| ps.velocity[2] > above)
                .map_or(0, |(_, points)| points);
            let points = rate / i32::from(level.max(1));
            fighter.fp = (fighter.fp - points * max_fp).max(0);
            fighter.force.jump_paid_at =
                now + rules::force::jump::PAY_EVERY_MS[usize::from(level >= 2)];
        }

        let legs = ps.legs.anim;
        let rising = ps.gravity > 0.0
            && ps.ground.is_none()
            && ps.flags & motion_flags::JUMP_PRESSED != 0
            && level > 0
            && ps.velocity[2] > 0.0
            && !is_leap(legs)
            && !is_special_move(legs)
            && !fighter.saber.in_special()
            && !fighter.force.stunned(now);
        if !rising {
            return false;
        }

        let height = ps.origin[2] - ps.takeoff_z;
        let top = HEIGHT[usize::from(level)];
        let holding = fighter.fp > 0 && self.cmd.up >= rules::movement::keys::JUMP_AXIS;
        let armed = fighter.weapon_wait_ms > 0;
        if (height <= HEIGHT[0] || holding) && height < top && ps.takeoff_z != 0.0 {
            if height > HEIGHT[0] {
                let anim = if fighter.force.jumping {
                    if ps.legs.timer < 1 {
                        landing_pose(legs)
                    } else {
                        None
                    }
                } else {
                    fighter.force.jumping = true;
                    leap_anim(
                        self.cmd.forward,
                        self.cmd.right,
                        legs,
                        level,
                        ps.view[1],
                        ps.velocity,
                    )
                };
                if let Some(anim) = anim {
                    let parts = if armed { Parts::LOWER } else { Parts::WHOLE };
                    self.animator.play(
                        ps,
                        parts,
                        anim,
                        play_flags::OVER_ANY | play_flags::HOLD_FULL,
                    );
                }
            }
            let strength = STRENGTH[usize::from(level) - 1];
            ps.velocity[2] = (top - height) / top * strength / 10.0 + JUMP_SPEED;
            ps.flags |= motion_flags::JUMP_PRESSED;
        } else if ps.velocity[2] > JUMP_SPEED {
            ps.velocity[2] = JUMP_SPEED;
        }
        self.cmd.up = 0;
        true
    }
}
