//! `spells.2da` and `feat.2da` row numbers crossing between a script and
//! the sim (`rules::rows`).
//!
//! Inside the sim every row number is canonical. A script of the game or of
//! a mod speaks the install's row numbers, which a patcher compiled into
//! it. KOTOR 2's impact scripts lent to a KOTOR 1 install speak KOTOR 2's
//! numbers, which are the canonical ones (`Library::speaks_install`). When
//! a script that speaks install numbers calls a routine, each argument that
//! is a row is made canonical before the routine runs (`ARGUMENTS`), and a
//! row it returns is made an install row again (`RETURNS`).
//!
//! A talent holds a canonical number. `TalentSpell` and `TalentFeat` make
//! it from an install row, and `GetIdFromTalent` gives the install row back.
//! A number outside the install's table is kept as it is, so -1 and a
//! script's constants that stand for no row (`FORCE_POWER_ALL_FORCE_POWERS`)
//! pass unchanged.

use rules::rows::{RowMaps, Table};
use script_ncs::vm::Value;

use super::Library;
use super::actions::Use;

/// The routines whose arguments are rows: each routine, the place of the
/// argument, and the table it counts rows of.
pub(super) const ARGUMENTS: [(&str, usize, Table); 21] = [
    ("GetHasSpell", 0, Table::Spells),
    ("GetHasSpellEffect", 0, Table::Spells),
    ("GetSpellAcquired", 0, Table::Spells),
    ("GrantSpell", 0, Table::Spells),
    ("TalentSpell", 0, Table::Spells),
    ("EventSpellCastAt", 1, Table::Spells),
    ("EffectSpellImmunity", 0, Table::Spells),
    ("GetSpellBaseForcePointCost", 0, Table::Spells),
    ("GetSpellFormMask", 0, Table::Spells),
    ("ActionCastSpellAtObject", 0, Table::Spells),
    ("ActionCastSpellAtLocation", 0, Table::Spells),
    ("ActionCastFakeSpellAtObject", 0, Table::Spells),
    ("ActionCastFakeSpellAtLocation", 0, Table::Spells),
    // KOTOR 2's forms are `spells.2da` rows (`FORM_SABER_I_SHII_CHO` is
    // row 258).
    ("IsFormActive", 1, Table::Spells),
    ("SetCurrentForm", 1, Table::Spells),
    ("GetHasFeat", 0, Table::Feats),
    ("GetFeatAcquired", 0, Table::Feats),
    ("GetHasFeatEffect", 0, Table::Feats),
    ("GrantFeat", 0, Table::Feats),
    ("TalentFeat", 0, Table::Feats),
    ("ActionUseFeat", 0, Table::Feats),
];

/// The routines that return a row, with the table it counts rows of.
/// `GetIdFromTalent` returns a row of its talent's table (`value_out`).
pub(super) const RETURNS: [(&str, Table); 6] = [
    ("GetSpellId", Table::Spells),
    ("GetLastSpell", Table::Spells),
    ("GetEffectSpellId", Table::Spells),
    ("GetLastForcePowerUsed", Table::Spells),
    ("GetLastCombatFeatUsed", Table::Feats),
    ("GetMetaMagicFeat", Table::Feats),
];

/// `GetCreatureTalentBest`'s excluded type and excluded row: the row counts
/// rows of the table its type names.
const EXCLUDED: (&str, usize, usize) = ("GetCreatureTalentBest", 4, 5);

/// The table of the talent type `kind` (`TALENT_TYPE_FORCE` or
/// `TALENT_TYPE_FEAT`), none for another type.
fn talent_table(library: &Library, kind: i32) -> Option<Table> {
    [
        ("TALENT_TYPE_FORCE", Table::Spells),
        ("TALENT_TYPE_FEAT", Table::Feats),
    ]
    .into_iter()
    .find(|(name, _)| library.constant(name) == Some(kind))
    .map(|(_, table)| table)
}

fn translate(value: &mut Value, maps: &RowMaps, table: Table, to_canonical: bool) {
    if let Value::Int(number) = value {
        let map = maps.get(table);
        *number = match to_canonical {
            true => map.value_to_canonical(*number),
            false => map.value_to_install(*number),
        };
    }
}

/// `args` of routine `name`, called by a script that speaks install
/// numbers, with each row made canonical.
pub(super) fn args_in(library: &Library, name: &str, args: &mut [Value]) {
    let maps = library.rows();
    for &(routine, at, table) in &ARGUMENTS {
        if routine == name
            && let Some(value) = args.get_mut(at)
        {
            translate(value, maps, table, true);
        }
    }
    let (routine, kind_at, row_at) = EXCLUDED;
    if routine == name
        && let Some(Value::Int(kind)) = args.get(kind_at)
        && let Some(table) = talent_table(library, *kind)
        && let Some(value) = args.get_mut(row_at)
    {
        translate(value, maps, table, true);
    }
}

/// `value`, returned by routine `name` to a script that speaks install
/// numbers, with a row made an install row. `talent` is the talent
/// `GetIdFromTalent` was given.
pub(super) fn value_out(
    library: &Library,
    name: &str,
    talent: Option<Use>,
    mut value: Value,
) -> Value {
    let table = match name {
        "GetIdFromTalent" => match talent {
            Some(Use::Power(_)) => Some(Table::Spells),
            Some(Use::Feat(_)) => Some(Table::Feats),
            None => None,
        },
        _ => RETURNS
            .iter()
            .find(|(routine, _)| *routine == name)
            .map(|&(_, table)| table),
    };
    if let Some(table) = table {
        translate(&mut value, library.rows(), table, false);
    }
    value
}
