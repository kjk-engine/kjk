//! TSL's Forms (`context/artifacts/i35/combat-force.md` section 3, from its
//! executable): the lightsaber Forms and the Force Forms a Jedi fights in.
//! A Form is a `spells.2da` row (258 to 268, `formmask` 0x40): known as any
//! power is known (`Sheet::powers`), in use as the sheet's `form`
//! (`CurrentForm`), chosen from the HUD's stance column with no action and
//! no cost (`Action::Form`), set by scripts (`SetCurrentForm`) and granted at
//! a Jedi's levels 11 to 14 (`levels::granted_forms`) and by the Masters
//! (`GrantSpell`).
//!
//! A lightsaber Form counts only while a lightsaber is in the right hand
//! (`saber`, the executable's `IsWieldingLightsaber`); a Force Form always.
//! What each does where our fight has the counterpart of the executable's
//! rule: to hit (`to_hit`), defence against the attacker that is its own
//! target or another (`defence`), the weapon's damage, critical multiplier
//! and threat range (`weapon`), the critical's confirmation (`confirm`), an
//! extra attack a round (`extra_attacks`, as Speed's), bolt deflection
//! (`deflection`), Force point regeneration (`fp_base`, `fp_bonus`) and
//! the powers' cost (`cost`), by the numbers of the book's chapter `forms`.
//! The Force powers' damage, duration and saves under a Form are the
//! powers' own impact script's (`k_sp1_generic` asks `IsFormActive`).

use crate::combat::sheet::{Delivery, Sheet, Weapon};
use rules::forms::affinity;
use rules::forms::ataru;
use rules::forms::juyo;
use rules::forms::makashi;
use rules::forms::niman;
use rules::forms::shien;
use rules::forms::shii_cho;
use rules::forms::soresu;

pub const SHII_CHO: u16 = 258;
pub const MAKASHI: u16 = 259;
pub const SORESU: u16 = 260;
pub const ATARU: u16 = 261;
pub const SHIEN: u16 = 262;
pub const NIMAN: u16 = 263;
pub const JUYO: u16 = 264;
/// Force Focus (Force Channel in the talk table's later lines).
pub const FOCUS: u16 = 265;
pub const POTENCY: u16 = 266;
pub const AFFINITY: u16 = 267;
pub const MASTERY: u16 = 268;

/// The lightsaber Forms, and every Form.
pub const SABER_FORMS: std::ops::RangeInclusive<u16> = SHII_CHO..=JUYO;
pub const ALL: std::ops::RangeInclusive<u16> = SHII_CHO..=MASTERY;

/// `spells.2da`'s `formmask` of a Form's row (`GetSpellFormMask`).
pub const FORM_MASK: u8 = 0x40;

/// The talk table's line telling `form` initiated as the player picks it
/// (TSL's 124098 for Shii-Cho to 124108 for Force Mastery, "{Shii-Cho}
/// Determination Initiated" and what it is for).
pub fn initiated(form: u16) -> u32 {
    124_098 + u32::from(form.saturating_sub(SHII_CHO))
}

/// Whether `sheet` holds a lightsaber in its right hand.
pub fn saber(sheet: &Sheet) -> bool {
    matches!(
        sheet.weapons[0].map(|weapon| weapon.delivery),
        Some(Delivery::Saber(_))
    )
}

/// Whether `form` is `sheet`'s in force (`IsFormActive`): its Form in use,
/// a lightsaber Form only with a lightsaber in the right hand.
pub fn is_active(sheet: &Sheet, form: u16) -> bool {
    sheet.form == form && form != 0 && (!SABER_FORMS.contains(&form) || saber(sheet))
}

/// `sheet`'s Form in force, 0 for none.
pub fn active(sheet: &Sheet) -> u16 {
    if is_active(sheet, sheet.form) {
        sheet.form
    } else {
        0
    }
}

/// What `attacker`'s Form adds to its attack on `target`: Shii-Cho's,
/// Makashi's against a lightsaber, Shien's, Niman's.
pub fn to_hit(attacker: &Sheet, target: &Sheet) -> i32 {
    match active(attacker) {
        SHII_CHO => shii_cho::TO_HIT,
        NIMAN => niman::TO_HIT,
        MAKASHI if saber(target) => makashi::TO_HIT_SABER,
        SHIEN => shien::TO_HIT,
        _ => 0,
    }
}

/// What `defender`'s Form adds to its defence against an attacker that is
/// its own target (`targeted`) or another: Shii-Cho's against others,
/// Soresu's against its target, Ataru's against its target and against
/// others, Shien's against its target, Niman's against anyone, Juyo's
/// against its target and against others.
pub fn defence(defender: &Sheet, targeted: bool) -> i32 {
    match (active(defender), targeted) {
        (SHII_CHO, false) => shii_cho::DEFENCE_OTHERS,
        (SORESU, true) => soresu::DEFENCE_TARGET,
        (ATARU, true) => ataru::DEFENCE_TARGET,
        (ATARU, false) => ataru::DEFENCE_OTHERS,
        (SHIEN, true) => shien::DEFENCE_TARGET,
        (NIMAN, _) => niman::DEFENCE,
        (JUYO, true) => juyo::DEFENCE_TARGET,
        (JUYO, false) => juyo::DEFENCE_OTHERS,
        _ => 0,
    }
}

/// `defender`'s sheet as it defends against one attacker (`defence`).
pub fn defending(defender: &Sheet, targeted: bool) -> Sheet {
    let mut sheet = *defender;
    let by = defence(defender, targeted);
    sheet.defence = (i32::from(sheet.defence) + by).clamp(-128, 127) as i8;
    sheet
}

/// `weapon` in the hands of `sheet` as its Form changes it: Makashi's
/// damage, Shien's critical multiplier, Soresu's threat range narrower and
/// Ataru's wider (never past a natural 20 alone, nor every roll).
pub fn weapon(sheet: &Sheet, mut weapon: Weapon) -> Weapon {
    match active(sheet) {
        MAKASHI => weapon.damage_bonus = weapon.damage_bonus.saturating_add(makashi::DAMAGE),
        SHIEN => weapon.crit_mult = weapon.crit_mult.saturating_add(shien::CRIT_MULT),
        SORESU if weapon.crit_threat > 1 => {
            weapon.crit_threat = weapon.crit_threat.saturating_sub(soresu::THREAT).max(1);
        }
        ATARU if weapon.crit_threat < 20 => {
            weapon.crit_threat = weapon.crit_threat.saturating_add(ataru::THREAT).min(20);
        }
        _ => {}
    }
    weapon
}

/// What `sheet`'s Form adds to a critical's confirmation roll: Juyo's.
pub fn confirm(sheet: &Sheet) -> i32 {
    if active(sheet) == JUYO {
        juyo::CONFIRM
    } else {
        0
    }
}

/// The attacks a round `sheet`'s Form adds: Juyo's (on-hand).
pub fn extra_attacks(sheet: &Sheet) -> i32 {
    if active(sheet) == JUYO {
        juyo::EXTRA_ATTACKS
    } else {
        0
    }
}

/// What `sheet`'s Form adds to a lightsaber's bolt deflection: Makashi's,
/// Soresu's, Ataru's, Shien's, Niman's.
pub fn deflection(sheet: &Sheet) -> i8 {
    match active(sheet) {
        MAKASHI => makashi::DEFLECTION,
        SORESU => soresu::DEFLECTION,
        ATARU => ataru::DEFLECTION,
        SHIEN => shien::DEFLECTION,
        NIMAN => niman::DEFLECTION,
        _ => 0,
    }
}

/// A Force power's cost under `sheet`'s Form (whole FP): Force Potency and
/// Force Mastery multiply it (`rules::forms::costs::MULTIPLIER`), rounded
/// down.
pub fn cost(sheet: &Sheet, cost: i32) -> i32 {
    match active(sheet) {
        POTENCY | MASTERY => (cost as f32 * rules::forms::costs::MULTIPLIER) as i32,
        _ => cost,
    }
}

/// Force point regeneration's base rate in a real fight under `sheet`'s
/// Form, from `regeneration.2da`'s out-of-combat and in-combat bases: Niman
/// and Force Affinity a share of the out-of-combat base, else the in-combat
/// base.
pub fn fp_base(sheet: &Sheet, out_of_combat: f32, in_combat: f32) -> f32 {
    match active(sheet) {
        NIMAN => out_of_combat * niman::FP_BASE,
        AFFINITY => out_of_combat * affinity::FP_BASE,
        _ => in_combat,
    }
}

/// What `sheet`'s Form adds to the regeneration rate in or out of a
/// fight: Force Focus's.
pub fn fp_bonus(sheet: &Sheet) -> f32 {
    if active(sheet) == FOCUS {
        rules::forms::focus::FP_BONUS
    } else {
        0.0
    }
}
