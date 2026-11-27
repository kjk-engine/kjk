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
//!
//! A run without `--project` takes what the mods in each game's
//! `override/` hold for kjk in the same way (`activate_mods`, `mods`): the
//! levels they change are one project a game, read from that folder.

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
    /// The project's own lighting rules (`lighting`): the kinds of fitting
    /// it changed, which the game reads in place of the light data's.
    pub lighting: crate::lighting::OwnLighting,
    /// The bake the project keeps of each of its levels that has one
    /// (`lighting::kept_bake`), by module: the folder of its lightmaps.
    pub bakes: std::collections::BTreeMap<String, std::path::PathBuf>,
    /// What could not be read: a prop whose model Jedi Academy's archives
    /// lack, or whose model does not parse. Such a prop is neither drawn
    /// nor solid. For mods, also what two of them both change.
    pub notes: Vec<String>,
    /// The quests the mods bring (`mods::journal_file`), each mod's
    /// journal. A project's own quests are in its copy of the game's
    /// journal, so it has none here.
    pub journals: Vec<fmt_odyssey::gff::Gff>,
    /// What each mod says of itself (`mods::Manifest`).
    pub mods: Vec<crate::mods::Manifest>,
    /// The mods' new maps' rooms cut from Jedi Academy levels
    /// (`mods::JaMap`), which the run builds before it opens a resolver
    /// (`kjk_game::ja_made::make_mods`). A project keeps its own in its
    /// maps, and has none here.
    pub ja_maps: Vec<crate::mods::JaMap>,
    /// A digest of what this changes of the world beyond the games' own
    /// files: the mods' kjk-only content and quests, or a project's digest.
    /// It is part of the key of every area's bake (`world_digest`).
    pub world: Option<[u8; 32]>,
    /// What two machines that play together compare (`compared_digest`):
    /// `world`, and for a project its compiled scripts too
    /// (`with_scripts`).
    pub compared: Option<[u8; 32]>,
}

/// The run's active projects: the one `--project` names, or one for each
/// game whose `override/` holds mods for kjk.
static ACTIVE: OnceLock<Vec<Active>> = OnceLock::new();

fn all() -> &'static [Active] {
    ACTIVE.get().map_or(&[], Vec::as_slice)
}

/// The active project, if one was activated: the one `--project` names,
/// else the first game's mods.
pub fn get() -> Option<&'static Active> {
    all().first()
}

/// The active project of `game` (`k1` or `tsl`).
fn of_game(game: &str) -> Option<&'static Active> {
    all()
        .iter()
        .find(|active| active.project.game.eq_ignore_ascii_case(game))
}

/// The kjk-only content the active project of `game` holds for its level
/// `module`.
pub fn kjk_of(game: &str, module: &str) -> Option<&'static crate::area::KjkArea> {
    of_game(game)?.project.kjk_of(module)
}

/// What the active project adds to the collision of `game`'s `module`;
/// `None` when no project is active or it edits another module.
pub fn added(game: &str, module: &str) -> Option<&'static Added> {
    let active = of_game(game)?;
    if active.project.module.eq_ignore_ascii_case(module) {
        return Some(&active.added);
    }
    active.parked_added.get(&module.to_ascii_lowercase())
}

/// The game's journal `jrl` with the quests of `game`'s mods added
/// (`journal::merge`). A game without a journal starts from an empty one.
/// `None` when no mod brings a quest: the journal is then the game's own.
pub fn merged_journal(
    game: &str,
    jrl: Option<fmt_odyssey::gff::Gff>,
) -> Option<fmt_odyssey::gff::Gff> {
    let active = of_game(game).filter(|active| !active.journals.is_empty())?;
    let mut merged = jrl.unwrap_or_else(crate::journal::empty);
    for journal in &active.journals {
        crate::journal::merge(&mut merged, journal);
    }
    Some(merged)
}

/// What the active project or the mods of `game` change of the world
/// beyond the games' own files (`Active::world`).
pub fn world_digest(game: &str) -> Option<[u8; 32]> {
    of_game(game)?.world
}

/// What two machines that play `game` together compare of the active
/// project or the mods (`Active::compared`).
pub fn compared_digest(game: &str) -> Option<[u8; 32]> {
    of_game(game)?.compared
}

/// `digest` (a project's, `Project::digest`) with the project's compiled
/// scripts: each `NAME.ncs` at the top of its `folder`, by name. Two
/// machines that play a project together compare this. The install's
/// fingerprint does not cover the project's folder, and the project's own
/// digest leaves its scripts out. That digest is part of the key of every
/// area's baked collision, which a changed script does not change.
pub fn with_scripts(digest: &[u8; 32], folder: &Path) -> [u8; 32] {
    let mut scripts: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && (path.extension().and_then(|extension| extension.to_str()))
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("ncs"))
        })
        .filter_map(|path| Some((path.file_stem()?.to_str()?.to_ascii_lowercase(), path)))
        .collect();
    scripts.sort();
    let mut hasher = blake3::Hasher::new();
    hasher.update(digest);
    for (name, path) in scripts {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        for part in [name.as_bytes(), &bytes] {
            hasher.update(&(part.len() as u64).to_le_bytes());
            hasher.update(part);
        }
    }
    *hasher.finalize().as_bytes()
}

/// The active project's digest, for `game`'s `module`.
pub fn digest(game: &str, module: &str) -> Option<[u8; 32]> {
    added(game, module)
        .and(of_game(game))
        .map(|active| active.digest)
}

/// The props the active project places in `game`'s `module`.
pub fn props(game: &str, module: &str) -> &'static [Prop] {
    added(game, module)
        .and(of_game(game))
        .and_then(|active| active.project.kjk_of(module))
        .map_or(&[], |kjk| kjk.props.as_slice())
}

/// The Jedi Academy ambient effects the active project places in `game`'s
/// `module`.
pub fn effects(game: &str, module: &str) -> &'static [crate::effects::AmbientEffect] {
    added(game, module)
        .and(of_game(game))
        .and_then(|active| active.project.kjk_of(module))
        .map_or(&[], |kjk| kjk.effects.as_slice())
}

/// The name the game plays each effect by (`AmbientEffect::played_name`)
/// that any level of the active projects of `game` places, each once.
pub fn effect_names(game: &str) -> Vec<String> {
    let mut names: Vec<String> = of_game(game)
        .into_iter()
        .flat_map(|active| {
            std::iter::once(&active.project.kjk)
                .chain(active.project.parked.values().map(|level| &level.kjk))
        })
        .flat_map(|kjk| kjk.effects.iter().map(|effect| effect.played_name()))
        .collect();
    names.sort();
    names.dedup();
    names
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

/// Every file of Jedi Academy's `base/` archives under `folder` whose name
/// ends with `ending`, sorted.
pub fn files_under(stack: &vfs::ja::JaStack, folder: &str, ending: &str) -> Vec<String> {
    let folder = fmt_ja::game_path(folder);
    let mut found: Vec<String> = stack
        .base()
        .into_keys()
        .filter(|name| name.starts_with(folder.as_str()) && name.ends_with(ending))
        .map(str::to_string)
        .collect();
    found.sort();
    found
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
    let mut notes = Vec::new();
    let lighting = match crate::lighting::OwnLighting::read(folder) {
        Ok(lighting) => lighting,
        Err(why) => {
            notes.push(why);
            crate::lighting::OwnLighting::default()
        }
    };
    let bakes = (project.level_list().into_iter())
        .filter_map(|(module, _)| {
            let bake = crate::lighting::kept_bake(folder, &module)?;
            Some((module, bake))
        })
        .collect();
    // Asked before the resolvers are told of the folder. A second call
    // used to tell them of its folder and was refused only after that,
    // which left them with one project's folder and the run with another's.
    if ACTIVE.get().is_some() {
        return Err("a project is already active".to_string());
    }
    vfs::project::activate(game, folder)?;
    let mut active = made(discovery, project, lighting, bakes, notes);
    active.world = Some(active.digest);
    active.compared = Some(with_scripts(&active.digest, folder));
    ACTIVE
        .set(vec![active])
        .map_err(|_| "a project is already active".to_string())?;
    Ok(get().expect("set above"))
}

/// Takes what the mods in each KOTOR game's `override/` hold for kjk
/// (`mods::read`) as this run's, one project a game, when no project was
/// activated: the levels they change, their own lighting rules and their
/// bakes. The games' own files of the mods are read from `override/` as
/// any mod's are. A game with no such mod has no project.
pub fn activate_mods(discovery: &Discovery) -> &'static [Active] {
    if ACTIVE.get().is_some() {
        return all();
    }
    let mut actives = Vec::new();
    for game in [Game::Kotor1, Game::Kotor2] {
        let Some(install) = discovery.get(game) else {
            continue;
        };
        // The game's `override/`, in whatever case it is named, and every
        // file under it by its lowercase path inside it. The folder alone
        // is listed, not the install, so a start stays quick.
        let Some(folder) = std::fs::read_dir(&install.data_root)
            .ok()
            .and_then(|entries| {
                entries.flatten().map(|entry| entry.path()).find(|path| {
                    path.is_dir()
                        && (path.file_name().and_then(|name| name.to_str()))
                            .is_some_and(|name| name.eq_ignore_ascii_case("override"))
                })
            })
        else {
            continue;
        };
        let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();
        let mut folders = vec![(String::new(), folder.clone())];
        while let Some((inside, at)) = folders.pop() {
            for entry in std::fs::read_dir(&at).into_iter().flatten().flatten() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                let inside = if inside.is_empty() {
                    name
                } else {
                    format!("{inside}/{name}")
                };
                let path = entry.path();
                if path.is_dir() {
                    // A bake lies two folders down (`kjk-bake/<module>/`).
                    if inside.matches('/').count() < 2 {
                        folders.push((inside, path));
                    }
                } else {
                    files.push((inside, path));
                }
            }
        }
        files.sort();
        let mut found = crate::mods::read(&files);
        // What the mods were built on: a file of the override folder, or
        // a mod file of the game's `modules/` folder. A mod's own module
        // file, a new map's, is looked for there too.
        if !found.mods.is_empty() {
            let modules: Vec<(String, std::path::PathBuf)> = std::fs::read_dir(&install.data_root)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_dir()
                        && (path.file_name().and_then(|name| name.to_str()))
                            .is_some_and(|name| name.eq_ignore_ascii_case("modules"))
                })
                .flat_map(|folder| std::fs::read_dir(folder).into_iter().flatten().flatten())
                .map(|entry| {
                    (
                        entry.file_name().to_string_lossy().to_ascii_lowercase(),
                        entry.path(),
                    )
                })
                .collect();
            let notes = crate::mods::needs_notes(&found.mods, &files, &modules);
            found.notes.extend(notes);
            found
                .notes
                .extend(crate::mods::module_notes(&found.mods, &modules));
        }
        if found.is_empty() && found.notes.is_empty() {
            continue;
        }
        let project = Project::of_mods(&folder, game.key(), found.levels);
        let mut active = made(discovery, project, found.lighting, found.bakes, found.notes);
        active.journals = found.journals;
        active.mods = found.mods;
        active.ja_maps = found.ja_maps;
        active.world = found.world;
        // A mod's scripts are files of `override/`, which the install's
        // fingerprint covers.
        active.compared = found.world;
        actives.push(active);
    }
    let _ = ACTIVE.set(actives);
    all()
}

/// The active project `project` makes: what its solid props and boxes add
/// to the collision of each of its levels, with its lighting and bakes.
fn made(
    discovery: &Discovery,
    project: Project,
    lighting: crate::lighting::OwnLighting,
    bakes: std::collections::BTreeMap<String, std::path::PathBuf>,
    mut notes: Vec<String>,
) -> Active {
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
    let (added, own_notes) = collision_of(&project, stack.as_ref());
    notes.extend(own_notes);
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
    let digest = project.digest();
    Active {
        project,
        added,
        parked_added,
        digest,
        lighting,
        bakes,
        notes,
        journals: Vec::new(),
        mods: Vec::new(),
        ja_maps: Vec::new(),
        world: None,
        compared: None,
    }
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
        .and(of_game(game))
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
        .and(of_game(game))
        .and_then(|active| active.project.kjk_of(module))
        .map_or_else(Vec::new, |kjk| {
            kjk.links
                .iter()
                .map(|link| (link.kind, link.from, link.to))
                .collect()
        })
}

/// The folder of the bake that the active project of `game` (`k1` or
/// `tsl`) keeps of its level `module`, when it keeps one: the game draws
/// the level's rooms with it (`game::fitted_lights::for_module`,
/// `game::viewer`).
///
/// The game is asked for with the level, here and in `map_lights` and
/// `map_lamps`. With mods in both games' folders the run has a project for
/// each, and a level of one name in both, as two new maps can be, used to
/// get the first game's bake, lights and lamps in either game.
pub fn bake_of(game: &str, module: &str) -> Option<&'static Path> {
    (of_game(game)?.bakes)
        .get(&module.to_ascii_lowercase())
        .map(std::path::PathBuf::as_path)
}

/// The folder of every bake the active project keeps.
pub fn bake_folders() -> Vec<&'static Path> {
    all()
        .iter()
        .flat_map(|active| active.bakes.values())
        .map(std::path::PathBuf::as_path)
        .collect()
}

/// Whether an active project lights any of its levels itself: it keeps a
/// bake of one, or one has lights or lamps that are the project's own
/// (`map_lights`, `map_lamps`). The game then builds its shaders with the
/// fitted lights (`game::fitted_lights::in_shaders`).
pub fn lights_any() -> bool {
    all().iter().any(|active| {
        let project = &active.project;
        !active.bakes.is_empty()
            || (std::iter::once(project.module.as_str()))
                .chain(project.parked.keys().map(String::as_str))
                .any(|module| {
                    let game = project.game.as_str();
                    map_lights(game, module).is_some_and(|lights| !lights.is_empty())
                        || map_lamps(game, module).is_some_and(|lamps| !lamps.is_empty())
                })
    })
}

/// The kinds of light fitting the active project changed, as `[[kind]]`
/// entries, when the project is of `game` (`k1` or `tsl`) and changed any:
/// the game reads them in place of the light data's
/// (`game::fitted_lights`).
pub fn own_kinds(game: &str) -> Option<String> {
    let active = of_game(game)?;
    (!active.lighting.kinds.is_empty()).then(|| active.lighting.kinds_text())
}

/// The lights placed by hand that the active project of `game` keeps for
/// its level `module`: a new map's, or an installed level's once they are the
/// project's own (`KjkArea::own_lights`). The game reads them in place of
/// `fittings.toml`'s (`game::fitted_lights`). `None` for a level whose
/// lights are the light data's.
pub fn map_lights(game: &str, module: &str) -> Option<&'static [crate::lights::PlacedLight]> {
    let active = of_game(game)?;
    let kjk = active.project.kjk_of(module)?;
    (active.project.is_map(module) || kjk.own_lights).then_some(kjk.lights.as_slice())
}

/// The lamps placed by hand that the active project of `game` keeps for
/// its level `module`, as `map_lights` gives its lights: the game draws them in
/// place of `fittings.toml`'s (`game::lamps`).
pub fn map_lamps(game: &str, module: &str) -> Option<&'static [crate::lamps::PlacedLamp]> {
    let active = of_game(game)?;
    let kjk = active.project.kjk_of(module)?;
    (active.project.is_map(module) || kjk.own_lights).then_some(kjk.lamps.as_slice())
}
