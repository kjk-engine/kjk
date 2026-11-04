//! Equipment: the picked character's slots, and what fits the one picked.

use egui::RichText;

use crate::ui::list::{self, Row, col, rest};
use crate::ui::pages::{self, entry_row, hint, searched};
use crate::ui::{App, ready};
use save_editor::model::session::{Change, SLOTS, slot_name};
use sim::inventory::slots;

/// Selection keys past the slots: what fits, by catalog key.
const FITS: u64 = 1 << 32;

/// What `who` wears in each slot, by id.
fn worn(app: &App, who: &edit::Who) -> Vec<Option<(u32, sim::inventory::Item)>> {
    let Some(open) = &app.open else {
        return Vec::new();
    };
    let save = &open.session.edited;
    let holder = edit::holder_of(save, who);
    SLOTS
        .iter()
        .map(|(slot, _)| {
            let holder = holder?;
            save.script
                .items
                .held_by(holder)
                .find(|(_, item)| item.slot == Some(*slot))
                .map(|(id, item)| (id, item.clone()))
        })
        .collect()
}

/// The slot picked (the slots' rows are keyed by their index).
fn picked(app: &App) -> Option<u8> {
    let key = app.selected?;
    let index = if key >= FITS {
        app.pages.slot?
    } else {
        key as usize
    };
    SLOTS.get(index).map(|(slot, _)| *slot)
}

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    let Some(member) = pages::member(app, ui) else {
        return;
    };
    let who = member.who;
    let name = member.name.clone();
    let worn = worn(app, &who);
    let catalog = ready(&app.catalogs, app.game());
    hint(
        ui,
        &format!("{name}: pick a slot, then double-click what to put on"),
    );
    let table = ui.allocate_ui(egui::vec2(ui.available_width(), 13.0 * 15.0 + 20.0), |ui| {
        list::rows(
            ui,
            "slots",
            &[col("slot", 130.0), rest("worn")],
            SLOTS.len(),
            |index| {
                let worn = worn.get(index).and_then(Option::as_ref);
                let item = worn.map(|(id, item)| pages::item_row(catalog, *id, item));
                Row {
                    key: index as u64,
                    icon: item.as_ref().and_then(|row| row.icon.clone()),
                    cells: vec![
                        SLOTS[index].1.to_string(),
                        item.map_or("—".to_string(), |row| row.cells[0].clone()),
                    ],
                    ..Row::default()
                }
            },
            &mut app.icons,
            catalog,
            &mut app.selected,
        )
    });
    if let Some(clicked) = table.inner {
        app.pages.slot = Some(clicked.key as usize);
    }
    let Some(slot) = picked(app) else {
        return;
    };
    let Some(catalog) = catalog else {
        hint(ui, "putting things on needs the game's install");
        return;
    };
    ui.separator();
    let bit = 1 << slots::rules(slot);
    let fits: Vec<_> = searched(&catalog.items, &app.search)
        .into_iter()
        .filter(|entry| entry.slots & bit != 0)
        .collect();
    hint(
        ui,
        &format!(
            "{} things fit the {} (search to narrow)",
            fits.len(),
            slot_name(slot)
        ),
    );
    let mut selected = app
        .selected
        .filter(|key| *key >= FITS)
        .map(|key| key - FITS);
    let clicked = list::rows(
        ui,
        "fits",
        &[
            rest("item"),
            col("kind", 110.0),
            col("stat", 50.0),
            col("resref", 110.0),
        ],
        fits.len(),
        |index| entry_row(fits[index]),
        &mut app.icons,
        Some(catalog),
        &mut selected,
    );
    let give = clicked
        .as_ref()
        .filter(|clicked| clicked.double)
        .map(|clicked| catalog.items[clicked.key as usize].resref.clone());
    if let Some(key) = selected {
        app.selected = Some(key + FITS);
    }
    if let Some(template) = give {
        app.apply(Change::GiveEquipped {
            who,
            template,
            slot,
        });
    }
}

pub fn details(app: &mut App, ui: &mut egui::Ui) {
    let Some(member) = app.member() else {
        return;
    };
    let who = member.who;
    let Some(slot) = picked(app) else {
        hint(ui, "Pick a slot to see what it holds.");
        return;
    };
    let index = SLOTS.iter().position(|(of, _)| *of == slot).unwrap_or(0);
    let worn = worn(app, &who).into_iter().nth(index).flatten();
    let catalog = ready(&app.catalogs, app.game());
    ui.label(RichText::new(slot_name(slot)).strong());
    let mut change = None;
    match &worn {
        Some((id, item)) => {
            let row = pages::item_row(catalog, *id, item);
            pages::head(
                ui,
                &mut app.icons,
                catalog,
                (row.icon.as_deref(), &row.cells[0], &item.template),
            );
            if ui.button("Take off").clicked() {
                change = Some(Change::Unequip(who, slot));
            }
        }
        None => hint(ui, "nothing worn here"),
    }
    let chosen = app
        .selected
        .filter(|key| *key >= FITS)
        .and_then(|key| catalog?.items.get((key - FITS) as usize));
    if let Some(entry) = chosen {
        ui.separator();
        let (icon, name, resref) = (
            entry.icon.clone(),
            entry.label().to_string(),
            entry.resref.clone(),
        );
        let detail = format!("{} {}", entry.kind, entry.detail);
        pages::head(
            ui,
            &mut app.icons,
            catalog,
            (icon.as_deref(), &name, &resref),
        );
        hint(ui, &detail);
        if ui.button(format!("Put on ({})", slot_name(slot))).clicked() {
            change = Some(Change::GiveEquipped {
                who,
                template: resref,
                slot,
            });
        }
        hint(ui, "a new one is made and put on");
    }
    if let Some(change) = change {
        app.apply(change);
    }
}
