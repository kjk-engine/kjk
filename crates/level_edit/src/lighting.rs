//! A project's own lighting rules: the kinds of light fitting and the
//! rooms' bake levels it changed, kept in its folder as `kjk-lighting.toml`.
//!
//! kjk's light data (`data/lights`) says what each kind of fitting does in
//! a game and what level each room of a module's bake aims at. A project
//! takes those until it changes one, and from then on keeps its own here:
//!
//! - a `[[kind]]` table for each kind it changed, in the layout of
//!   `data/lights/<game>.toml` with the kind's `reason` beside it. The game
//!   reads them in place of the light data's while the project is played
//!   (`game::fitted_lights`).
//! - a `[levels.<module>]` table for each of its levels whose rooms' bake
//!   levels it changed: each room's name and level. It is that module's
//!   whole list.
//!
//! A project that changed neither has no such file. The level editor's
//! Publish writes these to the light data, and they leave the file.
//!
//! A project also keeps a bake of a level's lightmaps (`kjk detail-bake`)
//! in its folder, as `kjk-bake/<module>/<lightmap>.png` (`keep_bake`). The
//! game draws the level's rooms with it while the project is played.

use std::collections::BTreeMap;
use std::path::Path;

use crate::kinds::KindEdit;

/// The file in a project's folder.
pub const FILE: &str = "kjk-lighting.toml";

/// The file of a bake's folder that says how much of its stored light each
/// baked lightmap takes when the game loads it (`game::viewer`).
pub const FILL: &str = "fill.txt";

/// The file of a bake's folder that says the least share of its stored
/// light each texel of a baked lightmap shows (`game::viewer`).
pub const FLOOR: &str = "floor.txt";

/// The file of a bake's folder that asks the game to show the rooms'
/// surfaces with no lightmap dark (`game::viewer`).
pub const UNLIT: &str = "unlit.txt";

/// The folder in a project's folder that holds its kept bakes, one
/// folder a module.
pub const BAKES: &str = "kjk-bake";

/// Where the project in `folder` keeps the bake of its level `module`.
pub fn bake_folder(folder: &Path, module: &str) -> std::path::PathBuf {
    folder.join(BAKES).join(module.to_ascii_lowercase())
}

/// The baked lightmaps (`NAME.png`) in `folder`.
fn pictures(folder: &Path) -> Vec<std::path::PathBuf> {
    let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && (path.extension().and_then(|e| e.to_str()))
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
        })
        .collect();
    found.sort();
    found
}

/// The bake the project in `folder` keeps of its level `module`: its
/// folder, when it holds a baked lightmap.
pub fn kept_bake(folder: &Path, module: &str) -> Option<std::path::PathBuf> {
    let bake = bake_folder(folder, module);
    (!pictures(&bake).is_empty()).then_some(bake)
}

/// The file in a bake's folder that holds a digest of what the bake was
/// made from: the level's lights, lamps, kinds of fitting, room levels and
/// rooms as they were when the bake started. The level editor writes it,
/// and marks a kept bake as stale once the level's own digest differs.
pub const MADE_FROM: &str = "made-from.txt";

/// Keeps the bake in `from` (a folder of baked lightmaps, `NAME.png`) as
/// the bake of the level `module` of the project in `folder`, in place of
/// the one kept before. How many lightmaps it holds.
pub fn keep_bake(folder: &Path, module: &str, from: &Path) -> Result<usize, String> {
    let found = pictures(from);
    if found.is_empty() {
        return Err(format!("{} holds no baked lightmap", from.display()));
    }
    let bake = bake_folder(folder, module);
    let failed = |error: std::io::Error| format!("{}: {error}", bake.display());
    // Made beside the old one, then put in its place, so a copy that fails
    // leaves the old bake.
    let fresh = bake.with_extension("new");
    let _ = std::fs::remove_dir_all(&fresh);
    std::fs::create_dir_all(&fresh).map_err(failed)?;
    for picture in &found {
        let name = picture.file_name().ok_or("a lightmap has no name")?;
        std::fs::copy(picture, fresh.join(name.to_ascii_lowercase())).map_err(failed)?;
    }
    // The notes the game reads beside the lightmaps (`FILL`, `FLOOR`,
    // `UNLIT`) and what the bake was made from (`MADE_FROM`), where the
    // bake has them.
    for note in [FILL, FLOOR, UNLIT, MADE_FROM] {
        let had = from.join(note);
        if had.is_file() {
            std::fs::copy(&had, fresh.join(note)).map_err(failed)?;
        }
    }
    if bake.exists() {
        std::fs::remove_dir_all(&bake).map_err(failed)?;
    }
    std::fs::rename(&fresh, &bake).map_err(failed)?;
    Ok(found.len())
}

/// Takes the bake of the level `module` out of the project in `folder`.
/// False when it kept none.
pub fn drop_bake(folder: &Path, module: &str) -> Result<bool, String> {
    let bake = bake_folder(folder, module);
    if !bake.exists() {
        return Ok(false);
    }
    std::fs::remove_dir_all(&bake).map_err(|error| format!("{}: {error}", bake.display()))?;
    // The folder of the bakes goes with its last one.
    let _ = std::fs::remove_dir(folder.join(BAKES));
    Ok(true)
}

/// What a project's `kjk-lighting.toml` holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OwnLighting {
    /// The kinds of fitting the project changed.
    pub kinds: Vec<KindEdit>,
    /// By module, the level each room of its bake aims at, for the modules
    /// whose levels the project changed.
    pub levels: BTreeMap<String, BTreeMap<String, f32>>,
}

impl OwnLighting {
    pub fn is_empty(&self) -> bool {
        self.kinds.is_empty() && self.levels.is_empty()
    }

    /// Reads the file's text.
    pub fn parse(text: &str) -> Result<OwnLighting, String> {
        let bad = |what: &str| format!("{FILE}: {what}");
        let document: toml_edit::DocumentMut =
            text.parse().map_err(|error| format!("{FILE}: {error}"))?;
        let mut own = OwnLighting::default();
        let tables = document
            .get("kind")
            .and_then(toml_edit::Item::as_array_of_tables);
        for table in tables.into_iter().flat_map(|tables| tables.iter()) {
            let texture = (table.get("texture").and_then(toml_edit::Item::as_str))
                .ok_or_else(|| bad("a kind names no texture"))?;
            let placeable = table.get("on").and_then(toml_edit::Item::as_str) == Some("placeable");
            let mut kind = crate::kinds::read_kind(text, texture, placeable)
                .ok_or_else(|| bad(&format!("the kind {texture} does not read")))?;
            kind.reason = (table.get("reason").and_then(toml_edit::Item::as_str))
                .unwrap_or("")
                .to_string();
            own.kinds.push(kind);
        }
        let modules = document.get("levels").and_then(toml_edit::Item::as_table);
        for (module, rooms) in modules.into_iter().flat_map(|modules| modules.iter()) {
            let rooms = rooms
                .as_table()
                .ok_or_else(|| bad(&format!("levels.{module} is not a table")))?;
            let mut levels = BTreeMap::new();
            for (room, level) in rooms.iter() {
                let level = (level.as_float())
                    .or_else(|| level.as_integer().map(|level| level as f64))
                    .ok_or_else(|| bad(&format!("the level of {room} is not a number")))?;
                levels.insert(room.to_ascii_lowercase(), level as f32);
            }
            own.levels.insert(module.to_ascii_lowercase(), levels);
        }
        Ok(own)
    }

    /// The file's text.
    pub fn write(&self) -> String {
        let mut out = String::from(
            "# This project's own lighting rules, in place of kjk's light data: the\n\
             # kinds of light fitting it changed, and the level each room of a\n\
             # level's bake aims at. Written by kjk's level editor\n\
             # (docs/LEVEL-EDIT.md).\n",
        );
        for kind in &self.kinds {
            out.push('\n');
            out.push_str(crate::kinds::kind_entry(kind).trim_end());
            out.push_str(&format!("\nreason = {:?}\n", kind.reason));
        }
        for (module, levels) in &self.levels {
            out.push_str(&format!("\n[levels.{module:?}]\n"));
            for (room, level) in levels {
                out.push_str(&format!("{room:?} = {level:?}\n"));
            }
        }
        out
    }

    /// The project's own rules in `folder`: none when it has no file.
    pub fn read(folder: &Path) -> Result<OwnLighting, String> {
        let path = folder.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => OwnLighting::parse(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(OwnLighting::default())
            }
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    /// Writes the file in `folder`, beside then over the old one. With
    /// nothing to hold, the file is removed.
    pub fn save(&self, folder: &Path) -> Result<(), String> {
        let path = folder.join(FILE);
        let failed = |error: std::io::Error| format!("{}: {error}", path.display());
        if self.is_empty() {
            return match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(failed(error)),
            };
        }
        let fresh = folder.join(format!("{FILE}.new"));
        std::fs::write(&fresh, self.write())
            .and_then(|()| std::fs::rename(&fresh, &path))
            .map_err(failed)
    }

    /// The kinds as `[[kind]]` entries alone, as the game reads them
    /// (`game::fitted_lights`).
    pub fn kinds_text(&self) -> String {
        self.kinds
            .iter()
            .map(|kind| format!("{}\n", crate::kinds::kind_entry(kind)))
            .collect()
    }
}
