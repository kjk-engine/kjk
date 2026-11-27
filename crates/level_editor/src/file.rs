//! The toolbar's File menu, and what leaving the level asks first.
//!
//! A project is opened once in a process (`level_edit::active`), so
//! another level is opened by starting the editor again on it, as the
//! picker does, and closing this one. Leaving a level with changes that are
//! not saved, by the menu or by closing the window, first asks whether to
//! save them. A headless run (a check) names what it would start instead
//! of starting it.

use bevy::prelude::*;
use bevy::window::WindowCloseRequested;
use bevy_egui::egui;

use crate::state::EditorState;
use crate::ui::UiState;

/// What leaving the level does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Leave {
    /// The editor closes.
    Quit,
    /// The editor starts again with these arguments, and this one closes.
    Start(Vec<String>),
}

/// What the File menu and the prompt hold.
#[derive(Default)]
pub(crate) struct FileState {
    /// What is waiting on the prompt's answer.
    pub(crate) asking: Option<Leave>,
    /// What leaves once the frame is done (`leave`).
    pub(crate) leaving: Option<Leave>,
    /// The name typed for a new map while its dialog is open.
    pub(crate) new_map: Option<String>,
    /// The projects of the projects' folder, and when they were read. They
    /// are read again once they are `RECENT_AGE` old, so a project made
    /// since shows.
    recent: Option<(std::time::Instant, Vec<crate::picker::Found>)>,
    /// What the Add a level menu's filter holds.
    level_filter: String,
}

/// How old the File menu's list of projects may be before it is read again.
const RECENT_AGE: std::time::Duration = std::time::Duration::from_secs(10);

/// Leaves by `leave`, asking first when there are changes not saved.
pub(crate) fn want(state: &EditorState, file: &mut FileState, leave: Leave) {
    if state.unsaved() {
        file.asking = Some(leave);
    } else {
        file.leaving = Some(leave);
    }
}

/// The arguments that open `project`, a project of the projects' folder.
fn open_args(found: &crate::picker::Found) -> Vec<String> {
    vec![
        found.game.clone(),
        found.module.clone(),
        "--project".into(),
        found.name.clone(),
    ]
}

/// The arguments that open the project being edited on its level
/// `module`, or add that level to it first (`main::setup`).
fn level_args(state: &EditorState, module: &str) -> Vec<String> {
    let project = &state.editor.project;
    vec![
        project.game.clone(),
        module.to_ascii_lowercase(),
        "--project".into(),
        project.name.clone(),
    ]
}

/// Opens the project's level `module`, adding it to the project when it
/// has no such level yet, or why not.
fn to_level(state: &EditorState, file: &mut FileState, module: &str) -> Result<(), String> {
    let module = module.to_ascii_lowercase();
    let project = &state.editor.project;
    if module == project.module {
        return Err(format!("{module} is the level being edited"));
    }
    if !project
        .level_list()
        .iter()
        .any(|(level, _)| *level == module)
        && state.setup.modules.binary_search(&module).is_err()
    {
        return Err(format!("the install has no module {module}"));
    }
    want(state, file, Leave::Start(level_args(state, &module)));
    Ok(())
}

/// The arguments that add the new map `name` to the project being edited
/// and open it, its area's light, fog and sound from the level being
/// edited (a new map's own base, for a new map), or why not.
fn new_map_args(state: &EditorState, name: &str) -> Result<Vec<String>, String> {
    if let Some(problem) = level_edit::map::name_problem(name) {
        return Err(problem);
    }
    let project = &state.editor.project;
    if project.level_list().iter().any(|(level, _)| level == name) {
        return Err(format!("the project has a level named {name} already"));
    }
    if state.setup.modules.binary_search(&name.to_string()).is_ok() {
        return Err(format!("the game has a module named {name} already"));
    }
    let base = project
        .map
        .as_ref()
        .map_or(project.module.clone(), |map| map.base.clone());
    Ok(vec![
        project.game.clone(),
        name.to_string(),
        "--project".into(),
        project.name.clone(),
        "--new-map".into(),
        base,
    ])
}

/// `edit ui file ...`, `edit ui quit` and `edit ui confirm ...`: done, or
/// why not. `None` for words that are none of them.
pub(crate) fn command(
    state: &mut EditorState,
    file: &mut FileState,
    words: &[&str],
) -> Option<Result<(), String>> {
    Some(match words {
        ["file", "open"] => {
            want(state, file, Leave::Start(Vec::new()));
            Ok(())
        }
        ["file", "open", name] => crate::picker::projects()
            .into_iter()
            .find(|found| found.name.eq_ignore_ascii_case(name))
            .map(|found| want(state, file, Leave::Start(open_args(&found))))
            .ok_or(format!("there is no project named {name}")),
        ["file", "level", module] => to_level(state, file, module),
        ["file", "new", name] => {
            new_map_args(state, name).map(|args| want(state, file, Leave::Start(args)))
        }
        ["quit"] => {
            want(state, file, Leave::Quit);
            Ok(())
        }
        ["confirm", answer @ ("save" | "discard" | "cancel")] => answer_prompt(state, file, answer),
        _ => return None,
    })
}

/// The prompt answered: save and leave, leave without saving, or stay. A
/// save that fails leaves the level open, and the reason is the answer.
fn answer_prompt(
    state: &mut EditorState,
    file: &mut FileState,
    answer: &str,
) -> Result<(), String> {
    let Some(leave) = file.asking.take() else {
        return Ok(());
    };
    match answer {
        "save" => {
            crate::state::save(state)
                .map_err(|why| format!("not saved, so the level stays open: {why}"))?;
            file.leaving = Some(leave);
        }
        "discard" => file.leaving = Some(leave),
        _ => {}
    }
    Ok(())
}

/// The File menu, at the toolbar's start.
pub(crate) fn menu(ui: &mut egui::Ui, state: &mut EditorState, ui_state: &mut UiState) {
    let file = &mut ui_state.file;
    ui.menu_button("File", |ui| {
        if ui
            .button("Open level...")
            .on_hover_text("Lists the projects and the levels to pick from")
            .clicked()
        {
            want(state, file, Leave::Start(Vec::new()));
            ui.close();
        }
        let current = state.editor.project.name.clone();
        if (file.recent.as_ref()).is_none_or(|(read, _)| read.elapsed() > RECENT_AGE) {
            file.recent = Some((std::time::Instant::now(), crate::picker::projects()));
        }
        let recent = file.recent.as_ref().map_or(&[][..], |(_, found)| found);
        let others: Vec<crate::picker::Found> = recent
            .iter()
            .filter(|found| found.name != current)
            .take(10)
            .cloned()
            .collect();
        ui.menu_button("Open recent", |ui| {
            if others.is_empty() {
                ui.label("No other project yet.");
            }
            for found in others {
                if ui
                    .button(format!("{}   {} {}", found.name, found.game, found.module))
                    .clicked()
                {
                    want(state, file, Leave::Start(open_args(&found)));
                    ui.close();
                }
            }
        });
        // The project's levels, and another level of the install added.
        let project = &state.editor.project;
        {
            let levels = project.level_list();
            let editing = project.module.clone();
            ui.menu_button("Levels of this project", |ui| {
                for (module, area) in levels {
                    let label = format!("{module}   ({area})");
                    if module == editing {
                        ui.add_enabled(false, egui::Button::new(format!("{label}, edited now")));
                    } else if ui
                        .button(label)
                        .on_hover_text("Opens the project on this level")
                        .clicked()
                    {
                        if let Err(why) = to_level(state, file, &module) {
                            state.say(why);
                        }
                        ui.close();
                    }
                }
                ui.separator();
                ui.menu_button("Add a level", |ui| {
                    ui.label("A level of the install, edited in this project beside the others:");
                    ui.add(
                        egui::TextEdit::singleline(&mut file.level_filter)
                            .hint_text("filter, e.g. danm14")
                            .desired_width(180.0),
                    );
                    let needle = file.level_filter.to_ascii_lowercase();
                    let mut picked = None;
                    egui::ScrollArea::vertical()
                        .max_height(300.0)
                        .show(ui, |ui| {
                            for module in state
                                .setup
                                .modules
                                .iter()
                                .filter(|module| needle.is_empty() || module.contains(&needle))
                                .take(200)
                            {
                                if ui.button(module).clicked() {
                                    picked = Some(module.clone());
                                }
                            }
                        });
                    if let Some(module) = picked {
                        if let Err(why) = to_level(state, file, &module) {
                            state.say(why);
                        }
                        ui.close();
                    }
                });
            });
        }
        if ui
            .button("New map...")
            .on_hover_text(
                "A map of your own, added to this project. Its light, fog and sound are this level's.",
            )
            .clicked()
        {
            file.new_map = Some(String::new());
            ui.close();
        }
        ui.separator();
        let save_keys = state.keys.map.shown(crate::keys::Action::Save);
        if ui.button(format!("Save    {save_keys}")).clicked() {
            state.pending_command = Some("save".into());
            ui.close();
        }
        if ui
            .button("Export as a mod")
            .on_hover_text(
                "Saves, then writes the project as a mod of the game to the exports folder in kjk's own folder: files to copy into the game's override folder, which KOTOR and kjk both read.",
            )
            .clicked()
        {
            state.pending_command = Some("export".into());
            ui.close();
        }
        ui.separator();
        if ui
            .button("Keys...")
            .on_hover_text("Shows which keys do what, and changes them.")
            .clicked()
        {
            ui_state.keys_window.open = true;
            ui.close();
        }
        if ui.button("Quit").clicked() {
            want(state, file, Leave::Quit);
            ui.close();
        }
    });
}

/// The prompt and the New map dialog, over everything.
pub(crate) fn dialogs(ctx: &egui::Context, state: &mut EditorState, file: &mut FileState) {
    if file.asking.is_some() {
        let mut answer = None;
        egui::Modal::new(egui::Id::new("unsaved")).show(ctx, |ui| {
            ui.heading("Save your changes?");
            ui.label(format!(
                "The project {} has changes that are not saved.",
                state.editor.project.name
            ));
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    answer = Some("save");
                }
                if ui.button("Don't save").clicked() {
                    answer = Some("discard");
                }
                if ui.button("Cancel").clicked() {
                    answer = Some("cancel");
                }
            });
        });
        if let Some(answer) = answer
            && let Err(why) = answer_prompt(state, file, answer)
        {
            state.say(why);
        }
    }
    let Some(name) = file.new_map.as_mut() else {
        return;
    };
    let mut close = false;
    let mut make = None;
    egui::Modal::new(egui::Id::new("new map")).show(ctx, |ui| {
        ui.heading("New map");
        ui.label("A map of your own, built from the game's rooms and added to this project as a further level. Its light, fog and sound start as this level's.");
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.add(
                egui::TextEdit::singleline(name)
                    .hint_text("my_map")
                    .desired_width(160.0),
            );
        });
        ui.label(
            egui::RichText::new("lowercase letters, digits and _, up to 12")
                .small()
                .weak(),
        );
        ui.horizontal(|ui| {
            let ready = level_edit::map::name_problem(name).is_none();
            if ui.add_enabled(ready, egui::Button::new("Make")).clicked() {
                make = Some(name.clone());
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if let Some(name) = make {
        match new_map_args(state, &name) {
            Ok(args) => {
                file.new_map = None;
                want(state, file, Leave::Start(args));
            }
            Err(why) => state.say(why),
        }
    } else if close {
        file.new_map = None;
    }
}

/// A request to close the window leaves as Quit does: asking first when
/// there are changes not saved.
pub(crate) fn close_requests(
    mut requests: MessageReader<WindowCloseRequested>,
    state: Res<EditorState>,
    mut ui_state: ResMut<UiState>,
) {
    if requests.read().count() > 0 && ui_state.file.asking.is_none() {
        want(&state, &mut ui_state.file, Leave::Quit);
    }
}

/// Leaves the level once the frame is done: starts the editor again where
/// that is asked for, and closes this one.
pub(crate) fn leave(
    mut state: ResMut<EditorState>,
    mut ui_state: ResMut<UiState>,
    mut exit: MessageWriter<AppExit>,
    headless: Res<Headless>,
) {
    let Some(leave) = ui_state.file.leaving.take() else {
        return;
    };
    if let Leave::Start(args) = &leave {
        if headless.0 {
            info!(target: "editor", "would start kjk-level-editor {}", args.join(" "));
        } else {
            let me = std::env::current_exe().map_err(|error| error.to_string());
            // The agent link stays as it is here: on in the editor that
            // takes over when it is on in this one.
            let link = crate::agent::carried(ui_state.link_on());
            if let Err(why) = me.and_then(|me| {
                let mut start = std::process::Command::new(me);
                start.args(args);
                match &link {
                    Some(name) => start.env(crate::agent::VAR, name),
                    None => start.env_remove(crate::agent::VAR),
                };
                start.spawn().map_err(|error| error.to_string())
            }) {
                // The level stays open, and the window says why.
                error!(target: "editor", "the editor did not start again: {why}");
                state.say(format!(
                    "the editor did not start again, so this level stays open: {why}"
                ));
                return;
            }
        }
    }
    // Left by choice: what was not saved was given up, so no copy of it
    // waits for the next start.
    crate::recover::clear(&mut state);
    info!(target: "editor", "left the level: {leave:?}");
    exit.write(AppExit::Success);
}

/// Whether the run has no window (`--headless`).
#[derive(Resource, Clone, Copy)]
pub(crate) struct Headless(pub(crate) bool);
