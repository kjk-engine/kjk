//! The pages, one per tab, each a list (`list::rows`) and the changes it
//! makes; the selected row's details on the right.

use egui::RichText;

use crate::ui::list::{self, Row, col, rest};
use crate::ui::shell::RED;
use crate::ui::{App, Tab};
use save_editor::model::catalog::{Catalog, Entry};

/// What the pages keep between frames.
#[derive(Default)]
pub struct State {
    /// The save dialog is open.
    pub saving: bool,
}

/// The current tab's page.
pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    match app.tab {
        Tab::Inventory => catalog_items(app, ui),
        other => {
            ui.label(RichText::new(format!("{}: not built yet", other.title())).weak());
        }
    }
}

/// `list` searched by the search field.
pub fn searched<'a>(list: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    Catalog::search(list, query).collect()
}

/// An entry as a row: icon, name (with "mod" tag), kind, stat, resref.
pub fn entry_row(entry: &Entry) -> Row {
    Row {
        key: u64::from(entry.key),
        icon: entry.icon.clone(),
        cells: vec![
            entry.label().to_string(),
            entry.kind.clone(),
            entry.detail.clone(),
            entry.resref.clone(),
        ],
        tag: entry.modded.then(|| "mod".to_string()),
        ..Row::default()
    }
}

/// Every item the install holds.
fn catalog_items(app: &mut App, ui: &mut egui::Ui) {
    let catalog = crate::ui::ready(&app.catalogs, app.game());
    let Some(catalog) = catalog else {
        ui.label(RichText::new(app.catalog_missing().unwrap_or_else(String::new)).color(RED));
        return;
    };
    let shown = searched(&catalog.items, &app.search);
    ui.label(
        RichText::new(format!("{} of {} items", shown.len(), catalog.items.len()))
            .small()
            .weak(),
    );
    list::rows(
        ui,
        "catalog items",
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
        &mut app.selected,
    );
}

/// The selected row's details.
pub fn details(app: &mut App, ui: &mut egui::Ui) {
    let catalog = crate::ui::ready(&app.catalogs, app.game());
    let entry = match (app.tab, catalog, app.selected) {
        (Tab::Inventory, Some(catalog), Some(key)) => catalog.items.get(key as usize),
        _ => None,
    };
    let Some(entry) = entry else {
        ui.label(RichText::new("Select a row to see it here.").small().weak());
        return;
    };
    ui.horizontal(|ui| {
        app.icons.show(ui, catalog, entry.icon.as_deref(), 40.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(entry.label()).strong());
            ui.label(RichText::new(&entry.resref).small().weak());
        });
    });
    if !entry.kind.is_empty() {
        ui.label(&entry.kind);
    }
    if !entry.detail.is_empty() {
        ui.label(&entry.detail);
    }
    if entry.modded {
        ui.label(RichText::new("a mod: from the install's override folder").small());
    }
}
