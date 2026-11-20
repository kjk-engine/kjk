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
    /// The projects of the projects' folder, read when the menu first opens.
    recent: Option<Vec<crate::picker::Found>>,
}

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
        let recent = file.recent.get_or_insert_with(crate::picker::projects);
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
                error!(target: "editor", "the editor did not start again: {why}");
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
