//! A picture of the window, for showing the editor's look without a screen
//! grab: `KJK_EDITOR_SHOT=<file.png>` saves the window once it has drawn a
//! few frames, then closes it.

use std::path::PathBuf;
use std::sync::Arc;

/// Frames drawn before the picture, so lazily loaded icons are in it.
const SETTLE: u32 = 20;

#[derive(Default)]
pub struct Shot {
    to: Option<PathBuf>,
    frames: u32,
    asked: bool,
}

impl Shot {
    /// The picture the environment asks for, if any.
    pub fn from_env() -> Shot {
        Shot {
            to: std::env::var_os("KJK_EDITOR_SHOT").map(PathBuf::from),
            ..Shot::default()
        }
    }

    /// Counts a frame, asks for the picture once settled and nothing is
    /// being read (`busy`), and closes the
    /// window once it is written.
    pub fn frame(&mut self, ctx: &egui::Context, busy: bool) {
        let Some(to) = self.to.clone() else {
            return;
        };
        self.frames += 1;
        ctx.request_repaint();
        if self.frames < SETTLE || busy || self.asked {
            return;
        }
        self.asked = true;
        let ctx2 = ctx.clone();
        ctx.request_screenshot(move |image: Arc<egui::ColorImage>| {
            let [w, h] = image.size;
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            match image::RgbaImage::from_raw(w as u32, h as u32, bytes).map(|img| img.save(&to)) {
                Some(Ok(())) => eprintln!("saved {}", to.display()),
                other => eprintln!("could not save {}: {other:?}", to.display()),
            }
            ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            ctx2.request_repaint();
        });
    }
}
