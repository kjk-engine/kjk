//! The level each room of a level's bake aims at (`docs/LEVEL-EDIT.md`,
//! "Lights"): the bake of the fitted lights (`kjk detail-bake`) scales each
//! room's light to a level. `data/lights/bake.toml` says where a module's
//! levels come from; a module with `levels` names a file of
//! `data/lights/levels/` with one line for each room, its name and its
//! level. The level editor shows each room's level and changes the file.

use std::collections::BTreeMap;

use toml_edit::{DocumentMut, Item, value};

use crate::lights::blocks;

/// A levels file's levels, by room (lowercase).
pub fn read_levels(text: &str) -> BTreeMap<String, f32> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            let room = words.next()?.to_ascii_lowercase();
            let level = words.next()?.parse::<f32>().ok()?;
            Some((room, level))
        })
        .collect()
}

/// A levels file holding `levels`. Each line of `old` (the file as it
/// was) stays as it is, but for a room whose level changed, which gets
/// the new level, and a room that has none any more, whose line goes. A
/// room the file has no line for gets one at the end. Without `old`, the
/// file is `note` as a comment, then a line for each room.
pub fn write_levels(old: Option<&str>, levels: &BTreeMap<String, f32>, note: &str) -> String {
    let mut out = String::new();
    let mut written: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    match old {
        Some(text) => {
            for line in text.lines() {
                let mut words = line.split_whitespace();
                let room = words.next().filter(|word| !word.starts_with('#'));
                let stated = words.next().and_then(|word| word.parse::<f32>().ok());
                match (room, stated) {
                    (Some(room), Some(stated)) => {
                        let room = room.to_ascii_lowercase();
                        match levels.get(&room) {
                            Some(level) if *level == stated => out.push_str(line),
                            Some(level) => out.push_str(&format!("{room} {level:.4}")),
                            None => continue,
                        }
                        written.insert(room);
                    }
                    _ => out.push_str(line),
                }
                out.push('\n');
            }
        }
        None => out.push_str(&format!("# {note}\n")),
    }
    for (room, level) in levels {
        if !written.contains(room) {
            out.push_str(&format!("{room} {level:.4}\n"));
        }
    }
    out
}

/// What `bake.toml` says of a module's bake, as the editor shows it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct BakeEntry {
    /// Whether the module has an entry of its own (else `[default]`'s).
    pub own: bool,
    pub share: Option<f32>,
    pub ceiling: Option<f32>,
    /// `original`, `lit` or `live`.
    pub of: String,
    /// Its levels file, relative to the light data.
    pub levels: Option<String>,
    /// Whether the even light fills what the lamps cannot.
    pub even: bool,
    pub sun: bool,
}

/// `module`'s bake as `bake.toml`'s text says it, `[default]`'s where it
/// has no entry.
pub fn bake_entry(text: &str, module: &str) -> BakeEntry {
    let Ok(document) = text.parse::<DocumentMut>() else {
        return BakeEntry::default();
    };
    let own = document
        .get("modules")
        .and_then(|modules| modules.get(module));
    let table = own.or_else(|| document.get("default"));
    let number = |key: &str| {
        let item = table?.get(key)?;
        item.as_float()
            .or_else(|| item.as_integer().map(|value| value as f64))
            .map(|value| value as f32)
    };
    let text = |key: &str| table?.get(key)?.as_str().map(str::to_string);
    BakeEntry {
        own: own.is_some(),
        share: number("share"),
        ceiling: number("ceiling"),
        of: text("of").unwrap_or_else(|| "original".to_string()),
        levels: text("levels"),
        even: table
            .and_then(|table| table.get("even"))
            .and_then(Item::as_bool)
            .unwrap_or(true),
        sun: table.is_some_and(|table| table.get("sun").is_some()),
    }
}

/// `text` (a `bake.toml`) with `module`'s bake aiming at the levels of
/// `file`: its entry's `levels` set and its `share` and `ceiling` taken
/// away (an entry has one or the other), its other fields kept; or a new
/// entry at the end, of `lit` levels, as the editor shows them.
pub fn with_levels_file(text: &str, module: &str, file: &str) -> Result<String, String> {
    let header = format!("[modules.{module}]");
    let mut parts = blocks(text);
    let at = parts
        .iter()
        .position(|block| block.trim_start().starts_with(&header));
    match at {
        Some(at) => {
            let mut document: DocumentMut = parts[at]
                .parse()
                .map_err(|error| format!("bake.toml: {error}"))?;
            let table = document
                .get_mut("modules")
                .and_then(|modules| modules.get_mut(module))
                .and_then(Item::as_table_mut)
                .ok_or("bake.toml: the module's entry does not read")?;
            table.remove("share");
            table.remove("ceiling");
            table.insert("levels", value(file));
            let blank = if parts[at].ends_with("\n\n") {
                "\n"
            } else {
                ""
            };
            parts[at] = format!("{}\n{blank}", document.to_string().trim_end_matches('\n'));
        }
        None => {
            let end = parts.last_mut().expect("one at least");
            if !end.is_empty() && !end.ends_with("\n\n") {
                end.push('\n');
            }
            parts.push(format!("{header}\nlevels = \"{file}\"\nof = \"lit\"\n"));
        }
    }
    let out = parts.concat();
    out.parse::<DocumentMut>()
        .map_err(|error| format!("bake.toml: {error}"))?;
    Ok(out)
}
