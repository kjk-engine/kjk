//! What a character's sheet decides in a fight under the Advanced rules
//! (the spec's section 4, `data/rules/duel.toml` chapter `sheet`). The mode
//! is a melee system for every melee weapon, so these rules read the weapon
//! in hand and not only a lightsaber.
//!
//! - **Level** sets the most block points a character can hold (`cap`).
//! - **Constitution** sets how long they wait before block points come
//!   back (`refill_wait`).
//! - **The Reflex save** sets how long the perfect block window stays open
//!   (`window_ms`).
//! - **The attacker's attack bonus against the blocker's Defense** sets
//!   what a blocked swing costs the blocker (`block_chance`, which the
//!   exchange turns into points in `exchange::blocked_cost`). A character
//!   without the proficiency feat for the weapon they block with counts
//!   their Defense `sheet::NOT_PROFICIENT` points lower (`proficient`).
//!
//! The Dueling feats, the Two-Weapon Fighting feats and KOTOR 2's
//! lightsaber Forms have no rule of their own here: KOTOR already counts
//! them in the attack bonus and the Defense, and a second rule would count
//! them twice. The Jedi Defense feats decide nothing about blocking a
//! swing; they are the bolts' rule (`shots`).

use crate::combat::damage;
use crate::combat::forms;
use crate::combat::sheet::{Sheet, Weapon, feats};
use rules::duel::meters;
use rules::duel::sheet;

/// The most block points a character of `level` levels can hold:
/// `sheet::BASE` and `sheet::PER_LEVEL` for each level, never more than
/// `meters::POINTS_MAX`. A character with no level reads as level 0 and
/// holds `sheet::BASE`.
pub fn cap(level: u8) -> u8 {
    let most = u32::from(sheet::BASE) + u32::from(sheet::PER_LEVEL) * u32::from(level);
    most.min(u32::from(meters::POINTS_MAX)) as u8
}

/// Stamina a special move uses up: `percent` of the fighter's maximum
/// (`cap`), rounded down, and at least 1.
pub fn special_cost(percent: i32, cap: u8) -> i32 {
    (percent * i32::from(cap) / 100).max(1)
}

/// Whether a fighter with `points` Stamina may start a special move that
/// uses up `cost`: it is not tired, and it has at least `cost`.
pub fn may_start_special(points: i32, cost: i32) -> bool {
    !super::exchange::tired(points) && points >= cost
}

/// How long a character whose Constitution modifier is `con_modifier`
/// waits before block points come back, in thousandths of the usual wait:
/// `sheet::CON_REFILL` thousandths shorter for each point of the modifier,
/// and that much longer for each negative point, held between
/// `sheet::REFILL_LIMITS`.
pub fn refill_wait(con_modifier: i32) -> i64 {
    let [shortest, longest] = sheet::REFILL_LIMITS;
    (1000 - sheet::CON_REFILL * i64::from(con_modifier)).clamp(shortest, longest)
}

/// How long the perfect block window stays open for a character whose
/// Reflex save is `reflex`, where the difficulty's own window is `base_ms`:
/// `sheet::REFLEX_WINDOW_MS` longer for each point of the save, and that
/// much shorter for each negative point, held between
/// `sheet::WINDOW_LIMITS_MS`.
pub fn window_ms(base_ms: u64, reflex: i32) -> u64 {
    let [shortest, longest] = sheet::WINDOW_LIMITS_MS;
    let open = base_ms as i64 + sheet::REFLEX_WINDOW_MS * i64::from(reflex);
    open.clamp(shortest as i64, longest as i64) as u64
}

/// Whether `sheet` holds the proficiency feat for `weapon` (or the weapon
/// needs none, as fists do).
pub fn proficient(sheet: &Sheet, weapon: &Weapon) -> bool {
    weapon
        .proficiency
        .is_none_or(|feat| sheet.feats.has(feat) || sheet.feats.has(feats::PROFICIENCY_ALL))
}

/// KOTOR's chance, in thousandths, that `attacker`'s swing with `weapon`
/// hits `blocker`, who blocks it with `blocker_weapon` (`damage::odds`
/// through `damage::margin`, the blocker's sheet as it defends against one
/// attacker). The chance is what a blocked swing's cost is multiplied by
/// (`exchange::blocked_cost`), so a strong attacker takes more block points
/// from a blocker than a weak one does. A blocker without the proficiency
/// feat for the weapon in their hand counts their Defense
/// `sheet::NOT_PROFICIENT` points lower, which raises the chance and so
/// costs them more.
pub fn block_chance(
    attacker: &Sheet,
    weapon: &Weapon,
    blocker: &Sheet,
    blocker_weapon: &Weapon,
) -> i64 {
    let defending = forms::defending(blocker, true);
    let (attack, mut defence) = damage::odds(attacker, weapon, &defending);
    if !proficient(blocker, blocker_weapon) {
        defence -= sheet::NOT_PROFICIENT;
    }
    damage::margin(attack, defence)
}
