//! Passive feats (`feat.2da`): what a feat a character simply has does,
//! where its rule applies, by the rows K1 and TSL share. A line's tiers
//! replace each other and each needs the one before (`prereqfeat`), so how
//! many of a line's rows a sheet holds is its tier.
//!
//! The rules are the feats' own descriptions (the talk table) and
//! `baseitems.2da`'s; the class defence feats (Scoundrel's Luck, Jedi
//! Sense, the droids' Logic Upgrades) are `acbonus.2da`'s class columns,
//! which the sheet already adds.

use crate::combat::sheet::{Delivery, Feats, Sheet, Weapon, build, feats, type_rows};

/// The game whose rules a feat follows where K1 and TSL differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Game {
    #[default]
    K1,
    Tsl,
}

impl Game {
    /// From a module's game name (`k1`, `tsl`).
    pub fn named(name: &str) -> Game {
        if name.eq_ignore_ascii_case("tsl") {
            Game::Tsl
        } else {
            Game::K1
        }
    }
}

/// The talk tables' (K1's and TSL's) "You do not have the proficiencies
/// required to equip that item.": an equip `may_equip` refuses.
pub const NOT_PROFICIENT: u32 = 1475;

/// Two-weapon fighting's to hit by Two-Weapon Fighting's tier (none,
/// the feat, Improved, Master): the main hand's and the off hand's.
const MAIN_HAND: [i32; 4] = rules::feats::two_weapon::MAIN_HAND;
const OFF_HAND: [i32; 4] = rules::feats::two_weapon::OFF_HAND;
/// A balanced (small) weapon in the off hand: to the main hand's.
const BALANCED: i32 = rules::feats::two_weapon::BALANCED;
/// TSL's Superior Two-Weapon Fighting by its highest rank: to the main
/// hand's and to the off hand's.
const SUPERIOR_MAIN: [i32; 3] = rules::feats::two_weapon::SUPERIOR_MAIN;
const SUPERIOR_OFF: [i32; 3] = rules::feats::two_weapon::SUPERIOR_OFF;
/// A pistol in each hand: to each hand's.
const PAIR_AIM: i32 = rules::weapons::pistol::PAIR_AIM;
/// Wookiee Toughness's HP a level by its tier.
const WOOKIEE_HP: [i32; 4] = rules::feats::toughness::WOOKIEE_HP;
/// Force Sensitive's Force points.
const FORCE_SENSITIVE_FP: u16 = rules::feats::force_sensitive::POINTS;

fn tier(feats: &Feats, line: &[u16]) -> i32 {
    i32::from(feats.count(line))
}

/// The highest tier of `line` a sheet holds, from 1; 0 for none. KOTOR 2
/// grants some lines' later tiers at class levels whatever the tiers
/// before them (a Scout's Targeting III at level 9), so a line's rule reads
/// its highest tier, not how many of its rows are held.
pub fn best_tier(feats: &Feats, line: &[u16]) -> usize {
    line.iter()
        .rposition(|row| feats.has(*row))
        .map_or(0, |at| at + 1)
}

/// `values`' entry for the highest tier of `line` the sheet holds
/// (`best_tier`), or `none` without one.
fn by_tier<T: Copy>(feats: &Feats, line: &[u16], values: &[T], none: T) -> T {
    best_tier(feats, line)
        .checked_sub(1)
        .and_then(|at| values.get(at).copied())
        .unwrap_or(none)
}

/// Targeting I to VIII: their bonus to hit with a ranged weapon
/// (`rules::feats::targeting::ATTACK`).
pub fn targeting(feats: &Feats, weapon: &Weapon) -> i32 {
    if !weapon.delivery.ranged() {
        return 0;
    }
    by_tier(
        feats,
        &feats::TARGETING,
        &rules::feats::targeting::ATTACK,
        0,
    )
}

/// Whether Finesse lets `sheet` hit with `weapon` by Dexterity where
/// Strength would serve: Finesse: Lightsabers with a lightsaber, Finesse:
/// Melee Weapons with any other melee weapon (fists are none).
pub fn finesse(feats: &Feats, weapon: &Weapon) -> bool {
    match weapon.delivery {
        Delivery::Saber(_) => feats.has(feats::FINESSE_LIGHTSABERS),
        Delivery::Melee | Delivery::Baton => feats.has(feats::FINESSE_MELEE_WEAPONS),
        Delivery::Unarmed | Delivery::Blaster(_) => false,
    }
}

/// Precise Shot I to V: their bonus to a ranged weapon's damage
/// (`rules::feats::precise_shot::DAMAGE`).
pub fn precise_shot(feats: &Feats, weapon: &Weapon) -> i64 {
    if !weapon.delivery.ranged() {
        return 0;
    }
    by_tier(
        feats,
        &feats::PRECISE_SHOT,
        &rules::feats::precise_shot::DAMAGE,
        0,
    )
}

/// Precise Shot I to V: the points their holder's bolts from `weapon` (a
/// ranged weapon) take off the deflection bonus of whoever tries to turn
/// them (`rules::feats::precise_shot::DEFLECT_PENALTY`).
pub fn deflect_penalty(feats: &Feats, weapon: &Weapon) -> i8 {
    if !weapon.delivery.ranged() {
        return 0;
    }
    let penalties = rules::feats::precise_shot::DEFLECT_PENALTY;
    by_tier(feats, &feats::PRECISE_SHOT, &penalties, 0)
}

/// `deflect` (a weapon's deflection bonus) with `penalty` taken off
/// (`deflect_penalty`), within the bonus's range.
pub fn deflect_after(deflect: i8, penalty: i8) -> i8 {
    deflect.saturating_sub(penalty)
}

/// Close Combat and Improved Close Combat: their bonus to hit with a
/// ranged weapon at a target `distance_m` away, within short range
/// (`rules::feats::close_combat`).
pub fn close_combat(feats: &Feats, weapon: &Weapon, distance_m: f32) -> i32 {
    use rules::feats::close_combat;
    if !weapon.delivery.ranged() || distance_m > close_combat::SHORT_RANGE_M {
        return 0;
    }
    by_tier(feats, &feats::CLOSE_COMBAT, &close_combat::ATTACK, 0)
}

/// Dual Strike, Improved and Master: their bonus to hit a target that
/// another of the attacker's side attacks (`rules::feats::dual_strike`).
pub fn dual_strike(feats: &Feats) -> i32 {
    by_tier(
        feats,
        &feats::DUAL_STRIKE,
        &rules::feats::dual_strike::ATTACK,
        0,
    )
}

/// Unarmed Specialist I to VIII: the dice of
/// `rules::feats::unarmed_specialist::DIE` they add to an unarmed attack.
pub fn unarmed_dice(feats: &Feats, weapon: &Weapon) -> u8 {
    if weapon.delivery != Delivery::Unarmed {
        return 0;
    }
    let dice = rules::feats::unarmed_specialist::DICE;
    by_tier(feats, &feats::UNARMED_SPECIALIST, &dice, 0)
}

/// Whether Evasion spares `feats`' holder the half a successful Reflex
/// save leaves of a blast.
pub fn evades(feats: &Feats) -> bool {
    feats.has(feats::EVASION)
}

/// Conditioning, Improved, Master: +1, +2, +3 to every save.
pub fn save_bonus(feats: &Feats) -> i32 {
    tier(feats, &feats::CONDITIONING)
}

/// Uncanny Dodge 1 and 2: `rules::feats::saves::UNCANNY_DODGE` a tier to
/// saves against grenades.
pub fn grenade_save_bonus(feats: &Feats) -> i32 {
    rules::feats::saves::UNCANNY_DODGE * tier(feats, &feats::UNCANNY_DODGE)
}

/// What feats add to `skill` (`skills` row): each line's tier by its own
/// step (`feats::SKILL_LINES`) and Skill Focus's
/// `rules::feats::skills::SKILL_FOCUS`. Both games' executables add them
/// whether or not the character has ranks in the skill. A skill only the
/// trained may use is 0 without ranks of its own, whatever the feats add,
/// and `stealth::total` applies that rule.
pub fn skill_bonus(feats: &Feats, skill: usize) -> i32 {
    let lines: i32 = feats::SKILL_LINES
        .iter()
        .filter(|(_, _, raised)| raised.contains(&skill))
        .map(|(line, per_tier, _)| per_tier * tier(feats, line))
        .sum();
    let focus = feats::SKILL_FOCUS
        .iter()
        .filter(|(row, of)| *of == skill && feats.has(*row))
        .count() as i32;
    lines + rules::feats::skills::SKILL_FOCUS * focus
}

/// Dueling, Improved, Master: +1, +2, +3 to hit and to defence with one
/// one-handed weapon in the right hand and nothing in the left.
pub fn dueling(sheet: &Sheet) -> i32 {
    match sheet.weapons {
        [Some(weapon), None] if weapon.build & build::ONE_HANDED != 0 => {
            tier(&sheet.feats, &feats::DUELING)
        }
        _ => 0,
    }
}

/// To hit with the weapon in `hand` (0 right, 1 left; an empty hand
/// fights with the right's) for how the hands are filled.
///
/// A character with a weapon in each hand, or with a double weapon
/// (`build::DOUBLE`), takes two-weapon fighting's penalty at their Two-Weapon
/// Fighting tier: `MAIN_HAND` for hand 0 and `OFF_HAND` for hand 1. A
/// balanced off-hand weapon eases the main hand's penalty by `BALANCED`. A
/// small weapon in the left hand is balanced, and so is a double weapon,
/// which is its own off-hand weapon. Both games' attack rolls count a
/// double weapon's ends this way. TSL's Superior Two-Weapon Fighting eases
/// them further by its highest rank (`SUPERIOR_MAIN`, `SUPERIOR_OFF`), with
/// blasters as with blades. A pistol in each hand eases both penalties by
/// `PAIR_AIM`. A character with one one-handed weapon alone gains Dueling's
/// bonus instead.
pub fn hand_attack(sheet: &Sheet, hand: u8) -> i32 {
    let tier = tier(&sheet.feats, &feats::TWO_WEAPON_FIGHTING).min(3) as usize;
    let off = match sheet.weapons {
        [Some(_), Some(off)] => off,
        [Some(main), None] if main.build & build::DOUBLE != 0 => main,
        _ => return dueling(sheet),
    };
    let pair = if sheet.pistol_pair() { PAIR_AIM } else { 0 };
    let superior =
        |by_rank: &[i32; 3]| by_tier(&sheet.feats, &feats::SUPERIOR_TWO_WEAPON, by_rank, 0);
    if hand == 1 {
        return OFF_HAND[tier] + superior(&SUPERIOR_OFF) + pair;
    }
    let balanced = if off.build & (build::SMALL | build::DOUBLE) != 0 {
        BALANCED
    } else {
        0
    };
    MAIN_HAND[tier] + balanced + superior(&SUPERIOR_MAIN) + pair
}

/// Whether one with `feats` may equip an item needing `needs`
/// (`baseitems.2da` `reqfeat0` to `reqfeat4`): every one held, a
/// proficiency also by Proficiency All.
pub fn may_equip(feats: &Feats, needs: &[u16]) -> bool {
    let all = feats.has(feats::PROFICIENCY_ALL);
    needs
        .iter()
        .all(|need| feats.has(*need) || (all && feats::PROFICIENCIES.contains(need)))
}

/// HP every level gives beside its hit die and CON: Toughness's and
/// Master Toughness's 1 each, Wookiee Toughness's by its tier (`WOOKIEE_HP`).
pub fn hp_per_level(feats: &Feats) -> i32 {
    let wookiee = tier(feats, &feats::WOOKIEE_TOUGHNESS).min(3) as usize;
    tier(feats, &feats::TOUGHNESS) + WOOKIEE_HP[wookiee]
}

/// What learning `feat` gives a sheet once, for `levels` levels gained
/// before it: Toughness and Master Toughness their HP for each
/// (retroactive), Force Sensitive its Force points.
pub fn learn(sheet: &mut Sheet, feat: u16, levels: u8) {
    if feats::TOUGHNESS.contains(&feat) {
        sheet.max_hp = sheet.max_hp.saturating_add(u16::from(levels));
    } else if feat == feats::FORCE_SENSITIVE {
        sheet.max_fp = sheet.max_fp.saturating_add(FORCE_SENSITIVE_FP);
    }
}

/// What `learn` gave a sheet of `levels` levels, taken back, for a tool
/// that takes a feat away outside play (the save editor; KOTOR never
/// does). Toughness's HP are one a level whenever it was learned (learning
/// gives those before, each level after its own), so they all go.
pub fn unlearn(sheet: &mut Sheet, feat: u16, levels: u8) {
    if feats::TOUGHNESS.contains(&feat) {
        sheet.max_hp = sheet.max_hp.saturating_sub(u16::from(levels));
    } else if feat == feats::FORCE_SENSITIVE {
        sheet.max_fp = sheet.max_fp.saturating_sub(FORCE_SENSITIVE_FP);
    }
}

/// Whole points of a KOTOR attack's `points` of damage type `row` its
/// target's feats take off: Improved Toughness (K1: points off physical
/// damage; TSL: a share of what is over a threshold) and Wookiee Toughness
/// (points; TSL's II and III points and a share of the rest), as
/// `rules::feats::toughness` gives them.
pub fn toughness(feats: &Feats, game: Game, row: Option<usize>, points: i64) -> i64 {
    let physical = row.is_some_and(|row| {
        [
            type_rows::BLUDGEONING,
            type_rows::PIERCING,
            type_rows::SLASHING,
        ]
        .contains(&(row as u8))
    });
    use rules::feats::toughness;
    let improved = match game {
        _ if !feats.has(feats::IMPROVED_TOUGHNESS) => 0,
        Game::K1 if physical => toughness::IMPROVED_K1,
        Game::K1 => 0,
        Game::Tsl => (points - toughness::IMPROVED_TSL_OVER).max(0) / toughness::IMPROVED_TSL_SHARE,
    };
    let [one, two, three] = toughness::WOOKIEE_FLAT;
    let [two_share, three_share] = toughness::WOOKIEE_SHARE;
    let wookiee = match feats.count(&feats::WOOKIEE_TOUGHNESS) {
        0 => 0,
        1 => one,
        2 => two + (points - two).max(0) / two_share,
        _ => three + (points - three).max(0) / three_share,
    };
    (improved + wookiee).min(points.max(0))
}
