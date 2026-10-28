//! The saber away from the hand (`docs/COMBAT.md`): thrown, flying
//! out and back, or knocked out of the air (or out of a hand) to fall and
//! lie until it may be called back.
//!
//! - Thrown with the alternate button (move_body's saber step: any style but
//!   the staff, the weapon ready, KOTOR's Throw Lightsaber known (level 1,
//!   Advanced level 2), its `spells.2da` cost paid, `CLEAR_AHEAD` clear
//!   ahead). It leaves the hilt at `OUT_SPEED` along the view; the Force
//!   pool does not refill while it flies.
//! - Out: after `HOLD_MS` it turns back when alternate is let go, after
//!   `MAX_OUT_MS`, beyond `RANGE_PER_LEVEL` × level from its owner, when the
//!   owner cannot use the Force or after a hit or block. At level 2 it is
//!   re-aimed every `AIM_L2_MS` at `AIMED_SPEED` at the point the owner's
//!   view meets the world (level 3, which K1 has no row for, every
//!   `AIM_L3_MS` at whatever it meets).
//! - Back: toward the owner's hand (`Thrown::home_speed`), re-aimed every
//!   `AIM_BACK_MS`, slower close in; caught within `CATCH`, the weapon ready
//!   `CATCH_MS` later.
//! - Hits: anyone whose origin is within `HIT_OUT` (`HIT_BACK` on the way
//!   back) in clear sight but its owner, once per `HIT_EVERY_MS`: a block
//!   (C6's arc against blades) parries and knocks it down if the blocker's
//!   defence beats the throw's level (sometimes when equal,
//!   `blocked_down`), else it turns back; otherwise it hits as the thrown
//!   saber's attack (`CADENCE_MS`, a delegated number standing for JA's
//!   30) and turns back.
//! - Down: it falls, and lies; after `RETRIEVE_MS` its owner holding attack
//!   calls it back, after `AUTO_RETURN_MS` it comes back by itself.
//!
//! Stand-ins (delegated decisions): the saber is a point with a box
//! (`HALF_SIZE`), not a spinning blade (JA's flying blade also cuts as it
//! goes); a falling saber stops where it lands instead of bouncing. The
//! numbers are the book's (`rules::saber_throw`).

use crate::math::{self, Vec3};
use crate::wire::{DecodeError, Reader, Writer};
use rules::saber_throw::back;
use rules::saber_throw::down;
use rules::saber_throw::hits;
use rules::saber_throw::out;

/// Where a saber away from the hand is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flight {
    /// Thrown this tick: the server puts it at the hilt.
    Leaving,
    Out,
    Back,
    /// Knocked out of the air or out of a hand.
    Falling,
    Lying,
}

const FLIGHTS: [Flight; 5] = [
    Flight::Leaving,
    Flight::Out,
    Flight::Back,
    Flight::Falling,
    Flight::Lying,
];

/// A saber away from the hand (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Thrown {
    pub flight: Flight,
    /// Where it is (units) and how it moves (units/s).
    pub at: Vec3,
    pub velocity: Vec3,
    /// When the flight began (sim ms): thrown, turned back, knocked down,
    /// landed.
    pub since: u64,
    /// When it is next aimed (sim ms).
    pub aim_at: u64,
    /// When it may hit or be blocked again (sim ms).
    pub hit_until: u64,
    /// The throw's level (1, 2; K1's rows).
    pub level: u8,
}

/// Throw speeds (units/s).
pub const OUT_SPEED: f32 = out::OUT_SPEED;
pub const AIMED_SPEED: f32 = out::AIMED_SPEED;
/// How far a throw reaches per level (units).
pub const RANGE_PER_LEVEL: f32 = out::RANGE_PER_LEVEL;
/// Out at least this long before letting go turns it back, at most the
/// other (ms).
pub const HOLD_MS: u64 = out::HOLD_MS;
pub const MAX_OUT_MS: u64 = out::MAX_OUT_MS;
/// Re-aim periods (ms): level 2, level 3, on the way back.
pub const AIM_L2_MS: u64 = out::AIM_L2_MS;
pub const AIM_L3_MS: u64 = out::AIM_L3_MS;
pub const AIM_BACK_MS: u64 = back::AIM_BACK_MS;
/// Caught within this of the hand (units); the weapon is ready so long
/// after (ms).
pub const CATCH: f32 = back::CATCH;
pub const CATCH_MS: i32 = back::CATCH_MS;
/// It hits within these of a character's origin, out and back (units).
pub const HIT_OUT: f32 = hits::HIT_OUT;
pub const HIT_BACK: f32 = hits::HIT_BACK;
/// One hit or block per this long (ms).
pub const HIT_EVERY_MS: u64 = hits::HIT_EVERY_MS;
/// A hit is worth the thrown saber's roll at this cadence (C2; ms).
pub const CADENCE_MS: u32 = hits::CADENCE_MS;
/// Down: not recalled before this, back by itself after that (ms).
pub const RETRIEVE_MS: u64 = down::RETRIEVE_MS;
pub const AUTO_RETURN_MS: u64 = down::AUTO_RETURN_MS;
/// How fast a knocked saber falls away (units/s) and falls (units/s²).
pub const KNOCKED_SPEED: f32 = down::KNOCKED_SPEED;
pub const GRAVITY: f32 = down::GRAVITY;
/// The clear space a throw needs ahead (units).
pub const CLEAR_AHEAD: f32 = out::CLEAR_AHEAD;
/// Its box (units either side).
pub const HALF_SIZE: f32 = hits::HALF_SIZE;

impl Thrown {
    pub(crate) const BLANK: Thrown = Thrown {
        flight: Flight::Leaving,
        at: [0.0; 3],
        velocity: [0.0; 3],
        since: 0,
        aim_at: 0,
        hit_until: 0,
        level: 0,
    };

    /// Thrown at `now` at `level`: placed by the server this tick.
    pub fn leaving(level: u8, now: u64) -> Thrown {
        Thrown {
            level,
            since: now,
            ..Thrown::BLANK
        }
    }

    /// Knocked away from `at` along `velocity` at `now` (a disarm, a
    /// thrown saber struck down).
    pub fn knocked(at: Vec3, velocity: Vec3, now: u64) -> Thrown {
        Thrown {
            flight: Flight::Falling,
            at,
            velocity,
            since: now,
            ..Thrown::BLANK
        }
    }

    /// In the air under its owner's power (the pool waits).
    pub fn flying(&self) -> bool {
        matches!(self.flight, Flight::Leaving | Flight::Out | Flight::Back)
    }

    /// Knocked away (it hits and blocks nothing).
    pub fn down(&self) -> bool {
        matches!(self.flight, Flight::Falling | Flight::Lying)
    }

    /// The range it hits within.
    pub fn reach(&self) -> f32 {
        if self.flight == Flight::Back {
            HIT_BACK
        } else {
            HIT_OUT
        }
    }

    /// How fast a saber `distance` units from its owner's hand flies home
    /// (units/s): faster at level 3, slowed by the nearest band it is within.
    /// A distance within no band (or NaN) is not slowed.
    pub fn home_speed(level: u8, distance: f32) -> f32 {
        let base = if level >= 3 {
            back::HOME_SPEED[1]
        } else {
            back::HOME_SPEED[0]
        };
        let slowdown = back::SLOWER_WITHIN
            .iter()
            .zip(back::SLOWER)
            .find(|&(&within, _)| distance < within)
            .map_or(0.0, |(_, slower)| slower);
        base - slowdown
    }

    /// Turned toward `target` at `speed`.
    pub fn steer(&mut self, target: Vec3, speed: f32) {
        let mut way = math::sub(target, self.at);
        math::normalize(&mut way);
        self.velocity = math::scale(way, speed);
    }

    /// Turned back at `now`.
    pub fn turn_back(&mut self, now: u64) {
        if self.flight == Flight::Out || self.flight == Flight::Leaving {
            self.flight = Flight::Back;
            self.since = now;
            self.aim_at = now;
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u8(self.flight as u8);
        for value in self.at.iter().chain(&self.velocity) {
            out.f32(*value);
        }
        out.u64(self.since);
        out.u64(self.aim_at);
        out.u64(self.hit_until);
        out.u8(self.level);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Thrown, DecodeError> {
        let at = input.at();
        let flight = FLIGHTS
            .get(usize::from(input.u8()?))
            .copied()
            .ok_or(input.invalid("saber flight", at))?;
        let mut values = [0.0f32; 6];
        for value in &mut values {
            *value = input.f32()?;
        }
        Ok(Thrown {
            flight,
            at: [values[0], values[1], values[2]],
            velocity: [values[3], values[4], values[5]],
            since: input.u64()?,
            aim_at: input.u64()?,
            hit_until: input.u64()?,
            level: input.u8()?,
        })
    }
}

/// The throw level a character's powers give (K1's `spells.2da`: Throw
/// Lightsaber 49 is level 1, Advanced 4 level 2), and the row it pays.
pub fn level<const N: usize>(powers: &crate::combat::sheet::Rows<N>) -> Option<(u8, u16)> {
    if powers.has(4) {
        Some((2, 4))
    } else if powers.has(49) {
        Some((1, 49))
    } else {
        None
    }
}

/// Whether a throw of `level` blocked by a defence of `defence` is knocked
/// out of the air: always by a better defence, on a `roll_1_10` within
/// `KNOCK_ODDS` by an equal one, never by a lesser.
pub fn blocked_down(defence: u8, level: u8, roll_1_10: i32) -> bool {
    match defence.cmp(&level) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Equal => roll_1_10 <= hits::KNOCK_ODDS,
        std::cmp::Ordering::Less => false,
    }
}
