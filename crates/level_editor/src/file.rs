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
/// has no such level yet, or says why not.
fn to_level(state: &mut EditorState, file: &mut FileState, module: &str) {
    let module = module.to_ascii_lowercase();
    let project = &state.editor.project;
    if module == project.module {
        state.say(format!("{module} is the level being edited"));
    } else if project.map.is_some() {
        state.say("a new map's project has one level");
    } else if state.setup.modules.binary_search(&module).is_err() {
        state.say(format!("the install has no module {module}"));
    } else {
        want(state, file, Leave::Start(level_args(state, &module)));
    }
}

/// The arguments that make the new map `name`, its area's light, fog and
/// sound from the level being edited (a new map's own base, for a new map),
/// or why not.
fn new_map_args(state: &EditorState, name: &str) -> Result<Vec<String>, String> {
    if let Some(problem) = level_edit::map::name_problem(name) {
        return Err(problem);
    }
    let folder = level_edit::folder_of(name, |var| std::env::var(var).ok())?;
    if level_edit::Project::exists(&folder) {
        return Err(format!("a project named {name} exists already"));
    }
    let project = &state.editor.project;
    let base = project
        .map
        .as_ref()
        .map_or(project.module.clone(), |map| map.base.clone());
    Ok(vec![
        project.game.clone(),
        name.to_string(),
        "--project".into(),
        name.to_string(),
        "--new-map".into(),
        base,
    ])
}

/// `edit ui file ...`, `edit ui quit` and `edit ui confirm ...`; false for
/// words that are none of them.
pub(crate) fn command(state: &mut EditorState, file: &mut FileState, words: &[&str]) -> bool {
    match words {
        ["file", "open"] => want(state, file, Leave::Start(Vec::new())),
        ["file", "open", name] => {
            let found = crate::picker::projects()
                .into_iter()
                .find(|found| found.name.eq_ignore_ascii_case(name));
            match found {
                Some(found) => want(state, file, Leave::Start(open_args(&found))),
                None => state.say(format!("there is no project named {name}")),
            }
        }
        ["file", "level", module] => to_level(state, file, module),
        ["file", "new", name] => match new_map_args(state, name) {
            Ok(args) => want(state, file, Leave::Start(args)),
            Err(why) => state.say(why),
        },
        ["quit"] => want(state, file, Leave::Quit),
        ["confirm", answer @ ("save" | "discard" | "cancel")] => answer_prompt(state, file, answer),
        _ => return false,
    }
    true
}

/// The prompt answered: save and leave, leave without saving, or stay.
fn answer_prompt(state: &mut EditorState, file: &mut FileState, answer: &str) {
    let Some(leave) = file.asking.take() else {
        return;
    };
    match answer {
        "save" => match crate::state::save(state) {
            Ok(()) => file.leaving = Some(leave),
            Err(why) => state.say(format!("not saved, so the level stays open: {why}")),
        },
        "discard" => file.leaving = Some(leave),
        _ => {}
    }
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
        if project.map.is_none() {
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
                        to_level(state, file, &module);
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
                        to_level(state, file, &module);
                        ui.close();
                    }
                });
            });
        }
        if ui
            .button("New map...")
            .on_hover_text("A map of your own, its light, fog and sound this level's")
            .clicked()
        {
            file.new_map = Some(String::new());
            ui.close();
        }
        ui.separator();
        if ui.button("Save    Ctrl+S").clicked() {
            state.pending_command = Some("save".into());
            ui.close();
        }
        ui.separator();
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
        if let Some(answer) = answer {
            answer_prompt(state, file, answer);
        }
    }
    let Some(name) = file.new_map.as_mut() else {
        return;
    };
    let mut close = false;
    let mut make = None;
    egui::Modal::new(egui::Id::new("new map")).show(ctx, |ui| {
        ui.heading("New map");
        ui.label("A map of your own, built from the game's rooms. Its light, fog and sound start as this level's.");
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
            if let Err(why) = me.and_then(|me| {
                std::process::Command::new(me)
                    .args(args)
                    .spawn()
                    .map_err(|error| error.to_string())
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
    info!(target: "editor", "left the level: {leave:?}");
    exit.write(AppExit::Success);
}

/// Whether the run has no window (`--headless`).
#[derive(Resource, Clone, Copy)]
pub(crate) struct Headless(pub(crate) bool);
