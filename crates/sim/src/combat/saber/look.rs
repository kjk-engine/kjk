//! The server's look: which way a character's legs face and how its spine
//! and neck turn, so that its skeleton (and so its blades) is posed as the
//! game's server poses it.
//!
//! The legs face the view, turned toward the way the character travels
//! when it runs (no further than a lean, the other way when it runs
//! backward on a diagonal). The spine takes up what is left between the
//! view and the legs, and part of the view's pitch; when the torso plays
//! an animation of its own the torso's turn in that animation is taken
//! back out. The spine's turn is shared out over the two lumbar bones and
//! the thoracic; the head is brought back level and its pitch shared over
//! the thoracic, the neck and the skull. A lock's frozen pose has no look
//! at all.
//!
//! The angles go round the game's 16-bit circle, and every step is the
//! game's own arithmetic in `f32` on the sim's trigonometry, so the bits
//! (and the sim hash) match.

use crate::math::{Vec3, atan2_deg};
use crate::motion::anim::{is_flip, is_knockdown, is_leap, is_roll, is_special_move, is_spin};
use crate::motion::{Anim, MotionState};
use crate::skeleton::{Angles, BoltMatrix, LOOK_BONES, Look};
use rules::sabers::look;

const PITCH: usize = 0;
const YAW: usize = 1;
const ROLL: usize = 2;

/// The legs' yaw (degrees, the model faces it) and the look's angle
/// overrides, in `LOOK_BONES` order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Facing {
    pub legs_yaw: f32,
    pub look: Look,
}

/// What the look reads off a character besides its movement state.
#[derive(Debug, Clone, Copy)]
pub struct Doing {
    /// What its skeleton's legs and torso play.
    pub anims: [Anim; 2],
    /// Its saber move is a special (`Move::special`).
    pub special_move: bool,
    /// Its skeleton is frozen on a lock's frame.
    pub locked: bool,
    pub alive: bool,
}

/// `angle` on the game's 16-bit circle, in [0, 360).
fn circle(angle: f32) -> f32 {
    let to_steps = 65536.0_f32 / 360.0;
    let to_degrees = 360.0_f32 / 65536.0;
    ((angle * to_steps) as i32 & 0xffff) as f32 * to_degrees
}

/// `angle` on the circle, then in (-180, 180].
fn half_circle(angle: f32) -> f32 {
    let on = circle(angle);
    if on > 180.0 { on - 360.0 } else { on }
}

/// `a - b` brought into [-180, 180] a whole turn at a time.
fn difference(a: f32, b: f32) -> f32 {
    let mut d = a - b;
    while d > 180.0 {
        d -= 360.0;
    }
    while d < -180.0 {
        d += 360.0;
    }
    d
}

/// The yaw of the flat way `(x, y)`, in [0, 360); 0 for no way at all.
fn flat_yaw(x: f32, y: f32) -> f32 {
    let yaw = if x == 0.0 && y == 0.0 {
        0.0
    } else if x != 0.0 {
        atan2_deg(y, x)
    } else if y > 0.0 {
        90.0
    } else {
        270.0
    };
    if yaw < 0.0 { yaw + 360.0 } else { yaw }
}

/// The pitch (up negative) and yaw a direction points along; no roll.
fn direction_angles(v: Vec3) -> Angles {
    let (pitch, yaw) = if v[0] == 0.0 && v[1] == 0.0 {
        (if v[2] > 0.0 { 90.0 } else { 270.0 }, 0.0)
    } else {
        let flat = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let pitch = atan2_deg(v[2], flat);
        let pitch = if pitch < 0.0 { pitch + 360.0 } else { pitch };
        (pitch, flat_yaw(v[0], v[1]))
    };
    [-pitch, yaw, 0.0]
}

/// How far the legs turn from the view toward the way the body travels:
/// the short way round, no more than the lean, and the other way when the
/// keys run backward on a diagonal.
fn travel_turn(head_yaw: f32, velocity: Vec3, keys: u8) -> f32 {
    let t = (head_yaw - flat_yaw(-velocity[0], -velocity[1])).rem_euclid(360.0);
    let from = if t < 180.0 { t } else { t - 360.0 };
    let off = from.abs();
    let behind = off < 90.0;
    let mut lean = (if behind { off } else { 180.0 - off }).min(look::LEAN);
    if behind && matches!(keys, 3..=5) {
        lean = -lean;
    }
    if from < 0.0 { -lean } else { lean }
}

/// One of the four lock breaks the old locks play.
fn lock_break(anim: Anim) -> bool {
    matches!(
        anim,
        Anim::BOTH_BF1BREAK
            | Anim::BOTH_BF2BREAK
            | Anim::BOTH_CWCIRCLEBREAK
            | Anim::BOTH_CCWCIRCLEBREAK
    )
}

/// A roll, the roll get-ups among them.
fn rolling(anim: Anim) -> bool {
    is_roll(anim)
        || matches!(
            anim,
            Anim::BOTH_GETUP_BROLL_B
                | Anim::BOTH_GETUP_BROLL_F
                | Anim::BOTH_GETUP_BROLL_L
                | Anim::BOTH_GETUP_BROLL_R
        )
}

/// An animation that carries its own turn of the whole body: a flip, a
/// spin, a special jump or attack, a knockdown.
fn whole_body(anim: Anim) -> bool {
    is_flip(anim)
        || matches!(
            anim,
            Anim::BOTH_FLIP_F | Anim::BOTH_FLIP_B | Anim::BOTH_FLIP_L | Anim::BOTH_FLIP_R
        )
        || is_spin(anim)
        || is_leap(anim)
        || is_special_move(anim)
        || is_knockdown(anim)
}

/// Where a character's legs face and how its spine, neck and head turn.
/// `motion` poses the skeleton for the torso's own turn; it is asked at
/// most once, and only when the torso plays something of its own.
pub fn facing(
    ps: &MotionState,
    doing: Doing,
    motion: impl FnOnce() -> Option<BoltMatrix>,
) -> Facing {
    let view = ps.view;
    let [legs, torso] = doing.anims;
    if doing.locked || lock_break(legs) || lock_break(torso) {
        return Facing {
            legs_yaw: circle(view[YAW]),
            look: [[0.0; 3]; LOOK_BONES.len()],
        };
    }

    let head_yaw = circle(view[YAW]);
    let still = rolling(legs) || doing.special_move;
    let moving = !still && ps.ground.is_some() && (ps.velocity[0] != 0.0 || ps.velocity[1] != 0.0);
    let keys = if doing.alive { ps.push_dir } else { 0 };
    let legs_yaw = if moving {
        circle(head_yaw + travel_turn(head_yaw, ps.velocity, keys))
    } else {
        head_yaw
    };

    let mut spine: Angles = [
        view[PITCH] * look::SPINE_PITCH,
        half_circle(view[YAW] - legs_yaw),
        0.0,
    ];

    let correcting = doing.alive
        && legs != torso
        && !whole_body(legs)
        && !whole_body(torso)
        && !is_roll(legs)
        && !doing.special_move;
    if correcting && let Some(bolt) = motion() {
        let back = |v: Vec3| [-v[0], -v[1], -v[2]];
        let mut turn = direction_angles(back(bolt.axes[1]));
        turn[ROLL] = -direction_angles(back(bolt.axes[0]))[PITCH];
        for (angle, turned) in spine.iter_mut().zip(turn) {
            *angle = half_circle(*angle - half_circle(turned));
        }
    }

    let shared = |[first, rest]: [f32; 2], of: Angles| [of[0] * first, of[1] * rest, of[2] * rest];
    let lower = shared(look::LOWER_LUMBAR, spine);
    let upper = shared(look::UPPER_LUMBAR, spine);
    let mut thoracic = spine.map(|angle| angle * look::THORACIC);

    let mut head: Angles = [0.0; 3];
    for (axis, angle) in head.iter_mut().enumerate() {
        let level = if axis == PITCH { 0.0 } else { view[axis] };
        let back = difference(half_circle(level), half_circle(view[axis]));
        *angle = back.clamp(look::HEAD_REACH[2 * axis], look::HEAD_REACH[2 * axis + 1]);
    }

    let own = shared(look::THORACIC_HEAD, head);
    for (angle, own) in thoracic.iter_mut().zip(own) {
        *angle = if *angle != 0.0 {
            (*angle + own) * 0.5
        } else {
            own
        };
    }

    Facing {
        legs_yaw,
        look: [
            lower,
            upper,
            thoracic,
            shared(look::CERVICAL, head),
            shared(look::CRANIUM, head),
        ],
    }
}
