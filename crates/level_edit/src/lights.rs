//! Lights placed by hand (`docs/LEVEL-EDIT.md`, "Lights"), for the fitted
//! lights of `game::fitted_lights`. An installed level's lights placed by
//! hand are the `[[placed]]` tables of `data/lights/fittings.toml` whose
//! `area` is its module. A new map's are the `[[light]]` tables of its
//! project's `kjk-area.toml` (`area::KjkArea::lights`).
//!
//! `write_placed` changes one area's `[[placed]]` tables of a
//! `fittings.toml` text and leaves every other line as it was: the file's
//! comments, the other areas' lights and their order.

use toml_edit::{DocumentMut, Item, Table, value};

/// A light placed by hand, in game space (metres).
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLight {
    pub at: [f32; 3],
    /// Its colour times its strength, as stored (1 is full white).
    pub colour: [f32; 3],
    /// How far it reaches.
    pub radius: f32,
    /// The way it shines and how narrow (0 shines all ways, 0.2 the half
    /// of space in front of a panel, 0.7 a spotlight).
    pub aim: [f32; 3],
    pub cone: f32,
    /// The way a strip runs and half its length; zeros for a light at one
    /// place.
    pub way: [f32; 3],
    pub half: f32,
    /// The glowing texture it stands for, if any.
    pub texture: String,
    /// Why it is there, in a sentence.
    pub reason: String,
}

impl Default for PlacedLight {
    fn default() -> PlacedLight {
        PlacedLight {
            at: [0.0; 3],
            colour: [1.0, 0.95, 0.85],
            radius: 5.0,
            aim: [0.0; 3],
            cone: 0.0,
            way: [0.0; 3],
            half: 0.0,
            texture: String::new(),
            reason: String::new(),
        }
    }
}

fn bad(what: impl std::fmt::Display) -> String {
    format!("lights: {what}")
}

pub(crate) fn number(item: Option<&Item>) -> Option<f32> {
    let item = item?;
    item.as_float()
        .or_else(|| item.as_integer().map(|value| value as f64))
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
}

/// The number a value of a list holds, when it is a real one. TOML
/// writes `nan` and `inf` as numbers too. Neither places, sizes nor turns
/// anything, so no reader of a project's files takes them.
pub(crate) fn real(value: &toml_edit::Value) -> Option<f32> {
    (value.as_float())
        .or_else(|| value.as_integer().map(|whole| whole as f64))
        .map(|number| number as f32)
        .filter(|number| number.is_finite())
}

/// Three real numbers, when the item is a list of just those. It used to
/// leave out what was no number and take what was left, so `nan` and `inf`
/// were taken, and a list of four with one of them wrong read as three.
pub(crate) fn triple(item: Option<&Item>) -> Option<[f32; 3]> {
    let array = item?.as_array()?;
    if array.len() != 3 {
        return None;
    }
    let mut out = [0.0; 3];
    for (slot, value) in out.iter_mut().zip(array.iter()) {
        *slot = real(value)?;
    }
    Some(out)
}

pub(crate) fn text(item: Option<&Item>) -> String {
    item.and_then(Item::as_str).unwrap_or("").to_string()
}

impl PlacedLight {
    /// The light a table holds (`[[placed]]` or `[[light]]`): `at`,
    /// `colour` and `radius` must be there.
    pub fn read(table: &Table) -> Result<PlacedLight, String> {
        Ok(PlacedLight {
            at: triple(table.get("at")).ok_or_else(|| bad("a light's at is not three numbers"))?,
            colour: triple(table.get("colour"))
                .ok_or_else(|| bad("a light's colour is not three numbers"))?,
            radius: number(table.get("radius")).ok_or_else(|| bad("a light has no radius"))?,
            aim: triple(table.get("aim")).unwrap_or([0.0; 3]),
            cone: number(table.get("cone")).unwrap_or(0.0),
            way: triple(table.get("way")).unwrap_or([0.0; 3]),
            half: number(table.get("half")).unwrap_or(0.0),
            texture: text(table.get("texture")),
            reason: text(table.get("reason")),
        })
    }

    /// The light as a table, its numbers as short as they read back the
    /// same. `head` comes first (a `[[placed]]` table's game and area).
    pub fn table(&self, head: &[(&str, &str)]) -> Table {
        let triple = triple_value;
        let mut table = Table::new();
        for (key, text) in head {
            table.insert(key, value(*text));
        }
        if !self.texture.is_empty() {
            table.insert("texture", value(self.texture.as_str()));
            table.insert("on", value("room"));
        }
        table.insert("at", triple(self.at));
        table.insert("colour", triple(self.colour));
        table.insert("radius", float(self.radius));
        if self.cone > 0.0 {
            table.insert("aim", triple(self.aim));
            table.insert("cone", float(self.cone));
        }
        if self.half > 0.0 {
            table.insert("way", triple(self.way));
            table.insert("half", float(self.half));
        }
        if !self.reason.is_empty() {
            table.insert("reason", value(self.reason.as_str()));
        }
        table
    }
}

/// A number as TOML in an f32's shortest form, so 0.2 is written as 0.2.
pub(crate) fn float(v: f32) -> Item {
    value(v.to_string().parse::<f64>().unwrap_or(f64::from(v)))
}

/// Three numbers as a TOML list, each in an f32's shortest form.
pub(crate) fn triple_value(v: [f32; 3]) -> Item {
    let mut array = toml_edit::Array::new();
    for x in v {
        array.push(x.to_string().parse::<f64>().unwrap_or(f64::from(x)));
    }
    value(array)
}

/// A TOML text cut at its table headers: the text before the first
/// header, then each table from its header line to the next header line.
/// Joined again they are the text.
pub(crate) fn blocks(text: &str) -> Vec<String> {
    let mut blocks: Vec<String> = vec![String::new()];
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with('[') {
            blocks.push(String::new());
        }
        blocks.last_mut().expect("one at least").push_str(line);
    }
    blocks
}

/// Whether a table of an area (`[[placed]]`, `[[lamp]]`) is `game`'s and
/// `area`'s.
pub(crate) fn is_of(table: &Table, game: &str, area: &str) -> bool {
    let field = |key: &str| table.get(key).and_then(Item::as_str).unwrap_or("");
    field("game").eq_ignore_ascii_case(game) && field("area").eq_ignore_ascii_case(area)
}

/// The tables `key` of a `fittings.toml` text that are `game`'s area
/// `area`'s (its module, as the game names it).
pub(crate) fn area_tables(
    text: &str,
    key: &str,
    game: &str,
    area: &str,
) -> Result<Vec<Table>, String> {
    Ok(tables(text, key)?
        .into_iter()
        .filter(|table| is_of(table, game, area))
        .collect())
}

/// Every table `key` of a `fittings.toml` text, of each game and area.
pub(crate) fn tables(text: &str, key: &str) -> Result<Vec<Table>, String> {
    let document: DocumentMut = text.parse().map_err(bad)?;
    let Some(tables) = document.get(key).and_then(Item::as_array_of_tables) else {
        return Ok(Vec::new());
    };
    Ok(tables.iter().cloned().collect())
}

/// The lights a `fittings.toml` text places by hand in `game`'s area
/// `area` (its module, as the game names it).
pub fn read_placed(text: &str, game: &str, area: &str) -> Result<Vec<PlacedLight>, String> {
    area_tables(text, "placed", game, area)?
        .iter()
        .map(PlacedLight::read)
        .collect()
}

/// `text` (a `fittings.toml`) with `game`'s area `area` placing `lights` by
/// hand: its `[[placed]]` tables replaced where the first of them stood
/// (else after the file's last line), and every other line kept. The file
/// is cut at its table headers, so each table is the text from its header
/// to the next one; the new tables are written in the file's own layout.
pub fn write_placed(
    text: &str,
    game: &str,
    area: &str,
    lights: &[PlacedLight],
) -> Result<String, String> {
    let head = [("game", game), ("area", area)];
    let tables: Vec<Table> = lights.iter().map(|light| light.table(&head)).collect();
    write_area_tables(text, "placed", game, area, &tables)
}

/// `text` (a `fittings.toml`) with the tables `key` of `game`'s area `area`
/// replaced by `tables`, as `write_placed` replaces its lights: where the
/// first of them stood (else after the file's last line), with every other
/// line kept.
pub(crate) fn write_area_tables(
    text: &str,
    key: &str,
    game: &str,
    area: &str,
    tables: &[Table],
) -> Result<String, String> {
    let header = format!("[[{key}]]");
    let mut blocks = blocks(text);
    let ours = |block: &str| {
        block.trim_start().starts_with(&header)
            && block
                .parse::<DocumentMut>()
                .ok()
                .and_then(|document| {
                    let tables = document.get(key)?.as_array_of_tables()?;
                    Some(tables.iter().any(|table| is_of(table, game, area)))
                })
                .unwrap_or(false)
    };
    let first = blocks.iter().position(|block| ours(block));
    blocks.retain(|block| !ours(block));
    let new: Vec<String> = tables
        .iter()
        .map(|table| format!("{header}\n{table}\n"))
        .collect();
    let mut out = String::new();
    let at = first.unwrap_or(blocks.len());
    for (index, block) in blocks.iter().enumerate() {
        if index == at {
            new.iter().for_each(|table| out.push_str(table));
        }
        out.push_str(block);
    }
    if at >= blocks.len() {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        new.iter().for_each(|table| out.push_str(table));
        // The file ends with its last line, not with a blank one.
        if out.ends_with("\n\n") {
            out.pop();
        }
    }
    // A text that is no TOML any more is a fault of this writer.
    out.parse::<DocumentMut>().map_err(bad)?;
    Ok(out)
}
