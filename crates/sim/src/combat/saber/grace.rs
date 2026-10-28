//! Defender grace (`docs/COMBAT.md`, C19). A remote player
//! sees a blow wind up as late as its view lags, so a blade's hit on one who
//! could block it (a sweep of a move that is not a special) is held for
//! g = min(its view lag, `GRACE_MS`), in ticks. If
//! during the hold a command of its comes that was made while it looked at
//! the world before the hit (the command's tick less its view lag is before
//! the hit's tick) and leaves it blocking the blow by state (C6, the test the
//! sweep makes), the hit becomes a block; otherwise it lands when the hold
//! ends. Only blocks get grace, never movement: where the blow met the body
//! stays where it met it.
//!
//! Held hits are the defender's (`Fighter::grace`), so they ride in the
//! snapshot: a save or a prediction's restore during a hold keeps them. A
//! fighter holds `HELD`; a hit held when all are taken lands the oldest at
//! once. A player on the host (no view lag) holds nothing, so solo and
//! lockstep runs are as they were.

use crate::math::Vec3;
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

use super::Style;

/// The longest hold (ms).
pub const GRACE_MS: u16 = rules::sabers::lag::GRACE_MS;
/// Hits a fighter holds at once.
pub const HELD: usize = 4;

/// A blade's hit held for its target (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Held {
    /// The striker, and the tick the hit would have landed.
    pub by: SimId,
    pub tick: u64,
    /// The ticks it is held: it lands at `tick + wait`.
    pub wait: u8,
    /// The striker's hand and style when it struck (its roll is that hand's
    /// weapon as the style's feat attack shapes it, `Kit::swing`), and the
    /// share of a roll it delivers at its swing's cadence (C5).
    pub hand: u8,
    pub style: Style,
    pub share: u16,
    pub cadence_ms: u32,
    /// Where the blade met the body and which way it moved (units).
    pub point: Vec3,
    pub direction: Vec3,
}

/// A fighter's held hits, in slots.
pub type Grace = [Option<Held>; HELD];

impl Held {
    /// The tick it lands unless it is blocked first.
    pub fn until(&self) -> u64 {
        self.tick + u64::from(self.wait)
    }

    fn encode(&self, out: &mut Writer) {
        out.u32(self.by.0);
        out.u64(self.tick);
        out.u8(self.wait);
        out.u8(self.hand);
        out.u8(self.style as u8);
        out.u16(self.share);
        out.u32(self.cadence_ms);
        for value in self.point.iter().chain(&self.direction) {
            out.f32(*value);
        }
    }

    fn decode(input: &mut Reader) -> Result<Held, DecodeError> {
        let by = SimId(input.u32()?);
        let tick = input.u64()?;
        let wait = input.u8()?;
        let hand = input.u8()?;
        let at = input.at();
        let style = Style::from_index(input.u8()?).ok_or(input.invalid("held style", at))?;
        let share = input.u16()?;
        let cadence_ms = input.u32()?;
        let mut values = [0.0f32; 6];
        for value in &mut values {
            *value = input.f32()?;
        }
        Ok(Held {
            by,
            tick,
            wait,
            hand,
            style,
            share,
            cadence_ms,
            point: [values[0], values[1], values[2]],
            direction: [values[3], values[4], values[5]],
        })
    }
}

/// What an empty slot holds on the wire.
const BLANK: Held = Held {
    by: SimId(0),
    tick: 0,
    wait: 0,
    hand: 0,
    style: Style::Fast,
    share: 0,
    cadence_ms: 0,
    point: [0.0; 3],
    direction: [0.0; 3],
};

/// The ticks of `msec` a hit is held on a player whose view lags `lag_ms`:
/// none for one on the host.
pub fn wait(lag_ms: u16, msec: u32) -> u8 {
    (u32::from(lag_ms.min(GRACE_MS)) / msec.max(1)).min(u32::from(u8::MAX)) as u8
}

/// Whether the command for `tick`, made `lag_ms` behind it, was made while
/// its player saw the world as it was before `hit_tick`.
pub fn saw_before(tick: u64, lag_ms: u16, msec: u32, hit_tick: u64) -> bool {
    tick.saturating_sub(u64::from(lag_ms) / u64::from(msec.max(1))) < hit_tick
}

/// Holds `held` in a free slot of `grace`; when none is free the oldest
/// leaves for it, and comes back to land at once.
pub fn hold(grace: &mut Grace, held: Held) -> Option<Held> {
    if let Some(free) = grace.iter_mut().find(|slot| slot.is_none()) {
        *free = Some(held);
        return None;
    }
    let oldest = grace
        .iter_mut()
        .min_by_key(|slot| slot.map_or(u64::MAX, |h| h.tick))
        .expect("a fighter holds some");
    oldest.replace(held)
}

pub(crate) fn encode(grace: &Grace, out: &mut Writer) {
    for slot in grace {
        write_fixed(out, slot.as_ref(), &BLANK, Held::encode);
    }
}

pub(crate) fn decode(input: &mut Reader) -> Result<Grace, DecodeError> {
    let mut grace = [None; HELD];
    for slot in &mut grace {
        *slot = read_fixed(input, &BLANK, Held::decode, "held hit flag")?;
    }
    Ok(grace)
}
