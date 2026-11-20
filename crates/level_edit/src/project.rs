//! A project's folder: `project.toml`, the area's `.git` file,
//! `kjk-area.toml` and the templates the project brings, read and written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fmt_odyssey::ResType;
use fmt_odyssey::gff::Gff;

use crate::area::{KjkArea, SCALES, SMALLEST_SIDE};
use crate::git::{self, Kind, RESREF_LEN, TAG_LEN};
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
            folder: folder.to_path_buf(),
            game: game.to_ascii_lowercase(),
            module: module.to_ascii_lowercase(),
            area: area.to_ascii_lowercase(),
            git,
            kjk: KjkArea::default(),
            templates: BTreeMap::new(),
        }
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
        project.kjk = kjk;
        project.templates = read_templates(folder)?;
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
        for entry in git::entries(&self.git) {
            let named = format!("{:?} {} of the GIT", entry.kind, entry.index);
            if matches!(entry.kind, Kind::Creature | Kind::Placeable) && entry.template.is_empty() {
                block(format!("{named} has no template"));
            }
            if entry.template.len() > RESREF_LEN {
                block(format!(
                    "{named}'s template {} is longer than {RESREF_LEN} characters",
                    entry.template
                ));
            }
            if entry.tag.len() > TAG_LEN {
                block(format!(
                    "{named}'s tag {} is longer than {TAG_LEN} characters",
                    entry.tag
                ));
            }
        }
        let mut names: Vec<&str> = Vec::new();
        for prop in &self.kjk.props {
            if prop.model.is_empty() {
                block(format!("prop {} has no model", prop.name));
            }
            if !(SCALES.0..=SCALES.1).contains(&prop.scale) {
                block(format!(
                    "prop {} is drawn at scale {}, outside {} to {}",
                    prop.name, prop.scale, SCALES.0, SCALES.1
                ));
            }
            names.push(&prop.name);
        }
        for shape in &self.kjk.boxes {
            if shape.size.iter().any(|side| *side < SMALLEST_SIDE) {
                block(format!(
                    "box {} has a side under {SMALLEST_SIDE} m",
                    shape.name
                ));
            }
            names.push(&shape.name);
        }
        names.sort_unstable();
        for pair in names.windows(2) {
            if pair[0] == pair[1] {
                block(format!("two props or boxes are named {}", pair[0]));
            }
        }
        problems
    }

    /// Writes the folder: `project.toml`, the area's `.git`, and
    /// `kjk-area.toml`. Refused, with nothing written, while a problem
    /// blocks it. `made_by` names the kjk that wrote it (its commit).
    /// Returns the files written.
    pub fn save(&self, made_by: &str) -> Result<Vec<PathBuf>, String> {
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
        let project = format!(
            "# A kjk level editor project (docs/LEVEL-EDIT.md).\n\
             game = {:?}\nmodule = {:?}\narea = {:?}\nmade_by = {:?}\n",
            self.game, self.module, self.area, made_by
        );
        let mut files = vec![
            (PROJECT_FILE.to_string(), project.into_bytes()),
            (format!("{}.git", self.area), self.git.write()),
            (AREA_FILE.to_string(), self.kjk.write().into_bytes()),
        ];
        for (resref, made) in &self.templates {
            files.push((
                format!("{resref}.{}", template::extension(made.restype)),
                made.gff.write(),
            ));
        }
        // A template no object uses any more (an edit undone) leaves the
        // folder.
        for (resref, path) in template_files(&self.folder) {
            if !self.templates.contains_key(&resref) {
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

    /// A digest of what the project changes: its GIT and its kjk-only
    /// content, as they are written. Play keys the area's baked collision
    /// by it.
    pub fn digest(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        for part in [
            self.game.as_bytes(),
            self.module.as_bytes(),
            self.area.as_bytes(),
        ] {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part);
        }
        let git = self.git.write();
        hasher.update(&(git.len() as u64).to_le_bytes());
        hasher.update(&git);
        hasher.update(self.kjk.write().as_bytes());
        for (resref, made) in &self.templates {
            let bytes = made.gff.write();
            for part in [resref.as_bytes(), &bytes] {
                hasher.update(&(part.len() as u64).to_le_bytes());
                hasher.update(part);
            }
        }
        *hasher.finalize().as_bytes()
    }
}

/// The templates in a project's folder (`RESREF.utp`, `RESREF.utc`) by
/// resref, with their paths.
fn template_files(folder: &Path) -> Vec<(String, PathBuf)> {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            let (resref, extension) = name.rsplit_once('.')?;
            matches!(extension, "utp" | "utc").then(|| (resref.to_string(), entry.path()))
        })
        .collect()
}

/// The templates a project's folder holds.
fn read_templates(folder: &Path) -> Result<BTreeMap<String, Template>, String> {
    let mut templates = BTreeMap::new();
    for (resref, path) in template_files(folder) {
        let restype = if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("utc"))
        {
            ResType::UTC
        } else {
            ResType::UTP
        };
        let bytes = std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        let gff = Gff::parse(&bytes).map_err(|error| format!("{}: {error}", path.display()))?;
        let from = gff
            .root
            .resref("TemplateResRef")
            .unwrap_or_else(String::new)
            .to_ascii_lowercase();
        templates.insert(resref, Template { restype, from, gff });
    }
    Ok(templates)
}
