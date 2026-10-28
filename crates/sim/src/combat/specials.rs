//! KOTOR's feat attacks for blasters, and sneak attacks
//! (`docs/COMBAT.md`, C7). KOTOR
//! lets a character pick one feat attack a round; here the alternate
//! button takes the one its blaster suits (`Special::of`), shaped as JA's
//! alternate fire for that kind of gun, with KOTOR's numbers (the feats'
//! descriptions in `dialog.tlk`):
//!
//! - **Power Blast**, Improved, Master (`feat.2da` 29, 18, 82; first for
//!   pistols, JA's Bryar charge): hold the button to charge, in
//!   `CHARGE_STEP_MS` steps up to `CHARGE_STEPS`; let go and one shot leaves
//!   with more damage at a penalty to hit (`rules::feats::specials::
//!   power_blast`). Its pace (C2) counts the charge as part of the attack's
//!   time, so a charge trades time for one heavy shot.
//! - **Rapid Shot**, Improved, **Multi Shot** (30, 92, 26; first for
//!   rifles and the rest, JA's E-11 fast fire): a press fires a burst
//!   (more with Multi Shot) `BURST_GAP_MS` apart from when the gun is next
//!   ready, and the gun is ready again when the burst would have taken one
//!   shot fewer at its own cadence (KOTOR's extra attack); a penalty to hit
//!   and to defence for the round (`rules::feats::specials::rapid_shot`).
//! - **Sniper Shot**, Improved, Master (31, 20, 77; first for disruptors,
//!   JA's zoom): hold the button to aim (movement slowed, `AIM_SPEED`, no
//!   jumping); each shot taken aiming has its threat range multiplied and
//!   leaves defence lower for the round (`rules::feats::specials::
//!   sniper_shot`). KOTOR's 6 s stun on a failed Fortitude save
//!   is left out: in real time it would freeze a fight on every lucky shot
//!   (the Stun power keeps that role).
//!
//! A special costs `COST_PERCENT` of the second bar (`Sheet::pool`: FP
//! for a Force user, stamina for anyone else), paid as it starts; without
//! enough, or with none of these feats, the alternate button fires as the
//! primary does.
//!
//! **Sneak attack** (60 to 69): a die a tier to a weapon's hit on a target
//! that cannot answer it (KOTOR: from behind, held, or unaware), within
//! `SNEAK_REACH`, not multiplied by a crit (`rules::feats::sneak_attack`). Here "behind" is the target's rear 90°,
//! "held" is stunned, knocked down or gripped, and "unaware" is a creature
//! that had no one to fight or has not yet taken in the foe it spotted
//! (its Awareness, `ai::reaction_ms`). Uncanny Dodge (56, 57) spares a
//! target the hits from behind.

use std::f32::consts::FRAC_1_SQRT_2;

use collision::UNIT;

use crate::combat::sheet::{Blaster, Sheet, Weapon, feats};
use crate::combat::{ROUND_MS, weapons};
use crate::math;
use crate::wire::{DecodeError, Reader, Writer, read_fixed, write_fixed};

/// What a special costs, in percent of the bar: regeneration refills the
/// bar (C13) so one can be kept up a round, KOTOR's one feat attack a
/// round.
pub const COST_PERCENT: u16 = rules::feats::specials::COST_PERCENT;
/// A Power Blast's charge: steps of this long (ms), up to `CHARGE_STEPS`
/// (JA's Bryar alternate).
pub const CHARGE_STEP_MS: u64 = rules::feats::specials::power_blast::STEP_MS;
pub const CHARGE_STEPS: u64 = rules::feats::specials::power_blast::STEPS;
/// A charge held this long goes off by itself (JA's
/// `MAX_WEAPON_CHARGE_TIME`, ms).
pub const CHARGE_LIMIT_MS: u64 = rules::feats::specials::power_blast::LIMIT_MS;
/// A burst's shots are this far apart (JA's E-11 alternate refire, ms), or
/// the gun's own refire if that is shorter.
pub const BURST_GAP_MS: u32 = rules::feats::specials::rapid_shot::GAP_MS;
/// Movement while aiming (JA's disruptor zoom).
pub const AIM_SPEED: f32 = rules::feats::specials::sniper_shot::AIM_SPEED;
/// How far a sneak attack reaches, in units.
pub const SNEAK_REACH: f32 = rules::feats::sneak_attack::REACH_M / UNIT;

/// A feat special and the `feat.2da` rows of its line.
type Line = (fn(u8) -> Special, &'static [u16]);

/// A blaster's feat special, with its feat line's tier (1 to 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Special {
    PowerBlast(u8),
    RapidShot(u8),
    SniperShot(u8),
}

impl Special {
    /// The special `sheet` takes with `blaster`: the line the gun's JA
    /// alternate suggests if it has that feat (Power Blast for pistols,
    /// Sniper Shot for disruptors, Rapid Shot for the rest), else the next
    /// it has in the order Power Blast, Rapid Shot, Sniper Shot.
    pub fn of(sheet: &Sheet, blaster: Blaster) -> Option<Special> {
        let lines: [Line; 3] = [
            (Special::PowerBlast, &feats::POWER_BLAST),
            (Special::RapidShot, &feats::RAPID_SHOT),
            (Special::SniperShot, &feats::SNIPER_SHOT),
        ];
        let order = match blaster {
            Blaster::Pistol => [0, 1, 2],
            Blaster::Disruptor => [2, 0, 1],
            _ => [1, 2, 0],
        };
        // A line's tier is the highest of its rows held.
        order.into_iter().find_map(|line| {
            let (special, rows) = lines[line];
            let tier = rows.iter().rposition(|row| sheet.feats.has(*row))?;
            Some(special(tier as u8 + 1))
        })
    }

    /// `weapon` as this special's shot carries it: Power Blast's damage and
    /// its penalty to hit; Sniper Shot's threat range. (Rapid Shot's penalty
    /// is the round's, `Shooting::exposed`.)
    pub fn shot(self, mut weapon: Weapon) -> Weapon {
        use rules::feats::specials::power_blast;
        use rules::feats::specials::sniper_shot;
        match self {
            Special::PowerBlast(tier) => {
                let bonus = power_blast::DAMAGE[usize::from(tier.clamp(1, 3) - 1)];
                weapon.damage_bonus = weapon.damage_bonus.saturating_add(bonus);
                weapon.attack_bonus = weapon
                    .attack_bonus
                    .saturating_sub(power_blast::ATTACK_PENALTY);
            }
            Special::SniperShot(tier) => {
                let times = sniper_shot::THREAT_TIMES[usize::from(tier.clamp(1, 3) - 1)];
                weapon.crit_threat = weapon.crit_threat.max(1).saturating_mul(times);
            }
            Special::RapidShot(_) => {}
        }
        weapon
    }

    /// KOTOR's penalties for the round after it: to hit, to defence.
    fn exposure(self) -> (i8, i8) {
        match self {
            Special::PowerBlast(_) => (0, 0),
            Special::RapidShot(tier) => {
                let penalty =
                    rules::feats::specials::rapid_shot::PENALTY[usize::from(tier.clamp(1, 3) - 1)];
                (penalty, penalty)
            }
            Special::SniperShot(_) => (0, rules::feats::specials::sniper_shot::DEFENCE_PENALTY),
        }
    }

    /// A Rapid Shot's shots: Multi Shot's (its third tier), else Rapid
    /// Shot's.
    fn burst(self) -> u8 {
        use rules::feats::specials::rapid_shot;
        match self {
            Special::RapidShot(3) => rapid_shot::MULTI_SHOTS,
            _ => rapid_shot::SHOTS,
        }
    }
}

/// What a special costs `sheet`, in hundredths of a point of its bar.
pub fn cost(sheet: &Sheet) -> i32 {
    i32::from(sheet.pool()) * i32::from(COST_PERCENT)
}

/// A fighter's blaster special under way, and the penalties of the last.
/// Carried in snapshots (one width whatever it holds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shooting {
    /// When the alternate button began a Power Blast's charge (sim ms).
    pub charging_from: Option<u64>,
    /// Shots left of a Rapid Shot's burst.
    pub burst: u8,
    /// The alternate button was held on the last step (a burst goes on a
    /// press).
    pub alt_held: bool,
    /// Aiming a Sniper Shot: movement halved, no jumping.
    pub aiming: bool,
    /// KOTOR's penalties after a special: to hit and to defence (whole
    /// points), until then (sim ms).
    pub exposed: (i8, i8, u64),
    /// A pistol in each hand (`combat::pair`): the left gun's own weapon
    /// timer (the right's is `Fighter::weapon_wait_ms`), and whether the left
    /// fires next.
    pub off_ms: i32,
    pub next_off: bool,
    /// The left gun's one attack of a Rapid Shot's round is still to fire
    /// (`pair::pull_special`).
    pub off_burst: bool,
    /// The hands the last shot left (bit 0 the right, bit 1 the left):
    /// the gun each view flashes and kicks.
    pub hands: u8,
}

impl Shooting {
    /// The penalties to hit and to defence at `now`.
    pub fn exposed(&self, now: u64) -> (i8, i8) {
        let (attack, defence, until) = self.exposed;
        if until > now {
            (attack, defence)
        } else {
            (0, 0)
        }
    }

    /// Drops a charge or a burst under way (the gun was taken out of hand).
    pub fn interrupt(&mut self) {
        self.charging_from = None;
        self.burst = 0;
        self.aiming = false;
        self.next_off = false;
        self.off_burst = false;
    }

    /// The penalty to hit of the round after a special set at `now` a
    /// point less, to none at most (TSL's Rapid Shot Bonus items): once a
    /// round, as the penalty is set once a burst.
    pub fn ease(&mut self, now: u64) {
        if self.exposed.2 == now + ROUND_MS {
            self.exposed.0 = (self.exposed.0 + 1).min(0);
        }
    }

    fn expose(&mut self, special: Special, now: u64) {
        let (attack, defence) = special.exposure();
        if (attack, defence) != (0, 0) {
            self.exposed = (attack, defence, now + ROUND_MS);
        }
    }

    pub(crate) fn encode(&self, out: &mut Writer) {
        write_fixed(out, self.charging_from.as_ref(), &0, |from, out| {
            out.u64(*from)
        });
        out.u8(self.burst);
        out.u8(u8::from(self.alt_held)
            | u8::from(self.aiming) << 1
            | u8::from(self.next_off) << 2
            | (self.hands & 3) << 3
            | u8::from(self.off_burst) << 5);
        let (attack, defence, until) = self.exposed;
        out.i8(attack);
        out.i8(defence);
        out.u64(until);
        out.u32(self.off_ms as u32);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Shooting, DecodeError> {
        let charging_from = read_fixed(input, &0, |input| input.u64(), "charge flag")?;
        let burst = input.u8()?;
        let at = input.at();
        let flags = input.u8()?;
        if flags > 63 {
            return Err(input.invalid("shooting flags", at));
        }
        Ok(Shooting {
            charging_from,
            burst,
            alt_held: flags & 1 != 0,
            aiming: flags & 2 != 0,
            next_off: flags & 4 != 0,
            hands: flags >> 3 & 3,
            off_burst: flags & 32 != 0,
            exposed: (input.i8()?, input.i8()?, input.u64()?),
            off_ms: input.u32()? as i32,
        })
    }
}

/// A step's trigger on a blaster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trigger {
    pub attack: bool,
    pub alt: bool,
    pub now: u64,
    pub msec: u32,
    /// The gun's refire (ms), Rage's factor applied.
    pub refire_ms: u32,
    /// The gun's own refire, which the pace counts (C2: Rage's faster
    /// fire is Rage's gain).
    pub pace_ms: u32,
}

/// A shot leaving this step: the special shaping it, if any, and the
/// attack's time the pace counts (C2, ms).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shot {
    pub special: Option<Special>,
    pub cadence_ms: u32,
}

/// Takes `cost` from `pool` if it holds that much.
fn afford(pool: &mut i32, cost: i32) -> bool {
    let enough = *pool >= cost;
    if enough {
        *pool -= cost;
    }
    enough
}

/// One step of a blaster's trigger: the cadence of `weapons` with the
/// character's `special` on the alternate button (see the module
/// documentation). `timer_ms` is the weapon timer, `pool` the bar a special
/// takes `cost` from.
pub fn pull(
    shooting: &mut Shooting,
    timer_ms: &mut i32,
    pool: &mut i32,
    special: Option<Special>,
    cost: i32,
    trigger: Trigger,
) -> Option<Shot> {
    let Trigger {
        attack,
        alt,
        now,
        msec,
        refire_ms,
        pace_ms,
    } = trigger;
    let press = alt && !shooting.alt_held;
    shooting.alt_held = alt;
    if !matches!(special, Some(Special::PowerBlast(_))) {
        shooting.charging_from = None;
    }
    if !matches!(special, Some(Special::RapidShot(_))) {
        shooting.burst = 0;
    }
    shooting.aiming = alt && matches!(special, Some(Special::SniperShot(_)));
    let ready = weapons::count_down(timer_ms, msec);
    match special {
        Some(special @ Special::RapidShot(_)) => {
            // A press starts a burst, its first shot as soon as the gun is
            // ready.
            if shooting.burst == 0 && press && afford(pool, cost) {
                shooting.burst = special.burst();
                shooting.expose(special, now);
            }
            if shooting.burst > 0 {
                if !ready {
                    return None;
                }
                shooting.burst -= 1;
                let gap = BURST_GAP_MS.min(refire_ms);
                let wait = if shooting.burst > 0 {
                    gap
                } else {
                    // From the first shot the gun waits (shots − 1) of its
                    // own refires in all: one attack more than its cadence
                    // (with nothing left to wait, ready on the next step).
                    let shots = u32::from(special.burst());
                    (shots - 1) * (refire_ms - gap)
                };
                *timer_ms += wait as i32;
                return Some(Shot {
                    special: Some(special),
                    cadence_ms: pace_ms,
                });
            }
        }
        Some(special @ Special::PowerBlast(_)) => {
            if let Some(from) = shooting.charging_from {
                let held = now.saturating_sub(from);
                if alt && held < CHARGE_LIMIT_MS {
                    return None;
                }
                shooting.charging_from = None;
                let steps = (held / CHARGE_STEP_MS).clamp(1, CHARGE_STEPS);
                *timer_ms += refire_ms as i32;
                return Some(Shot {
                    special: Some(special),
                    cadence_ms: pace_ms + (steps * CHARGE_STEP_MS) as u32,
                });
            }
            if alt && ready && afford(pool, cost) {
                shooting.charging_from = Some(now);
                return None;
            }
        }
        Some(special @ Special::SniperShot(_)) if shooting.aiming => {
            if !weapons::trigger(timer_ms, ready, attack, refire_ms) {
                return None;
            }
            let special = afford(pool, cost).then_some(special);
            if let Some(special) = special {
                shooting.expose(special, now);
            }
            return Some(Shot {
                special,
                cadence_ms: pace_ms,
            });
        }
        _ => {}
    }
    // Without a special, or the bar for one, the alternate fires as the
    // primary does.
    let held = attack || (alt && (special.is_none() || *pool < cost));
    weapons::trigger(timer_ms, ready, held, refire_ms).then_some(Shot {
        special: None,
        cadence_ms: pace_ms,
    })
}

/// Whether `from` is in the rear 90° of a character standing at `origin`
/// facing `yaw` (degrees; ground plane only).
pub fn behind(origin: [f32; 3], yaw: f32, from: [f32; 3]) -> bool {
    let (sin, cos) = math::sin_cos_deg(yaw);
    let (dx, dy) = (from[0] - origin[0], from[1] - origin[1]);
    let length = (dx * dx + dy * dy).sqrt();
    length > 0.0 && dx * cos + dy * sin < -length * FRAC_1_SQRT_2
}

/// How a sneak attack finds its target.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Exposure {
    /// The attacker is in the target's rear 90° (`behind`).
    pub behind: bool,
    /// Stunned, knocked down or gripped.
    pub held: bool,
    /// A creature that had no one to fight, or not yet aware of its foe.
    pub unaware: bool,
    /// From attacker to target (units).
    pub distance: f32,
}

/// The d6s of sneak attack `attacker` adds to a weapon's hit on `target`.
pub fn sneak_dice(attacker: &Sheet, target: &Sheet, exposure: Exposure) -> u8 {
    let tier = attacker.feats.count(&feats::SNEAK_ATTACK);
    let dodges = target.feats.count(&feats::UNCANNY_DODGE) > 0;
    let open = exposure.held || exposure.unaware || (exposure.behind && !dodges);
    if tier > 0 && open && exposure.distance <= SNEAK_REACH {
        tier
    } else {
        0
    }
}
