//! The classes as levels grow them (`sim::levels`), from `classes.2da` and
//! the tables it names: each class's attack, save and defence rows, its
//! feats (`feat.2da`'s `<class>_list`: 0 to 2 picked, 3 granted at
//! `<class>_granted`, 4 never), its powers (`spells.2da`'s column for the
//! class, the class level it may from), its skills (`skills.2da`), feats and
//! powers picked each level (`featgain`, `classpowergain`), and the XP each
//! level asks (`exptable.2da`). TSL's levels count a class's skill points
//! and the INT modifier whole, K1's half their sum
//! (`Classes::whole_skill_points`).

use asset_odyssey::module::Source;
use fmt_odyssey::twoda::TwoDa;
use sim::combat::sheet::Rows;
use sim::levels::{Class, Classes, Needs, SKILLS};

use crate::sheets::Tables;

/// `spells.2da`'s column for a Jedi class, by its feat code: the three
/// base classes, and TSL's six prestige classes (`CSWSpell::GetSpellLevel`
/// reads one for each).
const POWER_COLUMNS: [(&str, &str); 9] = [
    ("jgd", "guardian"),
    ("jcn", "consular"),
    ("jsn", "sentinel"),
    ("jwm", "weapmstr"),
    ("jma", "jedimaster"),
    ("jwa", "watchman"),
    ("sma", "marauder"),
    ("sld", "sithlord"),
    ("sas", "assassin"),
];

/// A class's level for a power in its `spells.2da` column: from that level
/// (0: any), none when -1 (`CSWSCreatureStats::CanChooseSpell` refuses only
/// the column's 0xff).
fn power_level(table: &TwoDa, power: usize, column: &str) -> Option<u8> {
    table
        .int(power, column)
        .and_then(|value| u8::try_from(value).ok())
        .filter(|level| *level != u8::MAX)
}

/// The 2DAs' abilities (`primaryabil`, `keyability`), strength to
/// charisma.
const ABILITIES: [&str; 6] = ["STR", "DEX", "CON", "INT", "WIS", "CHA"];

/// An ability's number (`combat::sheet::Ability`) by its 2DA name.
pub(crate) fn ability(name: &str) -> Option<u8> {
    ABILITIES
        .iter()
        .position(|of| name.eq_ignore_ascii_case(of))
        .map(|at| at as u8)
}

/// A by-level column of `name`, its rows in order while they read.
fn by_level<T: TryFrom<i64>>(tables: &mut Tables, name: &str, column: &str) -> Vec<T> {
    let Some(table) = tables.table(name) else {
        return Vec::new();
    };
    (0..table.row_labels.len())
        .map_while(|row| {
            table
                .int(row, column)
                .and_then(|value| T::try_from(value).ok())
        })
        .collect()
}

/// Rows of `table` with an order in `column`, in it (the rest after, by
/// row).
fn ordered(table: &TwoDa, rows: impl Iterator<Item = usize>, column: &str) -> Vec<usize> {
    let mut rows: Vec<(u32, usize)> = rows
        .map(|row| {
            (
                table
                    .int(row, column)
                    .and_then(|value| value.try_into().ok())
                    .unwrap_or(u32::MAX),
                row,
            )
        })
        .collect();
    rows.sort();
    rows.into_iter().map(|(_, row)| row).collect()
}

fn class(tables: &mut Tables, classes: &TwoDa, row: usize) -> Class {
    let code = |column| {
        classes
            .text(row, column)
            .unwrap_or_else(String::new)
            .to_ascii_lowercase()
    };
    let (feats, skills, gain) = (code("featstable"), code("skillstable"), code("featgain"));
    let mut class = Class {
        hit_die: classes
            .int(row, "hitdie")
            .and_then(|value| value.try_into().ok())
            .unwrap_or(0),
        force_die: classes
            .int(row, "forcedie")
            .and_then(|value| value.try_into().ok())
            .unwrap_or(0),
        skill_points: classes
            .int(row, "skillpointbase")
            .and_then(|value| value.try_into().ok())
            .unwrap_or(0),
        primary: classes
            .text(row, "primaryabil")
            .and_then(|name| ability(&name))
            .unwrap_or(0),
        ..Class::default()
    };
    if let Some(table) = classes.text(row, "attackbonustable") {
        class.bab = by_level(tables, &table, "bab");
    }
    if let Some(table) = classes.text(row, "savingthrowtable") {
        let [fort, reflex, will]: [Vec<i8>; 3] =
            ["fortsave", "refsave", "willsave"].map(|column| by_level(tables, &table, column));
        class.saves = (0..fort.len().min(reflex.len()).min(will.len()))
            .map(|at| [fort[at], reflex[at], will[at]])
            .collect();
    }
    if let Some(column) = classes.text(row, "armorclasscolumn") {
        class.defence = by_level(tables, "acbonus", &column.to_ascii_lowercase());
    }
    let reg: Vec<u8> = by_level(tables, "featgain", &format!("{gain}_reg"));
    let bonus: Vec<u8> = by_level(tables, "featgain", &format!("{gain}_bon"));
    class.feat_picks = reg.iter().zip(&bonus).map(|(a, b)| a + b).collect();
    class.power_picks = by_level(tables, "classpowergain", &feats);
    if let Some(table) = tables.table("feat") {
        // A sheet knows only so many feats and powers (`Rows`).
        let rows = 0..table.row_labels.len().min(usize::from(Rows::<4>::ROWS));
        for feat in rows.clone() {
            let granted: Option<u8> = table
                .int(feat, &format!("{feats}_granted"))
                .and_then(|value| value.try_into().ok());
            if let Some(level) = granted.filter(|level| *level > 0) {
                class.granted.push((level, feat as u16));
            }
            // TSL's creation grants a feat whose `_pc_granted` is 1
            // (`CSWClass::IsPCGrantedFeat`, read by the creation screens'
            // `CSWCLevelUpStats::AddGrantedFeats`).
            if table.int(feat, &format!("{feats}_pc_granted")) == Some(1) {
                class.pc_granted.push(feat as u16);
            }
        }
        let list = format!("{feats}_list");
        let picked = rows.filter(|&feat| {
            table
                .int(feat, &list)
                .and_then(|value| u8::try_from(value).ok())
                .is_some_and(|l| l <= 2)
        });
        class.feats = ordered(&table, picked, &format!("{feats}_recom"))
            .into_iter()
            .map(|feat| feat as u16)
            .collect();
    }
    let column = POWER_COLUMNS.iter().find(|(of, _)| *of == feats);
    if let (Some((_, column)), Some(table)) = (column, tables.table("spells")) {
        let rows = (0..table.row_labels.len().min(usize::from(Rows::<5>::ROWS))).filter(|&power| {
            table
                .int(power, "usertype")
                .and_then(|value| i32::try_from(value).ok())
                == Some(1)
                && power_level(&table, power, column).is_some()
        });
        class.powers = ordered(&table, rows, "light_recom")
            .into_iter()
            .filter_map(|power| Some((power_level(&table, power, column)?, power as u16)))
            .collect();
    }
    if let Some(table) = tables.table("skills") {
        for (skill, own) in class.class_skills.iter_mut().enumerate() {
            *own = table
                .int(skill, &format!("{skills}_class"))
                .and_then(|value| u8::try_from(value).ok())
                == Some(1);
        }
        let reco = format!("{skills}_reco");
        let rows = (0..SKILLS).filter(|&skill| {
            table
                .int(skill, &reco)
                .and_then(|value| u8::try_from(value).ok())
                .is_some_and(|n| n > 0)
        });
        class.skill_order = ordered(&table, rows, &reco)
            .into_iter()
            .map(|skill| skill as u8)
            .collect();
    }
    class
}

/// The install's classes; empty where `classes.2da` does not read.
pub fn classes(source: &dyn Source) -> Classes {
    let mut tables = Tables::new(source);
    let Some(table) = tables.table("classes") else {
        return Classes::default();
    };
    let mut classes = Classes {
        classes: (0..table.row_labels.len())
            .map(|row| class(&mut tables, &table, row))
            .collect(),
        levels: by_level(&mut tables, "exptable", "xp"),
        whole_skill_points: tables.tsl(),
        granted_powers: tables.tsl(),
        ..Classes::default()
    };
    if let Some(feat) = tables.table("feat") {
        for row in 0..feat.row_labels.len() {
            let needs = Needs {
                feats: ["prereqfeat1", "prereqfeat2"].map(|column| {
                    feat.int(row, column)
                        .and_then(|value| value.try_into().ok())
                }),
                level: feat
                    .int(row, "mincharlevel")
                    .and_then(|value| value.try_into().ok())
                    .unwrap_or(0),
                bab: feat
                    .int(row, "minattackbonus")
                    .and_then(|value| value.try_into().ok())
                    .unwrap_or(0),
                // Both games leave these blank.
                abilities: ["minstr", "mindex", "mincon", "minint", "minwis", "mincha"].map(
                    |column| {
                        feat.int(row, column)
                            .and_then(|value| value.try_into().ok())
                            .unwrap_or(0)
                    },
                ),
            };
            if needs != Needs::default() {
                classes.needs.insert(row as u16, needs);
            }
        }
    }
    if let Some(spells) = tables.table("spells") {
        for row in 0..spells.row_labels.len() {
            if let Some(need) = spells
                .int(row, "prerequisites")
                .and_then(|value| value.try_into().ok())
            {
                classes.prerequisites.insert(row as u16, need);
            }
        }
    }
    classes
}
