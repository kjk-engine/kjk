//! Powers & Feats: the picked character's, known first, then the rest
//! greyed; a pick past its prerequisites is warned of, never refused.

use egui::RichText;

use crate::ui::list::{self, Row, col, rest};
use crate::ui::pages::{self, hint, searched};
use crate::ui::{App, ready};
use save_editor::model::catalog::Entry;
use save_editor::model::session::Change;

/// The list shown, known first.
fn ordered(entries: Vec<&Entry>, known: impl Fn(u16) -> bool) -> Vec<(&Entry, bool)> {
    let mut rows: Vec<(&Entry, bool)> = entries
        .into_iter()
        .map(|entry| (entry, known(entry.key as u16)))
        .collect();
    rows.sort_by_key(|(_, known)| !known);
    rows
}

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    let Some(member) = pages::member(app, ui) else {
        return;
    };
    let character = member.character.clone();
    ui.horizontal(|ui| {
        if ui.selectable_label(!app.pages.powers, "Feats").clicked() {
            app.pages.powers = false;
            app.selected = None;
        }
        if ui
            .selectable_label(app.pages.powers, "Force powers")
            .clicked()
        {
            app.pages.powers = true;
            app.selected = None;
        }
    });
    let Some(catalog) = ready(&app.catalogs, app.game()) else {
        pages::missing(ui, "feats and powers by name need the game's install");
        return;
    };
    let powers = app.pages.powers;
    let list = if powers {
        &catalog.powers
    } else {
        &catalog.feats
    };
    let sheet = &character.sheet;
    let rows = ordered(searched(list, &app.search), |row| {
        if powers {
            sheet.powers.has(row)
        } else {
            sheet.feats.has(row)
        }
    });
    hint(ui, "Double-click a feat or power to add or remove it.");
    let classes = &catalog.data.classes;
    let clicked = list::rows(
        ui,
        "powers",
        &[
            rest(if powers { "power" } else { "feat" }),
            col("row", 40.0),
        ],
        rows.len(),
        |index| {
            let (entry, known) = rows[index];
            let row = entry.key as u16;
            let warning = (!known)
                .then(|| {
                    if powers {
                        edit::power_warning(classes, &character, row)
                    } else {
                        edit::feat_warning(classes, &character, row)
                    }
                })
                .flatten();
            Row {
                key: u64::from(entry.key),
                icon: entry.icon.clone(),
                cells: vec![entry.label().to_string(), entry.key.to_string()],
                grey: !known,
                warning,
                ..Row::default()
            }
        },
        &mut app.icons,
        Some(catalog),
        &mut app.selected,
    );
    if let Some(clicked) = clicked.filter(|clicked| clicked.double) {
        let row = clicked.key as u16;
        let known = rows
            .iter()
            .any(|(entry, known)| entry.key == u32::from(row) && *known);
        app.apply(toggle(character.who, powers, row, known));
    }
}

/// Granting or taking away row `row`.
fn toggle(who: edit::Who, powers: bool, row: u16, known: bool) -> Change {
    match (powers, known) {
        (true, true) => Change::RemovePower(who, row),
        (true, false) => Change::GrantPower(who, row),
        (false, true) => Change::RemoveFeat(who, row),
        (false, false) => Change::GrantFeat(who, row),
    }
}

pub fn details(app: &mut App, ui: &mut egui::Ui) {
    let (Some(member), Some(key)) = (app.member(), app.selected) else {
        hint(ui, "Pick a feat or power to see it here.");
        return;
    };
    let character = member.character.clone();
    let Some(catalog) = ready(&app.catalogs, app.game()) else {
        return;
    };
    let powers = app.pages.powers;
    let list = if powers {
        &catalog.powers
    } else {
        &catalog.feats
    };
    let Some(entry) = list.iter().find(|entry| u64::from(entry.key) == key) else {
        return;
    };
    let (icon, name, resref) = (
        entry.icon.clone(),
        entry.label().to_string(),
        entry.resref.clone(),
    );
    let row = entry.key as u16;
    let known = if powers {
        character.sheet.powers.has(row)
    } else {
        character.sheet.feats.has(row)
    };
    let warning = if powers {
        edit::power_warning(&catalog.data.classes, &character, row)
    } else {
        edit::feat_warning(&catalog.data.classes, &character, row)
    };
    pages::head(
        ui,
        &mut app.icons,
        Some(catalog),
        (icon.as_deref(), &name, &resref),
    );
    if !known && let Some(warning) = &warning {
        ui.label(
            RichText::new(format!("! {warning}"))
                .small()
                .color(crate::ui::shell::AMBER),
        );
        hint(ui, "granted all the same if you ask: try it in kjk");
    }
    let label = if known { "Take away" } else { "Grant" };
    if ui.button(label).clicked() {
        app.apply(toggle(character.who, powers, row, known));
    }
}
