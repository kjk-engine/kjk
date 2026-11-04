//! Quests & Flags: the journal by quest name with each quest's entries to
//! pick from; and the story flags (globals) by name, type and value, raw.

use egui::RichText;

use crate::ui::list::{self, Row, col, rest};
use crate::ui::pages::{self, hint, number};
use crate::ui::shell::AMBER;
use crate::ui::{App, ready};
use save_editor::model::catalog::{Kind, words};
use save_editor::model::session::Change;

/// The entry quest `tag` stands at in `save`, if any.
fn standing(save: &sim::Save, tag: &str) -> Option<i32> {
    save.script
        .journal
        .iter()
        .find(|(of, _)| of.eq_ignore_ascii_case(tag))
        .map(|(_, entry)| *entry)
}

/// A flag's value in `save`, in words.
fn value(save: &sim::Save, name: &str, kind: Kind) -> String {
    let script = &save.script;
    match kind {
        Kind::Number => script
            .numbers
            .get(name)
            .map_or("0".into(), ToString::to_string),
        Kind::Boolean => script
            .booleans
            .get(name)
            .copied()
            .unwrap_or(false)
            .to_string(),
        Kind::Text => script
            .strings
            .get(name)
            .map_or(String::new(), |text| format!("{text:?}")),
    }
}

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        for (flags, title) in [(false, "Quests"), (true, "Flags (advanced)")] {
            if ui
                .selectable_label(app.pages.flags == flags, title)
                .clicked()
                && app.pages.flags != flags
            {
                app.pages.flags = flags;
                app.selected = None;
            }
        }
    });
    let (Some(open), Some(catalog)) = (&app.open, ready(&app.catalogs, app.game())) else {
        pages::missing(ui, "quests and flags by name need the game's install");
        return;
    };
    let save = &open.session.edited;
    if app.pages.flags {
        ui.label(
            RichText::new("Advanced: the scripts read these as they are. You get what the story does with a value you set.")
                .small()
                .color(AMBER),
        );
        let words = words(&app.search);
        let shown: Vec<_> = catalog
            .globals
            .iter()
            .filter(|global| words.iter().all(|word| global.name.contains(word.as_str())))
            .collect();
        list::rows(
            ui,
            "flags",
            &[rest("flag"), col("type", 60.0), col("value", 120.0)],
            shown.len(),
            |index| {
                let global = shown[index];
                Row {
                    key: index as u64,
                    cells: vec![
                        global.name.clone(),
                        kind_name(global.kind).into(),
                        value(save, &global.name, global.kind),
                    ],
                    ..Row::default()
                }
            },
            &mut app.icons,
            None,
            &mut app.selected,
        );
        // Selection keys are positions in the filtered list: keep the name.
        app.pages.text_key = app
            .selected
            .and_then(|key| shown.get(key as usize))
            .map(|global| global.name.clone());
    } else {
        let words = words(&app.search);
        let shown: Vec<u32> = (catalog.quests.iter())
            .filter(|quest| quest.entry.matches(&words))
            .map(|quest| quest.entry.key)
            .collect();
        list::rows(
            ui,
            "quests",
            &[rest("quest"), col("now", 250.0)],
            shown.len(),
            |index| {
                let quest = &catalog.quests[shown[index] as usize];
                let now = standing(save, &quest.entry.resref);
                let stage =
                    now.and_then(|id| quest.stages.iter().find(|stage| stage.id as i32 == id));
                Row {
                    key: u64::from(quest.entry.key),
                    cells: vec![
                        quest.entry.label().to_string(),
                        match (now, stage) {
                            (None, _) => "not started".into(),
                            (Some(id), Some(stage)) if stage.end => format!("done ({id})"),
                            (Some(id), _) => format!("entry {id}"),
                        },
                    ],
                    tag: Some(quest.entry.resref.clone()),
                    grey: now.is_none(),
                    ..Row::default()
                }
            },
            &mut app.icons,
            None,
            &mut app.selected,
        );
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Number => "number",
        Kind::Boolean => "yes/no",
        Kind::Text => "text",
    }
}

pub fn details(app: &mut App, ui: &mut egui::Ui) {
    let (Some(open), Some(catalog)) = (&app.open, ready(&app.catalogs, app.game())) else {
        return;
    };
    let save = &open.session.edited;
    let mut change = None;
    if app.pages.flags {
        let Some(name) = app.pages.text_key.clone() else {
            hint(ui, "Pick a flag to set it.");
            return;
        };
        let Some(global) = catalog.globals.iter().find(|global| global.name == name) else {
            return;
        };
        ui.label(RichText::new(&global.name).strong());
        hint(ui, kind_name(global.kind));
        match global.kind {
            Kind::Number => {
                let now = i64::from(save.script.numbers.get(&name).copied().unwrap_or(0));
                if let Some(set) = number(
                    &mut app.pages,
                    ui,
                    &name,
                    now,
                    i64::from(i32::MIN)..=i64::from(i32::MAX),
                ) {
                    change = Some(Change::Number(name.clone(), set as i32));
                }
            }
            Kind::Boolean => {
                let mut now = save.script.booleans.get(&name).copied().unwrap_or(false);
                if ui.checkbox(&mut now, "true").changed() {
                    change = Some(Change::Boolean(name.clone(), now));
                }
            }
            Kind::Text => {
                ui.add(egui::TextEdit::singleline(&mut app.pages.text).desired_width(150.0));
                if ui.button("Set").clicked() {
                    change = Some(Change::Text(name.clone(), app.pages.text.clone()));
                }
            }
        }
    } else {
        let Some(quest) = app
            .selected
            .and_then(|key| catalog.quests.get(key as usize))
        else {
            hint(ui, "Pick a quest to choose its entry.");
            return;
        };
        let now = standing(save, &quest.entry.resref);
        ui.label(RichText::new(quest.entry.label()).strong());
        hint(ui, &quest.entry.resref);
        hint(ui, "pick the entry the journal should stand at:");
        egui::ScrollArea::vertical().show(ui, |ui| {
            for stage in &quest.stages {
                let text = format!(
                    "{}{} {}",
                    stage.id,
                    if stage.end { " (ends it)" } else { "" },
                    stage.text
                );
                let button =
                    egui::Button::selectable(now == Some(stage.id as i32), text.clone()).truncate();
                if ui.add(button).on_hover_text(&stage.text).clicked() {
                    change = Some(Change::Quest(quest.entry.resref.clone(), stage.id as i32));
                }
            }
        });
    }
    if let Some(change) = change {
        app.apply(change);
    }
}
