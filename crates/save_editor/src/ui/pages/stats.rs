//! Stats: the picked character's classes, abilities, skills, alignment,
//! points and XP, a TSL companion's influence, and the party's XP.

use egui::RichText;

use crate::ui::pages::{self, hint, number};
use crate::ui::{App, ready};
use edit::Who;
use save_editor::model::catalog::Catalog;
use save_editor::model::session::{ABILITIES, Change};
use sim::levels::Career;
use sim::script::party::{INFLUENCE_SLOTS, MAX_INFLUENCE};

pub fn draw(app: &mut App, ui: &mut egui::Ui) {
    let Some(member) = pages::member(app, ui) else {
        return;
    };
    let (who, name) = (member.who, member.name.clone());
    let character = member.character.clone();
    let Some(open) = &app.open else {
        return;
    };
    let save = &open.session.edited;
    let own_xp = edit::holder_of(save, &who).map(|holder| save.script.xp(holder));
    let party_xp = save.script.xp(sim::inventory::PARTY);
    let tsl = app.game() == Some(vfs::Game::Kotor2);
    let influence = match who {
        Who::Companion { npc, .. } | Who::Away { npc } if tsl && npc < INFLUENCE_SLOTS => {
            Some((npc, save.script.party.influence(npc)))
        }
        _ => None,
    };
    let catalog = ready(&app.catalogs, app.game());
    let sheet = character.sheet;
    let state = &mut app.pages;
    let mut changes = Vec::new();
    ui.label(RichText::new(format!("{name}, level {}", sheet.level)).strong());
    if let Some(catalog) = catalog {
        classes(ui, catalog, who, &character.career, &mut changes);
    }
    // Abilities and skills side by side, a row each, as far as each goes.
    egui::Grid::new("stats")
        .num_columns(4)
        .spacing([10.0, 2.0])
        .striped(true)
        .show(ui, |ui| {
            for index in 0..ABILITIES.len().max(sheet.skills.len()) {
                match ABILITIES.get(index) {
                    Some(ability) => {
                        ui.label(*ability);
                        if let Some(score) = number(
                            state,
                            ui,
                            ability,
                            i64::from(sheet.abilities[index]),
                            1..=99,
                        ) {
                            changes.push(Change::Ability(who, index, score as u8));
                        }
                    }
                    None => {
                        ui.label("");
                        ui.label("");
                    }
                }
                if let Some(&ranks) = sheet.skills.get(index) {
                    let skill = catalog
                        .and_then(|catalog| {
                            catalog
                                .skills
                                .iter()
                                .find(|entry| entry.key as usize == index)
                        })
                        .map_or(format!("skill {index}"), |entry| entry.label().to_string());
                    ui.label(skill);
                    if let Some(ranks) = number(
                        state,
                        ui,
                        &format!("skill {index}"),
                        i64::from(ranks),
                        0..=99,
                    ) {
                        changes.push(Change::Skill(who, index, ranks as u8));
                    }
                }
                ui.end_row();
            }
        });
    ui.separator();
    egui::Grid::new("points")
        .num_columns(3)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            ui.label("Hit points");
            if let Some(hp) = number(
                state,
                ui,
                "hp",
                i64::from(character.hp / 100),
                1..=i64::from(sheet.max_hp),
            ) {
                changes.push(Change::Hp(who, hp as i32 * 100));
            }
            hint(ui, &format!("of {}", sheet.max_hp));
            ui.end_row();
            ui.label("Force points");
            if let Some(fp) = number(
                state,
                ui,
                "fp",
                i64::from(character.fp / 100),
                0..=i64::from(sheet.pool()),
            ) {
                changes.push(Change::Fp(who, fp as i32 * 100));
            }
            hint(ui, &format!("of {}", sheet.pool()));
            ui.end_row();
            ui.label("Alignment");
            if let Some(value) = number(state, ui, "alignment", i64::from(sheet.good_evil), 0..=100)
            {
                changes.push(Change::Alignment(who, value as u8));
            }
            hint(ui, "0 dark … 100 light");
            ui.end_row();
            if let Some(xp) = own_xp {
                ui.label("XP");
                if let Some(xp) = number(state, ui, "xp", i64::from(xp), 0..=i64::from(i32::MAX)) {
                    changes.push(Change::Xp(who, xp as i32));
                }
                hint(ui, "level up in kjk once XP allows");
                ui.end_row();
            }
            if let Some((npc, now)) = influence {
                ui.label("Influence");
                // Unset (-1) until a script sets it: shown as the start.
                let shown = i64::from(now.max(i32::from(sim::script::party::START_INFLUENCE)));
                if let Some(value) =
                    number(state, ui, "influence", shown, 0..=i64::from(MAX_INFLUENCE))
                {
                    changes.push(Change::Influence(npc, value as u8));
                }
                hint(ui, &format!("0 … {MAX_INFLUENCE}"));
                ui.end_row();
            }
            ui.label("Party XP");
            if let Some(xp) = number(
                state,
                ui,
                "party xp",
                i64::from(party_xp),
                0..=i64::from(i32::MAX),
            ) {
                changes.push(Change::PartyXp(xp as i32));
            }
            hint(ui, "level up in kjk once XP allows");
            ui.end_row();
        });
    for change in changes {
        app.apply(change);
    }
}

/// The character's classes, each with a list to change it to another
/// class (its levels kept), and a list to add a second class.
fn classes(
    ui: &mut egui::Ui,
    catalog: &Catalog,
    who: Who,
    career: &Career,
    changes: &mut Vec<Change>,
) {
    let named = |class: u8| {
        catalog
            .classes
            .iter()
            .find(|entry| entry.key == u32::from(class))
            .map_or(format!("class {class}"), |entry| entry.label().to_string())
    };
    let others = || {
        catalog
            .classes
            .iter()
            .filter(|entry| !career.iter().any(|(had, _)| u32::from(*had) == entry.key))
    };
    egui::Grid::new("classes")
        .num_columns(3)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            for &(class, levels) in career {
                ui.label(named(class));
                egui::ComboBox::from_id_salt(("class", class))
                    .selected_text(format!("{levels} levels"))
                    .show_ui(ui, |ui| {
                        for entry in others() {
                            if ui.selectable_label(false, entry.label()).clicked() {
                                changes.push(Change::ChangeClass(who, class, entry.key as u8));
                            }
                        }
                    });
                hint(ui, "pick a class to change these levels to it");
                ui.end_row();
            }
            if career.is_empty() {
                hint(ui, "the save does not record this character's class");
                ui.end_row();
            } else if career.len() < 2 {
                ui.label("Second class");
                egui::ComboBox::from_id_salt("add class")
                    .selected_text("none")
                    .show_ui(ui, |ui| {
                        for entry in others() {
                            if ui.selectable_label(false, entry.label()).clicked() {
                                changes.push(Change::AddClass(who, entry.key as u8));
                            }
                        }
                    });
                hint(ui, "its levels are taken at the next level-up in kjk");
                ui.end_row();
            }
        });
    ui.separator();
}
