//! `kjk-level-editor`: kjk's level editor (`docs/LEVEL-EDIT.md`), a program
//! of its own built on kjk's game crate. It opens a KOTOR module's area
//! with a toolbar, an outliner, a details panel and an asset picker, and
//! saves what the user changes into a project, which `kjk play --project
//! NAME` plays. It never writes into a game install. Started with no
//! module, it lists the projects and the levels to pick from (`picker`).

mod picker;
mod state;
mod thumbs;
mod ui;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use asset_odyssey::module::ModuleIr;
use clap::Parser;
use fmt_odyssey::ResType;
use kjk_game::runtime;
use kjk_game::viewer::ResolverSource;
use level_edit::Project;
use vfs::Game;
use vfs::resolver::{Resolver, TextureQuality};

/// kjk's level editor: a KOTOR area's objects shown, moved, turned, copied
/// and deleted; templates, Jedi Academy props and collision boxes added;
/// saved into a project, never into the game
#[derive(Parser)]
#[command(name = "kjk-level-editor")]
struct Cli {
    /// k1 or tsl; with no game and module, the editor lists the levels to
    /// pick from
    game: Option<String>,
    /// The module, e.g. ebo_m12aa
    module: Option<String>,
    /// The project's name: the module's when left out. A project that does
    /// not exist is made from the module's area as the install has it
    #[arg(long)]
    project: Option<String>,
    /// Commands to run, e.g. 'edit select MetalBox; dump; quit'
    #[arg(long)]
    cmds: Option<String>,
    /// Run without a window, rendering into an off-screen image
    #[arg(long)]
    headless: bool,
    /// The headless frame size, e.g. 1600x900
    #[arg(long, value_parser = parse_size)]
    size: Option<(u32, u32)>,
}

fn parse_size(text: &str) -> Result<(u32, u32), String> {
    let (width, height) = text
        .split_once('x')
        .ok_or("use WIDTHxHEIGHT, e.g. 1600x900")?;
    let number = |part: &str| {
        part.parse::<u32>()
            .ok()
            .filter(|n| (16..=8192).contains(n))
            .ok_or(format!("{part}: a size from 16 to 8192"))
    };
    Ok((number(width)?, number(height)?))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let commands = match console::parse(cli.cmds.as_deref().unwrap_or("")) {
        Ok(commands) => commands,
        Err(error) => {
            eprintln!("--cmds: {error}");
            return ExitCode::from(2);
        }
    };
    let size = cli.size.unwrap_or(kjk_game::scene::HEADLESS_SIZE);
    let script = cli.cmds.clone().unwrap_or_else(String::new);
    let (game, module) = match (&cli.game, &cli.module) {
        (Some(game), Some(module)) => (game.clone(), module.clone()),
        (None, None) => {
            let headless = cli.headless;
            return run(runtime::RunOptions {
                commands,
                script,
                headless,
                size,
                games: Box::new(|| serde_json::Value::Null),
                scene: runtime::Scene::Loaded(Box::default()),
                plugins: Some(Box::new(move |app| picker::install(app, headless))),
            });
        }
        _ => {
            eprintln!("kjk-level-editor: give a game and a module, or neither to pick a level");
            return ExitCode::from(2);
        }
    };
    let setup = match setup(&game, &module, cli.project.as_deref()) {
        Ok(setup) => setup,
        Err(error) => {
            eprintln!("kjk-level-editor: {error}");
            return ExitCode::FAILURE;
        }
    };
    let (state, scene) = match state::EditorState::new(setup) {
        Ok(drawn) => drawn,
        Err(error) => {
            eprintln!("kjk-level-editor: the area does not draw: {error}");
            return ExitCode::FAILURE;
        }
    };
    run(runtime::RunOptions {
        commands,
        script,
        headless: cli.headless,
        size,
        games: Box::new(|| serde_json::Value::Null),
        scene: runtime::Scene::Loaded(Box::new(scene)),
        plugins: Some(Box::new(move |app| ui::install(app, state))),
    })
}

fn run(options: runtime::RunOptions) -> ExitCode {
    match runtime::run(options) {
        bevy::app::AppExit::Success => ExitCode::SUCCESS,
        bevy::app::AppExit::Error(code) => ExitCode::from(code.get()),
    }
}

/// The `kjk` program beside this one: what plays and audits a project.
fn kjk_program() -> Result<PathBuf, String> {
    let me = std::env::current_exe().map_err(|error| error.to_string())?;
    Ok(me.with_file_name(format!("kjk{}", std::env::consts::EXE_SUFFIX)))
}

/// The project in `folder` made for `game`'s `module`: its entry area's GIT
/// as the install has it, and no kjk-only content.
fn make(folder: &Path, game: Game, module: &str) -> Result<Project, String> {
    let discovery = kjk_game::discover();
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
    project.save(&format!("kjk {}", kjk_game::GIT_SHA))?;
    Ok(project)
}

/// Everything the editor starts from: the project (made first when there is
/// none), opened as this run's, the resolver with its folder, and the lists
/// the asset picker shows.
fn setup(game: &str, module: &str, project: Option<&str>) -> Result<state::EditSetup, String> {
    let game = Game::from_key(game)
        .filter(|game| matches!(game, Game::Kotor1 | Game::Kotor2))
        .ok_or("the editor edits k1 or tsl")?;
    let module = module.to_ascii_lowercase();
    let name = project.unwrap_or(&module);
    let folder = level_edit::folder_of(name, |var| std::env::var(var).ok())?;
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
    let discovery = kjk_game::discover();
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
    let scripts = sorted(ResType::NCS);
    let items = sorted(ResType::UTI);
    let waypoints = sorted(ResType::UTW);
    let triggers = sorted(ResType::UTT);
    let sounds = sorted(ResType::UTS);
    let stores = sorted(ResType::UTM);
    let ja = level_edit::active::ja_stack(&discovery);
    let models = ja.as_ref().map_or_else(Vec::new, |stack| {
        level_edit::active::models_under(stack, "models/map_objects/")
    });
    Ok(state::EditSetup {
        project: active.project.clone(),
        game,
        resolver,
        ja,
        placeables,
        creatures,
        models,
        scripts,
        items,
        waypoints,
        triggers,
        sounds,
        stores,
        program: kjk_program()?,
    })
}
