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

use crate::lights::{area_tables, float, number, text, triple, triple_value, write_area_tables};

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
    area_tables(text, "lamp", game, area)?
        .iter()
        .map(PlacedLamp::read)
        .collect()
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
    if !lamp.at.iter().chain([&lamp.yaw]).all(|v| v.is_finite()) {
        return Err("a lamp's numbers must be finite".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lamp_reads_back_as_written() {
        let lamp = PlacedLamp {
            yaw: 90.0,
            pitch: -45.0,
            roll: 12.5,
            reason: "Dark corner.".into(),
            ..PlacedLamp::new("M13AA_01A", "m13aa_01a_lamp2", [1.5, -2.0, 4.2])
        };
        let text = write_lamps("", "k1", "danm13", std::slice::from_ref(&lamp)).unwrap();
        assert_eq!(read_lamps(&text, "k1", "danm13").unwrap(), vec![lamp]);
        assert!(read_lamps(&text, "tsl", "danm13").unwrap().is_empty());
        assert!(text.contains("model = \"m13aa_01a\""));
        assert!(text.contains("on = \"room\""));
        assert!(text.contains("pitch = -45"));
        let flat = write_lamps(
            "",
            "k1",
            "danm13",
            &[PlacedLamp::new("plc_lamp", "", [0.0; 3])],
        );
        assert!(
            !flat.unwrap().contains("pitch"),
            "a lamp not tipped says nothing of it"
        );
    }

    #[test]
    fn lamps_and_lights_of_an_area_keep_each_other() {
        let light = crate::lights::PlacedLight {
            at: [1.0, 2.0, 3.0],
            ..Default::default()
        };
        let lamp = PlacedLamp::new("plc_lamp", "", [0.0; 3]);
        let text = "# Lights.\n\n[rules]\nstrong = 1.1\n";
        let text = crate::lights::write_placed(text, "k1", "danm13", std::slice::from_ref(&light))
            .unwrap();
        let text = write_lamps(&text, "k1", "danm13", std::slice::from_ref(&lamp)).unwrap();
        assert_eq!(
            crate::lights::read_placed(&text, "k1", "danm13").unwrap(),
            vec![light]
        );
        assert_eq!(read_lamps(&text, "k1", "danm13").unwrap(), vec![lamp]);
        assert!(text.starts_with("# Lights.\n\n[rules]\nstrong = 1.1\n"));
        // Taking the lamps away leaves the light.
        let text = write_lamps(&text, "k1", "danm13", &[]).unwrap();
        assert!(!text.contains("[[lamp]]"));
        assert!(text.contains("[[placed]]"));
    }

    #[test]
    fn lamps_are_placed_moved_turned_copied_and_deleted_with_undo() {
        use crate::{Edit, Editor, KjkArea, Project, Target, git};
        let mut editor = Editor::new(Project::new(
            &std::env::temp_dir().join("kjk-lamps"),
            "k1",
            "danm13",
            "m13aa",
            git::empty(),
        ));
        let lamp = PlacedLamp::new("m13aa_01a", "lamp2", [4.0, 5.0, 2.5]);
        editor.apply(Edit::AddLamp(lamp.clone())).unwrap();
        assert_eq!(editor.selected, Some(Target::Lamp(0)));
        editor
            .apply(Edit::Move(Target::Lamp(0), [1.0, 1.0, 2.0]))
            .unwrap();
        editor.apply(Edit::Turn(Target::Lamp(0), 450.0)).unwrap();
        assert_eq!(
            editor.placement(Target::Lamp(0)),
            Some(([1.0, 1.0, 2.0], 90.0))
        );
        editor.apply(Edit::Duplicate(Target::Lamp(0))).unwrap();
        assert_eq!(editor.project.kjk.lamps.len(), 2);
        let mut kept = editor.project.kjk.lamps[1].clone();
        kept.reason = "The hall's far end was dark.".into();
        editor.apply(Edit::SetLamp(1, kept)).unwrap();
        editor.apply(Edit::Delete(Target::Lamp(0))).unwrap();
        assert_eq!(
            editor.project.kjk.lamps[0].reason,
            "The hall's far end was dark."
        );
        assert!(
            editor
                .apply(Edit::AddLamp(PlacedLamp::new("", "", [0.0; 3])))
                .is_err()
        );
        assert!(editor.apply(Edit::Move(Target::Lamp(5), [0.0; 3])).is_err());
        // The delete, the change, the copy, the turn and the move undone.
        for _ in 0..5 {
            assert!(editor.undo());
        }
        assert_eq!(editor.project.kjk.lamps, vec![lamp.clone()]);

        // A new map keeps its lamps in `kjk-area.toml`; an installed
        // level's go to the light data.
        let area = KjkArea {
            lamps: vec![lamp],
            ..KjkArea::default()
        };
        assert_eq!(KjkArea::parse(&area.write()).unwrap(), area);
        let mut installed = editor.project.clone();
        installed.kjk = area;
        assert!(installed.own_kjk().lamps.is_empty());
    }

    #[test]
    fn a_whole_model_is_a_placeables_kind() {
        assert!(PlacedLamp::new("plc_lamp", "", [0.0; 3]).on_placeable);
        assert!(!PlacedLamp::new("m01aa_01a", "lamp", [0.0; 3]).on_placeable);
    }
}
