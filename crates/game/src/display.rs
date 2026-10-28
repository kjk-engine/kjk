//! The game's window as the options have it (`settings`, `Display`): a
//! window of a size, borderless over its whole screen, or fullscreen in
//! one of the screen's own modes; v-sync; the samples a pixel takes on
//! every camera drawing to the window (they share its main texture only
//! while they agree); and the brightness, a gamma over the whole frame
//! (`ui_composite`). Alt+Enter goes between the window and borderless.
//! The screen resolution page offers (`vm.displays`) the sizes of common
//! windows that fit the window's screen, borderless, and the screen's own
//! modes. Sizes are the screen's pixels, not its points.
//!
//! macOS has no exclusive fullscreen worth the name (winit panics when the
//! screen's mode will not change): there fullscreen is borderless, and the
//! page offers no screen modes.

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::window::{
    Monitor, MonitorSelection, OnMonitor, PresentMode, PrimaryMonitor, PrimaryWindow,
    VideoModeSelection, WindowMode, WindowPosition,
};
use ui::intent::Intent;
use ui::vm::DisplayChoice;

use crate::ui_composite::UiComposite;
use crate::ui_view::{Ui, UiIntents};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Window,
    Borderless,
    Fullscreen,
}

impl DisplayMode {
    /// The mode a setting names (a window when it names none).
    pub fn named(name: &str) -> DisplayMode {
        match name {
            "borderless" => DisplayMode::Borderless,
            "fullscreen" => DisplayMode::Fullscreen,
            _ => DisplayMode::Window,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            DisplayMode::Window => "window",
            DisplayMode::Borderless => "borderless",
            DisplayMode::Fullscreen => "fullscreen",
        }
    }
}

/// How the window is shown.
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Display {
    pub mode: DisplayMode,
    /// The window's or the fullscreen mode's size (screen pixels); none:
    /// as it is (a window), the screen's own (fullscreen).
    pub size: Option<UVec2>,
    pub vsync: bool,
    /// Samples a pixel takes (1, 2 or 4).
    pub samples: u32,
    /// The frame's encoded values to the power of 1 / this.
    pub gamma: f32,
    /// The area's grass is drawn (`grass::option`).
    pub grass: bool,
}

impl Default for Display {
    fn default() -> Display {
        Display {
            mode: DisplayMode::Window,
            size: None,
            vsync: true,
            samples: 4,
            gamma: 1.0,
            grass: true,
        }
    }
}

/// A size a setting names (`1920x1080`).
pub fn size_named(name: &str) -> Option<UVec2> {
    let (width, height) = name.split_once('x')?;
    let size = UVec2::new(width.trim().parse().ok()?, height.trim().parse().ok()?);
    (size.x > 0 && size.y > 0).then_some(size)
}

fn size_name(size: UVec2) -> String {
    format!("{}x{}", size.x, size.y)
}

/// The brightness slider's gamma: a half to twice, 1 in the middle.
pub fn gamma(fraction: f32) -> f32 {
    2f32.powf((fraction.clamp(0.0, 1.0) - 0.5) * 2.0)
}

/// Windows offered where they fit the screen: 4:3, 16:10, 16:9, 21:9.
const WINDOWS: [(u32, u32); 16] = [
    (1024, 768),
    (1280, 720),
    (1280, 800),
    (1280, 960),
    (1366, 768),
    (1440, 900),
    (1600, 900),
    (1600, 1200),
    (1680, 1050),
    (1920, 1080),
    (1920, 1200),
    (2560, 1080),
    (2560, 1440),
    (2560, 1600),
    (3440, 1440),
    (3840, 2160),
];

/// Whether the screen's own modes are offered (`fullscreen`).
const EXCLUSIVE: bool = !cfg!(target_os = "macos");

/// The smallest fullscreen mode offered.
const SMALLEST: UVec2 = UVec2::new(800, 600);

pub struct DisplayPlugin;

impl Plugin for DisplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Display>()
            .add_systems(Update, (alt_enter, offer))
            .add_systems(Last, (show, samples, brightness, facts));
    }
}

/// The screen the window is on, else the primary one.
fn screen<'a>(
    window: Option<&OnMonitor>,
    monitors: &'a Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
) -> Option<&'a Monitor> {
    let on = window.map(|on| on.0);
    monitors
        .iter()
        .find(|(entity, ..)| Some(*entity) == on)
        .or_else(|| monitors.iter().find(|(_, _, primary)| *primary))
        .or_else(|| monitors.iter().next())
        .map(|(_, monitor, _)| monitor)
}

/// The screen's sizes it can be put in, the largest first.
fn modes(monitor: &Monitor) -> Vec<UVec2> {
    let mut sizes: Vec<UVec2> = monitor
        .video_modes
        .iter()
        .map(|mode| mode.physical_size)
        .filter(|size| size.x >= SMALLEST.x && size.y >= SMALLEST.y)
        .collect();
    sizes.sort_by_key(|size| std::cmp::Reverse((size.x * size.y, size.x)));
    sizes.dedup();
    sizes
}

/// What the resolution page offers: windows that fit the screen (with
/// room for a title bar), the largest first; borderless; the screen's
/// own modes. The one `window` is shown as now is marked.
fn choices(monitor: &Monitor, window: &Window) -> Vec<DisplayChoice> {
    let full = UVec2::new(monitor.physical_width, monitor.physical_height);
    let (shown, now) = match window.mode {
        WindowMode::Windowed => (DisplayMode::Window, Some(window.resolution.physical_size())),
        WindowMode::BorderlessFullscreen(_) => (DisplayMode::Borderless, None),
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(mode)) => {
            (DisplayMode::Fullscreen, Some(mode.physical_size))
        }
        WindowMode::Fullscreen(..) => (DisplayMode::Fullscreen, Some(full)),
    };
    let choice = |mode: DisplayMode, size: Option<UVec2>, label: String| DisplayChoice {
        mode: mode.name().into(),
        size: size.map_or_else(String::new, size_name),
        label,
        current: mode == shown && size == now,
    };
    let windows = WINDOWS
        .iter()
        .rev()
        .map(|&(x, y)| UVec2::new(x, y))
        .filter(|size| size.x <= full.x && size.y < full.y)
        .map(|size| {
            let label = format!("Window {} x {}", size.x, size.y);
            choice(DisplayMode::Window, Some(size), label)
        });
    let borderless = choice(
        DisplayMode::Borderless,
        None,
        format!("Borderless {} x {}", full.x, full.y),
    );
    let exclusive = if EXCLUSIVE {
        modes(monitor)
    } else {
        Vec::new()
    };
    let fullscreen = exclusive.into_iter().map(|size| {
        let label = format!("Fullscreen {} x {}", size.x, size.y);
        choice(DisplayMode::Fullscreen, Some(size), label)
    });
    windows.chain([borderless]).chain(fullscreen).collect()
}

/// The resolution page's choices, for the screen the window is on now.
fn offer(
    windows: Query<(&Window, Option<&OnMonitor>), With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
    ui: Option<ResMut<Ui>>,
) {
    let (Some(mut ui), Ok((window, on))) = (ui, windows.single()) else {
        return;
    };
    let offered = screen(on, &monitors)
        .map(|monitor| choices(monitor, window))
        .unwrap_or_else(Vec::new);
    if ui.vm.displays != offered {
        ui.vm.displays = offered;
    }
}

/// Alt+Enter: the window borderless, or back to a window.
fn alt_enter(
    keys: Res<ButtonInput<KeyCode>>,
    display: Res<Display>,
    intents: Option<ResMut<UiIntents>>,
) {
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let enter = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]);
    let Some(mut intents) = intents.filter(|_| alt && enter) else {
        return;
    };
    let mode = match display.mode {
        DisplayMode::Window => DisplayMode::Borderless,
        _ => DisplayMode::Window,
    };
    intents
        .0
        .push(Intent::Setting("display.mode".into(), mode.name().into()));
}

/// The window as `Display` has it: mode and size when either changed,
/// v-sync always. A fullscreen size waits for its screen's modes to be
/// known (the screen's own mode meanwhile).
fn show(
    display: Res<Display>,
    mut windows: Query<(&mut Window, Option<&OnMonitor>), With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
    mut shown: Local<Option<(DisplayMode, Option<UVec2>)>>,
) {
    let Ok((mut window, on)) = windows.single_mut() else {
        return;
    };
    let present = if display.vsync {
        PresentMode::AutoVsync
    } else {
        PresentMode::AutoNoVsync
    };
    if window.present_mode != present {
        window.present_mode = present;
    }
    let wanted = (display.mode, display.size);
    if *shown == Some(wanted) {
        return;
    }
    let borderless = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    let mut settled = true;
    let mode = match display.mode {
        DisplayMode::Window => WindowMode::Windowed,
        DisplayMode::Borderless => borderless,
        DisplayMode::Fullscreen if !EXCLUSIVE => borderless,
        // One of the screen's own modes, borderless until they are known
        // (or when the size is none of them).
        DisplayMode::Fullscreen => {
            let modes = screen(on, &monitors).map(|monitor| &monitor.video_modes);
            settled = modes.is_some();
            let best = display.size.zip(modes).and_then(|(size, modes)| {
                modes
                    .iter()
                    .filter(|mode| mode.physical_size == size)
                    .max_by_key(|mode| (mode.refresh_rate_millihertz, mode.bit_depth))
            });
            best.map_or(borderless, |mode| {
                WindowMode::Fullscreen(
                    MonitorSelection::Current,
                    VideoModeSelection::Specific(*mode),
                )
            })
        }
    };
    if window.mode != mode {
        window.mode = mode;
    }
    if !settled {
        return;
    }
    *shown = Some(wanted);
    if display.mode == DisplayMode::Window
        && let Some(size) = display.size
    {
        window.resolution.set_physical_resolution(size.x, size.y);
        window.position = WindowPosition::Centered(MonitorSelection::Current);
    }
    let size = display.size;
    info!(target: "display", mode = ?window.mode, ?size, "window");
}

/// Every camera drawing to the window takes the samples `Display` has.
fn samples(
    display: Res<Display>,
    mut cameras: Query<(&mut Msaa, Option<&RenderTarget>), With<Camera>>,
) {
    let wanted = match display.samples {
        1 => Msaa::Off,
        2 => Msaa::Sample2,
        _ => Msaa::Sample4,
    };
    for (mut msaa, target) in &mut cameras {
        let window = target.is_none_or(|target| matches!(target, RenderTarget::Window(_)));
        if window && *msaa != wanted {
            *msaa = wanted;
        }
    }
}

/// The composite's gamma (the brightness slider).
fn brightness(display: Res<Display>, mut composites: Query<&mut UiComposite>) {
    for mut composite in &mut composites {
        if composite.gamma != display.gamma {
            composite.gamma = display.gamma;
        }
    }
}

/// `dump`'s `display`: the window as it is, and what the options ask.
fn facts(
    display: Res<Display>,
    windows: Query<&Window, With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    facts.0.insert(
        "display".into(),
        serde_json::json!({
            "mode": format!("{:?}", window.mode),
            "physical": [window.resolution.physical_width(), window.resolution.physical_height()],
            "scale": window.resolution.scale_factor(),
            "present": format!("{:?}", window.present_mode),
            "samples": display.samples,
            "gamma": display.gamma,
            "screens": monitors
                .iter()
                .map(|(_, monitor, primary)| {
                    serde_json::json!({
                        "size": [monitor.physical_width, monitor.physical_height],
                        "primary": primary,
                        "modes": monitor.video_modes.len(),
                    })
                })
                .collect::<Vec<_>>(),
        }),
    );
}
