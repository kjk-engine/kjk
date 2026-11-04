//! The window's panels around the page.

use egui::{Color32, RichText};

use crate::ui::list::{self, Row};
use crate::ui::{App, Tab, pages};
use save_editor::model::locate::{self, Found};

/// The red of a one-line note.
pub const RED: Color32 = Color32::from_rgb(230, 90, 80);
/// A banner's amber.
pub const AMBER: Color32 = Color32::from_rgb(230, 170, 40);

impl App {
    pub fn draw(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("install").show(ui, |ui| self.install_strip(ui));
        if self.open.is_some() {
            egui::Panel::top("party").show(ui, |ui| self.party_strip(ui));
        }
        egui::Panel::bottom("changes").show(ui, |ui| self.changes_bar(ui));
        egui::Panel::left("saves")
            .default_size(300.0)
            .resizable(true)
            .show(ui, |ui| self.saves_panel(ui));
        if self.open.is_some() {
            egui::Panel::right("details")
                .default_size(220.0)
                .resizable(true)
                .show(ui, |ui| pages::details(self, ui));
        }
        egui::CentralPanel::default().show(ui, |ui| self.page(ui));
    }

    /// The install in use, a switch between those found, and a banner when
    /// names are missing.
    fn install_strip(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let installs = locate::installs(&self.discovery);
            match self.game().and_then(|game| self.discovery.get(game)) {
                Some(install) => {
                    ui.label(
                        RichText::new(format!("{} install", locate::game_name(install.game)))
                            .strong(),
                    );
                    ui.label(
                        RichText::new(install.root.display().to_string())
                            .small()
                            .weak(),
                    );
                }
                None => {
                    ui.label(RichText::new("No install").strong());
                }
            }
            if installs.len() > 1 {
                ui.label(RichText::new("·").weak());
                for (game, _) in &installs {
                    let using = self.game() == Some(*game);
                    let hint = if self.open.is_some() {
                        "the open save's game decides; this lists what was found"
                    } else {
                        "the install in use until a save is opened"
                    };
                    ui.add_enabled(!using, egui::Button::new(locate::game_name(*game)).small())
                        .on_hover_text(hint);
                }
            }
            if ui.small_button("Choose install…").clicked() {
                self.choose_install();
            }
            if let Some(why) = self.catalog_missing() {
                ui.label(RichText::new(format!("! {why}")).color(AMBER));
            }
        });
    }

    /// A folder picked as an install, checked and told in words.
    fn choose_install(&mut self) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Choose a K1 or TSL install")
            .pick_folder()
        else {
            return;
        };
        match locate::check_path(&folder) {
            Found::Install { game, .. } => {
                self.chosen
                    .insert(game.env_var().to_string(), folder.display().to_string());
                self.discovery = locate::discovery_over(&self.chosen);
                self.load_catalog(game);
                self.note = None;
            }
            other => self.note = Some(format!("{}: {other}", folder.display())),
        }
    }

    /// The party's characters in a strip of small portraits.
    fn party_strip(&mut self, ui: &mut egui::Ui) {
        let catalog = super::ready(&self.catalogs, self.game());
        let Some(open) = &self.open else {
            return;
        };
        let mut picked = None;
        ui.horizontal(|ui| {
            ui.label(RichText::new(&open.row.name).strong());
            ui.label(RichText::new(&open.row.module).small().weak());
            ui.separator();
            for (index, member) in open.members.iter().enumerate() {
                let chosen = index == self.member;
                let response = ui
                    .scope(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        self.icons
                            .show(ui, catalog, member.portrait.as_deref(), 22.0);
                        let away = matches!(member.who, edit::Who::Away { .. });
                        let text = RichText::new(&member.name);
                        let text = if away { text.weak().italics() } else { text };
                        ui.selectable_label(chosen, text)
                    })
                    .inner;
                let response = if matches!(member.who, edit::Who::Away { .. }) {
                    response.on_hover_text("away from the party")
                } else {
                    response
                };
                if response.clicked() {
                    picked = Some(index);
                }
            }
        });
        if let Some(index) = picked {
            self.member = index;
            self.selected = None;
        }
    }

    /// The saves, one line each, and the folder they are in.
    fn saves_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Saves").strong());
            if ui.small_button("Choose folder…").clicked()
                && let Some(folder) = rfd::FileDialog::new()
                    .set_title("Choose kjk's saves folder")
                    .pick_folder()
            {
                match locate::check_path(&folder) {
                    Found::Saves { .. } => self.use_root(folder),
                    other => self.note = Some(format!("{}: {other}", folder.display())),
                }
            }
        });
        ui.label(RichText::new(&self.found).small().weak());
        if let Some(root) = &self.root {
            ui.add(
                egui::Label::new(RichText::new(root.display().to_string()).small().weak())
                    .truncate(),
            )
            .on_hover_text(format!("kjk's saves folder: {}", root.display()));
        }
        if let Some(why) = &self.refused {
            ui.label(RichText::new(why).small().color(RED));
        }
        let open = self.open.as_ref().map(|open| open.row.id.clone());
        let mut selected = self
            .rows
            .iter()
            .position(|row| open.as_deref() == Some(row.id.as_str()))
            .map(|index| index as u64);
        let rows = &self.rows;
        let clicked = list::rows(
            ui,
            "saves",
            &[list::rest("save"), list::col("saved (UTC)", 96.0)],
            rows.len(),
            |index| {
                let row = &rows[index];
                Row {
                    key: index as u64,
                    cells: vec![row.name.clone(), row.saved.clone()],
                    grey: row.readable.is_err(),
                    warning: row
                        .readable
                        .as_ref()
                        .err()
                        .map(|why| format!("does not open: {why}")),
                    ..Row::default()
                }
            },
            &mut self.icons,
            None,
            &mut selected,
        );
        let picked = clicked.and_then(|clicked| {
            self.rows
                .get(clicked.key as usize)
                .map(|row| row.id.clone())
        });
        if let Some(id) = picked {
            if self
                .open
                .as_ref()
                .is_some_and(|open| !open.session.changes.is_empty())
            {
                self.note =
                    Some("this save has changes: save or undo them before opening another".into());
            } else {
                self.open_save(&id);
            }
        }
    }

    /// The changes made, undo and save.
    fn changes_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let count = self
                .open
                .as_ref()
                .map_or(0, |open| open.session.changes.len());
            let last = self
                .open
                .as_ref()
                .and_then(|open| open.session.changes.last())
                .map(|applied| applied.words.clone());
            ui.label(format!(
                "{count} change{}",
                if count == 1 { "" } else { "s" }
            ))
            .on_hover_ui(|ui| {
                if let Some(open) = &self.open {
                    for applied in &open.session.changes {
                        ui.label(&applied.words);
                    }
                }
            });
            if ui
                .add_enabled(count > 0, egui::Button::new("Undo").small())
                .clicked()
            {
                self.undo();
            }
            if ui
                .add_enabled(count > 0, egui::Button::new("Save…").small())
                .clicked()
            {
                self.pages.saving = true;
            }
            if let Some(last) = last {
                ui.label(RichText::new(format!("last: {last}")).small().weak());
            }
            if let Some(note) = &self.note {
                ui.label(RichText::new(note).color(RED));
            }
        });
    }

    /// The tabs, the search field and the page.
    fn page(&mut self, ui: &mut egui::Ui) {
        let Some(_) = &self.open else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Pick a save on the left to open it.").weak());
            });
            return;
        };
        ui.horizontal(|ui| {
            for tab in Tab::ALL {
                if ui.selectable_label(self.tab == tab, tab.title()).clicked() && self.tab != tab {
                    self.tab = tab;
                    self.selected = None;
                }
            }
            ui.separator();
            let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::F);
            if ui.input_mut(|input| input.consume_shortcut(&shortcut)) {
                self.focus_search = true;
            }
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text(format!("Search ({})", ui.ctx().format_shortcut(&shortcut)))
                    .desired_width(180.0),
            );
            if std::mem::take(&mut self.focus_search) {
                field.request_focus();
            }
            if !self.search.is_empty() && ui.small_button("x").clicked() {
                self.search.clear();
            }
        });
        pages::draw(self, ui);
    }
}
