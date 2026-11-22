//! A project as a mod of the game: what the level editor exports, and what
//! kjk reads of such mods in an install's `override/`.
//!
//! An exported mod is a folder to copy into the game's own folder. It has
//! two parts.
//!
//! - **Files the games read.** `override/` holds each level's object list
//!   (`<area>.git`), the templates, conversations and compiled scripts the
//!   project brings, and an area file the project changed. A new map's
//!   module is `modules/<module>.mod`. KOTOR and kjk both read these, as
//!   they read any mod.
//! - **Files only kjk reads**, which the original game ignores. They are
//!   named so that two mods in one `override/` do not take each other's
//!   place:
//!   - `kjk-area-<area>.toml` for each level: its kjk-only content
//!     (`KjkArea`), with the level's `module` and `area` named at its top.
//!   - `kjk-lighting-<project>.toml`: the kinds of fitting and room levels
//!     the project changed (`lighting::OwnLighting`).
//!   - `kjk-bake/<module>/<lightmap>.png`: the bake the project keeps of a
//!     level.
//!
//! kjk reads the kjk-only files of every mod in `override/` when it runs
//! without `--project` (`active::activate_mods`). Two mods that change the
//! same area take each other's place, as their `<area>.git` files do.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::area::KjkArea;
use crate::lighting::OwnLighting;
use crate::project::Project;

/// The file that marks a folder as an exported mod, with how to install it.
pub const README: &str = "kjk-mod-readme.txt";

/// The file of a level's kjk-only content in `override/`.
pub fn area_file(area: &str) -> String {
    format!("kjk-area-{}.toml", area.to_ascii_lowercase())
}

/// The file of a mod's own lighting rules in `override/`.
pub fn lighting_file(name: &str) -> String {
    format!("kjk-lighting-{}.toml", name.to_ascii_lowercase())
}

/// What the mods in an `override/` hold for kjk.
#[derive(Debug, Default)]
pub struct Found {
    /// Each level a mod changes: its module, its area and its kjk-only
    /// content.
    pub levels: Vec<(String, String, KjkArea)>,
    /// The kinds of fitting the mods changed, a later file's kind in place
    /// of an earlier file's.
    pub lighting: OwnLighting,
    /// The folder of the bake a mod keeps of a level, by module.
    pub bakes: BTreeMap<String, PathBuf>,
    /// The files that did not read, each with why. Such a file is left out.
    pub notes: Vec<String>,
}

impl Found {
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty() && self.lighting.is_empty() && self.bakes.is_empty()
    }
}

/// Reads the kjk-only files among `files`: each file of an `override/` by
/// its lowercase path inside it (`/` between folders) and where it is.
pub fn read(files: &[(String, PathBuf)]) -> Found {
    let mut found = Found::default();
    for (inside, path) in files {
        let top = !inside.contains('/');
        if top && inside.starts_with("kjk-area-") && inside.ends_with(".toml") {
            match read_area(path) {
                Ok(level) => found.levels.push(level),
                Err(why) => found.notes.push(format!("{inside}: {why}")),
            }
        } else if top && inside.starts_with("kjk-lighting-") && inside.ends_with(".toml") {
            let read = std::fs::read_to_string(path)
                .map_err(|error| error.to_string())
                .and_then(|text| OwnLighting::parse(&text));
            match read {
                Ok(own) => {
                    for kind in own.kinds {
                        (found.lighting.kinds).retain(|had| {
                            (had.texture.as_str(), had.placeable)
                                != (kind.texture.as_str(), kind.placeable)
                        });
                        found.lighting.kinds.push(kind);
                    }
                    found.lighting.levels.extend(own.levels);
                }
                Err(why) => found.notes.push(format!("{inside}: {why}")),
            }
        } else if let Some(rest) = inside.strip_prefix("kjk-bake/")
            && rest.ends_with(".png")
            && let Some((module, _)) = rest.split_once('/')
            && let Some(folder) = path.parent()
        {
            found
                .bakes
                .entry(module.to_string())
                .or_insert_with(|| folder.to_path_buf());
        }
    }
    found
}

/// A level's `kjk-area-<area>.toml`: the module and area it names, and its
/// kjk-only content.
fn read_area(path: &Path) -> Result<(String, String, KjkArea), String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let document: toml_edit::DocumentMut = text.parse().map_err(|error| format!("{error}"))?;
    let named = |key: &str| {
        (document.get(key).and_then(toml_edit::Item::as_str))
            .map(str::to_ascii_lowercase)
            .filter(|name| !name.is_empty())
            .ok_or(format!("it names no {key}"))
    };
    Ok((named("module")?, named("area")?, KjkArea::parse(&text)?))
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    /// How many files the games read (`override/` and `modules/`).
    pub game_files: usize,
    /// How many files only kjk reads.
    pub kjk_files: usize,
}

fn failed(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |error| format!("{}: {error}", path.display())
}

/// Exports `project`, as its folder holds it (saved), as a mod in the
/// folder `to`, in place of an export made there before. A folder that
/// holds something else is refused.
pub fn export(project: &Project, to: &Path) -> Result<Exported, String> {
    let empty = std::fs::read_dir(to).map_or(true, |mut entries| entries.next().is_none());
    if !empty && !to.join(README).is_file() {
        return Err(format!(
            "{} holds other files: name an empty folder, or one a mod was exported to before",
            to.display()
        ));
    }
    if to.exists() {
        std::fs::remove_dir_all(to).map_err(failed(to))?;
    }
    let overrides = to.join("override");
    std::fs::create_dir_all(&overrides).map_err(failed(&overrides))?;
    let mut exported = Exported {
        game_files: 0,
        kjk_files: 0,
    };

    // What the games read: every file at the top of the project's folder
    // but the project's own notes (its `.toml` files, the scripts' debug
    // notes, a file half written).
    let kept_out = ["toml", "ndb", "new"];
    for entry in std::fs::read_dir(&project.folder)
        .map_err(failed(&project.folder))?
        .flatten()
    {
        let path = entry.path();
        let extension = (path.extension().and_then(|extension| extension.to_str()))
            .map_or_else(String::new, str::to_ascii_lowercase);
        if !path.is_file() || kept_out.contains(&extension.as_str()) {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let target = overrides.join(name.to_ascii_lowercase());
        std::fs::copy(&path, &target).map_err(failed(&target))?;
        exported.game_files += 1;
    }
    // A new map's module.
    let modules = project.folder.join("modules");
    for entry in std::fs::read_dir(&modules).into_iter().flatten().flatten() {
        let path = entry.path();
        let is_module = (path.extension().and_then(|extension| extension.to_str()))
            .is_some_and(|extension| extension.eq_ignore_ascii_case("mod"));
        if !path.is_file() || !is_module {
            continue;
        }
        let Some(name) = path.file_name() else {
            continue;
        };
        let folder = to.join("modules");
        std::fs::create_dir_all(&folder).map_err(failed(&folder))?;
        let target = folder.join(name.to_ascii_lowercase());
        std::fs::copy(&path, &target).map_err(failed(&target))?;
        exported.game_files += 1;
    }

    // What only kjk reads.
    for (module, area) in project.level_list() {
        let Some(kjk) = project.kjk_of(&module) else {
            continue;
        };
        // A new map's lights and lamps are its own. An installed level's
        // are, once the project changed them; until then they are the
        // light data's and stay out.
        let mut kjk = kjk.clone();
        if project.map.is_some() {
            kjk.own_lights = true;
        } else if !kjk.own_lights {
            kjk.lights.clear();
            kjk.lamps.clear();
        }
        if kjk == KjkArea::default() {
            continue;
        }
        let text = format!(
            "# A level of the mod {}, for kjk (docs/LEVEL-EDIT.md, \"Sharing a level\").\n\
             module = {module:?}\narea = {area:?}\n\n{}",
            project.name,
            kjk.write()
        );
        let target = overrides.join(area_file(&area));
        std::fs::write(&target, text).map_err(failed(&target))?;
        exported.kjk_files += 1;
        if let Some(bake) = crate::lighting::kept_bake(&project.folder, &module) {
            let folder = overrides.join(crate::lighting::BAKES).join(&module);
            std::fs::create_dir_all(&folder).map_err(failed(&folder))?;
            for entry in std::fs::read_dir(&bake).map_err(failed(&bake))?.flatten() {
                let path = entry.path();
                let Some(name) = path.file_name().filter(|_| path.is_file()) else {
                    continue;
                };
                let target = folder.join(name.to_ascii_lowercase());
                std::fs::copy(&path, &target).map_err(failed(&target))?;
                exported.kjk_files += 1;
            }
        }
    }
    let own = OwnLighting::read(&project.folder)?;
    if !own.is_empty() {
        let target = overrides.join(lighting_file(&project.name));
        std::fs::write(&target, own.write()).map_err(failed(&target))?;
        exported.kjk_files += 1;
    }

    let game = match project.game.as_str() {
        "tsl" => "Knights of the Old Republic II",
        _ => "Knights of the Old Republic",
    };
    let readme = format!(
        "{name}, a mod for Star Wars: {game}\n\
         Made with kjk's level editor.\n\
         \n\
         To install it, copy the folders beside this file into the game's own\n\
         folder, so that the files of override here go into the game's override\n\
         folder{modules}.\n\
         \n\
         It needs the game. It holds changed copies of some of the game's own\n\
         files, as mods of this game do.\n\
         \n\
         The original game reads {game_files} of its files. The other {kjk_files} are for kjk\n\
         alone, and the original game ignores them: the files named kjk-area and\n\
         kjk-lighting, and the folder kjk-bake. They hold what only kjk draws:\n\
         collision boxes, Jedi Academy props, lights, lamps and baked light.\n\
         kjk reads them when it plays without --project.\n\
         \n\
         To remove the mod, delete the files it added.\n",
        name = project.name,
        modules = if project.map.is_some() {
            ", and the files of modules into its modules folder"
        } else {
            ""
        },
        game_files = exported.game_files,
        kjk_files = exported.kjk_files,
    );
    let target = to.join(README);
    std::fs::write(&target, readme).map_err(failed(&target))?;
    Ok(exported)
}
