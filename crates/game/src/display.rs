//! The game's window as the options have it (`settings`, `Display`): a
//! window of a size, borderless over its whole screen, or fullscreen in
//! one of the screen's own modes; v-sync; a frame rate limit (`pace`); the
//! samples a pixel takes on every camera drawing to the window (they share
//! its main texture only while they agree); and the brightness, a gamma
//! over the whole frame (`ui_composite`). Alt+Enter goes between the window and borderless.
//! The screen resolution page offers (`vm.displays`) the sizes of common
//! windows that fit the window's screen, borderless, and the screen's own
//! modes. Sizes are the screen's pixels, not its points.
//!
//! Only Linux uses the screen's own modes (`exclusive_on`). On macOS and
//! Windows fullscreen is borderless over the whole screen, and the page
//! offers no screen modes. macOS has no exclusive fullscreen worth the name
//! (winit panics when the screen's mode will not change). On Windows,
//! winit changes the whole desktop's mode and makes the window topmost,
//! while the render thread may still be presenting to the swapchain made
//! for the old mode. A player on Vulkan with an NVIDIA card (v0.17.0) had
//! the GPU device lost one second after the switch. Bevy quits on a lost
//! device at the next frame, and its log never said so, so that frame never
//! came. The topmost window then covered the screen at the changed mode
//! with nothing answering, and the whole PC seemed frozen until it was
//! forced off. Borderless changes no mode and is not topmost, and
//! Alt+Enter's switch to it worked for both players who reported this.
//!
//! A change of display is put to the window once (`show`). A fullscreen
//! mode waits for its screen to be known, with the window left as it is
//! meanwhile, so no mode is put to the screen twice.
//!
//! What the settings ask is put to the screen and the GPU first
//! (`placed`): a window bigger than the screen, or than the GPU draws, and a
//! mode the screen no longer has are not tried. A display other than the
//! default is on trial until frames have run a while (`TRIAL`, a file
//! beside the settings): a start that finds the file opens in a window and
//! says so (`recover`); `kjk --reset-display` does the same by hand.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use bevy::app::MainScheduleOrder;
use bevy::camera::RenderTarget;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::render::renderer::RenderDevice;
use bevy::window::{
    Monitor, MonitorSelection, OnMonitor, PresentMode, PrimaryMonitor, PrimaryWindow,
    VideoModeSelection, WindowMode, WindowPosition,
};
use ui::vm::DisplayChoice;

use crate::settings::Settings;
use crate::temporal::Antialias;
use crate::ui_composite::UiComposite;
use crate::ui_view::Ui;

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
    /// The most frames a second the game draws (`pace`); none: as many as
    /// the GPU or v-sync allow.
    pub frame_limit: Option<u32>,
    /// Multisampling, TAA or DLSS (`temporal`).
    pub antialias: Antialias,
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
            frame_limit: None,
            antialias: Antialias::Msaa(4),
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

/// A display as the resolution page names it (`Window 1280 x 720`).
fn label(mode: DisplayMode, size: Option<UVec2>) -> String {
    let name = match mode {
        DisplayMode::Window => "Window",
        DisplayMode::Borderless => "Borderless",
        DisplayMode::Fullscreen => "Fullscreen",
    };
    size.map_or_else(
        || name.to_string(),
        |size| format!("{name} {} x {}", size.x, size.y),
    )
}

/// Whether a window of `size` fits a screen of `full`, with room for its
/// title bar.
fn fits_window(size: UVec2, full: UVec2) -> bool {
    size.x <= full.x && size.y < full.y
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

/// Whether the screen's own modes are offered and used on the system named
/// `os` (`std::env::consts::OS`). Windows and macOS take fullscreen as
/// borderless (see the module's notes).
fn exclusive_on(os: &str) -> bool {
    !matches!(os, "windows" | "macos")
}

/// Whether this system's fullscreen uses the screen's own modes
/// (`exclusive_on`). A resource, so a probe can try either.
#[derive(Resource, Debug, Clone, Copy)]
struct ScreenModes {
    exclusive: bool,
}

impl Default for ScreenModes {
    fn default() -> ScreenModes {
        ScreenModes {
            exclusive: exclusive_on(std::env::consts::OS),
        }
    }
}

/// The smallest fullscreen mode offered.
const SMALLEST: UVec2 = UVec2::new(800, 600);

pub struct DisplayPlugin;

impl Plugin for DisplayPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Display>()
            .init_resource::<Trial>()
            .init_resource::<ScreenModes>()
            .add_systems(Update, (alt_enter, offer))
            .add_systems(
                PostUpdate,
                cameras_in_step.before(bevy::camera::CameraUpdateSystems),
            )
            .add_systems(Last, (show, trial_settles, brightness, facts))
            .add_systems(Pace, pace);
    }

    /// `Pace` runs last of all the frame's schedules, after every plugin
    /// has put its own in order (the vitals' `FrameEnd` among them).
    fn finish(&self, app: &mut App) {
        app.world_mut()
            .resource_mut::<MainScheduleOrder>()
            .labels
            .push(Pace.intern());
    }
}

/// The schedule that holds a frame for the frame rate limit. It runs after
/// the frame's work, so the time spent waiting is not counted as work by
/// the vitals of a dump.
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct Pace;

/// The most frames a second the game draws in play when v-sync is off and
/// the player set no frame rate limit. Without it a graphics card runs at
/// its full power draw all the time, and a weak power supply can switch the
/// whole computer off under that load (issue 318).
const PLAY_CEILING: u32 = 240;

/// The most frames a second the game draws while a screen is open or the
/// window is in the background. A menu draws little, so without this it
/// would draw many hundreds of frames a second for nothing.
const RESTING_CEILING: u32 = 60;

/// Whether the safety ceilings are switched off (`KJK_UNCAPPED=1`). The
/// benchmark sets it, because it measures how fast the game can draw.
fn uncapped() -> bool {
    std::env::var("KJK_UNCAPPED").is_ok_and(|value| value == "1")
}

/// The frame rate the pacer holds frames to, or none to leave them alone.
/// The player's own limit counts first. When v-sync is off and no limit is
/// set, play is held to `PLAY_CEILING`. A screen or a background window is
/// held to `RESTING_CEILING` whether v-sync is on or off.
fn paced_rate(display: &Display, resting: bool, ceilings: bool) -> Option<u32> {
    if !ceilings {
        return display.frame_limit;
    }
    let play = display
        .frame_limit
        .or((!display.vsync).then_some(PLAY_CEILING));
    if resting {
        return Some(play.map_or(RESTING_CEILING, |rate| rate.min(RESTING_CEILING)));
    }
    play
}

/// Holds each frame until its time while a frame rate is due (`paced_rate`),
/// so the GPU and CPU rest between frames. It runs in every frame the app
/// runs, a menu's too. The ceilings apply only to a game with a window, so
/// headless runs keep their own pace. The sim keeps its own clock
/// (`sim_host`), so a limit changes how many ticks a frame steps, never what
/// a tick does.
fn pace(
    display: Res<Display>,
    windows: Query<&Window, With<PrimaryWindow>>,
    screens: Option<Res<crate::ui_view::UiOpen>>,
    mut ceilings: Local<Option<bool>>,
    mut next: Local<Option<Instant>>,
) {
    let window = windows.single().ok();
    let resting =
        window.is_some_and(|window| !window.focused) || screens.is_some_and(|open| open.0);
    let ceilings = window.is_some() && *ceilings.get_or_insert_with(|| !uncapped());
    let Some(fps) = paced_rate(&display, resting, ceilings) else {
        *next = None;
        return;
    };
    let period = Duration::from_secs_f64(1.0 / f64::from(fps));
    let (wait, due) = paced(*next, Instant::now(), period);
    *next = Some(due);
    if !wait.is_zero() {
        std::thread::sleep(wait);
    }
}

/// How long a frame ending at `now` waits, and when the next frame is due:
/// a period after the one before, so a frame that waited too long is made
/// up by the next one's shorter wait. A frame later than a whole period
/// waits for nothing and starts the count again from itself.
fn paced(due: Option<Instant>, now: Instant, period: Duration) -> (Duration, Instant) {
    let due = due.map_or(now, |last| last + period);
    if due + period <= now {
        return (Duration::ZERO, now);
    }
    (due.saturating_duration_since(now), due)
}

/// Every camera drawing to the window takes the window's new size in the
/// frame it changes. Bevy sizes a camera anew only when the window's
/// resize message comes, or when the camera is new or its projection
/// changes; but the window holds the size `show` asks for at once, and the
/// message comes only once the system has resized the window (later, on
/// Windows; a fullscreen mode's change takes a second). A camera made
/// meanwhile (a movie's, a fade's) took the new size while the others
/// kept the old, and the cameras on one window share the colour texture
/// the first of them sized: the new one's depth no longer matched it, and
/// wgpu ended the game ("Attachments have differing sizes": a resolution
/// changed in the options, or the settings' size taken at start).
fn cameras_in_step(
    windows: Query<&Window, (With<PrimaryWindow>, Changed<Window>)>,
    mut seen: Local<Option<(UVec2, f32)>>,
    mut cameras: Query<(&RenderTarget, &mut Projection)>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let now = (
        window.resolution.physical_size(),
        window.resolution.scale_factor(),
    );
    if seen.replace(now) == Some(now) {
        return;
    }
    // A changed projection has Bevy size its camera from the window again.
    for (target, mut projection) in &mut cameras {
        if matches!(target, RenderTarget::Window(_)) {
            projection.set_changed();
        }
    }
}

#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    vec![
        ("cameras_share_a_resize", cameras_share_a_resize()),
        ("stored_displays_fit", stored_displays_fit()),
        ("a_start_that_failed_opens_a_window", failed_start()),
        ("frame_limit_paces_frames", frame_limit_paces()),
        ("frame_rate_ceilings", frame_rate_ceilings()),
        ("display_modes_kept", display_modes_kept()),
        (
            "fullscreen_is_borderless_on_windows",
            fullscreen_by_system(),
        ),
        ("a_display_change_is_put_once", change_put_once()),
    ]
}

/// A 1920 x 1080 screen at 60 Hz with that one mode.
#[cfg(feature = "probes")]
fn probe_screen() -> Monitor {
    use bevy::window::VideoMode;
    Monitor {
        name: None,
        physical_width: 1920,
        physical_height: 1080,
        physical_position: IVec2::ZERO,
        refresh_rate_millihertz: Some(60_000),
        scale_factor: 1.0,
        video_modes: vec![VideoMode {
            physical_size: UVec2::new(1920, 1080),
            bit_depth: 32,
            refresh_rate_millihertz: 60_000,
        }],
    }
}

/// Windows and macOS use no screen modes and Linux does. Where none are
/// used, the resolution page offers no fullscreen rows, and a stored
/// fullscreen of 1920 x 1080 opens borderless over the screen. Where they
/// are used, the page offers the screen's 1920 x 1080 mode.
#[cfg(feature = "probes")]
fn fullscreen_by_system() -> Result<String, String> {
    let systems = ["windows", "macos", "linux"].map(|os| (os, exclusive_on(os)));
    let screen = probe_screen();
    let window = Window::default();
    let offered = |exclusive: bool| -> Vec<String> {
        choices(&screen, &window, exclusive)
            .into_iter()
            .filter(|choice| choice.mode == "fullscreen")
            .map(|choice| choice.label)
            .collect()
    };
    let (without, with) = (offered(false), offered(true));
    let stored = placed(
        DisplayMode::Fullscreen,
        Some(UVec2::new(1920, 1080)),
        Some((Entity::PLACEHOLDER, &screen)),
        u32::MAX,
        false,
    );
    let borderless = Placed {
        mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
        size: None,
        settled: true,
    };
    sim::probe::check(
        systems == [("windows", false), ("macos", false), ("linux", true)]
            && without.is_empty()
            && with == ["Fullscreen 1920 x 1080"]
            && stored == borderless,
        format!(
            "screen modes used {systems:?}; fullscreen rows without {without:?}, with {with:?}; stored fullscreen {:?}",
            stored.mode
        ),
    )
}

/// `show` over seven frames for each display, with the screen known only
/// from the third: the window's mode changes once, to the display's own,
/// and no mode is put meanwhile. Fullscreen 1920 x 1080 with the screen's
/// modes waits for the screen and then takes its mode; fullscreen without
/// them and borderless are borderless from the first frame. The same
/// display asked again changes nothing, and a window after it is one
/// change more. The trial's file names the display tried.
#[cfg(feature = "probes")]
fn change_put_once() -> Result<String, String> {
    #[derive(Resource, Default)]
    struct Seen(Vec<(u32, WindowMode)>);

    fn record(
        windows: Query<&Window, With<PrimaryWindow>>,
        mut frame: Local<u32>,
        mut last: Local<Option<WindowMode>>,
        mut seen: ResMut<Seen>,
    ) {
        *frame += 1;
        let Ok(window) = windows.single() else {
            return;
        };
        let was = last.replace(window.mode).unwrap_or(WindowMode::Windowed);
        if was != window.mode {
            seen.0.push((*frame, window.mode));
        }
    }

    let folder = std::env::temp_dir().join(format!("kjk-display-once-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let file = folder.join(TRIAL);
    let run = |mode: DisplayMode,
               size: Option<UVec2>,
               exclusive: bool|
     -> (Vec<(u32, WindowMode)>, Option<String>) {
        let _ = std::fs::remove_file(&file);
        let mut app = App::new();
        app.init_resource::<Time<Real>>()
            .init_resource::<Seen>()
            .insert_resource(ScreenModes { exclusive })
            .insert_resource(Trial {
                since: None,
                file: Some(file.clone()),
            })
            .insert_resource(Display {
                mode,
                size,
                ..Display::default()
            })
            .add_systems(Update, (show, record).chain());
        app.world_mut().spawn((Window::default(), PrimaryWindow));
        for frame in 1..=7 {
            if frame == 3 {
                app.world_mut().spawn((probe_screen(), PrimaryMonitor));
            }
            if frame == 5 {
                // The same display asked again.
                app.world_mut().resource_mut::<Display>().set_changed();
            }
            app.update();
        }
        let noted = std::fs::read_to_string(&file)
            .ok()
            .and_then(|text| text.lines().nth(1).map(str::to_string));
        app.world_mut().resource_mut::<Display>().mode = DisplayMode::Window;
        app.update();
        (
            std::mem::take(&mut app.world_mut().resource_mut::<Seen>().0),
            noted,
        )
    };
    let size = Some(UVec2::new(1920, 1080));
    let fullscreen = run(DisplayMode::Fullscreen, size, true);
    let as_borderless = run(DisplayMode::Fullscreen, size, false);
    let borderless = run(DisplayMode::Borderless, None, false);
    let _ = std::fs::remove_dir_all(&folder);

    let full = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    let exclusive = |(_, mode): &(u32, WindowMode)| {
        matches!(mode, WindowMode::Fullscreen(_, VideoModeSelection::Specific(video))
            if video.physical_size == UVec2::new(1920, 1080))
    };
    let shape =
        |seen: &[(u32, WindowMode)]| -> Vec<u32> { seen.iter().map(|(at, _)| *at).collect() };
    let ok = shape(&fullscreen.0) == [3, 8]
        && exclusive(&fullscreen.0[0])
        && as_borderless.0 == [(1, full), (8, WindowMode::Windowed)]
        && borderless.0 == [(1, full), (8, WindowMode::Windowed)]
        && [&fullscreen.1, &as_borderless.1, &borderless.1].map(|noted| noted.as_deref())
            == [
                Some("Fullscreen 1920 x 1080"),
                Some("Fullscreen 1920 x 1080"),
                Some("Borderless"),
            ];
    sim::probe::check(
        ok,
        format!(
            "fullscreen with modes {:?}; without {:?}; borderless {:?}; trial noted {:?}",
            fullscreen.0,
            as_borderless.0,
            borderless.0,
            [&fullscreen.1, &as_borderless.1, &borderless.1]
        ),
    )
}

/// Each display mode chosen on the resolution page is saved, and two starts
/// after it, each with its trial run out, open in it: a window of 1280 x
/// 720, borderless, and fullscreen at 1920 x 1080. Alt+Enter's choice is
/// saved too: a window becomes borderless, and borderless and fullscreen
/// become a window.
#[cfg(feature = "probes")]
fn display_modes_kept() -> Result<String, String> {
    let folder = std::env::temp_dir().join(format!("kjk-display-modes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let (path, trial) = (folder.join("settings.json"), folder.join(TRIAL));
    let shown = |display: &Display| (display.mode, display.size);
    // The display a start opens in, after it ran long enough for its trial
    // to end.
    let start = || {
        let mut settings = Settings::load_from(Some(path.clone()));
        let reset = recover_from(&mut settings, &trial);
        let display = settings.display();
        begin_trial(&trial, &label(display.mode, display.size));
        end_trial(&trial);
        (reset.is_some(), shown(&display))
    };
    let chosen = [
        (DisplayMode::Window, Some(UVec2::new(1280, 720))),
        (DisplayMode::Borderless, None),
        (DisplayMode::Fullscreen, Some(UVec2::new(1920, 1080))),
    ];
    let mut kept = Vec::new();
    for (mode, size) in chosen {
        let mut settings = Settings::load_from(Some(path.clone()));
        settings.apply("display.size", &size.map_or_else(String::new, size_name));
        settings.apply("display.mode", mode.name());
        kept.push([start(), start()]);
    }
    let wanted = chosen.map(|display| [(false, display); 2]);
    // Alt+Enter from fullscreen, then twice more, each read back by a start.
    let switched: Vec<DisplayMode> = (0..3)
        .map(|_| {
            switch_window(&mut Settings::load_from(Some(path.clone())));
            start().1.0
        })
        .collect();
    let _ = std::fs::remove_dir_all(&folder);
    let switches = [
        DisplayMode::Window,
        DisplayMode::Borderless,
        DisplayMode::Window,
    ];
    sim::probe::check(
        kept == wanted && switched == switches,
        format!("starts {kept:?}; Alt+Enter {switched:?}"),
    )
}

/// The frame rate limit's setting and pacing at 50 frames a second (20 ms
/// a frame): 0 and words set no limit; a first frame waits for nothing; a
/// frame done 5 ms after the one before waits 15 ms; one that woke 2 ms
/// late and then took 5 ms waits 13 ms, so the rate holds; one 30 ms late
/// waits for nothing and counts again from itself.
#[cfg(feature = "probes")]
fn frame_limit_paces() -> Result<String, String> {
    let named = ["0", "", "off", "60", " 144 "].map(ui::screens::options::frame_limit);
    let ms = Duration::from_millis;
    let period = ms(20);
    let start = Instant::now();
    let first = paced(None, start, period);
    let early = paced(Some(start), start + ms(5), period);
    let overslept = paced(Some(start + ms(20)), start + ms(27), period);
    let late = paced(Some(start), start + ms(50), period);
    let wanted = (
        [None, None, None, Some(60), Some(144)],
        (Duration::ZERO, start),
        (ms(15), start + ms(20)),
        (ms(13), start + ms(40)),
        (Duration::ZERO, start + ms(50)),
    );
    let got = (named, first, early, overslept, late);
    sim::probe::check(
        got == wanted,
        format!("{:?}", (named, first.0, early.0, overslept.0, late.0)),
    )
}

/// `paced_rate` for each mix of v-sync, a player's limit (none, 30 or 144),
/// a screen or background window, and the ceilings on or off. Play with
/// v-sync on and no limit is left alone. Play with v-sync off and no limit
/// is held to 240. A screen is held to 60, or to the player's limit when it
/// is lower. With the ceilings off (`KJK_UNCAPPED`), only the player's limit
/// counts.
#[cfg(feature = "probes")]
fn frame_rate_ceilings() -> Result<String, String> {
    let rows = [
        // (vsync, limit, resting, ceilings, wanted)
        (true, None, false, true, None),
        (false, None, false, true, Some(PLAY_CEILING)),
        (false, Some(144), false, true, Some(144)),
        (true, None, true, true, Some(RESTING_CEILING)),
        (false, None, true, true, Some(RESTING_CEILING)),
        (false, Some(30), true, true, Some(30)),
        (false, Some(144), true, true, Some(RESTING_CEILING)),
        (false, None, true, false, None),
        (true, Some(144), true, false, Some(144)),
    ];
    let wrong: Vec<_> = rows
        .iter()
        .filter(|&&(vsync, frame_limit, resting, ceilings, wanted)| {
            let display = Display {
                vsync,
                frame_limit,
                ..Display::default()
            };
            paced_rate(&display, resting, ceilings) != wanted
        })
        .collect();
    sim::probe::check(wrong.is_empty(), format!("wrong rows {wrong:?}"))
}

/// `placed` over the displays a settings file can hold, on a 1920 x 1080
/// screen at 60 Hz (or none), with a GPU of 16384 a side (or 1024).
#[cfg(feature = "probes")]
fn stored_displays_fit() -> Result<String, String> {
    use DisplayMode::{Borderless, Fullscreen, Window};
    use bevy::window::VideoMode;

    let mode = |hz: u32, bit_depth: u16| VideoMode {
        physical_size: UVec2::new(1920, 1080),
        bit_depth,
        refresh_rate_millihertz: hz * 1000,
    };
    let monitor = |refresh: Option<u32>| Monitor {
        name: None,
        physical_width: 1920,
        physical_height: 1080,
        physical_position: IVec2::ZERO,
        refresh_rate_millihertz: refresh,
        scale_factor: 1.0,
        video_modes: vec![mode(144, 32), mode(60, 16), mode(60, 32)],
    };
    let (at_60, unknown_rate) = (monitor(Some(60_000)), monitor(None));
    let entity = Entity::PLACEHOLDER;
    let borderless = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    let exclusive = |mode: VideoMode| {
        WindowMode::Fullscreen(
            MonitorSelection::Entity(entity),
            VideoModeSelection::Specific(mode),
        )
    };
    let size = |x: u32, y: u32| Some(UVec2::new(x, y));
    let window = |size: Option<UVec2>, settled: bool| Placed {
        mode: WindowMode::Windowed,
        size,
        settled,
    };
    let full = |mode: WindowMode, settled: bool| Placed {
        mode,
        size: None,
        settled,
    };
    let big = 16384;
    #[rustfmt::skip]
    let cases = [
        ("window that fits", Window, size(1280, 720), Some(&at_60), big, true, window(size(1280, 720), true)),
        ("window bigger than the screen", Window, size(3840, 2160), Some(&at_60), big, true, window(None, true)),
        ("window with no room for a title bar", Window, size(1920, 1080), Some(&at_60), big, true, window(None, true)),
        ("window bigger than the GPU draws", Window, size(1280, 720), Some(&at_60), 1024, true, window(None, true)),
        ("window before the screen is known", Window, size(1280, 720), None, big, true, window(None, false)),
        ("window as it is", Window, None, None, big, true, window(None, true)),
        ("borderless", Borderless, None, None, big, true, full(borderless, true)),
        ("fullscreen at the screen's rate", Fullscreen, size(1920, 1080), Some(&at_60), big, true, full(exclusive(mode(60, 32)), true)),
        ("fullscreen at the fastest rate", Fullscreen, size(1920, 1080), Some(&unknown_rate), big, true, full(exclusive(mode(144, 32)), true)),
        ("fullscreen in a mode gone", Fullscreen, size(1600, 1200), Some(&at_60), big, true, full(borderless, true)),
        ("fullscreen bigger than the GPU draws", Fullscreen, size(1920, 1080), Some(&at_60), 1024, true, full(borderless, true)),
        ("fullscreen before the screen is known", Fullscreen, size(1920, 1080), None, big, true, full(borderless, false)),
        ("fullscreen with no screen modes", Fullscreen, size(1920, 1080), Some(&at_60), big, false, full(borderless, true)),
    ];
    let wrong: Vec<String> = cases
        .iter()
        .filter_map(|(name, mode, size, screen, limit, exclusive, wanted)| {
            let got = placed(
                *mode,
                *size,
                screen.map(|m| (entity, m)),
                *limit,
                *exclusive,
            );
            (got != *wanted).then(|| format!("{name}: {got:?}, not {wanted:?}"))
        })
        .collect();
    sim::probe::check(
        wrong.is_empty(),
        if wrong.is_empty() {
            format!("{} displays placed as they hold", cases.len())
        } else {
            wrong.join("; ")
        },
    )
}

/// A settings file of a fullscreen mode, a game's folder and a key bound,
/// with the mode on trial: another process's trial is not ended by this
/// one; the next start puts the display back to a window and says what it
/// tried, once, keeping the folder and the key; a trial this process ends
/// leaves no file.
#[cfg(feature = "probes")]
fn failed_start() -> Result<String, String> {
    let folder = std::env::temp_dir().join(format!("kjk-display-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).map_err(|error| error.to_string())?;
    let (path, trial) = (folder.join("settings.json"), folder.join(TRIAL));
    let install = crate::settings::install_key(vfs::Game::Kotor1);
    let text = serde_json::json!({
        install.clone(): "C:/Games/KOTOR",
        "key.medpac": "KeyJ",
        "display.mode": "fullscreen",
        "display.size": "1920x1080",
    });
    std::fs::write(&path, text.to_string()).map_err(|error| error.to_string())?;
    let tried = label(DisplayMode::Fullscreen, Some(UVec2::new(1920, 1080)));

    std::fs::write(&trial, "1\nanother's\n").map_err(|error| error.to_string())?;
    end_trial(&trial);
    let others_kept = trial.exists();
    begin_trial(&trial, &tried);
    let mut settings = Settings::load_from(Some(path.clone()));
    let reset = recover_from(&mut settings, &trial);
    let again = recover_from(&mut settings, &trial);
    let read = Settings::load_from(Some(path));
    let display = read.display();
    let kept =
        read.value(&install) == Some("C:/Games/KOTOR") && read.value("key.medpac") == Some("KeyJ");
    begin_trial(&trial, &tried);
    end_trial(&trial);
    let ended = !trial.exists();
    let _ = std::fs::remove_dir_all(&folder);
    sim::probe::check(
        others_kept
            && reset == Some(DisplayReset(tried))
            && again.is_none()
            && display.mode == DisplayMode::Window
            && display.size.is_none()
            && kept
            && ended,
        format!(
            "another's kept {others_kept}; reset {reset:?}, again {again:?}; now {:?} {:?}; folder and key kept {kept}; ours ended {ended}",
            display.mode, display.size
        ),
    )
}

/// Bevy's own camera sizing over a window resized without its message yet
/// (as `show` leaves it until the system resizes the window), with a
/// camera made meanwhile: with `cameras_in_step` the old camera and the
/// new agree on the new size; without it they do not (the crash's cause,
/// shown so the probe can still see it).
#[cfg(feature = "probes")]
fn cameras_share_a_resize() -> Result<String, String> {
    use bevy::asset::AssetEvent;
    use bevy::render::camera::camera_system;
    use bevy::render::texture::ManualTextureViews;
    use bevy::window::{WindowCreated, WindowResized, WindowScaleFactorChanged};

    let sizes = |in_step: bool| -> Result<(UVec2, UVec2), String> {
        let mut app = App::new();
        app.add_message::<WindowResized>()
            .add_message::<WindowCreated>()
            .add_message::<WindowScaleFactorChanged>()
            .add_message::<AssetEvent<Image>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<ManualTextureViews>()
            .add_systems(
                PostUpdate,
                camera_system.in_set(bevy::camera::CameraUpdateSystems),
            );
        if in_step {
            app.add_systems(
                PostUpdate,
                cameras_in_step.before(bevy::camera::CameraUpdateSystems),
            );
        }
        let mut window = Window::default();
        window.resolution.set_physical_resolution(1280, 720);
        let window = app.world_mut().spawn((window, PrimaryWindow)).id();
        let old = app.world_mut().spawn(Camera3d::default()).id();
        app.update();
        app.world_mut()
            .get_mut::<Window>(window)
            .ok_or("no window")?
            .resolution
            .set_physical_resolution(800, 600);
        let new = app.world_mut().spawn(Camera3d::default()).id();
        app.update();
        let size = |camera: Entity| {
            app.world()
                .get::<Camera>(camera)
                .and_then(Camera::physical_target_size)
                .ok_or(format!("camera {camera} has no size"))
        };
        Ok((size(old)?, size(new)?))
    };
    let (old, new) = sizes(true)?;
    let (stale, fresh) = sizes(false)?;
    let wanted = UVec2::new(800, 600);
    sim::probe::check(
        old == wanted && new == wanted && stale != fresh,
        format!("in step: old {old}, new {new}; without: old {stale}, new {fresh}"),
    )
}

/// The screen the window is on, else the primary one.
fn screen<'a>(
    window: Option<&OnMonitor>,
    monitors: &'a Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
) -> Option<&'a Monitor> {
    screen_entity(window, monitors).map(|(_, monitor)| monitor)
}

/// `screen`, with its entity.
fn screen_entity<'a>(
    window: Option<&OnMonitor>,
    monitors: &'a Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
) -> Option<(Entity, &'a Monitor)> {
    let on = window.map(|on| on.0);
    monitors
        .iter()
        .find(|(entity, ..)| Some(*entity) == on)
        .or_else(|| monitors.iter().find(|(_, _, primary)| *primary))
        .or_else(|| monitors.iter().next())
        .map(|(entity, monitor, _)| (entity, monitor))
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
/// own modes where `exclusive`. The one `window` is shown as now is marked.
fn choices(monitor: &Monitor, window: &Window, exclusive: bool) -> Vec<DisplayChoice> {
    let full = UVec2::new(monitor.physical_width, monitor.physical_height);
    let (shown, now) = match window.mode {
        WindowMode::Windowed => (DisplayMode::Window, Some(window.resolution.physical_size())),
        WindowMode::BorderlessFullscreen(_) => (DisplayMode::Borderless, None),
        WindowMode::Fullscreen(_, VideoModeSelection::Specific(mode)) => {
            (DisplayMode::Fullscreen, Some(mode.physical_size))
        }
        WindowMode::Fullscreen(..) => (DisplayMode::Fullscreen, Some(full)),
    };
    // Borderless is named by the screen's size, and keeps none.
    let choice = |mode: DisplayMode, size: Option<UVec2>| DisplayChoice {
        mode: mode.name().into(),
        size: size.map_or_else(String::new, size_name),
        label: label(mode, size.or(Some(full))),
        current: mode == shown && size == now,
    };
    let windows = WINDOWS
        .iter()
        .rev()
        .map(|&(x, y)| UVec2::new(x, y))
        .filter(|size| fits_window(*size, full))
        .map(|size| choice(DisplayMode::Window, Some(size)));
    let borderless = choice(DisplayMode::Borderless, None);
    let own = if exclusive {
        modes(monitor)
    } else {
        Vec::new()
    };
    let fullscreen = own
        .into_iter()
        .map(|size| choice(DisplayMode::Fullscreen, Some(size)));
    windows.chain([borderless]).chain(fullscreen).collect()
}

/// The resolution page's choices, for the screen the window is on now. Also
/// the anti-aliasing choices and Shadows at Ultra this machine offers.
fn offer(
    windows: Query<(&Window, Option<&OnMonitor>), With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
    ui: Option<ResMut<Ui>>,
    temporal: Option<Res<crate::temporal::Temporal>>,
    screen_modes: Res<ScreenModes>,
    rays: Option<Res<crate::ray_structures::RayQueries>>,
) {
    let Some(mut ui) = ui else {
        return;
    };
    // Shadows at Ultra, where the card traces rays.
    let ultra = rays.is_some_and(|rays| rays.available);
    if ui.vm.ultra_shadows != ultra {
        ui.vm.ultra_shadows = ultra;
    }
    // The anti-aliasing choices, when what the machine offers is first known
    // or changes.
    if ui.vm.antialias.is_empty() || temporal.as_ref().is_some_and(|t| t.is_changed()) {
        let dlss = temporal.is_some_and(|temporal| temporal.dlss);
        ui.vm.antialias = crate::temporal::offered(dlss);
    }
    let Ok((window, on)) = windows.single() else {
        return;
    };
    let offered = screen(on, &monitors)
        .map(|monitor| choices(monitor, window, screen_modes.exclusive))
        .unwrap_or_else(Vec::new);
    if ui.vm.displays != offered {
        ui.vm.displays = offered;
    }
}

/// Alt+Enter: the window borderless, or back to a window. The choice is
/// saved at once and shown on the options pages. It went through the
/// screens' intents before, which the frame could drop unheard when this
/// system ran after the settings had taken theirs.
fn alt_enter(
    keys: Res<ButtonInput<KeyCode>>,
    settings: Option<ResMut<Settings>>,
    mut display: ResMut<Display>,
    ui: Option<ResMut<Ui>>,
) {
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    let enter = keys.any_just_pressed([KeyCode::Enter, KeyCode::NumpadEnter]);
    let Some(mut settings) = settings.filter(|_| alt && enter) else {
        return;
    };
    switch_window(&mut settings);
    display.set_if_neq(settings.display());
    if let Some(mut ui) = ui {
        settings.shown(&mut ui);
    }
}

/// A window made borderless, or a borderless or fullscreen one put back to
/// a window, and saved.
fn switch_window(settings: &mut Settings) {
    let mode = match DisplayMode::named(settings.value("display.mode").unwrap_or("")) {
        DisplayMode::Window => DisplayMode::Borderless,
        DisplayMode::Borderless | DisplayMode::Fullscreen => DisplayMode::Window,
    };
    settings.apply("display.mode", mode.name());
}

/// Where `show` puts the window: its mode, the size a window takes (none:
/// as it is), and whether the screen was known to choose them by.
#[derive(Debug, Clone, PartialEq)]
struct Placed {
    mode: WindowMode,
    size: Option<UVec2>,
    settled: bool,
}

/// The window `mode` and `size` make on `screen` (the window's, if known),
/// with a GPU drawing at most `limit` pixels a side, `exclusive` where the
/// screen's own modes are used. What would not hold is not tried:
/// - a window's size that does not fit the screen, or the GPU, leaves the
///   window as it is (the size kept from another screen, or edited);
/// - a fullscreen size the screen has no mode of (no longer), or the GPU
///   cannot draw, is borderless;
/// - of a size's modes, the one at the screen's refresh rate now, else the
///   fastest; the deepest colour of those;
/// - the screen named by its entity: winit's own idea of the window's
///   screen can be none, and Bevy panics then.
///
/// A window's size and a fullscreen mode wait for the screen. `show` puts
/// nothing to the window until they are settled.
fn placed(
    mode: DisplayMode,
    size: Option<UVec2>,
    screen: Option<(Entity, &Monitor)>,
    limit: u32,
    exclusive: bool,
) -> Placed {
    let drawn = |size: &UVec2| size.x <= limit && size.y <= limit;
    let borderless = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    let known = screen.is_some();
    match mode {
        DisplayMode::Window => Placed {
            mode: WindowMode::Windowed,
            size: size.filter(drawn).filter(|size| {
                screen.is_some_and(|(_, monitor)| {
                    let full = UVec2::new(monitor.physical_width, monitor.physical_height);
                    fits_window(*size, full)
                })
            }),
            settled: size.is_none() || known,
        },
        DisplayMode::Borderless => Placed {
            mode: borderless,
            size: None,
            settled: true,
        },
        DisplayMode::Fullscreen if !exclusive => Placed {
            mode: borderless,
            size: None,
            settled: true,
        },
        DisplayMode::Fullscreen => {
            let best = size
                .filter(drawn)
                .zip(screen)
                .and_then(|(size, (entity, monitor))| {
                    let now = monitor.refresh_rate_millihertz;
                    monitor
                        .video_modes
                        .iter()
                        .filter(|mode| mode.physical_size == size)
                        .max_by_key(|mode| {
                            let refresh = mode.refresh_rate_millihertz;
                            (Some(refresh) == now, refresh, mode.bit_depth)
                        })
                        .map(|mode| (entity, *mode))
                });
            Placed {
                mode: best.map_or(borderless, |(entity, mode)| {
                    WindowMode::Fullscreen(
                        MonitorSelection::Entity(entity),
                        VideoModeSelection::Specific(mode),
                    )
                }),
                size: None,
                settled: known,
            }
        }
    }
}

/// The window as `Display` has it: mode and size when either changed,
/// v-sync always, put to the screen and the GPU first (`placed`). A change
/// is put to the window once, when its screen is known; until then the
/// window stays as it is. A display other than the default goes on trial
/// (`Trial`).
#[allow(clippy::too_many_arguments)]
fn show(
    display: Res<Display>,
    mut windows: Query<(&mut Window, Option<&OnMonitor>), With<PrimaryWindow>>,
    monitors: Query<(Entity, &Monitor, Has<PrimaryMonitor>)>,
    device: Option<Res<RenderDevice>>,
    screen_modes: Res<ScreenModes>,
    mut trial: ResMut<Trial>,
    time: Res<Time<Real>>,
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
    let screen = screen_entity(on, &monitors);
    let limit = device.map_or(u32::MAX, |device| device.limits().max_texture_dimension_2d);
    let place = placed(
        display.mode,
        display.size,
        screen,
        limit,
        screen_modes.exclusive,
    );
    if !place.settled {
        return;
    }
    *shown = Some(wanted);
    if window.mode != place.mode {
        window.mode = place.mode;
    }
    if let Some(size) = place.size {
        window.resolution.set_physical_resolution(size.x, size.y);
        window.position = WindowPosition::Centered(MonitorSelection::Current);
    }
    let size = display.size;
    if size.is_some() && place.size.is_none() && display.mode == DisplayMode::Window {
        warn!(target: "display", ?size, limit, "the window's size does not fit the screen; the window stays as it is");
    }
    if display.mode == DisplayMode::Fullscreen && !screen_modes.exclusive {
        info!(target: "display", "fullscreen is borderless over the whole screen on this system");
    }
    let tried = (wanted != (DisplayMode::Window, None)).then(|| label(display.mode, size));
    trial.start(tried, time.elapsed_secs_f64());
    info!(target: "display", mode = ?window.mode, ?size, "window");
}

/// The file a display on trial is noted in, beside the settings.
const TRIAL: &str = "display-trial";

/// How long frames run before a display on trial is taken as good.
const TRIAL_SECONDS: f64 = 5.0;

/// The display on trial: since when (real seconds) and in which file.
#[derive(Resource)]
struct Trial {
    since: Option<f64>,
    file: Option<PathBuf>,
}

impl Default for Trial {
    fn default() -> Trial {
        Trial {
            since: None,
            file: trial_file(),
        }
    }
}

impl Trial {
    /// `tried` (its label) on trial from `now`; none ends the trial.
    fn start(&mut self, tried: Option<String>, now: f64) {
        let Some(file) = &self.file else {
            return;
        };
        match tried {
            Some(tried) => {
                begin_trial(file, &tried);
                self.since = Some(now);
            }
            None => {
                end_trial(file);
                self.since = None;
            }
        }
    }
}

/// The trial's file: `TRIAL` beside the settings' file.
fn trial_file() -> Option<PathBuf> {
    crate::settings::file().map(|file| file.with_file_name(TRIAL))
}

/// Notes `tried` on trial by this process in `file`.
fn begin_trial(file: &Path, tried: &str) {
    let text = format!("{}\n{tried}\n", std::process::id());
    if let Err(error) = std::fs::write(file, text) {
        warn!(target: "display", "{}: {error}", file.display());
    }
}

/// Removes `file` when this process wrote it (another may be trying its
/// own: the menu's next process, started as this one ends).
fn end_trial(file: &Path) {
    let ours = std::process::id().to_string();
    let written = std::fs::read_to_string(file).ok();
    if written.is_some_and(|text| text.lines().next() == Some(ours.as_str())) {
        let _ = std::fs::remove_file(file);
    }
}

/// A display on trial that has run `TRIAL_SECONDS` of frames is good.
fn trial_settles(mut trial: ResMut<Trial>, time: Res<Time<Real>>) {
    let now = time.elapsed_secs_f64();
    if trial
        .since
        .is_some_and(|since| now - since >= TRIAL_SECONDS)
    {
        trial.start(None, now);
    }
}

/// The run ended well: a display still on trial did no harm.
pub fn ended_well() {
    if let Some(file) = trial_file() {
        end_trial(&file);
    }
}

/// The display the last start tried and did not get going with, put back
/// to a window: what the main menu tells (`crash::notice`).
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct DisplayReset(pub String);

impl DisplayReset {
    pub fn notice(&self) -> String {
        format!(
            "kjk failed to start at {} last time, so it started in windowed mode. Change the resolution in Options.",
            self.0
        )
    }
}

/// A start after one that did not get going with the display it tried
/// (the trial's file is there): the window's mode and size back to their
/// defaults, saved; nothing else changes.
pub(crate) fn recover(settings: &mut Settings) -> Option<DisplayReset> {
    recover_from(settings, &trial_file()?)
}

fn recover_from(settings: &mut Settings, file: &Path) -> Option<DisplayReset> {
    let text = std::fs::read_to_string(file).ok()?;
    let _ = std::fs::remove_file(file);
    let tried = text.lines().nth(1).unwrap_or("its display").to_string();
    settings.apply("reset", "display");
    warn!(target: "display", %tried, "the last start did not get going with this display; a window instead");
    Some(DisplayReset(tried))
}

/// `kjk --reset-display`: the window's mode and size back to their
/// defaults, and no display on trial.
pub fn reset() {
    let mut settings = Settings::load();
    settings.apply("reset", "display");
    if let Some(file) = trial_file() {
        let _ = std::fs::remove_file(file);
    }
    eprintln!("the display is back to a window");
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
    if facts.wanted() {
        facts.0.insert(
            "display".into(),
            serde_json::json!({
                "mode": format!("{:?}", window.mode),
                "physical": [window.resolution.physical_width(), window.resolution.physical_height()],
                "scale": window.resolution.scale_factor(),
                "present": format!("{:?}", window.present_mode),
                "frame_limit": display.frame_limit,
                "antialias": display.antialias.name(),
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
}
