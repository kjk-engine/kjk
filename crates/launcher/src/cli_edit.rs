//! `kjk edit` and `--project`: a level editor project opened for editing
//! or for play (`docs/LEVEL-EDIT.md`).

use std::path::PathBuf;
use std::process::ExitCode;

use asset_odyssey::module::ModuleIr;
use fmt_odyssey::ResType;
use kjk_game::viewer::ResolverSource;
use level_edit::Project;
use vfs::Game;
use vfs::resolver::{Resolver, TextureQuality};

use crate::{discover, play, runtime};

/// The folder of the project `name`, from `KJK_PROJECTS` or kjk's folder.
pub(crate) fn folder_of(name: &str) -> Result<PathBuf, String> {
    if !level_edit::valid_name(name) {
        return Err(format!(
            "{name:?} cannot name a project: use 1 to 64 letters, digits, - and _"
        ));
    }
    level_edit::projects_folder(|var| std::env::var(var).ok())
        .map(|projects| projects.join(name))
        .ok_or("no folder for projects: set KJK_PROJECTS or KJK_HOME".into())
}

/// Makes the project `name` this run's (`level_edit::active`), before any
/// install is read. The project must exist: `kjk edit` makes one.
pub(crate) fn activate(name: &str) -> Result<(), String> {
    let folder = folder_of(name)?;
    if !Project::exists(&folder) {
        return Err(format!(
            "there is no project {name} in {}: `kjk edit GAME MODULE --project {name}` makes one",
            folder.display()
        ));
    }
    let active = level_edit::active::activate(&discover(), &folder)?;
    eprintln!(
        "project {}: {} {} from {}",
        active.project.name,
        active.project.game,
        active.project.module,
        folder.display()
    );
    for note in &active.notes {
        eprintln!("project {}: {note}", active.project.name);
    }
    Ok(())
}

/// The project in `folder` made for `game`'s `module`: its entry area's GIT
/// as the install has it, and no kjk-only content.
fn make(folder: &std::path::Path, game: Game, module: &str) -> Result<Project, String> {
    let discovery = discover();
    let install = discovery
        .get(game)
        .ok_or(format!("{} is not installed", game.key()))?;
    let index = vfs::DirIndex::of_install(install).map_err(|error| error.to_string())?;
    let resolver = Resolver::open(game, &index, Some(module), TextureQuality::High)
        .map_err(|error| format!("{module}: {error:?}"))?;
    let ir = ModuleIr::build(module, &ResolverSource(&resolver))
        .map_err(|refusal| format!("{module}: {refusal:?}"))?;
    let area = ir
        .areas
        .iter()
        .find(|area| area.resref == ir.entry_area)
        .or(ir.areas.first())
        .ok_or(format!("{module}: no area"))?;
    let bytes = resolver
        .read(&area.resref, ResType::GIT)
        .ok_or(format!("{module}: no {}.git", area.resref))?;
    let git = fmt_odyssey::gff::Gff::parse(&bytes)
        .map_err(|error| format!("{}.git: {error}", area.resref))?;
    let project = Project::new(folder, game.key(), module, &area.resref, git);
    project.save(&format!("kjk {}", env!("KJK_GIT_SHA")))?;
    Ok(project)
}

/// `kjk edit GAME MODULE [--project NAME]`: the editor on the project
/// (made first when there is none; named after the module when no name is
/// given).
pub(crate) fn edit(
    (game, module, project): (&str, &str, Option<&str>),
    (cmds, headless, size): (&str, bool, (u32, u32)),
) -> ExitCode {
    match setup(game, module, project) {
        Ok(setup) => play(cmds, headless, size, runtime::Scene::Edit(Box::new(setup))),
        Err(error) => {
            eprintln!("kjk edit: {error}");
            ExitCode::FAILURE
        }
    }
}

fn setup(
    game: &str,
    module: &str,
    project: Option<&str>,
) -> Result<kjk_game::editor::EditSetup, String> {
    let game = Game::from_key(game)
        .filter(|game| matches!(game, Game::Kotor1 | Game::Kotor2))
        .ok_or("the editor edits k1 or tsl")?;
    let module = module.to_ascii_lowercase();
    let name = project.unwrap_or(&module);
    let folder = folder_of(name)?;
    if Project::exists(&folder) {
        let found = Project::open(&folder)?;
        if found.game != game.key() || found.module != module {
            return Err(format!(
                "project {name} edits {} {}, not {} {module}",
                found.game,
                found.module,
                game.key()
            ));
        }
    } else {
        make(&folder, game, &module)?;
        eprintln!("made project {name} in {}", folder.display());
    }
    let discovery = discover();
    let active = level_edit::active::activate(&discovery, &folder)?;
    for note in &active.notes {
        eprintln!("project {name}: {note}");
    }
    let resolver = kjk_game::viewer::resolver(&discovery, game, Some(&module))?;
    let sorted = |restype| {
        let mut names = resolver.names(restype);
        names.sort();
        names.dedup();
        names
    };
    let placeables = sorted(ResType::UTP);
    let creatures = sorted(ResType::UTC);
    let ja = level_edit::active::ja_stack(&discovery);
    let models = ja.as_ref().map_or_else(Vec::new, |stack| {
        level_edit::active::models_under(stack, "models/map_objects/")
    });
    Ok(kjk_game::editor::EditSetup {
        project: active.project.clone(),
        game,
        resolver,
        ja,
        placeables,
        creatures,
        models,
        program: std::env::current_exe().map_err(|error| error.to_string())?,
    })
}
