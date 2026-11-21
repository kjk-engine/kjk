//! The project this run plays or edits (`--project NAME`), opened once at
//! the start, before any install is read.
//!
//! Activating a project does three things. Its folder is read over the
//! install's `override/` (`vfs::project`), so the area's changed GIT and the
//! templates the project brings win. Its props' models are read from Jedi
//! Academy's archives once. And its solid props and boxes are made ready to
//! join the collision of its module's area (`added`), which every plan of
//! that module builds (`session`, the collision verbs). A project of
//! several levels (`Project::parked`) does this for each of them.

use std::path::Path;
use std::sync::OnceLock;

use collision::clip_file::{Added, ClipFile};
use vfs::{Discovery, Game};

use crate::area::Prop;
use crate::project::Project;

/// The active project, with what it adds to the collision.
pub struct Active {
    pub project: Project,
    /// The solid props' triangles and the boxes' hulls of the project's
    /// level `project.module`.
    pub added: Added,
    /// The same for each of its other levels, by module.
    pub parked_added: std::collections::BTreeMap<String, Added>,
    /// `Project::digest`.
    pub digest: [u8; 32],
    /// What could not be read: a prop whose model Jedi Academy's archives
    /// lack, or whose model does not parse. Such a prop is neither drawn
    /// nor solid.
    pub notes: Vec<String>,
}

static ACTIVE: OnceLock<Active> = OnceLock::new();

/// The active project, if one was activated.
pub fn get() -> Option<&'static Active> {
    ACTIVE.get()
}

/// What the active project adds to the collision of `game`'s `module`;
/// `None` when no project is active or it edits another module.
pub fn added(game: &str, module: &str) -> Option<&'static Added> {
    let active = get()?;
    if !active.project.game.eq_ignore_ascii_case(game) {
        return None;
    }
    if active.project.module.eq_ignore_ascii_case(module) {
        return Some(&active.added);
    }
    active.parked_added.get(&module.to_ascii_lowercase())
}

/// The active project's digest, for `game`'s `module`.
pub fn digest(game: &str, module: &str) -> Option<[u8; 32]> {
    added(game, module).and(get()).map(|active| active.digest)
}

/// The props the active project places in `game`'s `module`.
pub fn props(game: &str, module: &str) -> &'static [Prop] {
    added(game, module)
        .and(get())
        .and_then(|active| active.project.kjk_of(module))
        .map_or(&[], |kjk| kjk.props.as_slice())
}

/// Jedi Academy's archives, as found on this machine; `None` without the
/// game.
pub fn ja_stack(discovery: &Discovery) -> Option<vfs::ja::JaStack> {
    let install = discovery.get(Game::JediAcademy)?;
    let index = vfs::DirIndex::of_install(install).ok()?;
    Some(vfs::ja::JaStack::open(&index))
}

/// A file's bytes by its path in Jedi Academy's `base/` archives, as Jedi
/// Academy itself reads them (a mod folder left out).
pub fn ja_read(stack: &vfs::ja::JaStack, path: &str) -> Result<Vec<u8>, String> {
    let path = fmt_ja::game_path(path);
    let base = stack.base();
    let found = base
        .get(path.as_str())
        .ok_or(format!("{path}: not in Jedi Academy's archives"))?;
    stack.read(&path, found.source)
}

/// A prop's model, parsed.
pub fn md3(stack: &vfs::ja::JaStack, path: &str) -> Result<fmt_ja::md3::Md3, String> {
    let bytes = ja_read(stack, path)?;
    fmt_ja::md3::parse(&bytes).map_err(|error| format!("{path}: {error}"))
}

/// Every MD3 model under `folder` (`models/map_objects/`) in `base/`,
/// sorted.
pub fn models_under(stack: &vfs::ja::JaStack, folder: &str) -> Vec<String> {
    let folder = fmt_ja::game_path(folder);
    stack
        .base()
        .into_keys()
        .filter(|name| name.starts_with(folder.as_str()) && name.ends_with(".md3"))
        .map(str::to_string)
        .collect()
}

/// What `project`'s solid props and boxes add to the collision, with notes
/// on the props that could not be read. Models are read from `stack`;
/// without it every solid prop is noted and left out.
pub fn collision_of(project: &Project, stack: Option<&vfs::ja::JaStack>) -> (Added, Vec<String>) {
    kjk_collision(&project.kjk, stack)
}

/// What a level's kjk-only content (`kjk`) adds to the collision, as
/// `collision_of` says.
pub fn kjk_collision(
    kjk: &crate::area::KjkArea,
    stack: Option<&vfs::ja::JaStack>,
) -> (Added, Vec<String>) {
    let mut added = Added::default();
    let mut notes = Vec::new();
    for shape in kjk.boxes.iter().filter(|shape| shape.role.solid()) {
        added.hulls.push(shape.hull());
    }
    for prop in kjk.props.iter().filter(|prop| prop.solid) {
        let Some(stack) = stack else {
            notes.push(format!("prop {}: Jedi Academy is not installed", prop.name));
            continue;
        };
        match md3(stack, &prop.model) {
            Ok(model) => added.triangles.extend(prop.triangles(&model)),
            Err(why) => notes.push(format!("prop {}: {why}", prop.name)),
        }
    }
    (added, notes)
}

/// `clip` (a module's built-in clip file, if it has one) with `added` in
/// it.
pub fn with_added(clip: Option<ClipFile>, added: &Added) -> ClipFile {
    let mut clip = clip.unwrap_or_else(ClipFile::default);
    clip.added.hulls.extend(added.hulls.iter().cloned());
    clip.added.triangles.extend(added.triangles.iter().copied());
    clip
}

/// Opens the project in `folder` as this run's (once: a second call is
/// refused). Its folder joins every resolver of its game made from now on
/// (`vfs::project::activate`).
pub fn activate(discovery: &Discovery, folder: &Path) -> Result<&'static Active, String> {
    let project = Project::open(folder)?;
    let game = Game::from_key(&project.game)
        .filter(|game| matches!(game, Game::Kotor1 | Game::Kotor2))
        .ok_or(format!(
            "{}: the game {:?} is not k1 or tsl",
            project.name, project.game
        ))?;
    let stack = if project.kjk.props.is_empty()
        && project
            .parked
            .values()
            .all(|level| level.kjk.props.is_empty())
    {
        None
    } else {
        ja_stack(discovery)
    };
    let (added, mut notes) = collision_of(&project, stack.as_ref());
    let mut parked_added = std::collections::BTreeMap::new();
    for (module, level) in &project.parked {
        let (added, level_notes) = kjk_collision(&level.kjk, stack.as_ref());
        notes.extend(
            level_notes
                .into_iter()
                .map(|note| format!("{module}: {note}")),
        );
        parked_added.insert(module.clone(), added);
    }
    vfs::project::activate(game, folder)?;
    let digest = project.digest();
    let active = Active {
        project,
        added,
        parked_added,
        digest,
        notes,
    };
    ACTIVE
        .set(active)
        .map_err(|_| "a project is already active".to_string())?;
    Ok(get().expect("set above"))
}

/// `clip` (the module's built-in clip file, if it has one) with what the
/// active project adds to `game`'s `module`, when it adds anything: the
/// clip file every plan and every collision verb builds the area with.
pub fn clip_for(game: &str, module: &str, clip: Option<ClipFile>) -> Option<ClipFile> {
    match added(game, module) {
        Some(added) => Some(with_added(clip, added)),
        None => clip,
    }
}

/// `floor` (an area's walkable faces) as the active project changes the
/// navigation mesh of `game`'s `module` (`nav::edited`); as it is when no
/// project is active or it edits another module.
pub fn nav_faces(game: &str, module: &str, floor: Vec<[[f32; 3]; 3]>) -> Vec<[[f32; 3]; 3]> {
    match added(game, module)
        .and(get())
        .and_then(|active| active.project.kjk_of(module))
    {
        Some(kjk) => crate::nav::edited(floor, &kjk.boxes),
        None => floor,
    }
}

/// The links between floors the active project adds to the navigation
/// mesh of `game`'s `module` (`collision::nav::NavMesh::add_links`): each
/// its kind, where it leaves and where it lands.
pub fn nav_links(game: &str, module: &str) -> Vec<(collision::nav::LinkKind, [f32; 3], [f32; 3])> {
    added(game, module)
        .and(get())
        .and_then(|active| active.project.kjk_of(module))
        .map_or_else(Vec::new, |kjk| {
            kjk.links
                .iter()
                .map(|link| (link.kind, link.from, link.to))
                .collect()
        })
}

/// The lights placed by hand in the active project's new map, when
/// `module` is it: the game reads them in place of `fittings.toml`'s
/// (`game::fitted_lights`). An installed level's are saved to that file.
pub fn map_lights(module: &str) -> Option<&'static [crate::lights::PlacedLight]> {
    let active = get()?;
    (active.project.map.is_some() && active.project.module.eq_ignore_ascii_case(module))
        .then_some(active.project.kjk.lights.as_slice())
}

/// The lamps placed by hand in the active project's new map, when `module`
/// is it: the game draws them in place of `fittings.toml`'s
/// (`game::lamps`). An installed level's are saved to that file.
pub fn map_lamps(module: &str) -> Option<&'static [crate::lamps::PlacedLamp]> {
    let active = get()?;
    (active.project.map.is_some() && active.project.module.eq_ignore_ascii_case(module))
        .then_some(active.project.kjk.lamps.as_slice())
}
