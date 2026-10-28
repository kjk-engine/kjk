//! Bolts in flight (`docs/COMBAT.md`): a
//! point, or a small box, on a straight line from where and when it was
//! fired, its start and velocity snapped to whole units as JA's are. Each
//! step traces it from where it was to where it is, against the level and
//! every body but its owner's; what it meets it hits (a body) or stops at
//! (a wall). Traced every 8 ms tick where JA traces every 25 ms server
//! frame: the same line, sampled finer (a logged divergence).
//!
//! A bolt that meets a remote player who could block it is held where it
//! met the body for that player's grace (`Held`, I22 C19 as blades have
//! it): turned if a command of its, made seeing the world before the hit,
//! leaves it blocking by state; landing when the hold ends.

use crate::combat::grenades::Lob;
use crate::combat::sheet::Weapon;
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer};

/// A bolt (see the module documentation).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Missile {
    /// Who fired it (or last deflected it): it passes through its owner.
    pub owner: SimId,
    /// The weapon it came from, and that attack's cadence (ms): what its
    /// hit is worth (`damage`).
    pub weapon: Weapon,
    pub cadence_ms: u32,
    /// Its share of the shot, in thousandths (a flechette shard carries a
    /// fifth).
    pub share: u16,
    /// Where and when it started (units, sim ms), and its velocity (units
    /// per second).
    pub base: [f32; 3],
    pub from_ms: u64,
    pub velocity: [f32; 3],
    /// Its box: this many units either side.
    pub half_size: f32,
    pub blockable: bool,
    /// When it goes, unseen, if it met nothing (a grenade: its blast).
    pub until_ms: u64,
    /// A grenade: it falls, bounces and bursts (`combat::grenades`).
    pub lob: Option<Lob>,
    /// Held at `base` for its target's grace (see the module documentation).
    pub held: Option<Held>,
    /// A shot a script decided (`script::effects::Strike`): on its target
    /// it lands the script's damage, if the script's blow is still to land;
    /// on anyone else, or with none, it goes harmlessly.
    pub scripted: bool,
}

/// A bolt held for its target's grace: whom it met, the tick it met them,
/// and the ticks it waits there.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Held {
    pub target: SimId,
    pub tick: u64,
    pub wait: u8,
}

impl Held {
    const BLANK: Held = Held {
        target: SimId(0),
        tick: 0,
        wait: 0,
    };

    /// The tick it lands unless it is turned first.
    pub fn until(&self) -> u64 {
        self.tick + u64::from(self.wait)
    }

    fn encode(&self, out: &mut Writer) {
        out.u32(self.target.0);
        out.u64(self.tick);
        out.u8(self.wait);
    }

    fn decode(input: &mut Reader) -> Result<Held, DecodeError> {
        Ok(Held {
            target: SimId(input.u32()?),
            tick: input.u64()?,
            wait: input.u8()?,
        })
    }
}

impl Missile {
    /// Where it is at sim time `ms` (units); held at its start before it,
    /// and while held for a grace. A grenade falls with JA's gravity from
    /// its base, or rests there.
    pub fn at(&self, ms: u64) -> [f32; 3] {
        if self.held.is_some() {
            return self.base;
        }
        let seconds = ms.saturating_sub(self.from_ms) as f32 / 1000.0;
        let mut at: [f32; 3] =
            std::array::from_fn(|axis| self.base[axis] + self.velocity[axis] * seconds);
        match self.lob {
            Some(lob) if lob.resting => return self.base,
            Some(_) => at[2] -= 0.5 * crate::motion::GRAVITY * seconds * seconds,
            None => {}
        }
        at
    }

    /// Its velocity at sim time `ms` (units/s): a grenade's falls.
    pub fn velocity_at(&self, ms: u64) -> [f32; 3] {
        let mut velocity = self.velocity;
        if self.lob.is_some_and(|lob| !lob.resting) {
            let seconds = ms.saturating_sub(self.from_ms) as f32 / 1000.0;
            velocity[2] -= crate::motion::GRAVITY * seconds;
        }
        velocity
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.owner.0);
        crate::combat::sheet::encode_weapon(&self.weapon, out);
        out.u32(self.cadence_ms);
        out.u16(self.share);
        for value in self.base.iter().chain(&self.velocity) {
            out.f32(*value);
        }
        out.u64(self.from_ms);
        out.f32(self.half_size);
        out.u8(u8::from(self.blockable));
        out.u64(self.until_ms);
        crate::wire::write_fixed(out, self.lob.as_ref(), &Lob::BLANK, Lob::encode);
        crate::wire::write_fixed(out, self.held.as_ref(), &Held::BLANK, Held::encode);
        out.u8(u8::from(self.scripted));
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Missile, DecodeError> {
        let owner = SimId(input.u32()?);
        let weapon = crate::combat::sheet::decode_weapon(input)?;
        let cadence_ms = input.u32()?;
        let share = input.u16()?;
        let mut values = [0.0f32; 6];
        for value in &mut values {
            *value = input.f32()?;
        }
        let from_ms = input.u64()?;
        let half_size = input.f32()?;
        let at = input.at();
        let blockable = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("blockable flag", at)),
        };
        Ok(Missile {
            owner,
            weapon,
            cadence_ms,
            share,
            base: [values[0], values[1], values[2]],
            from_ms,
            velocity: [values[3], values[4], values[5]],
            half_size,
            blockable,
            until_ms: input.u64()?,
            lob: crate::wire::read_fixed(input, &Lob::BLANK, Lob::decode, "lob flag")?,
            held: crate::wire::read_fixed(input, &Held::BLANK, Held::decode, "held bolt flag")?,
            scripted: input.u8()? != 0,
        })
    }
}
