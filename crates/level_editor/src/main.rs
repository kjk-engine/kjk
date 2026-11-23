//! `kjk-level-editor`: kjk's level editor (`docs/LEVEL-EDIT.md`), a program
//! of its own built on kjk's game crate. It opens a KOTOR module's area
//! with a toolbar, an outliner, a details panel and an asset picker, and
//! saves what the user changes into a project, which `kjk play --project
//! NAME` plays. It never writes into a game install. Started with no
//! module, it lists the projects and the levels to pick from (`picker`).

mod agent;
mod bake;
mod elsewhere;
mod file;
mod ja_rooms;
mod lamps;
mod markers;
mod picker;
mod recover;
mod script_edit;
mod script_view;
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
    /// pick from. `ja` opens a Jedi Academy level to divide it into rooms
    game: Option<String>,
    /// The module, e.g. ebo_m12aa; for `ja`, a level such as t1_sour
    module: Option<String>,
    /// The project's name: the module's when left out. A project that does
    /// not exist is made from the module's area as the install has it
    #[arg(long)]
    project: Option<String>,
    /// Make a new map named MODULE, its area's light, fog and sound taken
    /// from this module's area (docs/LEVEL-EDIT.md, "New maps")
    #[arg(long, value_name = "BASE")]
    new_map: Option<String>,
    /// The name the games show for a new map; its module's name when left
    /// out
    #[arg(long)]
    title: Option<String>,
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
                asks_before_closing: false,
            });
        }
        _ => {
            eprintln!("kjk-level-editor: give a game and a module, or neither to pick a level");
            return ExitCode::from(2);
        }
    };
    // A Jedi Academy level is opened to be divided into rooms.
    if game.eq_ignore_ascii_case("ja") {
        let (rooms, scene) = match ja_rooms::Rooms::open(&module, cli.project.as_deref()) {
            Ok(opened) => opened,
            Err(error) => {
                eprintln!("kjk-level-editor: {error}");
                return ExitCode::FAILURE;
            }
        };
        return run(runtime::RunOptions {
            commands,
            script,
            headless: cli.headless,
            size,
            games: Box::new(|| serde_json::Value::Null),
            scene: runtime::Scene::Loaded(Box::new(scene)),
            plugins: Some(Box::new(move |app| ja_rooms::install(app, rooms))),
            asks_before_closing: false,
        });
    }
    let new_map = cli
        .new_map
        .as_deref()
        .map(|base| (base, cli.title.as_deref().unwrap_or(module.as_str())));
    let setup = match setup(&game, &module, cli.project.as_deref(), new_map) {
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
    let headless = cli.headless;
    run(runtime::RunOptions {
        commands,
        script,
        headless,
        size,
        games: Box::new(|| serde_json::Value::Null),
        scene: runtime::Scene::Loaded(Box::new(scene)),
        plugins: Some(Box::new(move |app| ui::install(app, state, headless))),
        asks_before_closing: true,
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
    let (area, git) = install_area(game, module)?;
    let project = Project::new(folder, game.key(), module, &area, git);
    project.save(&format!("kjk {}", kjk_game::GIT_SHA))?;
    Ok(project)
}

/// `module`'s entry area as the install has it: its resref and its GIT.
fn install_area(game: Game, module: &str) -> Result<(String, fmt_odyssey::gff::Gff), String> {
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
    Ok((area.resref.clone(), git))
}

/// The project in `folder` that makes the new map `module`: no rooms and no
/// objects yet, its area starting from `base`'s entry area (its ARE and its
/// GIT's area properties). Refused when the install has a module of that
/// name.
fn make_map(
    folder: &Path,
    game: Game,
    module: &str,
    (base, title): (&str, &str),
    into: Option<Project>,
) -> Result<Project, String> {
    let discovery = kjk_game::discover();
    let install = discovery
        .get(game)
        .ok_or(format!("{} is not installed", game.key()))?;
    let index = vfs::DirIndex::of_install(install).map_err(|error| error.to_string())?;
    if Resolver::modules(&index).iter().any(|name| name == module) {
        return Err(format!("the install already has a module named {module}"));
    }
    let resolver = Resolver::open(game, &index, Some(base), TextureQuality::High)
        .map_err(|error| format!("{base}: {error:?}"))?;
    let ir = ModuleIr::build(base, &ResolverSource(&resolver))
        .map_err(|refusal| format!("{base}: {refusal:?}"))?;
    let area = ir
        .areas
        .iter()
        .find(|area| area.resref == ir.entry_area)
        .or(ir.areas.first())
        .ok_or(format!("{base}: no area"))?;
    let gff = |restype| {
        let bytes = resolver
            .read(&area.resref, restype)
            .ok_or(format!("{base}: no {}.{restype}", area.resref))?;
        fmt_odyssey::gff::Gff::parse(&bytes)
            .map_err(|error| format!("{}.{restype}: {error}", area.resref))
    };
    let (are, git) = (gff(ResType::ARE)?, gff(ResType::GIT)?);
    // A project that has levels takes the map as a further one.
    let project = match into {
        Some(mut project) => {
            project.add_map(module, title, (base, are, &git))?;
            project
        }
        None => Project::new_map(folder, game.key(), module, title, (base, are, &git))?,
    };
    project.save(&format!("kjk {}", kjk_game::GIT_SHA))?;
    project.save_module(&|_, _| None)?;
    Ok(project)
}

/// The fitted lights' rule data (`data/lights`) that lights placed by hand
/// are saved to: the folder `KJK_LIGHT_DATA` names, else the one of the
/// kjk checkout this program was built in (`target/<profile>/` lies in
/// it).
fn light_data() -> Option<PathBuf> {
    if let Some(folder) = std::env::var_os("KJK_LIGHT_DATA").filter(|folder| !folder.is_empty()) {
        return Some(PathBuf::from(folder));
    }
    let me = std::env::current_exe().ok()?;
    me.ancestors()
        .map(|folder| folder.join("data").join("lights"))
        .find(|folder| folder.join("fittings.toml").is_file())
}

/// The kjk checkout `program` (`target/<profile>/kjk`) was built in.
pub(crate) fn checkout_of(program: &Path) -> Option<PathBuf> {
    program
        .ancestors()
        .find(|folder| folder.join("Cargo.toml").is_file() && folder.join("data").is_dir())
        .map(Path::to_path_buf)
}

/// The bakes of `area` made in the kjk checkout at `root`, by
/// `tools/light-kinds` or the editor
/// (`target/light-kinds/game/<folder>/bake/<label>/<area>/b`), the newest
/// first.
pub(crate) fn bakes_of(root: &Path, area: &str) -> Vec<PathBuf> {
    let games = root.join("target").join("light-kinds").join("game");
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for folder in std::fs::read_dir(games).into_iter().flatten().flatten() {
        for label in std::fs::read_dir(folder.path().join("bake"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let bake = label.path().join(area).join("b");
            if bake.is_dir() {
                let when = std::fs::metadata(&bake)
                    .and_then(|meta| meta.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                found.push((when, bake));
            }
        }
    }
    found.sort_by_key(|(when, _)| std::cmp::Reverse(*when));
    found.into_iter().map(|(_, bake)| bake).collect()
}

/// Every room of the install that a new map can place: each room of every
/// area's layout that has a model, the first layout's place for it.
fn room_library(resolver: &Resolver) -> Vec<state::LibraryRoom> {
    let mut layouts = resolver.names(ResType::LYT);
    layouts.sort();
    layouts.dedup();
    let mut rooms: Vec<state::LibraryRoom> = Vec::new();
    for area in layouts {
        let Some(layout) = resolver
            .read(&area, ResType::LYT)
            .and_then(|bytes| fmt_odyssey::lyt::Layout::parse(&bytes).ok())
        else {
            continue;
        };
        for placed in layout.rooms {
            if fmt_odyssey::is_blank(placed.model.as_bytes())
                || rooms.iter().any(|room| room.model == placed.model)
                || resolver.find(&placed.model, ResType::MDL).is_none()
            {
                continue;
            }
            rooms.push(state::LibraryRoom {
                model: placed.model,
                from: placed.position,
                area: area.clone(),
                ja: None,
            });
        }
    }
    rooms.sort_by(|a, b| a.model.cmp(&b.model));
    rooms
}

/// Everything the editor starts from: the project (made first when there is
/// none), opened as this run's, the resolver with its folder, and the lists
/// the asset picker shows. `new_map` (a base module and a title) makes a
/// project that does not exist a new map's.
fn setup(
    game: &str,
    module: &str,
    project: Option<&str>,
    new_map: Option<(&str, &str)>,
) -> Result<state::EditSetup, String> {
    let game = Game::from_key(game)
        .filter(|game| matches!(game, Game::Kotor1 | Game::Kotor2))
        .ok_or("the editor edits k1 or tsl")?;
    let module = module.to_ascii_lowercase();
    let name = project.unwrap_or(&module);
    let folder = level_edit::folder_of(name, |var| std::env::var(var).ok())?;
    if Project::exists(&folder) {
        let mut found = Project::open(&folder)?;
        if found.game != game.key() {
            return Err(format!(
                "project {name} edits {}, not {}",
                found.game,
                game.key()
            ));
        }
        // A level the project does not edit yet is added to it: a new
        // map when one is asked for, else made from the install's area.
        if !found.level_list().iter().any(|(level, _)| *level == module) {
            if let Some(new_map) = new_map {
                make_map(&folder, game, &module, new_map, Some(found))?;
                eprintln!("added the new map {module} to project {name}");
            } else {
                found.switch_to(&module, || install_area(game, &module))?;
                found.save(&format!("kjk {}", kjk_game::GIT_SHA))?;
                eprintln!("added the level {module} to project {name}");
            }
        }
    } else if let Some(new_map) = new_map {
        make_map(&folder, game, &module, new_map, None)?;
        eprintln!("made the new map {module} in {}", folder.display());
    } else {
        make(&folder, game, &module)?;
        eprintln!("made project {name} in {}", folder.display());
    }
    let discovery = kjk_game::discover();
    let active = level_edit::active::activate(&discovery, &folder)?;
    for note in &active.notes {
        eprintln!("project {name}: {note}");
    }
    // The rooms the project cut from Jedi Academy levels, built into
    // memory before the resolver opens, so that it finds them as it finds
    // the install's rooms.
    let started = std::time::Instant::now();
    let (built, made_notes) = kjk_game::ja_made::build_kept(&discovery, game, &folder);
    for note in &made_notes {
        eprintln!("project {name}: {note}");
    }
    for room in &built {
        eprintln!(
            "project {name}: built the Jedi Academy room {:?} of {} as {}, with {} triangles drawn and {} of floor",
            room.ja.room, room.ja.level, room.resref, room.triangles, room.floor
        );
    }
    if !built.is_empty() {
        eprintln!(
            "project {name}: built {} Jedi Academy rooms in {:.1} s",
            built.len(),
            started.elapsed().as_secs_f32()
        );
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
    let catalog = script_view::catalog(&resolver);
    let talk = resolver.talk.get();
    let notes = resolver
        .read("nwscript", ResType::NSS)
        .map(|bytes| bytes.into_iter().map(char::from).collect::<String>())
        .map_or_else(std::collections::BTreeMap::new, |text| {
            script_ncs::catalog::notes(&text)
        });
    let items = sorted(ResType::UTI);
    let waypoints = sorted(ResType::UTW);
    let doors = sorted(ResType::UTD);
    // The globals scripts may keep (`globalcat.2da`): name and type.
    let globals = resolver.two_da("globalcat").map_or_else(Vec::new, |table| {
        let mut globals: Vec<(String, String)> = (0..table.row_labels.len())
            .filter_map(|row| {
                let name = table.text(row, "name")?;
                let kind = table.text(row, "type")?.to_ascii_lowercase();
                (!name.is_empty() && name != "****").then_some((name, kind))
            })
            .collect();
        globals.sort();
        globals
    });
    let modules = discovery
        .get(game)
        .and_then(|install| vfs::DirIndex::of_install(install).ok())
        .map_or_else(Vec::new, |index| {
            let mut names = kjk_game::modules::module_names(&index);
            names
                .iter_mut()
                .for_each(|name| *name = name.to_ascii_lowercase());
            // The project's own levels, its new maps among them, are led
            // to as the game's are.
            names.extend(
                active
                    .project
                    .level_list()
                    .into_iter()
                    .map(|(module, _)| module),
            );
            names.sort();
            names.dedup();
            names
        });
    let triggers = sorted(ResType::UTT);
    let sounds = sorted(ResType::UTS);
    let stores = sorted(ResType::UTM);
    let encounters = sorted(ResType::UTE);
    let mut rooms = room_library(&resolver);
    rooms.extend(built.into_iter().map(|room| state::LibraryRoom {
        model: room.resref,
        from: room.from,
        area: room.ja.level.clone(),
        ja: Some(room.ja),
    }));
    let light_data = light_data();
    let program = kjk_program()?;
    let tools = has_tools(&program);
    // The level edited now: the one asked for, of the project's levels.
    let mut project = active.project.clone();
    project.switch_to(&module, || {
        Err(format!("project {name} has no level {module}"))
    })?;
    let bakes = checkout_of(&program).map_or_else(Vec::new, |root| bakes_of(&root, &project.area));
    let ja = level_edit::active::ja_stack(&discovery);
    let models = ja.as_ref().map_or_else(Vec::new, |stack| {
        level_edit::active::models_under(stack, "models/map_objects/")
    });
    Ok(state::EditSetup {
        project,
        game,
        resolver,
        ja,
        placeables,
        creatures,
        models,
        scripts,
        catalog,
        notes,
        talk,
        items,
        waypoints,
        doors,
        modules,
        globals,
        triggers,
        sounds,
        stores,
        encounters,
        rooms,
        light_data,
        bakes,
        program,
        tools,
    })
}

/// Whether `program` (kjk) has the verbs that check and measure: its help
/// names `detail-bake` only in a build with the `probes` feature.
fn has_tools(program: &Path) -> bool {
    std::process::Command::new(program)
        .arg("--help")
        .output()
        .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("detail-bake"))
}
