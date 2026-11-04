//! The editor's window.

mod shot;

/// The app eframe runs.
pub struct App {
    shot: shot::Shot,
}

impl App {
    pub fn new() -> App {
        App {
            shot: shot::Shot::from_env(),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.label("No save open.");
        });
        self.shot.frame(ui.ctx());
    }
}
