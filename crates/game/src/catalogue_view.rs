//! The UI catalogue in the game (`kjk gui catalogue`, docs/UI.md, "The
//! catalogue in the game"). A story is staged (`story_stage::Staged`) in a
//! virtual window of one of the five sizes (`ui::catalogue::SIZES`), whatever
//! the real window is. A camera draws its screens into an image of the
//! device's exact pixel size, and a second camera shows that image in the
//! window, scaled to fit inside the device's outline (`ui::virtual_screen`). The mouse, the keys
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
use ui::catalogue::{Input, SIZES, Size, Step, Story};
use ui::draw::DrawList;
use ui::screens::key_mapping::PAD_RAW;
use ui::virtual_screen::{Placement, View, place};
use vfs::Game;

use chrome::{Chrome, Facts, Hints, Look};
use collab::{Collab, Tool};

mod chrome;
mod collab;

use crate::lifecycle_view::OverlayCamera;
use crate::scene::FrameSize;
use crate::sound::{Added, Now};
use crate::story_stage::{Kept, Scene, Staged, stage_with};
use crate::ui_input::{Held, gather};
use crate::ui_material::{UiMaterial, UiMaterialPlugin, UiParams};
use crate::ui_sounds::Rows;
use crate::ui_view::{Drawn, LAYER as SCREENS_LAYER, console_input};

/// The render layer of the quad that shows the image in the window.
const WINDOW_LAYER: usize = 10;

/// The stick's tilt from which a pad counts as used.
const STICK: f32 = 0.3;

/// How many frames the screens' image is drawn after a change, while their
/// meshes and textures reach the GPU.
const REDRAWN: u8 = 3;

/// What the catalogue's keys do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    /// The next of the five sizes.
    NextSize,
    /// A frame around every control, with its tag.
    Outlines,
    /// The next backdrop behind the screen.
    Backdrop,
    /// The tryout so far, saved as a story.
    SaveTryout,
    /// Back to the list of stories.
    List,
    /// KOTOR's layout or kjk's override of it.
    Layouts,
    /// The bar of the mode and the keys, hidden or shown.
    HideBar,
}

/// The catalogue's keys: what they do, the keyboard key and the pad chord.
/// Later keys of the catalogue join this table.
const KEYS: [(Action, &str, &str); 7] = [
    (Action::NextSize, "F6", "LB+North"),
    (Action::List, "F8", "LB+East"),
    (Action::Outlines, "F9", "LB+South"),
    (Action::Backdrop, "F10", "LB+DPadUp"),
    (Action::SaveTryout, "F11", "LB+DPadDown"),
    (Action::Layouts, "F5", "LB+DPadLeft"),
    (Action::HideBar, "F12", "LB+DPadRight"),
];

/// What the catalogue opens on.
#[derive(Debug, Clone)]
pub struct Start {
    pub module: String,
    pub story: Story,
    pub game: Game,
    /// The size it opens at, by name (`ui::catalogue::SIZES`).
    pub size: &'static str,
    /// It opens on the list of stories, with the cursor on `story`.
    pub list: bool,
}

pub struct CataloguePlugin {
    pub start: Start,
}

impl Plugin for CataloguePlugin {
    fn build(&self, app: &mut App) {
        let settings = crate::settings::Settings::load();
        let overrides = settings
            .value(ui::layout_files::OVERRIDES)
            .is_none_or(|value| value == "true");
        match Catalogue::open(&self.start, overrides) {
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
        app.add_plugins((
            UiMaterialPlugin,
            crate::pad::SticksPlugin,
            crate::gui_scene::StagedViewsPlugin,
        ))
        .insert_resource(ClearColor(Color::BLACK))
        .init_resource::<Painting>()
        .add_systems(
            Update,
            (step, views, paint)
                .chain()
                .after(console::ConsoleQueue)
                .after(crate::pad::Settled),
        )
        .configure_sets(
            Update,
            crate::gui_scene::StagedViews.after(views).before(paint),
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
    /// The window as the camera that shows the image last saw it: its
    /// logical size, and its physical pixels to each logical one.
    window: ((f32, f32), f32),
    placement: Placement,
    /// The screens as the last frame drew them.
    layers: Vec<DrawList>,
    /// The screen module and the layout of each of `layers`.
    names: Vec<(String, String)>,
    collab: Collab,
    chrome: Chrome,
    /// What `ui` commands of the console left for the next frame's input.
    scripted: Vec<Vec<String>>,
    scripted_hold: Option<[f32; 2]>,
    sticks: Held,
    last_pointer: Option<Vec2>,
    rows: Rows,
    /// The list or a story could not be opened, so it is not tried each
    /// frame.
    reopen_failed: bool,
    /// The screen is the list of stories, not a story.
    listing: bool,
    /// How many times the staged screens were replaced by ones with other
    /// assets than the ones before.
    assets_replaced: u32,
    /// kjk's overrides of KOTOR's layouts are applied (`ui::layout_files`).
    overrides: bool,
    /// The assets of each game shown, kept for the next story of the game.
    kept: Kept,
}

impl Catalogue {
    fn open(start: &Start, overrides: bool) -> Result<Catalogue, String> {
        let discovery = crate::discover();
        let size = SIZES
            .iter()
            .position(|size| size.name == start.size)
            .ok_or(format!("{}: no such size", start.size))?;
        let staged = if start.list {
            Catalogue::stage_list(None, &discovery, start, SIZES[size])?
        } else {
            Catalogue::stage(None, &discovery, start, SIZES[size])?
        };
        let window = SIZES[size].window();
        let mut catalogue = Catalogue {
            start: start.clone(),
            discovery,
            staged,
            size,
            window: (window, 1.0),
            placement: Placement {
                origin: [0.0; 2],
                size: [0.0; 2],
                scale: 1.0,
            },
            layers: Vec::new(),
            names: Vec::new(),
            collab: Collab::new(start),
            chrome: Chrome::default(),
            scripted: Vec::new(),
            scripted_hold: None,
            sticks: Held::default(),
            last_pointer: None,
            rows: Rows::default(),
            reopen_failed: false,
            listing: start.list,
            assets_replaced: 0,
            overrides,
            kept: Kept::default(),
        };
        catalogue.apply_overrides();
        catalogue.placement = catalogue.placed();
        Ok(catalogue)
    }

    /// The story opened with its steps taken, in the window of `size`.
    fn stage(
        kept: Option<&mut Kept>,
        discovery: &vfs::Discovery,
        start: &Start,
        size: Size,
    ) -> Result<Staged, String> {
        let story = &start.story;
        let capture = story.capture_for(start.game, Input::Mouse).ok_or(format!(
            "{}: no capture on {} for the mouse",
            story.name,
            start.game.key()
        ))?;
        let scene = Scene {
            game: start.game,
            screen: story.screen.clone(),
            steps: story.steps.clone(),
            error: story.error.clone(),
            capture: capture.map(str::to_string),
        };
        let mut staged = stage_with(kept, discovery, scene, size)?;
        staged.name_models();
        Ok(staged)
    }

    /// The story of `start` opened as `stage` does, with `more` steps taken
    /// after its own, or the list of stories when `list`. It uses the
    /// assets of the screens shown now and of the games shown before, so
    /// that the install's files are not read again. When it fails, the
    /// screens shown draw as they did.
    fn restage(&mut self, start: &Start, more: &[Step], list: bool) -> Result<Staged, String> {
        let size = self.size();
        let reclaimed = self.kept.reclaim(&self.discovery, &mut self.staged).is_ok();
        let kept = Some(&mut self.kept);
        let result = if list {
            Catalogue::stage_list(kept, &self.discovery, start, size)
        } else {
            Catalogue::stage(kept, &self.discovery, start, size)
        };
        let result = result.and_then(|mut staged| {
            more.iter()
                .try_for_each(|step| staged.step(&self.discovery, step))?;
            Ok(staged)
        });
        if result.is_err() && reclaimed {
            self.kept.restore(&mut self.staged);
        }
        result
    }

    /// `staged` shown in place of the screens so far. A change of game
    /// is a change of assets, and the textures and sounds made from the
    /// old ones belong to them (the new assets use the same texture
    /// numbers), so what was made from those is forgotten.
    fn replace(&mut self, staged: Staged) {
        let other_assets = self.staged.game() != staged.game();
        self.staged = staged;
        self.apply_overrides();
        if other_assets {
            self.assets_replaced += 1;
            self.chrome.forget_font();
        }
        self.rows = Rows::default();
    }

    /// The list of stories, with its cursor on the story of `start`.
    fn stage_list(
        kept: Option<&mut Kept>,
        discovery: &vfs::Discovery,
        start: &Start,
        size: Size,
    ) -> Result<Staged, String> {
        let scene = Scene {
            game: start.game,
            screen: format!("catalogue_list:{}/{}", start.module, start.story.name),
            steps: Vec::new(),
            error: None,
            capture: None,
        };
        stage_with(kept, discovery, scene, size)
    }

    /// The list of stories shown, its cursor on the story that was open.
    fn show_list(&mut self) {
        match self.restage(&self.start.clone(), &[], true) {
            Ok(staged) => {
                info!(target: "catalogue", "the list of stories");
                self.replace(staged);
                self.collab.restaged();
                self.listing = true;
            }
            Err(why) => {
                warn!(target: "catalogue", "{why}");
                self.reopen_failed = true;
            }
        }
    }

    /// The story `module/name` of the list opened live, on the game shown
    /// when it is drawn on it, else on its first game.
    fn open_story(&mut self, module: &str, name: &str) {
        let found = ui::catalogue::read_all(&ui::catalogue::folder()).and_then(|files| {
            let file = files.into_iter().find(|file| file.module == module);
            file.and_then(|file| {
                let story = file.stories.into_iter().find(|story| story.name == name)?;
                Some((file.module, story))
            })
            .ok_or(format!("{module}/{name}: no such story"))
        });
        let (module, story) = match found {
            Ok(found) => found,
            Err(why) => return warn!(target: "catalogue", "{why}"),
        };
        let game = if story.games.contains(&self.start.game) {
            Some(self.start.game)
        } else {
            story.games.first().copied()
        };
        let Some(game) = game else {
            return warn!(target: "catalogue", "{}: no game to draw it on", story.name);
        };
        let start = Start {
            module,
            story,
            game,
            size: self.start.size,
            list: false,
        };
        match self.restage(&start, &[], false) {
            Ok(staged) => {
                info!(target: "catalogue", story = start.story.name, "opened from the list");
                self.start = start;
                self.replace(staged);
                self.collab.opened(&self.start.module);
                self.listing = false;
            }
            Err(why) => warn!(target: "catalogue", "{why}"),
        }
    }

    fn size(&self) -> Size {
        SIZES[self.size]
    }

    /// What the bar says.
    fn facts(&self) -> Facts {
        Facts {
            story: if self.listing {
                "list of stories".to_string()
            } else {
                format!("{}/{}", self.start.module, self.start.story.name)
            },
            game: match self.start.game {
                Game::Kotor1 => "KOTOR 1",
                _ => "KOTOR 2",
            },
            layouts: ui::layout_files::describe(
                self.names.iter().map(|(_, layout)| layout.as_str()),
                self.overrides,
            ),
            look: self.look(),
        }
    }

    fn look(&self) -> Look {
        Look {
            size: self.size(),
            backdrop: self.collab.backdrop_name(),
            outlines: self.collab.outlines(),
            overrides: self.overrides,
        }
    }

    /// The part of the window the image is shown in: all of it but the bar.
    fn area(&self) -> (f32, f32) {
        let (window, factor) = self.window;
        let bar = self.chrome.bar_height(&self.facts(), window.0, factor);
        (window.0, (window.1 - bar).max(1.0))
    }

    fn view(&self) -> View {
        let size = self.size();
        View {
            image: size.pixels,
            window: self.area(),
            window_factor: self.window.1,
            margin: ui::device_frame::margin(&size),
        }
    }

    /// Where the image stands in the window now.
    fn placed(&self) -> Placement {
        place(&self.view())
    }

    /// The bar hidden or shown again. A notice says how to bring it back.
    fn hide_bar(&mut self, hidden: bool) {
        self.chrome.hidden = hidden;
        if hidden {
            let key = self.chrome.key_of(Action::HideBar);
            self.chrome
                .say("Bar hidden", &format!("Press {key} to show it again."));
        }
        info!(target: "catalogue", hidden, "bar");
    }

    /// The screens' setting for the overrides made to say what the key
    /// chose. The stack reads its layouts again when it changes.
    fn apply_overrides(&mut self) {
        self.staged
            .set_setting(ui::layout_files::OVERRIDES, self.overrides.to_string());
    }

    fn set_overrides(&mut self, on: bool) {
        self.overrides = on;
        self.apply_overrides();
        info!(target: "catalogue", on, "layout overrides");
    }

    fn set_size(&mut self, size: usize) {
        self.size = size % SIZES.len();
        info!(target: "catalogue", size = self.size().name, "size");
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::NextSize => self.set_size(self.size + 1),
            Action::Outlines => collab::act(self, Tool::Outlines),
            Action::Backdrop => collab::act(self, Tool::Backdrop),
            Action::SaveTryout => collab::act(self, Tool::SaveTryout),
            Action::List => self.show_list(),
            Action::Layouts => self.set_overrides(!self.overrides),
            Action::HideBar => self.hide_bar(!self.chrome.hidden),
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
                    warn!(target: "console", "ui catalogue: {name} is not one of the five sizes")
                }
            },
            ["catalogue", "overrides", "on" | "off" | "toggle"] => {
                self.set_overrides(match words[2] {
                    "on" => true,
                    "off" => false,
                    _ => !self.overrides,
                });
            }
            ["catalogue", "bar", "show" | "hide" | "toggle"] => {
                let hidden = match words[2] {
                    "show" => false,
                    "hide" => true,
                    _ => !self.chrome.hidden,
                };
                self.hide_bar(hidden);
            }
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
        let (stack, vm) = self.staged.stack_mut();
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
                if words[0] == "nav" {
                    self.chrome.hints = Hints::Pad;
                }
                self.scripted.push(request.to_vec());
            }
            other => warn!(target: "console", "ui: {other:?} is not a request of the catalogue"),
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
            cat.chrome.hints = if input.raw[at] == pad {
                Hints::Pad
            } else {
                Hints::Keyboard
            };
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

    cat.placement = cat.placed();

    // The pointer, from the window into the image, then into the virtual
    // window's logical pixels.
    if let Some(point) = input.pointer {
        let in_bar = point[1] >= cat.area().1;
        input.pointer = cat
            .placement
            .to_image(point)
            .filter(|_| !in_bar)
            .map(|pixel| [pixel[0] / size.factor, pixel[1] / size.factor]);
        if input.pointer.is_none() {
            input.click = false;
            input.held = false;
        }
    }
    input.window = size.window();
    input.scale_factor = size.factor;

    let (keyboard, mouse_used, pad_used) = (
        keys.as_deref()
            .is_some_and(|keys| keys.get_just_pressed().next().is_some()),
        input.click || input.wheel != 0.0 || input.moved,
        pads.iter().any(|(pad, sticks)| {
            pad.digital().get_just_pressed().next().is_some() || sticks.left.length() > STICK
        }),
    );
    if !scripted {
        if pad_used {
            cat.chrome.hints = Hints::Pad;
        } else if keyboard || mouse_used {
            cat.chrome.hints = Hints::Keyboard;
        }
    }
    let from = collab::source_of(
        keyboard,
        input.click || input.wheel != 0.0,
        pad_used,
        scripted,
    );
    collab::before(cat, &input, from);
    let live = cat.staged.live(&input);
    for why in &live.refusals {
        debug!(target: "ui", "refused: {why}");
    }
    crate::ui_sounds::play(
        cat.staged.assets_mut(),
        live.sounds.iter().copied(),
        (&mut now, &mut added),
        &mut cat.rows,
    );
    let mut picked = None;
    for intent in &live.intents {
        debug!(target: "catalogue", ?intent, "the screen asked for");
        if let ui::intent::Intent::OpenStory(module, name) = intent {
            picked = Some((module.clone(), name.clone()));
        }
    }
    cat.names = (live.shown.iter())
        .map(|shown| (shown.screen.clone(), shown.layout.clone()))
        .collect();
    cat.layers = live.shown.into_iter().map(|shown| shown.list).collect();
    cat.chrome.look_for_font(&cat.layers);
    let look = cat.look();
    cat.chrome.watch(look, time.delta_secs());
    if let Some((module, name)) = picked {
        cat.open_story(&module, &name);
    } else if !cat.reopen_failed && !cat.staged.stack().is_open() && !cat.staged.is_play() {
        cat.show_list();
    }
    collab::end(cat, &input, time.delta_secs());
    if facts.wanted() {
        facts.0.insert(
            "catalogue".into(),
            serde_json::json!({
                "listing": cat.listing,
                "story": format!("{}/{}", cat.start.module, cat.start.story.name),
                "game": cat.start.game.key(),
                "size": size.name,
                "device": [size.pixels.0, size.pixels.1],
                "factor": size.factor,
                "window": [cat.window.0.0, cat.window.0.1],
                "window_factor": cat.window.1,
                "image_at": cat.placement.origin,
                "image_size": cat.placement.size,
                "image_scale": cat.placement.scale,
                "bar_hidden": cat.chrome.hidden,
                "layout_overrides": cat.overrides,
                "screens": cat.staged.stack().screens().map(|screen| screen.0).collect::<Vec<_>>(),
                "focused": cat.staged.stack().focused(),
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
    /// The `assets_replaced` the textures made so far belong to.
    assets: u32,
    made: Option<Made>,
    /// The device's frame, the bar and the notice, drawn in the window over
    /// the image's quad.
    chrome: Drawn,
    chrome_assets: u32,
    /// The still of the world behind the screens, when the backdrop is a
    /// frame of play and it is made.
    still: collab::Shown,
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

/// The 3D views the story's controls ask for (`gui_scene::want_staged`),
/// drawn at the virtual screen's pixel size.
#[allow(clippy::too_many_arguments)]
fn views(
    mut commands: Commands,
    cat: Res<Catalogue>,
    mut scenes: ResMut<crate::gui_scene::GuiScenes>,
    mut cameras: Query<&mut Camera, Without<crate::ui_composite::UiComposite>>,
    mut visibilities: Query<&mut Visibility>,
    mut assets: crate::scene::ModelAssets,
    mut replaced: Local<u32>,
) {
    if *replaced != cat.assets_replaced {
        *replaced = cat.assets_replaced;
        crate::gui_scene::forget_views(&mut commands, &mut scenes);
    }
    let size = cat.size();
    let window = size.window();
    crate::gui_scene::want_staged(
        &mut commands,
        &mut scenes,
        (&cat.layers, cat.staged.assets(), cat.staged.vm()),
        (Vec2::new(window.0, window.1), size.factor),
        (&mut cameras, &mut visibilities, &mut assets),
    );
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
    mut scenes: ResMut<crate::gui_scene::GuiScenes>,
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
    if collab::show(
        &mut painting.still,
        collab::still(&cat),
        (virtual_window, SCREENS_LAYER),
        (&mut commands, &mut meshes, &mut materials, &mut images),
    ) {
        painting.fresh = REDRAWN;
    }
    // The screens' quads, made again when the screens or the size changed.
    let assets = cat.staged.assets();
    let screens = collab::composed(&cat);
    let composed = screens.lists();
    if painting.assets != cat.assets_replaced {
        painting.assets = cat.assets_replaced;
        painting.drawn.forget_all();
    }
    let released = painting.drawn.forget_released(assets);
    if painting.drawn.remake(&composed, virtual_window) || released {
        painting.fresh = REDRAWN;
        painting.drawn.paint_runs(
            &mut commands,
            &composed,
            (virtual_window, assets),
            (&mut *meshes, &mut *materials, &mut *images),
            Some(&mut scenes),
            &mut visibility,
            SCREENS_LAYER,
        );
    }
    // The frame, the bar and the notice, drawn in the window itself.
    let factor = cat.window.1;
    let mut lists = Vec::new();
    lists.push(ui::device_frame::draw(
        &size,
        &placement,
        window,
        2.0 / factor,
    ));
    lists.extend(cat.chrome.bar(&cat.facts(), window, factor));
    lists.extend(cat.chrome.notice(window, factor));
    if painting.chrome_assets != cat.assets_replaced {
        painting.chrome_assets = cat.assets_replaced;
        painting.chrome.forget_all();
    }
    painting.chrome.forget_released(assets);
    let window_size = Vec2::new(window.0, window.1);
    if painting.chrome.remake(&lists, window_size) {
        painting.chrome.paint_runs(
            &mut commands,
            &lists,
            (window_size, assets),
            (&mut *meshes, &mut *materials, &mut *images),
            None,
            &mut visibility,
            WINDOW_LAYER,
        );
    }
    if let Ok(mut camera) = cameras.get_mut(made.screens) {
        let draws = painting.fresh > 0 || painting.drawn.live();
        if camera.is_active != draws {
            camera.is_active = draws;
        }
    }
    painting.fresh = painting.fresh.saturating_sub(1);
}
