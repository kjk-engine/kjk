//! The sides of targeted blocks (the targeted blocks spec,
//! `docs/superpowers/specs/2026-11-15-targeted-blocks-design.md`, and the
//! block arcs spec). With targeted blocks on, a perfect block needs the
//! player's last camera turn to point toward the side that the swing comes
//! from: left, right or above.
//!
//! The block rule and the arcs both read the side of the turn from
//! `aim_side`, so the arcs and the rule always agree.

use super::unblockable;
use crate::combat::saber::{Move, Quad};
use crate::math;
use crate::motion::MotionState;
use crate::wire::{DecodeError, Reader, Writer};
use rules::duel::targeted;

/// The side a swing comes from, or the side a camera turn points toward,
/// as the blocker sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
    Above,
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

/// The side of a camera turn whose direction is `deg` degrees clockwise
/// from straight up, from 0 up to 360. Right runs from 45 up to 135
/// degrees, with 45 and without 135. Left runs from 225 up to 315, without
/// 225 and with 315, so that left and right mirror each other. Above is the
/// rest of the top. A turn from 135 to 225 degrees points down and has no
/// side.
pub fn aim_side(deg: f32) -> Option<Side> {
    match deg {
        deg if (45.0..135.0).contains(&deg) => Some(Side::Right),
        deg if deg > 225.0 && deg <= 315.0 => Some(Side::Left),
        deg if (135.0..=225.0).contains(&deg) => None,
        _ => Some(Side::Above),
    }
}

/// How long one part of the camera turn lasts (ms). A part is one step at
/// the sim's own rate (`rate::TICK_HZ`).
const PART_MS: u64 = 1000 / crate::rate::TICK_HZ as u64;

/// How many parts of the camera turn a body keeps. They cover
/// `targeted::TURN_MS`, rounded up to whole parts of `PART_MS`, so a turn
/// time that is not a whole number of parts is never cut short.
const PARTS: usize = targeted::TURN_MS.div_ceil(PART_MS) as usize;
const _: () = assert!(PARTS > 0, "the camera turn's time holds at least one part");

/// The camera turns of a body the player controls while it holds block,
/// and the direction of its last turn. Targeted blocks read that direction
/// as the player's aim.
///
/// Each step adds the change of the command's view angles to the part of
/// the turn that the step's end falls in. The parts cover the last
/// `targeted::TURN_MS`, rounded up to whole parts. When their sum is longer
/// than `targeted::TURN_DEG`, its direction becomes the aim. A shorter sum
/// leaves the aim as it was.
///
/// A step counts only when the body's keys also acted through it on the
/// step before (`keyed`). The command's angles before a takeover are another
/// mind's, so the change on the first step of a takeover is no turn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turn {
    /// The direction of the last turn, in degrees clockwise from straight
    /// up, from 0 up to 360. A turn up is 0, to the right 90, down 180 and
    /// to the left 270. None until the camera turns.
    pub aim: Option<f32>,
    /// The turn in each part, to the right and up, in the command's units
    /// (65 536 to the whole turn). A part is kept at its number modulo
    /// `PARTS`.
    parts: [[i32; 2]; PARTS],
    /// The number of the part that the last step went into. It is the
    /// step's end in sim time divided by `PART_MS`.
    part: u64,
    /// A seat's keys acted through the body on its last step, with targeted
    /// blocks on.
    pub keyed: bool,
}

impl Default for Turn {
    fn default() -> Self {
        Turn::NONE
    }
}

impl Turn {
    /// No turn and no aim. A body that no seat's keys act through, or that
    /// targeted blocks do not apply to, has this turn.
    pub const NONE: Turn = Turn {
        aim: None,
        parts: [[0; 2]; PARTS],
        part: 0,
        keyed: false,
    };

    /// No turn and no aim, for a body that a seat's keys act through and
    /// that does not hold block.
    pub const KEYED: Turn = Turn {
        keyed: true,
        ..Turn::NONE
    };

    /// The step that ends at `now` (sim ms) turned the command's view
    /// angles from `from` to `to` (pitch, yaw and roll, 65 536 to the
    /// whole turn). The angles wrap, so the change is taken the short way
    /// across the wrap.
    pub fn step(&mut self, now: u64, from: [i16; 3], to: [i16; 3]) {
        let part = now / PART_MS;
        // The parts that left the window are emptied. Time that went back
        // (a world loaded from an older snapshot) empties them all.
        let gone = part
            .checked_sub(self.part)
            .map_or(PARTS as u64, |gone| gone.min(PARTS as u64));
        for each in 1..=gone {
            self.parts[((self.part + each) % PARTS as u64) as usize] = [0; 2];
        }
        self.part = part;
        // The pitch grows looking down, and the yaw grows turning left.
        let changed = |axis: usize| i32::from(to[axis].wrapping_sub(from[axis]));
        let slot = &mut self.parts[(part % PARTS as u64) as usize];
        *slot = [slot[0] - changed(1), slot[1] - changed(0)];
        let [right, up] = (self.parts.iter()).fold([0i64; 2], |[right, up], [r, u]| {
            [right + i64::from(*r), up + i64::from(*u)]
        });
        let least = f64::from(targeted::TURN_DEG) * 65_536.0 / 360.0;
        if (right * right + up * up) as f64 > least * least {
            let deg = math::atan2_deg(right as f32, up as f32);
            // A negative angle is turned into the same angle from 0 up to
            // 360. The remainder keeps an angle a hair under 0, which
            // rounds to 360, at 0.
            self.aim = Some(if deg < 0.0 {
                (deg + 360.0) % 360.0
            } else {
                deg
            });
        }
    }

    /// The side the last turn points toward (`aim_side`). It is None before
    /// the first turn, and after a turn down.
    pub fn side(&self) -> Option<Side> {
        self.aim.and_then(aim_side)
    }

    /// One byte for no turn: 0 for `Turn::NONE` and 2 for `Turn::KEYED`.
    /// Otherwise a byte 1, whether there is an aim, the aim, the part's
    /// number and every part. A turn written in full is always keyed.
    pub(crate) fn encode(&self, out: &mut Writer) {
        for (tag, blank) in [(0, Turn::NONE), (2, Turn::KEYED)] {
            if *self == blank {
                out.u8(tag);
                return;
            }
        }
        out.u8(1);
        out.u8(u8::from(self.aim.is_some()));
        out.f32(self.aim.unwrap_or(0.0));
        out.u64(self.part);
        for [right, up] in self.parts {
            out.u32(right as u32);
            out.u32(up as u32);
        }
    }

    /// A turn as `encode` wrote it. A snapshot older than targeted blocks
    /// keeps none, and reads as `Turn::NONE`.
    pub(crate) fn decode(input: &mut Reader) -> Result<Turn, DecodeError> {
        if !input.since(crate::since::snapshot::FEAT_TARGETED_BLOCKS) {
            return Ok(Turn::NONE);
        }
        let at = input.at();
        match input.u8()? {
            0 => return Ok(Turn::NONE),
            2 => return Ok(Turn::KEYED),
            1 => {}
            _ => return Err(input.invalid("camera turn", at)),
        }
        let at = input.at();
        let aimed = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("camera turn aim", at)),
        };
        let aim = input.f32()?;
        let mut turn = Turn {
            aim: aimed.then_some(aim),
            part: input.u64()?,
            ..Turn::KEYED
        };
        for slot in &mut turn.parts {
            *slot = [input.u32()? as i32, input.u32()? as i32];
        }
        Ok(turn)
    }
}

/// The side of the swing of `attacker`, playing `mv`, and the side that the
/// last camera turn `turn` of `blocker` points toward, in that order. The
/// block rule and the arcs both read the sides here.
pub fn sides(
    blocker: &MotionState,
    turn: &Turn,
    attacker: &MotionState,
    mv: Move,
) -> (Option<Side>, Option<Side>) {
    (
        swing_side(mv, attacker.view[1], blocker.view[1]),
        turn.side(),
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
/// the last camera turn points toward the swing's side.
pub fn aimed(swing: Option<Side>, turned: Option<Side>) -> bool {
    swing.is_none() || swing == turned
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
