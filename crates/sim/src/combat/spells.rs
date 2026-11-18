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

/// The rulebook's rows of each kind of choice (`ai.choices.kinds`). The
/// kinds come in this order: disable, damage, area damage, heal and buff.
pub const ROWS_BY_KIND: [&[u16]; 5] = [
    &kinds::DISABLE,
    &kinds::DAMAGE,
    &kinds::AREA_DAMAGE,
    &kinds::HEAL,
    &kinds::BUFF,
];

/// The place in `ROWS_BY_KIND` of the kind that lists `row`, and whether the
/// row works only on droids. It is `None` for a row the rulebook does not
/// list.
pub fn kind_of_row(row: u16) -> Option<(usize, bool)> {
    let kind = ROWS_BY_KIND.iter().position(|rows| rows.contains(&row))?;
    Some((kind, kinds::DROIDS_ONLY.contains(&row)))
}

// Each row as a constant named by its label, and `NAMED`, every one by
// label. They are kept with the rows-by-label map (`rules::rows`), which
// gives a mod's table's rows these numbers.
pub use rules::rows::spells::*;
