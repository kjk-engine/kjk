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
//!   ready, and the gun is ready again a refire after the first shot, or as
//!   the burst ends (every shot past the first an extra attack, as KOTOR's
//!   one is); a penalty to hit and to defence for the round
//!   (`rules::feats::specials::rapid_shot`).
//! - **Sniper Shot**, Improved, Master (31, 20, 77; first for disruptors,
//!   JA's zoom): hold the button to aim (movement slowed, `AIM_SPEED`, no
//!   jumping); each shot taken aiming has its threat range multiplied and
//!   leaves defence lower for the round (`rules::feats::specials::
//!   sniper_shot`). KOTOR's 6 s stun on a failed Fortitude save
//!   is left out: in real time it would freeze a fight on every lucky shot
//!   (the Stun power keeps that role).
//!
//! A gun takes the line its wielder chose for that type (`combat::picks`)
//! before the one it suits.
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

/// The feat lines, in the order `Picks` numbers them.
const FEAT_LINES: [Line; Special::LINES] = [
    (Special::PowerBlast, &feats::POWER_BLAST),
    (Special::RapidShot, &feats::RAPID_SHOT),
    (Special::SniperShot, &feats::SNIPER_SHOT),
];

impl Special {
    /// How many feat lines there are.
    pub const LINES: usize = 3;

    /// The special `sheet` takes with `blaster`: the line its wielder chose
    /// for that gun type (`Picks`), if it still has it; else the line the
    /// gun's JA alternate suggests if it has that feat (Power Blast for
    /// pistols, Sniper Shot for disruptors, Rapid Shot for the rest), else
    /// the next it has in the order Power Blast, Rapid Shot, Sniper Shot.
    pub fn of(sheet: &Sheet, blaster: Blaster) -> Option<Special> {
        sheet
            .picks
            .gun(blaster)
            .into_iter()
            .chain(Self::order(blaster))
            .find_map(|line| Self::of_line(sheet, line))
    }

    /// The lines in the order `blaster` takes them.
    fn order(blaster: Blaster) -> [usize; Self::LINES] {
        match blaster {
            Blaster::Pistol => [0, 1, 2],
            Blaster::Disruptor => [2, 0, 1],
            _ => [1, 2, 0],
        }
    }

    /// `sheet`'s special of `line`, at the highest of its rows held.
    fn of_line(sheet: &Sheet, line: usize) -> Option<Special> {
        let (special, rows) = FEAT_LINES[line];
        let tier = rows.iter().rposition(|row| sheet.feats.has(*row))?;
        Some(special(tier as u8 + 1))
    }

    /// Whether `sheet` knows `line`.
    pub fn known(sheet: &Sheet, line: usize) -> bool {
        Self::of_line(sheet, line).is_some()
    }

    /// The line `row` belongs to, if a blaster's.
    pub fn line_of(row: u16) -> Option<usize> {
        FEAT_LINES.iter().position(|(_, rows)| rows.contains(&row))
    }

    /// This special's line.
    pub fn line(self) -> usize {
        match self {
            Special::PowerBlast(_) => 0,
            Special::RapidShot(_) => 1,
            Special::SniperShot(_) => 2,
        }
    }

    /// The `feat.2da` row of this special.
    pub fn row(self) -> u16 {
        let tier = match self {
            Special::PowerBlast(tier) | Special::RapidShot(tier) | Special::SniperShot(tier) => {
                tier
            }
        };
        FEAT_LINES[self.line()].1[usize::from(tier.clamp(1, 3) - 1)]
    }

    /// The line after the one `blaster` takes now, among those `sheet`
    /// knows; none with fewer than two.
    pub fn next_line(sheet: &Sheet, blaster: Blaster) -> Option<usize> {
        let known: Vec<usize> = Self::order(blaster)
            .into_iter()
            .filter(|line| Self::known(sheet, *line))
            .collect();
        let now = Self::of(sheet, blaster)?.line();
        let at = known.iter().position(|line| *line == now)?;
        (known.len() > 1).then(|| known[(at + 1) % known.len()])
    }

    /// `weapon` as this special's shot carries it: Power Blast's damage and
    /// its penalty to hit; Sniper Shot's threat range and bonus to hit. (Rapid Shot's penalty
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
                let tier = usize::from(tier.clamp(1, 3) - 1);
                weapon.crit_threat = weapon
                    .crit_threat
                    .max(1)
                    .saturating_mul(sniper_shot::THREAT_TIMES[tier]);
                weapon.attack_bonus = weapon
                    .attack_bonus
                    .saturating_add(sniper_shot::ATTACK_BONUS[tier]);
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
    pub(super) fn burst(self) -> u8 {
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
    /// The spread the last shots added (`combat::bloom`), thousandths of a
    /// degree, as it stood at `fired_at`.
    pub bloom_mdeg: u16,
    /// The aim's kick (pitch, yaw), thousandths of a degree, as it stood at
    /// `fired_at`; up is a smaller pitch.
    pub kick_mdeg: [i16; 2],
    /// When the last shot left (sim ms).
    pub fired_at: u64,
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
        self.clear_bloom();
    }

    /// The bloom and kick gone (`combat::bloom`): a gun out of hand, or put
    /// away, comes back true.
    pub fn clear_bloom(&mut self) {
        self.bloom_mdeg = 0;
        self.kick_mdeg = [0; 2];
        self.fired_at = 0;
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
        out.u16(self.bloom_mdeg);
        for axis in self.kick_mdeg {
            out.u16(axis as u16);
        }
        out.u64(self.fired_at);
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Shooting, DecodeError> {
        let charging_from = read_fixed(input, &0, |input| input.u64(), "charge flag")?;
        let burst = input.u8()?;
        let at = input.at();
        let flags = input.u8()?;
        if flags > 63 {
            return Err(input.invalid("shooting flags", at));
        }
        let exposed = (input.i8()?, input.i8()?, input.u64()?);
        let off_ms = input.u32()? as i32;
        // Saves before bloom read none: no shot in a save is mid-burst.
        let (bloom_mdeg, kick_mdeg, fired_at) =
            if input.since(crate::since::snapshot::FEAT_DYNAMIC_CROSSHAIR) {
                let bloom = input.u16()?;
                let kick = [input.u16()? as i16, input.u16()? as i16];
                (bloom, kick, input.u64()?)
            } else {
                (0, [0; 2], 0)
            };
        Ok(Shooting {
            charging_from,
            burst,
            alt_held: flags & 1 != 0,
            aiming: flags & 2 != 0,
            next_off: flags & 4 != 0,
            hands: flags >> 3 & 3,
            off_burst: flags & 32 != 0,
            exposed,
            off_ms,
            bloom_mdeg,
            kick_mdeg,
            fired_at,
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
                    // From the first shot the gun waits one of its own
                    // refires in all (with nothing left to wait, ready on
                    // the next step).
                    let shots = u32::from(special.burst());
                    refire_ms.saturating_sub((shots - 1) * gap)
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
