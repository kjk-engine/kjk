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
        let ratings: Vec<String> = (0..)
            .map(|rating| format!("c{rating}"))
            .take_while(|column| kills.column(column).is_some())
            .collect();
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
