//! The journal's quests from the install's `global.jrl`
//! (`sim::script::journal`): each category's tag, name, priority, planet,
//! plot row and entries; and the XP tables (`sim::script::xp`): `plot.2da`
//! and `xptable.2da`.

use std::collections::BTreeMap;

use asset_odyssey::module::Source;
use fmt_odyssey::ResType;
use fmt_odyssey::gff::{Gff, GffValue};
use sim::combat::balance::Balance;
use sim::script::journal::{JournalText, Quest, QuestEntry, Quests};
use sim::script::xp::Experience;

use crate::sheets::Tables;

/// A field's text: the talk-table line it names (`u32::MAX` for none) and
/// its own first string, which a mod's quests carry in place of a line.
fn journal_text(value: Option<&GffValue>) -> JournalText {
    match value {
        Some(GffValue::LocString(text)) => JournalText {
            strref: text.strref,
            own: (text.strings.first()).map_or_else(Vec::new, |(_, words)| words.clone()),
        },
        _ => JournalText {
            strref: u32::MAX,
            own: Vec::new(),
        },
    }
}

/// The install's quests; none when it has no journal that reads. The
/// quests of `game`'s mods for kjk are added to them
/// (`level_edit::active::merged_journal`): each mod brings its own quests
/// in a file of its own, since the games' journal is one file.
pub fn quests(source: &dyn Source, game: &str) -> Quests {
    let jrl = source
        .read("global", ResType::JRL)
        .and_then(|bytes| Gff::parse(&bytes).ok());
    let jrl = match level_edit::active::merged_journal(game, jrl.clone()) {
        Some(merged) => Some(merged),
        None => jrl,
    };
    let Some(jrl) = jrl else {
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
                    text: journal_text(entry.get("Text")),
                    xp: entry.float("XP_Percentage").unwrap_or(0.0),
                })
            })
            .collect();
        let field = |label: &str, none: i64| category.int(label).unwrap_or(none);
        let quest = Quest {
            name: journal_text(category.get("Name")),
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
    // TSL's: `autobalance.2da`'s `CRMod` by row, `npc.2da`'s `PercentXP` of
    // rows 13 and 14, as `CalculateDeathExperience` and
    // `ApplyDeathExperience` read them by index.
    if let Some(balance) = tables.table("autobalance") {
        experience.balance_cr = (0..balance.row_labels.len())
            .map(|row| {
                balance
                    .int(row, "crmod")
                    .and_then(|value| value.try_into().ok())
                    .unwrap_or(0)
            })
            .collect();
        // Its multipliers (`sim::combat::balance`): a cell that does not
        // read scales nothing.
        experience.balance = (0..balance.row_labels.len())
            .map(|row| {
                let column = |name| balance.float(row, name).unwrap_or(0.0);
                Balance {
                    vitality: column("vpmult"),
                    to_hit: column("tohitmult"),
                    armor: column("armormult"),
                    damage: column("damagemult"),
                    save: column("savemult"),
                    level: column("levelmult"),
                }
            })
            .collect();
    }
    if let Some(npc) = tables.table("npc").filter(|npc| npc.row_labels.len() > 14) {
        let percent = |row| {
            npc.int(row, "percentxp")
                .and_then(|value| value.try_into().ok())
                .unwrap_or(0)
        };
        experience.percent = Some((percent(13), percent(14)));
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
    let mut checks: Vec<(&'static str, Result<String, String>)> =
        [("xp_kills_k1_heading", "c"), ("xp_kills_tsl_heading", "")]
            .into_iter()
            .map(|(name, prefix)| {
                let kills = read(prefix);
                let what = format!("{kills:?}");
                (name, if kills == wanted { Ok(what) } else { Err(what) })
            })
            .collect();
    checks.push(("xp_death_tsl", tsl_death()));
    checks.push(("journal_bad_field", journal_bad_field()));
    checks.push(("journal_own_words", journal_own_words()));
    checks.push(("globals_first_row_wins", globals_first_row_wins()));
    checks
}

/// KOTOR 2's `globalcat.2da` read as its executable reads it
/// (`sheets::globals`). The made-up table declares `M478_Console_02` as a
/// number and then again as a boolean in another case, so the number
/// stays. A name of 22 letters is left out and one of 21 letters is kept.
/// The sixth string is left out, because the catalogue holds 5 strings.
fn globals_first_row_wins() -> Result<String, String> {
    use asset_odyssey::made::Files;
    use fmt_odyssey::twoda::TwoDa;
    let mut rows = vec![
        ("M478_Console_02", "Number"),
        ("m478_console_02", "Boolean"),
        ("a_name_of_22_letters_x", "Number"),
        ("a_name_of_21_letters", "Boolean"),
    ];
    let strings: Vec<String> = (1..=6).map(|n| format!("text_{n}")).collect();
    rows.extend(strings.iter().map(|name| (name.as_str(), "String")));
    let table = |columns: &[&str], cells: Vec<Vec<&str>>| {
        let bytes = |text: &str| text.as_bytes().to_vec();
        TwoDa {
            columns: columns.iter().map(|name| bytes(name)).collect(),
            row_labels: (0..cells.len())
                .map(|row| bytes(&row.to_string()))
                .collect(),
            cells: cells
                .into_iter()
                .map(|row| row.into_iter().map(bytes).collect())
                .collect(),
            column_separator: b'\t',
            row_separator: b'\t',
        }
        .write()
    };
    let catalogue = table(
        &["name", "type"],
        rows.iter().map(|(name, kind)| vec![*name, *kind]).collect(),
    );
    // Only KOTOR 2's data has `itemcreate.2da` (`Tables::tsl`).
    let tsl_marker = table(&["label"], vec![vec!["recipe"]]);
    let files = Files(
        [
            ("globalcat.2da".to_string(), catalogue),
            ("itemcreate.2da".to_string(), tsl_marker),
        ]
        .into(),
    );
    let seen = crate::sheets::globals(&mut Tables::new(&files));
    let seen: Vec<String> = seen
        .iter()
        .map(|(name, kind)| format!("{name} {kind:?}"))
        .collect();
    let wanted = [
        "a_name_of_21_letters Boolean",
        "m478_console_02 Number",
        "text_1 String",
        "text_2 String",
        "text_3 String",
        "text_4 String",
        "text_5 String",
    ];
    let what = format!("{seen:?}");
    if seen == wanted {
        Ok(what)
    } else {
        Err(format!("{what}, wanted {wanted:?}"))
    }
}

/// A `global.jrl` with one quest name whose stored size is one byte off
/// still gives every quest and entry. That name reads as none, as the
/// games read a field that does not fit. A mod's journal file like that had
/// read as no journal at all, so the journal screen showed no quest.
fn journal_bad_field() -> Result<String, String> {
    use asset_odyssey::made::Files;
    use fmt_odyssey::gff::{Gff, GffField, GffStruct, LocString};
    const MARK: &[u8] = b"cut short";
    let field = |label: &str, value| GffField {
        label: label.as_bytes().to_vec(),
        value,
    };
    let name = |strref, strings| GffValue::LocString(LocString { strref, strings });
    let quest = |tag: &str, title: GffValue, text: u32| GffStruct {
        id: 0,
        fields: vec![
            field("Tag", GffValue::String(tag.as_bytes().to_vec())),
            field("Name", title),
            field(
                "EntryList",
                GffValue::List(vec![GffStruct {
                    id: 0,
                    fields: vec![
                        field("ID", GffValue::Dword(10)),
                        field("Text", name(text, Vec::new())),
                    ],
                }]),
            ),
        ],
    };
    let jrl = Gff {
        file_type: *b"JRL ",
        root: GffStruct {
            id: u32::MAX,
            fields: vec![field(
                "Categories",
                GffValue::List(vec![
                    quest("Tel_Czerka", name(5, Vec::new()), 7),
                    quest("tel_bad", name(6, vec![(0, MARK.to_vec())]), 8),
                ]),
            )],
        },
    };
    let mut bytes = jrl.write();
    // The size sits before the strref, the count, the language and the length.
    let at = bytes
        .windows(MARK.len())
        .position(|window| window == MARK)
        .ok_or("the marked name was not written")?
        - 20;
    let size = u32::from_le_bytes(bytes[at..at + 4].try_into().expect("4 bytes"));
    bytes[at..at + 4].copy_from_slice(&(size + 1).to_le_bytes());
    let quests = quests(&Files([("global.jrl".to_string(), bytes)].into()), "k1");
    // A quest's tag, its name and its entries' IDs and texts.
    type Seen = (String, u32, Vec<(u32, u32)>);
    let seen: Vec<Seen> = quests
        .0
        .iter()
        .map(|(tag, quest)| {
            let entries = (quest.entries.iter()).map(|entry| (entry.id, entry.text.strref));
            (tag.clone(), quest.name.strref, entries.collect())
        })
        .collect();
    let wanted = vec![
        ("tel_bad".to_string(), u32::MAX, vec![(10, 8)]),
        ("tel_czerka".to_string(), 5, vec![(10, 7)]),
    ];
    let what = format!("{seen:?}");
    if seen == wanted {
        Ok(what)
    } else {
        Err(format!("{what}, wanted {wanted:?}"))
    }
}

/// A `global.jrl` whose quest keeps its name and its entry's text in the
/// fields themselves, naming no talk-table line, as M4-78EP's quests do
/// (#525). The journal shows those words. A quest that names a line shows
/// the line's words. These quests had shown no name and no text, so a
/// player saw no journal entry change on M4-78.
fn journal_own_words() -> Result<String, String> {
    use asset_odyssey::made::Files;
    use fmt_odyssey::gff::{Gff, GffField, GffStruct, LocString};
    let field = |label: &str, value| GffField {
        label: label.as_bytes().to_vec(),
        value,
    };
    let text = |strref, words: &str| {
        let strings = if words.is_empty() {
            Vec::new()
        } else {
            vec![(0, words.as_bytes().to_vec())]
        };
        GffValue::LocString(LocString { strref, strings })
    };
    let quest = |tag: &str, name: GffValue, entry: GffValue| GffStruct {
        id: 0,
        fields: vec![
            field("Tag", GffValue::String(tag.as_bytes().to_vec())),
            field("Name", name),
            field(
                "EntryList",
                GffValue::List(vec![GffStruct {
                    id: 0,
                    fields: vec![field("ID", GffValue::Dword(30)), field("Text", entry)],
                }]),
            ),
        ],
    };
    let jrl = Gff {
        file_type: *b"JRL ",
        root: GffStruct {
            id: u32::MAX,
            fields: vec![field(
                "Categories",
                GffValue::List(vec![
                    quest(
                        "radiation",
                        text(u32::MAX, "Radiation Leak"),
                        text(u32::MAX, "Go speak to IS-02."),
                    ),
                    quest("tel_czerka", text(5, ""), text(7, "")),
                ]),
            )],
        },
    };
    let files = Files([("global.jrl".to_string(), jrl.write())].into());
    let quests = quests(&files, "k1");
    let talk_table = |strref: u32| Some(format!("line {strref}").into_bytes());
    let seen: Vec<(String, String, String)> = quests
        .0
        .iter()
        .map(|(tag, quest)| {
            let words =
                |text: &JournalText| String::from_utf8_lossy(&text.words(talk_table)).into_owned();
            let entry = quest
                .entry(30)
                .map_or_else(String::new, |entry| words(&entry.text));
            (tag.clone(), words(&quest.name), entry)
        })
        .collect();
    let wanted = [
        ("radiation", "Radiation Leak", "Go speak to IS-02."),
        ("tel_czerka", "line 5", "line 7"),
    ]
    .map(|(tag, name, entry)| (tag.to_string(), name.to_string(), entry.to_string()));
    let what = format!("{seen:?}");
    if seen == wanted {
        Ok(what)
    } else {
        Err(format!("{what}, wanted {wanted:?}"))
    }
}

/// TSL's death XP (`Experience::tsl_kill`, the executable's
/// `CalculateDeathExperience` and `ApplyDeathExperience`): the row of the
/// level the XP reaches, the rating capped at 30 (column 31 and on never
/// read), an autobalanced creature's `CRMod` past the PC's level (at least
/// 0), `GAME_XP_General`'s percent, rounded up; `PER_NPC_Bonus` a share
/// by each member.
fn tsl_death() -> Result<String, String> {
    let kills: Vec<Vec<i32>> = (0..32)
        .map(|row| (0..40).map(|rating| row * 100 + rating).collect())
        .collect();
    let experience = Experience {
        kills,
        balance_cr: vec![0, -3, -2],
        percent: Some((75, 0)),
        ..Experience::default()
    };
    let seen = [
        experience.tsl_kill(3, 5.9, None, 3),
        experience.tsl_kill(40, 35.0, None, 3),
        experience.tsl_kill(10, 9.0, Some((1, 12)), 3),
        experience.tsl_kill(10, 9.0, Some((2, 1)), 3),
        Experience {
            percent: Some((100, 10)),
            ..experience.clone()
        }
        .tsl_kill(2, 1.0, None, 3),
    ];
    // Row 2, rating 5: 205 x 0.75 = 153.75, up to 154; row 30, rating 30:
    // 3030 x 0.75 = 2272.5, 2273; row 9, 12 - 3 = 9: 909 x 0.75 = 681.75,
    // 682; row 9, 1 - 2 below 0 so 0: 900 x 0.75 = 675; row 1, rating 1:
    // 101 x (0.1 x 3 + 1) = 131.3, 132.
    let wanted = [154, 2273, 682, 675, 132];
    let what = format!("{seen:?}");
    if seen == wanted {
        Ok(what)
    } else {
        Err(format!("{what}, wanted {wanted:?}"))
    }
}
