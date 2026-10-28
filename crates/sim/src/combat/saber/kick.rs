//! Kicks: the staff's kicks, the roll get-ups that kick on the way up, and
//! the hilt bash. While one plays inside its window it strikes along a flat
//! direction taken from the view (ahead, behind, to a side; the spin kick
//! sweeps round in slices, the back-and-front kick turns half way, the
//! split strikes to each side on alternate frames), with a reach and the
//! toss's push. Whether a kick floors the one it hits is decided here too.

use crate::math::{self, Vec3};
use crate::motion::is_knockdown;
use crate::motion::{Anim, AnimTable, MotionState};

use super::power;

use Anim::*;
use rules::saber_moves::kicks;

/// How far a kick reaches (units): the body's half width × a factor, JA's
/// staff kick range and a little.
pub const REACH: f32 =
    crate::motion::HALF_WIDTH * kicks::REACH_TIMES + kicks::STAFF_RANGE + kicks::REACH_PAD;
/// The kick's knockdown (ms).
pub const DOWN_MS: u64 = kicks::DOWN_MS;
/// The toss's lift (units/s).
pub const LIFT: f32 = kicks::LIFT;

/// A kick under way this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Kick {
    /// Flat unit direction.
    pub direction: Vec3,
    pub reach: f32,
    /// The toss's roll range (units/s).
    pub push: (f32, f32),
}

/// A kick's direction, reach and push.
fn strike(direction: Vec3, reach: f32, push: (f32, f32)) -> Kick {
    Kick {
        direction,
        reach,
        push,
    }
}

/// How long `anim`, at `timer` ms left, has played and has left (ms).
fn played(anims: &AnimTable, anim: Anim, timer: i32) -> (i32, i32) {
    (anims.get(anim).length_ms() - timer, timer)
}

/// A window `[from, left]`: at least `from` ms played and `left` to go.
fn open([from, left]: [i32; 2], (elapsed, remaining): (i32, i32)) -> bool {
    elapsed >= from && remaining >= left
}

/// The kick that strikes this tick, if any: the torso's hilt bash, or
/// what the legs play.
pub fn kick(ps: &MotionState, anims: &AnimTable, now: u64) -> Option<Kick> {
    let yaw = ps.view[1];
    let turned = |turn: f32| math::angle_vectors([0.0, yaw + turn, 0.0]);
    let (ahead, right, _) = turned(0.0);
    let behind = math::scale(ahead, -1.0);
    let left = math::scale(right, -1.0);
    let plain = (kicks::PUSH[0], kicks::PUSH[1]);
    let hard = (kicks::HARD_PUSH[0], kicks::HARD_PUSH[1]);

    if ps.torso.anim == BOTH_A7_HILT {
        let timing = played(anims, ps.torso.anim, ps.torso.timer);
        return open(kicks::WINDOW_MS, timing).then(|| strike(ahead, REACH, plain));
    }

    let timing = played(anims, ps.legs.anim, ps.legs.timer);
    let elapsed = timing.0;
    let within = |window: [i32; 2], direction: Vec3| {
        open(window, timing).then(|| strike(direction, REACH, plain))
    };
    match ps.legs.anim {
        BOTH_GETUP_BROLL_B | BOTH_GETUP_BROLL_F | BOTH_A7_KICK_F => within(kicks::WINDOW_MS, ahead),
        BOTH_A7_KICK_B => within(kicks::WINDOW_MS, behind),
        BOTH_A7_KICK_R => within(kicks::WINDOW_MS, right),
        BOTH_A7_KICK_L => within(kicks::WINDOW_MS, left),
        BOTH_A7_KICK_F_AIR => within(kicks::AIR_WINDOW_MS, ahead),
        BOTH_A7_KICK_B_AIR => within(kicks::AIR_WINDOW_MS, behind),
        BOTH_A7_KICK_R_AIR => within(kicks::AIR_WINDOW_MS, right),
        BOTH_A7_KICK_L_AIR => within(kicks::AIR_WINDOW_MS, left),
        BOTH_A7_KICK_S => {
            let [from, until] = kicks::SPIN_MS;
            (from <= elapsed && elapsed < until).then(|| {
                // The spin turns a step clockwise with every slice played.
                let slice = (elapsed - from) / kicks::SPIN_SLICE_MS;
                let (spun, _, _) = turned((-kicks::SPIN_TURN) * slice as f32);
                strike(spun, REACH, hard)
            })
        }
        BOTH_A7_KICK_BF => {
            let [front, back, done] = kicks::BACK_FRONT_MS;
            let direction = if front <= elapsed && elapsed < back {
                ahead
            } else if back <= elapsed && elapsed < done {
                behind
            } else {
                return None;
            };
            Some(strike(direction, REACH + kicks::BACK_FRONT_REACH, hard))
        }
        BOTH_A7_KICK_RL => {
            let frame = now / u64::from(power::JA_FRAME_MS);
            let direction = if frame % 2 == 1 { right } else { left };
            Some(strike(direction, REACH + kicks::SPLIT_REACH, hard))
        }
        _ => None,
    }
}

/// Whether a kick floors the one it hits: not one already down or
/// falling, nor one standing idle with a saber in hand.
pub fn floors(ps: &MotionState, weapon_wait_ms: i32, saber_in_hand: bool, floored: bool) -> bool {
    let idle_with_saber = weapon_wait_ms <= 0 && saber_in_hand && ps.ground.is_some();
    !(floored || is_knockdown(ps.legs.anim) || is_knockdown(ps.torso.anim) || idle_with_saber)
}
