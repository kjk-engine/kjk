//! Passive feats (`feat.2da`): what a feat a character simply has does,
//! where its rule applies, by the rows K1 and TSL share. A line's tiers
//! replace each other and each needs the one before (`prereqfeat`), so how
//! many of a line's rows a sheet holds is its tier.
//!
//! The rules are the feats' own descriptions (the talk table) and
//! `baseitems.2da`'s; the class defence feats (Scoundrel's Luck, Jedi
//! Sense, the droids' Logic Upgrades) are `acbonus.2da`'s class columns,
//! which the sheet already adds.

use crate::combat::sheet::{Rows, Sheet, build, feats, type_rows};

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
/// A pistol in each hand: to each hand's.
const PAIR_AIM: i32 = rules::weapons::pistol::PAIR_AIM;
/// Wookiee Toughness's HP a level by its tier.
const WOOKIEE_HP: [i32; 4] = rules::feats::toughness::WOOKIEE_HP;
/// Force Sensitive's Force points.
const FORCE_SENSITIVE_FP: u16 = rules::feats::force_sensitive::POINTS;

fn tier(feats: &Rows<4>, line: &[u16]) -> i32 {
    i32::from(feats.count(line))
}

/// Conditioning, Improved, Master: +1, +2, +3 to every save.
pub fn save_bonus(feats: &Rows<4>) -> i32 {
    tier(feats, &feats::CONDITIONING)
}

/// Uncanny Dodge 1 and 2: `rules::feats::saves::UNCANNY_DODGE` a tier to
/// saves against grenades.
pub fn grenade_save_bonus(feats: &Rows<4>) -> i32 {
    rules::feats::saves::UNCANNY_DODGE * tier(feats, &feats::UNCANNY_DODGE)
}

/// What feats add to `skill` (`skills` row) for one with `ranks` in it:
/// each line's tier by its own step (`feats::SKILL_LINES`), Skill Focus's
/// `rules::feats::skills::SKILL_FOCUS`; nothing without a rank of its own
/// (TSL's descriptions).
pub fn skill_bonus(feats: &Rows<4>, skill: usize, ranks: u8) -> i32 {
    if ranks == 0 {
        return 0;
    }
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
/// fights with the right's) for how the hands are filled: with a weapon in
/// each, or a double-bladed one, two-weapon fighting's penalty for the
/// hand at Two-Weapon Fighting's tier, a balanced weapon in the left
/// easing the right's, a pistol pair's aim easing both; with one one-handed
/// weapon alone, Dueling's bonus.
pub fn hand_attack(sheet: &Sheet, hand: u8) -> i32 {
    let tier = tier(&sheet.feats, &feats::TWO_WEAPON_FIGHTING).min(3) as usize;
    let left = hand == 1 && sheet.weapons[1].is_some();
    let pair = if sheet.pistol_pair() { PAIR_AIM } else { 0 };
    match sheet.weapons {
        [Some(_), Some(_)] if left => OFF_HAND[tier] + pair,
        [Some(_), Some(off)] => {
            let balanced = if off.build & build::SMALL != 0 {
                BALANCED
            } else {
                0
            };
            MAIN_HAND[tier] + balanced + pair
        }
        [Some(main), None] if main.build & build::DOUBLE != 0 => MAIN_HAND[tier],
        _ => dueling(sheet),
    }
}

/// Whether one with `feats` may equip an item needing `needs`
/// (`baseitems.2da` `reqfeat0` to `reqfeat4`): every one held, a
/// proficiency also by Proficiency All.
pub fn may_equip(feats: &Rows<4>, needs: &[u16]) -> bool {
    let all = feats.has(feats::PROFICIENCY_ALL);
    needs
        .iter()
        .all(|need| feats.has(*need) || (all && feats::PROFICIENCIES.contains(need)))
}

/// HP every level gives beside its hit die and CON: Toughness's and
/// Master Toughness's 1 each, Wookiee Toughness's by its tier (`WOOKIEE_HP`).
pub fn hp_per_level(feats: &Rows<4>) -> i32 {
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

/// Whole points of a KOTOR attack's `points` of damage type `row` its
/// target's feats take off: Improved Toughness (K1: points off physical
/// damage; TSL: a share of what is over a threshold) and Wookiee Toughness
/// (points; TSL's II and III points and a share of the rest), as
/// `rules::feats::toughness` gives them.
pub fn toughness(feats: &Rows<4>, game: Game, row: Option<usize>, points: i64) -> i64 {
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
