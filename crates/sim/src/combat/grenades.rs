//! Grenades (`docs/COMBAT.md`): JA's thermal
//! detonator physics, KOTOR's effects. A character holds the grenade button
//! to charge a throw (JA: strength = time held / `CHARGE_MS`, within
//! `MIN_STRENGTH` and 1) and lets go to throw the first kind it still
//! carries: `SPEED` × the strength along its view, `LIFT` more upward, from
//! its muzzle point (`MUZZLE`); it falls with gravity, bounces off walls and
//! floors keeping `BOUNCE` of its speed and comes to rest when slow on a
//! floor, and goes off `FUSE_MS` after the throw (JA's fuse and its
//! warning) or at once on touching a body (`rules::grenades`).
//!
//! The blast is KOTOR's: its row's impact script (`k_sup_grenade`) cast
//! where it bursts wherever the level has it (`world`); where no script can
//! run, `effect` stands in for it with the numbers the items' descriptions
//! give: everyone within `RADIUS` in its line of sight, the thrower too, takes
//! its damage of its type through the pipeline (`damage::Source::Blast`:
//! guards, friendly fire's area rule), halved by a Reflex save against its
//! DC; a save also spares its secondary effect where KOTOR says so. Stuns,
//! paralysis and entanglement are held to C11's 3 s with a second of
//! immunity after, as the Force's are.

use crate::combat::Fighter;
use crate::combat::force::ja;
use crate::combat::items::{self, Carried, Course, KINDS, Pack, Poisoned, Use};
use crate::combat::passive;
use crate::combat::sheet::{Ability, Sheet};
use crate::snapshot::SimId;
use crate::wire::{DecodeError, Reader, Writer};

/// The longest charge (ms) and the weakest throw (JA's thermal).
pub const CHARGE_MS: u64 = rules::grenades::throw::CHARGE_MS;
pub const MIN_STRENGTH: f32 = rules::grenades::throw::MIN_STRENGTH;
/// Throw speed at full strength, and the lift every throw gets (units/s).
pub const SPEED: f32 = rules::grenades::throw::SPEED;
pub const LIFT: f32 = rules::grenades::throw::LIFT;
/// Where it leaves the hand: units forward, right and up of the eye.
pub const MUZZLE: [f32; 3] = rules::grenades::throw::MUZZLE;
/// From the throw to the blast (ms).
pub const FUSE_MS: u64 = rules::grenades::flight::FUSE_MS;
/// What a bounce keeps of the speed, and below what speed (units/s) on a
/// floor (`rules::grenades::flight::FLOOR`) it rests.
pub const BOUNCE: f32 = rules::grenades::flight::BOUNCE;
pub const REST_SPEED: f32 = rules::grenades::flight::REST_SPEED;
/// The weapon timer a throw takes (ms; JA's thermal refire).
pub const REFIRE_MS: i32 = rules::grenades::throw::REFIRE_MS;
/// KOTOR's blast radius, in units.
pub const RADIUS: f32 = rules::grenades::blast::RADIUS_M / collision::UNIT;
/// The box a grenade flies as: this many units either side.
pub const HALF_SIZE: f32 = rules::grenades::throw::HALF_SIZE;

/// What a blast does to whom it catches beyond its damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Secondary {
    None,
    /// Cannot act: stun, paralysis, entanglement (ms asked; C11 caps it).
    Held(u64),
    /// Knocked down (JA's knockdown).
    Knockdown,
    /// Poisoned: `points` every `every_ms` for `for_ms`.
    Poison {
        points: u8,
        every_ms: u64,
        for_ms: u64,
    },
    /// Its DEX lowered by `points` for `for_ms`.
    Weaken {
        points: u8,
        for_ms: u64,
    },
}

/// A kind of grenade's blast.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    /// Whole points of `damage_type` (a `sheet::DAMAGE_TYPES` row), and
    /// on droids, if different.
    pub points: u16,
    pub droid_points: u16,
    pub damage_type: u8,
    /// The Reflex save's DC; `None` for no save.
    pub dc: Option<u8>,
    /// Whether a save halves the damage (else it spares the secondary
    /// only), and whether it spares the secondary.
    pub save_halves: bool,
    pub secondary: Secondary,
    pub save_spares: bool,
}

/// The blast of `spells.2da` row `row` (K1's and TSL's grenade rows, TSL's
/// launched rows and its two stronger sonic rows taken as theirs, by
/// label); `None` for a row that is not a grenade.
pub fn effect(row: u16) -> Option<Effect> {
    use crate::combat::spells::*;
    let row = match row {
        ITEM_ABILITY_GRENADE_LAUNCHED_FRAGMENTATION => ITEM_ABILITY_GRENADE_FRAGMENTATION,
        ITEM_ABILITY_GRENADE_LAUNCHED_STUN => ITEM_ABILITY_GRENADE_STUN,
        ITEM_ABILITY_GRENADE_LAUNCHED_THERMAL_DETONATOR => ITEM_ABILITY_GRENADE_THERMAL_DETONATOR,
        ITEM_ABILITY_GRENADE_LAUNCHED_POISON => ITEM_ABILITY_GRENADE_POISON,
        ITEM_ABILITY_GRENADE_LAUNCHED_ADHESIVE => ITEM_ABILITY_GRENADE_ADHESIVE,
        ITEM_ABILITY_GRENADE_LAUNCHED_CRYOBAN => ITEM_ABILITY_GRENADE_CRYOBAN,
        ITEM_ABILITY_GRENADE_LAUNCHED_FIRE => ITEM_ABILITY_GRENADE_FIRE,
        ITEM_ABILITY_GRENADE_LAUNCHED_ION => ITEM_ABILITY_GRENADE_ION,
        ITEM_ABILITY_GRENADE_LAUNCHED_SONIC
        | ITEM_ABILITY_GRENADE_SONIC_2
        | ITEM_ABILITY_GRENADE_SONIC_3
        | ITEM_ABILITY_GRENADE_LAUNCHED_SONIC_2
        | ITEM_ABILITY_GRENADE_LAUNCHED_SONIC_3 => ITEM_ABILITY_GRENADE_SONIC,
        row => row,
    };
    use rules::grenades::adhesive;
    use rules::grenades::cryoban;
    use rules::grenades::fire;
    use rules::grenades::fragmentation;
    use rules::grenades::ion;
    use rules::grenades::poison;
    use rules::grenades::sonic;
    use rules::grenades::stun;
    use rules::grenades::thermal_detonator;
    let blast = |points: u16, damage_type: u8, secondary: Secondary| Effect {
        points,
        droid_points: points,
        damage_type,
        dc: Some(rules::grenades::blast::DC),
        save_halves: true,
        secondary,
        save_spares: true,
    };
    Some(match row {
        // Piercing.
        ITEM_ABILITY_GRENADE_FRAGMENTATION => blast(fragmentation::POINTS, 1, Secondary::None),
        // Stunned, a save spares it.
        ITEM_ABILITY_GRENADE_STUN => Effect {
            save_halves: false,
            ..blast(0, 0, Secondary::Held(stun::HOLD_MS))
        },
        // Energy, and a knockdown.
        ITEM_ABILITY_GRENADE_THERMAL_DETONATOR => {
            blast(thermal_detonator::POINTS, 12, Secondary::Knockdown)
        }
        // Poisoned, a harder save spares it.
        ITEM_ABILITY_GRENADE_POISON => Effect {
            dc: Some(poison::DC),
            save_halves: false,
            ..blast(
                0,
                0,
                Secondary::Poison {
                    points: poison::POINTS,
                    every_ms: poison::EVERY_MS,
                    for_ms: poison::FOR_MS,
                },
            )
        },
        // Sonic, and DEX lowered for a while.
        ITEM_ABILITY_GRENADE_SONIC => blast(
            sonic::POINTS,
            10,
            Secondary::Weaken {
                points: sonic::DEX,
                for_ms: sonic::FOR_MS,
            },
        ),
        // Entangled, no save.
        ITEM_ABILITY_GRENADE_ADHESIVE => Effect {
            dc: None,
            save_halves: false,
            save_spares: false,
            ..blast(0, 0, Secondary::Held(adhesive::HOLD_MS))
        },
        // Cold, and paralysed.
        ITEM_ABILITY_GRENADE_CRYOBAN => {
            blast(cryoban::POINTS, 5, Secondary::Held(cryoban::HOLD_MS))
        }
        // Fire.
        ITEM_ABILITY_GRENADE_FIRE => blast(fire::POINTS, 8, Secondary::None),
        // Ion, worse on droids.
        ITEM_ABILITY_GRENADE_ION => Effect {
            droid_points: ion::DROID_POINTS,
            ..blast(ion::POINTS, 11, Secondary::None)
        },
        _ => return None,
    })
}

/// How strong a throw held `held_ms` is (JA's thermal).
pub fn strength(held_ms: u64) -> f32 {
    (held_ms.min(CHARGE_MS) as f32 / CHARGE_MS as f32).max(MIN_STRENGTH)
}

/// Whether `target` makes its Reflex save against `dc` with `d20` (1 to
/// 20): a natural 20 always, a natural 1 never, else d20 + its Reflex
/// (`Sheet::save`) + Uncanny Dodge's ≥ DC.
pub fn reflex(target: &Sheet, dc: u8, d20: i32) -> bool {
    let bonus = target.save(1) + passive::grenade_save_bonus(&target.feats);
    crate::combat::check(d20, bonus, i32::from(dc))
}

/// The velocity after a bounce off a surface with `normal`: reflected,
/// keeping `BOUNCE` of it; `None` when it comes to rest there.
pub fn bounce(velocity: [f32; 3], normal: [f32; 3]) -> Option<[f32; 3]> {
    let dot: f32 = (0..3).map(|axis| velocity[axis] * normal[axis]).sum();
    let out: [f32; 3] =
        std::array::from_fn(|axis| (velocity[axis] - 2.0 * dot * normal[axis]) * BOUNCE);
    let speed = (out[0] * out[0] + out[1] * out[1] + out[2] * out[2]).sqrt();
    (normal[2] <= rules::grenades::flight::FLOOR || speed >= REST_SPEED).then_some(out)
}

/// A grenade in flight (a `Missile` carries one): its row, and whether it
/// rests where its flight's base is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lob {
    pub row: u16,
    pub resting: bool,
}

impl Lob {
    pub(crate) const BLANK: Lob = Lob {
        row: 0,
        resting: false,
    };

    pub(crate) fn encode(&self, out: &mut Writer) {
        out.u16(self.row);
        out.u8(u8::from(self.resting));
    }

    pub(crate) fn decode(input: &mut Reader) -> Result<Lob, DecodeError> {
        let row = input.u16()?;
        let at = input.at();
        let resting = match input.u8()? {
            0 => false,
            1 => true,
            _ => return Err(input.invalid("resting flag", at)),
        };
        Ok(Lob { row, resting })
    }
}

/// One step of the grenade button for a character whose weapon timer is
/// `timer_ms`: a press with the timer run out and a grenade left starts the
/// charge; the release throws the kind an item talent chose
/// (`Pack::queued`) if one is left, else the first kind left. The throw:
/// the kind, and its strength.
pub fn pull(
    pack: &mut Pack,
    items: &[Option<Carried>; KINDS],
    timer_ms: i32,
    held: bool,
    now: u64,
) -> Option<(Carried, f32)> {
    let grenade = |u| u == Use::Grenade;
    match pack.charging_from {
        Some(_) if held => None,
        Some(from) => {
            pack.charging_from = None;
            let chosen = pack
                .queued
                .filter(|row| items::use_of(*row) == Some(Use::Grenade))
                .and_then(|row| pack.left_of(items, row));
            if chosen.is_some() {
                pack.queued = None;
            }
            let (at, kind) = chosen.or_else(|| pack.next(items, grenade))?;
            pack.spend(at, kind.row, now);
            Some((kind, strength(now.saturating_sub(from))))
        }
        None => {
            if held && timer_ms <= 0 && pack.next(items, grenade).is_some() {
                pack.charging_from = Some(now);
            }
            None
        }
    }
}

/// What a blast's `secondary` (thrown `by`) does to a fighter it caught at
/// `now`.
pub fn afflict(fighter: &mut Fighter, secondary: Secondary, by: SimId, now: u64) {
    match secondary {
        Secondary::None => {}
        Secondary::Held(ms) => {
            fighter.force.hold(now, ms);
        }
        Secondary::Knockdown => fighter.saber.knock_down(now, ja::KNOCKDOWN_MS),
        // Knight and Master Valor and the improved Energy Resistance keep
        // poison off.
        Secondary::Poison { .. } if fighter.effects.poison_immune(now) => {}
        Secondary::Poison {
            points,
            every_ms,
            for_ms,
        } => {
            let course = Course {
                points,
                every_ms: every_ms as u32,
                for_ms: for_ms as u32,
                ..Course::default()
            };
            fighter.pack.poison = Some(Poisoned::new(by, course, now, None));
        }
        Secondary::Weaken { points, for_ms } => {
            fighter.pack.drained = (items::drain(Ability::Dexterity, points), now + for_ms);
        }
    }
}
