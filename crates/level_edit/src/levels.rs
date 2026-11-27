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
    /// Whether a room its lamps cannot light takes its own stored light for
    /// the rest (`fill = "stored"`, `kjk detail-bake --fill-stored`). The
    /// bake then writes `fill.txt` beside its lightmaps.
    pub fill_stored: bool,
    /// The least and the most a room's lamps are scaled by (`--least`,
    /// `--most`), where the entry sets them.
    pub least: Option<f32>,
    pub most: Option<f32>,
    /// The least share of its stored linear light each texel of a baked
    /// lightmap shows in the game (`floor`, `--floor`). The bake then
    /// writes `floor.txt` beside its lightmaps.
    pub floor: Option<f32>,
    /// Whether every room with a lightmapped mesh is baked
    /// (`every_room = true`, `--every-room`).
    pub every_room: bool,
    /// Whether the backdrop keeps its stored light
    /// (`backdrop = "stored"`, `--stored-backdrop`).
    pub stored_backdrop: bool,
    /// What every level of the entry's levels file is times (`scale`).
    pub scale: Option<f32>,
    /// Whether the game shows the rooms' surfaces with no lightmap dark
    /// under the bake (`dark_unlit = true`). The bake's folder then holds
    /// `unlit.txt`.
    pub dark_unlit: bool,
    pub sun: bool,
    /// The sun and sky as `kjk detail-bake --sun` takes them: the numbers
    /// of `toward`, `colour`, `sky`, `toward2` and `colour2`, then `width`,
    /// as `tools/light-kinds/bake.sh` passes them.
    pub sun_args: Option<String>,
    /// The sun's skylights as `--skylights` takes them, by node name.
    pub skylights: Option<String>,
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
            .filter(|value| value.is_finite())
    };
    let text = |key: &str| table?.get(key)?.as_str().map(str::to_string);
    let flag = |key: &str| {
        table
            .and_then(|table| table.get(key))
            .and_then(Item::as_bool)
            .unwrap_or(false)
    };
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
        fill_stored: text("fill").as_deref() == Some("stored"),
        least: number("least"),
        most: number("most"),
        floor: number("floor"),
        every_room: flag("every_room"),
        stored_backdrop: text("backdrop").as_deref() == Some("stored"),
        scale: number("scale"),
        dark_unlit: flag("dark_unlit"),
        sun: table.is_some_and(|table| table.get("sun").is_some()),
        sun_args: table.and_then(|table| sun_args(table.get("sun")?)),
        skylights: table
            .and_then(|table| table.get("sun")?.get("skylights")?.as_array())
            .map(|names| {
                names
                    .iter()
                    .filter_map(|name| name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .filter(|names| !names.is_empty()),
    }
}

/// The share of a project's sun's level that the whole sky shows
/// (`project_sun_args`). The games' outdoor areas in `bake.toml` show 40 to
/// 80 percent.
const PROJECT_SKY_SHARE: f32 = 0.5;
/// Half a project's sun's width in radians (`project_sun_args`), as most
/// entries of `bake.toml` have it.
const PROJECT_SUN_WIDTH: f32 = 0.01;

/// The numbers for `kjk detail-bake --sun` from the sun a project sets for
/// its area (`area::SunLight`), seen from `middle` (game space): the way
/// to it, the level a surface facing it shows, the level a surface that
/// sees the whole sky shows (`PROJECT_SKY_SHARE` of the sun's), and half
/// its width (`PROJECT_SUN_WIDTH`). The level is what the game's light list
/// gives a character facing the sun at `middle`: its colour times its
/// multiplier, falling off as `odyssey.wgsl` has a lamp fall off over the
/// distance. `None` for a sun that does not reach `middle`.
pub fn project_sun_args(sun: &crate::area::SunLight, middle: [f32; 3]) -> Option<String> {
    let way: [f32; 3] = std::array::from_fn(|axis| sun.at[axis] - middle[axis]);
    let distance = way.iter().map(|part| part * part).sum::<f32>().sqrt();
    if distance <= 0.0 || distance >= sun.radius {
        return None;
    }
    let falloff = (sun.radius * sun.radius) / ((sun.radius + distance) * (sun.radius + distance));
    let level = sun.colour.map(|part| part * sun.multiplier * falloff);
    let sky = level.map(|part| part * PROJECT_SKY_SHARE);
    let toward = way.map(|part| part / distance);
    let numbers: Vec<String> = (toward.iter().chain(&level).chain(&sky))
        .chain(std::iter::once(&PROJECT_SUN_WIDTH))
        .map(|number| format!("{number:.4}"))
        .collect();
    Some(numbers.join(" "))
}

/// A `sun`'s numbers for `--sun`.
fn sun_args(sun: &Item) -> Option<String> {
    let mut words: Vec<String> = Vec::new();
    for key in ["toward", "colour", "sky", "toward2", "colour2"] {
        if let Some(array) = sun.get(key).and_then(Item::as_array) {
            for value in array.iter() {
                let number = value
                    .as_float()
                    .or_else(|| value.as_integer().map(|value| value as f64))
                    .filter(|number| number.is_finite())?;
                words.push(number.to_string());
            }
        }
    }
    if let Some(width) = sun.get("width") {
        let width = width
            .as_float()
            .or_else(|| width.as_integer().map(|value| value as f64))
            .filter(|width| width.is_finite())?;
        words.push(width.to_string());
    }
    (!words.is_empty()).then(|| words.join(" "))
}

/// The levels a module whose bake aims at a share of each room's level
/// gives its rooms: `share` times the room's level of kind `of` in the log
/// of a first bake (`kjk detail-bake --gpu`), and at most `ceiling`. The
/// log's lines are `name: original O, colour R G B, lit L` for `original`
/// and `lit`, and `name: N, live L; ...` for `live`. Empty when the log
/// has none.
pub fn levels_from_log(
    log: &str,
    of: &str,
    share: f32,
    ceiling: Option<f32>,
) -> BTreeMap<String, f32> {
    let mut levels = BTreeMap::new();
    for line in log.lines() {
        let Some((room, rest)) = line.trim_start().split_once(": ") else {
            continue;
        };
        if room.contains(' ') {
            continue;
        }
        let level = match of {
            "lit" if rest.starts_with("original ") => rest
                .rsplit_once("lit ")
                .and_then(|(_, number)| number.trim().parse::<f32>().ok()),
            "live" => rest.split_once(", live ").and_then(|(count, after)| {
                count.parse::<u64>().ok()?;
                after.split(';').next()?.trim().parse::<f32>().ok()
            }),
            "original" => rest
                .strip_prefix("original ")
                .and_then(|after| after.split(',').next()?.trim().parse::<f32>().ok()),
            _ => None,
        };
        if let Some(level) = level {
            let aimed = share * level;
            levels.insert(
                room.to_ascii_lowercase(),
                ceiling.map_or(aimed, |most| aimed.min(most)),
            );
        }
    }
    levels
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
            // The file's name is quoted as any text is. Written between
            // bare quotes, a `\\` or a `"` in it broke the file.
            parts.push(format!(
                "{header}\nlevels = {}\nof = \"lit\"\n",
                crate::quoted(file)
            ));
        }
    }
    let out = parts.concat();
    out.parse::<DocumentMut>()
        .map_err(|error| format!("bake.toml: {error}"))?;
    Ok(out)
}
