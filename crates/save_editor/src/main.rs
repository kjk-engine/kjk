//! `kjk-save-editor`: a window for opening a kjk save, changing it and
//! writing it back.

// A window program on Windows: no console beside it in a release build.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

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
    // The system's own font (none ships with the editor): without one there
    // is no window to write in, so say so and stop.
    let fonts = match save_editor::model::fonts::system() {
        Ok(fonts) => fonts,
        Err(why) => {
            let words = format!(
                "kjk save editor cannot start: {why}. Install a common font (DejaVu Sans or Noto Sans on Linux) and try again."
            );
            eprintln!("{words}");
            rfd::MessageDialog::new()
                .set_title("kjk save editor")
                .set_description(&words)
                .set_level(rfd::MessageLevel::Error)
                .show();
            std::process::exit(1);
        }
    };
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
        Box::new(|cc| {
            ui::use_fonts(&cc.egui_ctx, fonts);
            Ok(Box::new(ui::App::new(&cc.egui_ctx)))
        }),
    )
}
