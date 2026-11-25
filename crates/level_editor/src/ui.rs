//! The editor's window, laid out as a scene editor's: a toolbar along the
//! top, the outliner on the left, the selection's details on the right, the
//! asset picker along the bottom, and the area's view in the middle with
//! labels and a gizmo. Everything here turns clicks, drags and keys into
//! the editor's edits and `edit` commands (`state`).

use std::collections::BTreeMap;

use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiGlobalSettings, EguiPlugin, EguiPrimaryContextPass, PrimaryEguiContext, egui,
};
use console::{CameraPose, CameraRequest, ConsoleQueue};
use fmt_odyssey::ResType;
use level_edit::editor::EDITABLE;
use level_edit::{BoxRole, Change, CollisionBox, Edit, Kind, Prop, Target};
use transform_gizmo_egui::math::Transform as GizmoTransform;
use transform_gizmo_egui::{
    EnumSet, Gizmo, GizmoConfig, GizmoExt, GizmoMode, GizmoOrientation, GizmoResult, mint,
};

use kjk_game::scene::{FlyCamera, FlyControls, game_to_bevy};

use crate::state::{self, EditorState, STEP_M, TURN_DEG};
use crate::thumbs::{self, AssetKind, Thumbnails};

/// How many labels the view shows at once: those of the nearest objects.
const LABELS: usize = 24;
/// The asset picker's tiles: the thumbnail's side, in points.
const TILE: f32 = 96.0;
/// How long a frame may spend drawing thumbnails, milliseconds.
const THUMBNAIL_BUDGET_MS: f64 = 12.0;

/// The tool the gizmo shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
    Rotate,
    Scale,
    /// Two clicks in the view measure between the places clicked.
    Measure,
}

/// The asset picker's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Lights,
    Lamps,
    Effects,
    Rooms,
    Placeables,
    Creatures,
    Doors,
    Props,
    Shapes,
    Waypoints,
    Triggers,
    Sounds,
    Stores,
    Encounters,
}

/// Boxes ready to place: a name, a size in metres and a role.
const SHAPES: [(&str, [f32; 3], BoxRole); 7] = [
    ("Ledge", [2.0, 1.0, 0.3], BoxRole::Solid),
    ("Platform", [4.0, 4.0, 0.3], BoxRole::Walkable),
    ("Wall", [3.0, 0.2, 3.0], BoxRole::Solid),
    ("Pillar", [1.0, 1.0, 3.0], BoxRole::Solid),
    ("Step", [1.0, 1.0, 0.5], BoxRole::Walkable),
    ("Nav blocker", [2.0, 2.0, 2.0], BoxRole::NavBlock),
    ("Nav floor", [2.0, 2.0, 0.1], BoxRole::NavFloor),
];

/// The Lights tab's ready lights: a shape (`state::light_shape`) and its
/// name.
const LIGHT_SHAPES: [(&str, &str); 4] = [
    ("point", "Lamp, all ways"),
    ("spot", "Spot, straight down"),
    ("panel", "Panel, one way"),
    ("strip", "Strip, 2 m"),
];

/// A role's name in the panel.
fn role_label(role: BoxRole) -> &'static str {
    match role {
        BoxRole::Solid => "Solid",
        BoxRole::Walkable => "Solid, walked on",
        BoxRole::NavBlock => "Nav blocker",
        BoxRole::NavFloor => "Nav floor",
    }
}

/// What the picker hands over: an asset by kind and name, or a shape.
#[derive(Debug, Clone, PartialEq)]
enum Asset {
    Template(AssetKind, String),
    Shape(usize),
    /// An object with no model of its own: a waypoint, a trigger, a sound
    /// or a store, by its template.
    Object(Kind, String),
    /// A light placed by hand of a ready shape (`state::light_shape`).
    Light(&'static str),
    /// A Jedi Academy ambient effect, by its file's path.
    Effect(String),
}

/// The outliner's rows of the area's objects: each kind with the place and
/// the text of each of its objects the search keeps.
type ObjectRows = std::sync::Arc<Vec<(Kind, Vec<(usize, String)>)>>;

/// The asset picker's items: each asset with the name its tile shows.
type AssetItems = std::sync::Arc<Vec<(Asset, String)>>;

/// The window's own state, apart from the project's.
#[derive(Resource)]
pub struct UiState {
    pub tool: Tool,
    /// The File menu's and the prompt's state (`file`).
    pub(crate) file: crate::file::FileState,
    snapping: bool,
    snap_distance: f32,
    /// The places the measuring tool was clicked on: one, or the two it
    /// measures between.
    measure: Vec<[f32; 3]>,
    snap_angle: f32,
    outliner_search: String,
    /// The outliner's rows of the area's objects, kind by kind, as they
    /// were made for a count of the editor's changes and a search. They
    /// are made again only when one of the two changes.
    outliner_rows: Option<(u64, String, ObjectRows)>,
    /// The asset picker's items as they were made for a tab, a search and
    /// a count of the lamps found.
    asset_items: Option<((Tab, String, usize), AssetItems)>,
    tab: Tab,
    asset_search: String,
    /// Whether the asset picker also lists the templates only the game's
    /// other levels have (`elsewhere`).
    assets_elsewhere: bool,
    /// The asset picked in the picker: what a double-click or Enter places.
    picked: Option<Asset>,
    gizmo: Gizmo,
    /// The modes the gizmo was made for.
    gizmo_modes: Option<EnumSet<GizmoMode>>,
    /// A drag (of the gizmo or of a number) is going on: its edits join the
    /// first one's undo step.
    dragging: bool,
    thumbnails: Thumbnails,
    textures: BTreeMap<(AssetKind, String), egui::TextureHandle>,
    store: Option<store::Store>,
    /// The view's part of the window, in points.
    viewport: egui::Rect,
    /// The markers' icons, made the first time the view is shown.
    icons: Option<crate::markers::Icons>,
    fonts_set: bool,
    /// The behaviour section's text fields: each event's script, and the
    /// item to add, as typed, for the selection they were typed for.
    typed: BTreeMap<String, String>,
    typed_for: Option<Target>,
    /// The editor's count of changes when the fields were filled.
    typed_at: u64,
    /// The script the viewer shows, its tab, and the scripts read so far.
    viewing: Option<String>,
    view_tab: ViewTab,
    /// The name typed for a new script in the Scripts menu.
    new_script: String,
    /// The module whose doors and triggers Ways in lists, and the door or
    /// waypoint of this level they are led to.
    way_in_module: Option<String>,
    way_in_arrival: String,
    /// The area's and the module's script fields as typed, by label.
    area_typed: BTreeMap<String, String>,
    /// The event whose logic blocks are shown, for the selection.
    blocks_event: Option<(Target, String)>,
    /// The conversation window: the conversation open, its node selected
    /// with the link that led to it, the field being typed in, the name
    /// typed for a new one, and the tag and picks of a try.
    dialog_open: Option<String>,
    /// The conversation open as it was read (`level_edit::dialog::read`),
    /// for a count of the editor's changes and its name.
    dialog_read: Option<(u64, String, std::sync::Arc<fmt_odyssey::dlg::Dlg>)>,
    dialog_selected: Option<(
        level_edit::dialog::Side,
        usize,
        Option<level_edit::dialog::LinkAt>,
    )>,
    dialog_typing: Option<String>,
    new_dialog: String,
    dialog_try: (String, String),
    /// What each condition script checks (`condition_checks`), read once.
    talk_checks: BTreeMap<String, String>,
    /// The rows of each 2DA a picker lists (`table_rows`), read once.
    table_rows: BTreeMap<String, std::sync::Arc<Vec<(u16, String)>>>,
    /// What each row picker's filter holds, by the picker's name.
    row_filters: BTreeMap<String, String>,
    /// Whether the window of the project's items and quests is open.
    adventure_open: bool,
    /// What adds each quest entry, as that window shows it under the entry.
    quest_adders: crate::quest_links::Kept,
    /// What the fields of that window hold while they are typed in, by
    /// each field's name.
    texts: BTreeMap<String, String>,
    /// The property being chosen for each item of that window: its kind,
    /// its subtype, its value and its parameter.
    new_property: BTreeMap<String, (Option<u16>, u16, u16, u16)>,
    /// A number being dragged, by its field's name, with its value so far:
    /// it is made one change when the drag ends.
    number_drag: Option<(String, f64)>,
    /// The encounter whose spawn points are placed by clicks on the floor.
    spawning: Option<Target>,
    /// Where a drag that selects everything in its box began.
    boxing: Option<egui::Pos2>,
    /// A link between floors being placed: its kind, and where it leaves
    /// once the first click has set it.
    placing_link: Option<(collision::nav::LinkKind, Option<[f32; 3]>)>,
    /// The agent link while it is on (`agent`), and a copy of the
    /// selection asked for.
    link: Option<crate::agent::Link>,
    copy_asked: bool,
    /// The breakpoints by script, as line numbers, and the stop of the
    /// last debugged run shown.
    breakpoints: BTreeMap<String, std::collections::BTreeSet<u32>>,
    debug_at: usize,
    /// An outline being drawn by clicks on the floor: the trigger or
    /// encounter, and the points so far.
    drawing: Option<(Target, Vec<[f32; 3]>)>,
    /// The point of the selected outline being dragged.
    dragging_point: Option<usize>,
    /// The viewer is in a window of its own, not under the details.
    viewer_window: bool,
    read_scripts: BTreeMap<String, crate::script_view::ScriptView>,
    /// The editor's `script_round` the scripts above were read at.
    read_round: u32,
    /// The Keys window (`keys`).
    pub(crate) keys_window: crate::keys::Window,
    /// The conversation window's line text takes the keys on the next
    /// frame: a line was just added, to be written.
    dialog_focus: bool,
    /// A free move of the move tool's gizmo: how far the selection is from
    /// the camera, as a share of how far it was when the drag began. The
    /// mouse wheel changes it.
    free_move: Option<f64>,
}

impl UiState {
    /// Whether the agent link is on.
    pub(crate) fn link_on(&self) -> bool {
        self.link.is_some()
    }
}

/// The script viewer's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewTab {
    Source,
    Summary,
    Listing,
    /// What the last try of the script said (`script try`).
    Try,
    /// The last debugged run of the script, stop by stop (`script debug`).
    Debug,
}

impl Default for UiState {
    fn default() -> UiState {
        UiState {
            tool: Tool::Move,
            file: crate::file::FileState::default(),
            snapping: false,
            snap_distance: STEP_M,
            measure: Vec::new(),
            snap_angle: TURN_DEG,
            outliner_search: String::new(),
            outliner_rows: None,
            asset_items: None,
            tab: Tab::Placeables,
            asset_search: String::new(),
            assets_elsewhere: false,
            picked: None,
            gizmo: Gizmo::default(),
            gizmo_modes: None,
            dragging: false,
            thumbnails: Thumbnails::default(),
            textures: BTreeMap::new(),
            store: store::Store::open(),
            viewport: egui::Rect::NOTHING,
            icons: None,
            fonts_set: false,
            typed: BTreeMap::new(),
            typed_for: None,
            typed_at: 0,
            viewing: None,
            view_tab: ViewTab::Source,
            new_script: String::new(),
            way_in_module: None,
            way_in_arrival: String::new(),
            area_typed: BTreeMap::new(),
            blocks_event: None,
            dialog_open: None,
            dialog_read: None,
            dialog_selected: None,
            dialog_typing: None,
            new_dialog: String::new(),
            dialog_try: (String::new(), String::new()),
            talk_checks: BTreeMap::new(),
            table_rows: BTreeMap::new(),
            row_filters: BTreeMap::new(),
            adventure_open: false,
            quest_adders: crate::quest_links::Kept::default(),
            texts: BTreeMap::new(),
            new_property: BTreeMap::new(),
            number_drag: None,
            spawning: None,
            placing_link: None,
            boxing: None,
            link: std::env::var(crate::agent::VAR)
                // `1` turns it on. Any other name turns it on with a
                // folder of that name (`agent::Link::folder`).
                .is_ok_and(|on| !on.is_empty() && on != "0")
                .then(crate::agent::Link::open)
                .and_then(Result::ok),
            copy_asked: false,
            breakpoints: BTreeMap::new(),
            debug_at: 0,
            drawing: None,
            dragging_point: None,
            viewer_window: false,
            read_scripts: BTreeMap::new(),
            read_round: 0,
            keys_window: crate::keys::Window::default(),
            dialog_focus: false,
            free_move: None,
        }
    }
}

/// Adds the editor to the game's app: its state, egui, and the systems.
pub fn install(app: &mut App, state: EditorState, headless: bool) {
    app.insert_resource(state)
        .insert_resource(crate::file::Headless(headless))
        // Enhanced lighting: the scene's camera is HDR, whose texture egui
        // draws on (`attach_egui`).
        .insert_resource(kjk_game::rolloff::LightingMode::Enhanced)
        .insert_resource(FlyControls {
            keys_need_look: true,
            paused: false,
            keys: None,
        })
        .insert_resource(EguiGlobalSettings {
            auto_create_primary_context: false,
            ..default()
        })
        .add_plugins(EguiPlugin::default())
        .init_resource::<UiState>()
        .add_systems(Startup, egui_writes_gamma)
        .add_systems(PreUpdate, state::redraw)
        .add_systems(
            Update,
            (
                attach_egui,
                state::commands_from_console.after(ConsoleQueue),
                pick_pending,
                state::follow,
                crate::listen::hold,
                make_thumbnails,
                state::publish,
                take_shot,
            )
                .chain(),
        )
        .add_systems(EguiPrimaryContextPass, panels)
        .add_systems(Update, crate::file::close_requests)
        .add_systems(Last, crate::file::leave);
}

/// egui draws on the scene's camera. The scene's cameras keep stored,
/// gamma-encoded values (`kjk_game::scene`, `encoded_view`): under Classic
/// lighting in an `Rgba8Unorm` texture, which egui's pipeline does not
/// take, and under Enhanced lighting (which the editor uses, `install`) in
/// an HDR `Rgba16Float` one, which it does. A camera of egui's own over the
/// scene kept the strokes of earlier frames on its texture.
pub(crate) fn attach_egui(
    mut commands: Commands,
    cameras: Query<(Entity, Has<PrimaryEguiContext>), With<FlyCamera>>,
) {
    for (camera, attached) in &cameras {
        if !attached {
            commands.entity(camera).insert(PrimaryEguiContext);
        }
    }
}

/// egui's shader as it writes onto the scene's texture: its colour as
/// stored, gamma-encoded, as every camera on the world's target writes
/// (`kjk_game::scene`, `encoded_view`). bevy_egui's own shader turns the
/// colour to linear light last, for a texture that keeps linear values,
/// which left the panels too dark.
pub(crate) fn egui_writes_gamma(mut shaders: ResMut<Assets<bevy::shader::Shader>>) {
    const LINEAR: &str = "return vec4<f32>(linear_from_gamma_rgb(color_gamma.rgb), color_gamma.a);";
    let Some(mut shader) = shaders.get_mut(&bevy_egui::render::EGUI_SHADER_HANDLE) else {
        warn!(target: "editor", "egui's shader is not loaded: the panels' colours stay linear");
        return;
    };
    let bevy::shader::Source::Wgsl(source) = &shader.source else {
        return;
    };
    if !source.contains(LINEAR) {
        warn!(target: "editor", "egui's shader changed: the panels' colours stay linear");
        return;
    }
    let source = source.replace(LINEAR, "return color_gamma;");
    let path = shader.path.clone();
    *shader = bevy::shader::Shader::from_wgsl(source, path);
}

/// How many frames the panels stay out after a picture of the view alone
/// is taken (`take_shot`).
pub(crate) const BARE_AFTER: u8 = 2;

/// The picture of the window the agent link asked for (`shot`), written to
/// its file as a PNG once the next frame is drawn. A run without a window
/// has none to picture.
fn take_shot(
    mut commands: Commands,
    mut state: ResMut<EditorState>,
    headless: Res<crate::file::Headless>,
) {
    // A picture of the view alone: the panels are left out for a few
    // frames (`panels`), the picture is taken in the middle of them, and
    // they come back once it is taken. The frame the picture is asked in
    // is the one drawn next, so the panels stay out past it.
    if let Some((path, frames)) = state.pending_bare.take() {
        if headless.0 {
            state.pending_shot = Some(path);
        } else {
            if frames == BARE_AFTER {
                commands
                    .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
                    .observe(bevy::render::view::screenshot::save_to_disk(path.clone()));
            }
            if frames > 0 {
                state.pending_bare = Some((path, frames - 1));
            }
            return;
        }
    }
    let Some(path) = state.pending_shot.take() else {
        return;
    };
    if headless.0 {
        state.say("shot: this run has no window to picture (--cmds has screenshot)");
        return;
    }
    commands
        .spawn(bevy::render::view::screenshot::Screenshot::primary_window())
        .observe(bevy::render::view::screenshot::save_to_disk(path));
}

/// A click `edit pick` asked for, or the view's, at a point of the window.
fn pick_pending(
    mut state: ResMut<EditorState>,
    scene: Option<Res<kjk_game::scene::SceneContent>>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let Some(scene) = scene else { return };
    let state = &mut *state;
    let Some(point) = state.pending_pick.take() else {
        return;
    };
    // A marker under the point is picked first, as the view shows it.
    if let Some((camera, eye)) = cameras.iter().next() {
        let shown = crate::markers::layout(state, camera, eye);
        if let Some(target) = crate::markers::under(&shown, egui::pos2(point.x, point.y)) {
            state.editor.select(Some(target), state.pending_pick_adds);
            state.say(format!("picked {target:?}"));
            return;
        }
    }
    match state::ray_at(&cameras, point) {
        Some(ray) => {
            let floor = state::ground(state, ray);
            let picked = state::pick(state, &scene.0, ray, floor);
            state.editor.select(picked, state.pending_pick_adds);
            let (origin, direction) = (ray.0.to_array(), ray.1.to_array());
            match state.editor.selected {
                Some(target) => state.say(format!("picked {target:?}")),
                None => state.say(format!(
                    "nothing under {point}: the ray from {origin:.2?} along {direction:.2?} meets nothing"
                )),
            }
        }
        None => state.say(format!("no camera ray through {point}")),
    }
}

/// Up to a frame's budget of the thumbnails the picker asked for, from the
/// store or drawn.
fn make_thumbnails(mut state: ResMut<EditorState>, mut ui: ResMut<UiState>) {
    let started = std::time::Instant::now();
    let state = &mut *state;
    let ui = &mut *ui;
    while started.elapsed().as_secs_f64() * 1000.0 < THUMBNAIL_BUDGET_MS {
        let Some((kind, name)) = ui.thumbnails.next_wanted() else {
            break;
        };
        let picture = thumbnail(state, ui.store.as_ref(), kind, &name);
        ui.thumbnails.made.insert((kind, name), picture);
    }
}

/// One asset's thumbnail: the store's, else drawn and kept there.
fn thumbnail(
    state: &mut EditorState,
    store: Option<&store::Store>,
    kind: AssetKind,
    name: &str,
) -> Option<thumbs::Picture> {
    let resolver = &state.setup.resolver;
    let source = match kind {
        AssetKind::Placeable => resolver.read(name, fmt_odyssey::ResType::UTP)?,
        AssetKind::Creature => resolver.read(name, fmt_odyssey::ResType::UTC)?,
        AssetKind::Door => resolver.read(name, fmt_odyssey::ResType::UTD)?,
        AssetKind::Prop => level_edit::active::ja_read(state.setup.ja.as_ref()?, name).ok()?,
        AssetKind::Room => resolver.read(name, fmt_odyssey::ResType::MDL)?,
        AssetKind::Lamp => {
            let (model, _) = crate::lamps::LampAsset::of_key(name);
            resolver.read(&model, fmt_odyssey::ResType::MDL)?
        }
    };
    let key = thumbs::key(state.setup.game.key(), kind, name, &source);
    if let Some(bytes) = store.and_then(|store| store.get(thumbs::STORE_KIND, &key))
        && bytes.len() == thumbs::SIDE * thumbs::SIDE * 4
    {
        return Some(bytes);
    }
    let picture = match kind {
        AssetKind::Placeable => {
            let model = thumbs::placeable_model(resolver, name)?;
            let ir = kjk_game::viewer::model(resolver, &model).ok()??;
            thumbs::of_models(resolver, &[kjk_game::viewer::Placed::at(ir, [0.0; 3])])
        }
        AssetKind::Door => {
            let model = thumbs::door_model(resolver, name)?;
            let ir = kjk_game::viewer::model(resolver, &model).ok()??;
            thumbs::of_models(resolver, &[kjk_game::viewer::Placed::at(ir, [0.0; 3])])
        }
        AssetKind::Creature => {
            let models =
                kjk_game::viewer::creature_models(resolver, state.setup.game, name).ok()?;
            thumbs::of_models(resolver, &models)
        }
        AssetKind::Prop => {
            if !state.prop_models.contains_key(name) {
                let files = kjk_game::viewer_ja::Files::base(state.setup.ja.as_ref()?);
                let read = kjk_game::project_props::read(&files, name);
                state.prop_models.insert(name.to_string(), read);
            }
            thumbs::of_prop(state.prop_models[name].as_ref().ok()?)
        }
        AssetKind::Room => {
            let ir = kjk_game::viewer::model(resolver, name).ok()??;
            thumbs::of_room(resolver, &kjk_game::viewer::Placed::at(ir, [0.0; 3]))
        }
        AssetKind::Lamp => {
            let (model, mesh) = crate::lamps::LampAsset::of_key(name);
            let lamp = level_edit::PlacedLamp::new(&model, &mesh, [0.0; 3]);
            let ir = kjk_game::lamps::model(resolver, &lamp).ok()?;
            thumbs::of_models(resolver, &[kjk_game::viewer::Placed::at(ir, [0.0; 3])])
        }
    }?;
    if let Some(store) = store {
        let _ = store.put(thumbs::STORE_KIND, &key, &picture);
    }
    Some(picture)
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Creature => "Creatures",
        Kind::Door => "Doors",
        Kind::Placeable => "Placeables",
        Kind::Trigger => "Triggers",
        Kind::Waypoint => "Waypoints",
        Kind::Sound => "Sounds",
        Kind::Encounter => "Encounters",
        Kind::Store => "Stores",
    }
}

/// A camera pose that looks at `point` from a few metres away, from where
/// the camera is now.
fn framing(point: Vec3, eye: Vec3) -> CameraPose {
    let away = (eye - point).normalize_or(Vec3::new(-1.0, 0.0, 0.5).normalize());
    let distance = 4.0;
    let position = point + away * distance;
    let look = (point - position).normalize();
    CameraPose {
        position: position.to_array(),
        yaw: look.y.atan2(look.x).to_degrees(),
        pitch: look.z.asin().to_degrees(),
        fov: None,
    }
}

/// A Bevy matrix as the gizmo takes one: rows, in `f64`.
fn rows(matrix: Mat4) -> mint::RowMatrix4<f64> {
    let c = matrix.to_cols_array_2d();
    let row = |r: usize| mint::Vector4 {
        x: f64::from(c[0][r]),
        y: f64::from(c[1][r]),
        z: f64::from(c[2][r]),
        w: f64::from(c[3][r]),
    };
    mint::RowMatrix4 {
        x: row(0),
        y: row(1),
        z: row(2),
        w: row(3),
    }
}

/// How near the middle of the move gizmo (points) a press starts a free
/// move: the gizmo's inner circle, 15 points across its radius, and the
/// margin it picks within.
const FREE_MOVE_REACH: f32 = 22.0;

/// How much farther each step of the mouse wheel takes a free move: 15%.
const FREE_MOVE_STEP: f64 = 1.15;

/// The touchpad's movement (points) that makes one step of the wheel.
const WHEEL_STEP: f32 = 40.0;

fn to_mint(v: bevy::math::DVec3) -> mint::Vector3<f64> {
    mint::Vector3 {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

fn from_mint(v: mint::Vector3<f64>) -> bevy::math::DVec3 {
    bevy::math::DVec3::new(v.x, v.y, v.z)
}

/// A free move of the move tool: a drag of the gizmo's middle, which moves
/// the selection across the view. While it lasts, the mouse wheel takes
/// the selection farther from the camera (forward) or nearer (back), along
/// the line from the camera, so it stays under the pointer. A press near
/// the gizmo's middle starts it. The share of the distance from the camera
/// before this frame's wheel and after it, during a free move.
fn free_move(
    ctx: &egui::Context,
    ui_state: &mut UiState,
    (camera, eye): (&Camera, &GlobalTransform),
    transform: &GizmoTransform,
    eye_at: bevy::math::DVec3,
) -> Option<(f64, f64)> {
    let middle = from_mint(transform.translation);
    if ui_state.tool != Tool::Move {
        ui_state.free_move = None;
        return None;
    }
    if !ui_state.dragging && ctx.input(|input| input.pointer.primary_pressed()) {
        let pointer = ctx.input(|input| input.pointer.interact_pos());
        let on_screen = camera
            .world_to_viewport(eye, game_to_bevy() * middle.as_vec3())
            .ok()
            .map(|at| egui::pos2(at.x, at.y));
        let on_middle = (on_screen.zip(pointer))
            .is_some_and(|(at, pointer)| at.distance(pointer) <= FREE_MOVE_REACH);
        ui_state.free_move = on_middle.then_some(1.0);
    }
    let before = ui_state.free_move?;
    // The wheel's steps this frame: a mouse's notches, or a touchpad's
    // movement in points.
    let steps = ctx.input_mut(|input| {
        let steps: f32 = (input.events.iter())
            .map(|event| match event {
                egui::Event::MouseWheel { unit, delta, .. } => match unit {
                    egui::MouseWheelUnit::Line => delta.y,
                    egui::MouseWheelUnit::Point => delta.y / WHEEL_STEP,
                    egui::MouseWheelUnit::Page => delta.y * 3.0,
                },
                _ => 0.0,
            })
            .sum();
        // The wheel moves the selection; nothing scrolls under it.
        input.smooth_scroll_delta = egui::Vec2::ZERO;
        steps
    });
    let mut after = before * FREE_MOVE_STEP.powf(f64::from(steps));
    // The selection stays at least half a metre from the camera.
    let started = (middle - eye_at).length() / before;
    if started > 1e-6 {
        after = after.max(0.5 / started);
    }
    let after = after.min(100.0);
    ui_state.free_move = Some(after);
    Some((before, after))
}

/// The facing (degrees about Z) of a gizmo's transform.
fn gizmo_facing(transform: &GizmoTransform) -> f32 {
    let q = transform.rotation;
    let rotation = bevy::math::DQuat::from_xyzw(q.v.x, q.v.y, q.v.z, q.s);
    let ahead = rotation * bevy::math::DVec3::X;
    ahead.y.atan2(ahead.x).to_degrees() as f32
}

/// The selection as the gizmo moves it: where it stands, its turn about Z,
/// and its scale (a prop's, uniform) or size (a box's). Several selected,
/// it stands in their middle, turned as the one whose details show.
fn gizmo_transform(state: &EditorState, target: Target) -> Option<GizmoTransform> {
    let (mut position, facing) = state.editor.placement(target)?;
    let several = state.editor.selection().len() > 1;
    if several {
        position = state.editor.selection_centre()?;
    }
    let scale = match target {
        Target::Prop(at) => Vec3::splat(state.editor.project.kjk.props.get(at)?.scale),
        Target::Effect(_) => Vec3::ONE,
        Target::Box(at) => Vec3::from_array(state.editor.project.kjk.boxes.get(at)?.size),
        Target::Object(..)
        | Target::Room(_)
        | Target::Entry
        | Target::Sun
        | Target::Light(_)
        | Target::Lamp(_)
        | Target::Fitted(_) => Vec3::ONE,
    };
    let p = Vec3::from_array(position).as_dvec3();
    let mut r = bevy::math::DQuat::from_rotation_z(f64::from(facing).to_radians());
    // A lamp turns on every axis: its yaw, then its pitch, then its roll.
    if let Target::Lamp(at) = target
        && !several
        && let Some(lamp) = state.editor.project.kjk.lamps.get(at)
    {
        r = lamp_turn(lamp);
    }
    let s = scale.as_dvec3();
    Some(GizmoTransform::from_scale_rotation_translation(
        mint::Vector3 {
            x: s.x,
            y: s.y,
            z: s.z,
        },
        mint::Quaternion {
            v: mint::Vector3 {
                x: r.x,
                y: r.y,
                z: r.z,
            },
            s: r.w,
        },
        mint::Vector3 {
            x: p.x,
            y: p.y,
            z: p.z,
        },
    ))
}

/// A lamp's whole turn: its yaw about Z, after its pitch about Y, after
/// its roll about X (`level_edit::lamps`).
fn lamp_turn(lamp: &level_edit::PlacedLamp) -> bevy::math::DQuat {
    bevy::math::DQuat::from_euler(
        bevy::math::EulerRot::ZYX,
        f64::from(lamp.yaw).to_radians(),
        f64::from(lamp.pitch).to_radians(),
        f64::from(lamp.roll).to_radians(),
    )
}

/// `lamp` turned to `rotation`, as its yaw, pitch and roll in degrees.
fn lamp_turned(
    lamp: &level_edit::PlacedLamp,
    rotation: bevy::math::DQuat,
) -> level_edit::PlacedLamp {
    let (yaw, pitch, roll) = rotation.to_euler(bevy::math::EulerRot::ZYX);
    // Whole tenths of a degree, so a lamp let go reads back as shown.
    // Adding 0 turns a -0 into 0, which the details show as "0.0°".
    let degrees = |radians: f64| ((radians.to_degrees() * 10.0).round() / 10.0 + 0.0) as f32;
    level_edit::PlacedLamp {
        yaw: degrees(yaw).rem_euclid(360.0),
        pitch: degrees(pitch),
        roll: degrees(roll),
        ..lamp.clone()
    }
}

/// The edits a gizmo's new transform makes of the selection. `lamp` is
/// the selected lamp, which the gizmo turns on every axis.
fn gizmo_edits(
    target: Target,
    lamp: Option<&level_edit::PlacedLamp>,
    result: &GizmoResult,
    moved: &GizmoTransform,
) -> Vec<Edit> {
    let position = [
        moved.translation.x,
        moved.translation.y,
        moved.translation.z,
    ]
    .map(|v| v as f32);
    match result {
        GizmoResult::Translation { .. } => vec![Edit::Move(target, position)],
        GizmoResult::Rotation { .. } | GizmoResult::Arcball { .. } => {
            let q = moved.rotation;
            let rotation = bevy::math::DQuat::from_xyzw(q.v.x, q.v.y, q.v.z, q.s);
            if let (Target::Lamp(at), Some(lamp)) = (target, lamp) {
                return vec![Edit::SetLamp(at, lamp_turned(lamp, rotation))];
            }
            let ahead = rotation * bevy::math::DVec3::X;
            let facing = ahead.y.atan2(ahead.x).to_degrees() as f32;
            vec![Edit::Turn(target, facing)]
        }
        GizmoResult::Scale { .. } => {
            let s = [moved.scale.x, moved.scale.y, moved.scale.z].map(|v| v as f32);
            match target {
                Target::Prop(at) => vec![Edit::Scale(at, s[0])],
                Target::Box(at) => vec![Edit::Resize(
                    at,
                    s.map(|side| side.max(level_edit::area::SMALLEST_SIDE)),
                )],
                Target::Object(..)
                | Target::Room(_)
                | Target::Entry
                | Target::Sun
                | Target::Light(_)
                | Target::Lamp(_)
                | Target::Effect(_)
                | Target::Fitted(_) => Vec::new(),
            }
        }
    }
}

/// The gizmo's modes for the tool and the selection: none for a tool that
/// does not apply (scaling a creature).
fn modes(tool: Tool, target: Target) -> Option<EnumSet<GizmoMode>> {
    use transform_gizmo_egui::config::GizmoModeKind;
    let editable = match target {
        Target::Object(kind, _) => EDITABLE.contains(&kind),
        _ => true,
    };
    if !editable {
        return None;
    }
    match (tool, target) {
        (Tool::Select | Tool::Measure, _) => None,
        // The sun only turns round the level.
        (Tool::Move | Tool::Scale, Target::Sun) => None,
        (Tool::Move, _) => Some(GizmoMode::all_from_kind(GizmoModeKind::Translate)),
        // Everything turns about Z alone but a lamp, which is kjk's own and
        // turns on every axis.
        (Tool::Rotate, Target::Object(Kind::Trigger | Kind::Encounter, _)) => None,
        (Tool::Rotate, Target::Lamp(_)) => {
            Some(GizmoMode::RotateX | GizmoMode::RotateY | GizmoMode::RotateZ)
        }
        (Tool::Rotate, _) => Some(EnumSet::only(GizmoMode::RotateZ)),
        (Tool::Scale, Target::Prop(_)) => Some(EnumSet::only(GizmoMode::ScaleUniform)),
        (Tool::Scale, Target::Effect(_)) => None,
        (Tool::Scale, Target::Box(_)) => {
            Some(GizmoMode::ScaleX | GizmoMode::ScaleY | GizmoMode::ScaleZ)
        }
        (
            Tool::Scale,
            Target::Object(..)
            | Target::Room(_)
            | Target::Entry
            | Target::Light(_)
            | Target::Lamp(_),
        ) => None,
        (_, Target::Fitted(_)) => None,
    }
}

/// Applies an edit made by a drag: the first of a drag is an edit of its
/// own, the later ones join it.
fn drag_edit(state: &mut EditorState, ui: &mut UiState, edit: Edit) {
    if ui.dragging {
        state.apply_continuing(edit);
    } else {
        state.apply(edit);
        ui.dragging = true;
    }
}

/// Where a dropped or placed asset goes: the level under `point` of the
/// window, else a few metres ahead of the camera.
fn place_at(
    state: &mut EditorState,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    point: Option<Vec2>,
) -> [f32; 3] {
    let ray = point.and_then(|point| state::ray_at(cameras, point));
    match ray.and_then(|ray| state::ground(state, ray)) {
        Some(at) => at,
        None => state::place_point(state, cameras),
    }
}

/// Places `asset` at `at`.
fn place(state: &mut EditorState, asset: &Asset, at: [f32; 3], snapping: bool) {
    match asset {
        Asset::Template(AssetKind::Room, model) => {
            if let Err(why) = state::add_room(state, model, at) {
                state.say(why);
            } else if snapping && let Some(Target::Room(added)) = state.editor.selected {
                state::snap_to_seam(state, added, true);
            }
        }
        Asset::Template(
            kind @ (AssetKind::Placeable | AssetKind::Creature | AssetKind::Door),
            name,
        ) => {
            let kind = match kind {
                AssetKind::Placeable => Kind::Placeable,
                AssetKind::Door => Kind::Door,
                _ => Kind::Creature,
            };
            if let Err(why) = state::add_object(state, kind, name, at) {
                state.say(why);
            }
        }
        Asset::Object(kind, name) => {
            if let Err(why) = state::add_object(state, *kind, name, at) {
                state.say(why);
            }
        }
        Asset::Template(AssetKind::Prop, model) => {
            let name = state.editor.project.kjk.free_name("prop");
            state.apply(Edit::AddProp(Prop {
                name,
                model: model.clone(),
                position: at,
                yaw: 0.0,
                scale: 1.0,
                solid: true,
            }));
        }
        Asset::Light(shape) => {
            if let Some(light) = state::light_shape(shape, at) {
                state.apply(Edit::AddLight(light));
            }
        }
        Asset::Effect(path) => {
            if let Err(why) = state::add_effect(state, path, at) {
                state.say(why);
            }
        }
        Asset::Template(AssetKind::Lamp, key) => {
            let (model, mesh) = crate::lamps::LampAsset::of_key(key);
            let lamp = level_edit::PlacedLamp::new(&model, &mesh, at);
            if let Err(why) = state::add_lamp(state, lamp) {
                state.say(why);
            }
        }
        Asset::Shape(at_shape) => {
            let (stem, size, role) = SHAPES[*at_shape];
            let name = state
                .editor
                .project
                .kjk
                .free_name(&stem.to_ascii_lowercase().replace(' ', "-"));
            state.apply(Edit::AddBox(CollisionBox {
                name,
                centre: [at[0], at[1], at[2] + size[2] * 0.5],
                size,
                yaw: 0.0,
                role,
            }));
        }
    }
}

/// `edit ui ...`: the window's own state set by words, from `--cmds` and
/// the agent link. Words that are none of the window's, or that do not
/// read, are refused with the reason.
pub(crate) fn ui_command(
    state: &mut EditorState,
    ui: &mut UiState,
    words: &[&str],
) -> Result<(), String> {
    match words {
        ["tool", "select"] => ui.tool = Tool::Select,
        ["tool", "move"] => ui.tool = Tool::Move,
        ["tool", "rotate"] => ui.tool = Tool::Rotate,
        ["tool", "scale"] => ui.tool = Tool::Scale,
        ["tool", "measure"] => {
            ui.tool = Tool::Measure;
            ui.measure.clear();
        }
        ["tab", "rooms"] => ui.tab = Tab::Rooms,
        ["tab", "placeables"] => ui.tab = Tab::Placeables,
        ["tab", "creatures"] => ui.tab = Tab::Creatures,
        ["tab", "doors"] => ui.tab = Tab::Doors,
        ["tab", "props"] => ui.tab = Tab::Props,
        ["tab", "shapes"] => ui.tab = Tab::Shapes,
        ["tab", "waypoints"] => ui.tab = Tab::Waypoints,
        ["tab", "triggers"] => ui.tab = Tab::Triggers,
        ["tab", "sounds"] => ui.tab = Tab::Sounds,
        ["tab", "stores"] => ui.tab = Tab::Stores,
        ["tab", "encounters"] => ui.tab = Tab::Encounters,
        ["tab", "lights"] => ui.tab = Tab::Lights,
        ["tab", "lamps"] => ui.tab = Tab::Lamps,
        ["levels", on @ ("on" | "off")] => state.show_levels = *on == "on",
        ["search", rest @ ..] => ui.asset_search = rest.join(" "),
        ["elsewhere", on @ ("on" | "off")] => ui.assets_elsewhere = *on == "on",
        ["find", rest @ ..] => ui.outliner_search = rest.join(" "),
        ["snap", on @ ("on" | "off")] => ui.snapping = *on == "on",
        ["blocks", event] => {
            ui.blocks_event = state
                .editor
                .selected
                .map(|target| (target, event.to_string()));
        }
        ["blocks"] => ui.blocks_event = None,
        ["keys"] => ui.keys_window.open = true,
        ["keys", "off"] => ui.keys_window.open = false,
        ["adventure"] => ui.adventure_open = true,
        ["adventure", "off"] => ui.adventure_open = false,
        ["conversation"] => ui.dialog_open = None,
        ["conversation", name] => {
            ui.dialog_open = Some(name.to_ascii_lowercase());
            ui.dialog_selected = None;
        }
        ["conversation", "select", side, n] => {
            let side = if *side == "reply" {
                level_edit::dialog::Side::Reply
            } else {
                level_edit::dialog::Side::Entry
            };
            let n = (n.parse::<usize>()).map_err(|_| format!("{n}: a line's number"))?;
            ui.dialog_selected = Some((side, n, None));
        }
        ["link", "off"] => ui.link = None,
        ["link", "on"] => {
            let link = crate::agent::Link::open()
                .map_err(|why| format!("the agent link did not open: {why}"))?;
            ui.link = Some(link);
        }
        ["debug", n] => {
            let n = (n.parse::<usize>()).map_err(|_| format!("{n}: a stop's number"))?;
            ui.debug_at = n.saturating_sub(1);
        }
        ["break", script, line] => {
            let line = (line.parse::<u32>()).map_err(|_| format!("{line}: a line's number"))?;
            let marks = ui
                .breakpoints
                .entry(script.to_ascii_lowercase())
                .or_default();
            if !marks.remove(&line) {
                marks.insert(line);
            }
        }
        ["way_in", module] => ui.way_in_module = Some(module.to_ascii_lowercase()),
        ["way_in", module, arrival] => {
            ui.way_in_module = Some(module.to_ascii_lowercase());
            ui.way_in_arrival = arrival.to_string();
        }
        ["view"] => ui.viewing = None,
        ["view", script] => ui.viewing = Some(script.to_ascii_lowercase()),
        ["view", "window", on @ ("on" | "off")] => ui.viewer_window = *on == "on",
        ["view", "tab", tab] => {
            ui.view_tab = match *tab {
                "summary" => ViewTab::Summary,
                "listing" => ViewTab::Listing,
                "try" => ViewTab::Try,
                "debug" => ViewTab::Debug,
                _ => ViewTab::Source,
            }
        }
        words => {
            return crate::file::command(state, &mut ui.file, words).unwrap_or_else(|| {
                Err(format!(
                    "edit ui {}: takes tool, tab, search, elsewhere, find, snap, levels, view, way_in, blocks, adventure, conversation, keys, debug, break, link, file, quit or confirm",
                    words.join(" ")
                ))
            });
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn panels(
    mut contexts: EguiContexts,
    mut state: ResMut<EditorState>,
    mut ui_state: ResMut<UiState>,
    mut controls: ResMut<FlyControls>,
    mut camera_request: ResMut<CameraRequest>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let ctx = ctx.clone();
    let state = &mut *state;
    let ui = &mut *ui_state;
    if !ui.fonts_set {
        ui.fonts_set = true;
        match save_editor::model::fonts::system() {
            Ok(fonts) => use_fonts(&ctx, fonts),
            Err(why) => state.say(format!("the panels' text has no font: {why}")),
        }
    }
    for words in std::mem::take(&mut state.pending_ui) {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        if let Err(why) = ui_command(state, ui, &words) {
            state.say(why);
        }
    }
    let looking = buttons
        .as_ref()
        .is_some_and(|buttons| buttons.pressed(MouseButton::Right));
    // A picture of the view alone is being taken: no panel is drawn over
    // it for these frames.
    if state.pending_bare.is_some() {
        ctx.request_repaint();
        return;
    }

    let mut root = egui::Ui::new(
        ctx.clone(),
        "editor".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    toolbar(&mut root, state, ui);
    outliner(&mut root, state, ui, &mut camera_request, &cameras);
    details(&mut root, state, ui, &mut camera_request, &cameras);
    assets(&mut root, state, ui, &cameras);
    viewport(&mut root, state, ui, &cameras, looking);
    crate::file::dialogs(&ctx, state, &mut ui.file);
    recovery_window(&ctx, state);
    conversation_window(&ctx, state, ui);
    adventure_window(&ctx, state, ui);
    crate::keys::window(&ctx, state, &mut ui.keys_window, keys.as_deref());
    // The keys file, changed by hand, is read again. The camera flies by
    // its keys.
    let mut heard = Vec::new();
    state.keys.follow_file(&mut |said| heard.push(said));
    for said in heard {
        state.say(said);
    }
    let fly = crate::keys::fly_keys(&state.keys.map);
    if controls.keys.as_ref() != Some(&fly) {
        controls.keys = Some(fly);
    }
    room_levels(&ctx, state);
    // The buttons' commands, run as their keys run them.
    if let Some(command) = state.pending_command.take() {
        let words: Vec<&str> = command.split_whitespace().collect();
        state::run(state, &words, &cameras);
    }
    // Objects copied go to the window's clipboard; a copy pasted there
    // (Ctrl+V, with no field being typed in) is pasted where the view looks.
    if let Some(text) = state.copied.take() {
        ctx.copy_text(text);
    }
    if !ctx.egui_wants_keyboard_input() {
        let pasted = ctx.input(|input| {
            input.events.iter().find_map(|event| match event {
                egui::Event::Paste(text) if text.starts_with(level_edit::clip::HEAD) => {
                    Some(text.clone())
                }
                _ => None,
            })
        });
        if let Some(text) = pasted
            && let Err(why) = state::paste(state, &text, &cameras)
        {
            state.say(why);
        }
    }
    // The selection copied as text for an agent, and the agent link's turn.
    if std::mem::take(&mut ui.copy_asked) {
        ctx.copy_text(crate::agent::selection_text(state, &cameras));
        state.say(match state.editor.selected {
            Some(_) => "copied the selection for an agent",
            None => "copied the level and the camera for an agent",
        });
    }
    // The link is taken out for its turn, so a `ui` command it runs has
    // the window's state. It is not put back once such a command turned it
    // off.
    if let Some(mut link) = ui.link.take() {
        link.tick(state, ui, &cameras, &mut camera_request);
        if !link.off {
            ui.link = Some(link);
        }
    }

    // The keys, when no text field has them and the camera is not flying.
    let typing = ctx.egui_wants_keyboard_input();
    let pointer = ctx.pointer_hover_pos();
    let over_view = pointer.is_some_and(|at| ui.viewport.contains(at));
    controls.paused = typing || !(over_view || looking);
    if let Some(keys) = keys
        && !typing
        && !looking
        && !ui.keys_window.catching()
    {
        shortcuts(state, ui, &keys, &mut camera_request, &cameras);
    }
}

/// The system's fonts for the panels: egui's own are not shipped.
pub(crate) fn use_fonts(ctx: &egui::Context, fonts: save_editor::model::fonts::Fonts) {
    let mut definitions = egui::FontDefinitions::empty();
    for (name, font) in [("system", fonts.text), ("system mono", fonts.mono)] {
        definitions.font_data.insert(
            name.to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(font.bytes)),
        );
    }
    definitions
        .families
        .insert(egui::FontFamily::Proportional, vec!["system".into()]);
    definitions.families.insert(
        egui::FontFamily::Monospace,
        vec!["system mono".into(), "system".into()],
    );
    ctx.set_fonts(definitions);
}

fn shortcuts(
    state: &mut EditorState,
    ui: &mut UiState,
    keys: &ButtonInput<KeyCode>,
    camera_request: &mut CameraRequest,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    use crate::keys::{Action, Place};
    // The keys file says which keys do what (`keys`).
    for action in state.keys.map.pressed(Place::View, keys) {
        let command: &[&str] = match action {
            Action::SelectTool => {
                ui.tool = Tool::Select;
                &[]
            }
            Action::MoveTool => {
                ui.tool = Tool::Move;
                &[]
            }
            Action::RotateTool => {
                ui.tool = Tool::Rotate;
                &[]
            }
            Action::ScaleTool => {
                ui.tool = Tool::Scale;
                &[]
            }
            Action::Frame => {
                frame_selection(state, camera_request, cameras);
                &[]
            }
            Action::Delete => &["delete"],
            Action::Duplicate => &["duplicate"],
            Action::DropToFloor => &["drop"],
            Action::SelectAll => {
                select_everything(state);
                &[]
            }
            Action::SelectNone => {
                state.editor.select(None, false);
                &[]
            }
            Action::Undo => &["undo"],
            Action::Redo => &["redo"],
            Action::Save => &["save"],
            Action::Copy => &["copy"],
            Action::CopyForAgent => {
                ui.copy_asked = true;
                &[]
            }
            Action::PlacePicked => {
                if let Some(asset) = ui.picked.clone() {
                    let at = state::place_point(state, cameras);
                    place(state, &asset, at, ui.snapping);
                }
                &[]
            }
            Action::ShowCollision => {
                state.show_collision = !state.show_collision;
                &[]
            }
            Action::ShowNav => {
                state.show_nav = !state.show_nav;
                &[]
            }
            Action::ShowLights => {
                state.show_lights = !state.show_lights;
                &[]
            }
            Action::Play => &["play"],
            Action::PlayHere => &["play", "here"],
            Action::Walk => &["walk"],
            Action::Audit => &["audit"],
            // The fly keys work while the right mouse button is held, and
            // the conversation's keys in its window.
            Action::FlyForward
            | Action::FlyBack
            | Action::FlyLeft
            | Action::FlyRight
            | Action::FlyDown
            | Action::FlyUp
            | Action::FlyFaster
            | Action::TalkNext
            | Action::TalkBeside => &[],
        };
        if !command.is_empty() {
            state::run(state, command, cameras);
        }
    }
}

/// Selects every object, prop, collision box and lamp of the level, and
/// every room of a new map.
fn select_everything(state: &mut EditorState) {
    let mut all: Vec<Target> = (state.editor.entries().iter())
        .map(|entry| Target::Object(entry.kind, entry.index))
        .collect();
    let kjk = &state.editor.project.kjk;
    all.extend((0..kjk.props.len()).map(Target::Prop));
    all.extend((0..kjk.boxes.len()).map(Target::Box));
    all.extend((0..kjk.lamps.len()).map(Target::Lamp));
    all.extend((0..kjk.effects.len()).map(Target::Effect));
    let rooms = (state.editor.project.map.as_ref()).map_or(0, |map| map.rooms.len());
    all.extend((0..rooms).map(Target::Room));
    state.say(format!("{} selected", all.len()));
    state.editor.select_all(all);
}

pub(crate) fn frame_selection(
    state: &EditorState,
    camera_request: &mut CameraRequest,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let Some(target) = state.editor.selected else {
        return;
    };
    let Some((position, _)) = state.editor.placement(target) else {
        return;
    };
    let Some((_, eye)) = cameras.iter().next() else {
        return;
    };
    let eye = game_to_bevy().inverse() * eye.translation();
    camera_request.0 = Some(framing(Vec3::from_array(position) + Vec3::Z, eye));
}

fn toolbar(root: &mut egui::Ui, state: &mut EditorState, ui_state: &mut UiState) {
    egui::Panel::top("toolbar").show(root, |ui| {
        ui.horizontal(|ui| {
            crate::file::menu(ui, state, ui_state);
            ui.separator();
            for (tool, label, key) in [
                (Tool::Select, "Select", "Q"),
                (Tool::Move, "Move", "W"),
                (Tool::Rotate, "Rotate", "E"),
                (Tool::Scale, "Scale", "R"),
            ] {
                ui.selectable_value(&mut ui_state.tool, tool, label)
                    .on_hover_text(format!("{label} ({key})"));
            }
            if ui
                .selectable_value(&mut ui_state.tool, Tool::Measure, "Measure")
                .on_hover_text(
                    "Click two places of the level: the view shows how far apart they are. A third click starts again.",
                )
                .clicked()
            {
                ui_state.measure.clear();
            }
            ui.separator();
            ui.checkbox(&mut ui_state.snapping, "Snap");
            ui.add_enabled(
                ui_state.snapping,
                egui::DragValue::new(&mut ui_state.snap_distance)
                    .range(0.01..=10.0)
                    .speed(0.01)
                    .suffix(" m"),
            );
            ui.add_enabled(
                ui_state.snapping,
                egui::DragValue::new(&mut ui_state.snap_angle)
                    .range(1.0..=90.0)
                    .speed(0.5)
                    .suffix("°"),
            );
            ui.separator();
            if ui
                .add_enabled(state.editor.undo_depth() > 0, egui::Button::new("Undo"))
                .on_hover_text("Ctrl+Z")
                .clicked()
            {
                state.editor.undo();
                state.quiet = 0;
            }
            if ui
                .add_enabled(state.editor.redo_depth() > 0, egui::Button::new("Redo"))
                .on_hover_text("Ctrl+Y")
                .clicked()
            {
                state.editor.redo();
                state.quiet = 0;
            }
            ui.separator();
            for (label, hint, command) in [
                ("Save", "Ctrl+S", "save"),
                ("Play", "F5: save, then play the project in kjk", "play"),
                (
                    "Play here",
                    "Shift+F5: save, then play the project starting where the view looks at the floor, facing as the camera does",
                    "play here",
                ),
                (
                    "Audit",
                    "F6: save, then find where a body leaves the level",
                    "audit",
                ),
            ] {
                let there = command != "audit" || state.setup.tools;
                if ui
                    .add_enabled(there, egui::Button::new(label))
                    .on_hover_text(hint)
                    .on_disabled_hover_text(crate::state::NO_TOOLS)
                    .clicked()
                {
                    state.pending_command = Some(command.to_string());
                }
            }
            play_lighting_menu(ui, state);
            ui.checkbox(&mut state.show_collision, "Collision")
                .on_hover_text("F2: walkable floor green, solid surfaces orange, edge walls blue");
            lighting_menu(ui, state);
            markers_menu(ui, state);
            scripts_menu(ui, state, ui_state);
            conversations_menu(ui, state, ui_state);
            if ui
                .selectable_label(ui_state.adventure_open, "Items and quests")
                .on_hover_text("The project's own items and its own quests.")
                .clicked()
            {
                ui_state.adventure_open = !ui_state.adventure_open;
            }
            ui.separator();
            if ui
                .button("Copy")
                .on_hover_text(
                    "Ctrl+Shift+C: copies the selected object (else the level) and the camera as text, \
                     to paste to an agent",
                )
                .clicked()
            {
                ui_state.copy_asked = true;
            }
            let mut on = ui_state.link.is_some();
            let shown = match &ui_state.link {
                Some(link) => format!(
                    "While on, the editor writes what is selected to live.json and runs the commands an \
                     agent writes to inbox.txt, in {}",
                    link.folder.display()
                ),
                None => "Lets an agent on this computer see what is selected and change the level \
                         while you watch: the editor shares its state in a file and runs the commands \
                         the agent writes to another (docs/LEVEL-EDIT.md)"
                    .to_string(),
            };
            if ui.checkbox(&mut on, "Agent link").on_hover_text(shown).changed() {
                ui_state.link = if on {
                    match crate::agent::Link::open() {
                        Ok(link) => {
                            state.say(format!("agent link on: {}", link.folder.display()));
                            Some(link)
                        }
                        Err(why) => {
                            state.say(format!("the agent link did not open: {why}"));
                            None
                        }
                    }
                } else {
                    state.say("agent link off");
                    None
                };
            }
            if let Some(job) = &state.bake_job {
                ui.label(
                    egui::RichText::new(format!(
                        "Baking {} ({} s): {}",
                        job.label,
                        job.started.elapsed().as_secs(),
                        job.status
                    ))
                    .small(),
                );
            }
            if let Some((started, _)) = &state.audit_job {
                ui.label(
                    egui::RichText::new(format!("Auditing ({} s)", started.elapsed().as_secs()))
                        .small(),
                );
            }
            ui.checkbox(&mut state.show_nav, "Nav").on_hover_text(
                "F3: the navigation mesh the game's characters find their way over, \
                 each island its own colour; red under furniture, purple under a door \
                 or placeable",
            );
            ui.separator();
            let project = &state.editor.project;
            ui.label(format!(
                "{} ({} {}){}",
                project.name,
                project.game,
                project.module,
                if state.editor.unsaved() {
                    ", not saved"
                } else {
                    ""
                }
            ));
        });
    });
}

fn outliner(
    root: &mut egui::Ui,
    state: &mut EditorState,
    ui_state: &mut UiState,
    camera_request: &mut CameraRequest,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    egui::Panel::left("outliner")
        .resizable(true)
        .default_size(260.0)
        .show(root, |ui| {
            ui.heading("Outliner");
            ui.add(
                egui::TextEdit::singleline(&mut ui_state.outliner_search)
                    .hint_text("Search tags and templates"),
            );
            let search = ui_state.outliner_search.to_ascii_lowercase();
            let matches =
                |text: &str| search.is_empty() || text.to_ascii_lowercase().contains(&search);
            let changes = state.editor.changes();
            let object_rows = match &ui_state.outliner_rows {
                Some((at, had, rows)) if *at == changes && *had == search => rows.clone(),
                _ => {
                    let entries = state.editor.entries();
                    let rows: Vec<(Kind, Vec<(usize, String)>)> = Kind::ALL
                        .into_iter()
                        .map(|kind| {
                            let rows = entries
                                .iter()
                                .filter(|entry| entry.kind == kind)
                                .map(|entry| {
                                    let tag = state.tag_of(entry);
                                    (entry.index, format!("{tag}  ({})", entry.template))
                                })
                                .filter(|(_, text)| matches(text))
                                .collect();
                            (kind, rows)
                        })
                        .collect();
                    let rows = std::sync::Arc::new(rows);
                    ui_state.outliner_rows = Some((changes, search.clone(), rows.clone()));
                    rows
                }
            };
            let selection = state.editor.selection();
            let mut clicked: Option<(Target, bool)> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                // The level's sun, the game's or the project's.
                let sun = ui
                    .selectable_label(selection.contains(&Target::Sun), "Sun")
                    .on_hover_text("The light far over the level that casts daylight's shadows");
                if sun.clicked() {
                    clicked = Some((Target::Sun, false));
                }
                for (kind, rows) in object_rows.iter() {
                    let kind = *kind;
                    if rows.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(format!("{} ({})", kind_label(kind), rows.len()))
                        .default_open(matches!(kind, Kind::Creature | Kind::Placeable))
                        .show(ui, |ui| {
                            for (index, text) in rows {
                                let target = Target::Object(kind, *index);
                                let row =
                                    ui.selectable_label(selection.contains(&target), text.as_str());
                                if row.clicked() {
                                    clicked = Some((target, false));
                                }
                                if row.double_clicked() {
                                    clicked = Some((target, true));
                                }
                            }
                        });
                }
                let kjk = &state.editor.project.kjk;
                for (label, names) in [
                    (
                        "Jedi Academy props",
                        kjk.props
                            .iter()
                            .map(|p| format!("{}  ({})", p.name, p.model))
                            .collect::<Vec<_>>(),
                    ),
                    (
                        "Collision boxes",
                        kjk.boxes.iter().map(|b| b.name.clone()).collect(),
                    ),
                    (
                        "Jedi Academy effects",
                        kjk.effects
                            .iter()
                            .map(|e| format!("{}  ({})", e.name, e.played_name()))
                            .collect(),
                    ),
                ] {
                    let rows: Vec<(usize, String)> = names
                        .into_iter()
                        .enumerate()
                        .filter(|(_, text)| matches(text))
                        .collect();
                    if rows.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(format!("{label} ({})", rows.len()))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (at, text) in rows {
                                let target = match label {
                                    "Jedi Academy props" => Target::Prop(at),
                                    "Jedi Academy effects" => Target::Effect(at),
                                    _ => Target::Box(at),
                                };
                                let row = ui.selectable_label(selection.contains(&target), text);
                                if row.clicked() {
                                    clicked = Some((target, false));
                                }
                                if row.double_clicked() {
                                    clicked = Some((target, true));
                                }
                            }
                        });
                }
                if let Some(map) = &state.editor.project.map {
                    let module = &state.editor.project.module;
                    let rows: Vec<(usize, String)> = map
                        .rooms
                        .iter()
                        .enumerate()
                        .map(|(at, room)| {
                            let name = level_edit::map::room_name(module, at);
                            (at, format!("{name}  ({})", room.model))
                        })
                        .filter(|(_, text)| matches(text))
                        .collect();
                    egui::CollapsingHeader::new(format!("Rooms ({})", rows.len()))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (at, text) in rows {
                                let target = Target::Room(at);
                                let row = ui.selectable_label(selection.contains(&target), text);
                                if row.clicked() {
                                    clicked = Some((target, false));
                                }
                                if row.double_clicked() {
                                    clicked = Some((target, true));
                                }
                            }
                            let row = ui.selectable_label(
                                state.editor.selected == Some(Target::Entry),
                                "Entry (where the player starts)",
                            );
                            if row.clicked() {
                                clicked = Some((Target::Entry, false));
                            }
                            if row.double_clicked() {
                                clicked = Some((Target::Entry, true));
                            }
                        });
                }
                // The lights: those placed by hand, then the game's own by
                // where they come from (shown while the lights are drawn).
                let placed: Vec<(usize, String)> = (state.editor.project.kjk.lights)
                    .iter()
                    .enumerate()
                    .map(|(at, light)| {
                        let named = if light.reason.is_empty() {
                            format!("light {at}")
                        } else {
                            light.reason.clone()
                        };
                        (at, named)
                    })
                    .filter(|(_, text)| matches(text))
                    .collect();
                let mut sources: BTreeMap<String, Vec<usize>> = BTreeMap::new();
                for (at, (_, source)) in state.editor.seen_lights.iter().enumerate() {
                    if matches(source) {
                        sources.entry(source.clone()).or_default().push(at);
                    }
                }
                if !placed.is_empty() || !sources.is_empty() {
                    let fitted: usize = sources.values().map(Vec::len).sum();
                    egui::CollapsingHeader::new(format!("Lights ({})", placed.len() + fitted))
                        .default_open(false)
                        .show(ui, |ui| {
                            let mut row = |ui: &mut egui::Ui, target: Target, text: String| {
                                let row = ui.selectable_label(selection.contains(&target), text);
                                if row.clicked() {
                                    clicked = Some((target, false));
                                }
                                if row.double_clicked() {
                                    clicked = Some((target, true));
                                }
                            };
                            for (at, text) in placed {
                                row(ui, Target::Light(at), text);
                            }
                            for (source, lights) in sources {
                                egui::CollapsingHeader::new(format!("{source} ({})", lights.len()))
                                    .id_salt(("fitted", source.clone()))
                                    .show(ui, |ui| {
                                        for at in lights {
                                            row(ui, Target::Fitted(at), format!("{source} #{at}"));
                                        }
                                    });
                            }
                        });
                }
                // The lamps placed by hand.
                let lamps: Vec<(usize, String)> = (state.editor.project.kjk.lamps)
                    .iter()
                    .enumerate()
                    .map(|(at, lamp)| (at, format!("{at}: {}", lamp.label())))
                    .filter(|(_, text)| matches(text))
                    .collect();
                if !lamps.is_empty() {
                    egui::CollapsingHeader::new(format!("Lamps ({})", lamps.len()))
                        .default_open(true)
                        .show(ui, |ui| {
                            for (at, text) in lamps {
                                let target = Target::Lamp(at);
                                let row = ui.selectable_label(selection.contains(&target), text);
                                if row.clicked() {
                                    clicked = Some((target, false));
                                }
                                if row.double_clicked() {
                                    clicked = Some((target, true));
                                }
                            }
                        });
                }
            });
            if let Some((target, frame)) = clicked {
                let adds = ui.ctx().input(|input| input.modifiers.command);
                state.editor.select(Some(target), adds && !frame);
                if frame {
                    frame_selection(state, camera_request, cameras);
                }
            }
        });
}

fn details(
    root: &mut egui::Ui,
    state: &mut EditorState,
    ui_state: &mut UiState,
    camera_request: &mut CameraRequest,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let ctx = root.ctx().clone();
    let ctx = &ctx;
    egui::Panel::right("details")
        .resizable(true)
        .default_size(320.0)
        .show(root, |ui| {
            if ui_state.viewing.is_some() && ui_state.viewer_window {
                // A window of its own over the view, for writing a script.
                let mut open = true;
                egui::Window::new("Script")
                    .default_size([760.0, 640.0])
                    .resizable(true)
                    .open(&mut open)
                    .show(ctx, |ui| script_viewer(ui, state, ui_state));
                if !open {
                    ui_state.viewing = None;
                }
            } else if ui_state.viewing.is_some() {
                egui::Panel::bottom("script viewer")
                    .resizable(true)
                    .default_size(320.0)
                    .show(ui, |ui| script_viewer(ui, state, ui_state));
            }
            // The details scroll above the viewer, which keeps its own part.
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.heading("Details");
                    let Some(target) = state.editor.selected else {
                        ui.label(
                            "Nothing is selected. Click an object in the view or the outliner.",
                        );
                        area_settings(ui, state, ui_state);
                        ways_in(ui, state, ui_state);
                        nav_links(ui, state, ui_state);
                        messages(ui, state);
                        return;
                    };
                    let together = state.editor.selection().len();
                    if together > 1 {
                        ui.label(
                            egui::RichText::new(format!(
                                "{together} things are selected. Move, Rotate, Duplicate (Ctrl+D), Delete, Drop to floor (End) and copying them (Ctrl+C) act on all of them. The details below are the first one's."
                            ))
                            .small(),
                        );
                        // How far the selection reaches, and how far apart
                        // two things are.
                        let places: Vec<Vec3> = (state.editor.selection().into_iter())
                            .filter_map(|each| state.editor.placement(each))
                            .map(|(at, _)| Vec3::from_array(at))
                            .collect();
                        if let Some((low, high)) = kjk_game::scene::span(places.iter().copied()) {
                            let size = high - low;
                            ui.label(
                                egui::RichText::new(format!(
                                    "Their places span {:.2} m along X, {:.2} m along Y and {:.2} m in height.",
                                    size.x, size.y, size.z
                                ))
                                .small(),
                            );
                        }
                        if let [a, b] = places.as_slice() {
                            ui.label(
                                egui::RichText::new(format!(
                                    "The two are {:.2} m apart.",
                                    a.distance(*b)
                                ))
                                .small(),
                            );
                        }
                    }
                    let facts = state::describe(state, target);
                    let text = |key: &str| facts[key].as_str().map(str::to_string);
                    egui::Grid::new("facts").num_columns(2).show(ui, |ui| {
                        for (label, value) in [
                            ("Kind", text("kind")),
                            ("Template", text("template")),
                            ("Tag", text("tag")),
                            ("Name", text("name")),
                            ("Model", text("model")),
                        ] {
                            if let Some(value) = value {
                                ui.label(label);
                                ui.label(value);
                                ui.end_row();
                            }
                        }
                    });
                    let editable = match target {
                        Target::Object(kind, _) => EDITABLE.contains(&kind),
                        _ => true,
                    };
                    if !editable {
                        ui.label("This object is shown only.");
                    }
                    ui.separator();
                    if target == Target::Sun {
                        sun_details(ui, state, ui_state);
                        messages(ui, state);
                        return;
                    }
                    let Some((mut position, mut facing)) = state.editor.placement(target) else {
                        return;
                    };
                    let mut edits: Vec<Edit> = Vec::new();
                    let mut dragging = false;
                    let mut released = false;
                    ui.add_enabled_ui(editable, |ui| {
                        egui::Grid::new("placement").num_columns(2).show(ui, |ui| {
                            ui.label("Position");
                            ui.horizontal(|ui| {
                                let mut moved = false;
                                for (axis, label) in ["X", "Y", "Z"].iter().enumerate() {
                                    let response = ui.add(
                                        egui::DragValue::new(&mut position[axis])
                                            .prefix(format!("{label} "))
                                            .speed(0.01)
                                            .max_decimals(3),
                                    );
                                    moved |= response.changed();
                                    dragging |= response.dragged();
                                    released |= response.drag_stopped() || response.lost_focus();
                                }
                                if moved {
                                    edits.push(Edit::Move(target, position));
                                }
                            });
                            ui.end_row();
                            // A trigger's or an encounter's outline does not
                            // turn.
                            if !matches!(
                                target,
                                Target::Object(Kind::Trigger | Kind::Encounter, _)
                            ) {
                                ui.label("Facing");
                                let response = ui.add(
                                    egui::DragValue::new(&mut facing)
                                        .speed(0.5)
                                        .suffix("°")
                                        .max_decimals(1),
                                );
                                if response.changed() {
                                    edits.push(Edit::Turn(target, facing));
                                }
                                dragging |= response.dragged();
                                released |= response.drag_stopped() || response.lost_focus();
                            }
                            ui.end_row();
                            match target {
                                Target::Prop(at) => {
                                    if let Some(prop) =
                                        state.editor.project.kjk.props.get(at).cloned()
                                    {
                                        let mut scale = prop.scale;
                                        ui.label("Scale");
                                        let response = ui.add(
                                            egui::DragValue::new(&mut scale)
                                                .range(
                                                    level_edit::area::SCALES.0
                                                        ..=level_edit::area::SCALES.1,
                                                )
                                                .speed(0.01),
                                        );
                                        if response.changed() {
                                            edits.push(Edit::Scale(at, scale));
                                        }
                                        dragging |= response.dragged();
                                        released |=
                                            response.drag_stopped() || response.lost_focus();
                                        ui.end_row();
                                    }
                                }
                                Target::Box(at) => {
                                    if let Some(shape) =
                                        state.editor.project.kjk.boxes.get(at).cloned()
                                    {
                                        let mut size = shape.size;
                                        ui.label("Size");
                                        ui.horizontal(|ui| {
                                            let mut sized = false;
                                            for (axis, label) in ["X", "Y", "Z"].iter().enumerate()
                                            {
                                                let response = ui.add(
                                                    egui::DragValue::new(&mut size[axis])
                                                        .prefix(format!("{label} "))
                                                        .range(
                                                            level_edit::area::SMALLEST_SIDE..=100.0,
                                                        )
                                                        .speed(0.01)
                                                        .suffix(" m"),
                                                );
                                                sized |= response.changed();
                                                dragging |= response.dragged();
                                                released |= response.drag_stopped()
                                                    || response.lost_focus();
                                            }
                                            if sized {
                                                edits.push(Edit::Resize(at, size));
                                            }
                                        });
                                        ui.end_row();
                                        ui.label("Role");
                                        let mut role = shape.role;
                                        egui::ComboBox::from_id_salt("box role")
                                            .selected_text(role_label(role))
                                            .show_ui(ui, |ui| {
                                                for each in BoxRole::ALL {
                                                    ui.selectable_value(
                                                        &mut role,
                                                        each,
                                                        role_label(each),
                                                    );
                                                }
                                            });
                                        if role != shape.role {
                                            state.apply(Edit::Role(at, role));
                                        }
                                        ui.end_row();
                                    }
                                }
                                Target::Light(at) => {
                                    if let Some(light) =
                                        state.editor.project.kjk.lights.get(at).cloned()
                                    {
                                        light_rows(ui, &light, &mut |changed| {
                                            edits.push(Edit::SetLight(at, changed))
                                        });
                                    }
                                }
                                Target::Fitted(at) => {
                                    if let Some((light, source)) =
                                        state.editor.seen_lights.get(at).cloned()
                                    {
                                        ui.label("Source");
                                        ui.label(source);
                                        ui.end_row();
                                        ui.label("Colour");
                                        ui.label(format!(
                                            "{:.2} {:.2} {:.2}",
                                            light.colour[0], light.colour[1], light.colour[2]
                                        ));
                                        ui.end_row();
                                        ui.label("Reach");
                                        ui.label(format!("{:.1} m", light.radius));
                                        ui.end_row();
                                        if light.cone > 0.0 {
                                            ui.label("Cone");
                                            ui.label(format!("{:.2}", light.cone));
                                            ui.end_row();
                                        }
                                        if light.half > 0.0 {
                                            ui.label("Strip");
                                            ui.label(format!("{:.1} m each way", light.half));
                                            ui.end_row();
                                        }
                                    }
                                }
                                Target::Lamp(at) => {
                                    if let Some(lamp) =
                                        state.editor.project.kjk.lamps.get(at).cloned()
                                    {
                                        lamp_rows(ui, &lamp, &mut |changed| {
                                            edits.push(Edit::SetLamp(at, changed))
                                        });
                                    }
                                }
                                Target::Effect(at) => {
                                    if let Some(effect) =
                                        state.editor.project.kjk.effects.get(at).cloned()
                                    {
                                        let response = effect_rows(ui, &effect, &mut |changed| {
                                            edits.push(Edit::SetEffect(at, changed))
                                        });
                                        dragging |= response.0;
                                        released |= response.1;
                                    }
                                }
                                Target::Object(..) | Target::Room(_) | Target::Entry | Target::Sun => {}
                            }
                        });
                    });
                    for edit in edits {
                        drag_edit(state, ui_state, edit);
                    }
                    if released
                        || (!dragging
                            && !ui_state.gizmo.is_focused()
                            && !ctx.egui_is_using_pointer())
                    {
                        ui_state.dragging = false;
                    }
                    if let Target::Object(kind, index) = target {
                        behaviour(ui, state, ui_state, (kind, index));
                    }
                    if let Target::Fitted(_) = target {
                        kind_section(ui, state);
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.add_enabled_ui(editable, |ui| {
                            if ui.button("Duplicate").on_hover_text("Ctrl+D").clicked() {
                                state.pending_command = Some("duplicate".into());
                            }
                            if ui.button("Delete").on_hover_text("Delete").clicked() {
                                state.pending_command = Some("delete".into());
                            }
                            if ui
                                .button("Drop to floor")
                                .on_hover_text(
                                    "End: puts everything selected on the walkable floor under it",
                                )
                                .clicked()
                            {
                                state.pending_command = Some("drop".into());
                            }
                            if matches!(target, Target::Object(Kind::Door, _))
                                && ui
                                    .button("Snap to doorway")
                                    .on_hover_text(
                                        "Puts the door in the nearest doorway of the level's layout, facing as the doorway does",
                                    )
                                    .clicked()
                            {
                                state.pending_command = Some("snap door".into());
                            }
                        });
                        if let Target::Room(_) = target
                            && ui
                                .button("Meet a seam")
                                .on_hover_text(
                                    "Moves the room so that an edge of its floor meets the nearest edge of another room's",
                                )
                                .clicked()
                        {
                            state.pending_command = Some("seam".into());
                        }
                        if let Target::Fitted(_) = target
                            && ui
                                .button("Copy as a placed light")
                                .on_hover_text("An editable copy, saved with the level's lights")
                                .clicked()
                        {
                            state.pending_command = Some("light copy".into());
                        }
                        if ui.button("Frame").on_hover_text("F").clicked() {
                            frame_selection(state, camera_request, cameras);
                        }
                    });
                    messages(ui, state);
                });
        });
}

/// The field of the item to add, in `UiState::typed`.
const ITEM_FIELD: &str = "item to add";

/// The tag's field, in `UiState::typed`.
const TAG_FIELD: &str = "tag";

/// What the selected placeable or creature does: its template, its event
/// scripts, and a placeable's use and items. A change gives the object a
/// template of its own (`level_edit::template`).
fn behaviour(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    ui_state: &mut UiState,
    (kind, index): (Kind, usize),
) {
    use level_edit::template;
    let target = Target::Object(kind, index);
    let Some(entry) = state.editor.entry(kind, index) else {
        return;
    };
    let restype = template::restype(kind);
    let Some(gff) = state.behaviour_source(kind, &entry) else {
        ui.label(format!("The template {} does not read.", entry.template));
        return;
    };
    if ui_state.typed_for != Some(target) {
        ui_state.typed.clear();
        ui_state.typed_for = Some(target);
    }
    // A change made elsewhere (an undo, a command) shows in the fields,
    // unless one is being typed in.
    let typing = ui.ctx().egui_wants_keyboard_input();
    if ui_state.typed_at != state.editor.changes() && !typing {
        ui_state.typed.retain(|label, _| label == ITEM_FIELD);
        ui_state.typed_at = state.editor.changes();
    }
    ui.separator();
    ui.label(egui::RichText::new("Behaviour").strong());
    if kind == Kind::Sound {
        listen_object(ui, state, &entry.template);
    }
    let users = state.editor.users(&entry.template);
    let about = match state.editor.project.templates.get(&entry.template) {
        _ if restype.is_none() => format!(
            "Made from {}, and read whole from the area's object list: its tag and scripts are changed there.",
            entry.template
        ),
        Some(made) if users > 1 => format!(
            "Template {}: this project's copy of {}, used by {users} objects. A change gives this object a copy of its own.",
            entry.template, made.from
        ),
        Some(made) => format!(
            "Template {}: this project's copy of {}.",
            entry.template, made.from
        ),
        None => format!(
            "Template {}: the game's. A change gives this object a copy of its own.",
            entry.template
        ),
    };
    ui.label(egui::RichText::new(about).small());
    let mut changes: Vec<Change> = Vec::new();
    let placeable = restype == Some(fmt_odyssey::ResType::UTP);
    let holds_items = restype.is_some_and(template::holds_items);
    // The tag, which scripts find the object by.
    ui.horizontal(|ui| {
        ui.label("Tag");
        let shown = state.tag_of(&entry);
        let typed = ui_state
            .typed
            .entry(TAG_FIELD.into())
            .or_insert_with(|| shown.clone());
        let field = ui
            .add(egui::TextEdit::singleline(typed).desired_width(180.0))
            .on_hover_text(
                "The name scripts find this object by: a companion's waypoint, a door's partner.",
            );
        if field.lost_focus() && *typed != shown {
            changes.push(Change::Tag(typed.clone()));
        }
    });
    // The conversation the object speaks, where its kind has one.
    if gff.root.get("Conversation").is_some()
        || matches!(kind, Kind::Creature | Kind::Placeable | Kind::Door)
    {
        let now = gff
            .root
            .resref("Conversation")
            .map(|name| name.to_ascii_lowercase())
            .unwrap_or_else(String::new);
        ui.horizontal(|ui| {
            ui.label("Conversation");
            let typed = ui_state
                .typed
                .entry("Conversation".into())
                .or_insert_with(|| now.clone());
            let field = crate::choose::field(ui, "conversation", typed, ("none", 130.0), || {
                crate::choose::conversations(state)
            });
            let done = field.done();
            field.response.on_hover_text(
                "The conversation the object speaks when the player talks to it or uses it.",
            );
            if done && typed.to_ascii_lowercase() != now {
                changes.push(Change::Conversation(typed.trim().to_ascii_lowercase()));
            }
            let own = state.editor.project.conversations.contains_key(&now);
            let the_games =
                !now.is_empty() && state.setup.resolver.find(&now, ResType::DLG).is_some();
            // The game's conversation opens to be read, which changes
            // nothing, or is copied to be changed.
            let open_hint = if own {
                "Opens the project's conversation, to read and change it."
            } else {
                "Opens the game's conversation to be read. The project does not change."
            };
            if (own || the_games) && ui.small_button("Open").on_hover_text(open_hint).clicked() {
                state
                    .pending_ui
                    .push(vec!["conversation".into(), now.clone()]);
            }
            if !own
                && the_games
                && ui
                    .small_button("Edit a copy")
                    .on_hover_text("Copies the game's conversation into the project, to change it.")
                    .clicked()
            {
                state.pending_command = Some(format!("conversation copy {now}"));
            } else if now.is_empty()
                && ui
                    .small_button("New")
                    .on_hover_text("Makes a conversation of the project for this object.")
                    .clicked()
            {
                let base = state.tag_of(&entry).to_ascii_lowercase();
                let base: String = base
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .take(12)
                    .collect();
                let taken = |name: &str| {
                    state.editor.project.conversations.contains_key(name)
                        || state.setup.resolver.find(name, ResType::DLG).is_some()
                };
                let name = (1..)
                    .map(|n| {
                        format!("kjd{n}_{base}")
                            .chars()
                            .take(16)
                            .collect::<String>()
                    })
                    .find(|name| !taken(name))
                    .expect("some number is free");
                changes.push(Change::Conversation(name.clone()));
                state.pending_command = Some(format!("conversation new {name}"));
            }
        });
    }
    if placeable {
        ui.horizontal(|ui| {
            for (label, shown, hint) in [
                (
                    "Useable",
                    "Can be used",
                    "The player can use it: open it, or run its OnUsed script.",
                ),
                (
                    "HasInventory",
                    "Holds items",
                    "It is a container: using it opens its items.",
                ),
            ] {
                let mut on = template::flag(&gff, label).unwrap_or(false);
                if ui.checkbox(&mut on, shown).on_hover_text(hint).changed() {
                    changes.push(Change::Flag(label.to_string(), on));
                }
            }
        });
    }
    // Each event's script.
    let scripts = template::scripts(&gff);
    egui::Grid::new("scripts").num_columns(3).show(ui, |ui| {
        for (label, script) in &scripts {
            ui.label(label);
            let typed = ui_state
                .typed
                .entry(label.clone())
                .or_insert_with(|| script.clone());
            let known = typed.is_empty()
                || state.setup.scripts.binary_search(typed).is_ok()
                || state.project_scripts.contains(typed.as_str())
                || state.drafts.contains_key(typed.as_str());
            if !known {
                ui.visuals_mut().override_text_color = Some(egui::Color32::from_rgb(230, 120, 90));
            }
            let field = crate::choose::field(ui, ("script", label), typed, ("none", 150.0), || {
                crate::choose::scripts(state)
            });
            ui.visuals_mut().override_text_color = None;
            let done = field.done();
            field.response.on_hover_text(if known {
                "The script this event runs. Type a part of a compiled script's name and pick it."
            } else {
                "The install has no compiled script of this name."
            });
            if done && typed.to_ascii_lowercase() != *script {
                changes.push(Change::Script(label.clone(), typed.to_ascii_lowercase()));
            }
            ui.horizontal(|ui| {
                if !script.is_empty() {
                    let viewed = ui_state.viewing.as_deref() == Some(script.as_str());
                    if ui
                        .selectable_label(viewed, "View")
                        .on_hover_text("Shows what the script does, under the details.")
                        .clicked()
                    {
                        ui_state.viewing = (!viewed).then(|| script.clone());
                    }
                }
                let shown = ui_state.blocks_event.as_ref() == Some(&(target, label.clone()));
                if ui
                    .selectable_label(shown, "Blocks")
                    .on_hover_text("What the object does on this event, picked from a list rather than written.")
                    .clicked()
                {
                    ui_state.blocks_event = (!shown).then(|| (target, label.clone()));
                }
            });
            ui.end_row();
        }
    });
    if let Some((on, event)) = ui_state.blocks_event.clone()
        && on == target
    {
        blocks_panel(ui, state, target, &event);
    }
    crate::links::section(ui, state, (kind, index));
    if matches!(kind, Kind::Door | Kind::Trigger) {
        leads_to(ui, state, (kind, index));
    }
    if matches!(kind, Kind::Trigger | Kind::Encounter) {
        outline_details(ui, state, ui_state, (kind, index));
    }
    // A creature's, a store's or an encounter's numbers and lists.
    // A door's lock and toughness are its own entry's numbers.
    let fields_of = restype.or((kind == Kind::Door).then_some(ResType::UTD));
    if let Some(restype) = fields_of
        && !level_edit::fields::numbers(restype).is_empty()
    {
        template_fields(ui, state, ui_state, (target, restype), &gff, &mut changes);
    }
    // The items a placeable or a creature starts with, a store's stock.
    if holds_items {
        let items = template::items(&gff);
        let (heading, flag) = match restype {
            Some(fmt_odyssey::ResType::UTC) => (
                "Items it carries",
                Some((
                    "Dropable",
                    "Drops",
                    "The item drops when the creature dies.",
                )),
            ),
            Some(fmt_odyssey::ResType::UTM) => (
                "Items it sells",
                Some((
                    "Infinite",
                    "Never runs out",
                    "The store never runs out of the item.",
                )),
            ),
            _ => ("Items it starts with", None),
        };
        ui.label(format!("{heading}: {}", items.len()));
        for (at, item) in items.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(item);
                if let Some((label, shown, hint)) = flag {
                    let mut on = level_edit::fields::item_flag(&gff, at, label);
                    if ui.checkbox(&mut on, shown).on_hover_text(hint).changed() {
                        changes.push(Change::Field(level_edit::fields::FieldChange::ItemFlag(
                            at, label, on,
                        )));
                    }
                }
                if ui.small_button("Remove").clicked() {
                    changes.push(Change::RemoveItem(at));
                }
            });
        }
        ui.horizontal(|ui| {
            let typed = ui_state
                .typed
                .entry(ITEM_FIELD.into())
                .or_insert_with(String::new);
            // An item picked from the list is added at once.
            let field = crate::choose::field(ui, ITEM_FIELD, typed, ("an item's name", 180.0), || {
                crate::choose::items(state)
            });
            field.response.on_hover_text(
                "Type a part of an item's name, as the game shows it or as its file is named, and pick it.",
            );
            let ready = crate::choose::item_known(state, typed);
            let add = ui.add_enabled(ready, egui::Button::new("Add item")).clicked();
            if let Some(picked) = field.picked.filter(|_| ready) {
                changes.push(Change::AddItem(picked));
                typed.clear();
            } else if add {
                changes.push(Change::AddItem(typed.trim().to_ascii_lowercase()));
                typed.clear();
            }
        });
    }
    for change in changes {
        state.change_template(target, change);
        // The script fields show the template as it now is.
        ui_state.typed.retain(|label, _| label == ITEM_FIELD);
    }
}

/// The rows of the 2DA `table` as a picker lists them: each row's number
/// and its name (its talk-table `name`, with its label where that says
/// something else), read once. Rows with neither are left out.
fn table_rows(
    state: &EditorState,
    ui_state: &mut UiState,
    table: &str,
) -> std::sync::Arc<Vec<(u16, String)>> {
    if let Some(rows) = ui_state.table_rows.get(table) {
        return rows.clone();
    }
    let rows = crate::state::table_names(state, table);
    let rows = std::sync::Arc::new(rows);
    ui_state.table_rows.insert(table.to_string(), rows.clone());
    rows
}

/// A row of a 2DA's name as `rows` has it, else its number.
fn row_name(rows: &[(u16, String)], row: u16) -> String {
    rows.iter()
        .find(|(at, _)| *at == row)
        .map_or_else(|| format!("row {row}"), |(_, name)| format!("{row} {name}"))
}

/// A picker of a 2DA's rows: a button with `current`'s name that opens a
/// list with a filter. The row picked, if one was.
fn row_picker(
    ui: &mut egui::Ui,
    ui_state: &mut UiState,
    rows: &[(u16, String)],
    (id, shown): (&str, &str),
    current: Option<u16>,
) -> Option<u16> {
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    let mut picked = None;
    let filter = ui_state.row_filters.entry(id.to_string()).or_default();
    egui::containers::menu::MenuButton::new(shown)
        .config(config)
        .ui(ui, |ui| {
            ui.add(
                egui::TextEdit::singleline(filter)
                    .hint_text("filter")
                    .desired_width(220.0),
            );
            let needle = filter.to_ascii_lowercase();
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    let found = rows
                        .iter()
                        .filter(|(row, name)| {
                            needle.is_empty()
                                || name.to_ascii_lowercase().contains(&needle)
                                || row.to_string() == needle
                        })
                        .take(400);
                    for (row, name) in found {
                        if ui
                            .selectable_label(current == Some(*row), format!("{row}  {name}"))
                            .clicked()
                        {
                            picked = Some(*row);
                            ui.close();
                        }
                    }
                });
        });
    picked
}

/// A number of a template's, dragged or typed: the value to change it to
/// once the drag ends or the typing is done.
fn number_field(
    ui: &mut egui::Ui,
    ui_state: &mut UiState,
    key: &str,
    (now, min, max, whole): (f64, f64, f64, bool),
) -> Option<f64> {
    let mut value = match &ui_state.number_drag {
        Some((dragged, value)) if dragged == key => *value,
        _ => now,
    };
    let mut drag = egui::DragValue::new(&mut value)
        .range(min..=max)
        .speed(if whole { 0.2 } else { 0.05 });
    if whole {
        drag = drag.max_decimals(0);
    } else {
        drag = drag.max_decimals(2);
    }
    let response = ui.add(drag);
    if response.dragged() {
        ui_state.number_drag = Some((key.to_string(), value));
        return None;
    }
    let ended = response.drag_stopped()
        && ui_state
            .number_drag
            .as_ref()
            .is_some_and(|(dragged, _)| dragged == key);
    if ended {
        ui_state.number_drag = None;
    }
    let value = if whole { value.round() } else { value };
    ((ended || response.changed()) && (value - now).abs() > 1e-6).then_some(value)
}

/// A creature's, a store's or an encounter's fields in its behaviour: its
/// name, its numbers, and its lists (a creature's classes, powers, feats,
/// skills and equipment; an encounter's creatures and spawn points).
fn template_fields(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    ui_state: &mut UiState,
    (target, restype): (Target, fmt_odyssey::ResType),
    gff: &fmt_odyssey::gff::Gff,
    changes: &mut Vec<Change>,
) {
    use level_edit::fields::{self, FieldChange};
    let field = |change: FieldChange| Change::Field(change);
    // The names.
    let talk = state.setup.talk.clone();
    let said = |strref: u32| {
        talk.as_deref()
            .and_then(|talk| talk.text(strref))
            .map(|text| String::from_utf8_lossy(text).into_owned())
    };
    for (label, shown) in fields::names(restype) {
        let now = fields::name_of(gff, label, &said);
        ui.horizontal(|ui| {
            ui.label(*shown);
            let typed = ui_state
                .typed
                .entry(format!("name {label}"))
                .or_insert_with(|| now.clone());
            let edited = ui.add(egui::TextEdit::singleline(typed).desired_width(180.0)).on_hover_text(
                "The name the game shows. The editor keeps it in the template itself, with no talk-table line.",
            );
            if edited.lost_focus() && *typed != now {
                changes.push(field(FieldChange::Name(label.to_string(), typed.clone())));
            }
        });
    }
    // The numbers.
    let numbers = fields::numbers(restype);
    egui::CollapsingHeader::new(match restype {
        fmt_odyssey::ResType::UTC => "Statistics",
        fmt_odyssey::ResType::UTM => "Prices",
        fmt_odyssey::ResType::UTD => "Lock and toughness",
        _ => "Spawning",
    })
    .id_salt("template numbers")
    .default_open(true)
    .show(ui, |ui| {
        egui::Grid::new("template numbers grid")
            .num_columns(2)
            .show(ui, |ui| {
                for number in numbers {
                    let now = fields::number_of(gff, number.label);
                    ui.label(number.shown).on_hover_text(number.hint);
                    if number.switch {
                        let mut on = now.unwrap_or(0.0) != 0.0;
                        if ui
                            .checkbox(&mut on, "")
                            .on_hover_text(number.hint)
                            .changed()
                        {
                            changes.push(field(FieldChange::Number(
                                number.label.to_string(),
                                f64::from(u8::from(on)),
                            )));
                        }
                    } else if let Some(table) = number.table {
                        let rows = table_rows(state, ui_state, table);
                        let current = now.map(|row| row as u16);
                        let shown =
                            current.map_or_else(|| "none".to_string(), |row| row_name(&rows, row));
                        if let Some(row) = row_picker(
                            ui,
                            ui_state,
                            &rows,
                            (&format!("{} picker", number.label), &shown),
                            current,
                        ) {
                            changes.push(field(FieldChange::Number(
                                number.label.to_string(),
                                f64::from(row),
                            )));
                        }
                    } else {
                        let whole = number.store != fields::Store::Float;
                        if let Some(value) = number_field(
                            ui,
                            ui_state,
                            number.label,
                            (
                                now.unwrap_or(number.min.max(0.0)),
                                number.min,
                                number.max,
                                whole,
                            ),
                        ) {
                            changes
                                .push(field(FieldChange::Number(number.label.to_string(), value)));
                        }
                    }
                    ui.end_row();
                }
            });
    });
    match restype {
        fmt_odyssey::ResType::UTC => creature_lists(ui, state, ui_state, gff, changes),
        fmt_odyssey::ResType::UTE => encounter_lists(ui, state, ui_state, target, gff, changes),
        _ => {}
    }
}

/// A creature's classes with their Force powers, its feats, its skills and
/// its equipment.
fn creature_lists(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    ui_state: &mut UiState,
    gff: &fmt_odyssey::gff::Gff,
    changes: &mut Vec<Change>,
) {
    use level_edit::fields::{self, FieldChange};
    let field = |change: FieldChange| Change::Field(change);
    let classes = fields::classes(gff);
    let class_rows = table_rows(state, ui_state, "classes");
    let spell_rows = table_rows(state, ui_state, "spells");
    egui::CollapsingHeader::new(format!("Classes and Force powers ({})", classes.len()))
        .id_salt("template classes")
        .show(ui, |ui| {
            for (at, (class, levels, powers)) in classes.iter().enumerate() {
                ui.horizontal(|ui| {
                    if let Some(row) = row_picker(
                        ui,
                        ui_state,
                        &class_rows,
                        (&format!("class {at}"), &row_name(&class_rows, *class)),
                        Some(*class),
                    ) {
                        changes.push(field(FieldChange::Class(at, Some((row, *levels)))));
                    }
                    ui.label("levels");
                    if let Some(value) = number_field(
                        ui,
                        ui_state,
                        &format!("class {at} levels"),
                        (f64::from(*levels), 1.0, 50.0, true),
                    ) {
                        changes.push(field(FieldChange::Class(at, Some((*class, value as u16)))));
                    }
                    if ui.small_button("Remove").clicked() {
                        changes.push(field(FieldChange::Class(at, None)));
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(16.0);
                    for power in powers {
                        if ui
                            .small_button(format!("{} x", row_name(&spell_rows, *power)))
                            .on_hover_text("Takes the power away.")
                            .clicked()
                        {
                            changes.push(field(FieldChange::Power(at, *power, false)));
                        }
                    }
                    if let Some(row) = row_picker(
                        ui,
                        ui_state,
                        &spell_rows,
                        (&format!("class {at} power"), "Add a power"),
                        None,
                    ) {
                        changes.push(field(FieldChange::Power(at, row, true)));
                    }
                });
            }
            if classes.len() < 3 && ui.button("Add a class").clicked() {
                changes.push(field(FieldChange::Class(classes.len(), Some((0, 1)))));
            }
        });
    let feats = fields::feats(gff);
    let feat_rows = table_rows(state, ui_state, "feat");
    egui::CollapsingHeader::new(format!("Feats ({})", feats.len()))
        .id_salt("template feats")
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for feat in &feats {
                    if ui
                        .small_button(format!("{} x", row_name(&feat_rows, *feat)))
                        .on_hover_text("Takes the feat away.")
                        .clicked()
                    {
                        changes.push(field(FieldChange::Feat(*feat, false)));
                    }
                }
            });
            if let Some(row) = row_picker(
                ui,
                ui_state,
                &feat_rows,
                ("feat to add", "Add a feat"),
                None,
            ) {
                changes.push(field(FieldChange::Feat(row, true)));
            }
        });
    let skills = fields::skills(gff);
    let skill_rows = table_rows(state, ui_state, "skills");
    egui::CollapsingHeader::new("Skills")
        .id_salt("template skills")
        .show(ui, |ui| {
            egui::Grid::new("template skills grid")
                .num_columns(2)
                .show(ui, |ui| {
                    for at in 0..8usize {
                        let name = skill_rows
                            .iter()
                            .find(|(row, _)| usize::from(*row) == at)
                            .map_or_else(|| format!("Skill {at}"), |(_, name)| name.clone());
                        ui.label(name);
                        let rank = skills.get(at).copied().unwrap_or(0);
                        if let Some(value) = number_field(
                            ui,
                            ui_state,
                            &format!("skill {at}"),
                            (f64::from(rank), 0.0, 100.0, true),
                        ) {
                            changes.push(field(FieldChange::Skill(at, value as u8)));
                        }
                        ui.end_row();
                    }
                });
        });
    let equipped = fields::equipped(gff);
    egui::CollapsingHeader::new(format!("Equipment ({})", equipped.len()))
        .id_salt("template equipment")
        .show(ui, |ui| {
            egui::Grid::new("template equipment grid")
                .num_columns(2)
                .show(ui, |ui| {
                    for (slot, shown) in fields::SLOTS {
                        let now = equipped
                            .iter()
                            .find(|(at, _)| *at == slot)
                            .map(|(_, item)| item.clone())
                            .unwrap_or_else(String::new);
                        ui.label(shown);
                        let key = format!("slot {slot}");
                        let typed = ui_state.typed.entry(key.clone()).or_insert_with(|| now.clone());
                        let known = typed.is_empty() || crate::choose::item_known(state, typed);
                        if !known {
                            ui.visuals_mut().override_text_color = Some(egui::Color32::from_rgb(230, 120, 90));
                        }
                        let edited = crate::choose::field(ui, &key, typed, ("none", 150.0), || crate::choose::items(state));
                        ui.visuals_mut().override_text_color = None;
                        let done = edited.done();
                        edited.response.on_hover_text(
                            "The item the creature has here. Type a part of an item's name and pick it, or empty the field for none.",
                        );
                        let known = typed.is_empty() || crate::choose::item_known(state, typed);
                        if done && known && typed.to_ascii_lowercase() != now {
                            changes.push(field(FieldChange::Equip(slot, typed.to_ascii_lowercase())));
                        }
                        ui.end_row();
                    }
                });
        });
}

/// An encounter's creatures and its spawn points.
fn encounter_lists(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    ui_state: &mut UiState,
    target: Target,
    gff: &fmt_odyssey::gff::Gff,
    changes: &mut Vec<Change>,
) {
    use level_edit::fields::{self, FieldChange, Spawn};
    let field = |change: FieldChange| Change::Field(change);
    let spawns = fields::spawns(gff);
    egui::CollapsingHeader::new(format!("Creatures it spawns ({})", spawns.len()))
        .id_salt("template spawns")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("template spawns grid")
                .num_columns(4)
                .show(ui, |ui| {
                    for (at, spawn) in spawns.iter().enumerate() {
                        ui.label(&spawn.template);
                        ui.label("challenge");
                        if let Some(value) = number_field(
                            ui,
                            ui_state,
                            &format!("spawn {at} cr"),
                            (f64::from(spawn.cr), 0.0, 100.0, false),
                        ) {
                            changes.push(field(FieldChange::Spawn(
                                at,
                                Some(Spawn {
                                    cr: value as f32,
                                    ..spawn.clone()
                                }),
                            )));
                        }
                        ui.horizontal(|ui| {
                            let mut single = spawn.single;
                            if ui
                                .checkbox(&mut single, "Once")
                                .on_hover_text("The encounter spawns this creature once at most.")
                                .changed()
                            {
                                changes.push(field(FieldChange::Spawn(
                                    at,
                                    Some(Spawn {
                                        single,
                                        ..spawn.clone()
                                    }),
                                )));
                            }
                            if ui.small_button("Remove").clicked() {
                                changes.push(field(FieldChange::Spawn(at, None)));
                            }
                        });
                        ui.end_row();
                    }
                });
            ui.horizontal(|ui| {
                let key = "spawn to add".to_string();
                let typed = ui_state
                    .typed
                    .entry(key.clone())
                    .or_insert_with(String::new);
                // A creature picked from the list is added at once.
                let edited =
                    crate::choose::field(ui, &key, typed, ("a creature's name", 180.0), || {
                        crate::choose::creatures(state)
                    });
                let name = typed.trim().to_ascii_lowercase();
                let ready = state.setup.creatures.binary_search(&name).is_ok();
                let add = ui
                    .add_enabled(ready, egui::Button::new("Add creature"))
                    .clicked();
                if ready && (add || edited.picked.is_some()) {
                    // It counts for the challenge its template says.
                    let cr = state
                        .template_gff(&name, ResType::UTC)
                        .and_then(|creature| creature.root.float("ChallengeRating"))
                        .unwrap_or(1.0);
                    changes.push(field(FieldChange::Spawn(
                        spawns.len(),
                        Some(Spawn {
                            template: name,
                            cr,
                            single: false,
                        }),
                    )));
                    ui_state.typed.remove(&key);
                }
            });
        });
    // The spawn points, in the area's object list.
    let Target::Object(_, index) = target else {
        return;
    };
    let points = level_edit::git::spawn_points(&state.editor.project.git, index);
    egui::CollapsingHeader::new(format!("Spawn points ({})", points.len()))
        .id_salt("template spawn points")
        .default_open(true)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "The creatures appear at these points. With none, they appear where the party walks in.",
                )
                .small(),
            );
            let mut moved = points.clone();
            let mut changed = false;
            let mut remove = None;
            egui::Grid::new("spawn points grid")
                .num_columns(3)
                .show(ui, |ui| {
                    for (at, (point, facing)) in moved.iter_mut().enumerate() {
                        ui.label(format!("{}", at + 1));
                        ui.horizontal(|ui| {
                            for value in point.iter_mut().take(2) {
                                changed |= ui
                                    .add(egui::DragValue::new(value).speed(0.05).max_decimals(2))
                                    .changed();
                            }
                            let mut degrees = facing.to_degrees();
                            if ui
                                .add(
                                    egui::DragValue::new(&mut degrees)
                                        .speed(1.0)
                                        .max_decimals(0)
                                        .suffix("°"),
                                )
                                .on_hover_text("The way the creature faces as it appears.")
                                .changed()
                            {
                                *facing = degrees.to_radians();
                                changed = true;
                            }
                        });
                        if ui.small_button("Remove").clicked() {
                            remove = Some(at);
                        }
                        ui.end_row();
                    }
                });
            if let Some(at) = remove {
                moved.remove(at);
                changed = true;
            }
            if changed {
                if ui.ctx().input(|input| input.pointer.any_down()) {
                    drag_edit(state, ui_state, Edit::SpawnPoints(target, moved));
                } else {
                    state.apply(Edit::SpawnPoints(target, moved));
                }
            }
            let placing = ui_state.spawning == Some(target);
            if ui
                .selectable_label(placing, "Place spawn points")
                .on_hover_text("Each click on the floor in the view adds a spawn point. Enter or Escape ends.")
                .clicked()
            {
                ui_state.spawning = (!placing).then_some(target);
            }
        });
}

/// The rows of a 2DA as a list shows them: each row's number and its
/// `Resource` (the sound's file), or its label.
fn sound_rows(state: &EditorState, table: &str) -> Vec<(u32, String)> {
    let Some(table) = state.setup.resolver.two_da(table) else {
        return Vec::new();
    };
    (0..table.row_labels.len())
        .map(|row| {
            let name = table
                .text(row, "Resource")
                .filter(|name| !name.is_empty() && name != "****")
                .or_else(|| table.text(row, "Label"))
                .unwrap_or_else(|| "none".to_string());
            (row as u32, name)
        })
        .collect()
}

/// The area's weather (`level_edit::settings`): its chances of rain, snow
/// and lightning, its wind, whether it is indoors, and the rooms that keep
/// the weather out.
fn weather_settings(
    ui: &mut egui::Ui,
    are: &fmt_odyssey::gff::Gff,
    asked: &mut Vec<level_edit::settings::AreaChange>,
) {
    use fmt_odyssey::gff::GffValue;
    use level_edit::settings::{AreaChange, INDOORS, WEATHER_CHANCES, WIND, WIND_MOST};
    let flags = are.root.uint("Flags").unwrap_or(0);
    let number = |label: &str| are.root.int(label).unwrap_or(0) as i32;
    egui::Grid::new("area weather").num_columns(2).show(ui, |ui| {
        ui.label("Indoors");
        let mut indoors = flags & INDOORS != 0;
        if ui
            .checkbox(&mut indoors, "no weather")
            .on_hover_text("An area marked as indoors has no rain, snow or lightning.")
            .changed()
        {
            let flags = if indoors {
                flags | INDOORS
            } else {
                flags & !INDOORS
            };
            asked.push(AreaChange::Are("Flags".into(), GffValue::Dword(flags)));
        }
        ui.end_row();
        if !indoors {
            for (label, name) in WEATHER_CHANCES {
                let mut chance = number(label).clamp(0, 100);
                ui.label(format!("Chance of {name}"));
                if ui
                    .add(egui::Slider::new(&mut chance, 0..=100).suffix(" %"))
                    .on_hover_text(
                        "How often it falls: every 5 minutes of play the area rolls again. Rain wins over snow.",
                    )
                    .changed()
                {
                    asked.push(AreaChange::Are(label.into(), GffValue::Int(chance)));
                }
                ui.end_row();
            }
            let mut wind = number(WIND).clamp(0, WIND_MOST);
            ui.label("Wind");
            if ui
                .add(egui::Slider::new(&mut wind, 0..=WIND_MOST))
                .on_hover_text("0 is still. The wind blows the rain, the snow and the grass.")
                .changed()
            {
                asked.push(AreaChange::Are(WIND.into(), GffValue::Int(wind)));
            }
            ui.end_row();
        }
    });
    let rooms = level_edit::settings::dry_rooms(are);
    if flags & INDOORS == 0 && !rooms.is_empty() {
        egui::CollapsingHeader::new(format!(
            "Rooms that keep the weather out ({})",
            rooms.iter().filter(|(_, dry)| *dry).count()
        ))
        .id_salt("dry rooms")
        .show(ui, |ui| {
            for (name, dry) in rooms {
                let mut kept = dry;
                if ui.checkbox(&mut kept, &name).changed() {
                    asked.push(AreaChange::DryRoom(name, kept));
                }
            }
        });
    }
}

/// This level's own settings: music and ambient sound, fog and ambient
/// light, its weather, its scripts, and a new map's module scripts.
fn area_settings(ui: &mut egui::Ui, state: &mut EditorState, ui_state: &mut UiState) {
    use fmt_odyssey::gff::GffValue;
    use level_edit::settings::{AREA_SCRIPTS, AreaChange, MODULE_SCRIPTS, colour_of, colour_value};
    ui.separator();
    ui.label(egui::RichText::new("This level").strong());
    let mut asked: Vec<AreaChange> = Vec::new();
    let git = state.editor.project.git.clone();
    let music = sound_rows(state, "ambientmusic");
    let ambience = sound_rows(state, "ambientsound");
    let playing = state.listening.what().map(String::from);
    // The music or ambient sound to play (its field), or `None` to stop.
    let mut listen: Option<Option<String>> = None;
    egui::Grid::new("area sound").num_columns(3).show(ui, |ui| {
        let mut row_combo =
            |ui: &mut egui::Ui, label: &str, shown: &str, rows: &[(u32, String)]| {
                let now = level_edit::settings::property(&git, label);
                ui.label(shown);
                let name = rows
                    .iter()
                    .find(|(row, _)| *row == now)
                    .map_or_else(|| format!("{now}"), |(row, name)| format!("{row} {name}"));
                egui::ComboBox::from_id_salt(label)
                    .selected_text(name)
                    .height(320.0)
                    .show_ui(ui, |ui| {
                        for (row, name) in rows {
                            if ui
                                .selectable_label(*row == now, format!("{row} {name}"))
                                .clicked()
                            {
                                asked.push(AreaChange::Property(label.to_string(), *row));
                            }
                        }
                    });
                listen_button(
                    ui,
                    playing.as_deref() == Some(label),
                    now != 0,
                    label,
                    &mut listen,
                );
                ui.end_row();
            };
        row_combo(ui, "MusicDay", "Music by day", &music);
        row_combo(ui, "MusicNight", "Music by night", &music);
        row_combo(ui, "MusicBattle", "Music in battle", &music);
        row_combo(ui, "AmbientSndDay", "Ambient sound by day", &ambience);
        row_combo(ui, "AmbientSndNight", "Ambient sound by night", &ambience);
        for (label, shown) in [
            ("AmbientSndDayVol", "Its volume by day"),
            ("AmbientSndNitVol", "Its volume by night"),
        ] {
            let mut volume = level_edit::settings::property(&git, label).min(127);
            ui.label(shown);
            if ui.add(egui::Slider::new(&mut volume, 0..=127)).changed() {
                asked.push(AreaChange::Property(label.to_string(), volume));
            }
            ui.label("");
            ui.end_row();
        }
    });
    if let Some(are) = state::area_are(state) {
        let dword = |label: &str| are.root.uint(label).unwrap_or(0);
        let float = |label: &str| are.root.float(label).unwrap_or(0.0);
        egui::Grid::new("area look").num_columns(2).show(ui, |ui| {
            ui.label("Fog");
            let mut on = dword("SunFogOn") != 0;
            if ui.checkbox(&mut on, "on").changed() {
                asked.push(AreaChange::Are(
                    "SunFogOn".into(),
                    GffValue::Byte(u8::from(on)),
                ));
            }
            ui.end_row();
            if on {
                for (label, shown) in [
                    ("SunFogNear", "Fog starts at"),
                    ("SunFogFar", "Fog is thick at"),
                ] {
                    let mut metres = float(label);
                    ui.label(shown);
                    if ui
                        .add(
                            egui::DragValue::new(&mut metres)
                                .speed(0.5)
                                .range(0.0..=2000.0)
                                .suffix(" m"),
                        )
                        .changed()
                    {
                        asked.push(AreaChange::Are(
                            label.into(),
                            GffValue::Float(metres.to_bits()),
                        ));
                    }
                    ui.end_row();
                }
            }
            for (label, shown) in [
                ("SunFogColor", "Fog colour"),
                ("DynAmbientColor", "Ambient light"),
            ] {
                let mut rgb = colour_of(dword(label));
                ui.label(shown);
                if ui.color_edit_button_rgb(&mut rgb).changed() {
                    asked.push(AreaChange::Are(
                        label.into(),
                        GffValue::Dword(colour_value(rgb)),
                    ));
                }
                ui.end_row();
            }
        });
        weather_settings(ui, &are, &mut asked);
        ui.label(egui::RichText::new("The level's scripts").small());
        egui::Grid::new("area scripts")
            .num_columns(2)
            .show(ui, |ui| {
                for label in AREA_SCRIPTS {
                    let now = are
                        .root
                        .resref(label)
                        .map(|name| name.to_ascii_lowercase())
                        .unwrap_or_else(String::new);
                    ui.label(label);
                    let typed = ui_state
                        .area_typed
                        .entry(label.to_string())
                        .or_insert_with(|| now.clone());
                    let field = ui.add(
                        egui::TextEdit::singleline(typed)
                            .desired_width(150.0)
                            .hint_text("none"),
                    );
                    let typed = typed.clone();
                    if !field.has_focus() {
                        // Shown as the area has it while not being typed in.
                        ui_state.area_typed.remove(label);
                    }
                    if field.lost_focus() && typed.to_ascii_lowercase() != now {
                        asked.push(AreaChange::Are(
                            label.into(),
                            GffValue::ResRef(typed.trim().to_ascii_lowercase().into_bytes()),
                        ));
                    }
                    ui.end_row();
                }
            });
    }
    // A new map's sky: one of Jedi Academy's sky boxes, or none.
    if let Some(map) = &state.editor.project.map
        && !state.setup.ja_skies.is_empty()
    {
        let now = map.ja_sky.clone();
        let shown = |sky: &str| sky.trim_start_matches("textures/").to_string();
        egui::Grid::new("map sky").num_columns(2).show(ui, |ui| {
            ui.label("Sky");
            egui::ComboBox::from_id_salt("ja sky")
                .selected_text(now.as_deref().map_or_else(|| "none".to_string(), shown))
                .height(320.0)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(now.is_none(), "none").clicked() && now.is_some() {
                        asked.push(AreaChange::JaSky(None));
                    }
                    for sky in &state.setup.ja_skies {
                        let chosen = now.as_deref() == Some(sky.as_str());
                        if ui.selectable_label(chosen, shown(sky)).clicked() && !chosen {
                            asked.push(AreaChange::JaSky(Some(sky.clone())));
                        }
                    }
                })
                .response
                .on_hover_text(
                    "A sky box of Jedi Academy, drawn round the map. kjk reads its six pictures from the player's own Jedi Academy.",
                );
            ui.end_row();
        });
    }
    if let Some(map) = &state.editor.project.map {
        let scripts = map.module_scripts.clone();
        ui.label(egui::RichText::new("The module's scripts").small());
        egui::Grid::new("module scripts")
            .num_columns(2)
            .show(ui, |ui| {
                for label in MODULE_SCRIPTS {
                    let now = scripts.get(label).cloned().unwrap_or_else(String::new);
                    ui.label(label.trim_start_matches("Mod_"));
                    let typed = ui_state
                        .area_typed
                        .entry(label.to_string())
                        .or_insert_with(|| now.clone());
                    let field = ui.add(
                        egui::TextEdit::singleline(typed)
                            .desired_width(150.0)
                            .hint_text("none"),
                    );
                    let typed = typed.clone();
                    if !field.has_focus() {
                        // Shown as the area has it while not being typed in.
                        ui_state.area_typed.remove(label);
                    }
                    if field.lost_focus() && typed.to_ascii_lowercase() != now {
                        asked.push(AreaChange::ModuleScript(
                            label.into(),
                            typed.trim().to_ascii_lowercase(),
                        ));
                    }
                    ui.end_row();
                }
            });
    } else {
        ui.label(
            egui::RichText::new(
                "An installed module's own scripts stay as the game has them: every module.ifo has one name.",
            )
            .small(),
        );
    }
    // A change to the music or ambient sound playing plays the new one, at
    // its new volume.
    let replay = playing.as_deref().filter(|label| {
        asked.iter().any(|change| {
            matches!(change, AreaChange::Property(field, _)
                if field == label || (state::HEARD_SLOTS.iter())
                    .any(|(slot, volume, _)| slot == label && *volume == Some(field.as_str())))
        })
    });
    let replay = replay.map(String::from);
    for change in asked {
        state::change_area(state, change);
    }
    if let Some(label) = replay {
        listen = Some(Some(label));
    }
    match listen {
        Some(Some(label)) => {
            if let Err(why) = state::listen_area(state, &label) {
                state.say(why);
            }
        }
        Some(None) => state.listening.stop(),
        None => {}
    }
}

/// A button that plays the area's music or ambient sound of the field
/// `label`, or stops it while it plays. A field set to row 0 (none) has
/// nothing to play.
fn listen_button(
    ui: &mut egui::Ui,
    playing: bool,
    has_sound: bool,
    label: &str,
    listen: &mut Option<Option<String>>,
) {
    if playing {
        if ui
            .button("Stop")
            .on_hover_text("Stops playing it.")
            .clicked()
        {
            *listen = Some(None);
        }
    } else if ui
        .add_enabled(has_sound, egui::Button::new("Play"))
        .on_hover_text("Plays it in the editor, at the level the game plays it at.")
        .clicked()
    {
        *listen = Some(Some(label.to_string()));
    }
}

/// The selected sound object's Play button: it plays the object's sounds
/// as they sound next to the player, or stops them.
fn listen_object(ui: &mut egui::Ui, state: &mut EditorState, template: &str) {
    let key = state::listen_key(template);
    ui.horizontal(|ui| {
        if state.listening.is(&key) {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
            let file = match (state.listening.file(), state.listening.waits()) {
                (Some(file), _) => file.to_string(),
                (None, Some(seconds)) => format!("the next sound in {seconds} s"),
                (None, None) => String::new(),
            };
            if ui.button("Stop").clicked() {
                state.listening.stop();
            }
            ui.label(egui::RichText::new(file).small());
        } else if ui
            .button("Play its sounds")
            .on_hover_text(
                "Plays the object's sounds in the editor, as they sound next to the player, with the pauses between them.",
            )
            .clicked()
            && let Err(why) = state::listen_template(state, template)
        {
            state.say(why);
        }
    });
}

/// The logic blocks an object runs on `event`: each block's values, moved
/// up or down or removed, and a block added from the list.
fn blocks_panel(ui: &mut egui::Ui, state: &mut EditorState, target: Target, event: &str) {
    use level_edit::blocks::{self, Block, Param};
    ui.separator();
    ui.label(egui::RichText::new(format!("Blocks on {event}")).strong());
    let (script, held) = match state::event_blocks(state, target, event) {
        Ok(found) => found,
        Err(why) => {
            ui.label(why);
            return;
        }
    };
    let Some(mut list) = held else {
        ui.label(format!(
            "{event} runs {script}, which is written by hand or is the game's, not made of blocks."
        ));
        if ui
            .button("Replace it with blocks")
            .on_hover_text(
                "The event runs a new script made of blocks; the old script is left as it is.",
            )
            .clicked()
            && let Err(why) = state::set_blocks(state, target, event, Vec::new())
        {
            state.say(why);
        }
        return;
    };
    ui.label(
        egui::RichText::new(if script.is_empty() {
            "The event runs nothing yet. Add a block, and the blocks run in order from the top.".to_string()
        } else {
            format!("Written as the script {script}, compiled when the project is saved. The blocks run in order from the top.")
        })
        .small(),
    );
    let globals = state.setup.globals.clone();
    let mut changed = false;
    let mut moved: Option<(usize, i32)> = None;
    let mut removed: Option<usize> = None;
    let count = list.len();
    for (at, block) in list.iter_mut().enumerate() {
        let Some(kind) = blocks::kind(&block.key) else {
            ui.label(format!("{}: not a kind of block", block.key));
            continue;
        };
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(format!("{}. {}", at + 1, kind.title)).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Remove").clicked() {
                        removed = Some(at);
                    }
                    if at + 1 < count && ui.small_button("Down").clicked() {
                        moved = Some((at, 1));
                    }
                    if at > 0 && ui.small_button("Up").clicked() {
                        moved = Some((at, -1));
                    }
                });
            });
            egui::Grid::new(("block", at))
                .num_columns(2)
                .show(ui, |ui| {
                    for (n, (label, param)) in kind.params.iter().enumerate() {
                        if block.values.len() <= n {
                            block.values.push(String::new());
                        }
                        let value = &mut block.values[n];
                        ui.label(*label);
                        match param {
                            Param::GlobalNumber | Param::GlobalBoolean => {
                                let wanted = if *param == Param::GlobalNumber {
                                    "number"
                                } else {
                                    "boolean"
                                };
                                egui::ComboBox::from_id_salt(("global", at, n))
                                    .selected_text(if value.is_empty() {
                                        "pick one"
                                    } else {
                                        value.as_str()
                                    })
                                    .height(320.0)
                                    .show_ui(ui, |ui| {
                                        for (name, kind) in
                                            globals.iter().filter(|(_, kind)| kind == wanted)
                                        {
                                            let _ = kind;
                                            if ui.selectable_label(*value == *name, name).clicked()
                                            {
                                                *value = name.clone();
                                                changed = true;
                                            }
                                        }
                                    });
                            }
                            Param::Truth => {
                                let mut on = matches!(value.as_str(), "1" | "true" | "TRUE");
                                if ui.checkbox(&mut on, "true").changed() {
                                    *value = if on { "1" } else { "0" }.to_string();
                                    changed = true;
                                }
                            }
                            _ => {
                                let width = if *param == Param::Text { 220.0 } else { 150.0 };
                                changed |= ui
                                    .add(egui::TextEdit::singleline(value).desired_width(width))
                                    .changed();
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    }
    if let Some(at) = removed {
        list.remove(at);
        changed = true;
    }
    if let Some((at, by)) = moved {
        let to = (at as i32 + by) as usize;
        list.swap(at, to);
        changed = true;
    }
    let mut added = None;
    egui::ComboBox::from_id_salt("add a block")
        .selected_text("Add a block")
        .height(360.0)
        .show_ui(ui, |ui| {
            for kind in blocks::KINDS {
                if ui.selectable_label(false, kind.title).clicked() {
                    added = Some(Block::new(kind));
                }
            }
        });
    if let Some(block) = added {
        list.push(block);
        changed = true;
    }
    for problem in blocks::problems(&list) {
        ui.colored_label(ui.visuals().warn_fg_color, problem);
    }
    if let Some(why) = state.script_errors.get(&script) {
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!("It did not compile: {why}"),
        );
    }
    if changed && let Err(why) = state::set_blocks(state, target, event, list) {
        state.say(why);
    }
}

/// How a module is named in a list of where a link leads.
fn module_shown(own: &str, module: &str) -> String {
    match module {
        "" => "nowhere".to_string(),
        _ if module == own => format!("{module} (this level)"),
        _ => module.to_string(),
    }
}

/// How an arrival is named: its tag, and whether it is a door or a
/// waypoint; the module's entry for none.
fn arrival_shown(tag: &str, door: Option<bool>) -> String {
    match (tag, door) {
        ("", _) => "the module's entry".to_string(),
        (tag, Some(true)) => format!("{tag} (door)"),
        (tag, Some(false)) => format!("{tag} (waypoint)"),
        (tag, None) => tag.to_string(),
    }
}

/// Where the selected door or trigger leads: a module, and the door or
/// waypoint there to arrive at.
fn leads_to(ui: &mut egui::Ui, state: &mut EditorState, (kind, index): (Kind, usize)) {
    let Some(item) = level_edit::git::item(&state.editor.project.git, kind, index) else {
        return;
    };
    let link = level_edit::template::link_of(item);
    let own = state.editor.project.module.clone();
    ui.separator();
    ui.label(egui::RichText::new("Leads to").strong());
    ui.label(
        egui::RichText::new(if kind == Kind::Door {
            "Used, the door takes the party to another level, or to another place in this one."
        } else {
            "Walked into, the trigger takes the party to another level, or to another place in this one."
        })
        .small(),
    );
    let mut asked: Option<(String, String)> = None;
    egui::Grid::new("leads to").num_columns(2).show(ui, |ui| {
        ui.label("Level");
        egui::ComboBox::from_id_salt("leads to module")
            .selected_text(module_shown(&own, &link.module))
            .height(360.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(!link.leads(), "nowhere").clicked() {
                    asked = Some((String::new(), String::new()));
                }
                let modules = std::iter::once(own.clone())
                    .chain(state.setup.modules.iter().filter(|m| **m != own).cloned());
                for module in modules {
                    if ui
                        .selectable_label(link.module == module, module_shown(&own, &module))
                        .clicked()
                    {
                        asked = Some((module, String::new()));
                    }
                }
            });
        ui.end_row();
        if link.leads() {
            ui.label("Arrive at");
            match state::arrivals(state, &link.module) {
                Ok(arrivals) => {
                    let door = arrivals
                        .iter()
                        .find(|(tag, _)| tag.eq_ignore_ascii_case(&link.to))
                        .map(|(_, door)| *door);
                    egui::ComboBox::from_id_salt("leads to arrival")
                        .selected_text(arrival_shown(&link.to, door))
                        .height(360.0)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(link.to.is_empty(), arrival_shown("", None))
                                .clicked()
                            {
                                asked = Some((link.module.clone(), String::new()));
                            }
                            for (tag, door) in &arrivals {
                                if ui
                                    .selectable_label(
                                        link.to.eq_ignore_ascii_case(tag),
                                        arrival_shown(tag, Some(*door)),
                                    )
                                    .clicked()
                                {
                                    asked = Some((link.module.clone(), tag.clone()));
                                }
                            }
                        });
                }
                Err(why) => {
                    ui.label(egui::RichText::new(why).small());
                }
            }
            ui.end_row();
        }
    });
    if let Some((module, tag)) = asked
        && let Err(why) = state::link(state, &module, &tag)
    {
        state.say(why);
    }
}

/// An area of another level the project changes: its resref, its module,
/// and its doors and triggers that lead into this level.
type HeldWay = (
    String,
    String,
    Vec<(Kind, String, level_edit::template::Link)>,
);

/// Ways in from other levels: doors and triggers of other modules' entry
/// areas that this project leads into its level (`Project::others`).
fn ways_in(ui: &mut egui::Ui, state: &mut EditorState, ui_state: &mut UiState) {
    let own = state.editor.project.module.clone();
    ui.separator();
    ui.label(egui::RichText::new("Ways in from other levels").strong());
    ui.label(
        egui::RichText::new(
            "A door or trigger of another level can lead here. The project keeps a copy of that \
             level's object list with the change, which kjk plays wherever that level is loaded.",
        )
        .small(),
    );
    let mut asked: Option<String> = None;
    let held: Vec<HeldWay> = state
        .editor
        .project
        .others
        .iter()
        .map(|(area, other)| {
            let leading = level_edit::git::exits(&other.git)
                .into_iter()
                .filter(|(_, _, _, link)| link.module == own)
                .map(|(kind, _, tag, link)| (kind, tag, link))
                .collect();
            (area.clone(), other.module.clone(), leading)
        })
        .collect();
    for (area, module, leading) in &held {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("{module} ({area}):"));
            for (kind, tag, link) in leading {
                ui.label(format!(
                    "{} {tag} to {}",
                    if *kind == Kind::Door { "door" } else { "trigger" },
                    arrival_shown(&link.to, None)
                ));
            }
            if ui
                .small_button("Give back")
                .on_hover_text("The project no longer changes this level: its doors lead where the game has them.")
                .clicked()
            {
                asked = Some(format!("entrance drop {area}"));
            }
        });
    }
    // A new way in: a module, then one of its doors or triggers.
    ui.horizontal(|ui| {
        ui.label("From");
        let shown = ui_state
            .way_in_module
            .clone()
            .unwrap_or_else(|| "a level".to_string());
        egui::ComboBox::from_id_salt("way in module")
            .selected_text(shown)
            .height(360.0)
            .show_ui(ui, |ui| {
                for module in state.setup.modules.iter().filter(|m| **m != own) {
                    if ui
                        .selectable_label(ui_state.way_in_module.as_ref() == Some(module), module)
                        .clicked()
                    {
                        ui_state.way_in_module = Some(module.clone());
                    }
                }
            });
    });
    let here = level_edit::git::arrivals(&state.editor.project.git);
    ui.horizontal(|ui| {
        ui.label("Arriving at");
        let door = here
            .iter()
            .find(|(tag, _)| *tag == ui_state.way_in_arrival)
            .map(|(_, door)| *door);
        egui::ComboBox::from_id_salt("way in arrival")
            .selected_text(arrival_shown(&ui_state.way_in_arrival, door))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut ui_state.way_in_arrival,
                    String::new(),
                    arrival_shown("", None),
                );
                for (tag, door) in &here {
                    ui.selectable_value(
                        &mut ui_state.way_in_arrival,
                        tag.clone(),
                        arrival_shown(tag, Some(*door)),
                    );
                }
            });
    });
    if here.is_empty() {
        ui.label(
            egui::RichText::new(
                "Place a waypoint in this level to arrive at a place of your choosing.",
            )
            .small(),
        );
    }
    if let Some(module) = ui_state.way_in_module.clone() {
        match state::module_area(state, &module) {
            Ok((area, git)) => {
                let git = state
                    .editor
                    .project
                    .others
                    .get(&area)
                    .map_or(git, |other| other.git.clone());
                let exits = level_edit::git::exits(&git);
                if exits.is_empty() {
                    ui.label(format!("{module}'s area {area} has no doors or triggers."));
                }
                egui::ScrollArea::vertical()
                    .id_salt("way in exits")
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for (kind, index, tag, link) in exits {
                            ui.horizontal(|ui| {
                                let word = if kind == Kind::Door {
                                    "door"
                                } else {
                                    "trigger"
                                };
                                let now = if link.leads() {
                                    format!("to {}", module_shown(&own, &link.module))
                                } else {
                                    "leads nowhere".to_string()
                                };
                                ui.label(format!("{word} {tag}, {now}"));
                                if ui.small_button("Lead here").clicked() {
                                    let tag = &ui_state.way_in_arrival;
                                    if let Err(why) = state::entrance(
                                        state,
                                        &module,
                                        (word, &index.to_string()),
                                        tag,
                                    ) {
                                        state.say(why);
                                    }
                                }
                            });
                        }
                    });
            }
            Err(why) => {
                ui.label(egui::RichText::new(why).small());
            }
        }
    }
    if let Some(asked) = asked {
        state.pending_command = Some(asked);
    }
}

/// The Conversations menu: the project's conversations, each opened in
/// the conversation window, and a new one made or one of the game's
/// copied.
fn conversations_menu(ui: &mut egui::Ui, state: &mut EditorState, ui_state: &mut UiState) {
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new("Conversations")
        .config(config)
        .ui(ui, |ui| {
            let names: Vec<String> = state.editor.project.conversations.keys().cloned().collect();
            if names.is_empty() {
                ui.label("The project has no conversations of its own yet.");
            }
            for name in names {
                if ui.button(&name).clicked() {
                    state.pending_ui.push(vec!["conversation".into(), name]);
                    ui.close();
                }
            }
            ui.separator();
            ui.label(
                "A conversation's name, 1 to 16 letters, digits and _. Type a part of a game's \
                 conversation's name to find it and copy it.",
            );
            crate::choose::field(
                ui,
                "new conversation",
                &mut ui_state.new_dialog,
                ("kje_talk", 160.0),
                || crate::choose::game_conversations(state),
            );
            let name = ui_state.new_dialog.trim().to_ascii_lowercase();
            let good = !name.is_empty()
                && name.len() <= 16
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
            let the_games = good && state.setup.resolver.find(&name, ResType::DLG).is_some();
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(good, egui::Button::new("New"))
                    .on_hover_text("A conversation with no lines yet.")
                    .clicked()
                {
                    state.pending_command = Some(format!("conversation new {name}"));
                    ui_state.new_dialog.clear();
                    ui.close();
                }
                if ui
                    .add_enabled(the_games, egui::Button::new("Open the game's"))
                    .on_hover_text("Opens the game's conversation of this name to be read. The project does not change.")
                    .clicked()
                {
                    state.pending_ui.push(vec!["conversation".into(), name.clone()]);
                    ui_state.new_dialog.clear();
                    ui.close();
                }
                if ui
                    .add_enabled(the_games, egui::Button::new("Copy the game's"))
                    .on_hover_text(
                        "Copies the game's conversation of this name into the project, where it is \
                         changed and played in the place of the game's.",
                    )
                    .clicked()
                {
                    state.pending_command = Some(format!("conversation copy {name}"));
                    ui_state.new_dialog.clear();
                    ui.close();
                }
            });
        });
}

/// One node of a conversation's tree: the node, the link that led to it,
/// whether it is shown already above (then not opened again), the link's
/// condition, and the nodes it leads to.
struct TalkNode {
    side: level_edit::dialog::Side,
    index: usize,
    via: Option<level_edit::dialog::LinkAt>,
    again: bool,
    condition: String,
    children: Vec<TalkNode>,
}

/// The tree of `dlg`: one branch for each opening (the starting list's
/// links, in the order the game tries them), each node opened the first
/// time it is reached; then the nodes no link reaches.
fn talk_tree(dlg: &fmt_odyssey::dlg::Dlg) -> (Vec<TalkNode>, Vec<TalkNode>) {
    use level_edit::dialog::{LinkAt, Side};
    fn visit(
        dlg: &fmt_odyssey::dlg::Dlg,
        (side, index, depth): (Side, usize, usize),
        via: Option<(LinkAt, &fmt_odyssey::dlg::Link)>,
        seen: &mut std::collections::BTreeSet<(Side, usize)>,
    ) -> TalkNode {
        let child = via.is_some_and(|(_, link)| link.is_child);
        let again = !seen.insert((side, index)) || child || depth > 60;
        let mut made = TalkNode {
            side,
            index,
            via: via.map(|(at, _)| at),
            again,
            condition: via.map_or_else(String::new, |(_, link)| link.active.script.clone()),
            children: Vec::new(),
        };
        if again {
            return made;
        }
        let nodes = if side == Side::Entry {
            &dlg.entries
        } else {
            &dlg.replies
        };
        if let Some(node) = nodes.get(index) {
            for (n, link) in node.links.iter().enumerate() {
                made.children.push(visit(
                    dlg,
                    (side.other(), link.index as usize, depth + 1),
                    Some((LinkAt::From(side, index, n), link)),
                    seen,
                ));
            }
        }
        made
    }
    let mut seen = std::collections::BTreeSet::new();
    let openings = dlg
        .starts
        .iter()
        .enumerate()
        .map(|(n, link)| {
            visit(
                dlg,
                (Side::Entry, link.index as usize, 0),
                Some((LinkAt::Start(n), link)),
                &mut seen,
            )
        })
        .collect();
    let mut lost = Vec::new();
    for (side, count) in [
        (Side::Entry, dlg.entries.len()),
        (Side::Reply, dlg.replies.len()),
    ] {
        for index in 0..count {
            if !seen.contains(&(side, index)) {
                lost.push(visit(dlg, (side, index, 0), None, &mut seen));
            }
        }
    }
    (openings, lost)
}

/// What a condition script checks, said shortly from its compiled form:
/// the globals and tags it reads, else the routines it calls.
fn condition_checks(state: &EditorState, script: &str) -> String {
    let view = crate::script_view::read(
        &state.setup.resolver,
        state.setup.catalog.as_ref(),
        &state.editor.project.folder,
        script,
    );
    if view.listing.is_none() {
        return "a script that is not compiled".to_string();
    }
    let strings: Vec<&str> = view.strings.iter().map(String::as_str).take(4).collect();
    if !strings.is_empty() {
        return format!("reads {}", strings.join(", "));
    }
    let calls: Vec<String> = view
        .calls
        .iter()
        .filter_map(|(call, _)| {
            let name = call.split('(').next()?.split_whitespace().last()?;
            (!name.is_empty()).then(|| name.to_string())
        })
        .take(3)
        .collect();
    if calls.is_empty() {
        "checks nothing it names".to_string()
    } else {
        format!("calls {}", calls.join(", "))
    }
}

/// What a row of the conversation's tree is read from: the conversation,
/// what a node says, and the tags of the level's objects a script names.
type TalkRead<'a> = (
    &'a fmt_odyssey::dlg::Dlg,
    &'a dyn Fn(&fmt_odyssey::dlg::Node) -> String,
    &'a dyn Fn(&str) -> std::sync::Arc<Vec<String>>,
);

/// One node of the tree drawn: its row, clicked to select it, and below
/// it, foldable, the nodes it leads to. `names` gives the tags of the
/// level's objects that a script names (`links::script_names`).
fn draw_talk_node(
    ui: &mut egui::Ui,
    (dlg, said, names): TalkRead,
    node: &TalkNode,
    (depth, selected): (
        usize,
        Option<(
            level_edit::dialog::Side,
            usize,
            Option<level_edit::dialog::LinkAt>,
        )>,
    ),
    select: &mut Option<(
        level_edit::dialog::Side,
        usize,
        Option<level_edit::dialog::LinkAt>,
    )>,
) {
    use level_edit::dialog::Side;
    let nodes = if node.side == Side::Entry {
        &dlg.entries
    } else {
        &dlg.replies
    };
    let Some(found) = nodes.get(node.index) else {
        return;
    };
    let who = match node.side {
        Side::Entry if found.speaker.is_empty() => "Owner".to_string(),
        Side::Entry => found.speaker.clone(),
        Side::Reply => "Player".to_string(),
    };
    let mut text: String = said(found).chars().take(80).collect();
    if text.is_empty() {
        text = match node.side {
            // An empty reply is shown as Continue by the games.
            Side::Reply => "(Continue)".to_string(),
            Side::Entry => "(says nothing)".to_string(),
        };
    }
    let mut label = if node.again {
        format!(
            "{who}: goes on at {} {} above",
            if node.side == Side::Entry {
                "line"
            } else {
                "reply"
            },
            node.index
        )
    } else {
        format!("{who}: {text}")
    };
    if !node.condition.is_empty() && depth > 0 {
        label.push_str(&format!("  [if {}]", node.condition));
    }
    if !node.again && !found.script.script.is_empty() {
        // The objects of the level the script acts on, where it names any.
        let named = names(&found.script.script);
        if named.is_empty() {
            label.push_str(&format!("  [runs {}]", found.script.script));
        } else {
            label.push_str(&format!(
                "  [runs {}, which names {}]",
                found.script.script,
                named.join(", ")
            ));
        }
    }
    let colour = if node.side == Side::Entry {
        egui::Color32::from_rgb(235, 190, 140)
    } else {
        egui::Color32::from_rgb(150, 200, 255)
    };
    let chosen = selected.is_some_and(|(side, index, via)| {
        side == node.side && index == node.index && via == node.via
    });
    let row = |ui: &mut egui::Ui, select: &mut Option<_>| {
        let text = egui::RichText::new(&label)
            .color(colour)
            .monospace()
            .size(11.5);
        if ui.selectable_label(chosen, text).clicked() {
            *select = Some((node.side, node.index, node.via));
        }
    };
    if node.children.is_empty() || node.again {
        ui.horizontal(|ui| {
            // Lined up with the rows that have a fold button.
            ui.add_space(18.0);
            row(ui, select);
        });
        return;
    }
    let id = ui.make_persistent_id(("talk node", node.side, node.index, node.via));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, depth < 4)
        .show_header(ui, |ui| row(ui, select))
        .body(|ui| {
            for child in &node.children {
                draw_talk_node(ui, (dlg, said, names), child, (depth + 1, selected), select);
            }
        });
}

/// A text field's change made as one edit while it is typed in: the first
/// keystroke an edit of its own, the rest added to it, so one undo takes
/// the whole of it back.
fn typed_edit(
    state: &mut EditorState,
    ui_state: &mut UiState,
    key: &str,
    response: &egui::Response,
    edit: Option<Edit>,
) {
    if response.changed()
        && let Some(edit) = edit
    {
        if ui_state.dialog_typing.as_deref() == Some(key) {
            state.apply_continuing(edit);
        } else {
            state.apply(edit);
            ui_state.dialog_typing = Some(key.to_string());
        }
    }
    if response.lost_focus() {
        ui_state.dialog_typing = None;
    }
}

/// `typed_edit` for a search-and-pick field (`choose`): a pick from its
/// list is an edit of its own.
fn picked_edit(
    state: &mut EditorState,
    ui_state: &mut UiState,
    key: &str,
    field: &crate::choose::Picked,
    edit: Option<Edit>,
) {
    if field.picked.is_some() {
        if let Some(edit) = edit {
            state.apply(edit);
        }
        ui_state.dialog_typing = None;
    } else {
        typed_edit(state, ui_state, key, &field.response, edit);
    }
}

/// The row of the conversation window's grid that sets the skill check of
/// the link `at` (`level_edit::checks`): the skill, the score it asks for,
/// and the items it costs, each picked, not written. A check is written as
/// the link's condition, so a condition written by hand is replaced.
fn skill_check_row(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    name: &str,
    at: level_edit::dialog::LinkAt,
) {
    use level_edit::checks::{self, Check};
    let now = state::link_check(state, name, at);
    ui.label("Skill check");
    let mut chosen = now.clone();
    ui.horizontal(|ui| {
        let shown = (chosen.as_ref())
            .and_then(|check| checks::skill(&check.skill))
            .map_or("None", |skill| skill.title);
        egui::ComboBox::from_id_salt("talk check skill")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                if ui.selectable_label(chosen.is_none(), "None").clicked() {
                    chosen = None;
                }
                for skill in checks::SKILLS {
                    let on = chosen.as_ref().is_some_and(|check| check.skill == skill.key);
                    if ui.selectable_label(on, skill.title).clicked() && !on {
                        let (score, cost) = chosen.as_ref().map_or((5, 1), |check| (check.score, check.cost.max(1)));
                        chosen = Some(Check {
                            skill: skill.key.to_string(),
                            score,
                            cost: if skill.item.is_some() { cost } else { 0 },
                        });
                    }
                }
            })
            .response
            .on_hover_text(
                "The line or reply is offered only when the character in the conversation \
                 has this skill at the score below or higher. Computer Use, Repair and \
                 Security may also cost items, which are spent when it is used.",
            );
        if let Some(check) = chosen.as_mut() {
            ui.label("score");
            ui.add(egui::DragValue::new(&mut check.score).range(0..=checks::MOST_SCORE))
                .on_hover_text("The skill's total, as the character sheet shows it: ranks, ability, feats, items and effects.");
            match checks::skill(&check.skill).and_then(|skill| skill.item) {
                Some((_, _, many)) => {
                    ui.label("cost");
                    ui.add(egui::DragValue::new(&mut check.cost).range(0..=checks::MOST_COST).suffix(format!(" {many}")))
                        .on_hover_text("The items the party must carry, spent when the line or reply is used. 0 costs nothing.");
                }
                None => check.cost = 0,
            }
        }
    });
    ui.end_row();
    if let Some(check) = &chosen {
        ui.label("");
        ui.label(egui::RichText::new(check.described()).small());
        ui.end_row();
    }
    if chosen != now
        && let Err(why) = state::set_check(state, name, at, chosen)
    {
        state.say(why);
    }
}

/// The conversation window: the tree of the conversation open, the
/// selected line's text, speaker, scripts and links, and its tries.
fn conversation_window(ctx: &egui::Context, state: &mut EditorState, ui_state: &mut UiState) {
    use level_edit::dialog::{Edit as Talk, LinkAt, Side, from_talk_table, read, text_of};
    let Some(name) = ui_state.dialog_open.clone() else {
        return;
    };
    // The project's own conversation is changed here. A conversation only
    // the game has is shown as it is, to be read, and the project does not
    // change until Edit a copy copies it.
    let own = state.editor.project.conversations.contains_key(&name);
    // The conversation is read again only after a change of the project.
    let changes = state.editor.changes();
    let dlg = match &ui_state.dialog_read {
        Some((at, had, dlg)) if *at == changes && *had == name => dlg.clone(),
        _ => {
            let read_now = match state.editor.project.conversations.get(&name) {
                Some(gff) => Some(read(gff)),
                None => (state.setup.resolver.read(&name, ResType::DLG))
                    .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
                    .map(|gff| read(&gff)),
            };
            let Some(read_now) = read_now else {
                state.say(format!("there is no conversation {name}"));
                ui_state.dialog_open = None;
                return;
            };
            let dlg = std::sync::Arc::new(read_now);
            ui_state.dialog_read = Some((changes, name.clone(), dlg.clone()));
            dlg
        }
    };
    let talk = state.setup.talk.clone();
    let line_text = |node: &fmt_odyssey::dlg::Node| {
        text_of(node, &|strref| {
            talk.as_ref()?
                .text(strref)
                .map(|bytes| bytes.iter().map(|&b| char::from(b)).collect())
        })
    };
    let (openings, lost) = talk_tree(&dlg);
    // What each opening's condition checks, read once.
    for link in &dlg.starts {
        let script = &link.active.script;
        if !script.is_empty() && !ui_state.talk_checks.contains_key(script) {
            let checks = condition_checks(state, script);
            ui_state.talk_checks.insert(script.clone(), checks);
        }
    }
    let mut open = true;
    let mut asked: Vec<Edit> = Vec::new();
    let mut command: Option<String> = None;
    let mut select: Option<(Side, usize, Option<LinkAt>)> = None;
    let title = if own {
        format!("Conversation {name}")
    } else {
        format!("Conversation {name}, the game's")
    };
    egui::Window::new(title)
        .id(egui::Id::new("conversation window"))
        // It opens over the outliner's side, so the level's view stays
        // clear. It can be moved and resized from there.
        .default_pos([8.0, 44.0])
        .default_size([520.0, 560.0])
        .resizable(true)
        .open(&mut open)
        .show(ctx, |ui| {
            if !own {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        "This is the game's conversation, shown to be read. The project does not \
                         change while it is read.",
                    );
                    if ui
                        .button("Edit a copy")
                        .on_hover_text(
                            "Copies the game's conversation into the project, where it is changed and \
                             played in the place of the game's.",
                        )
                        .clicked()
                    {
                        command = Some(format!("conversation copy {name}"));
                    }
                });
                ui.separator();
            }
            // The conversation's own: its end scripts, and a try.
            ui.horizontal_wrapped(|ui| {
                for (label, now, ending) in [("When it ends", &dlg.on_end, true), ("When it is cut short", &dlg.on_abort, false)] {
                    ui.label(label);
                    let key = format!("{name} {label}");
                    let mut typed = now.clone();
                    let response = ui.add_enabled(own, egui::TextEdit::singleline(&mut typed).desired_width(110.0).hint_text("no script"));
                    let change = if ending { Talk::End(typed.trim().to_string()) } else { Talk::Abort(typed.trim().to_string()) };
                    typed_edit(state, ui_state, &key, &response, Some(Edit::Conversation(name.clone(), change)));
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(own, egui::Button::new("Add a starting line")).on_hover_text("A line the conversation may open with. The first whose condition holds is said.").clicked() {
                    select = Some((Side::Entry, dlg.entries.len(), Some(LinkAt::Start(dlg.starts.len()))));
                    asked.push(Edit::Conversation(name.clone(), Talk::Add { under: None }));
                    ui_state.dialog_focus = true;
                }
                ui.separator();
                if ui_state.dialog_try.0.is_empty()
                    && let Some(Target::Object(kind, index)) = state.editor.selected
                    && let Some(entry) = state.editor.entry(kind, index)
                {
                    ui_state.dialog_try.0 = state.tag_of(&entry);
                }
                ui.label("Try with");
                crate::choose::field(ui, "talk try", &mut ui_state.dialog_try.0, ("an object's tag", 110.0), || crate::choose::tags(state));
                ui.label("answering");
                ui.add(egui::TextEdit::singleline(&mut ui_state.dialog_try.1).desired_width(60.0).hint_text("1,2"));
                let trying = state.script_try.is_some();
                let tag = ui_state.dialog_try.0.trim().to_string();
                if ui
                    .add_enabled(!trying && !tag.is_empty() && !tag.contains(' '), egui::Button::new("Try"))
                    .on_hover_text(
                        "Saves, then plays the conversation in the level without a window: the player talks to \
                         the object of this tag and takes the replies by their place (1 the first), the first \
                         when the list runs out. What was said shows below.",
                    )
                    .clicked()
                {
                    let picks: Vec<&str> = ui_state.dialog_try.1.split([',', ' ']).filter(|p| !p.is_empty()).collect();
                    command = Some(format!("conversation try {name} {tag} {}", picks.join(" ")));
                }
                if trying {
                    ui.spinner();
                }
            });
            // The last try of this conversation.
            if let Some(tried) = state.tried.as_ref().filter(|tried| tried.script == name) {
                ui.separator();
                egui::CollapsingHeader::new(format!("Last try with {}", tried.on.as_deref().unwrap_or("?")))
                    .default_open(true)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical().id_salt("conversation try").max_height(160.0).show(ui, |ui| {
                            for line in &tried.lines {
                                ui.label(egui::RichText::new(line).monospace().size(11.0));
                            }
                        });
                    });
            }
            ui.separator();
            // The tree on the left, in a panel whose edge can be dragged,
            // and the selected line on the right. Each scrolls on its own,
            // so a long row of the tree never runs under the line's fields.
            let wide = ui.available_width();
            egui::Panel::left("conversation tree panel")
                .resizable(true)
                .default_size(wide * 0.55)
                .size_range(160.0..=(wide - 240.0).max(160.0))
                .show(ui, |ui| {
                // The tree: an opening at a time, in the order the game
                // tries them.
                egui::ScrollArea::both().id_salt("conversation tree").auto_shrink(false).show(ui, |ui| {
                    if openings.is_empty() && lost.is_empty() {
                        ui.label("No lines yet. Add a starting line.");
                    }
                    if openings.len() > 1 {
                        ui.label(
                            egui::RichText::new(
                                "The conversation opens with the first of these whose condition holds, tried from \
                                 the top. Select an opening's first line and use Up or Down to change the order.",
                            )
                            .small(),
                        );
                    }
                    let selected = ui_state.dialog_selected;
                    for (n, opening) in openings.iter().enumerate() {
                        let first: String = dlg
                            .entries
                            .get(opening.index)
                            .map(|node| line_text(node).chars().take(48).collect())
                            .unwrap_or_else(String::new);
                        let when = if opening.condition.is_empty() {
                            "always".to_string()
                        } else {
                            let checks = ui_state.talk_checks.get(&opening.condition).cloned().unwrap_or_else(String::new);
                            format!("if {} ({checks})", opening.condition)
                        };
                        let holds_selection = selected.is_some_and(|(_, _, via)| match (via, opening.via) {
                            (Some(level_edit::dialog::LinkAt::Start(a)), Some(level_edit::dialog::LinkAt::Start(b))) => a == b,
                            _ => false,
                        });
                        let id = ui.make_persistent_id(("talk opening", n));
                        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, openings.len() <= 2 || holds_selection)
                            .show_header(ui, |ui| {
                                ui.label(egui::RichText::new(format!("Opening {}, {when}", n + 1)).strong());
                                let quoted: String = first.chars().take(40).collect();
                                ui.label(egui::RichText::new(format!("\"{quoted}\"")).weak());
                            })
                            .body(|ui| {
                                ui.label(egui::RichText::new(format!("\"{first}\"")).small().weak());
                                draw_talk_node(ui, (&dlg, &line_text, &|script| crate::links::script_names(state, script)), opening, (0, selected), &mut select);
                            });
                    }
                    if !lost.is_empty() {
                        egui::CollapsingHeader::new(format!("Not reached from any opening ({})", lost.len()))
                            .id_salt("talk lost")
                            .show(ui, |ui| {
                                for node in &lost {
                                    draw_talk_node(ui, (&dlg, &line_text, &|script| crate::links::script_names(state, script)), node, (1, selected), &mut select);
                                }
                            });
                    }
                });
            });
            egui::CentralPanel::default().show(ui, |ui| {
                // The selected node.
                egui::ScrollArea::vertical().id_salt("conversation line").auto_shrink(false).show(ui, |ui| {
                let Some((side, index, via)) = ui_state.dialog_selected else {
                    ui.label("Click a line in the tree to change it.");
                    return;
                };
                let nodes = if side == Side::Entry { &dlg.entries } else { &dlg.replies };
                let Some(node) = nodes.get(index) else {
                    return;
                };
                let what = if side == Side::Entry { "Line" } else { "Reply" };
                ui.label(egui::RichText::new(format!("{what} {index}")).strong());
                ui.label(
                    egui::RichText::new(if side == Side::Entry {
                        "Said by the owner, or by the object whose tag is its speaker."
                    } else {
                        "An answer the player picks."
                    })
                    .small(),
                );
                if from_talk_table(node) {
                    ui.label(egui::RichText::new(format!("The game's line {} of its talk table:", node.text.strref)).small());
                    ui.label(line_text(node));
                    if ui.add_enabled(own, egui::Button::new("Write its own text")).on_hover_text("The line keeps its text in the conversation from now on, starting from the game's.").clicked() {
                        asked.push(Edit::Conversation(name.clone(), Talk::Text(side, index, line_text(node))));
                    }
                } else {
                    // While the line is typed, a key adds the next line under
                    // it, or another beside it, and moves the typing there.
                    let text_id = egui::Id::new(("talk line text", side, index));
                    let mut go_on = None;
                    if ui.memory(|memory| memory.has_focus(text_id)) {
                        for (action, beside) in [(crate::keys::Action::TalkBeside, true), (crate::keys::Action::TalkNext, false)] {
                            if go_on.is_none() && state.keys.map.of(action).iter().any(|chord| chord.take_from(ui.ctx())) {
                                go_on = Some(beside);
                            }
                        }
                    }
                    let mut text = line_text(node);
                    let response = ui.add(egui::TextEdit::multiline(&mut text).interactive(own).id(text_id).desired_rows(3).desired_width(f32::INFINITY).hint_text("What is said"));
                    if std::mem::take(&mut ui_state.dialog_focus) {
                        response.request_focus();
                    }
                    typed_edit(state, ui_state, &format!("{name} text {side:?} {index}"), &response, Some(Edit::Conversation(name.clone(), Talk::Text(side, index, text))));
                    let (next, beside) = (state.keys.map.shown(crate::keys::Action::TalkNext), state.keys.map.shown(crate::keys::Action::TalkBeside));
                    if !next.is_empty() || !beside.is_empty() {
                        let under = if side == Side::Entry { "a reply to it" } else { "the line that answers it" };
                        let next_says = if next.is_empty() { String::new() } else { format!("{next} adds {under}. ") };
                        let beside_says = if beside.is_empty() || via.is_none() { String::new() } else { format!("{beside} adds another {} beside it.", if side == Side::Entry { "line" } else { "reply" }) };
                        ui.label(egui::RichText::new(format!("{next_says}{beside_says}")).small().weak());
                    }
                    match go_on {
                        // The next line, under this one.
                        Some(false) => {
                            let others = if side == Side::Entry { dlg.replies.len() } else { dlg.entries.len() };
                            select = Some((side.other(), others, Some(LinkAt::From(side, index, node.links.len()))));
                            asked.push(Edit::Conversation(name.clone(), Talk::Add { under: Some((side, index)) }));
                            ui_state.dialog_focus = true;
                        }
                        // Another line beside this one: another opening, or
                        // another line under the same one.
                        Some(true) => match via {
                            Some(LinkAt::Start(_)) => {
                                select = Some((Side::Entry, dlg.entries.len(), Some(LinkAt::Start(dlg.starts.len()))));
                                asked.push(Edit::Conversation(name.clone(), Talk::Add { under: None }));
                                ui_state.dialog_focus = true;
                            }
                            Some(LinkAt::From(from, parent, _)) => {
                                let siblings = (if from == Side::Entry { &dlg.entries } else { &dlg.replies }).get(parent).map_or(0, |parent| parent.links.len());
                                let count = if side == Side::Entry { dlg.entries.len() } else { dlg.replies.len() };
                                select = Some((side, count, Some(LinkAt::From(from, parent, siblings))));
                                asked.push(Edit::Conversation(name.clone(), Talk::Add { under: Some((from, parent)) }));
                                ui_state.dialog_focus = true;
                            }
                            None => {}
                        },
                        None => {}
                    }
                }
                // A conversation of the game is read, not changed: its
                // fields below are shown but cannot be changed.
                if !own {
                    ui.disable();
                }
                egui::Grid::new("talk node").num_columns(2).show(ui, |ui| {
                    if side == Side::Entry {
                        for (label, now, hint) in [("Speaker", &node.speaker, "the owner"), ("Listener", &node.listener, "the player")] {
                            ui.label(label);
                            let mut typed = now.clone();
                            let field = crate::choose::field(ui, ("talk", label), &mut typed, (hint, 150.0), || crate::choose::tags(state));
                            let change = if label == "Speaker" { Talk::Speaker(side, index, typed.trim().to_string()) } else { Talk::Listener(side, index, typed.trim().to_string()) };
                            picked_edit(state, ui_state, &format!("{name} {label} {index}"), &field, Some(Edit::Conversation(name.clone(), change)));
                            ui.end_row();
                        }
                    }
                    ui.label("Runs");
                    // A line that spends a skill check's cost runs a script
                    // that spends it, then runs the line's own script: the
                    // field shows and changes the line's own script.
                    let paid = state::payment_of(state, &node.script.script);
                    ui.horizontal(|ui| {
                        let own_script = paid.as_ref().map_or_else(|| node.script.script.clone(), |(_, _, then)| then.clone());
                        let mut typed = own_script.clone();
                        let field = crate::choose::field(ui, "talk script", &mut typed, ("no script", 120.0), || crate::choose::scripts(state));
                        if paid.is_some() {
                            if (field.picked.is_some() || field.response.changed())
                                && let Err(why) = state::set_payment_then(state, &node.script.script, typed.trim())
                            {
                                state.say(why);
                            }
                        } else {
                            picked_edit(state, ui_state, &format!("{name} script {side:?} {index}"), &field, Some(Edit::Conversation(name.clone(), Talk::Script(side, index, typed.trim().to_string()))));
                        }
                        if !own_script.is_empty() && ui.small_button("Write it").on_hover_text("Opens the script to be written: the project's own, else a copy of the game's, else a new one.").clicked() {
                            command = Some(format!("script edit {own_script}"));
                        }
                    });
                    ui.end_row();
                    if let Some((skill, cost, _)) = &paid {
                        let items = level_edit::checks::skill(skill).and_then(|skill| skill.item).map_or("items", |(_, one, many)| if *cost == 1 { one } else { many });
                        ui.label("");
                        ui.label(egui::RichText::new(format!("First it spends {cost} {items}, the cost of its skill check.")).small());
                        ui.end_row();
                    }
                    if let Some(at) = via {
                        let link = match at {
                            LinkAt::Start(n) => dlg.starts.get(n),
                            LinkAt::From(from, parent, n) => (if from == Side::Entry { &dlg.entries } else { &dlg.replies })
                                .get(parent)
                                .and_then(|parent| parent.links.get(n)),
                        };
                        if let Some(link) = link {
                            ui.label(if side == Side::Entry { "Said only if" } else { "Offered only if" });
                            ui.horizontal(|ui| {
                                let mut typed = link.active.script.clone();
                                let field = crate::choose::field(ui, "talk condition", &mut typed, ("always", 120.0), || crate::choose::scripts(state));
                                picked_edit(state, ui_state, &format!("{name} condition {at:?}"), &field, Some(Edit::Conversation(name.clone(), Talk::Condition(at, typed.trim().to_string()))));
                                let script = link.active.script.clone();
                                if !script.is_empty() && ui.small_button("Write it").on_hover_text("Opens the condition to be written; a new one is a StartingConditional that answers TRUE.").clicked() {
                                    let known = state.project_scripts.contains(&script) || state.drafts.contains_key(&script) || state.setup.resolver.find(&script, ResType::NSS).is_some() || state.setup.resolver.find(&script, ResType::NCS).is_some();
                                    command = Some(if known { format!("script edit {script}") } else { format!("script new {script} conditional") });
                                }
                            });
                            ui.end_row();
                            skill_check_row(ui, state, &name, at);
                        }
                    }
                    ui.label("Journal");
                    ui.horizontal(|ui| {
                        let mut quest = node.quest.clone();
                        let mut entry = node.quest_entry;
                        let a = crate::choose::field(ui, "talk quest", &mut quest, ("quest tag", 100.0), || crate::choose::quests(state));
                        let b = ui.add(egui::DragValue::new(&mut entry).range(0..=1000));
                        let change = Some(Edit::Conversation(name.clone(), Talk::Quest(side, index, quest.trim().to_string(), entry)));
                        picked_edit(state, ui_state, &format!("{name} quest {side:?} {index}"), &a, change.clone());
                        typed_edit(state, ui_state, &format!("{name} quest entry {side:?} {index}"), &b, change);
                    });
                    ui.end_row();
                });
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    let under = if side == Side::Entry { "Add a reply" } else { "Add a line" };
                    if ui.button(under).clicked() {
                        let others = if side == Side::Entry { dlg.replies.len() } else { dlg.entries.len() };
                        select = Some((side.other(), others, Some(LinkAt::From(side, index, node.links.len()))));
                        asked.push(Edit::Conversation(name.clone(), Talk::Add { under: Some((side, index)) }));
                        ui_state.dialog_focus = true;
                    }
                    let others = if side == Side::Entry { &dlg.replies } else { &dlg.entries };
                    egui::ComboBox::from_id_salt("talk link to")
                        .selected_text("Go on at…")
                        .height(300.0)
                        .show_ui(ui, |ui| {
                            for (to, other) in others.iter().enumerate() {
                                let said: String = line_text(other).chars().take(50).collect();
                                if ui.selectable_label(false, format!("{to}: {said}")).clicked() {
                                    asked.push(Edit::Conversation(name.clone(), Talk::LinkTo { from: Some((side, index)), to }));
                                }
                            }
                        });
                    if let Some(at) = via {
                        if ui.small_button("Up").on_hover_text("Earlier among its links: replies are offered, and lines tried, in this order.").clicked() {
                            asked.push(Edit::Conversation(name.clone(), Talk::Shift(at, -1)));
                        }
                        if ui.small_button("Down").clicked() {
                            asked.push(Edit::Conversation(name.clone(), Talk::Shift(at, 1)));
                        }
                        if ui.small_button("Remove this link").on_hover_text("The line is no longer reached from here; it stays in the conversation.").clicked() {
                            asked.push(Edit::Conversation(name.clone(), Talk::Unlink(at)));
                            select = Some((side, index, None));
                        }
                    }
                    if ui.small_button("Delete").on_hover_text("Takes the line out of the conversation, with every link to it.").clicked() {
                        asked.push(Edit::Conversation(name.clone(), Talk::Delete(side, index)));
                        select = None;
                        ui_state.dialog_selected = None;
                    }
                });
                });
            });
        });
    if !open {
        ui_state.dialog_open = None;
    }
    if let Some(selected) = select {
        ui_state.dialog_selected = Some(selected);
    }
    // Nothing changes a conversation of the game: Edit a copy comes first.
    if !own {
        asked.clear();
    }
    for edit in asked {
        state.apply(edit);
    }
    if let Some(command) = command {
        state.pending_command = Some(command);
    }
}

/// The Scripts menu: the project's own scripts, each opened in the
/// viewer, and a new one made.
fn scripts_menu(ui: &mut egui::Ui, state: &mut EditorState, ui_state: &mut UiState) {
    // It holds a text field, so a click inside does not close it; its
    // buttons close it.
    let config = egui::containers::menu::MenuConfig::new()
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside);
    egui::containers::menu::MenuButton::new("Scripts")
        .config(config)
        .ui(ui, |ui| {
            let mut names: Vec<String> = state.project_scripts.iter().cloned().collect();
            names.extend(state.drafts.keys().cloned());
            names.sort();
            names.dedup();
            if names.is_empty() {
                ui.label("The project has no scripts of its own yet.");
            }
            for name in names {
                let changed = state
                    .drafts
                    .get(&name)
                    .is_some_and(crate::script_edit::Draft::changed);
                let label = if changed {
                    format!("{name} (not saved)")
                } else {
                    name.clone()
                };
                if ui.button(label).clicked() {
                    state.pending_command = Some(format!("script edit {name}"));
                    ui_state.view_tab = ViewTab::Source;
                    ui.close();
                }
            }
            ui.separator();
            ui.label("A new script, named 1 to 16 lowercase letters, digits and _:");
            ui.add(
                egui::TextEdit::singleline(&mut ui_state.new_script)
                    .hint_text("kje_door_opens")
                    .desired_width(160.0),
            );
            let name = ui_state.new_script.trim().to_ascii_lowercase();
            let problem = crate::script_edit::name_problem(&name);
            ui.horizontal(|ui| {
                for (label, hint, conditional) in [
                    (
                        "New",
                        "A main, run on an event: OnUsed, OnEnter, OnDeath.",
                        false,
                    ),
                    (
                        "New condition",
                        "A StartingConditional, which answers TRUE to show a conversation's line.",
                        true,
                    ),
                ] {
                    if ui
                        .add_enabled(problem.is_none(), egui::Button::new(label))
                        .on_hover_text(hint)
                        .on_disabled_hover_text(problem.clone().unwrap_or_else(String::new))
                        .clicked()
                    {
                        state.pending_command = Some(if conditional {
                            format!("script new {name} conditional")
                        } else {
                            format!("script new {name}")
                        });
                        ui_state.view_tab = ViewTab::Source;
                        ui_state.new_script.clear();
                        ui.close();
                    }
                }
            });
        });
}

/// The script viewer: what the script `UiState::viewing` names does, as its
/// source, a summary of its compiled form, or its listing
/// (`script_view`). A script of the project, or a copy of one of the
/// install's, is written here (`script_edit`), then saved, compiled, tried
/// in the module headless and given to the selected object's event, each
/// by an `edit script` command.
fn script_viewer(ui: &mut egui::Ui, state: &mut EditorState, ui_state: &mut UiState) {
    let Some(name) = ui_state.viewing.clone() else {
        return;
    };
    // Scripts compiled since they were read are read again.
    if ui_state.read_round != state.script_round {
        ui_state.read_scripts.clear();
        ui_state.read_round = state.script_round;
    }
    let view = ui_state
        .read_scripts
        .entry(name.clone())
        .or_insert_with(|| {
            crate::script_view::read(
                &state.setup.resolver,
                state.setup.catalog.as_ref(),
                &state.editor.project.folder,
                &name,
            )
        });
    let changed = state
        .drafts
        .get(&name)
        .is_some_and(crate::script_edit::Draft::changed);
    // The name and Close on one row, the tabs on the next, so that the
    // header fits a narrow panel.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&name).strong().monospace());
        if changed {
            ui.label(egui::RichText::new("not saved").small().weak());
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Close").clicked() {
                ui_state.viewing = None;
            }
            let (label, hint) = if ui_state.viewer_window {
                ("Dock", "Puts the viewer back under the details.")
            } else {
                (
                    "Window",
                    "Opens the viewer in a window of its own, with room to write.",
                )
            };
            if ui.small_button(label).on_hover_text(hint).clicked() {
                ui_state.viewer_window = !ui_state.viewer_window;
            }
        });
    });
    let drafted = state.drafts.contains_key(&name);
    let tried_here = state
        .tried
        .as_ref()
        .is_some_and(|tried| tried.script == name)
        || state
            .script_try
            .as_ref()
            .is_some_and(|run| run.script == name);
    let debugged_here = state
        .tried
        .as_ref()
        .is_some_and(|tried| tried.script == name && !tried.stops.is_empty());
    // A tab the script has nothing for shows the next that it has.
    let tab = match ui_state.view_tab {
        ViewTab::Source if view.source.is_none() && !drafted => ViewTab::Summary,
        ViewTab::Try if !tried_here => ViewTab::Source,
        ViewTab::Debug if !debugged_here => ViewTab::Source,
        tab => tab,
    };
    ui.horizontal(|ui| {
        for (each, label, there) in [
            (ViewTab::Source, "Source", view.source.is_some() || drafted),
            (ViewTab::Summary, "Summary", view.listing.is_some()),
            (ViewTab::Listing, "Listing", view.listing.is_some()),
            (ViewTab::Try, "Try", tried_here),
            (ViewTab::Debug, "Debug", debugged_here),
        ] {
            ui.add_enabled_ui(there, |ui| {
                if ui.selectable_label(tab == each, label).clicked() {
                    ui_state.view_tab = each;
                }
            });
        }
    });
    let from = match (&view.source, &view.compiled_from) {
        _ if drafted && tab == ViewTab::Source => {
            "Written here, and saved to the project's folder.".to_string()
        }
        (Some((_, layer)), _) if tab == ViewTab::Source => format!("Source from {layer}."),
        (_, Some(layer)) => format!(
            "Compiled, from {layer}.{}",
            if view.source.is_none() {
                " The install has no source of it."
            } else {
                ""
            }
        ),
        (_, None) => "The install has no compiled script of this name.".to_string(),
    };
    ui.label(egui::RichText::new(from).small().weak());
    if let Some(problem) = &view.problem {
        ui.label(format!("It does not read: {problem}"));
    }
    let error = state.script_errors.get(&name).cloned();
    if let Some(why) = &error {
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!("It did not compile: {why}"),
        );
    }
    match tab {
        ViewTab::Source if drafted => {
            script_tools(ui, state, &name);
            let error_line = error.as_deref().and_then(|why| {
                let rest = why.strip_prefix(&format!("{name}.nss line "))?;
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                digits.parse::<usize>().ok()
            });
            let Some(draft) = state.drafts.get_mut(&name) else {
                return;
            };
            let help = (state.setup.catalog.as_ref(), &state.setup.notes);
            // The look-up stands under the text, which takes the rest.
            let mut writing = false;
            egui::Panel::bottom("script look-up")
                .resizable(false)
                .show(ui, |ui| look_up(ui, help.0, help.1));
            let stop_line = state
                .tried
                .as_ref()
                .filter(|tried| tried.script == name)
                .and_then(|tried| tried.stops.get(ui_state.debug_at))
                .filter(|stop| stop.file == name)
                .map(|stop| stop.line as usize);
            let marks = ui_state.breakpoints.entry(name.clone()).or_default();
            writing |= script_text(ui, &mut draft.text, (error_line, stop_line, marks), help.0);
            // Ctrl+S while writing saves the scripts (the editor's own keys
            // are off while a text field has them).
            if writing
                && ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::S))
            {
                state.pending_command = Some("script write".into());
            }
        }
        ViewTab::Source => {
            let own = view
                .source
                .as_ref()
                .is_some_and(|(_, layer)| layer == "the project");
            let (label, hint) = if own {
                ("Edit", "Writes the project's script here.")
            } else {
                (
                    "Edit a copy in the project",
                    "Copies the install's source into the project, where it is written here; \
                     saved and compiled, it runs in the place of the install's script.",
                )
            };
            if ui.button(label).on_hover_text(hint).clicked() {
                state.pending_command = Some(format!("script edit {name}"));
            }
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                if let Some((source, _)) = &view.source {
                    let job = crate::script_view::highlighted(source, ui.visuals().dark_mode);
                    ui.add(egui::Label::new(job).extend());
                }
            });
        }
        ViewTab::Summary => {
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                ui.label(egui::RichText::new("Engine routines it calls").strong());
                if view.calls.is_empty() {
                    ui.label("none");
                }
                for (call, count) in &view.calls {
                    let times = if *count > 1 {
                        format!("   ({count} times)")
                    } else {
                        String::new()
                    };
                    ui.label(egui::RichText::new(format!("{call}{times}")).monospace());
                }
                ui.add_space(6.0);
                ui.label(egui::RichText::new("Strings it uses").strong());
                if view.strings.is_empty() {
                    ui.label("none");
                }
                for string in &view.strings {
                    ui.label(egui::RichText::new(format!("{string:?}")).monospace());
                }
            });
        }
        ViewTab::Listing => {
            egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
                if let Some(listing) = &view.listing {
                    ui.add(
                        egui::Label::new(egui::RichText::new(listing).monospace().size(11.0))
                            .extend(),
                    );
                }
            });
        }
        ViewTab::Try => tried(ui, state, &name),
        ViewTab::Debug => debugged(ui, state, ui_state, &name),
    }
}

/// The selected object, its tag and the events of its behaviour, for
/// trying a script for it and giving it one.
fn selected_events(state: &EditorState) -> Option<(String, Vec<String>)> {
    let Some(Target::Object(kind, index)) = state.editor.selected else {
        return None;
    };
    let entry = state.editor.entry(kind, index)?;
    let gff = state.behaviour_source(kind, &entry)?;
    let events = level_edit::template::scripts(&gff)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    Some((state.tag_of(&entry), events))
}

/// The row over a script being written: Save, Compile, Try and Give to.
fn script_tools(ui: &mut egui::Ui, state: &mut EditorState, name: &str) {
    let selected = selected_events(state);
    let tag = selected
        .as_ref()
        .map(|(tag, _)| tag.clone())
        .filter(|tag| !tag.is_empty());
    let mut ask = None;
    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Save")
            .on_hover_text("Writes the scripts changed here to the project's folder.")
            .clicked()
        {
            ask = Some("script write".to_string());
        }
        if ui
            .button("Compile")
            .on_hover_text("Writes the scripts, then compiles every script of the project.")
            .clicked()
        {
            ask = Some("script compile".to_string());
        }
        let trying = state.script_try.is_some();
        let whom = tag.as_deref().unwrap_or("the player");
        if ui
            .add_enabled(!trying, egui::Button::new(format!("Try for {whom}")))
            .on_hover_text(format!(
                "Saves the project, compiles, then runs {name} once in the module, without a \
                 window, as {whom}'s script (OBJECT_SELF), and steps on 3 s: what it called is \
                 under Try. Select an object to try it for that object."
            ))
            .clicked()
        {
            ask = Some(match &tag {
                Some(tag) => format!("script try {name} {tag}"),
                None => format!("script try {name}"),
            });
        }
        if ui
            .add_enabled(!trying, egui::Button::new(format!("Debug for {whom}")))
            .on_hover_text(
                "Tries it as Try does, recording each line it runs with its locals and globals: \
                 the Debug tab steps through them, and stops at the breakpoints set in the gutter.",
            )
            .clicked()
        {
            ask = Some(match &tag {
                Some(tag) => format!("script debug {name} {tag}"),
                None => format!("script debug {name}"),
            });
        }
        match &selected {
            Some((_, events)) if !events.is_empty() => {
                ui.menu_button("Give to", |ui| {
                    for event in events {
                        if ui
                            .button(event)
                            .on_hover_text(format!("The selected object runs {name} on {event}."))
                            .clicked()
                        {
                            ask = Some(format!("script {event} {name}"));
                            ui.close();
                        }
                    }
                });
            }
            _ => {
                ui.add_enabled(false, egui::Button::new("Give to"))
                    .on_disabled_hover_text(
                        "Select a placeable, creature, door or trigger to give it this script.",
                    );
            }
        }
    });
    if let Some(ask) = ask {
        state.pending_command = Some(ask);
    }
}

/// A script's text, written in place: coloured, with its lines numbered,
/// the line of a mistake in red. Whether it has the keys.
fn script_text(
    ui: &mut egui::Ui,
    text: &mut String,
    (error_line, stop_line, marks): (
        Option<usize>,
        Option<usize>,
        &mut std::collections::BTreeSet<u32>,
    ),
    catalog: Option<&script_ncs::Catalog>,
) -> bool {
    // The completion offered last frame, which Tab takes.
    let offered_id = egui::Id::new("script completion");
    let offered: Option<(usize, usize, String)> = ui.data(|data| data.get_temp(offered_id));
    let take = offered.is_some()
        && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
    let mut placed: Option<usize> = None;
    if take && let Some((from, to, name)) = offered.clone() {
        let (from_byte, to_byte) = (char_byte(text, from), char_byte(text, to));
        text.replace_range(from_byte..to_byte, &name);
        placed = Some(from + name.chars().count());
    }
    let font = egui::FontId::monospace(12.0);
    let dark = ui.visuals().dark_mode;
    let weak = ui.visuals().weak_text_color();
    let red = ui.visuals().error_fg_color;
    egui::ScrollArea::both()
        .id_salt("script text")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let lines = text.split('\n').count();
                let mut numbers = egui::text::LayoutJob::default();
                for line in 1..=lines {
                    let last = line == lines;
                    // A breakpoint's dot, the shown stop's arrow.
                    let mark = if Some(line) == stop_line {
                        ">"
                    } else if marks.contains(&(line as u32)) {
                        "o"
                    } else {
                        " "
                    };
                    let colour = if Some(line) == stop_line {
                        egui::Color32::YELLOW
                    } else if Some(line) == error_line || marks.contains(&(line as u32)) {
                        red
                    } else {
                        weak
                    };
                    numbers.append(
                        &format!("{mark}{line:>4}{}", if last { "" } else { "\n" }),
                        0.0,
                        egui::TextFormat {
                            font_id: font.clone(),
                            color: colour,
                            ..Default::default()
                        },
                    );
                }
                // A click on a line's number sets or clears its breakpoint.
                let gutter = ui
                    .add(
                        egui::Label::new(numbers)
                            .selectable(false)
                            .sense(egui::Sense::click()),
                    )
                    .on_hover_text("Click a line's number to stop a debugged run there.");
                if gutter.clicked()
                    && let Some(at) = gutter.interact_pointer_pos()
                {
                    let row = gutter.rect.height() / lines.max(1) as f32;
                    let line = ((at.y - gutter.rect.top()) / row).floor() as u32 + 1;
                    if !marks.remove(&line) {
                        marks.insert(line);
                    }
                }
                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, _: f32| {
                    let job = crate::script_view::highlighted(text.as_str(), dark);
                    ui.fonts_mut(|fonts| fonts.layout_job(job))
                };
                let output = egui::TextEdit::multiline(text)
                    .id_salt("script text field")
                    .code_editor()
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .desired_width(f32::INFINITY)
                    .desired_rows(24)
                    .lock_focus(true)
                    .layouter(&mut layouter)
                    .show(ui);
                let id = output.response.id;
                // The cursor after a completion taken.
                if let Some(at) = placed
                    && let Some(mut edit_state) = egui::TextEdit::load_state(ui.ctx(), id)
                {
                    let cursor = egui::text::CCursor::new(at);
                    edit_state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(cursor)));
                    edit_state.store(ui.ctx(), id);
                }
                let cursor =
                    placed.or_else(|| output.cursor_range.map(|range| range.primary.index.0));
                let focused = output.response.has_focus();
                ui.data_mut(|data| data.remove::<(usize, usize, String)>(offered_id));
                if focused && let (Some(cursor), Some(catalog)) = (cursor, catalog) {
                    let offer = completion(text, cursor, catalog);
                    if let Some(first) = offer.first() {
                        let from = cursor - word_before(text, cursor).chars().count();
                        ui.data_mut(|data| {
                            data.insert_temp(offered_id, (from, cursor, first.clone()))
                        });
                    }
                    ui.data_mut(|data| data.insert_temp(egui::Id::new("script offer"), offer));
                    ui.data_mut(|data| {
                        data.insert_temp(egui::Id::new("script word"), word_at(text, cursor))
                    });
                }
                focused
            })
            .inner
        })
        .inner
}

/// The byte of `text` its `at`th character starts at.
fn char_byte(text: &str, at: usize) -> usize {
    text.char_indices()
        .nth(at)
        .map_or(text.len(), |(byte, _)| byte)
}

/// The part of a name that stands before character `cursor`.
fn word_before(text: &str, cursor: usize) -> String {
    let before: Vec<char> = text.chars().take(cursor).collect();
    let start = before
        .iter()
        .rposition(|c| !(c.is_ascii_alphanumeric() || *c == '_'))
        .map_or(0, |at| at + 1);
    before[start..].iter().collect()
}

/// The whole name character `cursor` stands in or after.
fn word_at(text: &str, cursor: usize) -> String {
    let after: String = text
        .chars()
        .skip(cursor)
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    format!("{}{after}", word_before(text, cursor))
}

/// The routines and constants of `catalog` that the name before `cursor`
/// begins, at most eight, when it is two characters or more and is not one
/// of them whole already.
fn completion(text: &str, cursor: usize, catalog: &script_ncs::Catalog) -> Vec<String> {
    let typed = word_before(text, cursor);
    if typed.len() < 2 || typed.starts_with(|c: char| c.is_ascii_digit()) {
        return Vec::new();
    }
    let lower = typed.to_ascii_lowercase();
    let names = catalog
        .routines
        .iter()
        .map(|routine| routine.name.as_str())
        .chain(
            catalog
                .constants
                .iter()
                .map(|constant| constant.name.as_str()),
        );
    let mut found: Vec<String> = names
        .filter(|name| name.to_ascii_lowercase().starts_with(&lower) && *name != typed)
        .map(str::to_string)
        .collect();
    found.sort_by_key(|name| name.len());
    found.dedup();
    found.truncate(8);
    found
}

/// Under the script: the routine or constant the cursor stands in, its
/// prototype and what `nwscript.nss` says of it; and the names Tab would
/// complete.
fn look_up(
    ui: &mut egui::Ui,
    catalog: Option<&script_ncs::Catalog>,
    notes: &BTreeMap<String, String>,
) {
    let Some(catalog) = catalog else {
        return;
    };
    let offer: Vec<String> = ui
        .data(|data| data.get_temp(egui::Id::new("script offer")))
        .unwrap_or_else(Vec::new);
    let word: String = ui
        .data(|data| data.get_temp(egui::Id::new("script word")))
        .unwrap_or_else(String::new);
    if !offer.is_empty() {
        ui.label(
            egui::RichText::new(format!("Tab: {}", offer.join(", ")))
                .small()
                .monospace(),
        );
    }
    if let Some(routine) = catalog.routine_named(&word) {
        ui.label(
            egui::RichText::new(routine.to_string())
                .monospace()
                .small()
                .strong(),
        );
        if let Some(note) = notes.get(&routine.name) {
            let short: Vec<&str> = note.lines().take(4).collect();
            ui.label(egui::RichText::new(short.join("\n")).small());
        }
    } else if let Some(constant) = catalog.constant_named(&word) {
        ui.label(
            egui::RichText::new(format!("{} = {:?}", constant.name, constant.value))
                .monospace()
                .small(),
        );
    }
}

/// The Debug tab: the last debugged run of `name`, a stop at a time: its
/// source around the line, the calls that led there, its locals and the
/// script's globals; stepped forward and back, or on to the next
/// breakpoint.
fn debugged(ui: &mut egui::Ui, state: &EditorState, ui_state: &mut UiState, name: &str) {
    let Some(tried) = state.tried.as_ref().filter(|tried| tried.script == name) else {
        return;
    };
    let stops = &tried.stops;
    if stops.is_empty() {
        ui.label("The run started no line of the script.");
        return;
    }
    let last = stops.len() - 1;
    ui_state.debug_at = ui_state.debug_at.min(last);
    let breaking = |stop: &crate::script_edit::Stop| {
        ui_state
            .breakpoints
            .get(&stop.file)
            .is_some_and(|lines| lines.contains(&stop.line))
    };
    let at = ui_state.debug_at;
    let mut to = at;
    ui.horizontal_wrapped(|ui| {
        if ui.small_button("First").clicked() {
            to = 0;
        }
        if ui
            .add_enabled(at > 0, egui::Button::new("Step back").small())
            .clicked()
        {
            to = at - 1;
        }
        if ui
            .add_enabled(at < last, egui::Button::new("Step").small())
            .clicked()
        {
            to = at + 1;
        }
        if ui
            .add_enabled(at < last, egui::Button::new("Continue").small())
            .on_hover_text("On to the next stop at a breakpoint, else to the end.")
            .clicked()
        {
            to = (at + 1..=last)
                .find(|n| breaking(&stops[*n]))
                .unwrap_or(last);
        }
        if ui
            .add_enabled(at > 0, egui::Button::new("Back to a breakpoint").small())
            .clicked()
        {
            to = (0..at).rev().find(|n| breaking(&stops[*n])).unwrap_or(0);
        }
    });
    ui_state.debug_at = to;
    let stop = &stops[to];
    ui.label(
        egui::RichText::new(format!(
            "Stop {} of {}: {}.nss line {}, in {}",
            to + 1,
            stops.len(),
            stop.file,
            stop.line,
            stop.function
        ))
        .strong(),
    );
    ui.label(egui::RichText::new(format!("Called by: {}", stop.calls)).small());
    // The source around the line.
    if let Some(text) = state::script_text(state, &stop.file).or_else(|| {
        state
            .setup
            .resolver
            .read(&stop.file, fmt_odyssey::ResType::NSS)
            .map(|bytes| bytes.into_iter().map(char::from).collect())
    }) {
        let around: Vec<(usize, &str)> = text
            .lines()
            .enumerate()
            .map(|(n, line)| (n + 1, line))
            .filter(|(n, _)| n.abs_diff(stop.line as usize) <= 3)
            .collect();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            for (n, line) in around {
                let shown = egui::RichText::new(format!("{n:>4}  {line}"))
                    .monospace()
                    .size(11.0);
                ui.label(if n == stop.line as usize {
                    shown.color(egui::Color32::YELLOW)
                } else {
                    shown
                });
            }
        });
    }
    egui::ScrollArea::vertical()
        .id_salt("debug values")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Locals").strong());
            if stop.locals.is_empty() {
                ui.label("none");
            }
            egui::Grid::new("debug locals")
                .num_columns(3)
                .show(ui, |ui| {
                    for (name, ty, value) in &stop.locals {
                        ui.label(egui::RichText::new(name).monospace());
                        ui.label(egui::RichText::new(ty).small().weak());
                        ui.label(egui::RichText::new(value).monospace());
                        ui.end_row();
                    }
                });
            if !stop.globals.is_empty() {
                egui::CollapsingHeader::new(format!("Globals ({})", stop.globals.len()))
                    .id_salt("debug globals")
                    .show(ui, |ui| {
                        egui::Grid::new("debug globals grid")
                            .num_columns(3)
                            .show(ui, |ui| {
                                for (name, ty, value) in &stop.globals {
                                    ui.label(egui::RichText::new(name).monospace());
                                    ui.label(egui::RichText::new(ty).small().weak());
                                    ui.label(egui::RichText::new(value).monospace());
                                    ui.end_row();
                                }
                            });
                    });
            }
        });
}

/// The Try tab: a try running, or what the last one of the script said.
fn tried(ui: &mut egui::Ui, state: &EditorState, name: &str) {
    if let Some(run) = state.script_try.as_ref().filter(|run| run.script == name) {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(format!(
                "Trying {name} for {} ({} s): the module is being installed.",
                run.on.as_deref().unwrap_or("the player"),
                run.started.elapsed().as_secs()
            ));
        });
        return;
    }
    let Some(outcome) = state.tried.as_ref().filter(|tried| tried.script == name) else {
        return;
    };
    let whom = outcome.on.as_deref().unwrap_or("the player");
    if outcome.ok {
        ui.label(format!("{name} ran for {whom} without a fault."));
    } else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!("{name} did not run cleanly for {whom}."),
        );
    }
    let ours = format!("run {name} ");
    egui::ScrollArea::both().auto_shrink(false).show(ui, |ui| {
        // The script's own run stands out from the module's scripts that
        // ran beside it.
        let mut own = false;
        for line in &outcome.lines {
            let trimmed = line.trim_start();
            let at_run = trimmed
                .split_once("s ")
                .is_some_and(|(_, rest)| rest.starts_with("run "));
            if at_run {
                own = trimmed.contains(&ours);
            }
            let text = egui::RichText::new(line).monospace().size(11.0);
            let text = if own || !at_run && !line.starts_with("    ") {
                text.strong()
            } else {
                text.weak()
            };
            ui.add(egui::Label::new(text).extend());
        }
    });
}

/// The links between floors, under the navigation mesh's facts while it
/// is shown: each link, whether a walker crosses it, and why not; a jump or
/// a drop added by two clicks on the floor.
fn nav_links(ui: &mut egui::Ui, state: &mut EditorState, ui_state: &mut UiState) {
    use collision::nav::LinkKind;
    if !state.show_nav {
        return;
    }
    ui.separator();
    ui.label(egui::RichText::new("Links between floors").strong());
    ui.label(
        egui::RichText::new(
            "Characters jump or drop across a link where no way leads on foot. A jump goes \
             either way; a drop goes down only.",
        )
        .small(),
    );
    let links = state.editor.project.kjk.links.clone();
    let checks = state.nav_facts.as_ref().map(|nav| nav.links.clone());
    let mut remove = None;
    for (at, link) in links.iter().enumerate() {
        ui.horizontal(|ui| {
            let kind = if link.kind == LinkKind::Jump {
                "Jump"
            } else {
                "Drop"
            };
            let rise = link.to[2] - link.from[2];
            ui.label(format!("{} {kind}, {rise:+.1} m", at + 1));
            match checks.as_ref().and_then(|checks| checks.get(at)) {
                Some(Ok(())) => {
                    ui.colored_label(egui::Color32::from_rgb(120, 220, 120), "crossed");
                }
                Some(Err(why)) => {
                    ui.colored_label(egui::Color32::from_rgb(235, 110, 90), "not crossed")
                        .on_hover_text(why);
                }
                None => {}
            }
            if ui.small_button("Remove").clicked() {
                remove = Some(at);
            }
        });
        if let Some(Err(why)) = checks.as_ref().and_then(|checks| checks.get(at)) {
            ui.label(egui::RichText::new(why).small());
        }
    }
    if let Some(at) = remove {
        state.apply(Edit::RemoveLink(at));
    }
    match ui_state.placing_link {
        Some((_, None)) => {
            ui.label("Click the floor where characters leave. Escape stops.");
        }
        Some((_, Some(_))) => {
            ui.label("Click the floor where they land.");
        }
        None => {
            ui.horizontal(|ui| {
                if ui
                    .button("Add a jump")
                    .on_hover_text("Two clicks on the floor: where characters jump from, then where they land.")
                    .clicked()
                {
                    ui_state.placing_link = Some((LinkKind::Jump, None));
                }
                if ui
                    .button("Add a drop")
                    .on_hover_text("Two clicks on the floor: the edge characters step off, then where they land below.")
                    .clicked()
                {
                    ui_state.placing_link = Some((LinkKind::Drop, None));
                }
            });
        }
    }
}

/// The sun's details: where it stands seen from the level's middle (its
/// bearing and its height over the horizon), its colour, how strong it is
/// and how far it reaches; the game's sun given back, or one added to a
/// level that has none.
fn sun_details(ui: &mut egui::Ui, state: &mut EditorState, ui_state: &mut UiState) {
    let middle = state.editor.level_middle;
    let own = state.editor.project.kjk.sun.is_some();
    let Some(sun) = state.editor.sun() else {
        ui.label("This level has no sun. A sun is a light far over the level, which casts the shadows of daylight.");
        if ui.button("Add a sun").clicked() {
            state.pending_command = Some("sun add".into());
        }
        return;
    };
    ui.label(
        egui::RichText::new(if own {
            "The project's sun, in place of the game's. Play uses it."
        } else {
            "The game's sun. A change makes it the project's."
        })
        .small(),
    );
    let (bearing, height, distance) = sun.seen_from(middle);
    let mut changed: Option<level_edit::area::SunLight> = None;
    let mut dragging = false;
    egui::Grid::new("sun").num_columns(2).show(ui, |ui| {
        let (mut b, mut h) = (bearing, height);
        ui.label("Bearing")
            .on_hover_text("Degrees from east, turning left, seen from the level's middle.");
        let response = ui.add(
            egui::DragValue::new(&mut b)
                .speed(0.5)
                .suffix("°")
                .max_decimals(1),
        );
        dragging |= response.dragged();
        ui.end_row();
        ui.label("Height")
            .on_hover_text("Degrees over the horizon: 90 is straight overhead.");
        let height_drag = ui.add(
            egui::DragValue::new(&mut h)
                .speed(0.25)
                .range(-89.0..=89.9)
                .suffix("°")
                .max_decimals(1),
        );
        dragging |= height_drag.dragged();
        ui.end_row();
        if response.changed() || height_drag.changed() {
            changed = Some(sun.placed(middle, (b, h, distance), &collision::trig::sin_cos_deg));
        }
        let mut colour = sun.colour;
        ui.label("Colour");
        if ui.color_edit_button_rgb(&mut colour).changed() {
            changed = Some(level_edit::area::SunLight {
                colour,
                ..changed.unwrap_or(sun)
            });
        }
        ui.end_row();
        let mut multiplier = sun.multiplier;
        ui.label("Brightness")
            .on_hover_text("How strong it shines: its colour times this.");
        let response = ui.add(
            egui::DragValue::new(&mut multiplier)
                .speed(0.01)
                .range(0.0..=8.0)
                .max_decimals(2),
        );
        dragging |= response.dragged();
        if response.changed() {
            changed = Some(level_edit::area::SunLight {
                multiplier,
                ..changed.unwrap_or(sun)
            });
        }
        ui.end_row();
        let mut radius = sun.radius;
        ui.label("Reach").on_hover_text(
            "How far from it its light reaches, metres. A sun reaches 100 m or more.",
        );
        let response = ui.add(
            egui::DragValue::new(&mut radius)
                .speed(5.0)
                .range(level_edit::area::SunLight::LEAST_RADIUS..=20000.0)
                .suffix(" m")
                .max_decimals(0),
        );
        dragging |= response.dragged();
        if response.changed() {
            changed = Some(level_edit::area::SunLight {
                radius,
                ..changed.unwrap_or(sun)
            });
        }
        ui.end_row();
    });
    ui.label(
        egui::RichText::new(format!(
            "It stands {distance:.0} m from the level's middle."
        ))
        .small(),
    );
    if let Some(sun) = changed {
        if dragging {
            drag_edit(state, ui_state, Edit::SetSun(Some(sun)));
        } else {
            state.apply(Edit::SetSun(Some(sun)));
        }
    }
    if own {
        let label = if state.editor.game_sun.is_some() {
            "Use the game's sun"
        } else {
            "Take the sun out"
        };
        if ui.button(label).clicked() {
            state.apply(Edit::SetSun(None));
        }
    }
}

fn messages(ui: &mut egui::Ui, state: &EditorState) {
    if state.show_nav
        && let Some(nav) = &state.nav_facts
    {
        ui.separator();
        ui.label(egui::RichText::new("Navigation").strong());
        ui.label(format!(
            "{} faces in {} islands, {} portals.",
            nav.faces, nav.islands, nav.portals
        ));
        if nav.entry_off {
            ui.label("The entry stands on no face of the mesh.");
        }
        ui.label(if nav.apart == 0 {
            format!("Every face is reached from {}.", nav.from)
        } else {
            format!(
                "{} faces are not reached from {} on foot: the game's characters cannot walk there.",
                nav.apart, nav.from
            )
        });
        if nav.by_links > 0 {
            ui.label(format!(
                "The links lead to {} of those faces.",
                nav.by_links
            ));
        }
    }
    ui.separator();
    ui.label(egui::RichText::new("Messages").strong());
    for message in state.messages.iter().rev().take(6) {
        ui.label(egui::RichText::new(message).small());
    }
    let reaches: Vec<&String> = state
        .audit
        .iter()
        .filter(|line| {
            let line = line.trim_start();
            ["walk ", "jump ", "force ", "leaks"]
                .iter()
                .any(|word| line.starts_with(word))
        })
        .collect();
    if !reaches.is_empty() {
        ui.label(egui::RichText::new("Containment audit").strong());
        for line in reaches {
            ui.label(egui::RichText::new(line.trim()).small().monospace());
        }
    }
}

fn assets(
    root: &mut egui::Ui,
    state: &mut EditorState,
    ui_state: &mut UiState,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let ctx = root.ctx().clone();
    let ctx = &ctx;
    egui::Panel::bottom("assets")
        .resizable(true)
        .default_size(TILE + 90.0)
        .show(root, |ui| {
            // The panel's width before anything is laid out in it: a row
            // wider than the panel would widen what is left of it.
            let width = ui.available_width();
            let new_map = state.editor.project.map.is_some();
            if !new_map && ui_state.tab == Tab::Rooms {
                ui_state.tab = Tab::Placeables;
            }
            ui.horizontal_wrapped(|ui| {
                // Only a new map places rooms.
                let rooms = new_map.then_some((Tab::Rooms, "Rooms"));
                for (tab, label) in rooms.into_iter().chain([
                    (Tab::Placeables, "Placeables"),
                    (Tab::Creatures, "Creatures"),
                    (Tab::Doors, "Doors"),
                    (Tab::Props, "Jedi Academy props"),
                    (Tab::Shapes, "Collision shapes"),
                    (Tab::Waypoints, "Waypoints"),
                    (Tab::Triggers, "Triggers"),
                    (Tab::Sounds, "Sounds"),
                    (Tab::Stores, "Stores"),
                    (Tab::Encounters, "Encounters"),
                    (Tab::Lights, "Lights"),
                    (Tab::Lamps, "Lamps"),
                    (Tab::Effects, "Jedi Academy effects"),
                ]) {
                    if ui.selectable_value(&mut ui_state.tab, tab, label).changed() {
                        ui_state.thumbnails.forget_wanted();
                    }
                }
                ui.separator();
                ui.add(
                    egui::TextEdit::singleline(&mut ui_state.asset_search)
                        .hint_text("Search assets")
                        .desired_width(220.0),
                );
                ui.checkbox(&mut ui_state.assets_elsewhere, "Other levels")
                    .on_hover_text(
                        "Also lists the templates only the game's other levels have, each with its \
                         level. Placing one copies it into the project with the items, the \
                         conversation and the scripts it names.",
                    );
                ui.label("Drag into the view, or double-click to place at the view's centre.");
                if ui_state.tab == Tab::Lamps
                    && let Some((looked, of)) = state.lamp_library.progress()
                {
                    ui.label(format!("Looking for lamps: {looked} of {of} models"));
                }
            });
            let search = ui_state.asset_search.to_ascii_lowercase();
            // The lamps are found a few models a frame while their tab shows.
            if ui_state.tab == Tab::Lamps {
                let rooms = state.setup.rooms.iter().map(|room| room.model.clone());
                state.lamp_library.start(&state.setup.resolver, rooms);
                state
                    .lamp_library
                    .step(&state.setup.resolver, state.setup.game.key());
                if state.lamp_library.progress().is_some() {
                    ctx.request_repaint();
                }
            }
            // The items are made again only for another tab, another search
            // or more lamps found.
            let key = (
                ui_state.tab,
                search.clone(),
                state.lamp_library.found.len() * 2 + usize::from(ui_state.assets_elsewhere),
            );
            let kept = match &ui_state.asset_items {
                Some((had, items)) if *had == key => Some(items.clone()),
                _ => None,
            };
            let items = match kept {
                Some(items) => items,
                None => {
                    let items: Vec<(Asset, String)> = match ui_state.tab {
                        Tab::Placeables => state
                            .setup
                            .placeables
                            .iter()
                            .map(|name| {
                                (
                                    Asset::Template(AssetKind::Placeable, name.clone()),
                                    name.clone(),
                                )
                            })
                            .collect(),
                        Tab::Creatures => state
                            .setup
                            .creatures
                            .iter()
                            .map(|name| {
                                (
                                    Asset::Template(AssetKind::Creature, name.clone()),
                                    name.clone(),
                                )
                            })
                            .collect(),
                        Tab::Doors => state
                            .setup
                            .doors
                            .iter()
                            .map(|name| {
                                (Asset::Template(AssetKind::Door, name.clone()), name.clone())
                            })
                            .collect(),
                        Tab::Props => state
                            .setup
                            .models
                            .iter()
                            .map(|path| {
                                let shown = path
                                    .strip_prefix("models/map_objects/")
                                    .unwrap_or(path)
                                    .trim_end_matches(".md3")
                                    .to_string();
                                (Asset::Template(AssetKind::Prop, path.clone()), shown)
                            })
                            .collect(),
                        Tab::Waypoints
                        | Tab::Triggers
                        | Tab::Sounds
                        | Tab::Stores
                        | Tab::Encounters => {
                            let kind = match ui_state.tab {
                                Tab::Waypoints => Kind::Waypoint,
                                Tab::Triggers => Kind::Trigger,
                                Tab::Sounds => Kind::Sound,
                                Tab::Encounters => Kind::Encounter,
                                _ => Kind::Store,
                            };
                            state
                                .setup
                                .templates_of(kind)
                                .iter()
                                .map(|name| (Asset::Object(kind, name.clone()), name.clone()))
                                .collect()
                        }
                        Tab::Rooms => state
                            .setup
                            .rooms
                            .iter()
                            .map(|room| {
                                // A room cut from a Jedi Academy level shows
                                // its level and its name.
                                // A room cut in another project also
                                // shows that project.
                                let shown = room.ja.as_ref().map_or_else(
                                    || room.model.clone(),
                                    |ja| {
                                        let from = (room.ja_from.as_ref())
                                            .and_then(|folder| folder.file_name())
                                            .map_or(String::new(), |name| {
                                                format!(", project {}", name.to_string_lossy())
                                            });
                                        format!(
                                            "{} {} (Jedi Academy{from})",
                                            level_edit::ja_rooms::short(&ja.level),
                                            ja.room
                                        )
                                    },
                                );
                                (Asset::Template(AssetKind::Room, room.model.clone()), shown)
                            })
                            .collect(),
                        Tab::Lamps => (state.lamp_library.found.iter())
                            .map(|lamp| {
                                (Asset::Template(AssetKind::Lamp, lamp.key()), lamp.label())
                            })
                            .collect(),
                        Tab::Effects => state
                            .setup
                            .effects
                            .iter()
                            .map(|path| {
                                let shown = level_edit::effects::played_name(path);
                                (Asset::Effect(path.clone()), shown)
                            })
                            .collect(),
                        Tab::Lights => LIGHT_SHAPES
                            .iter()
                            .map(|(shape, label)| (Asset::Light(shape), label.to_string()))
                            .collect(),
                        Tab::Shapes => SHAPES
                            .iter()
                            .enumerate()
                            .map(|(at, (name, size, _))| {
                                (
                                    Asset::Shape(at),
                                    format!("{name} {}x{}x{} m", size[0], size[1], size[2]),
                                )
                            })
                            .collect(),
                    };
                    // The templates of the game's other levels, each with
                    // its level.
                    let mut items = items;
                    let others = match ui_state.tab {
                        Tab::Placeables => Some((Kind::Placeable, ResType::UTP)),
                        Tab::Creatures => Some((Kind::Creature, ResType::UTC)),
                        Tab::Doors => Some((Kind::Door, ResType::UTD)),
                        Tab::Waypoints => Some((Kind::Waypoint, ResType::UTW)),
                        Tab::Triggers => Some((Kind::Trigger, ResType::UTT)),
                        Tab::Sounds => Some((Kind::Sound, ResType::UTS)),
                        Tab::Stores => Some((Kind::Store, ResType::UTM)),
                        Tab::Encounters => Some((Kind::Encounter, ResType::UTE)),
                        _ => None,
                    };
                    if let (true, Some((kind, restype))) = (ui_state.assets_elsewhere, others) {
                        items.extend(state.elsewhere().names(restype).map(|(name, level)| {
                            (
                                Asset::Object(kind, name.to_string()),
                                format!("{name} ({level})"),
                            )
                        }));
                    }
                    let items: Vec<(Asset, String)> = items
                        .into_iter()
                        .filter(|(_, label)| {
                            search.is_empty() || label.to_ascii_lowercase().contains(&search)
                        })
                        .collect();
                    let items = std::sync::Arc::new(items);
                    ui_state.asset_items = Some((key, items.clone()));
                    items
                }
            };
            // A tile takes its width, its frame's margins and the space
            // after it.
            let stride = TILE
                + egui::Frame::group(ui.style()).total_margin().sum().x
                + ui.spacing().item_spacing.x;
            let per_row = ((width / stride).floor() as usize).max(1);
            let rows = items.len().div_ceil(per_row);
            let mut placed: Option<Asset> = None;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                ui,
                TILE + 26.0,
                rows,
                |ui, range| {
                    for row in range {
                        ui.horizontal(|ui| {
                            for (asset, label) in items.iter().skip(row * per_row).take(per_row) {
                                let picked = ui_state.picked.as_ref() == Some(asset);
                                let tag = asset_tag(state, asset);
                                let response = tile(ui, ctx, ui_state, asset, label, picked, tag);
                                if response.drag_started() {
                                    ui_state.picked = Some(asset.clone());
                                    egui::DragAndDrop::set_payload(ctx, asset.clone());
                                }
                                if response.dragged() {
                                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                                }
                                if response.clicked() {
                                    ui_state.picked = Some(asset.clone());
                                }
                                if response.double_clicked() {
                                    placed = Some(asset.clone());
                                }
                                if let Asset::Object(Kind::Sound, name) = asset {
                                    response.context_menu(|ui| {
                                        let key = state::listen_key(name);
                                        if state.listening.is(&key) {
                                            if ui.button("Stop").clicked() {
                                                state.listening.stop();
                                                ui.close();
                                            }
                                        } else if ui.button("Play its sounds").clicked() {
                                            if let Err(why) = state::listen_template(state, name) {
                                                state.say(why);
                                            }
                                            ui.close();
                                        }
                                    });
                                }
                            }
                        });
                    }
                },
            );
            if let Some(asset) = placed {
                ui_state.picked = Some(asset.clone());
                let at = place_at(state, cameras, None);
                place(state, &asset, at, ui_state.snapping);
            }
        });
}

/// The tag of an asset that another mod brings (`state::mod_tag`): a
/// template or a room's model the game reads from an override folder or
/// from a module's mod file.
fn asset_tag(state: &EditorState, asset: &Asset) -> Option<&'static str> {
    let (name, restype) = match asset {
        Asset::Template(AssetKind::Placeable, name) => (name, ResType::UTP),
        Asset::Template(AssetKind::Creature, name) => (name, ResType::UTC),
        Asset::Template(AssetKind::Door, name) => (name, ResType::UTD),
        Asset::Template(AssetKind::Room, name) => (name, ResType::MDL),
        Asset::Object(kind, name) => (name, kind.list_and_type().1),
        _ => return None,
    };
    state::mod_tag(state, name, restype)
}

/// One asset's tile: its thumbnail (or its kind while it is drawn) and its
/// name. `tag` says that another mod brings the asset (`asset_tag`).
fn tile(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    ui_state: &mut UiState,
    asset: &Asset,
    label: &str,
    picked: bool,
    tag: Option<&'static str>,
) -> egui::Response {
    let frame = egui::Frame::group(ui.style()).fill(if picked {
        ui.visuals().selection.bg_fill
    } else {
        ui.visuals().faint_bg_color
    });
    let shown = frame.show(ui, |ui| {
        ui.set_width(TILE);
        ui.vertical_centered(|ui| {
            let size = egui::vec2(TILE - 8.0, TILE - 8.0);
            match asset {
                Asset::Template(kind, name) => {
                    let key = (*kind, name.clone());
                    if !ui_state.textures.contains_key(&key)
                        && let Some(Some(picture)) = ui_state.thumbnails.get(*kind, name)
                    {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [thumbs::SIDE, thumbs::SIDE],
                            picture,
                        );
                        let texture = ctx.load_texture(
                            format!("thumbnail {kind:?} {name}"),
                            image,
                            egui::TextureOptions::LINEAR,
                        );
                        ui_state.textures.insert(key.clone(), texture);
                    }
                    match ui_state.textures.get(&key) {
                        Some(texture) => {
                            ui.add(
                                egui::Image::new((texture.id(), size)).sense(egui::Sense::hover()),
                            );
                        }
                        None => {
                            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                            ui.painter()
                                .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
                            let what = match ui_state.thumbnails.made.get(&key) {
                                Some(None) => "no picture",
                                _ => "drawing...",
                            };
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                what,
                                egui::FontId::proportional(11.0),
                                ui.visuals().weak_text_color(),
                            );
                        }
                    }
                }
                Asset::Object(kind, name) => {
                    // The object's marker, as the view shows it.
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let icon = match kind {
                        Kind::Sound => crate::markers::Icon::Sound,
                        Kind::Waypoint if level_edit::git::is_post(name) => {
                            crate::markers::Icon::Post
                        }
                        Kind::Waypoint => crate::markers::Icon::Waypoint,
                        Kind::Trigger | Kind::Encounter => crate::markers::Icon::Alert,
                        Kind::Placeable => crate::markers::Icon::Square,
                        _ => crate::markers::Icon::Dot,
                    };
                    let [r, g, b, _] = state::kind_colour(*kind).1;
                    let icons = ui_state
                        .icons
                        .get_or_insert_with(|| crate::markers::Icons::new(ctx));
                    let shown = egui::Rect::from_center_size(rect.center(), rect.size() * 0.5);
                    icons.paint(ui.painter(), icon, shown, egui::Color32::from_rgb(r, g, b));
                }
                Asset::Effect(_) => {
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let icons = ui_state
                        .icons
                        .get_or_insert_with(|| crate::markers::Icons::new(ctx));
                    let shown = egui::Rect::from_center_size(rect.center(), rect.size() * 0.5);
                    let [r, g, b, _] = state::EFFECT_MARKER.1;
                    icons.paint(
                        ui.painter(),
                        crate::markers::Icon::Flame,
                        shown,
                        egui::Color32::from_rgb(r, g, b),
                    );
                }
                Asset::Light(_) => {
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let icons = ui_state
                        .icons
                        .get_or_insert_with(|| crate::markers::Icons::new(ctx));
                    let shown = egui::Rect::from_center_size(rect.center(), rect.size() * 0.5);
                    icons.paint(
                        ui.painter(),
                        crate::markers::Icon::Light,
                        shown,
                        egui::Color32::from_rgb(255, 230, 160),
                    );
                }
                Asset::Shape(_) => {
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect.shrink(18.0),
                        2.0,
                        egui::Color32::from_rgba_unmultiplied(60, 230, 255, 150),
                    );
                }
            }
            ui.add(egui::Label::new(egui::RichText::new(label).small()).truncate());
        });
    });
    let id = egui::Id::new(("asset", format!("{asset:?}")));
    let response = ui.interact(shown.response.rect, id, egui::Sense::click_and_drag());
    let Some(tag) = tag else {
        return response.on_hover_text(label);
    };
    // The tag, in the tile's top left corner.
    let font = egui::FontId::proportional(10.0);
    let text = ui
        .painter()
        .layout_no_wrap(tag.to_string(), font, egui::Color32::BLACK);
    let corner = shown.response.rect.left_top() + egui::vec2(3.0, 3.0);
    let back = egui::Rect::from_min_size(corner, text.size() + egui::vec2(6.0, 2.0));
    ui.painter()
        .rect_filled(back, 3.0, egui::Color32::from_rgb(240, 190, 80));
    ui.painter()
        .galley(corner + egui::vec2(3.0, 1.0), text, egui::Color32::BLACK);
    let from = match tag {
        "override" => "A mod brings it: the game reads it from its override folder.",
        _ => "A mod brings it: the game reads it from the level's mod file.",
    };
    response.on_hover_text(format!("{label}\n{from}"))
}

fn viewport(
    root: &mut egui::Ui,
    state: &mut EditorState,
    ui_state: &mut UiState,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    looking: bool,
) {
    let ctx = root.ctx().clone();
    let ctx = &ctx;
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(root, |ui| {
            let rect = ui.max_rect();
            ui_state.viewport = rect;
            let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            let Some((camera, eye)) = cameras.iter().next() else {
                return;
            };
            let painter = ctx
                .layer_painter(egui::LayerId::new(
                    egui::Order::Background,
                    egui::Id::new("editor labels"),
                ))
                .with_clip_rect(rect);
            let icons = ui_state
                .icons
                .get_or_insert_with(|| crate::markers::Icons::new(ctx));
            let shown = crate::markers::layout(state, camera, eye);
            // Under the markers, so each line ends at a marker's edge.
            crate::links::in_view(&painter, state, camera, eye);
            crate::markers::draw(&painter, icons, &shown, state.editor.selected);
            labels(&painter, state, camera, eye);
            reach_in_view(&painter, state, camera, eye);
            // An edit that was refused says why over the view for a few
            // seconds, where the eye is.
            if let Some((when, why)) = &state.refusal
                && when.elapsed().as_secs_f32() < 5.0
            {
                let galley = painter.layout(
                    why.clone(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                    (rect.width() - 80.0).max(120.0),
                );
                let at = egui::pos2(rect.center().x - galley.size().x * 0.5, rect.top() + 16.0);
                let back = egui::Rect::from_min_size(at, galley.size()).expand(8.0);
                painter.rect_filled(back, 4.0, egui::Color32::from_rgb(150, 40, 40));
                painter.galley(at, galley, egui::Color32::WHITE);
            }
            let on_outline = outline_in_view(
                ui,
                &response,
                &painter,
                state,
                ui_state,
                (camera, eye),
                cameras,
            );

            // The gizmo on the selection.
            let mut on_gizmo = on_outline;
            if let Some(target) = state.editor.selected
                && let Some(modes) = modes(ui_state.tool, target)
                && let Some(transform) = gizmo_transform(state, target)
                && !looking
                && !on_outline
            {
                // The gizmo keeps handles of the modes it had before
                // (transform-gizmo 0.11): a change of modes makes a new one.
                if ui_state.gizmo_modes != Some(modes) {
                    ui_state.gizmo = Gizmo::default();
                    ui_state.gizmo_modes = Some(modes);
                }
                let to_bevy = Mat4::from_quat(game_to_bevy());
                let view = eye.to_matrix().inverse() * to_bevy;
                let projection = camera.clip_from_view();
                // The camera draws the whole window, under the panels, so
                // the gizmo is placed on the camera's viewport, not on the
                // view's part of the window: placed on that part, it stood
                // off its object toward the part's middle.
                let drawn_on = camera
                    .logical_viewport_rect()
                    .map(|area| {
                        egui::Rect::from_min_max(
                            egui::pos2(area.min.x, area.min.y),
                            egui::pos2(area.max.x, area.max.y),
                        )
                    })
                    .unwrap_or(rect);
                ui_state.gizmo.update_config(GizmoConfig {
                    view_matrix: rows(view),
                    projection_matrix: rows(projection),
                    viewport: drawn_on,
                    modes,
                    // A lamp turns about its own axes, to lay it along a
                    // wall.
                    orientation: if ui_state.tool == Tool::Scale
                        || (ui_state.tool == Tool::Rotate && matches!(target, Target::Lamp(_)))
                    {
                        GizmoOrientation::Local
                    } else {
                        GizmoOrientation::Global
                    },
                    snapping: ui_state.snapping,
                    snap_distance: ui_state.snap_distance,
                    snap_angle: ui_state.snap_angle.to_radians(),
                    snap_scale: 0.1,
                    pixels_per_point: ctx.pixels_per_point(),
                    ..GizmoConfig::default()
                });
                let several = state.editor.selection().len() > 1;
                // A free move (the gizmo's middle dragged): the mouse wheel
                // takes the selection nearer or farther.
                let eye_at = (game_to_bevy().inverse() * eye.translation()).as_dvec3();
                let shares = free_move(ctx, ui_state, (camera, eye), &transform, eye_at);
                let given = match shares {
                    Some((before, _)) => {
                        let mut given = transform;
                        given.translation = to_mint(state::along_view(
                            eye_at,
                            from_mint(transform.translation),
                            1.0 / before,
                        ));
                        given
                    }
                    None => transform,
                };
                // The gizmo is drawn in the view's part of the window alone:
                // a selection behind a panel does not put its handles over
                // the outliner or the details.
                let interacted = ui
                    .scope(|ui| {
                        ui.set_clip_rect(ui.clip_rect().intersect(rect));
                        ui_state.gizmo.interact(ui, &[given])
                    })
                    .inner;
                // The gizmo moves the selection on its plane; the free move
                // takes it along the line from the camera from there.
                let interacted = interacted.map(|(result, mut moved)| {
                    if let (Some((_, after)), GizmoResult::Translation { .. }) = (shares, result) {
                        for moved in &mut moved {
                            moved.translation = to_mint(state::along_view(
                                eye_at,
                                from_mint(moved.translation),
                                after,
                            ));
                        }
                    }
                    (result, moved)
                });
                if let Some((result, moved)) = interacted {
                    on_gizmo = true;
                    if several && let Some(moved) = moved.first() {
                        // Everything selected moves, or turns round the
                        // middle of it, by what the gizmo did.
                        let was = (transform.translation, gizmo_facing(&transform));
                        let edits = match result {
                            GizmoResult::Translation { .. } => {
                                let by = [
                                    moved.translation.x - was.0.x,
                                    moved.translation.y - was.0.y,
                                    moved.translation.z - was.0.z,
                                ]
                                .map(|v| v as f32);
                                state.editor.moves_by(by)
                            }
                            GizmoResult::Rotation { .. } | GizmoResult::Arcball { .. } => {
                                let turned =
                                    (gizmo_facing(moved) - was.1 + 540.0).rem_euclid(360.0) - 180.0;
                                let centre = [was.0.x, was.0.y, was.0.z].map(|v| v as f32);
                                state.editor.turns_round(
                                    centre,
                                    turned,
                                    collision::trig::sin_cos_deg(turned),
                                )
                            }
                            GizmoResult::Scale { .. } => Vec::new(),
                        };
                        if !edits.is_empty() {
                            if ui_state.dragging {
                                state.apply_many_continuing(edits);
                            } else {
                                state.apply_many(edits);
                                ui_state.dragging = true;
                            }
                        }
                    } else if let Some(moved) = moved.first() {
                        let lamp = match target {
                            Target::Lamp(at) => state.editor.project.kjk.lamps.get(at).cloned(),
                            _ => None,
                        };
                        for edit in gizmo_edits(target, lamp.as_ref(), &result, moved) {
                            drag_edit(state, ui_state, edit);
                        }
                    }
                }
                on_gizmo |= ui_state.gizmo.is_focused();
            }
            state.dragging = ui_state.dragging;
            if !ctx.input(|input| input.pointer.primary_down()) && !ctx.egui_is_using_pointer() {
                // A room let go with snapping on meets the nearest seam. A
                // door let go near a doorway stands in it. Anything else
                // lines up with what stands near it.
                if ui_state.dragging && ui_state.snapping {
                    match state.editor.selected {
                        Some(Target::Room(at)) => {
                            state::snap_to_seam(state, at, true);
                        }
                        Some(target @ Target::Object(Kind::Door, _)) => {
                            let _ = state::snap_door(state, target, state::DOOR_REACH, true);
                        }
                        Some(_) if ui_state.tool == Tool::Move => {
                            let _ = state::align_selected(state, state::ALIGN_REACH, true);
                        }
                        _ => {}
                    }
                }
                ui_state.dragging = false;
                ui_state.free_move = None;
            }

            // A click picks what is under the pointer. With the measuring
            // tool it marks the place under the pointer instead.
            if response.clicked()
                && !on_gizmo
                && let Some(at) = response.interact_pointer_pos()
            {
                if ui_state.tool == Tool::Measure {
                    let place = place_at(state, cameras, Some(Vec2::new(at.x, at.y)));
                    if ui_state.measure.len() >= 2 {
                        ui_state.measure.clear();
                    }
                    ui_state.measure.push(place);
                } else {
                    state.pending_pick = Some(Vec2::new(at.x, at.y));
                    state.pending_pick_adds = ctx.input(|input| input.modifiers.command);
                }
            }
            if ui_state.tool == Tool::Measure && !ui_state.measure.is_empty() {
                let to_screen = |point: [f32; 3]| {
                    let world = game_to_bevy() * Vec3::from_array(point);
                    (camera.world_to_viewport(eye, world).ok()).map(|at| egui::pos2(at.x, at.y))
                };
                let painter = ui.painter_at(rect);
                let colour = egui::Color32::from_rgb(120, 220, 255);
                let marks: Vec<Option<egui::Pos2>> = ui_state
                    .measure
                    .iter()
                    .map(|place| to_screen(*place))
                    .collect();
                for mark in marks.iter().flatten() {
                    painter.circle_filled(*mark, 4.0, colour);
                }
                if let [a, b] = ui_state.measure.as_slice() {
                    let flat = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
                    let all = (flat.powi(2) + (a[2] - b[2]).powi(2)).sqrt();
                    if let [Some(from), Some(to)] = marks.as_slice() {
                        painter.line_segment([*from, *to], egui::Stroke::new(2.0, colour));
                        let middle =
                            egui::pos2((from.x + to.x) * 0.5, (from.y + to.y) * 0.5 - 14.0);
                        let text = format!(
                            "{all:.2} m  ({flat:.2} m along the floor, {:.2} m up)",
                            b[2] - a[2]
                        );
                        let galley = painter.layout_no_wrap(
                            text,
                            egui::FontId::proportional(14.0),
                            egui::Color32::WHITE,
                        );
                        let back = egui::Rect::from_center_size(middle, galley.size()).expand(5.0);
                        painter.rect_filled(back, 3.0, egui::Color32::from_black_alpha(190));
                        painter.galley(
                            back.min + egui::vec2(5.0, 5.0),
                            galley,
                            egui::Color32::WHITE,
                        );
                    }
                }
            }
            // A drag from an empty part of the view selects everything whose
            // marker lies in its box (with Ctrl, joins the selection).
            if response.drag_started_by(egui::PointerButton::Primary)
                && !on_gizmo
                && !looking
                && let Some(at) = response.interact_pointer_pos()
            {
                ui_state.boxing = Some(at);
            }
            if let Some(from) = ui_state.boxing {
                let to = ctx.pointer_latest_pos().unwrap_or(from);
                let area = egui::Rect::from_two_pos(from, to);
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("selection box"),
                ));
                painter.rect_filled(
                    area,
                    0.0,
                    egui::Color32::from_rgba_unmultiplied(120, 180, 255, 30),
                );
                painter.rect_stroke(
                    area,
                    0.0,
                    egui::Stroke::new(1.5, egui::Color32::from_rgb(120, 180, 255)),
                    egui::StrokeKind::Inside,
                );
                if !ctx.input(|input| input.pointer.primary_down()) {
                    ui_state.boxing = None;
                    if area.width() > 4.0 && area.height() > 4.0 {
                        let shown = crate::markers::layout(state, camera, eye);
                        let inside: Vec<Target> = (shown.iter())
                            .filter(|marker| area.contains(marker.at))
                            .map(|marker| marker.target)
                            .collect();
                        let mut all = if ctx.input(|input| input.modifiers.command) {
                            state.editor.selection()
                        } else {
                            Vec::new()
                        };
                        for target in inside {
                            if !all.contains(&target) {
                                all.push(target);
                            }
                        }
                        state.say(format!("{} selected", all.len()));
                        state.editor.select_all(all);
                    }
                }
            }
            // An asset dropped from the picker is placed where the pointer
            // meets the level.
            if let Some(asset) = response.dnd_release_payload::<Asset>() {
                let point = ctx.pointer_interact_pos().map(|at| Vec2::new(at.x, at.y));
                let at = place_at(state, cameras, point);
                place(state, &asset, at, ui_state.snapping);
            }
        });
}

/// The selected trigger's or encounter's outline over the view, its points
/// dragged along the floor; and an outline being drawn, each click on the
/// floor its next point. Whether the pointer is busy with it.
fn outline_in_view(
    ui: &egui::Ui,
    response: &egui::Response,
    painter: &egui::Painter,
    state: &mut EditorState,
    ui_state: &mut UiState,
    (camera, eye): (&Camera, &GlobalTransform),
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> bool {
    let to_screen = |point: [f32; 3]| {
        let world = game_to_bevy() * Vec3::from_array(point);
        camera
            .world_to_viewport(eye, world)
            .ok()
            .map(|at| egui::pos2(at.x, at.y))
    };
    let floor_under = |state: &mut EditorState, at: egui::Pos2| {
        state::ray_at(cameras, Vec2::new(at.x, at.y)).and_then(|ray| state::ground(state, ray))
    };
    let colour = egui::Color32::from_rgb(255, 200, 60);
    // The links between floors: an arc from where characters leave to
    // where they land, green where they cross it, red where not.
    let checks = state.nav_facts.as_ref().map(|nav| nav.links.clone());
    for (at, link) in state.editor.project.kjk.links.iter().enumerate() {
        let tint = match checks.as_ref().and_then(|checks| checks.get(at)) {
            Some(Ok(())) => egui::Color32::from_rgb(120, 220, 120),
            Some(Err(_)) => egui::Color32::from_rgb(235, 110, 90),
            None => egui::Color32::from_rgb(230, 210, 120),
        };
        let top = link.from[2].max(link.to[2]) + 0.8;
        let points: Vec<egui::Pos2> = (0..=12)
            .filter_map(|step| {
                let t = step as f32 / 12.0;
                let along: [f32; 3] = std::array::from_fn(|axis| {
                    link.from[axis] + (link.to[axis] - link.from[axis]) * t
                });
                let lift = 4.0 * t * (1.0 - t) * (top - along[2].max(link.from[2].min(link.to[2])));
                to_screen([along[0], along[1], along[2] + lift.max(0.0)])
            })
            .collect();
        painter.add(egui::Shape::line(points, egui::Stroke::new(2.5, tint)));
        if let (Some(leave), Some(land)) = (to_screen(link.from), to_screen(link.to)) {
            painter.circle_stroke(leave, 5.0, egui::Stroke::new(2.0, tint));
            painter.circle_filled(land, 5.0, tint);
        }
    }
    if let Some((kind, leave)) = ui_state.placing_link {
        if let Some(from) = leave
            && let Some(at) = to_screen(from)
        {
            painter.circle_stroke(at, 6.0, egui::Stroke::new(2.0, colour));
        }
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
            && let Some(point) = floor_under(state, at)
        {
            match leave {
                None => ui_state.placing_link = Some((kind, Some(point))),
                Some(from) => {
                    state.apply(Edit::AddLink(level_edit::area::NavLink {
                        kind,
                        from,
                        to: point,
                    }));
                    ui_state.placing_link = None;
                }
            }
        }
        if ui.input(|input| input.key_pressed(egui::Key::Escape)) {
            ui_state.placing_link = None;
        }
        return true;
    }
    // The selected encounter's spawn points, each a ring with a tick the
    // way it faces.
    if let Some(Target::Object(Kind::Encounter, index)) = state.editor.selected {
        let spawn_colour = egui::Color32::from_rgb(255, 120, 200);
        for (at, facing) in level_edit::git::spawn_points(&state.editor.project.git, index) {
            let ahead = [
                at[0] + facing.cos() * 0.6,
                at[1] + facing.sin() * 0.6,
                at[2],
            ];
            if let (Some(centre), Some(tip)) = (to_screen(at), to_screen(ahead)) {
                painter.circle_stroke(centre, 6.0, egui::Stroke::new(2.0, spawn_colour));
                painter.line_segment([centre, tip], egui::Stroke::new(2.0, spawn_colour));
            }
        }
    }
    // Spawn points placed: each click on the floor adds one.
    if let Some(target @ Target::Object(Kind::Encounter, index)) = ui_state.spawning {
        if state.editor.selected != Some(target) {
            ui_state.spawning = None;
        } else {
            if response.clicked()
                && let Some(at) = response.interact_pointer_pos()
                && let Some(point) = floor_under(state, at)
            {
                let mut points = level_edit::git::spawn_points(&state.editor.project.git, index);
                points.push((point, 0.0));
                state.apply(Edit::SpawnPoints(target, points));
            }
            if ui.input(|input| {
                input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Escape)
            }) {
                ui_state.spawning = None;
            }
            return true;
        }
    }
    // Drawing: each click adds a point, Enter ends, Escape drops it.
    if let Some((_, points)) = ui_state.drawing.as_mut() {
        if response.clicked()
            && let Some(at) = response.interact_pointer_pos()
            && let Some(point) = floor_under(state, at)
        {
            points.push(point);
        }
        let shown: Vec<egui::Pos2> = points.iter().filter_map(|p| to_screen(*p)).collect();
        painter.add(egui::Shape::line(
            shown.clone(),
            egui::Stroke::new(2.0, colour),
        ));
        for at in &shown {
            painter.circle_filled(*at, 5.0, colour);
        }
        let (enter, escape) = ui.input(|input| {
            (
                input.key_pressed(egui::Key::Enter),
                input.key_pressed(egui::Key::Escape),
            )
        });
        if escape {
            ui_state.drawing = None;
        } else if enter {
            finish_drawing(state, ui_state);
        }
        return true;
    }
    // The selected outline's points, dragged along the floor.
    let Some(Target::Object(kind @ (Kind::Trigger | Kind::Encounter), index)) =
        state.editor.selected
    else {
        ui_state.dragging_point = None;
        return false;
    };
    let Some(entry) = state.editor.entry(kind, index) else {
        return false;
    };
    let at = entry.position;
    let points: Vec<[f32; 3]> = level_edit::git::outline(&state.editor.project.git, kind, index)
        .iter()
        .map(|p| [p[0] + at[0], p[1] + at[1], p[2] + at[2]])
        .collect();
    let shown: Vec<Option<egui::Pos2>> = points.iter().map(|p| to_screen(*p)).collect();
    let mut ring: Vec<egui::Pos2> = shown.iter().flatten().copied().collect();
    if let Some(first) = ring.first().copied() {
        ring.push(first);
    }
    painter.add(egui::Shape::line(ring, egui::Stroke::new(1.5, colour)));
    for at in shown.iter().flatten() {
        painter.circle_stroke(*at, 5.0, egui::Stroke::new(2.0, colour));
    }
    let pointer = ui.ctx().pointer_hover_pos();
    if response.drag_started()
        && let Some(pointer) = pointer
    {
        ui_state.dragging_point = shown
            .iter()
            .enumerate()
            .filter_map(|(at, point)| Some((at, point.as_ref()?.distance(pointer))))
            .filter(|(_, distance)| *distance <= 9.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(at, _)| at);
    }
    let Some(dragged) = ui_state.dragging_point else {
        return false;
    };
    if response.dragged()
        && let Some(pointer) = pointer
        && let Some(point) = floor_under(state, pointer)
    {
        let mut moved = points;
        if let Some(slot) = moved.get_mut(dragged) {
            *slot = point;
        }
        drag_edit(
            state,
            ui_state,
            Edit::Outline(Target::Object(kind, index), moved),
        );
    }
    if !response.dragged() {
        ui_state.dragging_point = None;
    }
    true
}

/// Ends the outline being drawn: three points or more become the object's
/// outline.
fn finish_drawing(state: &mut EditorState, ui_state: &mut UiState) {
    if let Some((target, points)) = ui_state.drawing.take() {
        if points.len() < 3 {
            state.say("an outline has three points or more: it was not changed");
        } else {
            state.apply(Edit::Outline(target, points));
        }
    }
}

/// A trigger's or an encounter's outline in the details: its points, each
/// moved by its numbers or removed, a point added, or a new outline drawn
/// in the view.
fn outline_details(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    ui_state: &mut UiState,
    (kind, index): (Kind, usize),
) {
    let target = Target::Object(kind, index);
    let Some(entry) = state.editor.entry(kind, index) else {
        return;
    };
    let at = entry.position;
    let mut points: Vec<[f32; 3]> =
        level_edit::git::outline(&state.editor.project.git, kind, index)
            .iter()
            .map(|p| [p[0] + at[0], p[1] + at[1], p[2] + at[2]])
            .collect();
    ui.separator();
    ui.label(egui::RichText::new("Outline").strong());
    if let Some((drawing, so_far)) = &ui_state.drawing
        && *drawing == target
    {
        ui.label(format!(
            "Click the floor for each corner, in order round it ({} so far). Enter or Done ends, \
             Escape leaves the outline as it was.",
            so_far.len()
        ));
        ui.horizontal(|ui| {
            if ui.button("Done").clicked() {
                finish_drawing(state, ui_state);
            }
            if ui.button("Cancel").clicked() {
                ui_state.drawing = None;
            }
        });
        return;
    }
    ui.label(
        egui::RichText::new("Drag a corner in the view along the floor, or set it here.").small(),
    );
    let mut changed = false;
    let mut remove = None;
    // Three corners are the fewest.
    let removable = points.len() > 3;
    egui::Grid::new("outline points")
        .num_columns(3)
        .show(ui, |ui| {
            for (at, point) in points.iter_mut().enumerate() {
                ui.label(format!("{}", at + 1));
                ui.horizontal(|ui| {
                    for value in point.iter_mut().take(2) {
                        changed |= ui
                            .add(egui::DragValue::new(value).speed(0.05).max_decimals(2))
                            .changed();
                    }
                });
                if ui
                    .add_enabled(removable, egui::Button::new("Remove").small())
                    .clicked()
                {
                    remove = Some(at);
                }
                ui.end_row();
            }
        });
    if let Some(at) = remove {
        points.remove(at);
        changed = true;
    }
    ui.horizontal(|ui| {
        if ui
            .button("Add a corner")
            .on_hover_text("Adds a corner halfway along the longest side.")
            .clicked()
            && points.len() >= 2
        {
            let longest = (0..points.len())
                .max_by(|a, b| {
                    let side = |at: &usize| {
                        let (p, q) = (points[*at], points[(*at + 1) % points.len()]);
                        (p[0] - q[0]).hypot(p[1] - q[1])
                    };
                    side(a).total_cmp(&side(b))
                })
                .unwrap_or(0);
            let (p, q) = (points[longest], points[(longest + 1) % points.len()]);
            points.insert(
                longest + 1,
                [
                    (p[0] + q[0]) / 2.0,
                    (p[1] + q[1]) / 2.0,
                    (p[2] + q[2]) / 2.0,
                ],
            );
            changed = true;
        }
        if ui
            .button("Draw a new outline")
            .on_hover_text("Click the floor in the view for each corner.")
            .clicked()
        {
            ui_state.drawing = Some((target, Vec::new()));
        }
    });
    if changed {
        if ui.ctx().input(|input| input.pointer.any_down()) {
            drag_edit(state, ui_state, Edit::Outline(target, points));
        } else {
            state.apply(Edit::Outline(target, points));
        }
    }
}

/// The nearest objects' kinds and tags, over the view.
fn labels(painter: &egui::Painter, state: &EditorState, camera: &Camera, eye: &GlobalTransform) {
    let eye_game = game_to_bevy().inverse() * eye.translation();
    let mut near: Vec<(f32, String, Vec3, bool)> = Vec::new();
    for entry in state.editor.entries() {
        if state.hidden.contains(&entry.kind)
            && state.editor.selected != Some(Target::Object(entry.kind, entry.index))
        {
            continue;
        }
        let at = Vec3::from_array(entry.position);
        let tag = if entry.tag.is_empty() {
            entry.template.clone()
        } else {
            entry.tag.clone()
        };
        let selected = state.editor.selected == Some(Target::Object(entry.kind, entry.index));
        near.push((at.distance(eye_game), tag, at, selected));
    }
    for (at, prop) in state.editor.project.kjk.props.iter().enumerate() {
        let selected = state.editor.selected == Some(Target::Prop(at));
        let position = Vec3::from_array(prop.position);
        near.push((
            position.distance(eye_game),
            prop.name.clone(),
            position,
            selected,
        ));
    }
    for (at, shape) in state.editor.project.kjk.boxes.iter().enumerate() {
        let selected = state.editor.selected == Some(Target::Box(at));
        let position = Vec3::from_array(shape.centre);
        near.push((
            position.distance(eye_game),
            shape.name.clone(),
            position,
            selected,
        ));
    }
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut shown = 0;
    for (_, text, at, selected) in near {
        if shown >= LABELS && !selected {
            continue;
        }
        let world = game_to_bevy() * (at + Vec3::Z * 0.8);
        let Ok(point) = camera.world_to_viewport(eye, world) else {
            continue;
        };
        let point = egui::pos2(point.x, point.y);
        if !painter.clip_rect().contains(point) {
            continue;
        }
        shown += 1;
        let colour = if selected {
            egui::Color32::YELLOW
        } else {
            egui::Color32::from_gray(230)
        };
        let galley = painter.layout_no_wrap(text, egui::FontId::proportional(12.0), colour);
        let rect = egui::Rect::from_center_size(point, galley.size()).expand(2.0);
        painter.rect_filled(rect, 3.0, egui::Color32::from_black_alpha(140));
        painter.galley(rect.min + egui::vec2(2.0, 2.0), galley, colour);
    }
}

/// How many straight pieces each ring of a light's reach is drawn with.
const REACH_PIECES: usize = 48;

/// The selected light's reach over the view, in its colour: three rings
/// round it (level, and upright along X and along Y) as wide as it
/// reaches, and a line the way it shines when it has a cone. Drawn each
/// frame over the scene, so selecting a light never draws the scene again,
/// and the rings follow a light while it is dragged.
pub(crate) fn reach_in_view(
    painter: &egui::Painter,
    state: &EditorState,
    camera: &Camera,
    eye: &GlobalTransform,
) {
    if !state.show_lights {
        return;
    }
    let light = match state.editor.selected {
        Some(Target::Light(at)) => state.editor.project.kjk.lights.get(at).cloned(),
        Some(Target::Fitted(at)) => state
            .editor
            .seen_lights
            .get(at)
            .map(|(light, _)| light.clone()),
        _ => None,
    };
    let Some(light) = light else {
        return;
    };
    let Some((at, _)) = state
        .editor
        .selected
        .and_then(|target| state.editor.placement(target))
    else {
        return;
    };
    let centre = Vec3::from_array(at);
    let to_screen = |point: Vec3| {
        let world = game_to_bevy() * point;
        let ahead = (world - eye.translation()).dot(*eye.forward()) > 0.0;
        (ahead)
            .then(|| camera.world_to_viewport(eye, world).ok())
            .flatten()
            .map(|at| egui::pos2(at.x, at.y))
    };
    let strongest = light.colour.iter().fold(0.0f32, |a, b| a.max(*b)).max(1e-6);
    let [r, g, b] = light
        .colour
        .map(|v| (v / strongest * 255.0).clamp(0.0, 255.0) as u8);
    let stroke = egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(r, g, b, 200));
    let rings: [(Vec3, Vec3); 3] = [(Vec3::X, Vec3::Y), (Vec3::X, Vec3::Z), (Vec3::Y, Vec3::Z)];
    for (u, v) in rings {
        // A ring is cut where it passes behind the camera.
        let mut piece: Vec<egui::Pos2> = Vec::new();
        for step in 0..=REACH_PIECES {
            let angle = step as f32 / REACH_PIECES as f32 * std::f32::consts::TAU;
            let point = centre + (u * angle.cos() + v * angle.sin()) * light.radius;
            match to_screen(point) {
                Some(shown) => piece.push(shown),
                None => {
                    if piece.len() > 1 {
                        painter.add(egui::Shape::line(std::mem::take(&mut piece), stroke));
                    }
                    piece.clear();
                }
            }
        }
        if piece.len() > 1 {
            painter.add(egui::Shape::line(piece, stroke));
        }
    }
    let aim = Vec3::from_array(light.aim);
    if light.cone > 0.0 && aim.length_squared() > 0.0 {
        let tip = centre + aim.normalize() * light.radius;
        if let (Some(from), Some(to)) = (to_screen(centre), to_screen(tip)) {
            painter.line_segment([from, to], stroke);
        }
    }
}

/// The rows of the details panel for a light placed by hand: its colour
/// and strength, reach, cone, strip and reason. A change hands the light
/// as changed to `changed`.
fn light_rows(
    ui: &mut egui::Ui,
    light: &level_edit::PlacedLight,
    changed: &mut dyn FnMut(level_edit::PlacedLight),
) {
    let mut light = light.clone();
    // Only what a widget says the user changed is an edit: a colour made
    // again every frame from its hue would round, and be an edit each frame.
    let mut touched = false;
    // The light stores one colour: the picker's colour times the strength.
    // The picker's colour and the strength the user set are remembered as
    // they were set, while they still make the light's colour. Otherwise
    // each frame would make the picker's colour full again and move its
    // darkness into the strength, so the picker could not be dragged down
    // and the strength would fall to 0 while it was dragged.
    let id = ui.make_persistent_id("light colour");
    let remembered: Option<([f32; 3], f32)> = ui.data(|data| data.get_temp(id));
    let (mut hue, mut power) = match remembered {
        Some((hue, power))
            if (hue.iter().zip(light.colour)).all(|(h, c)| (h * power - c).abs() <= 1e-4) =>
        {
            (hue, power)
        }
        _ => {
            let strength = light.colour.iter().fold(0.0f32, |a, b| a.max(*b));
            let hue = light.colour.map(|value| {
                if strength > 0.0 {
                    value / strength
                } else {
                    1.0
                }
            });
            (hue, strength)
        }
    };
    let before = hue;
    let mut recoloured = false;
    ui.label("Colour");
    ui.horizontal(|ui| {
        recoloured |= ui.color_edit_button_rgb(&mut hue).changed();
        recoloured |= ui
            .add(
                egui::DragValue::new(&mut power)
                    .range(0.0..=4.0)
                    .speed(0.01)
                    .prefix("strength "),
            )
            .changed();
    });
    ui.end_row();
    // A black picker colour would lose the colour's hue for good: the
    // strength turns a light off instead.
    if hue.iter().all(|value| *value < 1.0 / 255.0) {
        hue = before;
    }
    if recoloured {
        light.colour = hue.map(|value| value * power);
        ui.data_mut(|data| data.insert_temp(id, (hue, power)));
        touched = true;
    }
    ui.label("Reach");
    touched |= ui
        .add(
            egui::DragValue::new(&mut light.radius)
                .range(0.1..=50.0)
                .speed(0.05)
                .suffix(" m"),
        )
        .changed();
    ui.end_row();
    ui.label("Cone");
    touched |= ui
        .add(egui::Slider::new(&mut light.cone, 0.0..=0.9))
        .on_hover_text("0 shines all ways, 0.2 the half in front of a panel, 0.7 a spotlight")
        .changed();
    if light.cone > 0.0 && light.aim == [0.0; 3] {
        light.aim = [0.0, 0.0, -1.0];
    }
    ui.end_row();
    ui.label("Strip");
    touched |= ui
        .add(
            egui::DragValue::new(&mut light.half)
                .range(0.0..=20.0)
                .speed(0.05)
                .suffix(" m each way"),
        )
        .changed();
    if light.half > 0.0 && light.way == [0.0; 3] {
        light.way = [1.0, 0.0, 0.0];
    }
    ui.end_row();
    ui.label("Reason");
    touched |= ui.text_edit_singleline(&mut light.reason).changed();
    ui.end_row();
    if touched {
        changed(light);
    }
}

/// The details of a lamp placed by hand: its model and mesh, which kind of
/// fitting its texture is read as, and the reason it is there.
/// The details of a Jedi Academy ambient effect: its file, the way it
/// points over the level, and how often it plays. Gives whether a value is
/// being dragged, and whether a drag ended.
fn effect_rows(
    ui: &mut egui::Ui,
    effect: &level_edit::effects::AmbientEffect,
    changed: &mut dyn FnMut(level_edit::effects::AmbientEffect),
) -> (bool, bool) {
    use level_edit::effects::{EVERY_LEAST, EVERY_MOST};
    let mut effect = effect.clone();
    let (mut dragging, mut released) = (false, false);
    ui.label("Effect");
    ui.label(effect.played_name())
        .on_hover_text("The effect file, from the player's own Jedi Academy");
    ui.end_row();
    let mut touched = false;
    ui.label("Pitch").on_hover_text(
        "Degrees the effect points over the level: 90 points straight up, 0 along its facing, -90 straight down",
    );
    let response = ui.add(
        egui::DragValue::new(&mut effect.pitch)
            .range(-90.0..=90.0)
            .speed(1.0)
            .suffix("°"),
    );
    touched |= response.changed();
    dragging |= response.dragged();
    released |= response.drag_stopped() || response.lost_focus();
    ui.end_row();
    ui.label("Plays every").on_hover_text(
        "Seconds between two plays. Jedi Academy plays its ambient effects every 0.2 seconds unless its map says otherwise",
    );
    let response = ui.add(
        egui::DragValue::new(&mut effect.every)
            .range(EVERY_LEAST..=EVERY_MOST)
            .speed(0.01)
            .suffix(" s"),
    );
    touched |= response.changed();
    dragging |= response.dragged();
    released |= response.drag_stopped() || response.lost_focus();
    ui.end_row();
    if touched {
        changed(effect);
    }
    (dragging, released)
}

fn lamp_rows(
    ui: &mut egui::Ui,
    lamp: &level_edit::PlacedLamp,
    changed: &mut dyn FnMut(level_edit::PlacedLamp),
) {
    let mut lamp = lamp.clone();
    let mut touched = false;
    ui.label("Mesh");
    ui.label(if lamp.mesh.is_empty() {
        "the whole model"
    } else {
        lamp.mesh.as_str()
    });
    ui.end_row();
    ui.label("Kind of").on_hover_text(
        "Whether the fitted lights read its glowing texture as a room's fitting or a placeable's",
    );
    ui.horizontal(|ui| {
        touched |= ui
            .radio_value(&mut lamp.on_placeable, false, "room")
            .changed();
        touched |= ui
            .radio_value(&mut lamp.on_placeable, true, "placeable")
            .changed();
    });
    ui.end_row();
    for (label, degrees, hint) in [
        (
            "Pitch",
            &mut lamp.pitch,
            "Degrees the lamp is tipped about its own side: its front goes down for more",
        ),
        (
            "Roll",
            &mut lamp.roll,
            "Degrees the lamp is rolled about its own front",
        ),
    ] {
        ui.label(label).on_hover_text(hint);
        touched |= ui
            .add(
                egui::DragValue::new(degrees)
                    .speed(0.5)
                    .range(-180.0..=180.0)
                    .suffix("°")
                    .max_decimals(1),
            )
            .on_hover_text(hint)
            .changed();
        ui.end_row();
    }
    ui.label("Reason");
    touched |= ui.text_edit_singleline(&mut lamp.reason).changed();
    ui.end_row();
    if touched {
        changed(lamp);
    }
}

/// The toolbar's Lighting menu: how the view lights the rooms, which bake
/// the Baked view shows, whether the lights are drawn, and walk mode.
/// The Markers menu: which kinds of object show their markers and labels
/// over the view. A hidden kind still shows what is selected of it.
fn markers_menu(ui: &mut egui::Ui, state: &mut EditorState) {
    let label = if state.hidden.is_empty() {
        "Markers".to_string()
    } else {
        format!("Markers ({} hidden)", state.hidden.len())
    };
    ui.menu_button(label, |ui| {
        ui.label(egui::RichText::new("The kinds whose markers and labels the view shows.").small());
        for kind in Kind::ALL {
            let mut shown = !state.hidden.contains(&kind);
            if ui.checkbox(&mut shown, kind_label(kind)).changed() {
                state.hidden.retain(|hidden| *hidden != kind);
                if !shown {
                    state.hidden.push(kind);
                }
            }
        }
        ui.separator();
        if ui.button("Show all").clicked() {
            state.hidden.clear();
        }
        if ui.button("Hide all").clicked() {
            state.hidden = Kind::ALL.to_vec();
        }
    });
}

/// A text field that keeps what is typed until the typing is done. The
/// text to change `now` to, once the field is left with another text.
fn kept_text(
    ui: &mut egui::Ui,
    ui_state: &mut UiState,
    key: &str,
    now: &str,
    (width, rows): (f32, usize),
) -> Option<String> {
    let text = (ui_state.texts.entry(key.to_string())).or_insert_with(|| now.to_string());
    let edit = if rows > 1 {
        egui::TextEdit::multiline(text).desired_rows(rows)
    } else {
        egui::TextEdit::singleline(text)
    };
    let response = ui.add(edit.desired_width(width));
    if response.lost_focus() {
        let typed = ui_state.texts.remove(key)?;
        let typed = typed.split_whitespace().collect::<Vec<_>>().join(" ");
        return (typed != now).then_some(typed);
    }
    if !response.has_focus() {
        ui_state.texts.remove(key);
    }
    None
}

/// A field of the window that stays as typed until its button is pressed.
fn new_text(
    ui: &mut egui::Ui,
    ui_state: &mut UiState,
    key: &str,
    hint: &str,
    width: f32,
) -> String {
    let text = ui_state.texts.entry(key.to_string()).or_default();
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(width),
    );
    text.trim().to_string()
}

/// The window of the project's own items and quests (`level_edit::items`,
/// `level_edit::item_props`, `level_edit::journal`). Each change is one of
/// the `edit item` and `edit quest` commands.
fn adventure_window(ctx: &egui::Context, state: &mut EditorState, ui_state: &mut UiState) {
    if !ui_state.adventure_open {
        return;
    }
    let mut open = true;
    let mut command = None;
    egui::Window::new("Items and quests")
        .open(&mut open)
        .default_width(460.0)
        .default_height(520.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.heading("Items");
                adventure_items(ui, state, ui_state, &mut command);
                ui.separator();
                ui.heading("Quests");
                adventure_quests(ui, state, ui_state, &mut command);
            });
        });
    ui_state.adventure_open = open;
    if let Some(command) = command {
        state.pending_command = Some(command);
    }
}

/// The items of the project in its window, and a new one.
fn adventure_items(
    ui: &mut egui::Ui,
    state: &EditorState,
    ui_state: &mut UiState,
    command: &mut Option<String>,
) {
    let items: Vec<(String, level_edit::Template)> = (state.editor.project.templates.iter())
        .filter(|(_, made)| made.restype == ResType::UTI)
        .map(|(name, made)| (name.clone(), made.clone()))
        .collect();
    if items.is_empty() {
        ui.label("The project has no items of its own yet.");
    }
    // What the selection can be given: a creature's, a store's or a
    // container's items.
    let holder = match state.editor.selected {
        Some(Target::Object(kind @ (Kind::Creature | Kind::Placeable | Kind::Store), index)) => {
            state
                .editor
                .entry(kind, index)
                .map(|entry| state.tag_of(&entry))
        }
        _ => None,
    };
    let few = items.len() <= 3;
    for (name, made) in items {
        let (shown, text) = level_edit::items::shown(&made.gff);
        let title = if shown.is_empty() {
            name.clone()
        } else {
            format!("{shown} ({name})")
        };
        egui::CollapsingHeader::new(title)
            .id_salt(("item", &name))
            .default_open(few)
            .show(ui, |ui| {
                ui.label(format!("A copy of the game's item {}.", made.from));
                ui.horizontal_wrapped(|ui| {
                    let give = match &holder {
                        Some(tag) => format!("Give it to {tag}"),
                        None => "Give it to the selected object".to_string(),
                    };
                    if ui
                        .add_enabled(holder.is_some(), egui::Button::new(give))
                        .on_hover_text(
                            "Adds the item to what the selected creature carries, to what the selected store \
                             sells, or to what the selected container holds. A placeable is made a container \
                             first if it is not one.",
                        )
                        .on_disabled_hover_text(
                            "Select a creature, a store or a placeable in the level first.",
                        )
                        .clicked()
                    {
                        *command = Some(format!("item give {name}"));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Name");
                    let key = format!("item name {name}");
                    if let Some(typed) = kept_text(ui, ui_state, &key, &shown, (260.0, 1))
                        && !typed.is_empty()
                    {
                        *command = Some(format!("item name {name} {typed}"));
                    }
                });
                ui.label("Description");
                let key = format!("item text {name}");
                if let Some(typed) = kept_text(ui, ui_state, &key, &text, (400.0, 3)) {
                    *command = Some(format!("item text {name} {typed}"));
                }
                ui.horizontal(|ui| {
                    ui.label("Worth in credits");
                    let cost = made.gff.root.int("Cost").unwrap_or(0).to_string();
                    let key = format!("item cost {name}");
                    if let Some(typed) = kept_text(ui, ui_state, &key, &cost, (80.0, 1)) {
                        *command = Some(format!("item number {name} Cost {typed}"));
                    }
                });
                ui.label("Properties");
                let properties = state::properties_shown(state, &made.gff);
                if properties.is_empty() {
                    ui.label(egui::RichText::new("It has none.").small());
                }
                for (at, property) in properties.iter().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.small_button("Remove").clicked() {
                            *command = Some(format!("item property {name} remove {at}"));
                        }
                        ui.label(property);
                    });
                }
                new_property(ui, state, ui_state, &name, command);
                if ui
                    .button("Take the item out")
                    .on_hover_text("Takes it out of the project. What carries it keeps its name.")
                    .clicked()
                {
                    *command = Some(format!("item drop {name}"));
                }
            });
    }
    ui.add_space(4.0);
    ui.label(
        "A new item starts as a copy of one of the game's items. Find that item by its name as the \
         game shows it, or by its file name, then name the new item.",
    );
    egui::Grid::new("new item").num_columns(2).show(ui, |ui| {
        ui.label("Copy of");
        let base = ui_state.texts.entry("new item base".into()).or_default();
        let picked =
            crate::choose::field(ui, "new item base", base, ("Energy Shield", 220.0), || {
                crate::choose::game_items(state).to_vec()
            });
        let base = base.trim().to_ascii_lowercase();
        let base_known = state.setup.items.binary_search(&base).is_ok();
        ui.end_row();
        // A base picked while the name is empty names the new item after
        // it, which the field can then change.
        if picked.picked.is_some()
            && ui_state
                .texts
                .get("new item name")
                .is_none_or(|name| name.trim().is_empty())
        {
            ui_state
                .texts
                .insert("new item name".into(), free_item_name(state, &base));
        }
        ui.label("Name");
        let name =
            new_text(ui, ui_state, "new item name", "kje_reward", 220.0).to_ascii_lowercase();
        ui.end_row();
        let problem = if base.is_empty() {
            Some("Pick the item to copy.".to_string())
        } else if !base_known {
            Some(format!("The game has no item {base}."))
        } else if name.is_empty() {
            Some("Name the new item.".to_string())
        } else if let Some(problem) = level_edit::items::name_problem(&name) {
            Some(problem)
        } else if item_name_taken(state, &name) {
            Some(format!("There is an item named {name} already."))
        } else {
            None
        };
        ui.label("");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(problem.is_none(), egui::Button::new("New item"))
                .clicked()
            {
                *command = Some(format!("item new {name} {base}"));
                ui_state.texts.remove("new item name");
                ui_state.texts.remove("new item base");
            }
            if let Some(problem) = problem {
                ui.label(egui::RichText::new(problem).small().weak());
            }
        });
        ui.end_row();
    });
}

/// Whether an item named `name` exists already: the project's or the
/// game's.
fn item_name_taken(state: &EditorState, name: &str) -> bool {
    state.editor.project.templates.contains_key(name)
        || state.setup.resolver.find(name, ResType::UTI).is_some()
}

/// A name for a new item copied from `base` that no item has: `p_` and
/// the base's name without its leading `g_`, with a number after it when
/// that is taken.
fn free_item_name(state: &EditorState, base: &str) -> String {
    let stem: String = base.trim_start_matches("g_").chars().take(11).collect();
    (0..100)
        .map(|n| {
            if n == 0 {
                format!("p_{stem}")
            } else {
                format!("p_{stem}_{n}")
            }
        })
        .find(|name| !item_name_taken(state, name))
        .unwrap_or_else(|| format!("p_{stem}"))
}

/// The pickers that choose a property for the item `name`, and the button
/// that adds it.
fn new_property(
    ui: &mut egui::Ui,
    state: &EditorState,
    ui_state: &mut UiState,
    name: &str,
    command: &mut Option<String>,
) {
    let (kind, mut subtype, mut value, mut param) =
        (ui_state.new_property.get(name).copied()).unwrap_or((None, 0, 0, 0));
    let mut chosen = kind;
    ui.horizontal_wrapped(|ui| {
        let kinds = table_rows(state, ui_state, "itempropdef");
        let shown = kind.map_or_else(|| "Add a property".to_string(), |row| row_name(&kinds, row));
        let id = format!("property kind {name}");
        if let Some(row) = row_picker(ui, ui_state, &kinds, (&id, &shown), kind) {
            (chosen, subtype, value, param) = (Some(row), 0, 0, 0);
        }
        let Some(row) = chosen else {
            return;
        };
        let Some(of) = state::property_kind(state, row) else {
            return;
        };
        let mut pick = |ui: &mut egui::Ui, table: &str, what: &str, now: &mut u16| {
            let rows = table_rows(state, ui_state, table);
            if rows.is_empty() {
                return;
            }
            let id = format!("property {what} {name}");
            if let Some(row) = row_picker(
                ui,
                ui_state,
                &rows,
                (&id, &row_name(&rows, *now)),
                Some(*now),
            ) {
                *now = row;
            }
        };
        if let Some(table) = &of.subtypes {
            pick(ui, table, "subtype", &mut subtype);
        }
        if let Some((_, table)) = &of.values {
            pick(ui, table, "value", &mut value);
        }
        if let Some((_, table)) = state::param_table(state, &of, subtype) {
            pick(ui, &table, "parameter", &mut param);
        }
        if ui.button("Add").clicked() {
            *command = Some(format!(
                "item property {name} add {row} {subtype} {value} {param}"
            ));
        }
    });
    ui_state
        .new_property
        .insert(name.to_string(), (chosen, subtype, value, param));
}

/// The project's own quests in its window, and a new one. A quest of the
/// project names itself; the game's quests take their names from the talk
/// table and are left out.
fn adventure_quests(
    ui: &mut egui::Ui,
    state: &EditorState,
    ui_state: &mut UiState,
    command: &mut Option<String>,
) {
    let journal = state::journal(state);
    let quests: Vec<level_edit::journal::Shown> = (level_edit::journal::tags(&journal).iter())
        .filter_map(|tag| level_edit::journal::quest(&journal, tag))
        .filter(|(_, name, _)| !name.is_empty())
        .collect();
    if quests.is_empty() {
        ui.label("The project has no quests of its own yet.");
    }
    for (tag, name, entries) in quests {
        egui::CollapsingHeader::new(format!("{name} ({tag})"))
            .id_salt(("quest", &tag))
            .show(ui, |ui| {
                for (id, text, end) in &entries {
                    ui.horizontal(|ui| {
                        let closes = if *end { ", closes the quest" } else { "" };
                        ui.label(format!("{id}{closes}"));
                        let key = format!("quest entry {tag} {id}");
                        if let Some(typed) = kept_text(ui, ui_state, &key, text, (340.0, 2))
                            && !typed.is_empty()
                        {
                            let verb = if *end { "end" } else { "entry" };
                            *command = Some(format!("quest {verb} {tag} {id} {typed}"));
                        }
                    });
                    let adders = ui_state.quest_adders.of(state, &tag, *id);
                    ui.indent(("quest adders", &tag, *id), |ui| {
                        quest_adders(ui, ui_state, (&tag, *id), &adders, command);
                    });
                }
                ui.label("A new entry: its number, its text, and whether it closes the quest.");
                ui.horizontal(|ui| {
                    let id = new_text(ui, ui_state, &format!("new entry id {tag}"), "10", 40.0);
                    let text = new_text(ui, ui_state, &format!("new entry text {tag}"), "", 260.0);
                    let good = id.parse::<u32>().is_ok() && !text.is_empty();
                    for (button, verb) in [("Add", "entry"), ("Add as the last", "end")] {
                        if ui.add_enabled(good, egui::Button::new(button)).clicked() {
                            *command = Some(format!("quest {verb} {tag} {id} {text}"));
                            ui_state.texts.remove(&format!("new entry id {tag}"));
                            ui_state.texts.remove(&format!("new entry text {tag}"));
                        }
                    }
                });
                if ui.button("Take the quest out").clicked() {
                    *command = Some(format!("quest drop {tag}"));
                }
            });
    }
    ui.add_space(4.0);
    ui.label("A new quest: the tag scripts know it by, and the name the journal shows.");
    ui.horizontal(|ui| {
        let tag = new_text(ui, ui_state, "new quest tag", "kje_quest", 110.0);
        let name = new_text(ui, ui_state, "new quest name", "The Lost Tomb", 220.0);
        let good = level_edit::journal::tag_problem(&tag).is_none() && !name.is_empty();
        if ui
            .add_enabled(good, egui::Button::new("New quest"))
            .clicked()
        {
            *command = Some(format!("quest new {tag} {name}"));
            ui_state.texts.remove("new quest tag");
            ui_state.texts.remove("new quest name");
        }
    });
}

/// Under a quest's entry: what adds it, each with a button that opens it.
/// A script opens in the script viewer, and a conversation's line in the
/// conversation window. An entry that nothing adds offers a new script that
/// adds it.
fn quest_adders(
    ui: &mut egui::Ui,
    ui_state: &mut UiState,
    (tag, id): (&str, u32),
    adders: &[crate::quest_links::Adder],
    command: &mut Option<String>,
) {
    use crate::quest_links::Adder;
    let small = |text: String| egui::RichText::new(text).small();
    if adders.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(small("Nothing in the project adds this entry yet.".into()));
            if ui
                .small_button("New script")
                .on_hover_text(
                    "Makes a script that adds this entry, and opens it. \
                     The script does nothing until an event runs it, \
                     such as a creature's OnDeath or the level's OnEnter.",
                )
                .clicked()
            {
                *command = Some(format!("quest script {tag} {id}"));
                ui_state.view_tab = ViewTab::Source;
            }
        });
        return;
    }
    for adder in adders {
        ui.horizontal_wrapped(|ui| {
            ui.label(small(crate::quest_links::sentences(adder)));
            match adder {
                Adder::Script { name, .. } => {
                    if ui
                        .small_button("Open")
                        .on_hover_text(format!("Opens the script {name} in the script viewer."))
                        .clicked()
                    {
                        *command = Some(format!("script edit {name}"));
                        ui_state.view_tab = ViewTab::Source;
                    }
                }
                Adder::Line {
                    conversation,
                    side,
                    index,
                    ..
                } => {
                    if ui
                        .small_button("Open")
                        .on_hover_text(format!(
                            "Opens the conversation {conversation} with this line selected."
                        ))
                        .clicked()
                    {
                        ui_state.dialog_open = Some(conversation.clone());
                        ui_state.dialog_selected = Some((*side, *index, None));
                    }
                }
            }
        });
    }
}

/// The window that offers the work an editor that did not close left
/// (`recover`): to take it, or to discard it.
fn recovery_window(ctx: &egui::Context, state: &mut EditorState) {
    let Some(folder) = state.recovery.clone() else {
        return;
    };
    let mut command = None;
    egui::Window::new("Work that was not saved")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
        .show(ctx, |ui| {
            ui.label(format!(
                "The editor did not close the project {} the last time. A copy of the work that was not saved was kept, {}.",
                state.editor.project.name,
                crate::recover::describe(&folder)
            ));
            ui.label(
                egui::RichText::new(
                    "Recover takes it into the level as one edit, which Undo takes back. Nothing is written to the project until you save.",
                )
                .small(),
            );
            ui.horizontal(|ui| {
                if ui.button("Recover").clicked() {
                    command = Some("recover");
                }
                if ui.button("Discard").clicked() {
                    command = Some("recover discard");
                }
            });
        });
    if let Some(command) = command {
        state.pending_command = Some(command.to_string());
    }
}

/// The Lighting that Play starts kjk with (`play_lighting`), beside the Play
/// buttons. A choice is kept for the next time the editor opens.
fn play_lighting_menu(ui: &mut egui::Ui, state: &mut EditorState) {
    let shown = crate::play_lighting::choice(state.play_lighting);
    let mut chosen = None;
    egui::ComboBox::from_id_salt("play-lighting")
        .selected_text(shown.name)
        .width(130.0)
        .show_ui(ui, |ui| {
            for choice in &crate::play_lighting::CHOICES {
                if ui
                    .selectable_label(choice.value == state.play_lighting, choice.name)
                    .on_hover_text(choice.hint)
                    .clicked()
                {
                    chosen = Some(choice.value);
                }
            }
        })
        .response
        .on_hover_text(format!(
            "The lighting Play and Play here start kjk with. {}. The game's options are not changed.",
            shown.hint
        ));
    if let Some(value) = chosen {
        state.pending_command = Some(format!("play lighting {}", value.unwrap_or("options")));
    }
}

fn lighting_menu(ui: &mut egui::Ui, state: &mut EditorState) {
    use kjk_game::fitted_lights::Mode;
    ui.menu_button("Lighting", |ui| {
        // Whose the level's lights and lamps placed by hand are.
        if state.editor.project.map.is_none() {
            let own = state::lights_own(state);
            ui.label(
                egui::RichText::new(if own {
                    "This level's lights and lamps placed by hand are the project's own. A save keeps them in the project, and play with the project uses them."
                } else {
                    "This level's lights and lamps placed by hand are kjk's. A change makes them the project's own."
                })
                .small(),
            );
            ui.separator();
        }
        // The rules the project changed, and the step that gives all of its
        // lighting to kjk's light data.
        {
            let kinds = state::kinds_own(state).len();
            let levels = state::levels_own(state);
            let lights = state::lights_own(state);
            if kinds > 0 || levels {
                let mut held = Vec::new();
                if kinds > 0 {
                    held.push(format!("{kinds} kinds of fitting it changed"));
                }
                if levels {
                    held.push("this level's room levels".to_string());
                }
                ui.label(
                    egui::RichText::new(format!(
                        "The project keeps {} as its own. They apply to this project only.",
                        held.join(" and ")
                    ))
                    .small(),
                );
            }
            // The bake the project keeps, which play with the project uses.
            let kept = state::bake_kept(state);
            ui.label(
                egui::RichText::new(match &kept {
                    Some(_) => {
                        "The project keeps a bake of this level. Play with the project is lit with it."
                    }
                    None => {
                        "The project keeps no bake of this level. Play with the project is lit as the game stores it."
                    }
                })
                .small(),
            );
            let shown_is_kept = kept.is_some() && state.bake == kept;
            if ui
                .add_enabled(
                    state.bake.is_some() && !shown_is_kept,
                    egui::Button::new("Keep the bake shown in the project"),
                )
                .on_hover_text(
                    "Copies the lightmaps of the bake the Baked view shows into the project's folder, in place of the bake kept before.",
                )
                .on_disabled_hover_text(
                    "The Baked view shows no bake, or it shows the one the project keeps already.",
                )
                .clicked()
            {
                state.pending_command = Some("bake keep".into());
                ui.close();
            }
            if kept.is_some() && ui.button("Take the project's bake out").clicked() {
                state.pending_command = Some("bake drop".into());
                ui.close();
            }
            ui.separator();
            let own = lights || kinds > 0 || levels;
            if ui
                .add_enabled(
                    own && state.setup.light_data.is_some(),
                    egui::Button::new("Publish to kjk's light data"),
                )
                .on_hover_text(
                    "Writes the project's own lights, lamps, kinds of fitting and room levels to data/lights, where the next build of kjk takes them for every player, then saves the project without them.",
                )
                .on_disabled_hover_text(
                    "Publishing needs lighting of the project's own, and kjk built from source (or KJK_LIGHT_DATA set to a data/lights folder).",
                )
                .clicked()
            {
                state.pending_command = Some("publish".into());
                ui.close();
            }
            ui.separator();
        }
        for (mode, label, hint) in [
            (Mode::Off, "Game", "The game's stored lightmaps"),
            (
                Mode::Live,
                "Fitted live",
                "Half the stored light and the fitted lights, as the trial plays",
            ),
            (
                Mode::LampsOnly,
                "Lamps only",
                "None of the stored light: only what the fitted lights give",
            ),
            (
                Mode::Baked,
                "Baked",
                "A bake of the fitted lights in the rooms' lightmaps",
            ),
        ] {
            if ui
                .radio(state.light_view == mode, label)
                .on_hover_text(hint)
                .clicked()
            {
                state.light_view = mode;
                if mode == Mode::Baked && state.bake.is_none() {
                    state.bake = state.setup.bakes.first().cloned();
                }
            }
        }
        if !state.setup.bakes.is_empty() {
            ui.menu_button("Bake shown", |ui| {
                for bake in state.setup.bakes.clone() {
                    let label = bake
                        .ancestors()
                        .nth(2)
                        .and_then(|folder| folder.file_name())
                        .map_or_else(
                            || bake.display().to_string(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                    if ui
                        .radio(state.bake.as_ref() == Some(&bake), label)
                        .clicked()
                    {
                        state.bake = Some(bake);
                        state.light_view = Mode::Baked;
                    }
                }
            });
        }
        ui.separator();
        ui.checkbox(&mut state.show_lights, "Show lights   F4");
        ui.checkbox(&mut state.show_levels, "Room levels");
        ui.separator();
        let baking = state.bake_job.is_some();
        // A kept bake made before the lights were changed.
        if state::bake_stale(state) {
            ui.colored_label(
                egui::Color32::from_rgb(255, 190, 90),
                "The project's bake is older than the level's lights.",
            );
            if ui
                .add_enabled(
                    !baking && state.setup.tools,
                    egui::Button::new("Bake again and keep"),
                )
                .on_hover_text(
                    "Saves, bakes the level from its lights as they are now, and keeps the new bake in the project in place of the old one",
                )
                .on_disabled_hover_text(if baking {
                    "A bake is running."
                } else {
                    crate::state::NO_TOOLS
                })
                .clicked()
            {
                state.pending_command = Some("bake renew".into());
                ui.close();
            }
            ui.separator();
        }
        if ui
            .add_enabled(!baking && state.setup.tools, egui::Button::new("Bake now"))
            .on_hover_text(
                "Saves, then bakes the area's lightmaps from its fitted lights as the view has them, \
                 as data/lights/bake.toml sets the module; the bake is shown when it is done",
            )
            .on_disabled_hover_text(if baking {
                "A bake is running."
            } else {
                crate::state::NO_TOOLS
            })
            .clicked()
        {
            state.pending_command = Some("bake".into());
            ui.close();
        }
        if state.setup.bakes.len() >= 2
            && ui
                .button("Compare with the bake before")
                .on_hover_text("Shows the newest bake and the one before it in turn")
                .clicked()
        {
            state.pending_command = Some("bake compare".into());
        }
        if ui
            .button("Walk   F7")
            .on_hover_text("Saves, then walks the level in play's walk mode, lit as the view is")
            .clicked()
        {
            state.pending_command = Some("walk".into());
            ui.close();
        }
        ui.separator();
        ui.label(
            egui::RichText::new(match &state.setup.light_data {
                Some(folder) => format!(
                    "Publish writes the project's own lights to {}",
                    folder.join("fittings.toml").display()
                ),
                None => "No light data folder found (set KJK_LIGHT_DATA)".to_string(),
            })
            .small()
            .weak(),
        );
    });
}

/// The selected fitted light's kind of fitting, changed for every lamp of
/// it in every level of the game: whether it is a lamp, its strength,
/// reach, colour and cone, and why it was changed.
fn kind_section(ui: &mut egui::Ui, state: &mut EditorState) {
    let kind = match state::selected_kind(state) {
        Ok(kind) => kind,
        Err(why) => {
            ui.label(egui::RichText::new(why).small().weak());
            return;
        }
    };
    ui.separator();
    ui.label(egui::RichText::new(format!("Kind {} (on {}s)", kind.texture, kind.on())).strong());
    ui.label(
        egui::RichText::new("A change lights every lamp of this kind, in every level.")
            .small()
            .weak(),
    );
    let mut changed = kind.clone();
    // Only what a widget says the user changed is an edit (`light_rows`).
    let mut touched = false;
    egui::Grid::new("kind").num_columns(2).show(ui, |ui| {
        ui.label("Lamp");
        touched |= ui.checkbox(&mut changed.lamp, "sheds light").changed();
        ui.end_row();
        ui.label("Form");
        ui.label(changed.form.as_str());
        ui.end_row();
        ui.label("Strength");
        touched |= ui
            .add_enabled(
                changed.lamp,
                egui::DragValue::new(&mut changed.strength)
                    .range(0.0..=4.0)
                    .speed(0.01),
            )
            .changed();
        ui.end_row();
        ui.label("Reach");
        touched |= ui
            .add_enabled(
                changed.lamp,
                egui::DragValue::new(&mut changed.reach)
                    .range(0.1..=20.0)
                    .speed(0.05)
                    .suffix(" m"),
            )
            .changed();
        ui.end_row();
        ui.label("Colour");
        ui.add_enabled_ui(changed.lamp, |ui| {
            touched |= ui.color_edit_button_rgb(&mut changed.colour).changed();
        });
        ui.end_row();
        ui.label("Cone");
        touched |= ui
            .add_enabled(
                changed.lamp,
                egui::Slider::new(&mut changed.cone, 0.0..=0.9),
            )
            .changed();
        ui.end_row();
        ui.label("Reason");
        touched |= ui.text_edit_singleline(&mut changed.reason).changed();
        ui.end_row();
    });
    if touched {
        state::set_kind(state, changed);
    }
}

/// The Room levels window: each room's level as its stored lightmaps show
/// it, and the level the bake aims it at, changed here and saved to the
/// module's levels file.
fn room_levels(ctx: &egui::Context, state: &mut EditorState) {
    if !state.show_levels {
        return;
    }
    let mut open = true;
    let mut edits: Vec<(String, Option<f32>)> = Vec::new();
    egui::Window::new("Room levels")
        .open(&mut open)
        .default_width(420.0)
        .show(ctx, |ui| {
            let entry = &state.bake_entry;
            let source = match (&entry.levels, entry.share) {
                (Some(file), _) => format!("aims at the levels of {file}"),
                (None, Some(share)) => format!(
                    "aims at {share} times each room's level{}",
                    entry.ceiling.map_or_else(String::new, |most| format!(", {most} at most"))
                ),
                (None, None) => "has no bake setting".into(),
            };
            ui.label(format!(
                "The bake of {} {source}, of the {} level{}{}.",
                state.editor.project.module,
                entry.of,
                if entry.even { "" } else { ", with no even light" },
                if entry.sun { ", under its sun" } else { "" },
            ));
            ui.label(
                egui::RichText::new(
                    "Stored and lit are each room's level from its stored lightmaps, about as the bake measures them. A target set here makes the module aim at its file's levels.",
                )
                .small()
                .weak(),
            );
            let Some(measures) = state.room_measures.clone() else {
                ui.label("Measuring the rooms (not in the Baked view)...");
                return;
            };
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                egui::Grid::new("levels").num_columns(4).striped(true).show(ui, |ui| {
                    for title in ["Room", "Stored", "Lit", "Target"] {
                        ui.label(egui::RichText::new(title).strong());
                    }
                    ui.end_row();
                    for measure in measures {
                        ui.label(&measure.room);
                        ui.label(format!("{:.3}", measure.original));
                        ui.label(format!("{:.3}", measure.lit));
                        match state.editor.project.levels.get(&measure.room).copied() {
                            Some(mut level) => {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add(egui::DragValue::new(&mut level).range(0.0..=4.0).speed(0.002).max_decimals(4))
                                        .changed()
                                    {
                                        edits.push((measure.room.clone(), Some(level)));
                                    }
                                    if ui.small_button("x").on_hover_text("No target").clicked() {
                                        edits.push((measure.room.clone(), None));
                                    }
                                });
                            }
                            None => {
                                if ui.small_button("Set").on_hover_text("Aims at its lit level as it is").clicked() {
                                    edits.push((measure.room.clone(), Some(measure.lit)));
                                }
                            }
                        }
                        ui.end_row();
                    }
                });
            });
        });
    state.show_levels = open;
    for (room, level) in edits {
        state.apply(Edit::SetLevel(room, level));
    }
}
