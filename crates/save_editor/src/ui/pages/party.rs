//! Party: the companions the party table holds, in the party or not;
//! made available, joined, sent off or forgotten. Those changes are queued
//! for the save's scripts and happen when the save loads.

use egui::RichText;

use crate::ui::list::{self, Row, col, rest};
use crate::ui::pages::hint;
use crate::ui::{App, ready};
use save_editor::model::session::Change;

/// A companion's state now and after the queued changes, in words.
fn state(member: bool, available: bool) -> &'static str {
    match (member, available) {
        (true, _) => "in the party",
        (false, true) => "available",
        (false, false) => "not available",
    }
}

/// Companion `npc`'s name: the install's template, else the one the
/// party table keeps, else its template's resref.
fn name(app: &App, npc: u8) -> String {
    let catalog = ready(&app.catalogs, app.game());
    let Some(open) = &app.open else {
        return format!("companion {npc}");
    };
    let Some(slot) = open.session.edited.script.party.available.get(&npc) else {
        return format!("companion {npc}");
    };
    let creature = catalog.map(|catalog| {
        let creature = catalog.creature(&slot.template);
        match slot.template_data.as_deref() {
            Some(bytes) if creature.name.is_empty() => catalog.creature_from(bytes),
            _ => creature,
        }
    });
    creature
        .map(|creature| creature.name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| slot.template.clone())
}

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    let Some(open) = &app.open else {
        return;
    };
    let save = &open.session.edited;
    let party = &save.script.party;
    let (available, members) = edit::party_after(save);
    let mut npcs: Vec<u8> = party
        .available
        .keys()
        .copied()
        .chain(available.iter().copied())
        .collect();
    npcs.sort();
    npcs.dedup();
    let rows: Vec<Row> = npcs
        .iter()
        .map(|&npc| {
            let now = state(
                party.members.contains(&npc),
                party.available.contains_key(&npc),
            );
            let after = state(members.contains(&npc), available.contains(&npc));
            Row {
                key: u64::from(npc),
                cells: vec![
                    name(app, npc),
                    npc.to_string(),
                    now.to_string(),
                    if now == after {
                        String::new()
                    } else {
                        format!("{after} when you load the save")
                    },
                ],
                grey: now == "not available" && after == now,
                ..Row::default()
            }
        })
        .collect();
    hint(
        ui,
        "party changes are queued: they happen when you load the save in kjk",
    );
    list::rows(
        ui,
        "party",
        &[
            rest("companion"),
            col("no.", 30.0),
            col("now", 90.0),
            col("after loading", 200.0),
        ],
        rows.len(),
        |index| rows[index].clone(),
        &mut app.icons,
        None,
        &mut app.selected,
    );
}

pub fn details(app: &mut App, ui: &mut egui::Ui) {
    let Some(open) = &app.open else {
        return;
    };
    let (available, members) = edit::party_after(&open.session.edited);
    let mut change = None;
    if let Some(npc) = app.selected.map(|key| key as u8) {
        ui.label(RichText::new(name(app, npc)).strong());
        hint(
            ui,
            &format!(
                "companion {npc}: {}",
                state(members.contains(&npc), available.contains(&npc))
            ),
        );
        ui.horizontal(|ui| {
            if members.contains(&npc) {
                if ui.button("Leave the party").clicked() {
                    change = Some(Change::Leave(npc));
                }
            } else if available.contains(&npc) {
                if ui.button("Join the party").clicked() {
                    change = Some(Change::Join(npc));
                }
                if ui.button("Not available").clicked() {
                    change = Some(Change::Unavailable(npc));
                }
            }
        });
        ui.separator();
    }
    ui.label(RichText::new("Make a companion available").strong());
    hint(
        ui,
        "by party number and template (the game's own, e.g. p_bastilla)",
    );
    ui.horizontal(|ui| {
        ui.label("no.");
        ui.add(egui::DragValue::new(&mut app.pages.npc).range(0..=11));
        ui.add(
            egui::TextEdit::singleline(&mut app.pages.text)
                .hint_text("template")
                .desired_width(110.0),
        );
    });
    if ui
        .add_enabled(
            !app.pages.text.trim().is_empty(),
            egui::Button::new("Make available"),
        )
        .clicked()
    {
        change = Some(Change::Available(
            app.pages.npc,
            app.pages.text.trim().to_lowercase(),
        ));
    }
    if let Some(change) = change {
        app.apply(change);
    }
}
