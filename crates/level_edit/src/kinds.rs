//! A kind of light fitting, as the level editor changes it
//! (`docs/LEVEL-EDIT.md`, "Lights"). A kind is a glowing texture, on rooms
//! or on placeables, and says how strong its lights are, how far they
//! reach, their colour and their cone, or that it is no lamp.
//!
//! Two files of the light data hold a kind. `<game>.toml` (`k1.toml`,
//! `tsl.toml`) is what the game reads, one `[[kind]]` entry each; it is
//! written by `tools/light-kinds/kinds.py` from the survey and the rules.
//! `fittings.toml`'s `[[fitting]]` entries are the rules that differ for
//! one kind, which `kinds.py` lays over what the general rules give. A
//! kind changed in the editor is written to both: its `[[kind]]` entry, so
//! the game shows it at once, and a `[[fitting]]` with the same numbers,
//! so the next `kinds.py` gives it again.
//!
//! Both files are changed block by block (`lights::blocks`): only the
//! kind's own entry changes, and every other line stays as it was.

use toml_edit::{DocumentMut, Item, Table, value};

use crate::lights::{blocks, float, triple_value};

/// A kind of fitting, as far as the editor changes it.
#[derive(Debug, Clone, PartialEq)]
pub struct KindEdit {
    /// The glowing texture, lowercase.
    pub texture: String,
    /// On placeables (else on rooms).
    pub placeable: bool,
    /// Whether it is a lamp at all.
    pub lamp: bool,
    /// `point`, `panel`, `down`, `strip` or `faint`.
    pub form: String,
    pub colour: [f32; 3],
    pub reach: f32,
    pub strength: f32,
    pub cone: f32,
    /// Why it was changed, for its `[[fitting]]`.
    pub reason: String,
}

impl KindEdit {
    /// Its `on` field: `room` or `placeable`.
    pub fn on(&self) -> &'static str {
        if self.placeable { "placeable" } else { "room" }
    }

    /// Whether a table (a `[[kind]]` or a `[[fitting]]`) names this kind.
    fn names(&self, table: &Table) -> bool {
        let field = |key: &str| table.get(key).and_then(Item::as_str).unwrap_or("");
        field("texture").eq_ignore_ascii_case(&self.texture) && field("on") == self.on()
    }

    /// Sets its numbers in `table`; a kind that is no lamp has
    /// `lamp = false` and none of them.
    fn set_in(&self, table: &mut Table) {
        // A field the table has keeps its place; a new one goes at the end.
        if !self.lamp {
            for key in ["form", "colour", "reach", "strength", "cone"] {
                table.remove(key);
            }
            table.insert("lamp", value(false));
            return;
        }
        table.remove("lamp");
        table.insert("form", value(self.form.as_str()));
        table.insert("colour", triple_value(self.colour));
        table.insert("reach", float(self.reach));
        table.insert("strength", float(self.strength));
        if self.cone > 0.0 {
            table.insert("cone", float(self.cone));
        } else {
            table.remove("cone");
        }
    }
}

fn number(table: &Table, key: &str) -> Option<f32> {
    let item = table.get(key)?;
    item.as_float()
        .or_else(|| item.as_integer().map(|value| value as f64))
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
}

/// The kind `texture` (on placeables or rooms) as a `<game>.toml` text
/// holds it; `None` when it has no entry.
pub fn read_kind(text: &str, texture: &str, placeable: bool) -> Option<KindEdit> {
    let document: DocumentMut = text.parse().ok()?;
    let probe = KindEdit {
        texture: texture.to_ascii_lowercase(),
        placeable,
        lamp: true,
        form: String::new(),
        colour: [1.0; 3],
        reach: 0.0,
        strength: 0.0,
        cone: 0.0,
        reason: String::new(),
    };
    let table = document
        .get("kind")?
        .as_array_of_tables()?
        .iter()
        .find(|table| probe.names(table))?;
    let colour = crate::lights::triple(table.get("colour"));
    Some(KindEdit {
        lamp: table.get("lamp").and_then(Item::as_bool).unwrap_or(true),
        form: table
            .get("form")
            .and_then(Item::as_str)
            .unwrap_or("point")
            .to_string(),
        colour: colour.unwrap_or([1.0; 3]),
        reach: number(table, "reach").unwrap_or(2.5),
        strength: number(table, "strength").unwrap_or(1.0),
        cone: number(table, "cone").unwrap_or(0.0),
        ..probe
    })
}

/// The kind as a `[[kind]]` entry of `<game>.toml`, alone: what the game
/// reads (`game::fitted_lights::set_kinds`).
pub fn kind_entry(kind: &KindEdit) -> String {
    let mut table = Table::new();
    table.insert("texture", value(kind.texture.as_str()));
    table.insert("on", value(kind.on()));
    kind.set_in(&mut table);
    format!("[[kind]]\n{table}")
}

/// `text` (a `<game>.toml`) with `kind`'s entry changed, where it stood, or
/// added at the end when it has none.
pub fn write_kind(text: &str, kind: &KindEdit) -> Result<String, String> {
    let mut parts = blocks(text);
    let mut found = false;
    for block in &mut parts {
        if !block.trim_start().starts_with("[[kind]]") {
            continue;
        }
        let Ok(mut document) = block.parse::<DocumentMut>() else {
            continue;
        };
        let Some(table) = document
            .get_mut("kind")
            .and_then(Item::as_array_of_tables_mut)
            .and_then(|tables| tables.get_mut(0))
        else {
            continue;
        };
        if !kind.names(table) {
            continue;
        }
        kind.set_in(table);
        let blank = if block.ends_with("\n\n") { "\n" } else { "" };
        *block = format!("{}{blank}", document.to_string().trim_end_matches('\n')) + "\n";
        found = true;
    }
    let mut out: String = parts.concat();
    if !found {
        if !out.is_empty() && !out.ends_with("\n\n") {
            out.push('\n');
        }
        out.push_str(&kind_entry(kind));
    }
    out.parse::<DocumentMut>()
        .map_err(|error| format!("kinds: {error}"))?;
    Ok(out)
}

/// `text` (a `fittings.toml`) with `game`'s `[[fitting]]` for `kind` holding
/// its numbers and its reason: the last such entry changed where it stood
/// (its other fields kept), or a new one added at the end.
pub fn write_fitting(text: &str, game: &str, kind: &KindEdit) -> Result<String, String> {
    let mut parts = blocks(text);
    let ours = |block: &str| -> Option<DocumentMut> {
        if !block.trim_start().starts_with("[[fitting]]") {
            return None;
        }
        let document = block.parse::<DocumentMut>().ok()?;
        let table = document.get("fitting")?.as_array_of_tables()?.get(0)?;
        let of_game = table
            .get("game")
            .and_then(Item::as_str)
            .is_some_and(|name| name.eq_ignore_ascii_case(game));
        (of_game && kind.names(table)).then_some(document)
    };
    let last = parts.iter().rposition(|block| ours(block).is_some());
    let reason = |table: &mut Table| {
        if !kind.reason.is_empty() {
            table.insert("reason", value(kind.reason.as_str()));
        }
    };
    match last {
        Some(at) => {
            let mut document = ours(&parts[at]).expect("found above");
            let table = document
                .get_mut("fitting")
                .and_then(Item::as_array_of_tables_mut)
                .and_then(|tables| tables.get_mut(0))
                .expect("found above");
            kind.set_in(table);
            reason(table);
            let blank = if parts[at].ends_with("\n\n") {
                "\n"
            } else {
                ""
            };
            parts[at] = format!("{}\n{blank}", document.to_string().trim_end_matches('\n'));
        }
        None => {
            let mut table = Table::new();
            table.insert("game", value(game));
            table.insert("texture", value(kind.texture.as_str()));
            table.insert("on", value(kind.on()));
            kind.set_in(&mut table);
            reason(&mut table);
            let end = parts.last_mut().expect("one at least");
            if !end.is_empty() && !end.ends_with("\n\n") {
                end.push('\n');
            }
            parts.push(format!("[[fitting]]\n{table}"));
        }
    }
    let out = parts.concat();
    out.parse::<DocumentMut>()
        .map_err(|error| format!("fittings: {error}"))?;
    Ok(out)
}
