//! Inventory: what the party carries and its credits; adding from every
//! item the install holds (mods marked), removing, restacking.

use egui::RichText;

use crate::ui::list::{self, col, rest};
use crate::ui::pages::{self, entry_row, hint, item_row, searched};
use crate::ui::{App, ready};
use save_editor::model::session::Change;
use sim::inventory::PARTY;

/// Selection keys of the catalog's rows, past the items' ids.
const ADDING: u64 = 1 << 40;

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    let Some(open) = &app.open else {
        return;
    };
    let save = &open.session.edited;
    let gold = i64::from(save.script.gold);
    let mut change = None;
    ui.horizontal(|ui| {
        ui.label("Credits");
        if let Some(credits) =
            pages::number(&mut app.pages, ui, "credits", gold, 0..=i64::from(i32::MAX))
        {
            change = Some(Change::Credits(credits as i32));
        }
        ui.separator();
        ui.toggle_value(&mut app.pages.adding, "Add items…");
    });
    let catalog = ready(&app.catalogs, app.game());
    if app.pages.adding {
        let Some(catalog) = catalog else {
            pages::missing(ui, "adding items needs the game's install");
            return;
        };
        let shown = searched(&catalog.items, &app.search);
        hint(
            ui,
            &format!(
                "{} of {} items the install holds: double-click to give the party one (count on the right)",
                shown.len(),
                catalog.items.len()
            ),
        );
        let mut selected = app
            .selected
            .filter(|key| *key >= ADDING)
            .map(|key| key - ADDING);
        let clicked = list::rows(
            ui,
            "add items",
            &[
                rest("item"),
                col("kind", 110.0),
                col("stat", 50.0),
                col("resref", 110.0),
            ],
            shown.len(),
            |index| entry_row(shown[index]),
            &mut app.icons,
            Some(catalog),
            &mut selected,
        );
        if let Some(key) = selected {
            app.selected = Some(key + ADDING);
        }
        if let Some(clicked) = clicked.filter(|clicked| clicked.double) {
            change = Some(Change::AddItem {
                holder: PARTY,
                template: catalog.items[clicked.key as usize].resref.clone(),
                stack: app.pages.add_count.max(1),
            });
        }
    } else {
        let carried: Vec<(u32, &sim::inventory::Item)> = save.script.items.held_by(PARTY).collect();
        let shown: Vec<_> = carried
            .into_iter()
            .map(|(id, item)| item_row(catalog, id, item))
            .filter(|row| {
                let lower = format!("{} {}", row.cells[0], row.cells[4]).to_lowercase();
                app.search
                    .split_whitespace()
                    .all(|word| lower.contains(&word.to_lowercase()))
            })
            .collect();
        hint(ui, &format!("the party carries {} things", shown.len()));
        let mut selected = app.selected.filter(|key| *key < ADDING);
        list::rows(
            ui,
            "carried",
            &[
                rest("item"),
                col("count", 40.0),
                col("kind", 110.0),
                col("stat", 50.0),
                col("resref", 110.0),
            ],
            shown.len(),
            |index| shown[index].clone(),
            &mut app.icons,
            catalog,
            &mut selected,
        );
        if selected.is_some() {
            app.selected = selected;
        }
    }
    if let Some(change) = change {
        app.apply(change);
    }
}

pub fn details(app: &mut App, ui: &mut egui::Ui) {
    let catalog = ready(&app.catalogs, app.game());
    let mut change = None;
    match app.selected {
        Some(key) if key >= ADDING => {
            let Some(entry) =
                catalog.and_then(|catalog| catalog.items.get((key - ADDING) as usize))
            else {
                return;
            };
            let (icon, name, resref) = (
                entry.icon.clone(),
                entry.label().to_string(),
                entry.resref.clone(),
            );
            let about = [entry.kind.clone(), entry.detail.clone()].join(" ");
            let modded = entry.modded;
            pages::head(
                ui,
                &mut app.icons,
                catalog,
                (icon.as_deref(), &name, &resref),
            );
            hint(ui, &about);
            if modded {
                hint(ui, "a mod: from the install's override folder");
            }
            ui.horizontal(|ui| {
                ui.label("Count");
                let mut count = app.pages.add_count.max(1);
                ui.add(egui::DragValue::new(&mut count).range(1..=100));
                app.pages.add_count = count;
                if ui.button("Give the party").clicked() {
                    change = Some(Change::AddItem {
                        holder: PARTY,
                        template: resref.clone(),
                        stack: count,
                    });
                }
            });
        }
        Some(key) => {
            let id = key as u32;
            let Some(item) = app
                .open
                .as_ref()
                .and_then(|open| open.session.edited.script.items.get(&id).cloned())
            else {
                return;
            };
            let row = item_row(catalog, id, &item);
            pages::head(
                ui,
                &mut app.icons,
                catalog,
                (row.icon.as_deref(), &row.cells[0], &item.template),
            );
            hint(ui, &format!("{} {}", row.cells[2], row.cells[3]));
            ui.horizontal(|ui| {
                ui.label("Stack");
                if let Some(stack) = pages::number(
                    &mut app.pages,
                    ui,
                    &format!("stack {id}"),
                    i64::from(item.stack),
                    1..=999,
                ) {
                    change = Some(Change::Stack(id, stack as u16));
                }
            });
            if ui.button("Remove").clicked() {
                change = Some(Change::RemoveItem(id));
            }
            if item.plot {
                ui.label(RichText::new("a plot item: the story may need it").small());
            }
        }
        None => hint(
            ui,
            "Pick an item to see it here, or Add items… to give the party more.",
        ),
    }
    if let Some(change) = change {
        app.apply(change);
    }
}
