//! What `kjk-level-editor` shows when it is started with no module: the
//! projects made so far, and every module of each KOTOR game installed,
//! with its entry area's name, to pick one to edit. When Jedi Academy is
//! installed, its levels are listed too, to cut into rooms (`ja_rooms`).
//! A project that keeps a Jedi Academy level's rooms is listed with that
//! level, beside its KOTOR level if it has one.
//!
//! A project is opened once in a process, before any install is read
//! (`level_edit::active`), so the picker does not load the level itself. It
//! starts the editor again, as a program of its own, on what was picked,
//! and closes.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use fmt_odyssey::ResType;
use vfs::Game;

use kjk_game::scene::FlyControls;

/// A module the picker lists.
struct Module {
    resref: String,
    /// Its entry area's name, once read (`names`); `None` until then, and
    /// for one that has none.
    name: Option<String>,
}

/// A project found in the projects' folder.
#[derive(Debug, Clone)]
pub(crate) struct Found {
    pub(crate) name: String,
    pub(crate) game: String,
    pub(crate) module: String,
    /// When its folder last changed, for the newest first.
    changed: Option<std::time::SystemTime>,
}

/// The picker's state.
#[derive(Resource)]
struct Picker {
    /// Each KOTOR game installed, with its modules.
    games: Vec<(Game, Vec<Module>)>,
    tab: usize,
    projects: Vec<Found>,
    /// The modules' names as the thread reads them.
    names: std::sync::Mutex<Receiver<(Game, String, String)>>,
    search: String,
    /// The module picked: its game and resref.
    picked: Option<(Game, String)>,
    project: String,
    /// The name of a new map to make from the picked level.
    new_map: String,
    message: Option<String>,
    headless: bool,
    fonts_set: bool,
}

/// Adds the picker to the game's app.
pub fn install(app: &mut App, headless: bool) {
    let discovery = kjk_game::discover();
    let mut games = Vec::new();
    let mut indexes = Vec::new();
    for game in [Game::Kotor1, Game::Kotor2] {
        let Some(install) = discovery.get(game) else {
            continue;
        };
        let Ok(index) = vfs::DirIndex::of_install(install) else {
            continue;
        };
        let modules = vfs::resolver::Resolver::modules(&index)
            .into_iter()
            .map(|resref| Module { resref, name: None })
            .collect();
        games.push((game, modules));
        indexes.push((game, index));
    }
    // Jedi Academy's levels, to cut into rooms. A level is named as the
    // game names it: its path without `maps/` and `.bsp`.
    if let Ok(stack) = kjk_game::viewer_ja::stack(&discovery) {
        let files = kjk_game::viewer_ja::Files::new(&stack);
        let mut levels: Vec<Module> = (files.names())
            .filter_map(|name| name.strip_prefix("maps/")?.strip_suffix(".bsp"))
            .map(|level| Module {
                resref: level.to_string(),
                name: None,
            })
            .collect();
        levels.sort_by(|a, b| a.resref.cmp(&b.resref));
        if !levels.is_empty() {
            games.push((Game::JediAcademy, levels));
        }
    }
    let (send, names) = channel();
    std::thread::Builder::new()
        .name("module names".into())
        .spawn(move || {
            for (game, index) in indexes {
                read_names(game, &index, &send);
            }
        })
        .expect("the names' thread starts");
    app.insert_resource(Picker {
        games,
        tab: 0,
        projects: projects(),
        names: std::sync::Mutex::new(names),
        search: String::new(),
        picked: None,
        project: String::new(),
        new_map: String::new(),
        message: None,
        headless,
        fonts_set: false,
    })
    .insert_resource(kjk_game::rolloff::LightingMode::Enhanced)
    .insert_resource(FlyControls {
        keys_need_look: true,
        paused: true,
        keys: None,
    })
    .insert_resource(bevy_egui::EguiGlobalSettings {
        auto_create_primary_context: false,
        ..default()
    })
    .add_plugins(EguiPlugin::default())
    .add_systems(Startup, crate::ui::egui_writes_gamma)
    .add_systems(Update, (crate::ui::attach_egui, take_names, commands))
    .add_systems(EguiPrimaryContextPass, window)
    .add_systems(Update, publish);
}

/// Each module's entry area's name (its ARE's `Name`), else the module's
/// own (its IFO's `Mod_Name`), sent as it is read.
fn read_names(
    game: Game,
    index: &vfs::DirIndex,
    send: &std::sync::mpsc::Sender<(Game, String, String)>,
) {
    let Ok(base) = vfs::resolver::Resolver::base(game, index, vfs::resolver::TextureQuality::High)
    else {
        return;
    };
    let talk = base.talk.get();
    let text = |loc: Option<&fmt_odyssey::gff::LocString>| -> Option<String> {
        let loc = loc?;
        if let Some((_, own)) = loc.strings.first()
            && !own.is_empty()
        {
            return Some(fmt_odyssey::code_page(own));
        }
        let line = talk.as_ref()?.text(loc.strref)?;
        (!line.is_empty()).then(|| fmt_odyssey::code_page(line))
    };
    for resref in vfs::resolver::Resolver::modules(index) {
        let Ok(resolver) = base.with_module(index, &resref) else {
            continue;
        };
        let Some(ifo) = resolver
            .read("module", ResType::IFO)
            .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
        else {
            continue;
        };
        let area = ifo.root.resref("Mod_Entry_Area").and_then(|area| {
            let bytes = resolver.read(&area, ResType::ARE)?;
            let are = fmt_odyssey::gff::Gff::parse(&bytes).ok()?;
            text(are.root.locstring("Name"))
        });
        let name = area.or_else(|| text(ifo.root.locstring("Mod_Name")));
        if let Some(name) = name
            && send.send((game, resref, name.trim().to_string())).is_err()
        {
            return;
        }
    }
}

/// The Jedi Academy levels the projects keep rooms of
/// (`level_edit::ja_rooms`), each as a project of the game `ja`.
fn ja_projects(folder: &std::path::Path) -> Vec<Found> {
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (level_edit::ja_rooms::kept(&entry.path()).into_iter()).map(move |(level, changed)| {
                Found {
                    name: name.clone(),
                    game: Game::JediAcademy.key().to_string(),
                    module: level_edit::ja_rooms::short(&level).to_string(),
                    changed,
                }
            })
        })
        .collect()
}

/// The projects in the projects' folder, the newest first.
pub(crate) fn projects() -> Vec<Found> {
    let Some(folder) = level_edit::projects_folder(|var| std::env::var(var).ok()) else {
        return Vec::new();
    };
    let mut found: Vec<Found> = std::fs::read_dir(&folder)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let project = level_edit::Project::open(&path).ok()?;
            let changed = std::fs::metadata(path.join(level_edit::project::PROJECT_FILE))
                .and_then(|meta| meta.modified())
                .ok();
            let changed = [
                level_edit::project::AREA_FILE,
                &format!("{}.git", project.area),
            ]
            .iter()
            .filter_map(|file| {
                std::fs::metadata(path.join(file))
                    .and_then(|m| m.modified())
                    .ok()
            })
            .chain(changed)
            .max();
            Some(Found {
                name: project.name,
                game: project.game,
                module: project.module,
                changed,
            })
        })
        .collect();
    found.extend(ja_projects(&folder));
    found.sort_by(|a, b| b.changed.cmp(&a.changed).then(a.name.cmp(&b.name)));
    found
}

fn take_names(mut picker: ResMut<Picker>) {
    let picker = &mut *picker;
    let names: Vec<(Game, String, String)> = match picker.names.lock() {
        Ok(receiver) => receiver.try_iter().collect(),
        Err(_) => Vec::new(),
    };
    for (game, resref, name) in names {
        if let Some((_, modules)) = picker.games.iter_mut().find(|(g, _)| *g == game)
            && let Some(module) = modules.iter_mut().find(|m| m.resref == resref)
        {
            module.name = Some(name);
        }
    }
}

/// The editor started on `game`'s `module`, in project `project`, and the
/// picker closed; with `base`, on a new map named `module` whose area
/// starts from `base`'s. A Jedi Academy level is started in the rooms tool.
/// Any project keeps its rooms, whatever game the project edits. A
/// headless picker (a check) names what it would start.
fn open(
    picker: &mut Picker,
    exit: &mut MessageWriter<AppExit>,
    (game, module, project): (Game, &str, &str),
    base: Option<&str>,
) {
    if base.is_some() && game == Game::JediAcademy {
        picker.message = Some(
            "A new map starts from a level of KOTOR 1 or KOTOR 2. Pick one of those.".to_string(),
        );
        return;
    }
    if base.is_some()
        && let Some(problem) = level_edit::map::name_problem(module)
    {
        picker.message = Some(format!("{problem}."));
        return;
    }
    let folder = match level_edit::folder_of(project, |var| std::env::var(var).ok()) {
        Ok(folder) => folder,
        Err(why) => {
            picker.message = Some(why);
            return;
        }
    };
    // A project of another game cannot take the level. Any other project
    // takes a level of its game beside the ones it has (`main::setup`).
    if game != Game::JediAcademy
        && let Ok(found) = level_edit::Project::open(&folder)
        && found.game != game.key()
    {
        picker.message = Some(format!(
            "The project {project} edits {} {}. Give this one another name.",
            found.game, found.module
        ));
        return;
    }
    let mut args = vec![game.key(), module, "--project", project];
    if let Some(base) = base {
        args.extend(["--new-map", base]);
    }
    if picker.headless {
        let message = format!("would start kjk-level-editor {}", args.join(" "));
        info!(target: "editor", "{message}");
        picker.message = Some(message);
        return;
    }
    let me: Result<PathBuf, String> = std::env::current_exe().map_err(|error| error.to_string());
    match me.and_then(|me| {
        std::process::Command::new(me)
            .args(args)
            .spawn()
            .map_err(|error| error.to_string())
    }) {
        Ok(_) => {
            exit.write(AppExit::Success);
        }
        Err(why) => picker.message = Some(format!("The editor did not start: {why}")),
    }
}

/// `edit open GAME MODULE [PROJECT]` and `edit search TEXT`, for checks.
fn commands(
    mut picker: ResMut<Picker>,
    mut requests: ResMut<console::EditRequests>,
    mut exit: MessageWriter<AppExit>,
) {
    for words in std::mem::take(&mut requests.0) {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["open", game, module, rest @ ..] => {
                let Some(game) = Game::from_key(game) else {
                    picker.message = Some(format!("{game} is not k1, tsl or ja"));
                    continue;
                };
                let project =
                    (rest.first()).map_or_else(|| project_for(module), |name| name.to_string());
                open(&mut picker, &mut exit, (game, module, &project), None);
            }
            ["new", game, base, name] => {
                let Some(game) = Game::from_key(game) else {
                    picker.message = Some(format!("{game} is not k1 or tsl"));
                    continue;
                };
                open(&mut picker, &mut exit, (game, name, name), Some(base));
            }
            ["search", rest @ ..] => picker.search = rest.join(" "),
            ["tab", tab] => {
                if let Some(at) = picker.games.iter().position(|(game, _)| game.key() == *tab) {
                    picker.tab = at;
                }
            }
            _ => picker.message = Some(format!("edit {}: not a picker command", words.join(" "))),
        }
    }
}

/// The project's name a level starts with: the level's own, a folder in
/// it joined by `-` (`mp/ffa3` is `mp-ffa3`), since a project's name is a
/// folder's name.
fn project_for(level: &str) -> String {
    level.replace('/', "-")
}

fn game_label(game: Game) -> &'static str {
    match game {
        Game::Kotor1 => "KOTOR 1",
        Game::Kotor2 => "KOTOR 2",
        Game::JediAcademy => "Jedi Academy",
    }
}

fn window(
    mut contexts: EguiContexts,
    mut picker: ResMut<Picker>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let ctx = ctx.clone();
    let picker = &mut *picker;
    if !picker.fonts_set {
        picker.fonts_set = true;
        if let Ok(fonts) = save_editor::model::fonts::system() {
            crate::ui::use_fonts(&ctx, fonts);
        }
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "picker".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    let mut opening: Option<(Game, String, String)> = None;
    // A new map to make: its game, the level it starts from and its name.
    let mut making: Option<(Game, String, String)> = None;
    egui::Panel::left("projects")
        .resizable(true)
        .default_size(320.0)
        .show(&mut root, |ui| {
            ui.heading("Projects");
            if picker.projects.is_empty() {
                ui.label("No project yet. Pick a level on the right to make one.");
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                for project in &picker.projects {
                    let game = Game::from_key(&project.game);
                    // The level's name, once the thread has read it.
                    let level = picker
                        .games
                        .iter()
                        .find(|(g, _)| Some(*g) == game)
                        .and_then(|(_, modules)| {
                            modules.iter().find(|m| m.resref == project.module)
                        })
                        .and_then(|m| m.name.clone());
                    let under = format!(
                        "{}   {}{}",
                        game.map_or(project.game.clone(), |game| game_label(game).to_string()),
                        level.map_or(String::new(), |name| format!("{name}, ")),
                        project.module
                    );
                    let mut text = egui::text::LayoutJob::default();
                    text.append(
                        &project.name,
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(15.0),
                            color: ui.visuals().strong_text_color(),
                            ..Default::default()
                        },
                    );
                    text.append(
                        &format!("\n{under}"),
                        0.0,
                        egui::TextFormat {
                            font_id: egui::FontId::proportional(12.0),
                            color: ui.visuals().weak_text_color(),
                            ..Default::default()
                        },
                    );
                    let row = ui.add(
                        egui::Button::new(text)
                            .wrap_mode(egui::TextWrapMode::Wrap)
                            .min_size(egui::vec2(ui.available_width(), 0.0)),
                    );
                    if row.clicked()
                        && let Some(game) = game
                    {
                        opening = Some((game, project.module.clone(), project.name.clone()));
                    }
                }
            });
        });
    egui::Panel::bottom("open").show(&mut root, |ui| {
        ui.horizontal(|ui| {
            match &picker.picked {
                Some((game, module)) => {
                    ui.label(format!("{} {module}", game_label(*game)));
                }
                None => {
                    ui.label("Pick a level.");
                }
            }
            ui.separator();
            ui.label("Project");
            ui.add(egui::TextEdit::singleline(&mut picker.project).desired_width(200.0));
            let ready = picker.picked.is_some() && level_edit::valid_name(&picker.project);
            if ui.add_enabled(ready, egui::Button::new("Open")).clicked()
                && let Some((game, module)) = picker.picked.clone()
            {
                opening = Some((game, module, picker.project.clone()));
            }
            if let Some(message) = &picker.message {
                ui.separator();
                ui.label(message);
            }
        });
        ui.horizontal(|ui| {
            ui.label("New map named");
            ui.add(
                egui::TextEdit::singleline(&mut picker.new_map)
                    .hint_text("my_map")
                    .desired_width(140.0),
            );
            let kotor =
                (picker.picked.as_ref()).is_some_and(|(game, _)| *game != Game::JediAcademy);
            let problem = level_edit::map::name_problem(&picker.new_map);
            let ready = kotor && problem.is_none();
            // Why Make cannot be pressed yet, said beside it.
            let why = if !kotor {
                Some(
                    "Pick a KOTOR level first. The new map takes its light, fog and sound."
                        .to_string(),
                )
            } else if picker.new_map.is_empty() {
                None
            } else {
                problem.map(|problem| format!("This name does not work, because {problem}."))
            };
            let make = ui
                .add_enabled(ready, egui::Button::new("Make"))
                .on_hover_text(
                    "Makes a map of your own, built from the game's rooms. Its light, \
                     fog and sound start as the picked level's.",
                )
                .on_disabled_hover_text(why.clone().unwrap_or_else(|| {
                    "Name the map first. A name has lowercase letters, digits and _, up to 12."
                        .into()
                }));
            if let Some(why) = &why {
                ui.label(
                    egui::RichText::new(why)
                        .small()
                        .color(ui.visuals().warn_fg_color),
                );
            }
            if make.clicked()
                && let Some((game, module)) = picker.picked.clone()
            {
                let name = picker.new_map.clone();
                making = Some((game, module, name));
            }
            ui.label(
                egui::RichText::new("lowercase letters, digits and _, up to 12")
                    .small()
                    .weak(),
            );
        });
    });
    egui::CentralPanel::default().show(&mut root, |ui| {
        ui.horizontal(|ui| {
            for at in 0..picker.games.len() {
                let label = game_label(picker.games[at].0);
                ui.selectable_value(&mut picker.tab, at, label);
            }
            ui.separator();
            ui.add(
                egui::TextEdit::singleline(&mut picker.search)
                    .hint_text("Search names and modules")
                    .desired_width(260.0),
            );
        });
        let Some((game, modules)) = picker.games.get(picker.tab) else {
            ui.label("Neither KOTOR 1, KOTOR 2 nor Jedi Academy is installed.");
            return;
        };
        let game = *game;
        let search = picker.search.to_ascii_lowercase();
        let rows: Vec<(&str, &str)> = modules
            .iter()
            .map(|m| (m.resref.as_str(), m.name.as_deref().unwrap_or("")))
            .filter(|(resref, name)| {
                search.is_empty()
                    || resref.contains(&search)
                    || name.to_ascii_lowercase().contains(&search)
            })
            .collect();
        let mut clicked: Option<(String, bool)> = None;
        egui::ScrollArea::vertical().auto_shrink(false).show_rows(
            ui,
            22.0,
            rows.len(),
            |ui, range| {
                for (resref, name) in &rows[range] {
                    let picked = picker.picked.as_ref() == Some(&(game, resref.to_string()));
                    let label = if name.is_empty() {
                        resref.to_string()
                    } else {
                        format!("{name}   ({resref})")
                    };
                    let row = ui.selectable_label(picked, label);
                    if row.clicked() {
                        clicked = Some((resref.to_string(), false));
                    }
                    if row.double_clicked() {
                        clicked = Some((resref.to_string(), true));
                    }
                }
            },
        );
        if let Some((resref, now)) = clicked {
            picker.project = project_for(&resref);
            picker.picked = Some((game, resref.clone()));
            if now {
                opening = Some((game, resref.clone(), project_for(&resref)));
            }
        }
    });
    if let Some((game, base, name)) = making {
        open(picker, &mut exit, (game, &name, &name), Some(&base));
    }
    if let Some((game, module, project)) = opening {
        open(picker, &mut exit, (game, &module, &project), None);
    }
}

/// The picker's facts for `dump`.
fn publish(picker: Res<Picker>, mut facts: ResMut<console::DumpFacts>) {
    if !facts.wanted() {
        return;
    }
    facts.0.insert(
        "picker".into(),
        serde_json::json!({
            "games": picker.games.iter().map(|(game, modules)| serde_json::json!({
                "game": game.key(),
                "modules": modules.len(),
                "named": modules.iter().filter(|m| m.name.is_some()).count(),
            })).collect::<Vec<_>>(),
            "projects": picker.projects.iter().map(|p| serde_json::json!({
                "name": p.name, "game": p.game, "module": p.module,
            })).collect::<Vec<_>>(),
            "message": picker.message,
        }),
    );
}
