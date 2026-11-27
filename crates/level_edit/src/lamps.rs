//! Lamps placed by hand (`docs/LEVEL-EDIT.md`, "Lamps"): a model of the
//! game drawn where the area had none, whose glowing meshes the fitted
//! lights of `game::fitted_lights` light as they light the area's own.
//!
//! A lamp is one of two things:
//! - **A whole model**, such as a placeable's lamp (`mesh` empty). It is
//!   drawn as the model is.
//! - **A light mesh**, the one mesh of a model that is a light fitting,
//!   such as a ceiling panel of a room (`mesh` names its node). Only that
//!   mesh is drawn, with its middle where the lamp stands.
//!
//! The fitted lights read a lamp's glowing texture as a kind of fitting of
//! rooms or of placeables (`on`), as they read the kind for the area's own
//! meshes. A light mesh taken from a room is of a room's kind.
//!
//! An installed level's lamps are the `[[lamp]]` tables of
//! `data/lights/fittings.toml` whose `area` is its module, so the game
//! draws them without a project. A new map's are the `[[lamp]]` tables of
//! its project's `kjk-area.toml` (`area::KjkArea::lamps`).
//!
//! ```toml
//! [[lamp]]
//! game = "k1"
//! area = "danm13"
//! model = "m13aa_01a"
//! mesh = "m13aa_01a_lamp2"
//! on = "room"
//! at = [102.5, 64.0, 4.2]
//! yaw = 90.0
//! pitch = 45.0
//! reason = "The corridor to the council chamber was dark."
//! ```
//!
//! A lamp turns on every axis, which no object of the games' own does: an
//! area's object list keeps one facing for each. A lamp is kjk's alone, so
//! it is turned first by its roll about its own front (X), then tipped by
//! its pitch about its own side (Y), then turned by its yaw about the
//! vertical (Z). A light mesh tipped against a wall shows its lit face to
//! the room and hides its back, which has no faces, in the wall.

use toml_edit::{Item, Table, value};

use crate::lights::{float, is_of, number, tables, text, triple, triple_value, write_area_tables};

/// A lamp placed by hand, in game space (metres).
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLamp {
    /// The model's name (its MDL's resref), lower case.
    pub model: String,
    /// The node of the one mesh drawn, as the model names it; empty for the
    /// whole model.
    pub mesh: String,
    /// Whether its glowing texture is read as a kind of fitting of
    /// placeables rather than of rooms.
    pub on_placeable: bool,
    /// Where it stands: a whole model's origin, a light mesh's middle.
    pub at: [f32; 3],
    /// Degrees from +X, turning left.
    pub yaw: f32,
    /// Degrees it is tipped about its own side (Y, before its yaw): its
    /// front goes down for more.
    pub pitch: f32,
    /// Degrees it is rolled about its own front (X, before its pitch).
    pub roll: f32,
    /// Why it is there, in a sentence.
    pub reason: String,
}

impl PlacedLamp {
    /// A lamp of `model` (and its mesh `mesh`, if any) at `at`. A whole
    /// model is read as a placeable's, a light mesh as a room's.
    pub fn new(model: &str, mesh: &str, at: [f32; 3]) -> PlacedLamp {
        PlacedLamp {
            model: model.to_ascii_lowercase(),
            mesh: mesh.to_string(),
            on_placeable: mesh.is_empty(),
            at,
            yaw: 0.0,
            pitch: 0.0,
            roll: 0.0,
            reason: String::new(),
        }
    }

    /// The name the editor shows: the model, and its mesh for a light
    /// mesh.
    pub fn label(&self) -> String {
        match self.mesh.is_empty() {
            true => self.model.clone(),
            false => format!("{} / {}", self.model, self.mesh),
        }
    }

    /// The lamp a table holds (`[[lamp]]`): `model` and `at` must be there.
    pub fn read(table: &Table) -> Result<PlacedLamp, String> {
        let model = text(table.get("model"));
        if model.is_empty() {
            return Err(bad("a lamp has no model"));
        }
        let mesh = text(table.get("mesh"));
        let on_placeable = match table.get("on").and_then(Item::as_str) {
            None => mesh.is_empty(),
            Some(on) if on.eq_ignore_ascii_case("placeable") => true,
            Some(on) if on.eq_ignore_ascii_case("room") => false,
            Some(on) => return Err(bad(format!("a lamp's on is {on:?}, not room or placeable"))),
        };
        Ok(PlacedLamp {
            model: model.to_ascii_lowercase(),
            mesh,
            on_placeable,
            at: triple(table.get("at")).ok_or_else(|| bad("a lamp's at is not three numbers"))?,
            yaw: number(table.get("yaw")).unwrap_or(0.0),
            pitch: number(table.get("pitch")).unwrap_or(0.0),
            roll: number(table.get("roll")).unwrap_or(0.0),
            reason: text(table.get("reason")),
        })
    }

    /// The lamp as a table, its numbers as short as they read back the
    /// same. `head` comes first (a `[[lamp]]` table's game and area).
    pub fn table(&self, head: &[(&str, &str)]) -> Table {
        let mut table = Table::new();
        for (key, text) in head {
            table.insert(key, value(*text));
        }
        table.insert("model", value(self.model.as_str()));
        if !self.mesh.is_empty() {
            table.insert("mesh", value(self.mesh.as_str()));
        }
        table.insert(
            "on",
            value(if self.on_placeable {
                "placeable"
            } else {
                "room"
            }),
        );
        table.insert("at", triple_value(self.at));
        for (key, degrees) in [
            ("yaw", self.yaw),
            ("pitch", self.pitch),
            ("roll", self.roll),
        ] {
            if degrees != 0.0 {
                table.insert(key, float(degrees));
            }
        }
        if !self.reason.is_empty() {
            table.insert("reason", value(self.reason.as_str()));
        }
        table
    }
}

fn bad(what: impl std::fmt::Display) -> String {
    format!("lamps: {what}")
}

/// The lamps a `fittings.toml` text places by hand in `game`'s area `area`
/// (its module, as the game names it).
pub fn read_lamps(text: &str, game: &str, area: &str) -> Result<Vec<PlacedLamp>, String> {
    LampTables::read(text)?.of_area(game, area)
}

/// The `[[lamp]]` tables of a `fittings.toml` text, of every game and
/// area, read once. The game keeps them and asks them for the lamps of
/// each area it loads (`of_area`). `read_lamps` reads the whole text for
/// one area.
#[derive(Debug, Clone, Default)]
pub struct LampTables(Vec<Table>);

impl LampTables {
    /// The lamp tables of `text`.
    pub fn read(text: &str) -> Result<LampTables, String> {
        tables(text, "lamp").map(LampTables)
    }

    /// The lamps of `game`'s area `area` (its module, as the game names
    /// it).
    pub fn of_area(&self, game: &str, area: &str) -> Result<Vec<PlacedLamp>, String> {
        (self.0.iter())
            .filter(|table| is_of(table, game, area))
            .map(PlacedLamp::read)
            .collect()
    }
}

/// `text` (a `fittings.toml`) with `game`'s area `area` placing `lamps`:
/// its `[[lamp]]` tables replaced where the first of them stood (else after
/// the file's last line), and every other line kept, as
/// `lights::write_placed` writes the lights.
pub fn write_lamps(
    text: &str,
    game: &str,
    area: &str,
    lamps: &[PlacedLamp],
) -> Result<String, String> {
    let head = [("game", game), ("area", area)];
    let tables: Vec<Table> = lamps.iter().map(|lamp| lamp.table(&head)).collect();
    write_area_tables(text, "lamp", game, area, &tables)
}

/// What is wrong with a lamp, if anything.
pub fn problem(lamp: &PlacedLamp) -> Result<(), String> {
    if lamp.model.is_empty() || lamp.model.len() > crate::git::RESREF_LEN {
        return Err(format!("{:?} is not a model's name", lamp.model));
    }
    let turns = [&lamp.yaw, &lamp.pitch, &lamp.roll];
    if !lamp.at.iter().chain(turns).all(|v| v.is_finite()) {
        return Err("a lamp's numbers must be finite".into());
    }
    Ok(())
}
