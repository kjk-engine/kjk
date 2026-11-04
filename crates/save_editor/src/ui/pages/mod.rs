//! The pages, one per tab: each a list (`list::rows`) and the changes it
//! makes through `Session::apply`; the selected row's details and actions
//! on the right. A refused change shows as the red note, never a modal.

mod equipment;
mod inventory;
mod party;
mod powers;
mod quests;
mod stats;

use std::collections::HashMap;

use egui::RichText;

use crate::ui::list::Row;
use crate::ui::shell::{AMBER, RED};
use crate::ui::{App, Member, Tab};
use save_editor::model::catalog::{Catalog, Entry};

/// What the pages keep between frames.
#[derive(Default)]
pub struct State {
    /// The save dialog is open.
    pub saving: bool,
    /// Inventory: the catalog is shown to add from, and how many.
    adding: bool,
    add_count: u16,
    /// Powers & Feats: powers shown (else feats).
    powers: bool,
    /// Quests & Flags: flags shown (else quests).
    flags: bool,
    /// Numbers being edited, by field, until they are let go.
    edits: HashMap<String, i64>,
    /// Text being edited (a string flag, a template).
    text: String,
    /// Party: the companion number to make available.
    npc: u8,
    /// Equipment: the slot picked, by its row.
    slot: Option<usize>,
    /// Quests & Flags: the flag picked, by name.
    text_key: Option<String>,
}

/// The current tab's page.
pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    if app.catalog().is_none()
        && let Some(why) = app.catalog_missing()
    {
        ui.label(RichText::new(format!("! {why}")).small().color(AMBER));
    }
    match app.tab {
        Tab::Equipment => equipment::draw(app, ui),
        Tab::Inventory => inventory::draw(app, ui),
        Tab::Powers => powers::draw(app, ui),
        Tab::Stats => stats::draw(app, ui),
        Tab::Party => party::draw(app, ui),
        Tab::Quests => quests::draw(app, ui),
    }
}

/// The selected row's details.
pub fn details(app: &mut App, ui: &mut egui::Ui) {
    match app.tab {
        Tab::Equipment => equipment::details(app, ui),
        Tab::Inventory => inventory::details(app, ui),
        Tab::Powers => powers::details(app, ui),
        Tab::Party => party::details(app, ui),
        Tab::Quests => quests::details(app, ui),
        Tab::Stats => hint(
            ui,
            "Change a value and let go of it: each is one change, undone with Undo.",
        ),
    }
}

/// A small grey line.
pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().weak());
}

/// The page's own red line, when it has nothing to show.
pub fn missing(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(RED));
}

/// `list` searched by the search field.
pub fn searched<'a>(list: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    Catalog::search(list, query).collect()
}

/// An entry as a row: icon, name (with a "mod" tag), kind, stat, resref.
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

/// An item of the save as a row (name, count, kind, stat, resref), named
/// by the install when it can be.
pub fn item_row(catalog: Option<&Catalog>, id: u32, item: &sim::inventory::Item) -> Row {
    let entry = catalog.and_then(|catalog| catalog.item(&item.template));
    let of = |field: fn(&Entry) -> String| entry.map_or(String::new(), field);
    Row {
        key: u64::from(id),
        icon: entry.and_then(|entry| entry.icon.clone()),
        cells: vec![
            entry.map_or(item.template.clone(), |entry| entry.label().to_string()),
            item.stack.to_string(),
            of(|entry| entry.kind.clone()),
            of(|entry| entry.detail.clone()),
            item.template.clone(),
        ],
        tag: entry
            .filter(|entry| entry.modded)
            .map(|_| "mod".to_string()),
        ..Row::default()
    }
}

/// A big icon, name and resref: the head of a details panel.
pub fn head(
    ui: &mut egui::Ui,
    icons: &mut crate::ui::icons::Icons,
    catalog: Option<&Catalog>,
    (icon, name, resref): (Option<&str>, &str, &str),
) {
    ui.horizontal(|ui| {
        icons.show(ui, catalog, icon, 40.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(name).strong());
            if !resref.is_empty() {
                ui.label(RichText::new(resref).small().weak());
            }
        });
    });
}

/// The party strip's pick, or a line saying there is none.
pub fn member<'a>(app: &'a App, ui: &mut egui::Ui) -> Option<&'a Member> {
    let member = app.member();
    if member.is_none() {
        missing(ui, "no character to edit in this save");
    }
    member
}

/// A number field edited in place: shown as `value`, and once let go at
/// another value, that value (one change per edit, not one per frame).
pub fn number(
    state: &mut State,
    ui: &mut egui::Ui,
    key: &str,
    value: i64,
    range: std::ops::RangeInclusive<i64>,
) -> Option<i64> {
    let mut shown = state.edits.get(key).copied().unwrap_or(value);
    // The value as the save has it, even outside `range` (a body below one
    // hit point): only what the user changes is held to it.
    let field = egui::DragValue::new(&mut shown)
        .range(range)
        .clamp_existing_to_range(false)
        .speed(0.2);
    let response = ui.add(field);
    if response.changed() {
        state.edits.insert(key.to_string(), shown);
    }
    if response.dragged() || response.has_focus() {
        return None;
    }
    // Let go: what the user changed, once.
    state.edits.remove(key).filter(|edited| *edited != value)
}

impl State {
    /// Row `key` picked from outside (a picture of the window): on the
    /// Equipment page, the slot of that row.
    pub fn select(&mut self, key: u64) {
        self.slot = Some(key as usize);
    }
}
