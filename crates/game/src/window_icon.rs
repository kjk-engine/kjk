//! The window's icon: Windows' title bar and taskbar, Linux's docks. macOS
//! takes the app's own (`kjk.app`'s `kjk.icns`) and has no window icons.
//! Rendered from the logo (`docs/logo/kjk.svg`) by `cargo xtask icon`.

use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy::winit::WINIT_WINDOWS;

const ICON: &[u8] = include_bytes!("../icon.png");

pub struct WindowIconPlugin;

impl Plugin for WindowIconPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, set_icon);
    }
}

/// Sets the icon once winit has made the window (the first frames).
fn set_icon(
    window: Query<Entity, With<PrimaryWindow>>,
    mut set: Local<bool>,
    _main: NonSendMarker,
) {
    let Ok(entity) = window.single() else {
        return;
    };
    if *set {
        return;
    }
    WINIT_WINDOWS.with_borrow(|windows| {
        let Some(window) = windows.get_window(entity) else {
            return;
        };
        *set = true;
        let icon = image::load_from_memory_with_format(ICON, image::ImageFormat::Png)
            .map_err(|error| error.to_string())
            .and_then(|image| {
                let image = image.into_rgba8();
                let (width, height) = image.dimensions();
                winit::window::Icon::from_rgba(image.into_raw(), width, height)
                    .map_err(|error| error.to_string())
            });
        match icon {
            Ok(icon) => window.set_window_icon(Some(icon)),
            Err(error) => warn!("window icon: {error}"),
        }
    });
}
