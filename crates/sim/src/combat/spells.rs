//! The `spells.2da` rows the native rules name, each by its row's `label`
//! (the nwscript constant of the same name; K1's 132 rows are TSL's first
//! 132, label for label). A row is the spell an item's Cast Spell
//! property, a power or a talent names; the games' own impact scripts
//! (`impactscript`: `k_sp1_generic`, `k_sup_healing`, `k_sup_bands`,
//! `k_sup_comshots`, `k_sup_grenade`) decide what one does wherever they
//! run, and the tables keyed by these names (`combat::items::use_of`,
//! `combat::grenades::effect`, `combat::force::of_row`, `force::crowd`) only
//! give a row JA's shape and stand in where no script can run.
//!
//! `kjk combat items` checks every name here against both games'
//! `spells.2da` labels (`NAMED`).
//!
//! These numbers are canonical numbers (`rules::rows`). A mod's table may
//! have a named row at another row. The resolver then serves the table in
//! canonical order (`vfs::rows`), so the row is found here all the same.

use rules::ai::choices::kinds;

/// K1's Force powers (`usertype` 1 that a sheet can know), by row: every
/// row `combat::force::of_row` maps, and the saber throws.
pub const K1_FORCE_POWERS: [u16; 44] = [
    4, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 22, 23, 24, 25, 26, 27, 28, 29, 30,
    31, 32, 33, 34, 35, 36, 37, 38, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
];

/// How many rows KOTOR 1's own `spells.2da` has: KOTOR 2's first row past
/// them is its heat shield. KOTOR 2's powers lent to KOTOR 1 (`vfs::lent`)
/// come after them, by their KOTOR 2 numbers.
pub const K1_SPELL_ROWS: u16 = K1_ROWS;

/// Revitalize's rows: it raises the fallen members of the caster's party,
/// and does nothing where none has fallen.
pub const RAISES_THE_FALLEN: [u16; 3] = [
    FORCE_POWER_REVITALIZE,
    FORCE_POWER_IMPROVED_REVITALIZE,
    FORCE_POWER_MASTER_REVITALIZE,
];

/// What a Force power, grenade or item does in a fight, as the combat AI
/// chooses it (`ai.choices.kinds`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// It stuns, holds, knocks down, slows or weakens its target.
    Disable,
    /// It hurts one target.
    Damage,
    /// It hurts every enemy in an area.
    AreaDamage,
    /// It restores hit points.
    Heal,
    /// It strengthens the user or the user's allies.
    Buff,
}

/// The rulebook's rows of each kind (`ai.choices.kinds`).
pub const ROWS_BY_KIND: [(Kind, &[u16]); 5] = [
    (Kind::Disable, &kinds::DISABLE),
    (Kind::Damage, &kinds::DAMAGE),
    (Kind::AreaDamage, &kinds::AREA_DAMAGE),
    (Kind::Heal, &kinds::HEAL),
    (Kind::Buff, &kinds::BUFF),
];

/// The kind of `row`, and whether the row works only on droids. It is
/// `None` for a row the rulebook does not list.
pub fn kind_of_row(row: u16) -> Option<(Kind, bool)> {
    let (kind, _) = ROWS_BY_KIND.iter().find(|(_, rows)| rows.contains(&row))?;
    Some((*kind, kinds::DROIDS_ONLY.contains(&row)))
}

/// Whether `row` is one of a droid's own weapons, such as a stun ray, a
/// flame thrower or an ion blast. A droid weapon's label starts with
/// `DROID_ITEM_` (`NAMED`). A droid's shield has such a label too, but it is
/// a shield (`items::use_of`) and not a weapon.
pub fn droid_weapon(row: u16) -> bool {
    use crate::combat::items::{Use, use_of};
    let droid_item = NAMED
        .iter()
        .any(|&(label, named)| named == row && label.starts_with("DROID_ITEM_"));
    droid_item && !matches!(use_of(row), Some(Use::Shield { .. }))
}

// Each row as a constant named by its label, and `NAMED`, every one by
// label. They are kept with the rows-by-label map (`rules::rows`), which
// gives a mod's table's rows these numbers.
pub use rules::rows::spells::*;
