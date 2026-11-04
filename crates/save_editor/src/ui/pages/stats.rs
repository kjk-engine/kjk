//! Stats: the picked character's abilities, skills, alignment, points and
//! XP, and the party's XP.

use egui::RichText;

use crate::ui::pages::{self, hint, number};
use crate::ui::{App, ready};
use save_editor::model::session::{ABILITIES, Change};

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
    let catalog = ready(&app.catalogs, app.game());
    let sheet = character.sheet;
    let state = &mut app.pages;
    let mut changes = Vec::new();
    ui.label(RichText::new(format!("{name}, level {}", sheet.level)).strong());
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
