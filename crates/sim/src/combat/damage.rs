//! The one pipeline every hit goes through (`docs/COMBAT.md`), in JA's
//! order where JA has one. JA decides whether a hit landed (the caller: a bolt
//! met a body, a blade swept through one) and whether it was blocked; this
//! decides how much, from KOTOR's numbers. Two steps:
//!
//! `assess` (no state changes):
//! 1. the size: a weapon's dice, bonus dice and item bonus (+STR melee,
//!    its specialization's bonus) × the margin `p`, KOTOR's chance to hit
//!    (`margin`, `rules::combat::margin`; C1), × the pace
//!    `PACE` × cadence / `ROUND_MS` (C2; a blade's cadence is the time since its
//!    holder's last attack began, `sweep::Swing::begin`), × `crithitmult` on
//!    a d20 in the threat range (C4), plus sneak attack d6s never
//!    multiplied, then the target's
//!    guards by damage type (immunity, vulnerability, resistance; the
//!    weapon's least guarded type; a bonus against the target's race as
//!    its own type: step 9) and its feats' damage reduction (Improved and
//!    Wookiee Toughness, `passive::toughness`), applied before the margin
//!    and pace; a
//!    power's JA points (a point is 1% of the target's maximum:
//!    KOTOR's impact stand-in until scripts run); a script's whole points;
//! 2. JA's hit-location multiplier, then the shot's share (a shard's fifth);
//! 3. the knockback, from that damage (`rules::combat::knockback`);
//!    sabers and scripts none; none from a hit step 4
//!    refuses (JA pushes with it all the same, QUIRKS.md Q-0031, fixed);
//! 4. may it harm (C17): a script always; a plot target never; one side
//!    (the party, or allies by faction) by the friendly-fire mode; a
//!    hostile yes; a neutral outsider never from the party's attacks, nor
//!    from anyone's area (KOTOR's blasts and area powers take the caster's
//!    enemies: Tatooine's banthas live through the Sand People's
//!    grenades [verify]), but a stray bolt from anyone else does hurt it;
//! 5. at least `rules::combat::damage::LEAST` when it harms (JA's minimum of 1).
//!
//! `apply`: the Min1HP floor, the hit points, death, the event.
//!
//! Integer arithmetic from the roll on (hundredths of an HP, C3; factors in
//! thousandths, saturating); the dice and the crit come from the `authority`
//! stream, so prediction never draws them.

use crate::combat::fighter::{Event, Fighter, Life, Location, Outcome};
use crate::combat::forms;
use crate::combat::gear::{Caps, Roll, immunity};
use crate::combat::grenades;
use crate::combat::passive::{self, Game};
use crate::combat::sheet::{Ability, Delivery, Sheet, Weapon, feats, flags, races, type_rows};
use crate::inventory::properties::OWN_TYPE;
use crate::rng::SimRng;
use crate::snapshot::SimId;

/// I22's pace constant P (C2).
pub const PACE: i64 = rules::combat::damage::PACE;
/// KOTOR's round: the time the pace is measured against.
pub use crate::combat::ROUND_MS;
/// The cadence a whole roll is paced at (ms): KOTOR's round over P.
pub const WHOLE_CADENCE_MS: u32 = (ROUND_MS / PACE as u64) as u32;
/// How many times over a mighty character's blows count (`flags::MIGHTY`).
pub const MIGHTY_TIMES: i64 = rules::combat::damage::MIGHTY_TIMES;

/// Who can hurt whom in the party (C17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FriendlyFire {
    /// The party never hurts itself (JA's and KOTOR's default).
    #[default]
    Off,
    /// Area attacks hurt allies at half; aimed ones do not.
    Area,
    /// Everything hurts allies.
    Full,
}

impl FriendlyFire {
    /// Whether this mode keeps `relation`'s attacker's blades, fists and
    /// powers off its target altogether: one side (or the same character)
    /// with friendly fire off. What a blow that lands takes is `share`'s.
    pub fn spares(self, relation: Relation) -> bool {
        self == FriendlyFire::Off && matches!(relation, Relation::Itself | Relation::Party)
    }
}

/// How the attacker stands to the target (`module::relation`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relation {
    /// The same character (a grenade at one's feet).
    Itself,
    /// On one side: both in the party (players and their companions), or
    /// allies by faction (friendly, `Repute::friendly`). KOTOR never lets a
    /// side hurt its own; JA's NPC teams obey friendly fire as players do.
    Party,
    /// Hostile by faction.
    Hostile,
    /// Neither: a neutral outsider, `aimed` at when its attacker is outside
    /// the party and it is that one's target. KOTOR's attack lands on its
    /// target alone; JA's swings and bolts meet whoever stands in the way
    /// (a Sand Person's gaffi stick cut down the bantha by the player).
    Outsider { aimed: bool },
}

/// What dealt a hit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Source {
    /// A weapon's attack: the weapon, the attack's cadence (its refire or
    /// move time, ms), the share of the attack this hit carries in
    /// thousandths (a flechette shard a fifth) and whether it hit an area
    /// (splash, cones).
    Weapon {
        weapon: Weapon,
        cadence_ms: u32,
        share: u16,
        area: bool,
    },
    /// A power's JA points (a point is 1% of the target's maximum) and up
    /// to `extra` more by the roll (lightning's 1 or 2), whether it hit an
    /// area, whether it pushes, and its damage type if it has one
    /// (lightning's electrical: what an energy shield asks; JA's points are
    /// not KOTOR attacks, so resistances leave them be).
    Power {
        points: i32,
        extra: u8,
        area: bool,
        pushes: bool,
        damage_type: Option<u8>,
    },
    /// A script's `EffectDamage`: whole points of a damage type row (none:
    /// untyped), never filtered.
    Script {
        damage: u16,
        damage_type: Option<u8>,
        /// An impact script's (a cast's: a power, a grenade): between
        /// friends it keeps the friendly-fire rules of an area attack.
        cast: bool,
        /// A scripted shot's (`CutsceneAttack` fired): the gun it came
        /// from, the hit's weapon; a scene's melee blow's (`Strike`): the
        /// attacker's melee weapon in hand.
        shot: Option<Delivery>,
        /// A blow the story stages (`death::in_place`): a script's own
        /// `EffectDamage` (not a cast's), a scene's melee blow, a scripted
        /// shot. The sim's own script-shaped damage (a poison's doses, Force
        /// stun's sting, a saber lock's loss) is free play's, not staged.
        staged: bool,
    },
    /// A grenade's blast (`combat::grenades`, by its `spells.2da` row):
    /// KOTOR's whole points of its type, halved by a Reflex save; an area.
    Blast { row: u16 },
    /// A script's `EffectDeath`: its target dies, a party member too,
    /// downed or not: the story's death (I30 P13); never filtered.
    Death,
}

/// One hit to resolve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blow {
    pub attacker: Option<SimId>,
    pub relation: Relation,
    pub source: Source,
    pub location: Location,
    /// Unit direction the hit pushes (game space), if it pushes.
    pub direction: Option<[f32; 3]>,
    /// Sneak attack d6s a weapon's hit adds (`combat::specials`).
    pub sneak: u8,
    /// What the attacker's TSL autobalance multiplies each of its weapon's
    /// dice by (`combat::balance::dice`); 1 for none.
    pub dice: u8,
}

/// What a hit is worth, before it lands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Assessed {
    pub outcome: Outcome,
    /// Hundredths of an HP it takes (0 when refused).
    pub damage: i32,
    /// Units per second to add to the target's velocity, and the
    /// knockback timer (ms) to hold it with.
    pub push: Option<([f32; 3], i32)>,
    /// A blast's Reflex save was made.
    pub saved: bool,
    /// The damage type it dealt (a `sheet::DAMAGE_TYPES` row), if typed:
    /// what an energy shield asks.
    pub damage_type: Option<u8>,
}

/// What a hit did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Resolved {
    pub outcome: Outcome,
    /// Hundredths of an HP taken.
    pub damage: i32,
    pub push: Option<([f32; 3], i32)>,
    pub died: bool,
    /// A party member went down (I30 P11).
    pub downed: bool,
}

/// The attacker's attack bonus with `weapon` (whole points): BAB, what runs
/// on it (`Sheet::attack`) and the item's bonus (with the hands' feats as
/// the weapon was taken in hand, `passive::hand_attack`) within the caps
/// (`gear::Caps`), DEX (ranged) or STR (melee), the weapon's focus's bonus,
/// less `rules::feats::weapon::NOT_PROFICIENT` without its proficiency (a
/// template's weapon: a player cannot equip one).
pub fn attack_bonus(sheet: &Sheet, weapon: &Weapon) -> i32 {
    let ability = if weapon.delivery.ranged() {
        Ability::Dexterity
    } else {
        Ability::Strength
    };
    let effects = Caps::hold(
        Caps::of(sheet).attack,
        i32::from(sheet.attack) + i32::from(weapon.attack_bonus),
    );
    let mut bonus = i32::from(sheet.bab) + effects + sheet.modifier(ability);
    if weapon.focus.is_some_and(|feat| sheet.feats.has(feat)) {
        bonus += rules::feats::weapon::FOCUS;
    }
    let proficient = weapon.proficiency.is_none_or(|feat| sheet.feats.has(feat))
        || sheet.feats.has(feats::PROFICIENCY_ALL);
    if !proficient {
        bonus -= rules::feats::weapon::NOT_PROFICIENT;
    }
    bonus
}

/// What STR adds to `weapon`'s damage for `sheet`: its modifier in melee;
/// ranged, its penalty, or with Mighty its bonus up to Mighty's most.
fn strength(sheet: &Sheet, weapon: &Weapon) -> i64 {
    let modifier = sheet.modifier(Ability::Strength);
    let add = if !weapon.delivery.ranged() || modifier < 0 {
        modifier
    } else {
        modifier.min(weapon.mighty.map_or(0, i32::from))
    };
    i64::from(add)
}

/// A die of `sides` (1 to `sides`), whole points: what the damage rolls
/// draw from (the `authority` stream in a hit, a mind's hash in a round).
pub type Die<'a> = &'a mut dyn FnMut(u8) -> i64;

/// `count` dice of `sides` rolled.
fn dice(count: u8, sides: u8, die: Die<'_>) -> i64 {
    (0..count).map(|_| die(sides.max(1))).sum()
}

/// `roll` rolled: its dice and its flat bonus.
fn rolled(roll: Roll, die: Die<'_>) -> i64 {
    dice(roll.dice, roll.die, die) + i64::from(roll.bonus)
}

/// `die` on the `authority` stream.
fn stream_die(rng: &mut SimRng) -> impl FnMut(u8) -> i64 + '_ {
    |sides| i64::from(rng.below(u32::from(sides))) + 1
}

/// The two sides of KOTOR's attack roll with `weapon`: the attacker's bonus
/// (`attack_bonus`, its weapon's against the target's kind and its TSL Form's,
/// `forms::to_hit`) and the target's defence against it (its own, its gear's
/// against the attacker and the weapon's damage types).
pub fn odds(attacker: &Sheet, weapon: &Weapon, target: &Sheet) -> (i32, i32) {
    let versus = weapon
        .versus
        .filter(|versus| versus.against.matches(target, 0));
    let attack = attack_bonus(attacker, weapon)
        + versus.map_or(0, |versus| i32::from(versus.attack))
        + forms::to_hit(attacker, target);
    let defence = target.defence() + target.gear.defence_against(attacker, weapon.damage_types);
    (attack, defence)
}

/// Whether a hit whose d20 was `d20` is a critical (reone's attack roll):
/// the d20 in `weapon`'s threat range (1: a natural 20), the target not
/// immune to criticals, and a second d20 (`confirm`, drawn only then)
/// plus the attack bonus reaching the defence (`odds`, with what the
/// attacker's TSL Form adds to a confirmation, `confirming`; none known: no
/// confirmation asked).
pub fn critical(
    d20: i64,
    weapon: &Weapon,
    target: &Sheet,
    odds: Option<(i32, i32)>,
    confirm: &mut dyn FnMut() -> i64,
) -> bool {
    let threat = 21 - i64::from(weapon.crit_threat.max(1));
    d20 >= threat
        && !target.gear.immune_to(immunity::CRITICAL_HIT)
        && odds.is_none_or(|(attack, defence)| confirm() + i64::from(attack) >= i64::from(defence))
}

/// `odds` for confirming `attacker`'s critical: Juyo's +4 (`forms::confirm`).
pub fn confirming(attacker: &Sheet, (attack, defence): (i32, i32)) -> (i32, i32) {
    (attack + forms::confirm(attacker), defence)
}

/// A weapon hit's own roll (whole points): its dice (each × `factor`, an
/// autobalanced attacker's, TSL's `GetDamageRoll` and `GetUnarmedDamageRoll`:
/// `combat::balance`), its bonus dice and its item bonus, with its
/// attacker's STR (`strength`) and its specialization's bonus; at least
/// `rules::combat::damage::LEAST_ROLL`.
pub fn weapon_roll(weapon: &Weapon, attacker: Option<&Sheet>, factor: u8, die: Die<'_>) -> i64 {
    let mut roll = dice(weapon.dice, weapon.die, die) * i64::from(factor)
        + dice(weapon.bonus_dice, weapon.bonus_die, die)
        + i64::from(weapon.damage_bonus);
    if let Some(sheet) = attacker {
        roll += strength(sheet, weapon);
        if weapon
            .specialization
            .is_some_and(|feat| sheet.feats.has(feat))
        {
            roll += rules::feats::weapon::SPECIALIZATION;
        }
    }
    roll.max(rules::combat::damage::LEAST_ROLL)
}

/// What a weapon hit of `roll` (`weapon_roll`) does to `target` (whole
/// points) and the damage type row it dealt as: × the weapon's critical
/// multiplier on a critical; sneak attack d6s (none on a target immune to
/// sneak attacks or criticals) and Massive Criticals' dice never
/// multiplied; then the types (step 9, after the crit as in KOTOR): the
/// weapon's damage as the type its target guards least, less its soak
/// (physical damage from a weapon of too little enhancement); other types'
/// bonuses and one against the target's kind as their own types (×
/// the critical's multiplier); less the target's feats' toughness.
#[allow(clippy::too_many_arguments)]
pub fn weapon_damage(
    weapon: &Weapon,
    attacker: Option<&Sheet>,
    target: &Sheet,
    roll: i64,
    crit: bool,
    sneak: u8,
    game: Game,
    die: Die<'_>,
) -> (i64, Option<usize>) {
    let mult = if crit {
        i64::from(weapon.crit_mult.max(1))
    } else {
        1
    };
    let sneaks = !target.gear.immune_to(immunity::SNEAK_ATTACK)
        && !target.gear.immune_to(immunity::CRITICAL_HIT);
    let sneak = dice(
        if sneaks { sneak } else { 0 },
        rules::feats::sneak_attack::DIE,
        die,
    );
    let massive = if crit { rolled(weapon.massive, die) } else { 0 };
    let versus = attacker.and(
        weapon
            .versus
            .filter(|versus| versus.against.matches(target, 0)),
    );
    let guards = &target.guards;
    let whole = roll * mult + sneak + massive;
    let row = guards.weakest(weapon.damage_types, whole);
    let mut hurt = row.map_or(whole, |row| guards.take(row, whole));
    let gear = &target.gear;
    if row.is_some_and(|row| row <= usize::from(type_rows::SLASHING))
        && weapon.enhancement < gear.soak_bypass
    {
        hurt = (hurt - i64::from(gear.soak)).max(0);
    }
    let own = row.map_or(0, |row| row as u8);
    let typed = weapon
        .extras
        .iter()
        .flatten()
        .map(|extra| (extra.damage_type, extra.roll))
        .chain(versus.map(|versus| (versus.damage_type, versus.damage)));
    for (kind, extra) in typed {
        let kind = if kind == OWN_TYPE { own } else { kind };
        let extra = rolled(extra, die).max(0) * mult;
        hurt += guards.take(usize::from(kind), extra);
    }
    hurt -= passive::toughness(&target.feats, game, row, hurt);
    (hurt, row)
}

/// KOTOR's chance to hit, in thousandths (C1, `rules::combat::margin`).
pub fn margin(attack_bonus: i32, defence: i32) -> i64 {
    use rules::combat::margin;
    (i64::from(margin::BASE + attack_bonus - defence) * 1000 / margin::STEP)
        .clamp(margin::LEAST, margin::MOST)
}

/// KOTOR's chance that `attacker`'s `weapon` hits `target`, in thousandths
/// (`odds` through `margin`).
pub fn chance(attacker: &Sheet, weapon: &Weapon, target: &Sheet) -> i64 {
    let (attack, defence) = odds(attacker, weapon, target);
    margin(attack, defence)
}

/// The pace factor for an attack of `cadence_ms`, in thousandths (C2).
pub fn pace(cadence_ms: u32) -> i64 {
    PACE * i64::from(cadence_ms) * 1000 / ROUND_MS as i64
}

/// Whether the attacker may harm the target, at what share (thousandths)
/// (C17).
pub fn share(relation: Relation, source: &Source, target: &Sheet, mode: FriendlyFire) -> i64 {
    use rules::combat::friendly_fire;
    if matches!(source, Source::Death) {
        return 1000;
    }
    if target.flags & flags::PLOT != 0 {
        return 0;
    }
    // A script's damage hurts whoever it names, friend or foe, but not a
    // plot creature (as Aurora's `EffectDamage`).
    if let Source::Script { cast, .. } = source {
        let friends = matches!(relation, Relation::Party | Relation::Itself);
        if !(*cast && friends) {
            return 1000;
        }
        return match mode {
            FriendlyFire::Off => 0,
            _ if relation == Relation::Itself => friendly_fire::OWN,
            FriendlyFire::Area => friendly_fire::ALLY_AREA,
            FriendlyFire::Full => 1000,
        };
    }
    let area = matches!(
        source,
        Source::Weapon { area: true, .. } | Source::Power { area: true, .. } | Source::Blast { .. }
    );
    match (relation, mode) {
        (Relation::Hostile, _) => 1000,
        (Relation::Outsider { aimed: false }, _) => 0,
        (Relation::Outsider { aimed: true }, _) if area => 0,
        (Relation::Outsider { aimed: true }, _) => 1000,
        (Relation::Party | Relation::Itself, FriendlyFire::Off) => 0,
        (Relation::Party, FriendlyFire::Area) if !area => 0,
        (Relation::Party, FriendlyFire::Area) => friendly_fire::ALLY_AREA,
        (Relation::Party, FriendlyFire::Full) => 1000,
        (Relation::Itself, _) => friendly_fire::OWN,
    }
}

/// What `blow` is worth against `target`, drawing its dice from `rng` (the
/// `authority` stream); nothing changes yet.
pub fn assess(
    blow: &Blow,
    attacker: Option<&Sheet>,
    target: &Sheet,
    mode: FriendlyFire,
    game: Game,
    rng: &mut SimRng,
) -> Assessed {
    let mut outcome = Outcome::Hit;
    // Its types took all of it: no minimum hundredth.
    let mut guarded = false;
    let mut saved = false;
    let mut damage_type = None;
    // A test's mighty player (`kjk playthrough --mighty`): its blows count
    // `MIGHTY_TIMES` over, scripts' as they are; a weapon's before its
    // target's guards, as a stronger blow (a door's hardness of 125, the
    // Sky Ramp's damaged force shield, meets ten times the blade's roll).
    let mighty = attacker.is_some_and(|sheet| sheet.flags & flags::MIGHTY != 0)
        && !matches!(blow.source, Source::Script { .. });
    let (sized, pushes) = match blow.source {
        Source::Script {
            damage,
            damage_type: kind,
            ..
        } => {
            // Through the target's guards of its type, as a weapon's or a
            // grenade's: Force lightning (`k_sp1_generic`'s EffectDamage)
            // meets energy resistance and the Force shields.
            damage_type = kind;
            let hurt = kind.map_or(i64::from(damage), |kind| {
                target.guards.take(usize::from(kind), i64::from(damage))
            });
            guarded = hurt == 0 && damage > 0;
            (hurt * 100, false)
        }
        Source::Death => (i64::from(i32::MAX), false),
        Source::Blast { row } => match grenades::effect(row) {
            Some(effect) => {
                damage_type = Some(effect.damage_type);
                let d20 = rng.below(rules::interaction::checks::DIE) as i32 + 1;
                saved = effect
                    .dc
                    .is_some_and(|dc| grenades::reflex(target, dc, d20));
                let points = if target.race == races::DROID {
                    effect.droid_points
                } else {
                    effect.points
                };
                let points = if saved && effect.save_halves {
                    points / rules::grenades::blast::SAVE_DIVIDES
                } else {
                    points
                };
                let row = usize::from(effect.damage_type);
                let hurt = target.guards.take(row, i64::from(points));
                let hurt = hurt - passive::toughness(&target.feats, game, Some(row), hurt);
                guarded = hurt == 0;
                (hurt * 100, true)
            }
            None => (0, false),
        },
        Source::Power {
            points,
            extra,
            pushes,
            damage_type: typed,
            ..
        } => {
            damage_type = typed;
            let roll = if extra > 0 {
                rng.below(u32::from(extra) + 1)
            } else {
                0
            };
            let points = i64::from(points) + i64::from(roll);
            (points.saturating_mul(i64::from(target.max_hp)), pushes)
        }
        Source::Weapon {
            weapon,
            cadence_ms,
            share: part,
            ..
        } => {
            // The margin model (C1): the hit landed (its blade or bolt met
            // the body), its size the chance KOTOR's roll would have hit.
            let odds = attacker.map(|sheet| odds(sheet, &weapon, target));
            let p = odds.map_or(1000, |(attack, defence)| margin(attack, defence));
            let roll = weapon_roll(&weapon, attacker, blow.dice, &mut stream_die(rng));
            let roll = if mighty {
                roll.saturating_mul(MIGHTY_TIMES)
            } else {
                roll
            };
            let d20 = i64::from(rng.below(rules::interaction::checks::DIE)) + 1;
            let confirm = attacker
                .zip(odds)
                .map(|(sheet, odds)| confirming(sheet, odds));
            let crit = critical(d20, &weapon, target, confirm, &mut || {
                i64::from(rng.below(rules::interaction::checks::DIE)) + 1
            });
            if crit {
                outcome = Outcome::Critical;
            }
            let (hurt, row) = weapon_damage(
                &weapon,
                attacker,
                target,
                roll,
                crit,
                blow.sneak,
                game,
                &mut stream_die(rng),
            );
            damage_type = row.map(|row| row as u8);
            guarded = hurt == 0;
            let centi = (hurt * 100 * p / 1000).saturating_mul(pace(cadence_ms)) / 1000;
            (
                centi.saturating_mul(i64::from(part)) / 1000,
                !matches!(weapon.delivery, Delivery::Saber(_)),
            )
        }
    };
    let located = sized.saturating_mul(blow.location.per_mille()) / 1000;
    let share = share(blow.relation, &blow.source, target, mode);
    // Knockback by JA-equivalent points (damage × 100 / max HP), from the
    // damage before the share, capped (`rules::combat::knockback`); a hit
    // that may not harm pushes nothing either (QUIRKS.md Q-0031, fixed).
    use rules::combat::knockback;
    let equivalent = (located / i64::from(target.max_hp.max(1))).min(knockback::MOST_POINTS) as i32;
    let push = blow
        .direction
        .filter(|_| pushes && equivalent > 0 && share > 0)
        .map(|dir| {
            let speed = knockback::PER_POINT * equivalent as f32;
            (
                dir.map(|axis| axis * speed),
                (equivalent * knockback::MS_PER_POINT)
                    .clamp(knockback::LEAST_MS, knockback::MOST_MS),
            )
        });
    let damage = if share == 0 {
        outcome = Outcome::Refused;
        0
    } else if guarded {
        0
    } else {
        (located.saturating_mul(share) / 1000).max(rules::combat::damage::LEAST)
    };
    let damage = if mighty && !matches!(blow.source, Source::Weapon { .. }) {
        damage.saturating_mul(MIGHTY_TIMES)
    } else {
        damage
    };
    Assessed {
        outcome,
        damage: i32::try_from(damage).unwrap_or(i32::MAX),
        push,
        saved,
        damage_type,
    }
}

/// Lands an assessed hit on `state` (the target's, with its sheet; `party`
/// whether it is in the party): Min1HP holds it at 1 HP, the hit points
/// fall, a fall or a death is marked, the attacker remembered and the event
/// recorded. At 0 HP a party member is downed (I30 P11) and anyone else
/// dies; a `Source::Death` kills a party member too, downed or standing.
pub fn apply(
    blow: &Blow,
    assessed: Assessed,
    target: &Sheet,
    state: &mut Fighter,
    now: u64,
    tick: u64,
    party: bool,
) -> Resolved {
    let mut damage = assessed.damage;
    if target.flags & (flags::MIN_ONE_HP | flags::IMMORTAL) != 0 {
        damage = damage.min(state.hp.saturating_sub(100).max(0));
    }
    state.hp = state.hp.saturating_sub(damage);
    let death = matches!(blow.source, Source::Death);
    let falls = state.hp <= 0 && (state.alive() || (death && state.downed()));
    let downed = falls && party && !death;
    let died = falls && !downed;
    if downed {
        state.fall(now);
    } else if died {
        state.life = Life::Dead;
        state.fallen = None;
    }
    // Attacked: a hit that harms, or a foe's that does not (a plot
    // character struck still hears of it and answers, as KOTOR's
    // OnPhysicalAttacked fires: Jolee and the katarns on Kashyyyk).
    if let Some(by) = blow.attacker
        && (damage > 0 || blow.relation == Relation::Hostile)
    {
        state.last_attacker = Some((by, now));
    }
    state.record(Event {
        sequence: 0,
        tick,
        by: blow.attacker,
        result: assessed.outcome,
        location: blow.location,
        damage,
        weapon: match blow.source {
            Source::Weapon { weapon, .. } => Some(weapon.delivery),
            Source::Script { shot, .. } => shot,
            _ => None,
        },
        at: None,
        visual: None,
    });
    Resolved {
        outcome: assessed.outcome,
        damage,
        push: assessed.push,
        died,
        downed,
    }
}
