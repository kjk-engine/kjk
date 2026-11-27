//! A Jedi Academy level's rooms, as a project keeps them
//! (`docs/LEVEL-EDIT.md`, "Jedi Academy levels").
//!
//! The level's file is Jedi Academy's and stays in its install. A project
//! keeps only how the user cut the level into rooms: the rooms' names, and
//! the cuts in the order they were made, each with the room it gives its
//! inside to and the shape the user placed (`asset_ja::level_rooms`). A
//! shape's points are places in the very file they were placed in, so the
//! rooms also name the level and the digest of its file. Rooms made for
//! another file of the same name are refused.
//!
//! The file is `ja-rooms/<level>.toml` in the project's folder, the
//! level's path with its folders joined by `-` (`maps/mp/ffa3.bsp` is
//! `ja-rooms/mp-ffa3.toml`). Lengths are in metres:
//!
//! ```toml
//! level = "maps/t1_sour.bsp"
//! digest = "9f2c…"
//! rest = "the rest of the level"
//!
//! [[room]]
//! name = "hangar"
//!
//! [[cut]]
//! room = "hangar"
//! up = 12.0
//! down = 1.0
//! margin = 0.1
//! points = [[150.0, -110.0, 2.4], [162.0, -110.0, 2.4], [156.0, -95.0, 2.4]]
//! ```

use std::path::{Path, PathBuf};

/// The folder of a project's levels' rooms.
pub const FOLDER: &str = "ja-rooms";

/// One cut: the room it gives its inside to, and its shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Cut {
    /// The room, as its place in `JaRooms::rooms`.
    pub room: usize,
    pub points: Vec<[f32; 3]>,
    pub up: f32,
    pub down: f32,
    pub margin: f32,
}

/// A level's rooms.
#[derive(Debug, Clone, PartialEq)]
pub struct JaRooms {
    /// The level's path in Jedi Academy's files (`maps/t1_sour.bsp`).
    pub level: String,
    /// The digest of the level's file (`digest`).
    pub digest: String,
    /// The name of what no cut took.
    pub rest: String,
    /// The cut rooms' names, in the order they were made.
    pub rooms: Vec<String>,
    /// The cuts, in the order they were made.
    pub cuts: Vec<Cut>,
}

/// The digest of a level's file, as its rooms name it.
pub fn digest(file: &[u8]) -> String {
    blake3::hash(file).to_hex().to_string()
}

/// The name of the KOTOR room that kjk builds from room `room` of the
/// level `level` (its path, `maps/t1_sour.bsp`): `ja` and 14 letters and
/// digits of a digest of the two. kjk makes the room's model and walkmesh
/// under that name in memory (`kjk_game::ja_made`), and a new map places
/// it as it places a KOTOR room.
pub fn room_resref(level: &str, room: &str) -> String {
    let key = format!("{}#{room}", level.to_ascii_lowercase());
    let digest = blake3::hash(key.as_bytes()).to_hex();
    format!("ja{}", &digest[..14])
}

/// Whether any of the rooms `models` of an area of the game `game` (its
/// key) is a room kjk made in memory from a Jedi Academy level: a room of
/// a new map whose copy the run made (`kjk_game::ja_made`). Such an
/// area's navigation mesh joins its floors as a Jedi Academy level's are
/// joined (`asset_ja::level_collision::joined_nav`).
pub fn holds_made<'a>(game: &str, models: impl IntoIterator<Item = &'a str>) -> bool {
    let Some(game) = vfs::Game::from_key(game) else {
        return false;
    };
    (models.into_iter()).any(|model| vfs::made::holds(game, model, fmt_odyssey::ResType::WOK))
}

/// A level as the user names it: its path without `maps/` and `.bsp`
/// (`t1_sour`, `mp/ffa3`).
pub fn short(level: &str) -> &str {
    level.trim_start_matches("maps/").trim_end_matches(".bsp")
}

/// The name of the file that keeps `level`'s rooms, without its ending:
/// the level's short name with every separator made a `-` (`mp/ffa3` is
/// `mp-ffa3`). `\` and `:` are made one too. Only `/` was, so a level
/// named with `..\` in a project's or a mod's file named a file outside
/// the folder.
pub fn file_stem(level: &str) -> String {
    short(level).replace(['/', '\\', ':'], "-")
}

/// The file of `level`'s rooms in a project's folder.
pub fn path(folder: &Path, level: &str) -> PathBuf {
    folder
        .join(FOLDER)
        .join(format!("{}.toml", file_stem(level)))
}

/// The levels a project's folder keeps rooms of: each level's path, and
/// when its file last changed. A file that does not read is left out.
pub fn kept(folder: &Path) -> Vec<(String, Option<std::time::SystemTime>)> {
    let mut levels: Vec<_> = std::fs::read_dir(folder.join(FOLDER))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
                return None;
            }
            let rooms = JaRooms::parse(&std::fs::read_to_string(&path).ok()?).ok()?;
            let changed = entry.metadata().and_then(|meta| meta.modified()).ok();
            Some((rooms.level, changed))
        })
        .collect();
    levels.sort();
    levels
}

impl JaRooms {
    pub fn parse(text: &str) -> Result<JaRooms, String> {
        let document: toml_edit::DocumentMut = text.parse().map_err(|error| format!("{error}"))?;
        let word = |key: &str| {
            (document.get(key).and_then(toml_edit::Item::as_str))
                .map(str::to_string)
                .ok_or(format!("no {key}"))
        };
        let tables = |key: &str| {
            (document.get(key))
                .and_then(toml_edit::Item::as_array_of_tables)
                .into_iter()
                .flat_map(|tables| tables.iter())
        };
        let number = crate::lights::real;
        let mut rooms: Vec<String> = Vec::new();
        for table in tables("room") {
            let name = (table.get("name").and_then(toml_edit::Item::as_str))
                .ok_or("a room has no name")?;
            if rooms.iter().any(|other| other == name) {
                return Err(format!("two rooms are named {name}"));
            }
            rooms.push(name.to_string());
        }
        let mut cuts = Vec::new();
        for table in tables("cut") {
            let room = (table.get("room").and_then(toml_edit::Item::as_str))
                .ok_or("a cut names no room")?;
            let room = (rooms.iter().position(|other| other == room))
                .ok_or(format!("a cut names the room {room}, which is not listed"))?;
            let length = |key: &str| {
                (table.get(key).and_then(toml_edit::Item::as_value))
                    .and_then(number)
                    .unwrap_or(0.0)
            };
            let listed = (table.get("points").and_then(toml_edit::Item::as_array))
                .ok_or("a cut has no points")?;
            let mut points = Vec::with_capacity(listed.len());
            for value in listed {
                // Every part must be a number: one left out would make a
                // point of the three that are left.
                let place: Vec<f32> = (value.as_array())
                    .into_iter()
                    .flat_map(|parts| parts.iter())
                    .map(number)
                    .collect::<Option<_>>()
                    .ok_or("a cut's point is not three numbers")?;
                let place: [f32; 3] = place
                    .try_into()
                    .map_err(|_| "a cut's point is not three numbers")?;
                points.push(place);
            }
            cuts.push(Cut {
                room,
                points,
                up: length("up"),
                down: length("down"),
                margin: length("margin"),
            });
        }
        Ok(JaRooms {
            level: word("level")?,
            digest: word("digest")?,
            rest: word("rest")?,
            rooms,
            cuts,
        })
    }

    pub fn write(&self) -> String {
        let mut out = format!(
            "# A Jedi Academy level cut into rooms: the rooms' names, and the cuts in\n\
             # the order they were made. Everything inside a cut's shape belongs to\n\
             # its room, and a later cut takes from an earlier one. Lengths are in\n\
             # metres. Written by kjk's level editor (docs/LEVEL-EDIT.md). The\n\
             # level's file itself stays in the game.\n\
             level = {}\ndigest = {}\nrest = {}\n",
            crate::quoted(&self.level),
            crate::quoted(&self.digest),
            crate::quoted(&self.rest)
        );
        for room in &self.rooms {
            out.push_str(&format!("\n[[room]]\nname = {}\n", crate::quoted(room)));
        }
        for cut in &self.cuts {
            let points: Vec<String> = (cut.points.iter())
                .map(|[x, y, z]| format!("[{x:?}, {y:?}, {z:?}]"))
                .collect();
            out.push_str(&format!(
                "\n[[cut]]\nroom = {}\nup = {:?}\ndown = {:?}\nmargin = {:?}\npoints = [{}]\n",
                crate::quoted(self.rooms.get(cut.room).map_or("", String::as_str)),
                cut.up,
                cut.down,
                cut.margin,
                points.join(", ")
            ));
        }
        out
    }

    /// The rooms of `level` kept in `folder`, if it keeps any. Rooms of a
    /// file with another digest than `digest` are an error, which says so.
    pub fn read(folder: &Path, level: &str, digest: &str) -> Result<Option<JaRooms>, String> {
        let path = path(folder, level);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{}: {error}", path.display())),
        };
        let rooms =
            JaRooms::parse(&text).map_err(|error| format!("{}: {error}", path.display()))?;
        if rooms.digest != digest {
            return Err(format!(
                "{}: the rooms were made for another file of {level} than the one installed",
                path.display()
            ));
        }
        Ok(Some(rooms))
    }

    /// Writes the rooms into `folder`, beside then over the old file.
    pub fn save(&self, folder: &Path) -> Result<PathBuf, String> {
        let path = path(folder, &self.level);
        let failed = |error: std::io::Error| format!("{}: {error}", path.display());
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(failed)?;
        }
        let beside = path.with_extension("toml.new");
        std::fs::write(&beside, self.write()).map_err(failed)?;
        std::fs::rename(&beside, &path).map_err(failed)?;
        Ok(path)
    }
}
