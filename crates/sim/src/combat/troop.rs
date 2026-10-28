//! A gunner's pacing in a fight: JA's troopers' tempo
//! (`docs/COMBAT.md`), under KOTOR's choice of whom to fight. It stands
//! and shoots, facing its enemy squarely, its shots spaced as JA's NPCs
//! space theirs and their spread wide at first, closing as it keeps a
//! clear shot. Every few seconds it checks it still sees its enemy; when it
//! does not it makes for a spot that does, and stands there. It holds its
//! ground against an enemy come close, as KOTOR's gunners do.
//!
//! Its dice are the minds' (`pace::Dice`); its state rides in the snapshot
//! (`Troop`).

use crate::combat::pace::Dice;
use crate::combat::sheet::Blaster;
use crate::math::Vec3;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};
use rules::ai::gunners;
use rules::ai::gunners::aim;
use rules::ai::gunners::spread;

/// How near its enemy a spot it moves to may be (units).
pub const AVOID: f32 = rules::ai::gunners::spots::AVOID;

/// A gunner's timers (sim ms each ends at), aim and spot.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Troop {
    /// Whom it last took aim at anew (id + 1; 0: none).
    pub rated: u32,
    /// Its aim (JA's `currentAim`): its shots' spread widens as it falls
    /// (`spread`).
    pub aim: i8,
    pub aim_at: u64,
    pub sight_check_at: u64,
    /// The spot it makes for, if any.
    pub spot: Option<Vec3>,
    /// No shot before this; the shots left in a burst; its shots' pace
    /// (ms: the damage's cadence, C2).
    pub shot_at: u64,
    pub burst: u8,
    pub pace_ms: u32,
    /// Whether it saw its enemy on its last think.
    pub sees: bool,
}

/// How a creature's gun is fired (JA's medium difficulty):
/// the gap between shots (ms), and for a burst weapon the shots in a
/// burst, fired at the gun's own rate before the gap.
pub fn spacing(blaster: Blaster) -> (u64, Option<(u8, u8)>) {
    let [pistol, rifle, repeater, rest] = gunners::SPACING_MS;
    let [least, most] = gunners::BURST;
    match blaster {
        Blaster::Pistol => (pistol, None),
        Blaster::Rifle | Blaster::Bowcaster => (rifle, None),
        Blaster::Repeater => (repeater, Some((least, most))),
        Blaster::Disruptor | Blaster::Ion | Blaster::Sonic => (rest, None),
    }
}

/// The share (thousandths) of a KOTOR round's damage a second a creature's
/// shots deal (C2's pace factor takes the rest; `ai.gunners.damage_share`).
const DAMAGE_SHARE: u64 = gunners::DAMAGE_SHARE;

/// The pace (ms) a creature's shots fired every `every_ms` take for their
/// damage (C2), at `DAMAGE_SHARE`.
fn paced(every_ms: u64) -> u32 {
    (every_ms * DAMAGE_SHARE / 1000) as u32
}

/// A shot's spread at `aim`, degrees each way on pitch and yaw
/// (`BLASTER_NPC_SPREAD` and the aim's share), no less than
/// `ai.gunners.spread.floor`.
pub fn spread(aim: i8) -> f32 {
    (spread::BASE + f32::from(spread::PAR - aim) * spread::PER_AIM).max(spread::FLOOR)
}

impl Troop {
    /// Takes aim at a new enemy (`G_AimSet` on anger): an enemy's aim
    /// starts worse twice over (medium, `ai.gunners.aim.foe_start`), an
    /// ally's a little (`ai.gunners.aim.ally_start`).
    pub fn rate(&mut self, foe: u32, ally: bool, dice: &mut Dice) {
        if self.rated == foe + 1 {
            return;
        }
        self.rated = foe + 1;
        let best = i64::from(aim::BEST);
        self.aim = if ally {
            let [worst, least] = aim::ALLY_START;
            dice.roll_between(best - worst, best - least)
        } else {
            let [worst, least] = aim::FOE_START;
            dice.roll_between(best - worst * aim::FOE_TIMES, best - least * aim::FOE_TIMES)
        } as i8;
        self.aim_at = 0;
    }

    /// Its aim moved by `change` once its wait is over, the next wait
    /// `ai.gunners.aim.every_ms` (as JA's NPCs adjust it; the first call only
    /// starts the wait).
    pub fn adjust_aim(&mut self, change: i8, now: u64, dice: &mut Dice) {
        if self.aim_at != 0 && now >= self.aim_at {
            self.aim = self.aim.saturating_add(change).clamp(aim::WORST, aim::BEST);
        }
        if self.aim_at == 0 || now >= self.aim_at {
            let [low, high] = aim::EVERY_MS;
            self.aim_at = now + dice.ms(low, high);
        }
    }

    /// Whether it fires now (`ShootThink`): its gap over, a shot; a burst
    /// weapon's burst at the gun's own rate (`refire_ms`), then its gap,
    /// give or take `ai.gunners.jitter_ms`; a `pair` (a pistol in each hand)
    /// `ai.gunners.pair_spacing` of the gap. Sets the pace its shots' damage
    /// takes.
    pub fn fire(
        &mut self,
        (blaster, pair): (Blaster, bool),
        refire_ms: u32,
        now: u64,
        dice: &mut Dice,
    ) -> bool {
        if now < self.shot_at {
            return false;
        }
        let (gap, burst) = spacing(blaster);
        let jitter = gunners::JITTER_MS;
        let gap = gap.saturating_add_signed(dice.roll_between(-jitter, jitter));
        match burst {
            Some((low, high)) => {
                if self.burst == 0 {
                    self.burst = dice.roll_between(i64::from(low), i64::from(high)) as u8;
                    self.pace_ms = paced(
                        (u64::from(self.burst) * u64::from(refire_ms) + gap)
                            / u64::from(self.burst),
                    );
                }
                self.burst -= 1;
                self.shot_at = if self.burst == 0 {
                    now + gap
                } else {
                    now + u64::from(refire_ms)
                };
            }
            // A pistol in each hand: the guns take turns, each shot a
            // pistol's (`combat::pair`), so they come oftener at its pace.
            None => {
                self.pace_ms = paced(gap);
                self.shot_at = now
                    + if pair {
                        gap * gunners::PAIR_SPACING / 1000
                    } else {
                        gap
                    };
            }
        }
        true
    }

    /// Whether a check that runs every `low` to `high` ms is due now, its
    /// next one set.
    pub fn due(at: &mut u64, now: u64, (low, high): (u64, u64), dice: &mut Dice) -> bool {
        if now < *at {
            return false;
        }
        *at = now + dice.ms(low, high);
        true
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u32(self.rated);
        out.u8(self.aim as u8);
        for time in [self.aim_at, self.sight_check_at, self.shot_at] {
            out.u64(time);
        }
        write_fixed(out, self.spot.as_ref(), &[0.0; 3], |spot, out| {
            for value in spot {
                out.f32(*value);
            }
        });
        out.u8(self.burst);
        out.u32(self.pace_ms);
        out.u8(u8::from(self.sees));
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Troop, DecodeError> {
        Ok(Troop {
            rated: input.u32()?,
            aim: input.u8()? as i8,
            aim_at: input.u64()?,
            sight_check_at: input.u64()?,
            shot_at: input.u64()?,
            spot: read_fixed(
                input,
                &[0.0; 3],
                |input| Ok([input.f32()?, input.f32()?, input.f32()?]),
                "troop spot flag",
            )?,
            burst: input.u8()?,
            pace_ms: input.u32()?,
            sees: input.u8()? != 0,
        })
    }
}
