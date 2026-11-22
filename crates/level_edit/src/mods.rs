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
//!   - `kjk-journal-<project>.jrl`: the project's own quests
//!     (`journal::own_quests`). The game's journal, `global.jrl`, is one
//!     file for the whole game, so a mod does not bring a copy of it: kjk
//!     adds every mod's quests to the game's journal when it loads.
//!   - `kjk-mod-<project>.toml`: the mod's name, its version, its levels,
//!     its quests and every file it brings, each with its size and its
//!     digest (`Manifest`).
//!
//! kjk reads the kjk-only files of every mod in `override/` when it runs
//! without `--project` (`active::activate_mods`). Two mods that change the
//! same area take each other's place, as their `<area>.git` files do. kjk
//! says so at its start: a file two mods list, a file of a mod that is
//! missing or was replaced, and a quest two mods bring (`read`).

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

/// The file of a mod's own quests in `override/`.
pub fn journal_file(name: &str) -> String {
    format!("kjk-journal-{}.jrl", name.to_ascii_lowercase())
}

/// The file that says what a mod is and what it brings, in `override/`.
pub fn manifest_file(name: &str) -> String {
    format!("kjk-mod-{}.toml", name.to_ascii_lowercase())
}

/// What a mod says of itself (`manifest_file`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    /// The levels it changes or makes, by module.
    pub levels: Vec<String>,
    /// The tags of the quests it brings.
    pub quests: Vec<String>,
    /// Every file it brings: its lowercase path from the game's folder
    /// (`override/kje_item.uti`), its size in bytes and its BLAKE3 digest
    /// in hex.
    pub files: Vec<(String, u64, String)>,
}

impl Manifest {
    fn write(&self) -> String {
        let list = |names: &[String]| {
            let quoted: Vec<String> = names.iter().map(|name| format!("{name:?}")).collect();
            quoted.join(", ")
        };
        let mut text = format!(
            "# The mod {}, made with kjk's level editor (docs/LEVEL-EDIT.md, \"Sharing a level\").\n\
             # To remove the mod, delete the files listed here, then this file.\n\
             name = {:?}\nversion = {:?}\nlevels = [{}]\nquests = [{}]\n",
            self.name,
            self.name,
            self.version,
            list(&self.levels),
            list(&self.quests)
        );
        for (path, size, hash) in &self.files {
            text.push_str(&format!(
                "\n[[file]]\npath = {path:?}\nsize = {size}\nhash = {hash:?}\n"
            ));
        }
        text
    }

    fn parse(text: &str) -> Result<Manifest, String> {
        let document: toml_edit::DocumentMut = text.parse().map_err(|error| format!("{error}"))?;
        let word = |key: &str| {
            (document.get(key).and_then(toml_edit::Item::as_str))
                .map_or_else(String::new, str::to_string)
        };
        let words = |key: &str| {
            (document.get(key).and_then(toml_edit::Item::as_array)).map_or_else(Vec::new, |list| {
                list.iter()
                    .filter_map(|item| item.as_str().map(str::to_ascii_lowercase))
                    .collect()
            })
        };
        let name = word("name");
        if name.is_empty() {
            return Err("it names no mod".into());
        }
        let files = (document.get("file"))
            .and_then(toml_edit::Item::as_array_of_tables)
            .map_or_else(Vec::new, |tables| {
                tables
                    .iter()
                    .filter_map(|table| {
                        let path = table.get("path")?.as_str()?.to_ascii_lowercase();
                        let size = table.get("size")?.as_integer()?;
                        let hash = table.get("hash")?.as_str()?.to_string();
                        Some((path, u64::try_from(size).ok()?, hash))
                    })
                    .collect()
            });
        Ok(Manifest {
            name,
            version: word("version"),
            levels: words("levels"),
            quests: words("quests"),
            files,
        })
    }
}

/// The largest file whose digest is checked when kjk starts. A larger one
/// (a new map's module) is checked by its size alone, so a start stays
/// quick.
const CHECKED_BYTES: u64 = 1 << 20;

fn digest_of(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(blake3::hash(&bytes).to_hex().to_string())
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
    /// The mods' own quests (`journal_file`), each file's journal, in the
    /// files' order.
    pub journals: Vec<fmt_odyssey::gff::Gff>,
    /// What each mod says of itself (`manifest_file`).
    pub mods: Vec<Manifest>,
    /// A digest of what the mods change of the world that the games' own
    /// files do not hold: the levels' kjk-only content and the quests.
    /// Two machines that play together compare it.
    pub world: Option<[u8; 32]>,
    /// The files that did not read, each with why (such a file is left
    /// out), and what two mods both change.
    pub notes: Vec<String>,
}

impl Found {
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
            && self.lighting.is_empty()
            && self.bakes.is_empty()
            && self.journals.is_empty()
            && self.mods.is_empty()
    }
}

/// Reads the kjk-only files among `files`: each file of an `override/` by
/// its lowercase path inside it (`/` between folders) and where it is.
pub fn read(files: &[(String, PathBuf)]) -> Found {
    let mut found = Found::default();
    let mut world = blake3::Hasher::new();
    let mut of_world = false;
    let mut hash_in = |inside: &str, path: &Path| {
        if let Ok(bytes) = std::fs::read(path) {
            for part in [inside.as_bytes(), &bytes] {
                world.update(&(part.len() as u64).to_le_bytes());
                world.update(part);
            }
            of_world = true;
        }
    };
    for (inside, path) in files {
        let top = !inside.contains('/');
        if top && inside.starts_with("kjk-area-") && inside.ends_with(".toml") {
            match read_area(path) {
                Ok(level) => {
                    found.levels.push(level);
                    hash_in(inside, path);
                }
                Err(why) => found.notes.push(format!("{inside}: {why}")),
            }
        } else if top && inside.starts_with("kjk-journal-") && inside.ends_with(".jrl") {
            let read = std::fs::read(path)
                .map_err(|error| error.to_string())
                .and_then(|bytes| {
                    fmt_odyssey::gff::Gff::parse(&bytes).map_err(|error| error.to_string())
                });
            match read {
                Ok(journal) => {
                    found.journals.push(journal);
                    hash_in(inside, path);
                }
                Err(why) => found.notes.push(format!("{inside}: {why}")),
            }
        } else if top && inside.starts_with("kjk-mod-") && inside.ends_with(".toml") {
            let read = std::fs::read_to_string(path)
                .map_err(|error| error.to_string())
                .and_then(|text| Manifest::parse(&text));
            match read {
                Ok(manifest) => found.mods.push(manifest),
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
    found.world = of_world.then(|| *world.finalize().as_bytes());
    let clashes = clashes(&found, files);
    found.notes.extend(clashes);
    found
}

/// What the mods found both change, and what of a mod is missing or was
/// replaced, as notes for the person: the games read one file of a name,
/// so the mod copied last wins without a word.
fn clashes(found: &Found, files: &[(String, PathBuf)]) -> Vec<String> {
    let mut notes = Vec::new();
    // A file two mods list.
    let mut owner: BTreeMap<&str, &str> = BTreeMap::new();
    let mut shared: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for manifest in &found.mods {
        for (path, ..) in &manifest.files {
            match owner.get(path.as_str()) {
                Some(first) if *first != manifest.name => {
                    (shared.entry((first, &manifest.name)).or_default()).push(path);
                }
                Some(_) => {}
                None => {
                    owner.insert(path, &manifest.name);
                }
            }
        }
    }
    for ((first, second), paths) in shared {
        let shown: Vec<&str> = paths.iter().take(6).copied().collect();
        let more = match paths.len().saturating_sub(shown.len()) {
            0 => String::new(),
            more => format!(" and {more} more"),
        };
        notes.push(format!(
            "the mods {first} and {second} both bring {}{more}: the game reads the copy that was installed last, so one of the two mods is not whole",
            shown.join(", ")
        ));
    }
    // A file of a mod that is not there, or is not the one the mod brought.
    for manifest in &found.mods {
        let (mut missing, mut replaced) = (Vec::new(), Vec::new());
        for (path, size, hash) in &manifest.files {
            let Some(inside) = path.strip_prefix("override/") else {
                continue;
            };
            let Some((_, on_disk)) = files.iter().find(|(had, _)| had == inside) else {
                missing.push(path.as_str());
                continue;
            };
            let length = std::fs::metadata(on_disk).map_or(0, |meta| meta.len());
            let same = length == *size
                && (length > CHECKED_BYTES || digest_of(on_disk).is_ok_and(|now| now == *hash));
            if !same {
                replaced.push(path.as_str());
            }
        }
        for (what, paths) in [
            ("are missing", missing),
            (
                "are not the ones it brought (another mod, or a later change, replaced them)",
                replaced,
            ),
        ] {
            if paths.is_empty() {
                continue;
            }
            let shown: Vec<&str> = paths.iter().take(6).copied().collect();
            let more = match paths.len().saturating_sub(shown.len()) {
                0 => String::new(),
                more => format!(" and {more} more"),
            };
            notes.push(format!(
                "the mod {} {} has files that {what}: {}{more}",
                manifest.name,
                manifest.version,
                shown.join(", ")
            ));
        }
    }
    // A quest two mods bring.
    let mut merged = crate::journal::empty();
    for journal in &found.journals {
        for tag in crate::journal::merge(&mut merged, journal) {
            notes.push(format!(
                "two mods bring a quest tagged {tag}: the first one read is kept"
            ));
        }
    }
    notes
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
    let journal = format!("{}.jrl", crate::journal::NAME);
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
        // The journal is one file for the whole game: the mod brings its
        // own quests in a file of its own, below.
        if name.to_string_lossy().eq_ignore_ascii_case(&journal) {
            continue;
        }
        let target = overrides.join(name.to_ascii_lowercase());
        std::fs::copy(&path, &target).map_err(failed(&target))?;
        exported.game_files += 1;
    }
    let quests = (project.templates.get(crate::journal::NAME))
        .and_then(|made| crate::journal::own_quests(&made.gff));
    let quest_tags = (quests.as_ref()).map_or_else(Vec::new, crate::journal::tags);
    if let Some(quests) = &quests {
        let target = overrides.join(journal_file(&project.name));
        std::fs::write(&target, quests.write()).map_err(failed(&target))?;
        exported.kjk_files += 1;
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
        if project.is_map(&module) {
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

    // What the mod is and every file it brings, for telling two mods apart
    // and for taking one out again.
    let mut manifest = Manifest {
        name: project.name.clone(),
        version: project.version.clone(),
        levels: (project.level_list().into_iter())
            .map(|(module, _)| module)
            .collect(),
        quests: quest_tags,
        files: Vec::new(),
    };
    let mut folders = vec![(String::new(), to.to_path_buf())];
    while let Some((inside, at)) = folders.pop() {
        for entry in std::fs::read_dir(&at).map_err(failed(&at))?.flatten() {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let inside = if inside.is_empty() {
                name
            } else {
                format!("{inside}/{name}")
            };
            let path = entry.path();
            if path.is_dir() {
                folders.push((inside, path));
            } else {
                let size = std::fs::metadata(&path).map_err(failed(&path))?.len();
                manifest.files.push((inside, size, digest_of(&path)?));
            }
        }
    }
    manifest.files.sort();
    let target = overrides.join(manifest_file(&project.name));
    std::fs::write(&target, manifest.write()).map_err(failed(&target))?;
    exported.kjk_files += 1;

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
         alone, and the original game ignores them: the files named kjk-area,\n\
         kjk-lighting, kjk-journal and kjk-mod, and the folder kjk-bake. They\n\
         hold what only kjk plays: collision boxes, Jedi Academy props, lights,\n\
         lamps, baked light and the mod's quests. kjk reads them when it plays\n\
         without --project. The original game does not show the mod's quests\n\
         in its journal.\n\
         \n\
         This is version {version}. To remove the mod, delete the files that\n\
         {manifest} in the override folder lists, then that file.\n",
        version = project.version,
        manifest = manifest_file(&project.name),
        name = project.name,
        modules = if (project.level_list().iter()).any(|(module, _)| project.is_map(module)) {
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
