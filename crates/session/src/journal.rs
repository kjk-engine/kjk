//! The journal's quests from the install's `global.jrl`
//! (`sim::script::journal`): each category's tag, name, priority, planet,
//! plot row and entries; and the XP tables (`sim::script::xp`): `plot.2da`
//! and `xptable.2da`.

use std::collections::BTreeMap;

use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffValue};
use sim::script::journal::{Quest, QuestEntry, Quests};
use sim::script::xp::Experience;

use crate::sheets::Tables;

/// A talk-table line a field names, `u32::MAX` for none.
fn strref(value: Option<&GffValue>) -> u32 {
    match value {
        Some(GffValue::LocString(text)) => text.strref,
        _ => u32::MAX,
    }
}

/// The install's quests; none when it has no journal that reads.
pub(crate) fn quests(source: &dyn Source) -> Quests {
    let Some(jrl) = source
        .read("global", ResType::JRL)
        .and_then(|bytes| Gff::parse(&bytes).ok())
    else {
        return Quests::default();
    };
    let mut quests = BTreeMap::new();
    for category in jrl.root.list("Categories") {
        let Some(tag) = category.resref("Tag") else {
            continue;
        };
        let entries = category
            .list("EntryList")
            .iter()
            .filter_map(|entry| {
                Some(QuestEntry {
                    id: u32::try_from(entry.int("ID")?).ok()?,
                    end: entry.int("End").unwrap_or(0) != 0,
                    text: strref(entry.get("Text")),
                    xp: entry.float("XP_Percentage").unwrap_or(0.0),
                })
            })
            .collect();
        let field = |label: &str, none: i64| category.int(label).unwrap_or(none);
        let quest = Quest {
            name: strref(category.get("Name")),
            priority: field("Priority", 0).clamp(0, i64::from(u32::MAX)) as u32,
            planet: field("PlanetID", -1) as i32,
            plot: field("PlotIndex", -1) as i32,
            entries,
        };
        quests.insert(tag, quest);
    }
    Quests(quests)
}

/// The install's XP tables; empty where one does not read.
pub(crate) fn experience(source: &dyn Source) -> Experience {
    let mut tables = Tables::new(source);
    let mut experience = Experience::default();
    if let Some(plot) = tables.table("plot") {
        experience.plots = (0..plot.row_labels.len())
            .map(|row| {
                let label = plot.text(row, "label").unwrap_or_else(String::new);
                (
                    label.to_ascii_lowercase(),
                    plot.int(row, "xp")
                        .and_then(|value| value.try_into().ok())
                        .unwrap_or(0),
                )
            })
            .collect();
    }
    if let Some(kills) = tables.table("xptable") {
        // K1 heads its challenge ratings `c0`, `c1`, …; TSL `0`, `1`, … (its
        // `CSWSCreature::CalculateDeathExperience` reads the column after
        // `level` by the rating's number).
        let named = |prefix: &str| -> Vec<String> {
            (0..)
                .map(|rating| format!("{prefix}{rating}"))
                .take_while(|column| kills.column(column).is_some())
                .collect()
        };
        let ratings = Some(named("c"))
            .filter(|columns| !columns.is_empty())
            .unwrap_or_else(|| named(""));
        experience.kills = (0..kills.row_labels.len())
            .map(|row| {
                let worth = |column: &String| {
                    kills
                        .int(row, column)
                        .and_then(|value| value.try_into().ok())
                        .unwrap_or(0)
                };
                ratings.iter().map(worth).collect()
            })
            .collect();
    }
    experience
}

/// The kills' table read from either game's heading (`experience`): K1's
/// `c0`, `c1`, … and TSL's `0`, `1`, …, each a death's XP by the party's
/// level and the challenge rating. TSL's had read as no ratings at all, so
/// no death there gave XP (the chain's Exile came to Korriban two levels
/// short of the secret tomb's gate). No install.
pub fn probes() -> Vec<(&'static str, Result<String, String>)> {
    use asset_odyssey::made::Files;
    use fmt_odyssey::twoda::TwoDa;
    let read = |prefix: &str| {
        let table = TwoDa {
            columns: ["level", &format!("{prefix}0"), &format!("{prefix}1")]
                .map(|name| name.as_bytes().to_vec())
                .to_vec(),
            row_labels: vec![b"0".to_vec(), b"1".to_vec()],
            cells: [["1", "100", "125"], ["2", "75", "100"]]
                .map(|row| row.map(|cell| cell.as_bytes().to_vec()).to_vec())
                .to_vec(),
            column_separator: b'\t',
            row_separator: b'\t',
        };
        let files = Files([("xptable.2da".to_string(), table.write())].into());
        experience(&files).kills
    };
    let wanted = vec![vec![100, 125], vec![75, 100]];
    [("xp_kills_k1_heading", "c"), ("xp_kills_tsl_heading", "")]
        .into_iter()
        .map(|(name, prefix)| {
            let kills = read(prefix);
            let what = format!("{kills:?}");
            (name, if kills == wanted { Ok(what) } else { Err(what) })
        })
        .collect()
}
