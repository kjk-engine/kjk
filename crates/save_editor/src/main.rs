//! `kjk-save-editor`: a window for opening a kjk save, changing it and
//! writing it back.

mod ui;

const USAGE: &str = "usage: kjk-save-editor [--help]\n\
Opens kjk's save editor. It finds kjk's saves and the games' installs by itself.";

fn main() -> eframe::Result {
    if std::env::args()
        .skip(1)
        .any(|arg| arg == "--help" || arg == "-h")
    {
        println!("kjk save editor {}\n{USAGE}", save_editor::VERSION);
        return Ok(());
    }
    let title = format!("kjk save editor {}", save_editor::VERSION);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(&title)
            .with_inner_size([1100.0, 680.0])
            .with_min_inner_size([720.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        &title,
        options,
        Box::new(|cc| Ok(Box::new(ui::App::new(&cc.egui_ctx)))),
    )
}
