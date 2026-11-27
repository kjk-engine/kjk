//! A project's folder: `project.toml`, the area's `.git` file,
//! `kjk-area.toml` and the templates the project brings, read and written.
//!
//! A project of the install may edit several levels (`Project::levels`).
//! Every module of KOTOR holds one area, so a level is a module and its
//! area. The project's first level, its home, is the one `project.toml`
//! names with `module` and `area`, and its kjk-only content is
//! `kjk-area.toml`. Each further level is listed in `levels` as
//! `[module, area]`, and its kjk-only content is `kjk-area-<area>.toml`.
//! Every level's GIT is `<area>.git` and its changed ARE `<area>.are`,
//! which the project's layer serves wherever that area is loaded. The
//! editor edits one level at a time (the project's `module`, `area`,
//! `git`, `kjk`, `are` and `map`) and parks the others in `levels`.
//!
//! A level is one of the install's, or a new map the project makes
//! (`map`). A project may hold both kinds, and several new maps: an
//! adventure of new maps reached from a level of the game. A new map's
//! own file is `kjk-map.toml` for the home level and
//! `kjk-map-<area>.toml` for another, and its module is written to
//! `modules/<module>.mod`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;

use crate::area::{KjkArea, SCALES, SMALLEST_SIDE};
use crate::git::{self, Kind, RESREF_LEN, TAG_LEN};
use crate::map::{MAP_FILE, NewMap};
use crate::template::{self, Template};

/// The file that says what a project edits.
pub const PROJECT_FILE: &str = "project.toml";
/// The file of what only kjk understands (`area`).
pub const AREA_FILE: &str = "kjk-area.toml";

/// A project, as its folder holds it or as the editor has changed it.
#[derive(Debug, Clone, PartialEq)]
pub struct Project {
    /// The folder's name.
    pub name: String,
    /// The version an exported mod says it is (`mods`), as `project.toml`
    /// keeps it: any short text, `1` until it is set.
    pub version: String,
    pub folder: PathBuf,
    /// `k1` or `tsl`.
    pub game: String,
    /// The module whose area it changes, lowercase (`ebo_m12aa`).
    pub module: String,
    /// The area's resref, lowercase (`m12aa`): the name of its `.git`.
    pub area: String,
    pub git: Gff,
    pub kjk: KjkArea,
    /// The placeable and creature templates the project brings, by resref
    /// (`template`): each a file of the folder, `RESREF.utp` or `.utc`.
    pub templates: BTreeMap<String, Template>,
    /// The map the level edited now is, when its module is the project's
    /// own (`map`). `None` for a level of the install.
    pub map: Option<NewMap>,
    /// The kinds of light fitting changed in the editor (`kinds`). The
    /// editor keeps them in the project's `kjk-lighting.toml` (`lighting`),
    /// which this type neither reads nor writes. Before, they were saved to
    /// the light data, never to the project's folder.
    pub kinds: Vec<crate::kinds::KindEdit>,
    /// The level each room of the bake aims at (`levels`), as the editor
    /// has it. The editor keeps a level's changed ones in the project's
    /// `kjk-lighting.toml` (`lighting`), and Publish writes them to the
    /// light data's levels file.
    pub levels: BTreeMap<String, f32>,
    /// Other areas of the install whose doors and triggers the project
    /// leads elsewhere (into its own new map, most often), by area resref:
    /// each a copy of that area's GIT, saved as `<area>.git` in the folder,
    /// which the project's layer serves wherever that area is loaded.
    pub others: BTreeMap<String, OtherArea>,
    /// The project's own area's ARE as the project changes it (music is the
    /// GIT's): fog, ambient light and the area's scripts. `None` keeps the
    /// install's. A new map's ARE is its map's (`NewMap`).
    pub are: Option<Gff>,
    /// The project's conversations by name (`dialog`): its own, and its
    /// copies of the game's, each `NAME.dlg` in the folder, which the
    /// project's layer serves in place of the game's of that name.
    pub conversations: BTreeMap<String, Gff>,
    /// The project's first level, as `project.toml` names it: (module,
    /// area). The level edited now is `module` and `area`.
    pub home: (String, String),
    /// The project's other levels of the install, by module: every level
    /// it edits but the one edited now.
    pub parked: BTreeMap<String, Level>,
}

/// A level of the install that a project edits, while another is edited
/// (`Project::levels`): its area, its GIT, its kjk-only content and its
/// changed ARE. A new map the project makes has its map here, and its ARE
/// is its map's.
#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub area: String,
    pub git: Gff,
    pub kjk: KjkArea,
    pub are: Option<Gff>,
    pub map: Option<NewMap>,
}

/// The file of a new map's own content: `kjk-map.toml` for the home
/// level's area, `kjk-map-<area>.toml` for another's.
pub fn map_file(home_area: &str, area: &str) -> String {
    if home_area.eq_ignore_ascii_case(area) {
        MAP_FILE.to_string()
    } else {
        format!("kjk-map-{}.toml", area.to_ascii_lowercase())
    }
}

/// The file of a level's kjk-only content: `kjk-area.toml` for the home
/// level's area, `kjk-area-<area>.toml` for another's.
pub fn kjk_file(home_area: &str, area: &str) -> String {
    if home_area.eq_ignore_ascii_case(area) {
        AREA_FILE.to_string()
    } else {
        format!("kjk-area-{}.toml", area.to_ascii_lowercase())
    }
}

/// The further levels `project.toml` lists (`levels`): (module, area),
/// each lowercase.
fn levels_of(document: &toml_edit::DocumentMut) -> Vec<(String, String)> {
    pairs_of(document, "levels")
}

fn pairs_of(document: &toml_edit::DocumentMut, key: &str) -> Vec<(String, String)> {
    let Some(list) = document.get(key).and_then(toml_edit::Item::as_array) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|pair| {
            let pair = pair.as_array()?;
            let module = pair.get(0)?.as_str()?.to_ascii_lowercase();
            let area = pair.get(1)?.as_str()?.to_ascii_lowercase();
            Some((module, area))
        })
        .collect()
}

/// An area of another module whose doors and triggers the project changes
/// (`Project::others`): its module and its GIT as the project has it.
#[derive(Debug, Clone, PartialEq)]
pub struct OtherArea {
    pub module: String,
    pub git: Gff,
}

/// Something wrong with a project. A problem that `blocks` stops the save;
/// the others are warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub blocks: bool,
    pub what: String,
}

fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
}

/// A string field of `project.toml`.
/// The other areas `project.toml` lists (`others`): (module, area), each
/// lowercase.
fn others_of(document: &toml_edit::DocumentMut) -> Vec<(String, String)> {
    pairs_of(document, "others")
}

fn field(document: &toml_edit::DocumentMut, key: &str) -> Result<String, String> {
    document
        .get(key)
        .and_then(toml_edit::Item::as_str)
        .map(str::to_ascii_lowercase)
        .ok_or(format!("{PROJECT_FILE}: {key} is missing"))
}

impl Project {
    /// A new project of `module`'s area `area`, starting from the area's
    /// own GIT (`git`) and with no kjk-only content. Nothing is written
    /// until `save`.
    pub fn new(folder: &Path, game: &str, module: &str, area: &str, git: Gff) -> Project {
        Project {
            name: folder
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(String::new),
            version: "1".to_string(),
            folder: folder.to_path_buf(),
            game: game.to_ascii_lowercase(),
            module: module.to_ascii_lowercase(),
            area: area.to_ascii_lowercase(),
            git,
            kjk: KjkArea::default(),
            templates: BTreeMap::new(),
            map: None,
            kinds: Vec::new(),
            levels: BTreeMap::new(),
            others: BTreeMap::new(),
            are: None,
            conversations: BTreeMap::new(),
            home: (module.to_ascii_lowercase(), area.to_ascii_lowercase()),
            parked: BTreeMap::new(),
        }
    }

    /// Every level the project edits, by module, with its area: the one
    /// edited now and the parked ones, the home level first.
    pub fn level_list(&self) -> Vec<(String, String)> {
        let mut list: Vec<(String, String)> =
            std::iter::once((self.module.clone(), self.area.clone()))
                .chain(
                    self.parked
                        .iter()
                        .map(|(module, level)| (module.clone(), level.area.clone())),
                )
                .collect();
        list.sort_by_key(|(module, _)| (*module != self.home.0, module.clone()));
        list
    }

    /// The kjk-only content of the project's level `module`: the one edited
    /// now, else a parked one.
    pub fn kjk_of(&self, module: &str) -> Option<&KjkArea> {
        if self.module.eq_ignore_ascii_case(module) {
            return Some(&self.kjk);
        }
        self.parked
            .get(&module.to_ascii_lowercase())
            .map(|level| &level.kjk)
    }

    /// The new map of the project's level `module`, if it is one.
    pub fn map_of(&self, module: &str) -> Option<&NewMap> {
        if self.module.eq_ignore_ascii_case(module) {
            return self.map.as_ref();
        }
        (self.parked.get(&module.to_ascii_lowercase())).and_then(|level| level.map.as_ref())
    }

    /// Whether the project's level `module` is a new map it makes.
    pub fn is_map(&self, module: &str) -> bool {
        if self.module.eq_ignore_ascii_case(module) {
            return self.map.is_some();
        }
        (self.parked.get(&module.to_ascii_lowercase())).is_some_and(|level| level.map.is_some())
    }

    /// Adds a new map to the project as the level `module` and edits it
    /// now, parking the level edited before. The map starts as
    /// `Project::new_map` makes one.
    pub fn add_map(
        &mut self,
        module: &str,
        title: &str,
        (base, are, base_git): (&str, Gff, &Gff),
    ) -> Result<(), String> {
        let module = module.to_ascii_lowercase();
        if let Some(problem) = crate::map::name_problem(&module) {
            return Err(problem);
        }
        if self.level_list().iter().any(|(had, _)| *had == module) {
            return Err(format!("{} has a level {module} already", self.name));
        }
        if self.others.contains_key(&module) || self.templates.contains_key(&module) {
            return Err(format!("{} has a file named {module} already", self.name));
        }
        let made = Project::new_map(
            &self.folder,
            &self.game,
            &module,
            title,
            (base, are, base_git),
        )?;
        self.parked.insert(
            module.clone(),
            Level {
                area: made.area,
                git: made.git,
                kjk: KjkArea::default(),
                are: None,
                map: made.map,
            },
        );
        self.switch_to(&module, || Err("the map was just added".into()))
    }

    /// Edits the level `module` now, parking the one edited before. A
    /// level the project does not have yet is added, made from the
    /// install's area that `fresh` gives (its area and GIT). A project's
    /// copy of that area's doors (`others`) becomes the new level's GIT.
    pub fn switch_to(
        &mut self,
        module: &str,
        fresh: impl FnOnce() -> Result<(String, Gff), String>,
    ) -> Result<(), String> {
        let module = module.to_ascii_lowercase();
        if module == self.module {
            return Ok(());
        }
        let level = match self.parked.remove(&module) {
            Some(level) => level,
            None => {
                let (area, git) = fresh()?;
                let area = area.to_ascii_lowercase();
                let git = match self.others.remove(&area) {
                    Some(other) if other.module == module => other.git,
                    Some(other) => {
                        self.others.insert(area.clone(), other);
                        git
                    }
                    None => git,
                };
                Level {
                    area,
                    git,
                    kjk: KjkArea::default(),
                    are: None,
                    map: None,
                }
            }
        };
        let parked = Level {
            area: std::mem::replace(&mut self.area, level.area),
            git: std::mem::replace(&mut self.git, level.git),
            kjk: std::mem::replace(&mut self.kjk, level.kjk),
            are: std::mem::replace(&mut self.are, level.are),
            map: std::mem::replace(&mut self.map, level.map),
        };
        let was = std::mem::replace(&mut self.module, module);
        self.parked.insert(was, parked);
        Ok(())
    }

    /// A project that makes a new map: the module `module`, whose one area
    /// has the module's name, with no rooms and no objects. Its area starts
    /// from `base`'s area: its ARE (`are`) and the GIT's area properties
    /// (`base_git`: music and ambient sound).
    pub fn new_map(
        folder: &Path,
        game: &str,
        module: &str,
        title: &str,
        (base, are, base_git): (&str, Gff, &Gff),
    ) -> Result<Project, String> {
        if let Some(problem) = crate::map::name_problem(module) {
            return Err(problem);
        }
        let mut git = git::empty();
        if let Some(properties) = base_git.root.get("AreaProperties") {
            git.root.fields.push(fmt_odyssey::gff::GffField {
                label: b"AreaProperties".to_vec(),
                value: properties.clone(),
            });
        }
        let mut project = Project::new(folder, game, module, module, git);
        project.map = Some(NewMap::new(title, base, module, are));
        Ok(project)
    }

    /// Where the project's own module is written: `modules/<module>.mod`
    /// in its folder.
    pub fn module_path(&self) -> PathBuf {
        self.folder
            .join("modules")
            .join(format!("{}.mod", self.module))
    }

    /// What the mods in `folder` (an install's `override/`) hold for kjk
    /// (`mods::read`), as a project of `game` to play: each of `levels`
    /// (its module, its area and its kjk-only content) is one of its
    /// levels. It holds no object list of its own: the games read the mods'
    /// `<area>.git` files from `override/` as they read any mod's.
    pub fn of_mods(folder: &Path, game: &str, levels: Vec<(String, String, KjkArea)>) -> Project {
        let empty = || Gff {
            file_type: *b"GIT ",
            root: fmt_odyssey::gff::GffStruct {
                id: 0,
                fields: Vec::new(),
            },
        };
        let mut levels = levels.into_iter();
        let (module, area, kjk) =
            (levels.next()).unwrap_or_else(|| (String::new(), String::new(), KjkArea::default()));
        let mut project = Project::new(folder, game, &module, &area, empty());
        project.name = "mods".to_string();
        project.kjk = kjk;
        for (module, area, kjk) in levels {
            // Of two mods' content for one level the first read is kept,
            // and `mods::read` tells the person of the other.
            let module = module.to_ascii_lowercase();
            if module == project.module || project.parked.contains_key(&module) {
                continue;
            }
            project.parked.insert(
                module,
                Level {
                    area: area.to_ascii_lowercase(),
                    git: empty(),
                    kjk,
                    are: None,
                    map: None,
                },
            );
        }
        project
    }

    /// Whether `folder` holds a project.
    pub fn exists(folder: &Path) -> bool {
        folder.join(PROJECT_FILE).is_file()
    }

    /// The project `folder` holds. Its `project.toml` and its area's GIT
    /// must read. A missing `kjk-area.toml` is an area with no kjk-only
    /// content.
    pub fn open(folder: &Path) -> Result<Project, String> {
        let document: toml_edit::DocumentMut = read_text(&folder.join(PROJECT_FILE))?
            .parse()
            .map_err(|error| format!("{PROJECT_FILE}: {error}"))?;
        let game = field(&document, "game")?;
        let module = field(&document, "module")?;
        let area = field(&document, "area")?;
        // Every name the file gives is checked before any becomes part of
        // a path. One with `..` or a separator named a file outside the
        // folder, which was read here, and deleted by a save or a bake.
        let named = [(module.clone(), area.clone())]
            .into_iter()
            .chain(levels_of(&document))
            .chain(others_of(&document));
        for (level, its_area) in named {
            for name in [level, its_area] {
                if let Some(why) = crate::file_name_problem(&name) {
                    return Err(format!("{PROJECT_FILE}: {why}"));
                }
            }
        }
        let git_path = folder.join(format!("{area}.git"));
        let bytes =
            std::fs::read(&git_path).map_err(|error| format!("{}: {error}", git_path.display()))?;
        let git = Gff::parse(&bytes).map_err(|error| format!("{area}.git: {error}"))?;
        let area_path = folder.join(AREA_FILE);
        let kjk = if area_path.is_file() {
            KjkArea::parse(&read_text(&area_path)?)?
        } else {
            KjkArea::default()
        };
        let mut project = Project::new(folder, &game, &module, &area, git);
        if let Some(version) = document.get("version").and_then(toml_edit::Item::as_str) {
            project.version = version.to_string();
        }
        project.kjk = kjk;
        project.templates = read_templates(folder)?;
        let map_path = folder.join(MAP_FILE);
        if map_path.is_file() {
            let are_path = folder.join(format!("{area}.are"));
            let bytes = std::fs::read(&are_path)
                .map_err(|error| format!("{}: {error}", are_path.display()))?;
            let are = Gff::parse(&bytes).map_err(|error| format!("{area}.are: {error}"))?;
            project.map = Some(NewMap::parse(&read_text(&map_path)?, are)?);
        } else {
            let are_path = folder.join(format!("{area}.are"));
            if are_path.is_file() {
                let bytes = std::fs::read(&are_path)
                    .map_err(|error| format!("{}: {error}", are_path.display()))?;
                project.are =
                    Some(Gff::parse(&bytes).map_err(|error| format!("{area}.are: {error}"))?);
            }
        }
        for (name, path) in dialog_files(folder) {
            let bytes =
                std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let gff = Gff::parse(&bytes).map_err(|error| format!("{name}.dlg: {error}"))?;
            project.conversations.insert(name, gff);
        }
        for (module, area) in levels_of(&document) {
            let read_gff = |name: String| -> Result<Option<Gff>, String> {
                let path = folder.join(&name);
                if !path.is_file() {
                    return Ok(None);
                }
                let bytes =
                    std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
                Gff::parse(&bytes)
                    .map(Some)
                    .map_err(|error| format!("{name}: {error}"))
            };
            let git = read_gff(format!("{area}.git"))?
                .ok_or(format!("the level {module}'s {area}.git is missing"))?;
            let mut are = read_gff(format!("{area}.are"))?;
            // A further level that is a new map: its ARE is its map's.
            let map_path = folder.join(map_file(&project.home.1, &area));
            let map = if map_path.is_file() {
                let are = are
                    .take()
                    .ok_or(format!("the map {module}'s {area}.are is missing"))?;
                Some(NewMap::parse(&read_text(&map_path)?, are)?)
            } else {
                None
            };
            let kjk_path = folder.join(kjk_file(&project.home.1, &area));
            let kjk = if kjk_path.is_file() {
                KjkArea::parse(&read_text(&kjk_path)?)?
            } else {
                KjkArea::default()
            };
            project.parked.insert(
                module,
                Level {
                    area,
                    git,
                    kjk,
                    are,
                    map,
                },
            );
        }
        for (module, other) in others_of(&document) {
            let path = folder.join(format!("{other}.git"));
            let bytes =
                std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            let git = Gff::parse(&bytes).map_err(|error| format!("{other}.git: {error}"))?;
            project.others.insert(other, OtherArea { module, git });
        }
        Ok(project)
    }

    /// What is wrong with the project. A creature or placeable without a
    /// template, a template name longer than a resref, a tag the games
    /// would cut, a prop without a model or at a scale out of
    /// `area::SCALES`, a box with a side under `area::SMALLEST_SIDE`, and
    /// two props or boxes of one name block the save.
    pub fn problems(&self) -> Vec<Problem> {
        let mut problems = Vec::new();
        let mut block = |what: String| problems.push(Problem { blocks: true, what });
        // Every level is checked alike: the one being edited, and each
        // parked one, whose problems name it. Only the level being edited
        // was, so a parked level with a fault was saved without a word.
        let edited = (
            self.module.as_str(),
            &self.git,
            &self.kjk,
            self.map.as_ref(),
        );
        let parked = (self.parked.iter())
            .map(|(module, level)| (module.as_str(), &level.git, &level.kjk, level.map.as_ref()));
        for (module, git, kjk, map) in std::iter::once(edited).chain(parked) {
            let of_level = if module == self.module {
                String::new()
            } else {
                format!("the level {module}: ")
            };
            for what in level_problems(module, git, kjk, map) {
                block(format!("{of_level}{what}"));
            }
        }
        if self.others.contains_key(&self.area) {
            block(format!(
                "{} is the project's own area; its doors are changed in its own GIT",
                self.area
            ));
        }
        for level in self.parked.values() {
            if self.others.contains_key(&level.area) {
                block(format!(
                    "{} is one of the project's levels; its doors are changed in its own GIT",
                    level.area
                ));
            }
        }
        for (name, conversation) in &self.conversations {
            for problem in crate::dialog::problems(conversation) {
                problems.push(Problem {
                    blocks: false,
                    what: format!("the conversation {name}: {problem}"),
                });
            }
        }
        problems
    }

    /// What is wrong with a name of the project that becomes part of a
    /// path, if any is (`file_name_problem`): its levels' and their areas',
    /// and the other areas it changes with their modules.
    pub fn unsafe_name(&self) -> Option<String> {
        let levels = self.level_list();
        let others = (self.others.iter()).map(|(area, other)| (other.module.clone(), area.clone()));
        (levels.into_iter().chain(others))
            .flat_map(|(module, area)| [module, area])
            .find_map(|name| crate::file_name_problem(&name))
    }

    /// Writes the folder: `project.toml`, the area's `.git`, and
    /// `kjk-area.toml`. Refused, with nothing written, while a problem
    /// blocks it. `made_by` names the kjk that wrote it (its commit).
    /// Returns the files written.
    pub fn save(&self, made_by: &str) -> Result<Vec<PathBuf>, String> {
        if let Some(why) = self.unsafe_name() {
            return Err(format!("not saved: {why}"));
        }
        let blocking: Vec<String> = self
            .problems()
            .into_iter()
            .filter(|problem| problem.blocks)
            .map(|problem| problem.what)
            .collect();
        if !blocking.is_empty() {
            return Err(format!("not saved: {}", blocking.join("; ")));
        }
        std::fs::create_dir_all(&self.folder)
            .map_err(|error| format!("{}: {error}", self.folder.display()))?;
        let mut project = format!(
            "# A kjk level editor project (docs/LEVEL-EDIT.md).\n\
             game = {}\nmodule = {}\narea = {}\nmade_by = {}\nversion = {}\n",
            crate::quoted(&self.game),
            crate::quoted(&self.home.0),
            crate::quoted(&self.home.1),
            crate::quoted(made_by),
            crate::quoted(&self.version)
        );
        let further: Vec<(String, String)> = self
            .level_list()
            .into_iter()
            .filter(|(module, _)| *module != self.home.0)
            .collect();
        if !further.is_empty() {
            let listed: Vec<String> = further
                .iter()
                .map(|(module, area)| {
                    format!("[{}, {}]", crate::quoted(module), crate::quoted(area))
                })
                .collect();
            project.push_str(&format!(
                "# Further levels it edits: [module, area].\nlevels = [{}]\n",
                listed.join(", ")
            ));
        }
        if !self.others.is_empty() {
            let listed: Vec<String> = self
                .others
                .iter()
                .map(|(area, other)| {
                    format!(
                        "[{}, {}]",
                        crate::quoted(&other.module),
                        crate::quoted(area)
                    )
                })
                .collect();
            project.push_str(&format!(
                "# Other modules' areas whose doors and triggers it changes: [module, area].\n\
                 others = [{}]\n",
                listed.join(", ")
            ));
        }
        // An area no longer changed leaves the folder.
        let before = read_text(&self.folder.join(PROJECT_FILE))
            .ok()
            .and_then(|text| text.parse::<toml_edit::DocumentMut>().ok())
            .map_or_else(Vec::new, |document| others_of(&document));
        for (_, area) in before {
            // The list is the file's on disk, which may not be this
            // program's own. A name that is no file's name deletes nothing.
            if crate::file_name_problem(&area).is_some() {
                continue;
            }
            let path = self.folder.join(format!("{area}.git"));
            if area != self.area && !self.others.contains_key(&area) && path.is_file() {
                std::fs::remove_file(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
            }
        }
        let mut files = vec![
            (PROJECT_FILE.to_string(), project.into_bytes()),
            (format!("{}.git", self.area), self.git.write()),
            (
                kjk_file(&self.home.1, &self.area),
                self.own_kjk().write().into_bytes(),
            ),
        ];
        for (module, level) in &self.parked {
            files.push((format!("{}.git", level.area), level.git.write()));
            files.push((
                kjk_file(&self.home.1, &level.area),
                level_kjk(level).write().into_bytes(),
            ));
            if let Some(map) = &level.map {
                files.push((
                    map_file(&self.home.1, &level.area),
                    map.write().into_bytes(),
                ));
                files.push((
                    format!("{}.are", level.area),
                    map_are_of(module, map).write(),
                ));
            } else if let Some(are) = &level.are {
                files.push((format!("{}.are", level.area), are.write()));
            }
        }
        for (resref, made) in &self.templates {
            files.push((
                format!("{resref}.{}", template::extension(made.restype)),
                made.gff.write(),
            ));
        }
        if let Some(map) = &self.map {
            files.push((map_file(&self.home.1, &self.area), map.write().into_bytes()));
            files.push((format!("{}.are", self.area), self.map_are(map).write()));
        } else if let Some(are) = &self.are {
            files.push((format!("{}.are", self.area), are.write()));
        }
        for (area, other) in &self.others {
            files.push((format!("{area}.git"), other.git.write()));
        }
        for (name, conversation) in &self.conversations {
            files.push((format!("{name}.dlg"), conversation.write()));
        }
        // A conversation taken out of the project leaves the folder.
        for (name, path) in dialog_files(&self.folder) {
            if !self.conversations.contains_key(&name) {
                std::fs::remove_file(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
            }
        }
        // A template no object uses any more (an edit undone) leaves the
        // folder.
        // So does a file of a template's name and another kind: an item
        // taken out, and an object's template then brought in under its
        // name, left the item's file beside the new one.
        for (resref, path) in template_files(&self.folder) {
            let ending = path
                .extension()
                .map(|ending| ending.to_string_lossy().to_ascii_lowercase());
            let kept = self
                .templates
                .get(&resref)
                .is_some_and(|made| ending.as_deref() == Some(template::extension(made.restype)));
            if !kept {
                std::fs::remove_file(&path)
                    .map_err(|error| format!("{}: {error}", path.display()))?;
            }
        }
        let mut written = Vec::new();
        for (name, bytes) in files {
            let path = self.folder.join(name);
            // Written beside, then moved over the old file, so a failed
            // write leaves the old one whole.
            let fresh = path.with_extension("new");
            std::fs::write(&fresh, &bytes)
                .and_then(|()| std::fs::rename(&fresh, &path))
                .map_err(|error| format!("{}: {error}", path.display()))?;
            written.push(path);
        }
        Ok(written)
    }

    /// A digest of what the project changes: its levels' GITs and kjk-only
    /// content, as they are written, its templates, its conversations, the
    /// other areas it changes, and the cuts of its Jedi Academy rooms as
    /// its folder holds them. Play keys the area's baked collision by it,
    /// and machines that play a project together compare it.
    ///
    /// Every level goes in alike, by its module's name, so the digest does
    /// not tell which level is being edited. The level being edited used
    /// to go in apart from the parked ones, and going to another level
    /// changed the digest with nothing changed. Every part has its length
    /// before it. The cuts were left out, so a room cut again kept the
    /// digest, and with it the collision baked for the room as it was.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        let mut part = |bytes: &[u8]| {
            hasher.update(&(bytes.len() as u64).to_le_bytes());
            hasher.update(bytes);
        };
        part(self.game.as_bytes());
        // Each level: its module, its area, its GIT, its kjk-only content,
        // its ARE when it is no new map, and a new map with its ARE.
        type Parts<'a> = (
            &'a str,
            &'a str,
            &'a Gff,
            KjkArea,
            Option<&'a Gff>,
            Option<&'a NewMap>,
        );
        let mut levels: Vec<Parts> = vec![(
            self.module.as_str(),
            self.area.as_str(),
            &self.git,
            self.own_kjk(),
            self.are.as_ref(),
            self.map.as_ref(),
        )];
        levels.extend(self.parked.iter().map(|(module, level)| {
            (
                module.as_str(),
                level.area.as_str(),
                &level.git,
                level_kjk(level),
                level.are.as_ref(),
                level.map.as_ref(),
            )
        }));
        levels.sort_by(|a, b| a.0.cmp(b.0));
        part(&(levels.len() as u64).to_le_bytes());
        for (module, area, git, kjk, are, map) in levels {
            part(module.as_bytes());
            part(area.as_bytes());
            part(&git.write());
            part(kjk.write().as_bytes());
            match map {
                Some(map) => {
                    part(b"map");
                    part(map.write().as_bytes());
                    part(&map_are_of(module, map).write());
                }
                None => {
                    part(b"level");
                    part(&are.map_or_else(Vec::new, Gff::write));
                }
            }
        }
        part(&(self.templates.len() as u64).to_le_bytes());
        for (resref, made) in &self.templates {
            part(resref.as_bytes());
            part(&made.gff.write());
        }
        part(&(self.conversations.len() as u64).to_le_bytes());
        for (name, conversation) in &self.conversations {
            part(name.as_bytes());
            part(&conversation.write());
        }
        part(&(self.others.len() as u64).to_le_bytes());
        for (area, other) in &self.others {
            part(area.as_bytes());
            part(other.module.as_bytes());
            part(&other.git.write());
        }
        // The cuts of the Jedi Academy rooms, each file by its name.
        let mut cuts: Vec<(String, PathBuf)> =
            std::fs::read_dir(self.folder.join(crate::ja_rooms::FOLDER))
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_file())
                .filter_map(|path| Some((path.file_name()?.to_str()?.to_ascii_lowercase(), path)))
                .filter(|(name, _)| name.ends_with(".toml"))
                .collect();
        cuts.sort();
        for (name, path) in cuts {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            part(name.as_bytes());
            part(&bytes);
        }
        *hasher.finalize().as_bytes()
    }

    /// What the project's `kjk-area.toml` holds: its kjk-only content. An
    /// installed level's lights and lamps placed by hand are left out while
    /// they are the ones kjk's light data holds, and kept once they are the
    /// project's own (`KjkArea::own_lights`).
    pub fn own_kjk(&self) -> KjkArea {
        if self.map.is_some() {
            return self.kjk.clone();
        }
        installed_kjk(&self.kjk)
    }

    /// The map's ARE with its rooms' list made from its rooms.
    pub fn map_are(&self, map: &NewMap) -> Gff {
        map_are_of(&self.module, map)
    }

    /// Writes the module of each new map the project makes,
    /// `modules/<module>.mod` (`map::NewMap::module_files`), from the
    /// install's rooms that `read` gives. A project that makes no map has
    /// none to write. A parked map's is written too. Only the map being
    /// edited had its module written, so another map's stayed as it was
    /// when that map was last edited.
    pub fn save_module(
        &self,
        read: &dyn Fn(&str, ResType) -> Option<Vec<u8>>,
    ) -> Result<Vec<PathBuf>, String> {
        let edited = (self.module.as_str(), self.area.as_str(), self.map.as_ref());
        let parked = (self.parked.iter())
            .map(|(module, level)| (module.as_str(), level.area.as_str(), level.map.as_ref()));
        let mut written = Vec::new();
        for (module, area, map) in std::iter::once(edited).chain(parked) {
            let Some(map) = map else {
                continue;
            };
            let files = map.module_files(module, area, read)?;
            let bytes = fmt_odyssey::erf::write(fmt_odyssey::erf::ErfKind::Mod, &files)?;
            let path = self.folder.join("modules").join(format!("{module}.mod"));
            if let Some(folder) = path.parent() {
                std::fs::create_dir_all(folder)
                    .map_err(|error| format!("{}: {error}", folder.display()))?;
            }
            let fresh = path.with_extension("new");
            std::fs::write(&fresh, &bytes)
                .and_then(|()| std::fs::rename(&fresh, &path))
                .map_err(|error| format!("{}: {error}", path.display()))?;
            written.push(path);
        }
        Ok(written)
    }
}

/// What stops the save in one level: its module's name, its object list,
/// its kjk-only content, and its map when it is a new map.
fn level_problems(module: &str, git: &Gff, kjk: &KjkArea, map: Option<&NewMap>) -> Vec<String> {
    let mut problems = Vec::new();
    for entry in git::entries(git) {
        let named = format!("{:?} {} of the GIT", entry.kind, entry.index);
        if matches!(entry.kind, Kind::Creature | Kind::Placeable) && entry.template.is_empty() {
            problems.push(format!("{named} has no template"));
        }
        if entry.template.len() > RESREF_LEN {
            problems.push(format!(
                "{named}'s template {} is longer than {RESREF_LEN} characters",
                entry.template
            ));
        }
        if entry.tag.len() > TAG_LEN {
            problems.push(format!(
                "{named}'s tag {} is longer than {TAG_LEN} characters",
                entry.tag
            ));
        }
    }
    let mut names: Vec<&str> = Vec::new();
    for prop in &kjk.props {
        if prop.model.is_empty() {
            problems.push(format!("prop {} has no model", prop.name));
        }
        if !(SCALES.0..=SCALES.1).contains(&prop.scale) {
            problems.push(format!(
                "prop {} is drawn at scale {}, outside {} to {}",
                prop.name, prop.scale, SCALES.0, SCALES.1
            ));
        }
        names.push(&prop.name);
    }
    for shape in &kjk.boxes {
        // A side that is no number is under no size, so it is asked for
        // by itself.
        let fits = |side: &f32| side.is_finite() && *side >= SMALLEST_SIDE;
        if !shape.size.iter().all(fits) {
            problems.push(format!(
                "box {} has a side under {SMALLEST_SIDE} m",
                shape.name
            ));
        }
        names.push(&shape.name);
    }
    names.sort_unstable();
    for pair in names.windows(2) {
        if pair[0] == pair[1] {
            problems.push(format!("two props or boxes are named {}", pair[0]));
        }
    }
    if let Some(map) = map {
        problems.extend(crate::map::name_problem(module));
        if map.rooms.len() > 999 {
            problems.push("a map has 999 rooms at most".into());
        }
    }
    problems
}

/// The ARE of the new map `map` of the module `module`, with its rooms'
/// list made from its rooms.
fn map_are_of(module: &str, map: &NewMap) -> Gff {
    let mut map = map.clone();
    map.rooms_into_are(module);
    map.are
}

/// What a parked level's kjk-area file holds: all of a new map's content,
/// and an installed level's as `installed_kjk` gives it.
fn level_kjk(level: &Level) -> KjkArea {
    if level.map.is_some() {
        return level.kjk.clone();
    }
    installed_kjk(&level.kjk)
}

/// What an installed level's kjk-area file holds of `kjk`: all of it when
/// its lights and lamps placed by hand are the project's own, else all but
/// them, which are the light data's.
fn installed_kjk(kjk: &KjkArea) -> KjkArea {
    if kjk.own_lights {
        return kjk.clone();
    }
    KjkArea {
        lights: Vec::new(),
        lamps: Vec::new(),
        ..kjk.clone()
    }
}

/// The templates in a project's folder (`RESREF.utp`, `RESREF.utc`) by
/// resref, with their paths.
/// The conversations at the top of `folder`, by lowercase name.
fn dialog_files(folder: &Path) -> Vec<(String, PathBuf)> {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let resref = name.strip_suffix(".dlg")?;
            Some((resref.to_string(), entry.path()))
        })
        .collect()
}

fn template_files(folder: &Path) -> Vec<(String, PathBuf)> {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let (resref, extension) = name.rsplit_once('.')?;
            template::TYPES
                .iter()
                .any(|(_, of)| *of == extension)
                .then(|| (resref.to_string(), entry.path()))
        })
        .collect()
}

/// The templates a project's folder holds.
///
/// A project keeps one template of a name, whatever its kind
/// (`Project::templates`). Two files of one name and two kinds, as
/// `foo.utc` and `foo.utp`, are refused. One of the two used to be left out
/// without a word, whichever the folder listed first.
fn read_templates(folder: &Path) -> Result<BTreeMap<String, Template>, String> {
    let mut templates = BTreeMap::new();
    let mut files: BTreeMap<String, PathBuf> = BTreeMap::new();
    for (resref, path) in template_files(folder) {
        if let Some(other) = files.insert(resref.clone(), path.clone()) {
            let name = |path: &Path| {
                path.file_name()
                    .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
            };
            return Err(format!(
                "{} and {} are two templates named {resref}, and a project keeps one of a name: \
                 take one of the two files out of {}",
                name(&other),
                name(&path),
                folder.display()
            ));
        }
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(String::new);
        let restype = template::TYPES
            .iter()
            .find(|(_, of)| *of == extension)
            .map_or(ResType::UTP, |(restype, _)| *restype);
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let gff = Gff::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        // A store's template names itself in `ResRef`.
        let from = gff
            .root
            .resref("TemplateResRef")
            .or_else(|| gff.root.resref("ResRef"))
            .unwrap_or_else(String::new)
            .to_ascii_lowercase();
        // An item names itself, and notes what it was copied from.
        let from = match restype {
            ResType::UTI => crate::items::base_of(&gff).unwrap_or(from),
            _ => from,
        };
        templates.insert(resref, Template { restype, from, gff });
    }
    Ok(templates)
}
