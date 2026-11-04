//! The save screen: every change in words with its warning, what the
//! write will check and what it means for older saves; then the result.

use egui::RichText;

use crate::ui::App;
use crate::ui::shell::{AMBER, RED};
use save_editor::model::{locate, write};

/// How a write went, as the screen shows it.
pub enum Outcome {
    Saved(String),
    Refused { why: String, saved_over: bool },
}

impl App {
    /// The save screen, while it is open.
    pub fn save_dialog(&mut self, ctx: &egui::Context) {
        if !self.pages.saving {
            return;
        }
        let mut open = true;
        let mut write = None;
        egui::Window::new("Save")
            .collapsible(false)
            .resizable(true)
            .default_width(460.0)
            .open(&mut open)
            .show(ctx, |ui| {
                let Some(open) = &self.open else {
                    return;
                };
                let session = &open.session;
                if let Some(outcome) = &self.outcome {
                    match outcome {
                        Outcome::Saved(words) => {
                            ui.label(RichText::new(words).strong());
                        }
                        Outcome::Refused { why, saved_over } => {
                            ui.label(RichText::new(why).color(RED));
                            if *saved_over && ui.button("Save anyway").clicked() {
                                write = Some(true);
                            }
                        }
                    }
                    return;
                }
                ui.label(RichText::new(format!("{} into {}", open.row.name, open.row.id)).strong());
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for applied in &session.changes {
                        ui.label(&applied.words);
                        if let Some(warning) = &applied.warning {
                            ui.label(RichText::new(format!("    ! {warning}")).small().color(AMBER));
                        }
                    }
                });
                ui.separator();
                let installed = locate::game_of(&session.opened).and_then(|game| self.discovery.get(game));
                match installed {
                    Some(install) => ui.label(format!(
                        "Checked first: read back, then loaded in its module with {} and stepped {} ticks.",
                        install.root.display(),
                        write::TICKS
                    )),
                    None => ui.label(
                        RichText::new("Checked first by reading it back. Its game's install is not found, so loading it is skipped.")
                            .color(AMBER),
                    ),
                };
                if let Some(note) = write::older_note(session) {
                    ui.label(RichText::new(note).color(AMBER));
                }
                ui.label(RichText::new(match &session.backup {
                    Some(backup) => format!("Backed up already this session: {}", backup.display()),
                    None => "The save as it is now is backed up beside it first.".to_string(),
                }).small().weak());
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        write = Some(false);
                    }
                    if ui.button("Cancel").clicked() {
                        self.pages.saving = false;
                    }
                });
            });
        if !open {
            self.pages.saving = false;
        }
        if !self.pages.saving {
            self.outcome = None;
        }
        if let Some(force) = write {
            self.write(force);
        }
    }

    /// The open save written, the outcome kept for the screen.
    fn write(&mut self, force: bool) {
        // Written only into the folder it was opened from.
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let Some(root) = open.session.root.clone() else {
            return;
        };
        let result = write::save(&mut open.session, &root, Some(&self.discovery), force);
        self.outcome = Some(match result {
            Ok(saved) => {
                let checked = match saved.checked {
                    edit::Checked::Stepped { ticks } => {
                        format!("Checked by loading it ({ticks} ticks).")
                    }
                    edit::Checked::RoundTrip => {
                        "Checked by reading it back (no install to load it in).".into()
                    }
                };
                Outcome::Saved(format!(
                    "Saved. {checked} Backup: {}",
                    saved.backup.display()
                ))
            }
            Err(why) => Outcome::Refused {
                saved_over: why.starts_with("kjk saved over"),
                why,
            },
        });
        self.refresh_members();
    }
}
