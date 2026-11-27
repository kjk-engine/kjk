//! The UI catalogue in the game (`kjk gui catalogue`, docs/UI.md, "The
//! catalogue in the game"). A story is staged (`story_stage::Staged`) in a
//! virtual window of one of the six sizes (`ui::catalogue::SIZES`), whatever
//! the real window is. A camera draws its screens into an image of the
//! device's exact pixel size, and a second camera shows that image in the
//! window in one of three modes (`ui::virtual_screen`). The mouse, the keys
//! and the pad reach the screens through the same mapping, so a click lands
//! on the control it is over.
//!
//! The catalogue's own keys are in `KEYS`. Each has a keyboard key and a pad
//! chord, named as `UiInput::raw` names them.

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::camera::{
    ClearColorConfig, OrthographicProjection, Projection, RenderTarget, ScalingMode,
};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureFormat};
use bevy::window::PrimaryWindow;
use console::{DumpFacts, UiRequests};
use ui::catalogue::{Input, SIZES, Size, Story};
use ui::draw::DrawList;
use ui::screens::key_mapping::PAD_RAW;
use ui::virtual_screen::{
    DISPLAY_DENSITY, Mode, OWNER_DISTANCE_CM, Placement, View, place, seen_scale, viewing,
};
use vfs::Game;

use collab::{Collab, Tool};

mod collab;

use crate::lifecycle_view::OverlayCamera;
use crate::scene::FrameSize;
use crate::sound::{Added, Now};
use crate::story_stage::{Staged, stage};
use crate::ui_input::{Held, gather};
use crate::ui_material::{UiMaterial, UiMaterialPlugin, UiParams};
use crate::ui_sounds::Rows;
use crate::ui_view::{Drawn, LAYER as SCREENS_LAYER, console_input};

/// The render layer of the quad that shows the image in the window.
const WINDOW_LAYER: usize = 14;

/// How fast the right stick pans the image, in logical pixels a second at
/// full tilt.
const PAN_SPEED: f32 = 900.0;

/// The stick's tilt from which it pans.
const STICK: f32 = 0.3;

/// How many frames the screens' image is drawn after a change, while their
/// meshes and textures reach the GPU.
const REDRAWN: u8 = 3;

/// The settings that say how big a pixel of the owner's display is and how
/// far the owner sits from it.
const DISPLAY_SETTING: &str = "catalogue.display_density";
const DISTANCE_SETTING: &str = "catalogue.distance_cm";

/// What the catalogue's keys do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// The next of the six sizes.
    NextSize,
    /// The next of the three ways to show the image.
    NextMode,
    /// A frame around every control, with its tag.
    Outlines,
    /// The next backdrop behind the screen.
    Backdrop,
    /// The tryout so far, saved as a story.
    SaveTryout,
}

/// The catalogue's keys: what they do, the keyboard key and the pad chord.
/// Later keys of the catalogue join this table.
const KEYS: [(Action, &str, &str); 5] = [
    (Action::NextSize, "F6", "LB+North"),
    (Action::NextMode, "F7", "LB+West"),
    (Action::Outlines, "F9", "LB+South"),
    (Action::Backdrop, "F10", "LB+DPadUp"),
    (Action::SaveTryout, "F11", "LB+DPadDown"),
];

/// What the catalogue opens on.
#[derive(Debug, Clone)]
pub struct Start {
    pub module: String,
    pub story: Story,
    pub game: Game,
    /// The size it opens at, by name (`ui::catalogue::SIZES`).
    pub size: &'static str,
}

pub struct CataloguePlugin {
    pub start: Start,
}

impl Plugin for CataloguePlugin {
    fn build(&self, app: &mut App) {
        let settings = crate::settings::Settings::load();
        let number = |key: &str, default: f32| {
            settings
                .value(key)
                .and_then(|value| value.parse::<f32>().ok())
                .filter(|value| *value > 0.0)
                .unwrap_or(default)
        };
        let display = (
            number(DISPLAY_SETTING, DISPLAY_DENSITY),
            number(DISTANCE_SETTING, OWNER_DISTANCE_CM),
        );
        match Catalogue::open(&self.start, display) {
            Ok(catalogue) => {
                app.insert_resource(catalogue);
            }
            Err(why) => {
                eprintln!("catalogue: {why}");
                app.add_systems(Update, |mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::error());
                });
                return;
            }
        }
        app.add_plugins((UiMaterialPlugin, crate::pad::SticksPlugin))
            .insert_resource(ClearColor(Color::BLACK))
            .init_resource::<Painting>()
            .add_systems(
                Update,
                (step, paint)
                    .chain()
                    .after(console::ConsoleQueue)
                    .after(crate::pad::Settled),
            );
    }
}

/// The story on its virtual screen, and how it is shown.
#[derive(Resource)]
struct Catalogue {
    start: Start,
    discovery: vfs::Discovery,
    staged: Staged,
    /// The place of the size in `SIZES`.
    size: usize,
    mode: Mode,
    /// How far the window is into an image larger than itself, in the
    /// window's logical pixels.
    pan: [f32; 2],
    /// The owner's display: its pixels to the inch, and the owner's
    /// distance from it in centimetres.
    display: (f32, f32),
    /// The window as the camera that shows the image last saw it: its
    /// logical size, and its physical pixels to each logical one.
    window: ((f32, f32), f32),
    placement: Placement,
    /// The screens as the last frame drew them.
    layers: Vec<DrawList>,
    /// The screen module and the layout of each of `layers`.
    names: Vec<(String, String)>,
    collab: Collab,
    /// What `ui` commands of the console left for the next frame's input.
    scripted: Vec<Vec<String>>,
    scripted_hold: Option<[f32; 2]>,
    /// Where the pointer was when the right button last moved the image.
    dragging: Option<[f32; 2]>,
    sticks: Held,
    last_pointer: Option<Vec2>,
    rows: Rows,
    /// The story could not be opened again, so it is not tried each frame.
    reopen_failed: bool,
}

impl Catalogue {
    fn open(start: &Start, display: (f32, f32)) -> Result<Catalogue, String> {
        let discovery = crate::discover();
        let size = SIZES
            .iter()
            .position(|size| size.name == start.size)
            .ok_or(format!("{}: no such size", start.size))?;
        let staged = Catalogue::stage(&discovery, start, SIZES[size])?;
        let window = SIZES[size].window();
        let mut catalogue = Catalogue {
            start: start.clone(),
            discovery,
            staged,
            size,
            mode: Mode::Fit,
            pan: [0.0; 2],
            display,
            window: (window, 1.0),
            placement: Placement {
                origin: [0.0; 2],
                size: [0.0; 2],
                scale: 1.0,
                pan: [0.0; 2],
            },
            layers: Vec::new(),
            names: Vec::new(),
            collab: Collab::new(start),
            scripted: Vec::new(),
            scripted_hold: None,
            dragging: None,
            sticks: Held::default(),
            last_pointer: None,
            rows: Rows::default(),
            reopen_failed: false,
        };
        catalogue.placement = catalogue.placed();
        Ok(catalogue)
    }

    /// The story opened with its steps taken, in the window of `size`.
    fn stage(discovery: &vfs::Discovery, start: &Start, size: Size) -> Result<Staged, String> {
        let story = &start.story;
        let key = start.game.key();
        let capture = story
            .capture_for(key, Input::Mouse)
            .ok_or(format!("{}: no capture on {key} for the mouse", story.name))?;
        stage(
            discovery,
            (start.game, &story.screen),
            (&story.steps, story.error.clone(), capture),
            (size.pixels, size.factor),
        )
    }

    fn size(&self) -> Size {
        SIZES[self.size]
    }

    /// How many display pixels a pixel of the device covers.
    fn seen(&self) -> f32 {
        let size = self.size();
        seen_scale((size, viewing(&size)), self.display)
    }

    /// Where the image stands in the window now.
    fn placed(&self) -> Placement {
        place(&View {
            mode: self.mode,
            image: self.size().pixels,
            window: self.window.0,
            window_factor: self.window.1,
            seen: self.seen(),
            pan: self.pan,
        })
    }

    fn set_size(&mut self, size: usize) {
        self.size = size % SIZES.len();
        self.pan = [0.0; 2];
        info!(target: "catalogue", size = self.size().name, "size");
    }

    fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        self.pan = [0.0; 2];
        info!(target: "catalogue", mode = mode.name(), "mode");
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::NextSize => self.set_size(self.size + 1),
            Action::NextMode => self.set_mode(self.mode.next()),
            Action::Outlines => collab::act(self, Tool::Outlines),
            Action::Backdrop => collab::act(self, Tool::Backdrop),
            Action::SaveTryout => collab::act(self, Tool::SaveTryout),
        }
    }

    /// A `ui` command of the console for the catalogue itself. Whether it
    /// was one.
    fn command(&mut self, words: &[&str]) -> bool {
        match words {
            ["catalogue", "size", "next"] => self.set_size(self.size + 1),
            ["catalogue", "size", name] => match SIZES.iter().position(|size| size.name == *name) {
                Some(at) => self.set_size(at),
                None => {
                    warn!(target: "console", "ui catalogue: {name} is not one of the six sizes")
                }
            },
            ["catalogue", "mode", "next"] => self.set_mode(self.mode.next()),
            ["catalogue", "mode", name] => match Mode::named(name) {
                Some(mode) => self.set_mode(mode),
                None => warn!(target: "console", "ui catalogue: {name} is not fit, pixel or seen"),
            },
            ["catalogue", "pan", x, y] => match (x.parse(), y.parse()) {
                (Ok(x), Ok(y)) => self.pan = [x, y],
                _ => warn!(target: "console", "ui catalogue pan X Y: window pixels"),
            },
            ["catalogue", "density", value] => match value.parse::<f32>() {
                Ok(value) if value > 0.0 => self.display.0 = value,
                _ => warn!(target: "console", "ui catalogue density N: pixels to the inch"),
            },
            ["catalogue", "distance", value] => match value.parse::<f32>() {
                Ok(value) if value > 0.0 => self.display.1 = value,
                _ => warn!(target: "console", "ui catalogue distance N: centimetres"),
            },
            _ => return false,
        }
        true
    }

    /// A `ui` command of the console, for the screens or the catalogue.
    fn request(&mut self, request: &[String]) {
        let words: Vec<&str> = request.iter().map(String::as_str).collect();
        if self.command(&words) {
            return;
        }
        let Staged { stack, vm, .. } = &mut self.staged;
        match words.as_slice() {
            ["press", tag] => stack.press(tag),
            ["double", tag, row] => match row.parse() {
                Ok(row) => stack.double(tag, row),
                Err(_) => warn!(target: "console", "ui double: {row} is not a row's number"),
            },
            ["open", name] => {
                if !stack.open(name, vm) {
                    warn!(target: "console", "ui: no screen {name}");
                }
            }
            ["close"] => stack.pop(),
            [
                "nav" | "type" | "back" | "pointer" | "click" | "raw" | "down" | "drag" | "up",
                ..,
            ] => {
                self.scripted.push(request.to_vec());
            }
            other => warn!(target: "console", "ui: {other:?} is not a request of the catalogue"),
        }
    }

    /// The story opened again, as the stack's last screen closed.
    fn restage(&mut self) {
        match Catalogue::stage(&self.discovery, &self.start, self.size()) {
            Ok(staged) => {
                info!(target: "catalogue", story = self.start.story.name, "opened again, its last screen closed");
                self.staged = staged;
                self.collab.restaged();
            }
            Err(why) => {
                warn!(target: "catalogue", "{why}");
                self.reopen_failed = true;
            }
        }
    }
}

/// The window's logical size and its physical pixels to each logical one:
/// from the camera that shows the image, else from the window, else from the
/// headless frame.
fn window_of(
    camera: Option<&Camera>,
    window: Option<&Window>,
    frame: Option<&FrameSize>,
) -> Option<((f32, f32), f32)> {
    if let Some(camera) = camera
        && let (Some(logical), Some(physical)) = (
            camera.logical_viewport_size(),
            camera.physical_viewport_size(),
        )
        && logical.x >= 1.0
        && logical.y >= 1.0
    {
        return Some(((logical.x, logical.y), physical.x as f32 / logical.x));
    }
    if let Some(window) = window
        && window.width() >= 1.0
        && window.height() >= 1.0
    {
        return Some(((window.width(), window.height()), window.scale_factor()));
    }
    frame.map(|frame| ((frame.0 as f32, frame.1 as f32), 1.0))
}

/// This frame's input for the screens, from the real devices and the
/// console, and the catalogue's own keys taken out of it.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn step(
    mut cat: ResMut<Catalogue>,
    (keys, mouse): (
        Option<Res<ButtonInput<KeyCode>>>,
        Option<Res<ButtonInput<MouseButton>>>,
    ),
    (mut typed, mut wheel): (MessageReader<KeyboardInput>, MessageReader<MouseWheel>),
    pads: crate::pad::Pads,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<&Camera, With<WindowCamera>>,
    (frame, time, run): (
        Option<Res<FrameSize>>,
        Res<Time>,
        Option<Res<console::RunFolder>>,
    ),
    (mut requests, mut now, mut added, mut facts): (
        ResMut<UiRequests>,
        ResMut<Now>,
        ResMut<Added>,
        ResMut<DumpFacts>,
    ),
) {
    use ui::input::Key;
    let cat = &mut *cat;
    if let Some(window) = window_of(
        cameras.iter().next(),
        windows.iter().next(),
        frame.as_deref(),
    ) {
        cat.window = window;
    }
    collab::begin(
        cat,
        run.as_ref().map(|run| run.0.as_path()),
        time.delta_secs(),
    );
    for request in std::mem::take(&mut requests.0) {
        cat.collab.source = "console";
        cat.request(&request);
    }
    let scripted = !cat.scripted.is_empty();
    let size = cat.size();
    let mut input = gather(
        keys.as_deref(),
        mouse.as_deref(),
        &mut typed,
        &mut wheel,
        Some((&pads, true)),
        windows.iter().next(),
        Vec2::new(cat.window.0.0, cat.window.0.1),
        time.delta_secs(),
        &mut cat.sticks,
        &mut cat.last_pointer,
    );
    console_input(
        &std::mem::take(&mut cat.scripted),
        &mut input,
        &mut cat.scripted_hold,
    );

    // The catalogue's keys, which the screens do not see.
    let mut chord = false;
    for (action, key, pad) in KEYS {
        let pad = format!("{PAD_RAW}{pad}");
        if let Some(at) = input.raw.iter().position(|raw| raw == key || *raw == pad) {
            chord |= input.raw[at] == pad;
            cat.collab.source = match (input.raw[at] == pad, scripted) {
                (true, _) => "pad",
                (_, true) => "console",
                _ => "keyboard",
            };
            input.raw.remove(at);
            cat.act(action);
        }
    }
    if chord {
        input.keys.clear();
    }

    // The image moved by the right mouse button dragged and by the right
    // stick, where it is larger than the window.
    let before = cat.placed();
    let spare = before.spare(cat.window.0);
    let pannable = spare[0] > 0.0 || spare[1] > 0.0;
    let dragged = mouse
        .as_deref()
        .is_some_and(|mouse| mouse.pressed(MouseButton::Right));
    match (input.pointer, dragged) {
        (Some(at), true) => {
            if let Some(from) = cat.dragging {
                cat.pan = [
                    before.pan[0] + from[0] - at[0],
                    before.pan[1] + from[1] - at[1],
                ];
            }
            cat.dragging = Some(at);
        }
        _ => cat.dragging = None,
    }
    let stick = pads
        .iter()
        .map(|(_, sticks)| sticks.right)
        .max_by(|a, b| a.length().total_cmp(&b.length()))
        .unwrap_or(Vec2::ZERO);
    if pannable && stick.length() > STICK {
        cat.pan = [
            before.pan[0] + stick.x * PAN_SPEED * time.delta_secs(),
            before.pan[1] - stick.y * PAN_SPEED * time.delta_secs(),
        ];
        input
            .keys
            .retain(|key| !matches!(key, Key::ScrollUp | Key::ScrollDown));
    }
    if dragged {
        input.click = false;
        input.held = false;
    }
    cat.placement = cat.placed();
    cat.pan = cat.placement.pan;

    // The pointer, from the window into the image, then into the virtual
    // window's logical pixels.
    if let Some(point) = input.pointer {
        input.pointer = cat
            .placement
            .to_image(point)
            .map(|pixel| [pixel[0] / size.factor, pixel[1] / size.factor]);
        if input.pointer.is_none() {
            input.click = false;
            input.held = false;
        }
    }
    input.window = size.window();
    input.scale_factor = size.factor;

    let from = collab::source_of(
        keys.as_deref()
            .is_some_and(|keys| keys.get_just_pressed().next().is_some()),
        input.click || input.wheel != 0.0,
        pads.iter().any(|(pad, sticks)| {
            pad.digital().get_just_pressed().next().is_some() || sticks.left.length() > STICK
        }),
        scripted,
    );
    collab::before(cat, &input, from);
    let live = cat.staged.live(&input);
    for why in &live.refusals {
        debug!(target: "ui", "refused: {why}");
    }
    crate::ui_sounds::play(
        &mut cat.staged.assets,
        live.sounds.iter().copied(),
        (&mut now, &mut added),
        &mut cat.rows,
    );
    for intent in &live.intents {
        debug!(target: "catalogue", ?intent, "the screen asked for");
    }
    cat.names = (live.shown.iter())
        .map(|shown| (shown.screen.clone(), shown.layout.clone()))
        .collect();
    cat.layers = live.shown.into_iter().map(|shown| shown.list).collect();
    if !cat.reopen_failed && !cat.staged.stack.is_open() && cat.staged.over.is_none() {
        cat.restage();
    }
    collab::end(cat, &input, time.delta_secs());
    if facts.wanted() {
        facts.0.insert(
            "catalogue".into(),
            serde_json::json!({
                "story": format!("{}/{}", cat.start.module, cat.start.story.name),
                "game": cat.start.game.key(),
                "size": size.name,
                "mode": cat.mode.name(),
                "device": [size.pixels.0, size.pixels.1],
                "factor": size.factor,
                "window": [cat.window.0.0, cat.window.0.1],
                "window_factor": cat.window.1,
                "image_at": cat.placement.origin,
                "image_size": cat.placement.size,
                "image_scale": cat.placement.scale,
                "pan": cat.placement.pan,
                "seen": cat.seen(),
                "display_density": cat.display.0,
                "owner_distance_cm": cat.display.1,
                "screens": cat.staged.stack.screens().map(|screen| screen.0).collect::<Vec<_>>(),
                "focused": cat.staged.stack.focused(),
            }),
        );
    }
}

/// The camera that draws the screens into their image.
#[derive(Component)]
struct ScreensCamera;

/// The camera that shows the image in the window.
#[derive(Component)]
struct WindowCamera;

/// The quad that shows the image.
#[derive(Component)]
struct ImageQuad;

/// The GPU side: the image the screens are drawn into, their cameras, and
/// the runs of quads drawn.
#[derive(Resource, Default)]
struct Painting {
    drawn: Drawn,
    /// Frames left to draw the screens' image after a change. It keeps what
    /// it shows the rest of the time.
    fresh: u8,
    made: Option<Made>,
}

struct Made {
    image: Handle<Image>,
    screens: Entity,
    window: Entity,
    quad: Entity,
}

/// The image the screens are drawn into: a device's pixels, kept as drawn.
fn target(pixels: (u32, u32)) -> Image {
    Image::new_target_texture(pixels.0, pixels.1, TextureFormat::Rgba8Unorm, None)
}

fn fixed(width: f32, height: f32) -> Projection {
    let mut projection = OrthographicProjection::default_3d();
    projection.scaling_mode = ScalingMode::Fixed { width, height };
    Projection::Orthographic(projection)
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn paint(
    mut commands: Commands,
    cat: Res<Catalogue>,
    mut painting: ResMut<Painting>,
    (mut meshes, mut materials, mut images): (
        ResMut<Assets<Mesh>>,
        ResMut<Assets<UiMaterial>>,
        ResMut<Assets<Image>>,
    ),
    main: Query<Option<&RenderTarget>, (With<Camera2d>, Without<OverlayCamera>)>,
    mut projections: Query<&mut Projection>,
    mut cameras: Query<&mut Camera, With<ScreensCamera>>,
    mut places: Query<&mut Transform>,
    mut visibility: Query<&mut Visibility>,
) {
    let painting = &mut *painting;
    let size = cat.size();
    let virtual_window = Vec2::new(size.window().0, size.window().1);
    let Some(made) = &painting.made else {
        let Some(on) = main.iter().next() else {
            return;
        };
        let image = images.add(target(size.pixels));
        let screens = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: 12,
                    clear_color: ClearColorConfig::Custom(Color::BLACK),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Msaa::Off,
                fixed(virtual_window.x, virtual_window.y),
                Tonemapping::None,
                DebandDither::Disabled,
                Transform::from_xyz(0.0, 0.0, 100.0),
                RenderLayers::layer(SCREENS_LAYER),
                OverlayCamera,
                ScreensCamera,
            ))
            .id();
        let mut window = commands.spawn((
            Camera3d::default(),
            crate::scene::over_world(13),
            crate::scene::encoded_view(),
            fixed(cat.window.0.0, cat.window.0.1),
            Transform::from_xyz(0.0, 0.0, 100.0),
            RenderLayers::layer(WINDOW_LAYER),
            OverlayCamera,
            WindowCamera,
        ));
        if let Some(on) = on {
            window.insert(on.clone());
        }
        let window = window.id();
        let quad = commands
            .spawn((
                Mesh3d(meshes.add(Rectangle::new(1.0, 1.0))),
                MeshMaterial3d(materials.add(UiMaterial {
                    params: UiParams {
                        size: Vec2::new(size.pixels.0 as f32, size.pixels.1 as f32),
                        shape: 0,
                        view: 1,
                        grey: 0,
                    },
                    texture: Some(image.clone()),
                    additive: false,
                })),
                Transform::IDENTITY,
                RenderLayers::layer(WINDOW_LAYER),
                NoFrustumCulling,
                ImageQuad,
            ))
            .id();
        painting.made = Some(Made {
            image,
            screens,
            window,
            quad,
        });
        return;
    };
    // The image the size of the device.
    if let Some(mut image) = images.get_mut(&made.image)
        && image.size() != UVec2::new(size.pixels.0, size.pixels.1)
    {
        painting.fresh = REDRAWN;
        image.resize(Extent3d {
            width: size.pixels.0,
            height: size.pixels.1,
            depth_or_array_layers: 1,
        });
    }
    if let Ok(mut projection) = projections.get_mut(made.screens) {
        *projection = fixed(virtual_window.x, virtual_window.y);
    }
    // The quad that shows it, in the window's logical pixels from its
    // middle.
    let (window, _) = cat.window;
    if let Ok(mut projection) = projections.get_mut(made.window) {
        *projection = fixed(window.0, window.1);
    }
    let placement = cat.placement;
    if let Ok(mut at) = places.get_mut(made.quad) {
        *at = Transform {
            translation: Vec3::new(
                placement.origin[0] + placement.size[0] / 2.0 - window.0 / 2.0,
                window.1 / 2.0 - (placement.origin[1] + placement.size[1] / 2.0),
                0.0,
            ),
            scale: Vec3::new(placement.size[0], placement.size[1], 1.0),
            ..Transform::IDENTITY
        };
    }
    // The screens' quads, made again when the screens or the size changed.
    let assets = &cat.staged.assets;
    let composed = collab::composed(&cat);
    let released = painting.drawn.forget_released(assets);
    if painting.drawn.remake(&composed, virtual_window) || released {
        painting.fresh = REDRAWN;
        painting.drawn.paint_runs(
            &mut commands,
            &composed,
            (virtual_window, assets),
            (&mut *meshes, &mut *materials, &mut *images),
            None,
            &mut visibility,
            SCREENS_LAYER,
        );
    }
    if let Ok(mut camera) = cameras.get_mut(made.screens) {
        let draws = painting.fresh > 0;
        if camera.is_active != draws {
            camera.is_active = draws;
        }
    }
    painting.fresh = painting.fresh.saturating_sub(1);
}
