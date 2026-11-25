//! KOTOR's screens in the game: each frame the open screens step with the
//! frame's input (`ui_input`) and draw over everything else: a camera of
//! their own draws them into an image the window's size, and another,
//! on whatever the scene draws into (so headless captures show them too),
//! composites it over the world in gamma space (`ui_composite`).
//!
//! The draw list goes to the GPU as runs of quads that share a texture,
//! in the list's order, each run nearer the camera than the last; a
//! quad's clip is cut on the CPU. Images and colours go as their sRGB
//! bytes, unconverted, so the screens blend as KOTOR's do.
//!
//! Intents that leave the screens wait in `UiIntents` for the systems
//! that act on them; `UiOpen` says whether any screen is open.
//!
//! In play, KOTOR's HUD (`ui::screens::hud`) is drawn first, under the
//! screens, fitted to the window's shape, but not during a conversation;
//! `KotorHud` says it is in use (the fight's own HUD then stands down).
//! Words floating over creatures (`ui::screens::floaty`) go over it, with
//! it; a bark's bubble (`ui::screens::bark`) over them, under the screens;
//! a conversation's screen (`ui::screens::dialog`) in its place while this
//! player talks.
//!
//! The floating target block and the floating words follow points in the
//! world. They are placed again in `PostUpdate`, after the scene's camera
//! has its place for the frame, and the changed layers are drawn again
//! there (`float_layers`, `Floated`).

use std::collections::BTreeMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::camera::{
    ClearColorConfig, OrthographicProjection, Projection, RenderTarget, ScalingMode,
};
use bevy::core_pipeline::tonemapping::{DebandDither, Tonemapping};
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::MouseWheel;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::window::{CursorOptions, PrimaryWindow};
use ui::assets::GuiAssets;
use ui::draw::{DrawList, Quad, TextureId, fit, turn};
use ui::intent::Intent;
use ui::raster::texel;
use ui::screen::Stack;
use ui::screens::action_bar::ActionBar;
use ui::screens::tab::Page;
use ui::vm::ViewModel;

use console::UiRequests;

use crate::lifecycle_view::OverlayCamera;
use crate::ui_composite::{UiComposite, UiCompositePlugin};
use crate::ui_input::{Held, gather};
use crate::ui_material::{UiMaterial, UiMaterialPlugin, UiParams};

/// The screens' render layer: nothing else is on it.
pub(crate) const LAYER: usize = 9;

/// The open screens, what they draw with and what they show.
#[derive(Resource)]
pub struct Ui {
    pub stack: Stack,
    pub assets: Option<GuiAssets>,
    pub vm: ViewModel,
    pub(crate) layers: Vec<DrawList>,
    /// The console's `nav`, `type` and `back`, for this frame's step.
    scripted: Vec<Vec<String>>,
    /// Where the console's `ui down` and `ui drag` hold the main button
    /// down, until `ui up` lets it go.
    scripted_hold: Option<[f32; 2]>,
    /// KOTOR's HUD layout, once read (none: it does not read), where it was
    /// read from, and whether it is the game's own rather than a mod's.
    hud: Option<Option<(fmt_odyssey::gui::Gui, ui::skin::Source, bool)>>,
    /// KOTOR's HUD layout as last placed, with what it was placed from and
    /// how much it was stretched down (`hud_gui`).
    hud_placed: Option<(HudPlaced, Arc<fmt_odyssey::gui::Gui>, f32)>,
    /// How many times the game's settings changed in `vm.settings`
    /// (`Settings::shown`).
    pub(crate) settings_revision: u32,
    /// The bark bubble's layout, the same way.
    bark: Option<Option<ui::screens::bark::Bubble>>,
    /// The conversation screen's layout, the same way.
    dialog: Option<Option<ui::screens::dialog::Dialog>>,
    /// The status summary's layout, the same way.
    summary: Option<Option<(fmt_odyssey::gui::Gui, ui::skin::Source)>>,
    /// The area transition panel's layout, the same way.
    transition: Option<Option<ui::screens::transition::Panel>>,
    /// KOTOR's HUD as last drawn, and its banner, with what they were
    /// drawn from (`hud_layer`).
    hud_drawn: [Option<(HudDrawn, HudKept)>; 2],
    /// The datapad (its item id) whose box the status summary shows now
    /// (`progress`): while the box is up, the journal key opens the
    /// inventory on it instead of opening the journal.
    pub(crate) datapad_up: Option<u32>,
    /// The datapad the journal key opened the inventory on, whose box
    /// `progress` then takes down.
    pub(crate) datapad_read: Option<u32>,
    /// Screen hotkeys the console pressed (`ui hotkey journal`), for this
    /// frame's step, by their screens' names.
    hotkeys: Vec<String>,
    /// The display's physical pixels to each of the window's logical
    /// pixels, as of the last step (`UiInput::scale_factor`).
    scale_factor: f32,
    /// What the floating target block keeps from frame to frame, and the
    /// free pointer this frame (window pixels).
    float: ui::screens::hud::FloatHold,
    float_pointer: Option<[f32; 2]>,
    /// The floating target block and the floating words, which are placed
    /// after the camera moves (`float_layers`).
    floating: Floating,
    /// The status summary's OK as last drawn (window pixels: left, top,
    /// width, height), whether the pointer is over it, and whether the
    /// player pressed it since `progress` last looked.
    summary_ok: Option<[f32; 4]>,
    pub(crate) summary_hover: bool,
    pub(crate) summary_closed: bool,
    /// The run has no window (`scene::Headless`).
    headless: bool,
    /// The player hid the HUD with the Hide interface key (`play_keys`).
    /// Any open screen shows it again.
    pub(crate) hud_hidden: bool,
    /// Every save on disk with the game it belongs to, as last read
    /// (`save_view::refresh`). `vm.saves` lists only the saves of the
    /// campaign the menu or the game is on now.
    pub(crate) all_saves: Vec<(vfs::Game, ui::vm::SaveView)>,
    /// The campaign `vm.saves` was last filtered for.
    pub(crate) saves_for: Option<vfs::Game>,
}

/// The floating target block and the words floating over creatures. Both
/// follow points in the world, so they are placed in `PostUpdate`, after the
/// scene's camera has its place for the frame (`float_layers`). `step` runs
/// before the camera moves. It puts both into the frame's layers as they
/// were last placed, and notes here where they stand.
#[derive(Default)]
struct Floating {
    /// The window `step` laid the frame's layers out in.
    window: (f32, f32),
    /// The floating target block as last placed, while there is one.
    block: Option<DrawList>,
    /// The floating words as last placed, while there are any.
    words: Option<DrawList>,
    /// Where the block's quads start among the HUD's quads this frame. It
    /// is set while `step` made the HUD.
    block_at: Option<usize>,
    /// The HUD's layer is the first of this frame's layers. The floating
    /// words are the second layer while there are any.
    shown: bool,
    /// The target's point on the screen that the block in this frame's
    /// layers was placed with.
    point: Option<ui::vm::TargetPoint>,
    /// `float_layers` changed this frame's layers.
    changed: bool,
}

/// What KOTOR's HUD layout is placed from (`hud_gui`), besides the layout
/// itself.
#[derive(PartialEq)]
struct HudPlaced {
    window: (f32, f32),
    /// Whether the HUD's pictures keep their shape (`hud_pictures`).
    pictures: bool,
    /// The Interface option "Target block at the top centre" is on.
    target_top: bool,
    /// The player plays with a controller, so the action slots stand in a
    /// cross (`hud::bar_cross`).
    cross: bool,
    /// How many action slots the HUD shows. The frame round the row is as
    /// wide as they are (`hud::bar_extra`).
    slots: usize,
    /// Which of the Form slot and the feat slot the HUD shows, inside the
    /// frame after the slots (`hud::EXTRA`).
    extras: [bool; 2],
}

/// KOTOR's HUD as kept between frames (`draw_hud`): all of it but what
/// changes on most frames, which `hud_layer` adds to a copy each frame.
struct HudKept {
    /// The HUD without its minimap and its duel bars, with what pulses at
    /// full alpha.
    out: DrawList,
    /// Which of `out`'s quads pulse (`ui::paint::Painter::pulses`).
    pulses: Vec<std::ops::Range<usize>>,
    /// Where in `out`'s quads the minimap's arrow goes (its picture goes
    /// before them all), and where the duel bars go.
    arrow_at: usize,
    duel_at: usize,
}

/// The parts of the HUD drawn afresh each frame (`hud_moving`).
#[derive(Default)]
struct HudMoving {
    /// The minimap's picture, under the layout, and its arrow, over it.
    under: Vec<Quad>,
    arrow: Option<Quad>,
    duel: Vec<Quad>,
}

/// All the kept part of KOTOR's HUD is drawn from (`hud_layer`): the
/// view-model's parts it reads, the window, the palette, the campaign and
/// how often the module's art was let go. The same again, it draws the same.
#[derive(PartialEq)]
struct HudDrawn {
    window: (f32, f32),
    scale_factor: f32,
    /// Whether the HUD's pictures keep their shape (`hud_pictures`).
    pictures: bool,
    /// The HUD's view without what `hud_moving` draws: the player's place
    /// and turn on the minimap, and how full the duel bars are.
    hud: Option<ui::vm::HudView>,
    party: Vec<ui::vm::MemberView>,
    /// The target as the HUD drawn under the floating block shows it: its
    /// menu point left out, as the block is drawn afresh each frame
    /// (`target_block`).
    target: Option<ui::vm::TargetView>,
    /// The Interface option "Target block at the top centre" is on.
    target_top: bool,
    /// How many times the settings had changed (`Ui::settings_revision`).
    /// The HUD reads settings such as "Hide Quick Menu Buttons" and the Text
    /// size, so any change of a setting draws it again.
    settings: u32,
    calls: ui::vm::CallsView,
    requests: Option<ui::vm::RequestsView>,
    crosshair: bool,
    palette: ui::paint::Palette,
    game: vfs::Game,
    released: u32,
}

impl Ui {
    /// The screens of `stack`, drawn from `assets` and showing `vm`, before
    /// their first frame. `headless` says that the run has no window.
    pub(crate) fn new(
        stack: Stack,
        assets: Option<GuiAssets>,
        vm: ViewModel,
        headless: bool,
    ) -> Ui {
        Ui {
            stack,
            assets,
            vm,
            layers: Vec::new(),
            scripted: Vec::new(),
            scripted_hold: None,
            hud: None,
            hud_placed: None,
            settings_revision: 0,
            bark: None,
            transition: None,
            dialog: None,
            summary: None,
            hud_drawn: [None, None],
            datapad_up: None,
            datapad_read: None,
            hotkeys: Vec::new(),
            scale_factor: 1.0,
            float: ui::screens::hud::FloatHold::default(),
            float_pointer: None,
            floating: Floating::default(),
            summary_ok: None,
            summary_hover: false,
            summary_closed: false,
            headless,
            hud_hidden: false,
            all_saves: Vec::new(),
            saves_for: None,
        }
    }

    /// The target's point on the screen that the floating target block in
    /// this frame's layers was placed with (`float_layers`). None while no
    /// block floats.
    pub(crate) fn block_point(&self) -> Option<ui::vm::TargetPoint> {
        self.floating.point
    }

    /// Where the floating target block was last placed (`hud::FloatHold`):
    /// its left, its top and its height in the HUD's frame pixels.
    pub(crate) fn block_placed(&self) -> Option<(i32, i32, i32)> {
        self.float.placed
    }

    /// Whether the status summaries stand in the window's corner rather
    /// than in its middle. They do when the Interface option "Notifications
    /// in the corner" is on, and always in a headless run. Nobody can press
    /// the OK of a summary waiting in the middle of a headless run, and while
    /// it waits the run could not target or use anything.
    pub(crate) fn corner_notices(&self) -> bool {
        self.headless || ui::draw::kjk_option(&self.vm.settings, ui::draw::CORNER_NOTICES)
    }

    /// The conversation screen placed for what it shows now, in a window
    /// of `window` pixels, while there is a conversation and its layout
    /// reads.
    pub fn dialog_placed(&mut self, window: (f32, f32)) -> Option<ui::screens::dialog::Placed> {
        let view = self.vm.dialog.as_ref()?;
        let assets = self.assets.as_mut()?;
        let dialog = self
            .dialog
            .get_or_insert_with(|| ui::screens::dialog::Dialog::read(assets))
            .as_ref()?;
        dialog.place(assets, view, window)
    }

    /// `draw` with KOTOR 2's sizes of text and borders, in the display's
    /// physical pixels (`ui::draw::Sizing::at`), for a layer drawn `scale`
    /// window pixels to each of its own pixels in a window of `window`
    /// pixels. The textures hold that sizing while `draw` runs, with the
    /// font letter the stack set, and `draw` is given it too. The fonts,
    /// the painter's borders and a list's rows all read it. The sizing the
    /// textures had before is put back afterwards.
    fn sized<T>(
        &mut self,
        window: (f32, f32),
        scale: f32,
        draw: impl FnOnce(&mut Ui, ui::draw::Sizing) -> Option<T>,
    ) -> Option<T> {
        use ui::draw::Textures;
        let assets = self.assets.as_mut()?;
        let before = assets.sizing();
        let sizing = before.at(window, self.scale_factor, scale);
        assets.set_sizing(sizing);
        let drawn = draw(self, sizing);
        self.assets.as_mut()?.set_sizing(before);
        drawn
    }
}

/// How many window pixels a pixel of the HUD's frame takes (`hud::place`
/// fits the frame to the window at kjk's scale).
fn hud_scale(hud: &fmt_odyssey::gui::Gui, window: (f32, f32)) -> f32 {
    let (width, height) = hud.frame();
    fit(window, (width as f32, height as f32)).0
}

/// Whether KOTOR's HUD is drawn this frame.
#[derive(Resource, Default)]
pub struct KotorHud(pub bool);

/// Intents the screens raised for the game (sessions, actions, settings).
#[derive(Resource, Default)]
pub struct UiIntents(pub Vec<Intent>);

/// Whether a screen is open (the character idles, the cursor is free).
#[derive(Resource, Default)]
pub struct UiOpen(pub bool);

/// The camera that composites the screens over the world, the window's
/// size (what the screens are laid out in).
#[derive(Component)]
pub(crate) struct UiCamera;

/// The camera that draws the screens into their image.
#[derive(Component)]
struct ScreensCamera;

/// After the screens stepped: their intents for this frame are in
/// `UiIntents`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Stepped;

/// The floating target block and the floating words are placed with the
/// frame's camera and drawn (`float_layers`). What projects their points
/// onto the screen runs before this set.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Floated;

/// The keyboard's and the mouse's buttons, when there are any.
type Buttons<'w> = (
    Option<Res<'w, ButtonInput<KeyCode>>>,
    Option<Res<'w, ButtonInput<MouseButton>>>,
);

/// A game's state the screens read: its snapshots, the keys.
type InGame<'w> = (
    Option<Res<'w, crate::sim_host::PresentedSnapshot>>,
    Option<Res<'w, crate::pad::Actions>>,
);

/// A movie up (it takes the keys and the pointer), the pointer's state (a
/// HUD click only while it is free), and where a click on a portrait, its
/// pazaak icon or the solo mode button goes.
type HudPointer<'w, 's> = (
    Option<Res<'w, crate::movie_view::Showing>>,
    Option<Res<'w, crate::splash::SplashShowing>>,
    Query<'w, 's, &'static CursorOptions, With<PrimaryWindow>>,
    (
        ResMut<'w, crate::leader_view::Picked>,
        ResMut<'w, crate::pazaak_view::WatchAsked>,
        ResMut<'w, crate::leader_view::SoloAsked>,
    ),
);

/// Quads that share a texture, in draw order: positions, texture
/// coordinates and colours, four corners a quad.
struct Run {
    key: (TextureId, bool, bool),
    positions: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    colors: Vec<[f32; 4]>,
}

/// A run's corners, texture coordinates and colours.
type Geometry = (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<[f32; 4]>);

/// A run's entity: its mesh and what it was made of (a run drawn the same
/// as the frame before is not sent again), the material it has, whether
/// it shows.
struct Shown {
    entity: Entity,
    mesh: Handle<Mesh>,
    geometry: Geometry,
    key: (TextureId, bool, bool),
    shown: bool,
}

/// A run's quads as a mesh.
fn run_mesh((positions, uvs, colors): &Geometry) -> Mesh {
    let quads = positions.len() / 4;
    let indices: Vec<u32> = (0..quads as u32)
        .flat_map(|q| [0, 2, 1, 0, 3, 2].map(|i| q * 4 + i))
        .collect();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone())
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors.clone())
    .with_inserted_indices(Indices::U32(indices))
}

/// Duration of the seamless KOTOR 1 <-> KOTOR 2 main menu wipe (seconds).
const SWITCH_DURATION: f32 = 0.48;

/// Active visual wipe between the previous campaign's menu snapshot and the
/// newly selected campaign's menu.
struct Transition {
    elapsed: f32,
    duration: f32,
    direction: f32,
    tint: [f32; 3],
}

impl Transition {
    fn for_campaign(target: vfs::Game) -> Transition {
        match target {
            vfs::Game::Kotor2 => Transition {
                elapsed: 0.0,
                duration: SWITCH_DURATION,
                direction: 1.0,
                // KOTOR 2 eerie emerald-teal accent.
                tint: [0.15, 0.88, 0.68],
            },
            _ => Transition {
                elapsed: 0.0,
                duration: SWITCH_DURATION,
                direction: -1.0,
                // KOTOR 1 Republic blue-steel accent.
                tint: [0.22, 0.65, 1.0],
            },
        }
    }
}

/// Coordinates a zero-stutter, single-frame-capture campaign switch on the
/// main menu (`waiting` -> GPU snapshot -> `captured` -> swap + `active` wipe).
#[derive(Resource, Default)]
struct CampaignSwitch {
    waiting: Option<vfs::Game>,
    captured: Option<vfs::Game>,
    active: Option<Transition>,
}

/// The GPU side: the camera, each run's entity and mesh, and the images
/// and materials made so far.
#[derive(Resource, Default)]
pub(crate) struct Drawn {
    camera: Option<Entity>,
    /// The screens' camera, its target image, and the transition snapshot image.
    screens: Option<(Entity, Handle<Image>, Handle<Image>)>,
    runs: Vec<Shown>,
    images: BTreeMap<TextureId, Handle<Image>>,
    materials: BTreeMap<(TextureId, bool, bool), Handle<UiMaterial>>,
    layers: Vec<DrawList>,
    size: Vec2,
    /// The assets' `released` these copies were kept at.
    released: u32,
    /// Frames left to draw the screens' image again after a change (their
    /// meshes, textures and size reach the GPU over the next frames); none,
    /// and it keeps what it shows, its camera resting.
    fresh: u8,
    /// A shown run shows a 3D view (`gui_scene`), which moves of itself.
    live: bool,
}

impl Drawn {
    /// Whether the meshes on the GPU side were made from `ui`'s layers as
    /// they are now.
    pub(crate) fn shows(&self, ui: &Ui) -> bool {
        self.layers == ui.layers
    }
}

/// How many frames the screens' image is drawn after a change.
const REDRAWN: u8 = 3;

pub struct UiViewPlugin {
    assets: std::sync::Mutex<Option<GuiAssets>>,
    start: Option<&'static str>,
}

impl UiViewPlugin {
    /// Screens drawn from `assets`, opening `start` first.
    pub fn new(assets: Option<GuiAssets>, start: Option<&'static str>) -> UiViewPlugin {
        UiViewPlugin {
            assets: std::sync::Mutex::new(assets),
            start,
        }
    }
}

impl Plugin for UiViewPlugin {
    fn build(&self, app: &mut App) {
        let assets = self.assets.lock().expect("the UI's assets lock").take();
        if assets.is_none() {
            warn!("the game's screens cannot be drawn (see above)");
        }
        let mut stack = Stack::new(ui::screens::open);
        if let Some(assets) = &assets {
            stack.palette = ui::paint::Palette::of(assets.game);
        }
        let mut vm = ViewModel::default();
        vm.session.version = format!(
            "kjk {} ({})",
            env!("KJK_VERSION"),
            &env!("KJK_GIT_SHA")[..8.min(env!("KJK_GIT_SHA").len())]
        );
        // NVIDIA's DLSS licence: the SDK credited where the game names itself.
        if cfg!(dlss) {
            vm.session.version = format!("{} {}", vm.session.version, ui::strings::DLSS_CREDIT);
        }
        vm.session.campaign = assets.as_ref().map(|assets| assets.game);
        if let Some(start) = self.start
            && !stack.open(start, &vm)
        {
            warn!("no screen {start}");
        }
        let headless = app
            .world()
            .get_resource::<crate::scene::Headless>()
            .is_some_and(|headless| headless.0);
        app.add_plugins((
            UiMaterialPlugin,
            UiCompositePlugin,
            crate::gui_scene::GuiScenePlugin,
        ))
        .insert_resource(Ui::new(stack, assets, vm, headless))
        .init_resource::<crate::progress::SummaryHold>()
        .init_resource::<UiIntents>()
        .init_resource::<crate::leader_view::Picked>()
        .init_resource::<crate::leader_view::SoloAsked>()
        .init_resource::<crate::pazaak_view::WatchAsked>()
        .init_resource::<KotorHud>()
        .init_resource::<UiOpen>()
        .init_resource::<Drawn>()
        .init_resource::<CampaignSwitch>()
        .add_systems(
            Update,
            (requests, step, draw, sync_switch, rest_screens)
                .chain()
                .in_set(Stepped)
                .after(console::ConsoleQueue),
        )
        .add_systems(
            PostUpdate,
            // After the scene's camera has its place for the frame. Before
            // the renderer learns of the changed meshes and of what shows,
            // so the frame draws the layers as placed here.
            (float_layers, draw.run_if(floated), rest_screens)
                .chain()
                .in_set(Floated)
                .after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::asset::AssetEventSystems)
                .before(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate)
                .before(bevy::camera::visibility::VisibilitySystems::CheckVisibility),
        )
        .add_systems(
            PostUpdate,
            host_composite.after(bevy::camera::visibility::VisibilitySystems::VisibilityPropagate),
        )
        .add_systems(Last, unclaimed);
    }
}

/// Whether two cameras draw to the same place: the primary window (none
/// given, or a window), or the same image.
fn same_target(a: Option<&RenderTarget>, b: Option<&RenderTarget>) -> bool {
    let image = |target: Option<&RenderTarget>| match target {
        None | Some(RenderTarget::Window(_)) => Some(None),
        Some(RenderTarget::Image(image)) => Some(Some(image.handle.id())),
        Some(_) => None,
    };
    image(a).is_some_and(|a| image(b) == Some(a))
}

/// On the window's last camera before the composite's, while it composites
/// in the composite camera's stead (`host_composite`).
#[derive(Component)]
struct CompositeHost;

/// The composite camera draws only while something shows on its layers (the
/// fight HUD's parts, a conversation's): with nothing there, it rests, and
/// the window's last active camera before it composites the screens over
/// the world after its own pass, as it would have, so the frame pays for
/// one 3D pass fewer. It takes the composite back the frame something shows.
#[allow(clippy::type_complexity)]
fn host_composite(
    mut commands: Commands,
    drawn: Res<Drawn>,
    mut cameras: Query<(
        Entity,
        &mut Camera,
        &RenderLayers,
        Option<&RenderTarget>,
        Option<&UiComposite>,
        Has<CompositeHost>,
    )>,
    shown: Query<(&RenderLayers, &InheritedVisibility), With<Mesh3d>>,
) {
    let Some(ui) = drawn.camera else {
        return;
    };
    let Ok((_, camera, layers, target, Some(composite), _)) = cameras.get(ui) else {
        return;
    };
    let (order, target, composite) = (camera.order, target.cloned(), composite.clone());
    let busy = shown
        .iter()
        .any(|(on, visible)| visible.get() && on.intersects(layers));
    // The window's camera drawn last before it, the one it would draw over.
    let host = (!busy)
        .then(|| {
            cameras
                .iter()
                .filter(|(entity, camera, _, at, ..)| {
                    *entity != ui
                        && camera.is_active
                        && camera.order < order
                        && same_target(*at, target.as_ref())
                })
                .max_by_key(|(_, camera, ..)| camera.order)
                .map(|(entity, ..)| entity)
        })
        .flatten();
    for (entity, mut camera, _, _, held, hosting) in &mut cameras {
        if entity == ui {
            let active = host.is_none();
            if camera.is_active != active {
                camera.is_active = active;
            }
        } else if Some(entity) == host {
            if !hosting || held.is_none_or(|held| held != &composite) {
                commands
                    .entity(entity)
                    .insert((composite.clone(), CompositeHost));
            }
        } else if hosting {
            commands
                .entity(entity)
                .remove::<(UiComposite, CompositeHost)>();
        }
    }
}

/// Intents nothing took this frame are dropped (a screen's action no
/// system handles yet), with a note.
fn unclaimed(mut intents: ResMut<UiIntents>) {
    for intent in intents.0.drain(..) {
        debug!(target: "ui", ?intent, "nothing took it");
    }
}

/// The console's `ui` verbs: `open <screen>`, `show <layout>`, `press
/// <TAG>`, `double <LIST> <row>` (a double click on the row, from 0), `nav up|down|left|right|activate|primary|second`, `raw <KEY>`, `type <text>`, `back`, `close`,
/// `security [spike]` (`lock_view`), `use TAG open|close|talk|use`,
/// `wear TEMPLATE`, `unwear TEMPLATE` (`use_view`), `act [N]` (the use key on the target, its Nth action from
/// 0 chosen first, `target_view`), `break`,
/// `resume`, `ready`, `force` (`party_calls`), `holster`, `swap` (the
/// weapon sets, `weapon_set_view`), `form ROW` (TSL's Form of that
/// `spells.2da` row, `form_view`), `portrait 2|3` (a click on
/// the HUD's second or third portrait, `leader_view`), `watch 2|3` (a
/// click on that portrait's pazaak icon, `pazaak_view`), `reply N` (the Nth
/// reply offered, from 1), `leave` (a conversation's, or watching a match's,
/// `pazaak_view`), `accept` and `end` (the host's answer
/// to the first guest asking it in) (`dialogue_view`); `status journal`,
/// `pad KIND` and `pad off` (the pad a headless run shows the prompts
/// of, `pad::ScriptedPad`); `status xp N` and `status credits N` (a status for the status summary,
/// as a script's would be, `progress`); and as the screens and keys do,
/// `save [name]`, `quicksave`, `load <id>` (`save_view`). The power wheel
/// takes `powerwheel hold`, `powerwheel point X Y`, `powerwheel step N` and
/// `powerwheel up` (`wheel_view`).
#[allow(clippy::type_complexity)]
fn requests(
    mut ui: ResMut<Ui>,
    mut pending: ResMut<UiRequests>,
    mut intents: ResMut<UiIntents>,
    (mut shown, mut security, mut uses, mut dressing, mut acted, mut calls, mut picked, mut pinned): (
        ResMut<crate::script_screens::Shown>,
        ResMut<crate::lock_view::Asked>,
        ResMut<crate::use_view::Asked>,
        ResMut<crate::use_view::Dressing>,
        ResMut<crate::target_view::Acted>,
        ResMut<crate::party_calls::Asked>,
        ResMut<crate::leader_view::Picked>,
        ResMut<crate::target_view::Pinned>,
    ),
    (mut replies, mut called, mut watch, mut picking, mut orbit, mut told): (
        Option<ResMut<crate::dialogue_view::ReplyInput>>,
        Option<ResMut<crate::dialogue_view::Called>>,
        ResMut<crate::pazaak_view::WatchAsked>,
        ResMut<crate::pick_view::Picking>,
        Option<ResMut<crate::camera_tune::Orbit>>,
        Option<ResMut<crate::progress::Told>>,
    ),
    (mut world_pad, mut power_wheel): (
        ResMut<crate::pad::ScriptedPad>,
        ResMut<crate::wheel_view::Wheel>,
    ),
) {
    use sim::script::dialogue::Pick;
    let ui = &mut *ui;
    let mut answer = |pick: Pick| {
        if let Some(replies) = replies.as_deref_mut() {
            replies.pick = Some(pick);
        }
    };
    for request in std::mem::take(&mut pending.0) {
        let words: Vec<&str> = request.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["open", name] => {
                if !ui.stack.open(name, &ui.vm) {
                    warn!(target: "console", "ui: no screen {name}");
                }
            }
            ["show", layout] => ui
                .stack
                .push(Box::new(ui::screens::Layout(layout.to_string()))),
            ["press", tag] => ui.stack.press(tag),
            ["double", tag, row] => match row.parse() {
                Ok(row) => ui.stack.double(tag, row),
                Err(_) => warn!(target: "console", "ui double: {row} is not a row's number"),
            },
            ["close"] => ui.stack.pop(),
            ["key" | "wheel" | "look", ..] => {
                if !picking.script(&words) {
                    warn!(target: "console", "ui: {words:?} is no key, wheel or look request");
                }
            }
            ["powerwheel", ..] => {
                if !power_wheel.script(&words) {
                    warn!(target: "console", "ui: {words:?} is no power wheel request");
                }
            }
            ["camera", what @ ..] => {
                if !orbit.as_deref_mut().is_some_and(|orbit| orbit.script(what)) {
                    warn!(target: "console", "ui camera: {what:?} is no orbit or zoom request");
                }
            }
            ["script", what @ ..] => match crate::script_screens::parse(what) {
                Some(screen) => shown.0.push(screen),
                None => warn!(target: "console", "ui script: {what:?} is not a script's screen"),
            },
            [
                "nav" | "type" | "back" | "pointer" | "click" | "raw" | "down" | "drag" | "up",
                ..,
            ] => {
                ui.scripted.push(request);
            }
            ["security"] => security.0.push(None),
            ["security", spike] => security.0.push(Some(spike.to_string())),
            ["act"] => acted.0 = true,
            ["hotkey", name]
                if *name == HIDE_INTERFACE || HOTKEYS.iter().any(|(_, screen)| screen == name) =>
            {
                ui.hotkeys.push(name.to_string());
            }
            ["pad", "off"] => world_pad.0 = None,
            ["pad", kind] => match crate::pad::PadKind::named(kind) {
                Some(kind) => world_pad.0 = Some(kind),
                None => {
                    warn!(target: "console", "ui pad: {kind} is not xbox, playstation, nintendo, deck or off")
                }
            },
            ["target"] => pinned.0 = None,
            ["target", tag] => pinned.0 = Some(tag.to_string()),
            ["act", chosen] => match chosen.parse() {
                Ok(chosen) => *acted = crate::target_view::Acted(true, Some(chosen)),
                Err(_) => warn!(target: "console", "ui act: {chosen} is not an action's number"),
            },
            ["use", tag, word] => match crate::use_view::verb(word) {
                Some(verb) => uses.0.push((tag.to_string(), verb)),
                None => warn!(target: "console", "ui use: {word} is not open, close, talk or use"),
            },
            [verb @ ("wear" | "unwear"), template, on @ ..] if on.len() <= 1 => dressing.0.push((
                template.to_ascii_lowercase(),
                *verb == "wear",
                on.first().map(|tag| tag.to_string()),
            )),
            // A setting changed as the options screens change it
            // (`ui set display.size 800x600`).
            ["status", what @ ..] => {
                let kind = match what {
                    ["journal"] => Some(sim::StatusKind::Journal),
                    ["xp", n] => n.parse().ok().map(sim::StatusKind::Xp),
                    ["credits", n] => n.parse().ok().map(sim::StatusKind::Credits),
                    ["light"] => Some(sim::StatusKind::Alignment { light: true }),
                    ["dark"] => Some(sim::StatusKind::Alignment { light: false }),
                    ["received", item] => Some(sim::StatusKind::Received(item.to_string())),
                    ["lost"] => Some(sim::StatusKind::Lost),
                    _ => None,
                };
                match (kind, told.as_deref_mut()) {
                    (Some(kind), Some(told)) => told.statuses.push(kind),
                    _ => {
                        warn!(target: "console", "ui status: {what:?} is not journal, xp N, credits N, light, dark, received ITEM or lost")
                    }
                }
            }
            ["datapad", template] => match told.as_deref_mut() {
                Some(told) => told.datapads.push(template.to_string()),
                None => warn!(target: "console", "ui datapad: no summary to tell"),
            },
            ["set", key, value] => intents
                .0
                .push(Intent::Setting(key.to_string(), value.to_string())),
            ["save", name @ ..] => intents.0.push(Intent::Save {
                slot: None,
                name: name.join(" "),
            }),
            ["reload"] => intents.0.push(Intent::ReloadLatest),
            ["quicksave"] => intents.0.push(Intent::Save {
                slot: Some(crate::save_view::quick_id(ui).into()),
                name: crate::save_view::QUICK_NAME.into(),
            }),
            ["load", id] => intents
                .0
                .push(Intent::Session(ui::intent::SessionIntent::Solo {
                    save: Some(id.to_string()),
                    character: None,
                    campaign: None,
                })),
            ["break"] => calls.0.push(sim::action::Action::Break),
            ["resume"] => calls.0.push(sim::action::Action::Resume),
            ["ready"] => calls.0.push(sim::action::Action::Ready),
            ["force"] => calls.0.push(sim::action::Action::Force),
            ["holster"] => calls.0.push(sim::action::Action::Holster),
            ["swap"] => calls.0.push(sim::action::Action::SwitchWeapons),
            ["solo", on @ ("on" | "off")] => calls.0.push(sim::action::Action::Solo(*on == "on")),
            ["form", row] => match row.parse() {
                Ok(row) => calls.0.push(sim::action::Action::Form(row)),
                Err(_) => warn!(target: "console", "ui form: {row} is not a spells.2da row"),
            },
            ["portrait", n @ ("2" | "3")] => picked.0 = member(&ui.vm, n),
            ["reply", n] => match n.parse::<u8>() {
                Ok(n @ 1..=30) => answer(Pick::Reply(n - 1)),
                _ => warn!(target: "console", "ui reply: {n} is not a reply's number"),
            },
            ["watch", n @ ("2" | "3")] => watch.icon = member(&ui.vm, n),
            ["leave"] => {
                answer(Pick::Leave);
                watch.leave = true;
            }
            [word @ ("accept" | "end")] => {
                if let Some(called) = called.as_deref_mut() {
                    called.0 = Some(*word == "accept");
                }
            }
            other => warn!(target: "console", "ui: {other:?} is not a ui request"),
        }
    }
}

/// The body of the HUD's `n`th portrait (`MemberView::id`), as the
/// console's `ui portrait` and `ui watch` name it.
fn member(vm: &ui::vm::ViewModel, n: &str) -> Option<u32> {
    let place = n.parse::<usize>().ok()?.checked_sub(1)?;
    Some(vm.party.get(place)?.id)
}

/// The console's `ui nav`, `ui type`, `ui back`, `ui pointer X Y`, `ui
/// click X Y` (window pixels) and `ui raw KEY` (a key pressed by name, as
/// `KeyH` or `Digit3`), put into this frame's input. `ui nav primary` is a
/// controller's X and `ui nav second` its Y. A scripted
/// pointer is there for its frame only, as a real one that moved and left.
///
/// `ui down X Y` presses the main button there and holds it down, `ui drag X
/// Y` moves the held pointer, and `ui up X Y` lets the button go there. The
/// pointer stays where the button is held, frame after frame, until `ui up`
/// (`held`), so a drag can be pictured halfway.
fn console_input(
    pending: &[Vec<String>],
    input: &mut ui::input::UiInput,
    held: &mut Option<[f32; 2]>,
) {
    use ui::input::Key;
    if let Some(at) = *held {
        input.pointer = Some(at);
        input.held = true;
    }
    let (keys, text) = (&mut input.keys, &mut input.text);
    for words in pending {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        if let [verb @ ("pointer" | "click" | "down" | "drag" | "up"), x, y] = words.as_slice()
            && let (Ok(x), Ok(y)) = (x.parse::<f32>(), y.parse::<f32>())
        {
            input.pointer = Some([x, y]);
            input.moved = true;
            input.click |= matches!(*verb, "click" | "down");
            *held = match *verb {
                "down" | "drag" => Some([x, y]),
                "up" => None,
                _ => *held,
            };
            input.held = held.is_some();
            continue;
        }
        match words.as_slice() {
            ["nav", "up"] => keys.push(Key::Up),
            ["nav", "down"] => keys.push(Key::Down),
            ["nav", "left"] => keys.push(Key::Left),
            ["nav", "right"] => keys.push(Key::Right),
            ["nav", "activate"] => keys.push(Key::Activate),
            ["nav", "primary"] => keys.push(Key::Primary),
            ["nav", "second"] => keys.push(Key::Second),
            ["raw", key] => input.raw.push(key.to_string()),
            ["back"] => keys.push(Key::Back),
            ["type", rest @ ..] => text.push_str(&rest.join(" ")),
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn step(
    mut ui: ResMut<Ui>,
    (keys, mouse): Buttons,
    (mut typed, mut wheel): (MessageReader<KeyboardInput>, MessageReader<MouseWheel>),
    gamepads: crate::pad::Pads,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<&Camera, With<UiCamera>>,
    time: Res<Time>,
    (mut held, mut last, mut sized): (Local<Held>, Local<Option<Vec2>>, Local<Option<(Vec2, f32)>>),
    (mut intents, mut open, mut states, mut kotor_hud, mut switch): (
        ResMut<UiIntents>,
        ResMut<UiOpen>,
        ResMut<console::States>,
        ResMut<KotorHud>,
        ResMut<CampaignSwitch>,
    ),
    (mut exit, mut facts, replies, mut hold, headless, scenes): (
        MessageWriter<AppExit>,
        ResMut<console::DumpFacts>,
        Option<Res<crate::dialogue_view::ReplyInput>>,
        ResMut<crate::progress::SummaryHold>,
        Option<Res<crate::scene::Headless>>,
        Option<Res<crate::gui_scene::GuiScenes>>,
    ),
    heard: Option<ResMut<crate::ui_sounds::Heard>>,
    (presented, actions): InGame,
    (showing, splash, cursors, (mut picked, mut watch, mut solo)): HudPointer,
) {
    let top = ui.stack.top().map(|screen| screen.0);
    crate::crash::note(|crumbs| {
        if crumbs.screen.as_deref() != top {
            crumbs.screen = top.map(str::to_owned);
        }
    });
    // A movie or the startup splash over the screens takes the keys and the
    // pointer, and so does the main menu held back until its 3D view is
    // drawn (`gui_scene`).
    let movie = showing.is_some_and(|showing| showing.0) || splash.is_some_and(|splash| splash.0);
    let blind = movie || scenes.is_some_and(|scenes| scenes.holding());
    let (keys, mouse) = if blind { (None, None) } else { (keys, mouse) };
    let window = windows.iter().next();
    let camera = cameras.iter().next();
    let size = camera
        .and_then(Camera::logical_viewport_size)
        .or_else(|| window.map(|window| Vec2::new(window.width(), window.height())))
        .unwrap_or(Vec2::new(1280.0, 720.0));
    // The display's physical pixels to each logical one: KOTOR 2 sizes its
    // text and borders in physical pixels (`ui::draw::Sizing::physical`).
    let scale_factor = camera
        .and_then(|camera| {
            let physical = camera.physical_viewport_size()?;
            Some(physical.x as f32 / camera.logical_viewport_size()?.x.max(1.0))
        })
        .or_else(|| window.map(Window::scale_factor))
        .unwrap_or(1.0);
    // A minimized window is 0 by 0 pixels. The screens are then laid out at
    // the last size the window had, not at 0 by 0 with a scale of 0.
    let (size, scale_factor) = match *sized {
        Some(kept) if size.x < 1.0 || size.y < 1.0 || scale_factor <= 0.0 => kept,
        _ => *sized.insert((size, scale_factor)),
    };
    // KOTOR 2's status summary waiting for its OK, shown over play: it
    // takes the keys, the pads and the pointer (`summary_input`).
    let talking = replies.as_ref().is_some_and(|replies| replies.active);
    let (minigame, faded) = presented.as_ref().map_or((false, false), |presented| {
        (
            presented.0.current().minigame.is_some(),
            crate::fade_view::covered(&presented.0) > 0.0,
        )
    });
    let waiting = presented.is_some()
        && !movie
        && !talking
        && !minigame
        && !faded
        && !ui.stack.is_open()
        && ui.vm.summaries.iter().any(|view| view.ok.is_some());
    // A movie takes the pads too; B is Back only on a screen (in play it
    // crouches), and LB's Start is the quick save.
    let pads = (!blind).then_some((&gamepads, ui.stack.is_open() || waiting));
    let mut input = gather(
        keys.as_deref(),
        mouse.as_deref(),
        &mut typed,
        &mut wheel,
        pads,
        window,
        size,
        time.delta_secs(),
        &mut held,
        &mut last,
    );
    // The held main menu takes no typed key either (Enter on Solo).
    if blind && !movie {
        input = ui::input::UiInput {
            window: input.window,
            dt: input.dt,
            ..ui::input::UiInput::default()
        };
    }
    input.scale_factor = scale_factor;
    let ui = &mut *ui;
    if let Some(target) = switch.captured.take() {
        crate::runtime::set_campaign(ui, target);
        switch.active = Some(Transition::for_campaign(target));
    } else if let Some(active) = &mut switch.active {
        // Clamp dt so first-switch texture/mipmap upload cannot eat the whole
        // transition in a single frame.
        active.elapsed += time.delta_secs().min(0.05);
        if active.elapsed >= active.duration {
            switch.active = None;
        }
    }
    ui.scale_factor = scale_factor;
    console_input(
        &std::mem::take(&mut ui.scripted),
        &mut input,
        &mut ui.scripted_hold,
    );
    ui.summary_hover = false;
    if waiting {
        summary_input(ui, &mut input);
    }
    // The summary's OK held in a window holds the world too (`sim_host`).
    hold.0 = waiting && !headless.is_some_and(|headless| headless.0);
    if presented.is_some() {
        play_keys(
            ui,
            &mut input,
            actions.as_deref().filter(|_| !movie && !waiting),
        );
    }
    ui.stack.veil_world = presented.is_some();
    let frame = match &mut ui.assets {
        Some(assets) => ui.stack.step(assets, &ui.vm, &input),
        None => Default::default(),
    };
    for why in &frame.refusals {
        // A layout that does not load closes its screen unseen (a TSL
        // galaxy map once did, its console then doing nothing).
        if why.starts_with("layout ") {
            warn!(target: "ui", "refused: {why}");
        } else {
            debug!(target: "ui", "refused: {why}");
        }
    }
    if let Some(mut heard) = heard {
        heard.0.extend(frame.sounds.iter().copied());
    }
    for intent in frame.intents {
        match intent {
            Intent::Quit => {
                exit.write(AppExit::Success);
            }
            Intent::View(member) => ui.vm.viewed = member,
            Intent::Campaign(campaign) => {
                let on_menu = ui.stack.top().is_some_and(|screen| screen.0 == "main_menu");
                let changing = ui
                    .assets
                    .as_ref()
                    .is_some_and(|assets| assets.game != campaign);
                if on_menu && changing {
                    switch.waiting = Some(campaign);
                } else {
                    switch.waiting = None;
                    switch.captured = None;
                    switch.active = None;
                    crate::runtime::set_campaign(ui, campaign);
                }
            }
            other => intents.0.push(other),
        }
    }
    if facts.wanted() {
        facts.0.insert(
            "ui".into(),
            serde_json::json!({
                "screens": ui.stack.screens().map(|screen| screen.0).collect::<Vec<_>>(),
                "focused": ui.stack.focused(),
                "quads": frame.layers.iter().map(|layer| layer.quads.len()).collect::<Vec<_>>(),
                "refusals": frame.refusals,
                "window": [size.x, size.y],
                "scale_factor": scale_factor,
                "hud_hidden": ui.hud_hidden,
            }),
        );
    }
    let mut layers = frame.layers;
    // The floating target block holds its place under a free pointer
    // (`hud::FloatHold`).
    let free = cursors.iter().next().is_some_and(|cursor| cursor.visible);
    ui.float.tick(time.delta_secs());
    ui.float_pointer = input.pointer.filter(|_| free);
    ui.floating.window = (size.x, size.y);
    ui.floating.block_at = None;
    ui.floating.shown = false;
    let hud = presented
        .is_some()
        .then(|| hud_layer(ui, (size.x, size.y), false))
        .flatten();
    kotor_hud.0 = hud.is_some();
    // As KOTOR's, it gives way to a conversation's screen, to a minigame's
    // own (its models' HUD, `minigame_view`), and to a fade (the black
    // between a cutscene's conversations).
    let mut under = 0;
    // The Hide interface key hides the HUD with the words floating over
    // creatures, the area transition panel and the arena's panels. The
    // power wheel still shows while its key is held, so the player sees
    // what they choose.
    let hidden = ui.hud_hidden;
    if let Some(hud) = hud.filter(|_| !talking && !minigame && !faded) {
        if !hidden {
            layers.insert(0, hud);
            under = 1;
            ui.floating.shown = true;
        }
        let shown = |layer: Option<DrawList>| layer.filter(|_| !hidden);
        for layer in [
            // The floating words as last placed. `float_layers` places them
            // again once the camera has moved.
            shown(ui.floating.words.clone()),
            shown(transition_layer(ui, (size.x, size.y))),
            wheel_layer(ui, (size.x, size.y)),
            shown(arena_layer(ui, (size.x, size.y))),
            shown(arena_panel_layer(ui, (size.x, size.y))),
        ]
        .into_iter()
        .flatten()
        {
            layers.insert(under, layer);
            under += 1;
        }
        // A click on a companion's portrait, the pointer free (in play the
        // mouse looks, the cursor held); on its pazaak icon, watching its
        // player's match instead; on the minimap, the map page; on the solo
        // mode button, the end of solo mode. A hidden HUD takes no click.
        if let Some(point) = input
            .pointer
            .filter(|_| input.click && free && !hidden && !ui.stack.is_open())
        {
            match hud_click(ui, (size.x, size.y), point) {
                Some(HudClick::Watch(id)) => watch.icon = Some(id),
                Some(HudClick::Portrait(id)) => picked.0 = Some(id),
                Some(HudClick::Solo) => solo.0 = true,
                Some(HudClick::Map) => {
                    ui.stack.open("map", &ui.vm);
                }
                None => {}
            }
        }
    }
    // A conversation's screen under the screens, the HUD and barks given
    // way to it; the host's request card over both.
    let in_play = presented.is_some();
    let dialog = in_play
        .then(|| dialog_layer(ui, (size.x, size.y)))
        .flatten();
    let card = in_play.then(|| card_layer(ui, (size.x, size.y))).flatten();
    for layer in [card, dialog].into_iter().flatten() {
        layers.insert(under, layer);
    }
    // The status summary over the HUD, under the screens.
    if let Some(summaries) = presented
        .is_some()
        .then(|| summary_layer(ui, (size.x, size.y)))
        .flatten()
        .filter(|_| !talking && !minigame && !faded)
    {
        layers.insert(under, summaries);
    }
    // A bark over the HUD (a minigame's too), under the screens. The Hide
    // interface key hides it with the HUD.
    if let Some(bark) = presented
        .is_some()
        .then(|| bark_layer(ui, (size.x, size.y)))
        .flatten()
        .filter(|_| !talking && !hidden)
    {
        layers.insert(under, bark);
    }
    // A terminal's camera feed: its look over the view, under the screens.
    if presented.is_some() && ui.vm.camera_feed {
        layers.insert(
            under,
            ui::screens::feed::draw((size.x, size.y), ui.stack.clock()),
        );
    }
    // A break's or ready check's banner over everything: the screens, a
    // conversation, a watching player's view.
    if let Some(banner) = presented
        .is_some()
        .then(|| hud_layer(ui, (size.x, size.y), true))
        .flatten()
    {
        layers.push(banner);
    }
    ui.layers = layers;
    // In a window the summary waiting for its OK frees the pointer and
    // holds the player as a screen does.
    open.0 = ui.stack.is_open() || hold.0;
    states.0.retain(|key, _| !key.starts_with("ui_"));
    for screen in ui.stack.screens() {
        states.0.insert(format!("ui_{}", screen.0), true);
    }
    states.0.insert("ui".into(), open.0);
}

/// Whether the HUD's pictures keep their shape (`hud::place`): the HUD is
/// the game's own layout, not a mod's, and "Maintain image aspect ratio" is
/// on. False while the HUD has not been read.
fn hud_pictures(ui: &Ui) -> bool {
    let own = matches!(ui.hud, Some(Some((_, _, true))));
    own && ui::draw::image_aspect(&ui.vm.settings)
}

/// KOTOR's HUD layout placed for a window of `window` pixels, where it was
/// read from, and how much it was stretched down (`hud::place`), when it
/// reads. The placed layout is kept and shared until the window, the
/// pictures' shape (`hud_pictures`) or the target block's place
/// (`target_top`) changes. A caller that changes the layout clones it first.
fn hud_gui(
    ui: &mut Ui,
    window: (f32, f32),
) -> Option<(Arc<fmt_odyssey::gui::Gui>, ui::skin::Source, f32)> {
    use ui::screens::hud::{LAYOUT, backed, known, place};
    let assets = ui.assets.as_mut()?;
    if ui.hud.is_none() {
        let read = assets.screen_layout(LAYOUT).ok().map(|(mut gui, source)| {
            let own = !ui::assets::Assets::layout_is_mod(assets, LAYOUT, source);
            if !own && let Ok(stock) = assets.stock_screen_layout(LAYOUT) {
                known(&mut gui, &stock);
            }
            if let Ok(campaign) = assets.layout(LAYOUT) {
                backed(&mut gui, &campaign);
            }
            (gui, source, own)
        });
        ui.hud = Some(read);
        // A layout placed before this one was read is another layout's.
        ui.hud_placed = None;
    }
    let from = HudPlaced {
        window,
        pictures: hud_pictures(ui),
        target_top: target_top(ui),
        cross: ui.vm.pad_playing,
        slots: ui.vm.hud.as_ref().map_or(0, |hud| hud.bar.len()),
        extras: (ui.vm.hud.as_ref())
            .map_or([false; 2], |hud| hud.extra().map(|slot| slot.is_some())),
    };
    let (layout, source, _) = ui.hud.as_ref()?.as_ref()?;
    if let Some((last, gui, stretch)) = &ui.hud_placed
        && *last == from
    {
        return Some((gui.clone(), *source, *stretch));
    }
    let (mut gui, source) = (layout.clone(), *source);
    let stretch = place(&mut gui, window, from.pictures);
    if from.target_top {
        ui::screens::hud::to_top(&mut gui);
    }
    ui::screens::hud::bar_extra(&mut gui, from.slots, from.extras);
    ui::screens::hud::solo_beside_portrait(&mut gui);
    if from.cross {
        ui::screens::hud::bar_cross(&mut gui);
    }
    let gui = Arc::new(gui);
    ui.hud_placed = Some((from, gui.clone(), stretch));
    Some((gui, source, stretch))
}

/// Whether the Interface option "Target block at the top centre" is on. Off,
/// the block floats over the target as KOTOR 2's does (`target_block`).
fn target_top(ui: &Ui) -> bool {
    ui::draw::kjk_option(&ui.vm.settings, ui::draw::TARGET_TOP)
}

/// The floating target block for a window of `window` pixels, drawn afresh
/// each frame over the HUD as KOTOR 2 moves it (`hud::floated`), while
/// there is a target and the block is not at the top centre. The HUD under
/// it leaves the block out. `float_layers` calls this once a frame, which
/// `hud::FloatHold` counts on.
fn target_block(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    if target_top(ui) {
        return None;
    }
    let Some(target) = ui.vm.target.clone() else {
        let pointer = ui.float_pointer;
        ui.float.point(None, pointer);
        return None;
    };
    let (gui, source, _) = hud_gui(ui, window)?;
    // Its own copy: the block is moved in it and the rest is dropped.
    let mut gui = fmt_odyssey::gui::Gui::clone(&gui);
    ui.sized(window, hud_scale(&gui, window), |ui, sizing| {
        ui::screens::hud::floated(&mut gui, &target, sizing.pixel, &mut ui.float);
        let pointer = ui.float_pointer.map(|point| {
            let (width, height) = gui.frame();
            let (scale, [left, top]) = fit(window, (width as f32, height as f32));
            [(point[0] - left) / scale, (point[1] - top) / scale]
        });
        ui.float.point(Some((&gui, &target)), pointer);
        gui.controls
            .retain(|control| ui::screens::hud::in_block(&control.tag));
        let Ui {
            assets, vm, stack, ..
        } = ui;
        let mut out = DrawList::default();
        let mut painter = ui::paint::Painter::new(assets.as_mut()?, stack.palette);
        painter.source = source;
        painter.pulsing = stack.pulsing();
        painter.layout(
            &gui,
            &|control| ui::screens::hud::state(control, vm),
            &mut out,
        );
        ui::screens::hud::target_cycle(&gui, vm, &mut painter, &mut out);
        Some(out)
    })
}

/// The power wheel for a window of `window` pixels, in the frame of the
/// HUD's layout as placed for the window, while the wheel is open
/// (`ui::screens::wheel`).
fn wheel_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let wheel = ui.vm.wheel.clone()?;
    let (gui, source, _) = hud_gui(ui, window)?;
    ui.sized(window, hud_scale(&gui, window), |ui, sizing| {
        let Ui { assets, stack, .. } = ui;
        let mut painter = ui::paint::Painter::new(assets.as_mut()?, stack.palette);
        painter.source = source;
        painter.pulsing = stack.pulsing();
        Some(ui::screens::wheel::draw(
            &gui,
            &wheel,
            sizing.pixel,
            &mut painter,
        ))
    })
}

/// What a click on the HUD is on.
enum HudClick {
    /// A companion's pazaak icon (`hud::watch_at`), by its body.
    Watch(u32),
    /// A companion's portrait (`hud::portrait_at`), by its body.
    Portrait(u32),
    /// The minimap (`hud::minimap_at`): the map page.
    Map,
    /// The solo mode button (`hud::solo_at`): the end of solo mode.
    Solo,
}

/// What the click at `point` (window pixels) on the HUD is on, if anything.
fn hud_click(ui: &mut Ui, window: (f32, f32), point: [f32; 2]) -> Option<HudClick> {
    use ui::screens::hud;
    let (gui, ..) = hud_gui(ui, window)?;
    let (width, height) = gui.frame();
    let (scale, [left, top]) = fit(window, (width as f32, height as f32));
    let at = [(point[0] - left) / scale, (point[1] - top) / scale];
    let vm = &ui.vm;
    hud::watch_at(&gui, vm, at)
        .map(HudClick::Watch)
        .or_else(|| hud::portrait_at(&gui, vm, at).map(HudClick::Portrait))
        .or_else(|| hud::minimap_at(&gui, vm, at).then_some(HudClick::Map))
        .or_else(|| hud::solo_at(&gui, vm, at).then_some(HudClick::Solo))
}

/// KOTOR's HUD for a window of `window` pixels, when the game is in play
/// and its layout reads; with `banner`, only its banner (a break, a ready
/// check: `hud::banner`), when there is one.
fn hud_layer(ui: &mut Ui, window: (f32, f32), banner: bool) -> Option<DrawList> {
    if banner {
        ui.vm.calls.banner.as_ref()?;
    } else {
        ui.vm.hud.as_ref()?;
    }
    let vm = &ui.vm;
    let from = HudDrawn {
        window,
        scale_factor: ui.scale_factor,
        pictures: hud_pictures(ui),
        hud: vm.hud.clone().map(|hud| ui::vm::HudView {
            minimap: hud.minimap.map(|minimap| ui::vm::MinimapView {
                at: [0.0; 2],
                spin: 0.0,
                ..minimap
            }),
            duel: hud.duel.map(|_| ui::vm::DuelBars::default()),
            ..hud
        }),
        party: vm.party.clone(),
        target: vm.target.clone().map(|target| ui::vm::TargetView {
            point: None,
            ..target
        }),
        target_top: target_top(ui),
        settings: ui.settings_revision,
        calls: vm.calls.clone(),
        requests: vm.requests.clone(),
        crosshair: dot_shown(vm),
        palette: ui.stack.palette,
        game: ui.assets.as_ref()?.game,
        released: ui.assets.as_ref()?.released(),
    };
    let at = usize::from(banner);
    if !matches!(&ui.hud_drawn[at], Some((last, _)) if *last == from) {
        let kept = draw_hud(ui, window, banner)?;
        ui.hud_drawn[at] = Some((from, kept));
    }
    let moving = if banner {
        HudMoving::default()
    } else {
        hud_moving(ui, window).unwrap_or(HudMoving::default())
    };
    let (_, kept) = ui.hud_drawn[at].as_ref()?;
    // The kept quads with this frame's among them, in the order one
    // drawing of the whole HUD gives.
    let (arrow_at, duel_at) = (kept.arrow_at, kept.duel_at);
    let arrows = usize::from(moving.arrow.is_some());
    let added = moving.under.len() + arrows + moving.duel.len();
    let mut quads = Vec::with_capacity(kept.out.quads.len() + added);
    quads.extend_from_slice(&moving.under);
    quads.extend_from_slice(&kept.out.quads[..arrow_at]);
    quads.extend(moving.arrow);
    quads.extend_from_slice(&kept.out.quads[arrow_at..duel_at]);
    quads.extend_from_slice(&moving.duel);
    quads.extend_from_slice(&kept.out.quads[duel_at..]);
    // What pulses was drawn at full alpha: this frame's pulse dims it.
    let pulse = ui.stack.pulsing();
    for range in &kept.pulses {
        let mut moved = moving.under.len();
        if range.start >= arrow_at {
            moved += arrows;
        }
        if range.start >= duel_at {
            moved += moving.duel.len();
        }
        for quad in &mut quads[range.start + moved..range.end + moved] {
            quad.color[3] *= pulse;
        }
    }
    let mut out = DrawList {
        size: kept.out.size,
        fill: kept.out.fill,
        quads,
        wide: kept.out.wide.clone(),
        sized: kept.out.sized.clone(),
    };
    if !banner {
        // The floating target block as last placed. `float_layers` places
        // it again once the camera has moved, and finds it here.
        ui.floating.block_at = Some(out.quads.len());
        if let Some(block) = &ui.floating.block {
            out.quads.extend(block.quads.iter().cloned());
        }
    }
    // A blaster's reticle over the HUD as kept: its gap changes as it
    // fires, the HUD under it seldom. Placed in the HUD's frame as placed
    // (`hud::place` fits it to the window).
    let placed = ui.hud.as_ref().and_then(|hud| hud.as_ref());
    let frame = placed.map(|(gui, ..)| ui::screens::hud::fitted(gui, window));
    // The interface colour of the HUD's layout: KOTOR 1's blue or KOTOR 2's
    // green.
    let recoloured = placed
        .zip(ui.assets.as_ref())
        .is_some_and(|((_, source, _), assets)| ui::draw::Textures::recoloured(assets, *source));
    let tint = ui::skin::interface_colour(recoloured);
    // One of the display's physical pixels in the frame's pixels.
    let pixel = placed.map_or(1.0, |(gui, ..)| {
        1.0 / (hud_scale(gui, window) * ui.scale_factor).max(f32::EPSILON)
    });
    let crosshair = ui.vm.crosshair && !banner;
    if let (Some(frame), true) = (frame, crosshair) {
        use ui::screens::hud::{MIN_GAP, block_arcs, gap_px, hitmarker, reticle};
        let height = frame.1 as f32;
        let gap = ui.vm.reticle.map_or(MIN_GAP * height / 1080.0, |shown| {
            gap_px(shown.cone, shown.fov, height)
        });
        // While the player blocks with targeted blocks on, the arcs of
        // targeted blocks take the crosshair's place (`block_reticle_view`).
        if let (Some(shown), None) = (ui.vm.reticle, ui.vm.block_reticle) {
            out.quads
                .extend(reticle(frame, shown.family, gap, pixel, tint));
        }
        // A landed shot's mark about it (`hitmarker_view`).
        if let Some((mark, age_ms)) = ui.vm.hitmark {
            out.quads.extend(hitmarker(frame, mark, gap, age_ms));
        }
        if let Some(view) = ui.vm.block_reticle {
            out.quads.extend(block_arcs(frame, view));
        }
    }
    // The perfect block mark, as a ring wherever no arcs show it: also
    // while the crosshair is down, as it is under a side camera.
    let arcs = crosshair && ui.vm.block_reticle.is_some();
    if let (Some(frame), true, false, false) = (frame, ui.vm.perfect_mark, arcs, banner) {
        out.quads.extend(ui::screens::hud::perfect_ring(frame));
    }
    Some(out)
}

/// The minimap and the duel bars for a window of `window` pixels, as this
/// frame's view has them. They change on most frames of play, so
/// `hud_layer` draws them each frame and keeps the rest of the HUD.
fn hud_moving(ui: &mut Ui, window: (f32, f32)) -> Option<HudMoving> {
    let hud = ui.vm.hud.as_ref()?;
    if hud.minimap.is_none() && hud.duel.is_none() {
        return None;
    }
    let (gui, source, stretch) = hud_gui(ui, window)?;
    ui.sized(window, hud_scale(&gui, window), |ui, _| {
        let Ui {
            assets, vm, stack, ..
        } = ui;
        let assets = assets.as_mut()?;
        let hud = vm.hud.as_ref()?;
        // The minimap's picture under the layout (its border), the arrow over.
        let minimap = hud
            .minimap
            .as_ref()
            .and_then(|view| ui::screens::hud::minimap(&gui, view, stretch, assets, source));
        let mut duel = DrawList::default();
        if hud.duel.is_some() {
            let mut painter = ui::paint::Painter::new(assets, stack.palette);
            painter.source = source;
            if let Some(art) = ui::screens::hud::BarArt::vitality(&gui, &mut painter) {
                ui::screens::hud::duel_bars(&gui, vm, art, stretch, &mut duel);
            }
        }
        Some(HudMoving {
            under: minimap
                .iter()
                .flat_map(ui::screens::hud::Minimap::under)
                .collect(),
            arrow: minimap.and_then(|minimap| minimap.arrow),
            duel: duel.quads,
        })
    })
}

/// The kept part of KOTOR's HUD drawn afresh (`hud_layer`): all but its
/// minimap and its duel bars (`hud_moving`), with what pulses in it at full
/// alpha. Its text and borders are KOTOR 2's size in the display's
/// physical pixels (`Ui::sized`). Its pieces stand where KOTOR 2 places
/// them (`hud::place`). The target block is left out while it floats
/// (`target_block`), and stands at the top centre with the Interface option
/// "Target block at the top centre" on (`hud::to_top`).
fn draw_hud(ui: &mut Ui, window: (f32, f32), banner: bool) -> Option<HudKept> {
    let (gui, source, _) = hud_gui(ui, window)?;
    // The floating target block is drawn on its own (`target_block`).
    let floating = !target_top(ui);
    ui.sized(window, hud_scale(&gui, window), |ui, sizing| {
        let Ui {
            assets, vm, stack, ..
        } = ui;
        let assets = assets.as_mut()?;
        let mut out = DrawList::default();
        let mut painter = ui::paint::Painter::new(assets, stack.palette);
        painter.source = source;
        painter.layout(
            &gui,
            &|control| {
                if banner {
                    ui::screens::hud::banner(control, vm)
                } else if floating && ui::screens::hud::in_block(&control.tag) {
                    ui::paint::ControlState {
                        hidden: true,
                        ..ui::paint::ControlState::default()
                    }
                } else {
                    ui::screens::hud::state(control, vm)
                }
            },
            &mut out,
        );
        let arrow_at = out.quads.len();
        if !banner {
            ui::screens::hud::effects(&gui, vm, &mut painter, &mut out);
            ui::screens::hud::busy(&gui, vm, &mut painter, &mut out);
            ui::screens::hud::players(&gui, vm, &mut painter, &mut out);
            ui::screens::hud::action_keys(&gui, vm, &mut painter, &mut out);
            if !floating {
                ui::screens::hud::target_cycle(&gui, vm, &mut painter, &mut out);
            }
        }
        let duel_at = out.quads.len();
        if dot_shown(vm) {
            out.quads.extend(ui::screens::hud::crosshair(
                &gui,
                sizing.pixel,
                painter.interface_colour(),
            ));
        }
        let pulses = std::mem::take(&mut painter.pulses);
        Some(HudKept {
            out,
            pulses,
            arrow_at,
            duel_at,
        })
    })
}

/// Whether the HUD draws its crosshair dot (`ui::screens::hud::crosshair`).
/// A blaster draws its own crosshair instead. A character with a melee
/// weapon readied has no crosshair. While the player blocks with targeted
/// blocks on, the arcs of targeted blocks take the dot's place. The HUD's
/// redraw key (`HudDrawn::crosshair`) holds this value, so the HUD is drawn
/// again when it changes.
pub(crate) fn dot_shown(vm: &ui::vm::ViewModel) -> bool {
    vm.crosshair && vm.reticle.is_none() && vm.block_reticle.is_none() && !vm.melee_out
}

/// Places the floating target block and the floating words with the
/// frame's camera, and puts them into the frame's layers in place of the
/// ones `step` put there. It runs after the transforms are propagated, after
/// the target's point is projected (`target_view`) and after the floating
/// words are projected (`floaty_view`). When either moved, `draw` runs
/// again after it, so the frame shows both where this frame's camera sees
/// them. The rest of the HUD stays as `step` drew it.
fn float_layers(mut ui: ResMut<Ui>) {
    let ui = &mut *ui;
    let window = ui.floating.window;
    let shown = ui.floating.shown;
    let before = ui.floating.block.take();
    ui.floating.point = None;
    ui.floating.changed = false;
    // The block is placed whenever `step` made the HUD, also while a
    // conversation covers it, so that it holds its place as before.
    if let Some(at) = ui.floating.block_at {
        let block = target_block(ui, window);
        if shown
            && block != before
            && let Some(hud) = ui.layers.first_mut()
        {
            let was = before.map_or(0, |block| block.quads.len());
            let quads = block.iter().flat_map(|block| block.quads.iter().cloned());
            hud.quads.splice(at..at + was, quads);
            ui.floating.changed = true;
        }
        if block.is_some() {
            ui.floating.point = ui.vm.target.as_ref().and_then(|target| target.point);
        }
        ui.floating.block = block;
    }
    let before = ui.floating.words.take();
    if !shown {
        return;
    }
    let words = floaty_layer(ui, window);
    if words != before {
        match (before.is_some(), &words) {
            (true, Some(words)) => ui.layers[1] = words.clone(),
            (true, None) => {
                ui.layers.remove(1);
            }
            (false, Some(words)) => ui.layers.insert(1, words.clone()),
            (false, None) => {}
        }
        ui.floating.changed = true;
    }
    ui.floating.words = words;
}

/// Whether `float_layers` changed this frame's layers, so that they are
/// drawn again.
fn floated(ui: Res<Ui>) -> bool {
    ui.floating.changed
}

/// The words floating over creatures for a window of `window` pixels,
/// while there are any (`ui::screens::floaty`).
fn floaty_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let Ui {
        assets,
        vm,
        stack,
        hud,
        ..
    } = ui;
    if vm.floaties.is_empty() && vm.overhead.is_empty() {
        return None;
    }
    let mut painter = ui::paint::Painter::new(assets.as_mut()?, stack.palette);
    // The Stamina bar wears the HUD's portrait bar art, in its layout's
    // source.
    let art = hud
        .as_ref()
        .and_then(Option::as_ref)
        .and_then(|(gui, source, _)| {
            use ui::screens::hud::BarArt;
            painter.source = *source;
            Some(ui::screens::floaty::OverheadArt {
                upper: BarArt::vitality(gui, &mut painter)?,
            })
        });
    let mut out = match art {
        Some(art) => ui::screens::floaty::bars(&vm.overhead, art, window),
        None => DrawList {
            size: window,
            ..DrawList::default()
        },
    };
    let words = ui::screens::floaty::draw(&mut painter, &vm.floaties, window);
    out.quads.extend(words.quads);
    Some(out)
}

/// The combat arena's panel of keys for a window of `window` pixels, in the
/// arena (`crate::arena`): in the HUD's frame, under its minimap.
fn arena_panel_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let panel = ui.vm.arena_panel.clone()?;
    let (hud, ..) = hud_gui(ui, window)?;
    let (wide, tall) = hud.frame();
    let at = hud
        .find(ui::screens::arena_panel::UNDER)
        .map_or([8.0, 132.0], |map| {
            let extent = &map.extent;
            [extent.left as f32, (extent.top + extent.height) as f32]
        });
    let Ui { assets, stack, .. } = ui;
    let mut painter = ui::paint::Painter::new(assets.as_mut()?, stack.palette);
    Some(ui::screens::arena_panel::draw(
        &mut painter,
        &panel,
        (wide as f32, tall as f32),
        at,
    ))
}

/// The floors stage's line for a window of `window` pixels
/// (`crate::floor_arena`).
fn arena_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let Ui {
        assets, vm, stack, ..
    } = ui;
    let words = vm.arena.as_ref()?;
    let mut painter = ui::paint::Painter::new(assets.as_mut()?, stack.palette);
    Some(ui::screens::arena_line::line(&mut painter, words, window))
}

/// `window` (logical pixels) in the display's physical pixels, and the
/// text size there. The text size is Aspyr's for the window times the
/// player's text scale (`ui::draw::Sizing::text_size`), read from the
/// sizing the stack's step left.
fn physical_window(ui: &Ui, window: (f32, f32)) -> ((i32, i32), f32) {
    use ui::draw::Textures;
    let wide = (window.0 * ui.scale_factor, window.1 * ui.scale_factor);
    let sizing = ui
        .assets
        .as_ref()
        .map_or(ui::draw::Sizing::default(), |assets| assets.sizing());
    (
        (wide.0.round() as i32, wide.1.round() as i32),
        sizing.text_size(wide),
    )
}

/// The bark bubble's bottom in physical pixels for a window of `window`
/// pixels, while a bark shows and its layout reads (`bark_layer`).
fn bark_bottom(ui: &mut Ui, window: (f32, f32)) -> Option<i32> {
    let factor = ui.scale_factor;
    let Ui {
        assets, bark, vm, ..
    } = ui;
    let said = vm.bark.as_ref()?;
    let assets = assets.as_mut()?;
    let bubble = bark
        .get_or_insert_with(|| ui::screens::bark::Bubble::read(assets))
        .as_ref()?;
    let gui = ui::screens::bark::place(&bubble.gui, assets, bubble.source, said, window)?;
    let (width, height) = gui.frame();
    let (scale, [_, top]) = ui::draw::fill(window, (width as f32, height as f32));
    let bottom = gui
        .controls
        .iter()
        .map(|control| control.extent.top + control.extent.height)
        .max()?;
    Some(((top + bottom as f32 * scale) * factor).round() as i32)
}

/// How far below the top-centre target block the area name stands, in
/// layout pixels of the HUD.
const UNDER_BLOCK: i32 = 4;

/// The bottom of the target block and its action's words in physical
/// pixels for a window of `window` pixels, plus a small gap, while the
/// Interface option "Target block at the top centre" is on. The area name
/// stands under it there, as kjk placed it before, so the two never cover
/// each other.
fn top_block_bottom(ui: &mut Ui, window: (f32, f32)) -> Option<i32> {
    if !target_top(ui) {
        return None;
    }
    let factor = ui.scale_factor;
    let (hud, _, _) = hud_gui(ui, window)?;
    let (width, height) = hud.frame();
    let (scale, [_, top]) = ui::draw::fill(window, (width as f32, height as f32));
    let bottom = ui::screens::hud::block_bottom(&hud) + UNDER_BLOCK;
    Some(((top + bottom as f32 * scale) * factor).round() as i32)
}

/// The area transition panel for a window of `window` pixels, where
/// KOTOR 2 puts it, while the player faces a door or exit that leads out
/// and the layout reads (`ui::screens::transition`). It is placed in the
/// window's physical pixels, as KOTOR 2 places it, and greyed while a
/// member of the party is down.
fn transition_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let words = ui.vm.transition.clone()?;
    let healthy = ui::screens::transition::healthy(ui.vm.party.iter().map(|member| member.hp));
    let (physical, text) = physical_window(ui, window);
    let under = bark_bottom(ui, window).max(top_block_bottom(ui, window));
    let per_pixel = 1.0 / ui.scale_factor.max(f32::EPSILON);
    ui.sized(window, per_pixel, |ui, _| {
        let Ui {
            assets,
            stack,
            transition,
            ..
        } = ui;
        let assets = assets.as_mut()?;
        let panel = transition
            .get_or_insert_with(|| ui::screens::transition::Panel::read(assets))
            .as_ref()?;
        let mut gui = ui::screens::transition::place(&panel.gui, physical, text, under);
        ui::screens::transition::shown(&mut gui, healthy);
        let mut out = DrawList::default();
        let mut painter = ui::paint::Painter::new(assets, stack.palette);
        painter.source = panel.source;
        painter.layout(
            &gui,
            &|control| ui::screens::transition::state(control, &words, panel.words, healthy),
            &mut out,
        );
        Some(out)
    })
}

/// The status summaries for a window of `window` pixels, centred in it as
/// KOTOR 2 centres its summary, while there are any and the layout reads
/// (`ui::screens::status_summary`). They are placed in the window's
/// physical pixels, as KOTOR 2 places them. With the Interface option
/// "Notifications in the corner" on, they stand at the window's right side
/// instead, under the HUD's menu buttons and as wide as them (`corner`).
fn summary_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    ui.summary_ok = None;
    if ui.vm.summaries.is_empty() {
        return None;
    }
    let corner = ui.corner_notices().then(|| corner(ui, window)).flatten();
    let (physical, text) = physical_window(ui, window);
    let per_pixel = 1.0 / ui.scale_factor.max(f32::EPSILON);
    ui.sized(window, per_pixel, |ui, _| {
        let Ui {
            assets,
            vm,
            stack,
            summary,
            ..
        } = ui;
        let assets = assets.as_mut()?;
        let (gui, source) = summary
            .get_or_insert_with(|| {
                assets
                    .screen_layout(ui::screens::status_summary::LAYOUT)
                    .ok()
            })
            .as_ref()?;
        let mut painter = ui::paint::Painter::new(assets, stack.palette);
        let (out, ok) = ui::screens::status_summary::draw(
            &mut painter,
            gui,
            *source,
            &vm.summaries,
            (physical, text),
            corner,
        );
        ui.summary_ok = ok.map(|rect| rect.map(|value| value * per_pixel));
        Some(out)
    })
}

/// Where the notifications in the corner stand, in physical pixels
/// (`summary_layer`), when the HUD's layout reads. They start 12 window
/// pixels under the backing of the HUD's menu buttons in the top right
/// corner, as wide as the backing and lined up with its right edge. With the
/// Feedback option "Hide Quick Menu Buttons" on, or a layout without the
/// backing, they start 12 pixels under the window's top and are as wide as
/// their words.
fn corner(ui: &mut Ui, window: (f32, f32)) -> Option<ui::screens::status_summary::Corner> {
    let factor = ui.scale_factor;
    let hidden = ui::feedback::on(&ui.vm.settings, ui::feedback::HIDE_MENU_BUTTONS);
    let (hud, ..) = hud_gui(ui, window)?;
    let (width, height) = hud.frame();
    let (scale, [left, top]) = fit(window, (width as f32, height as f32));
    let backing = hud
        .find("LBL_MENUBG")
        .filter(|_| !hidden)
        .map(|backing| backing.extent);
    let below = backing.map_or(0.0, |backing| (backing.top + backing.height) as f32);
    let under = backing.map(|backing| {
        let right = left + (backing.left + backing.width) as f32 * scale;
        (right * factor, backing.width as f32 * scale * factor)
    });
    Some(ui::screens::status_summary::Corner {
        top: (top + below * scale + 12.0) * factor,
        under,
    })
}

/// The bark bubble for a window of `window` pixels, while there is a bark
/// and its layout reads (`ui::screens::bark`).
fn bark_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let Ui {
        assets,
        bark,
        vm,
        stack,
        ..
    } = ui;
    let said = vm.bark.as_ref()?;
    let assets = assets.as_mut()?;
    let bubble = bark
        .get_or_insert_with(|| ui::screens::bark::Bubble::read(assets))
        .as_ref()?;
    let gui = ui::screens::bark::place(&bubble.gui, assets, bubble.source, said, window)?;
    let mut out = DrawList {
        fill: true,
        ..DrawList::default()
    };
    let mut painter = ui::paint::Painter::new(assets, stack.palette);
    painter.source = bubble.source;
    painter.layout(
        &gui,
        &|control| ui::screens::bark::state(control, said, bubble.words),
        &mut out,
    );
    Some(out)
}

/// The conversation screen for a window of `window` pixels, while this
/// player is in a cinematic conversation (`ui::screens::dialog`).
fn dialog_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let placed = ui.dialog_placed(window)?;
    let Ui {
        assets,
        dialog,
        vm,
        stack,
        ..
    } = ui;
    let (view, assets) = (vm.dialog.as_ref()?, assets.as_mut()?);
    let source = dialog.as_ref()?.as_ref()?.source;
    let mut out = DrawList {
        fill: true,
        ..DrawList::default()
    };
    let palette = ui::screens::dialog::palette(Some(assets.game), stack.palette);
    let mut painter = ui::paint::Painter::new(assets, palette);
    placed.draw(view, source, &mut painter, &mut out);
    Some(out)
}

/// The host's request card for a window of `window` pixels while a guest
/// asks it in (`ui::screens::dialog::Dialog::card`), whether the host
/// talks (under its top bar) or walks.
fn card_layer(ui: &mut Ui, window: (f32, f32)) -> Option<DrawList> {
    let talking = ui.dialog_placed(window);
    let Ui {
        assets,
        dialog,
        vm,
        stack,
        ..
    } = ui;
    let (requests, assets) = (vm.requests.as_ref()?, assets.as_mut()?);
    let dialog = dialog
        .get_or_insert_with(|| ui::screens::dialog::Dialog::read(assets))
        .as_ref()?;
    let card = dialog.card(assets, requests, window, talking.as_ref())?;
    let mut out = DrawList::default();
    let mut painter = ui::paint::Painter::new(assets, stack.palette);
    card.draw(dialog.source, &mut painter, &mut out);
    Some(out)
}

/// KOTOR 2's status summary waiting for its OK (`progress`) takes this
/// frame's keys and click. The pointer over the OK lights it. A click on
/// the OK closes the summary. So do Enter, Escape, and a pad's A, B, X and
/// Y, as KOTOR 2 closes it on its accept, back and the pad's two other face
/// buttons.
fn summary_input(ui: &mut Ui, input: &mut ui::input::UiInput) {
    use ui::input::Key;
    let over =
        ui.summary_ok
            .zip(input.pointer)
            .is_some_and(|([left, top, width, height], [x, y])| {
                (left..left + width).contains(&x) && (top..top + height).contains(&y)
            });
    let pressed = input
        .keys
        .iter()
        .any(|key| matches!(key, Key::Activate | Key::Back | Key::Primary | Key::Second));
    ui.summary_hover = over;
    ui.summary_closed |= pressed || (input.click && over);
    input.keys.clear();
    input.click = false;
}

/// The keys of play that `step` reads: the screens' keys (`in_game`), and
/// the Hide interface key, which hides the HUD or shows it again. Any open
/// screen (the menu that Escape opens, a page, a store) shows the HUD
/// again, so a player who forgets the key gets it back from the menu.
pub(crate) fn play_keys(
    ui: &mut Ui,
    input: &mut ui::input::UiInput,
    actions: Option<&crate::pad::Actions>,
) {
    let hide = ui.vm.dialog.is_none()
        && actions.is_some_and(|actions| {
            actions.pressed(crate::settings::Action::HideInterface)
                || ui.hotkeys.iter().any(|name| name == HIDE_INTERFACE)
        });
    in_game(ui, input, actions);
    ui.hud_hidden ^= hide;
    ui.hud_hidden &= !ui.stack.is_open();
}

/// The console's name of the Hide interface key (`ui hotkey
/// hide_interface`).
const HIDE_INTERFACE: &str = "hide_interface";

/// The screens' keys in a game: Escape (or gamepad Start) opens the menu
/// at its options; I, J, M, P and K (a pad's Select, the inventory) open
/// their screens, or switch the open tab to theirs, or close theirs, but
/// not during a conversation. The equipment, character and messages pages
/// have keys of their own once the player binds them.
fn in_game(ui: &mut Ui, input: &mut ui::input::UiInput, actions: Option<&crate::pad::Actions>) {
    use ui::input::Key;
    if !ui.stack.is_open() && input.keys.contains(&Key::Back) {
        input.keys.retain(|key| *key != Key::Back);
        ui.stack.open("options_in_game", &ui.vm);
    }
    // A conversation takes the screen, as in KOTOR: no key opens a screen
    // over it, and the map open when one begins closes (its return to the
    // Ebon Hawk). A screen a script opens meanwhile (party selection) stays.
    if ui.vm.dialog.is_some() {
        if ui.stack.top().is_some_and(|top| top.0 == "map") {
            ui.stack.clear();
        }
        return;
    }
    let scripted = std::mem::take(&mut ui.hotkeys);
    let Some(actions) = actions else {
        return;
    };
    let pressed =
        |action, name: &str| actions.pressed(action) || scripted.iter().any(|s| s == name);
    // A datapad's box up: the journal key opens the inventory's datapads
    // with that one chosen, its words beside the list.
    if pressed(crate::settings::Action::Journal, "journal")
        && !ui.stack.is_open()
        && let Some(item) = ui.datapad_up.take()
    {
        ui.stack.open(&format!("inventory:{item}"), &ui.vm);
        ui.datapad_read = Some(item);
        return;
    }
    for (action, name) in HOTKEYS {
        if !pressed(action, name) {
            continue;
        }
        // The action bar setup and the messages are pages under a tab (the
        // abilities' and the journal's), so their keys turn the menu to
        // them as a tab's key does.
        let tab = ui::screens::tabs::under_tabs;
        match ui.stack.top() {
            None => {
                ui.stack.open(name, &ui.vm);
            }
            Some(top) if top.0 == name => ui.stack.clear(),
            Some(top) if tab(top.0) && tab(name) => {
                ui.stack.switch(name, &ui.vm);
            }
            Some(_) => {}
        }
    }
}

/// The screens a key opens in a game.
const HOTKEYS: [(crate::settings::Action, &str); 9] = {
    use crate::settings::Action;
    [
        (Action::Inventory, "inventory"),
        (Action::Equipment, "equip"),
        (Action::Character, "character"),
        (Action::Messages, "messages"),
        (Action::Journal, "journal"),
        (Action::Map, "map"),
        (Action::Party, "party"),
        (Action::Abilities, "abilities"),
        (Action::BarSetup, ActionBar::NAME),
    ]
};

/// The player down closes every screen once (`Stack::fell`), before a
/// script's panel for the fall opens; the whole party down (I30 P13) holds
/// the lost box open on every seat, the host's with Load, until the world
/// is no longer lost.
pub fn fall(
    mut ui: ResMut<Ui>,
    presented: Option<Res<crate::sim_host::PresentedSnapshot>>,
    own: Option<Res<crate::sim_host::OwnSeat>>,
    here: Option<Res<crate::sim_host::WorldHere>>,
) {
    let ui = &mut *ui;
    let (down, lost) = presented
        .zip(own.as_ref())
        .map_or((false, false), |(presented, own)| {
            let snapshot = presented.0.current();
            (dead(snapshot, own.0), snapshot.lost.is_some())
        });
    ui.stack.fell(down || lost);
    // The host is the machine the world runs on, whatever seat its player
    // has: a guest may play the main character (`lost_view_host`).
    ui.vm.lost = lost.then(|| ui::vm::LostView {
        host: lost_view_host(here.as_deref()),
    });
    ui.stack.sync("lost", lost, &ui.vm);
}

/// Whether this machine's lost box offers Load: the world runs here (alone
/// or hosting), not on another player's machine.
pub(crate) fn lost_view_host(here: Option<&crate::sim_host::WorldHere>) -> bool {
    here.is_some()
}

/// `seat`'s body is down or dead (a seat that watches is not).
fn dead(snapshot: &sim::Snapshot, seat: sim::Seat) -> bool {
    snapshot
        .body(seat)
        .and_then(|entity| entity.fighter.as_ref())
        .is_some_and(|fighter| !fighter.alive())
}

/// A quad's corners in the window (its layer's layout placed in `size`,
/// `DrawList::placed`) and its texture coordinates, after its clip and
/// turn; `None` when nothing of it shows.
fn corners(quad: &Quad, layer: &DrawList, size: Vec2) -> Option<[([f32; 2], [f32; 2]); 4]> {
    let [rx, ry, rw, rh] = quad.rect;
    if rw <= 0.0 || rh <= 0.0 {
        return None;
    }
    let [cx, cy, cw, ch] = quad.clip.unwrap_or(quad.rect);
    let (left, top) = (rx.max(cx), ry.max(cy));
    let (right, bottom) = ((rx + rw).min(cx + cw), (ry + rh).min(cy + ch));
    if right <= left || bottom <= top {
        return None;
    }
    let (scale, [ox, oy]) = layer.placed((size.x, size.y));
    // A wide quad's sides are the window's (`DrawList::wide`).
    let wide = layer.wide.contains(&quad.texture);
    let place = |x: f32, y: f32| {
        let across = if wide {
            (x - rx) / rw * size.x
        } else {
            ox + x * scale
        };
        [-size.x / 2.0 + across, size.y / 2.0 - (oy + y * scale)]
    };
    let centre = [rx + rw / 2.0, ry + rh / 2.0];
    let at = |x: f32, y: f32| {
        let (u, v) = texel(quad, (x - rx) / rw, (y - ry) / rh);
        let [x, y] = turn([x, y], centre, quad.spin);
        (place(x, y), [u, v])
    };
    Some([
        at(left, top),
        at(right, top),
        at(right, bottom),
        at(left, bottom),
    ])
}

/// A texture with its mipmaps (each half the last, averaged), so a screen
/// drawn smaller than its layout reads them (`ui.wgsl`). The levels larger
/// than the GPU takes are left out, as the scene's textures are reduced
/// (`scene::fit_to_gpu`).
fn upload(assets: &GuiAssets, id: TextureId, images: &mut Assets<Image>) -> Option<Handle<Image>> {
    let pixels = assets.pixels(id)?;
    let mut mips = mipmaps(pixels.width, pixels.height, &pixels.rgba);
    let (mut width, mut height) = (pixels.width.max(1), pixels.height.max(1));
    let side = crate::scene::texture_side();
    if width.max(height) > side {
        let mut level = 0;
        while (width.max(height) >> level) > side && level + 1 < mips.len() {
            level += 1;
        }
        mips.drain(..level);
        let (from_width, from_height) = (width, height);
        (width, height) = ((width >> level).max(1), (height >> level).max(1));
        warn!(
            "a screen's texture ({from_width}x{from_height}) is larger than this GPU takes \
             ({side} texels a side); it is drawn at {width}x{height}"
        );
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = mips.len() as u32;
    image.data = Some(mips.concat());
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        ..ImageSamplerDescriptor::linear()
    });
    Some(images.add(image))
}

/// `rgba` (`width` × `height`) and each level below it down to one texel
/// across, each texel the average of the two by two above it.
fn mipmaps(width: u32, height: u32, rgba: &[u8]) -> Vec<Vec<u8>> {
    let mut levels = vec![rgba.to_vec()];
    let (mut w, mut h) = (width.max(1) as usize, height.max(1) as usize);
    while w > 1 || h > 1 {
        let above = levels.last().expect("the first level");
        levels.push(asset_odyssey::texture::halved(above, w, h));
        (w, h) = ((w / 2).max(1), (h / 2).max(1));
    }
    levels
}

/// Advances a pending main-menu campaign switch once the target 3D scene is
/// ready, requesting a GPU snapshot of `screens` on this frame so the next
/// frame can swap cleanly and run the transition wipe.
fn sync_switch(
    mut switch: ResMut<CampaignSwitch>,
    mut ui: ResMut<Ui>,
    mut drawn: ResMut<Drawn>,
    scenes: Res<crate::gui_scene::GuiScenes>,
    mut composites: Query<&mut UiComposite, With<UiCamera>>,
) {
    if let Some(target) = switch.waiting {
        if drawn.camera.is_none() {
            switch.waiting = None;
            crate::runtime::set_campaign(&mut ui, target);
        } else if scenes.main_menu_ready(target) {
            switch.waiting = None;
            switch.captured = Some(target);
            drawn.fresh = REDRAWN;
        }
    }
    let Some(camera) = drawn.camera else {
        return;
    };
    let Ok(mut composite) = composites.get_mut(camera) else {
        return;
    };
    composite.capture = switch.captured.is_some();
    if let Some(active) = &switch.active {
        composite.progress = (active.elapsed / active.duration).clamp(0.0, 1.0);
        composite.direction = active.direction;
        composite.tint = active.tint;
    } else {
        composite.progress = -1.0;
    }
}

/// The screens' camera draws their image only while it changes (`fresh`)
/// or shows a 3D view; the rest of the time the image keeps what it shows,
/// and the frame pays for no camera to draw it again. It also draws while a
/// pipeline warm-up is under way, which compiles its pipelines through it.
fn rest_screens(
    mut drawn: ResMut<Drawn>,
    mut cameras: Query<&mut Camera, (With<ScreensCamera>, Without<UiCamera>)>,
    warmup: Option<Res<crate::warmup::Warmup>>,
) {
    let draws = drawn.fresh > 0 || drawn.live || warmup.is_some();
    drawn.fresh = drawn.fresh.saturating_sub(1);
    let Some((screens, ..)) = drawn.screens else {
        return;
    };
    if let Ok(mut camera) = cameras.get_mut(screens)
        && camera.is_active != draws
    {
        camera.is_active = draws;
    }
}

/// Whether a window `pixels` large is minimized. A minimized window is 0 by
/// 0 pixels on Windows.
pub(crate) fn minimized(pixels: UVec2) -> bool {
    pixels.cmpeq(UVec2::ZERO).any()
}

/// The size the screens' image is made again at, for a window `pixels`
/// large and an image `image` large, or none when the image is kept. A
/// minimized window keeps the image it had. An image 0 pixels wide cannot
/// be made, and the screens' projection would divide by 0. The image is not
/// made again on every frame while the window stays minimized either.
pub(crate) fn screens_image(image: UVec2, pixels: UVec2) -> Option<UVec2> {
    (!minimized(pixels) && image != pixels).then_some(pixels)
}

#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    ui: Res<Ui>,
    mut drawn: ResMut<Drawn>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<UiMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scenes: ResMut<crate::gui_scene::GuiScenes>,
    scene: Query<Option<&RenderTarget>, (With<Camera>, Without<OverlayCamera>)>,
    own: Query<&Camera, With<UiCamera>>,
    mut projections: Query<&mut Projection, With<ScreensCamera>>,
    mut visibility: Query<&mut Visibility>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let drawn = &mut *drawn;
    if facts.wanted() {
        facts.0.insert(
            "ui_drawn".into(),
            serde_json::json!({
                "camera": drawn.camera.is_some(),
                "size": [drawn.size.x, drawn.size.y],
                "runs": drawn.runs.len(),
                "images": drawn.images.len(),
            }),
        );
    }
    if drawn.camera.is_none() {
        let Some(target) = scene.iter().next() else {
            return;
        };
        let mut target_image = Image::new_target_texture(1, 1, TextureFormat::Rgba8Unorm, None);
        target_image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let image = images.add(target_image);
        let mut prev_image = Image::new_target_texture(1, 1, TextureFormat::Rgba8Unorm, None);
        prev_image.texture_descriptor.usage |= TextureUsages::COPY_DST;
        let previous = images.add(prev_image);
        let screens = commands
            .spawn((
                Camera3d::default(),
                Camera {
                    order: 12,
                    clear_color: ClearColorConfig::Custom(Color::NONE),
                    ..default()
                },
                RenderTarget::Image(image.clone().into()),
                Msaa::Off,
                Projection::Orthographic(OrthographicProjection::default_3d()),
                Tonemapping::None,
                DebandDither::Disabled,
                Transform::from_xyz(0.0, 0.0, 100.0),
                RenderLayers::layer(LAYER),
                OverlayCamera,
                ScreensCamera,
            ))
            .id();
        // It draws the HUD too, under the screens (`hud`): the HUD's view,
        // its layers given it while there is one.
        let mut camera = commands.spawn((
            Camera3d::default(),
            crate::scene::over_world(13),
            crate::hud::overlay_view(),
            crate::scene::encoded_view(),
            RenderLayers::none(),
            OverlayCamera,
            UiCamera,
            UiComposite::new(image.clone(), previous.clone()),
        ));
        if let Some(target) = target {
            camera.insert(target.clone());
        }
        drawn.camera = Some(camera.id());
        drawn.screens = Some((screens, image, previous));
        drawn.fresh = REDRAWN;
        return;
    }
    let Some((size, pixels)) = own.iter().next().and_then(|camera| {
        Some((
            camera.logical_viewport_size()?,
            camera.physical_viewport_size()?,
        ))
    }) else {
        return;
    };
    // The screens' image the window's pixels, laid out in its points.
    if let Some((screens, image, previous)) = &drawn.screens {
        if let Some(mut image) = images.get_mut(image)
            && let Some(pixels) = screens_image(image.size(), pixels)
        {
            image.resize(Extent3d {
                width: pixels.x,
                height: pixels.y,
                depth_or_array_layers: 1,
            });
            drawn.fresh = REDRAWN;
        }
        if let Some(mut prev) = images.get_mut(previous)
            && let Some(pixels) = screens_image(prev.size(), pixels)
        {
            prev.resize(Extent3d {
                width: pixels.x,
                height: pixels.y,
                depth_or_array_layers: 1,
            });
        }
        if !minimized(pixels)
            && let Ok(mut projection) = projections.get_mut(*screens)
            && let Projection::Orthographic(ortho) = &mut *projection
            && !matches!(ortho.scaling_mode,
                ScalingMode::Fixed { width, height } if (width, height) == (size.x, size.y))
        {
            ortho.scaling_mode = ScalingMode::Fixed {
                width: size.x,
                height: size.y,
            };
        }
    }
    // The textures the assets let go, let go here too.
    if let Some(assets) = &ui.assets
        && assets.released() != drawn.released
    {
        drawn.released = assets.released();
        let kept = |id: &TextureId| {
            *id == TextureId::WHITE
                || assets.scene_of(*id).is_some()
                || assets.pixels(*id).is_some()
        };
        drawn.images.retain(|id, _| kept(id));
        drawn.materials.retain(|(id, ..), _| kept(id));
        drawn.fresh = REDRAWN;
    }
    // A minimized window keeps the screens' meshes made for the last size
    // it had, rather than laying them out 0 points wide.
    if minimized(pixels) || (drawn.layers == ui.layers && drawn.size == size) {
        return;
    }
    drawn.fresh = REDRAWN;
    drawn.layers = ui.layers.clone();
    drawn.size = size;
    let Some(assets) = &ui.assets else {
        return;
    };
    let mut runs: Vec<Run> = Vec::new();
    for layer in &ui.layers {
        for quad in &layer.quads {
            let Some(corners) = corners(quad, layer, size) else {
                continue;
            };
            let key = (quad.texture, quad.additive, quad.grey);
            if runs.last().is_none_or(|run| run.key != key) {
                runs.push(Run {
                    key,
                    positions: Vec::new(),
                    uvs: Vec::new(),
                    colors: Vec::new(),
                });
            }
            let z = runs.len() as f32 * 0.01;
            let color = quad.color;
            let run = runs.last_mut().expect("pushed above");
            for (position, uv) in corners {
                run.positions.push([position[0], position[1], z]);
                run.uvs.push(uv);
                run.colors.push(color);
            }
        }
    }
    let count = runs.len();
    drawn.live = runs.iter().any(|run| assets.scene_of(run.key.0).is_some());
    for (at, run) in runs.into_iter().enumerate() {
        let Run {
            key,
            positions,
            uvs,
            colors,
        } = run;
        let geometry = (positions, uvs, colors);
        if !drawn.materials.contains_key(&key) {
            let scene = assets.scene_of(key.0).is_some();
            let texture = if key.0 == TextureId::WHITE {
                None
            } else if scene {
                Some(scenes.image(key.0, &mut images))
            } else {
                if !drawn.images.contains_key(&key.0)
                    && let Some(image) = upload(assets, key.0, &mut images)
                {
                    drawn.images.insert(key.0, image);
                }
                drawn.images.get(&key.0).cloned()
            };
            let pixels = (key.0 != TextureId::WHITE)
                .then(|| assets.pixels(key.0))
                .flatten();
            let material = materials.add(UiMaterial {
                params: UiParams {
                    size: pixels.map_or(Vec2::ONE, |pixels| {
                        Vec2::new(pixels.width as f32, pixels.height as f32)
                    }),
                    shape: u32::from(pixels.is_some_and(|pixels| pixels.shape)),
                    view: u32::from(scene),
                    grey: u32::from(key.2),
                },
                texture,
                additive: key.1,
            });
            drawn.materials.insert(key, material);
        }
        let material = drawn.materials[&key].clone();
        match drawn.runs.get_mut(at) {
            Some(run) => {
                if run.geometry != geometry
                    && let Some(mut slot) = meshes.get_mut(&run.mesh)
                {
                    *slot = run_mesh(&geometry);
                    run.geometry = geometry;
                }
                if run.key != key {
                    commands.entity(run.entity).insert(MeshMaterial3d(material));
                    run.key = key;
                }
                if !run.shown
                    && let Ok(mut shown) = visibility.get_mut(run.entity)
                {
                    *shown = Visibility::Inherited;
                    run.shown = true;
                }
            }
            None => {
                let handle = meshes.add(run_mesh(&geometry));
                let entity = commands
                    .spawn((
                        Mesh3d(handle.clone()),
                        MeshMaterial3d(material),
                        Transform::IDENTITY,
                        RenderLayers::layer(LAYER),
                        // Only the screens' camera draws this layer, so culling would save nothing,
                        // and Bevy would work the box out again each time the text changes.
                        NoFrustumCulling,
                    ))
                    .id();
                drawn.runs.push(Shown {
                    entity,
                    mesh: handle,
                    geometry,
                    key,
                    shown: true,
                });
            }
        }
    }
    for run in drawn.runs.iter_mut().skip(count).filter(|run| run.shown) {
        if let Ok(mut shown) = visibility.get_mut(run.entity) {
            *shown = Visibility::Hidden;
            run.shown = false;
        }
    }
}

/// The probes of the Hide interface key and of the menu's page keys (`kjk
/// sim probes`, group "hide interface").
#[cfg(feature = "probes")]
pub fn hide_probes() -> sim::probe::Checks {
    use crate::settings::Action;
    use ui::input::Key;
    let fresh = || {
        Ui::new(
            Stack::new(ui::screens::open),
            None,
            ViewModel::default(),
            true,
        )
    };
    // One frame of play's keys, with `pressed` down and the screens' `keys`
    // pressed, and whether the HUD is hidden after it.
    let frame = |ui: &mut Ui, pressed: &[Action], keys: &[Key]| {
        let mut actions = crate::pad::Actions::default();
        for action in pressed {
            actions.set_pressed(*action);
        }
        let mut input = ui::input::UiInput {
            keys: keys.to_vec(),
            ..ui::input::UiInput::default()
        };
        play_keys(ui, &mut input, Some(&actions));
        ui.hud_hidden
    };
    let top = |ui: &Ui| ui.stack.top().map(|screen| screen.0.to_string());
    let hide = [Action::HideInterface];
    vec![
        ("the key hides the HUD and shows it again", {
            let mut ui = fresh();
            let steps = [
                frame(&mut ui, &hide, &[]),
                frame(&mut ui, &[], &[]),
                frame(&mut ui, &hide, &[]),
            ];
            sim::probe::check(
                steps == [true, true, false],
                format!("hidden after a press, a frame, a press: {steps:?}"),
            )
        }),
        ("the Escape menu shows the HUD again", {
            let mut ui = fresh();
            frame(&mut ui, &hide, &[]);
            let in_menu = frame(&mut ui, &[], &[Key::Back]);
            let menu = top(&ui);
            ui.stack.clear();
            let after = frame(&mut ui, &[], &[]);
            sim::probe::check(
                !in_menu && !after && menu.as_deref() == Some("options_in_game"),
                format!("menu {menu:?}, hidden in it {in_menu}, after it {after}"),
            )
        }),
        ("a page key opens its page and shows the HUD again", {
            let mut ui = fresh();
            frame(&mut ui, &hide, &[]);
            let hidden = frame(&mut ui, &[Action::Equipment], &[]);
            let equip = top(&ui);
            frame(&mut ui, &[Action::Character], &[]);
            let character = top(&ui);
            frame(&mut ui, &[Action::Messages], &[]);
            let messages = top(&ui);
            frame(&mut ui, &[Action::Messages], &[]);
            let closed = top(&ui);
            let pages = [equip, character, messages];
            sim::probe::check(
                !hidden
                    && pages
                        == [Some("equip"), Some("character"), Some("messages")]
                            .map(|page| page.map(str::to_string))
                    && closed.is_none(),
                format!("hidden {hidden}, pages {pages:?}, the key again {closed:?}"),
            )
        }),
        ("a conversation keeps the key from hiding the HUD", {
            let mut ui = fresh();
            ui.vm.dialog = Some(ui::vm::DialogView::default());
            let hidden = frame(&mut ui, &hide, &[]);
            sim::probe::check(!hidden, format!("hidden in a conversation {hidden}"))
        }),
    ]
}
