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
}

/// The asset picker's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Lights,
    Rooms,
    Placeables,
    Creatures,
    Props,
    Shapes,
    Waypoints,
    Triggers,
    Sounds,
    Stores,
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
}

/// The window's own state, apart from the project's.
#[derive(Resource)]
pub struct UiState {
    pub tool: Tool,
    /// The File menu's and the prompt's state (`file`).
    pub(crate) file: crate::file::FileState,
    snapping: bool,
    snap_distance: f32,
    snap_angle: f32,
    outliner_search: String,
    tab: Tab,
    asset_search: String,
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
    fonts_set: bool,
    /// The behaviour section's text fields: each event's script, and the
    /// item to add, as typed, for the selection they were typed for.
    typed: BTreeMap<String, String>,
    typed_for: Option<Target>,
    /// The editor's count of changes when the fields were filled.
    typed_at: u64,
    /// The field whose matches are listed under the section.
    completing: Option<String>,
    /// The script the viewer shows, its tab, and the scripts read so far.
    viewing: Option<String>,
    view_tab: ViewTab,
    read_scripts: BTreeMap<String, crate::script_view::ScriptView>,
    /// The editor's `script_round` the scripts above were read at.
    read_round: u32,
}

/// The script viewer's tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ViewTab {
    Source,
    Summary,
    Listing,
}

impl Default for UiState {
    fn default() -> UiState {
        UiState {
            tool: Tool::Move,
            file: crate::file::FileState::default(),
            snapping: false,
            snap_distance: STEP_M,
            snap_angle: TURN_DEG,
            outliner_search: String::new(),
            tab: Tab::Placeables,
            asset_search: String::new(),
            picked: None,
            gizmo: Gizmo::default(),
            gizmo_modes: None,
            dragging: false,
            thumbnails: Thumbnails::default(),
            textures: BTreeMap::new(),
            store: store::Store::open(),
            viewport: egui::Rect::NOTHING,
            fonts_set: false,
            typed: BTreeMap::new(),
            typed_for: None,
            typed_at: 0,
            completing: None,
            viewing: None,
            view_tab: ViewTab::Source,
            read_scripts: BTreeMap::new(),
            read_round: 0,
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
                make_thumbnails,
                state::publish,
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
    match state::ray_at(&cameras, point) {
        Some(ray) => {
            let floor = state::ground(state, ray);
            state.editor.selected = state::pick(state, &scene.0, ray, floor);
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
        AssetKind::Prop => level_edit::active::ja_read(state.setup.ja.as_ref()?, name).ok()?,
        AssetKind::Room => resolver.read(name, fmt_odyssey::ResType::MDL)?,
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

/// The selection as the gizmo moves it: where it stands, its turn about Z,
/// and its scale (a prop's, uniform) or size (a box's).
fn gizmo_transform(state: &EditorState, target: Target) -> Option<GizmoTransform> {
    let (position, facing) = state.editor.placement(target)?;
    let scale = match target {
        Target::Prop(at) => Vec3::splat(state.editor.project.kjk.props.get(at)?.scale),
        Target::Box(at) => Vec3::from_array(state.editor.project.kjk.boxes.get(at)?.size),
        Target::Object(..)
        | Target::Room(_)
        | Target::Entry
        | Target::Light(_)
        | Target::Fitted(_) => Vec3::ONE,
    };
    let p = Vec3::from_array(position).as_dvec3();
    let r = bevy::math::DQuat::from_rotation_z(f64::from(facing).to_radians());
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

/// The edits a gizmo's new transform makes of the selection.
fn gizmo_edits(target: Target, result: &GizmoResult, moved: &GizmoTransform) -> Vec<Edit> {
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
                | Target::Light(_)
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
        (Tool::Select, _) => None,
        (Tool::Move, _) => Some(GizmoMode::all_from_kind(GizmoModeKind::Translate)),
        // Everything turns about Z alone.
        (Tool::Rotate, Target::Object(Kind::Trigger | Kind::Encounter, _) | Target::Room(_)) => {
            None
        }
        (Tool::Rotate, _) => Some(EnumSet::only(GizmoMode::RotateZ)),
        (Tool::Scale, Target::Prop(_)) => Some(EnumSet::only(GizmoMode::ScaleUniform)),
        (Tool::Scale, Target::Box(_)) => {
            Some(GizmoMode::ScaleX | GizmoMode::ScaleY | GizmoMode::ScaleZ)
        }
        (Tool::Scale, Target::Object(..) | Target::Room(_) | Target::Entry | Target::Light(_)) => {
            None
        }
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
        Asset::Template(kind @ (AssetKind::Placeable | AssetKind::Creature), name) => {
            let kind = if *kind == AssetKind::Placeable {
                Kind::Placeable
            } else {
                Kind::Creature
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
        match words.as_slice() {
            ["tool", "select"] => ui.tool = Tool::Select,
            ["tool", "move"] => ui.tool = Tool::Move,
            ["tool", "rotate"] => ui.tool = Tool::Rotate,
            ["tool", "scale"] => ui.tool = Tool::Scale,
            ["tab", "rooms"] => ui.tab = Tab::Rooms,
            ["tab", "placeables"] => ui.tab = Tab::Placeables,
            ["tab", "creatures"] => ui.tab = Tab::Creatures,
            ["tab", "props"] => ui.tab = Tab::Props,
            ["tab", "shapes"] => ui.tab = Tab::Shapes,
            ["tab", "waypoints"] => ui.tab = Tab::Waypoints,
            ["tab", "triggers"] => ui.tab = Tab::Triggers,
            ["tab", "sounds"] => ui.tab = Tab::Sounds,
            ["tab", "stores"] => ui.tab = Tab::Stores,
            ["tab", "lights"] => ui.tab = Tab::Lights,
            ["levels", on @ ("on" | "off")] => state.show_levels = *on == "on",
            ["search", rest @ ..] => ui.asset_search = rest.join(" "),
            ["find", rest @ ..] => ui.outliner_search = rest.join(" "),
            ["snap", on @ ("on" | "off")] => ui.snapping = *on == "on",
            ["view"] => ui.viewing = None,
            ["view", script] => ui.viewing = Some(script.to_ascii_lowercase()),
            ["view", "tab", tab] => {
                ui.view_tab = match *tab {
                    "summary" => ViewTab::Summary,
                    "listing" => ViewTab::Listing,
                    _ => ViewTab::Source,
                }
            }
            words => {
                if !crate::file::command(state, &mut ui.file, words) {
                    state.say(format!(
                        "edit ui {}: takes tool, tab, search, find, snap, view, file, quit or confirm",
                        words.join(" ")
                    ));
                }
            }
        }
    }
    let looking = buttons
        .as_ref()
        .is_some_and(|buttons| buttons.pressed(MouseButton::Right));

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
    room_levels(&ctx, state);
    // The buttons' commands, run as their keys run them.
    if let Some(command) = state.pending_command.take() {
        let words: Vec<&str> = command.split_whitespace().collect();
        state::run(state, &words, &cameras);
    }

    // The keys, when no text field has them and the camera is not flying.
    let typing = ctx.egui_wants_keyboard_input();
    let pointer = ctx.pointer_hover_pos();
    let over_view = pointer.is_some_and(|at| ui.viewport.contains(at));
    controls.paused = typing || !(over_view || looking);
    if let Some(keys) = keys
        && !typing
        && !looking
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
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let pressed = |key| keys.just_pressed(key);
    if ctrl {
        let command: &[&str] = if pressed(KeyCode::KeyZ) {
            &["undo"]
        } else if pressed(KeyCode::KeyY) {
            &["redo"]
        } else if pressed(KeyCode::KeyS) {
            &["save"]
        } else if pressed(KeyCode::KeyD) {
            &["duplicate"]
        } else {
            &[]
        };
        if !command.is_empty() {
            state::run(state, command, cameras);
        }
        return;
    }
    for (key, tool) in [
        (KeyCode::KeyQ, Tool::Select),
        (KeyCode::KeyW, Tool::Move),
        (KeyCode::KeyE, Tool::Rotate),
        (KeyCode::KeyR, Tool::Scale),
    ] {
        if pressed(key) {
            ui.tool = tool;
        }
    }
    if pressed(KeyCode::Delete) {
        state::run(state, &["delete"], cameras);
    }
    if pressed(KeyCode::KeyF) {
        frame_selection(state, camera_request, cameras);
    }
    if pressed(KeyCode::F2) {
        state.show_collision = !state.show_collision;
    }
    if pressed(KeyCode::F3) {
        state.show_nav = !state.show_nav;
    }
    if pressed(KeyCode::F4) {
        state.show_lights = !state.show_lights;
    }
    if pressed(KeyCode::F7) {
        state::run(state, &["walk"], cameras);
    }
    if pressed(KeyCode::F5) {
        state::run(state, &["play"], cameras);
    }
    if pressed(KeyCode::F6) {
        state::run(state, &["audit"], cameras);
    }
    if pressed(KeyCode::Escape) {
        state.editor.selected = None;
    }
    if pressed(KeyCode::Enter)
        && let Some(asset) = ui.picked.clone()
    {
        let at = state::place_point(state, cameras);
        place(state, &asset, at, ui.snapping);
    }
}

fn frame_selection(
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
                    "Audit",
                    "F6: save, then find where a body leaves the level",
                    "audit",
                ),
            ] {
                if ui.button(label).on_hover_text(hint).clicked() {
                    state.pending_command = Some(command.to_string());
                }
            }
            ui.checkbox(&mut state.show_collision, "Collision")
                .on_hover_text("F2: walkable floor green, solid surfaces orange, edge walls blue");
            lighting_menu(ui, state);
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
            let entries = state.editor.entries();
            let mut clicked: Option<(Target, bool)> = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for kind in Kind::ALL {
                    let rows: Vec<_> = entries
                        .iter()
                        .filter(|entry| entry.kind == kind)
                        .map(|entry| {
                            let tag = state.tag_of(entry);
                            (entry.index, format!("{tag}  ({})", entry.template))
                        })
                        .filter(|(_, text)| matches(text))
                        .collect();
                    if rows.is_empty() {
                        continue;
                    }
                    egui::CollapsingHeader::new(format!("{} ({})", kind_label(kind), rows.len()))
                        .default_open(matches!(kind, Kind::Creature | Kind::Placeable))
                        .show(ui, |ui| {
                            for (index, text) in rows {
                                let target = Target::Object(kind, index);
                                let row = ui
                                    .selectable_label(state.editor.selected == Some(target), text);
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
                                let target = if label.starts_with("Jedi") {
                                    Target::Prop(at)
                                } else {
                                    Target::Box(at)
                                };
                                let row = ui
                                    .selectable_label(state.editor.selected == Some(target), text);
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
                                let row = ui
                                    .selectable_label(state.editor.selected == Some(target), text);
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
                                let row = ui
                                    .selectable_label(state.editor.selected == Some(target), text);
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
            });
            if let Some((target, frame)) = clicked {
                state.editor.selected = Some(target);
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
            if ui_state.viewing.is_some() {
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
                        messages(ui, state);
                        return;
                    };
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
                        ui.label("Doors, triggers, sounds, encounters and stores are shown only.");
                    }
                    ui.separator();
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
                            // A room does not turn: the layout places rooms
                            // unturned.
                            if !matches!(target, Target::Room(_)) {
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
                                Target::Object(..) | Target::Room(_) | Target::Entry => {}
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
                        });
                        if let Target::Room(_) = target
                            && ui
                                .button("Meet a seam")
                                .on_hover_text(
                                    "Moves the room so that an edge of its floor meets the                                      nearest edge of another room's",
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

/// The most matches listed under a script's or an item's field.
const MATCHES: usize = 8;

/// The field of the item to add, in `UiState::typed`.
const ITEM_FIELD: &str = "item to add";

/// The tag's field, in `UiState::typed`.
const TAG_FIELD: &str = "tag";

/// The names in `names` that hold what `typed` holds, at most `MATCHES`.
fn matches<'n>(names: &'n [String], typed: &str) -> Vec<&'n String> {
    let needle = typed.to_ascii_lowercase();
    names
        .iter()
        .filter(|name| !needle.is_empty() && name.contains(&needle) && **name != needle)
        .take(MATCHES)
        .collect()
}

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
        ui_state.completing = None;
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
            let known = typed.is_empty() || state.setup.scripts.binary_search(typed).is_ok();
            let mut field = egui::TextEdit::singleline(typed)
                .desired_width(150.0)
                .hint_text("none");
            if !known {
                field = field.text_color(egui::Color32::from_rgb(230, 120, 90));
            }
            let field = ui.add(field).on_hover_text(if known {
                "The script this event runs. Type a compiled script's name."
            } else {
                "The install has no compiled script of this name."
            });
            if field.has_focus() {
                ui_state.completing = Some(label.clone());
            }
            if field.lost_focus() && typed.to_ascii_lowercase() != *script {
                changes.push(Change::Script(label.clone(), typed.to_ascii_lowercase()));
            }
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
            ui.end_row();
        }
    });
    // The matches of the script being typed.
    if let Some(label) = ui_state.completing.clone()
        && scripts.iter().any(|(l, _)| *l == label)
    {
        let typed = ui_state
            .typed
            .get(&label)
            .cloned()
            .unwrap_or_else(String::new);
        let found = matches(&state.setup.scripts, &typed);
        if !found.is_empty() {
            ui.label(egui::RichText::new(format!("Scripts for {label}")).small());
            ui.horizontal_wrapped(|ui| {
                for name in found {
                    if ui.small_button(name.as_str()).clicked() {
                        ui_state.typed.insert(label.clone(), name.clone());
                        changes.push(Change::Script(label.clone(), name.clone()));
                        ui_state.completing = None;
                    }
                }
            });
        }
    }
    // A placeable's items.
    if placeable {
        let items = template::items(&gff);
        ui.label(format!("Items it starts with: {}", items.len()));
        for (at, item) in items.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(item);
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
            let field = ui.add(
                egui::TextEdit::singleline(typed)
                    .desired_width(150.0)
                    .hint_text("an item template"),
            );
            if field.has_focus() {
                ui_state.completing = Some(ITEM_FIELD.into());
            }
            let ready = state.setup.items.binary_search(typed).is_ok();
            if ui
                .add_enabled(ready, egui::Button::new("Add item"))
                .clicked()
            {
                changes.push(Change::AddItem(typed.clone()));
                typed.clear();
            }
        });
        if ui_state.completing.as_deref() == Some(ITEM_FIELD) {
            let typed = ui_state
                .typed
                .get(ITEM_FIELD)
                .cloned()
                .unwrap_or_else(String::new);
            ui.horizontal_wrapped(|ui| {
                for name in matches(&state.setup.items, &typed) {
                    if ui.small_button(name.as_str()).clicked() {
                        ui_state.typed.insert(ITEM_FIELD.into(), name.clone());
                    }
                }
            });
        }
    }
    for change in changes {
        state.change_template(target, change);
        // The script fields show the template as it now is.
        ui_state.typed.retain(|label, _| label == ITEM_FIELD);
    }
}

/// The script viewer: what the script `UiState::viewing` names does, as its
/// source, a summary of its compiled form, or its listing
/// (`script_view`).
fn script_viewer(ui: &mut egui::Ui, state: &EditorState, ui_state: &mut UiState) {
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
    // The name and Close on one row, the tabs on the next, so that the
    // header fits a narrow panel.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&name).strong().monospace());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Close").clicked() {
                ui_state.viewing = None;
            }
        });
    });
    // A tab the script has nothing for shows the next that it has.
    let tab = match ui_state.view_tab {
        ViewTab::Source if view.source.is_none() => ViewTab::Summary,
        tab => tab,
    };
    ui.horizontal(|ui| {
        for (each, label, there) in [
            (ViewTab::Source, "Source", view.source.is_some()),
            (ViewTab::Summary, "Summary", view.listing.is_some()),
            (ViewTab::Listing, "Listing", view.listing.is_some()),
        ] {
            ui.add_enabled_ui(there, |ui| {
                if ui.selectable_label(tab == each, label).clicked() {
                    ui_state.view_tab = each;
                }
            });
        }
    });
    let from = match (&view.source, &view.compiled_from) {
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
    if let Some(why) = state.script_errors.get(&name) {
        ui.colored_label(
            ui.visuals().error_fg_color,
            format!("It did not compile at the last save: {why}"),
        );
    }
    egui::ScrollArea::both()
        .auto_shrink(false)
        .show(ui, |ui| match tab {
            ViewTab::Source => {
                if let Some((source, _)) = &view.source {
                    let job = crate::script_view::highlighted(source, ui.visuals().dark_mode);
                    ui.add(egui::Label::new(job).extend());
                }
            }
            ViewTab::Summary => {
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
            }
            ViewTab::Listing => {
                if let Some(listing) = &view.listing {
                    ui.add(
                        egui::Label::new(egui::RichText::new(listing).monospace().size(11.0))
                            .extend(),
                    );
                }
            }
        });
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
                "{} faces are not reached from {}: the game's characters cannot get there.",
                nav.apart, nav.from
            )
        });
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
                    (Tab::Props, "Jedi Academy props"),
                    (Tab::Shapes, "Collision shapes"),
                    (Tab::Waypoints, "Waypoints"),
                    (Tab::Triggers, "Triggers"),
                    (Tab::Sounds, "Sounds"),
                    (Tab::Stores, "Stores"),
                    (Tab::Lights, "Lights"),
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
                ui.label("Drag into the view, or double-click to place at the view's centre.");
            });
            let search = ui_state.asset_search.to_ascii_lowercase();
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
                Tab::Waypoints | Tab::Triggers | Tab::Sounds | Tab::Stores => {
                    let kind = match ui_state.tab {
                        Tab::Waypoints => Kind::Waypoint,
                        Tab::Triggers => Kind::Trigger,
                        Tab::Sounds => Kind::Sound,
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
                        (
                            Asset::Template(AssetKind::Room, room.model.clone()),
                            room.model.clone(),
                        )
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
            let items: Vec<(Asset, String)> = items
                .into_iter()
                .filter(|(_, label)| {
                    search.is_empty() || label.to_ascii_lowercase().contains(&search)
                })
                .collect();
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
                                let response = tile(ui, ctx, ui_state, asset, label, picked);
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

/// One asset's tile: its thumbnail (or its kind while it is drawn) and its
/// name.
fn tile(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    ui_state: &mut UiState,
    asset: &Asset,
    label: &str,
    picked: bool,
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
                Asset::Object(kind, _) => {
                    // The object's marker, in its kind's colour.
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    let [r, g, b, _] = state::kind_colour(*kind).1;
                    let c = rect.center();
                    let (w, h) = (rect.width() * 0.22, rect.height() * 0.34);
                    ui.painter().add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(c.x, c.y - h),
                            egui::pos2(c.x + w, c.y),
                            egui::pos2(c.x, c.y + h),
                            egui::pos2(c.x - w, c.y),
                        ],
                        egui::Color32::from_rgb(r, g, b),
                        egui::Stroke::NONE,
                    ));
                }
                Asset::Light(_) => {
                    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
                    ui.painter().circle_filled(
                        rect.center(),
                        rect.width() * 0.22,
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
    ui.interact(shown.response.rect, id, egui::Sense::click_and_drag())
        .on_hover_text(label)
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
                    egui::Order::Middle,
                    egui::Id::new("editor labels"),
                ))
                .with_clip_rect(rect);
            labels(&painter, state, camera, eye);

            // The gizmo on the selection.
            let mut on_gizmo = false;
            if let Some(target) = state.editor.selected
                && let Some(modes) = modes(ui_state.tool, target)
                && let Some(transform) = gizmo_transform(state, target)
                && !looking
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
                ui_state.gizmo.update_config(GizmoConfig {
                    view_matrix: rows(view),
                    projection_matrix: rows(projection),
                    viewport: rect,
                    modes,
                    orientation: if ui_state.tool == Tool::Scale {
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
                if let Some((result, moved)) = ui_state.gizmo.interact(ui, &[transform]) {
                    on_gizmo = true;
                    if let Some(moved) = moved.first() {
                        for edit in gizmo_edits(target, &result, moved) {
                            drag_edit(state, ui_state, edit);
                        }
                    }
                }
                on_gizmo |= ui_state.gizmo.is_focused();
            }
            state.dragging = ui_state.dragging;
            if !ctx.input(|input| input.pointer.primary_down()) && !ctx.egui_is_using_pointer() {
                // A room let go with snapping on meets the nearest seam.
                if ui_state.dragging
                    && ui_state.snapping
                    && let Some(Target::Room(at)) = state.editor.selected
                {
                    state::snap_to_seam(state, at, true);
                }
                ui_state.dragging = false;
            }

            // A click picks what is under the pointer.
            if response.clicked()
                && !on_gizmo
                && let Some(at) = response.interact_pointer_pos()
            {
                state.pending_pick = Some(Vec2::new(at.x, at.y));
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

/// The nearest objects' kinds and tags, over the view.
fn labels(painter: &egui::Painter, state: &EditorState, camera: &Camera, eye: &GlobalTransform) {
    let eye_game = game_to_bevy().inverse() * eye.translation();
    let mut near: Vec<(f32, String, Vec3, bool)> = Vec::new();
    for entry in state.editor.entries() {
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
    // The colour as a hue at full strength, and its strength apart.
    let strength = light.colour.iter().fold(0.0f32, |a, b| a.max(*b));
    let mut hue = light.colour.map(|value| {
        if strength > 0.0 {
            value / strength
        } else {
            1.0
        }
    });
    let mut power = strength;
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
    if recoloured {
        light.colour = hue.map(|value| value * power);
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

/// The toolbar's Lighting menu: how the view lights the rooms, which bake
/// the Baked view shows, whether the lights are drawn, and walk mode.
fn lighting_menu(ui: &mut egui::Ui, state: &mut EditorState) {
    use kjk_game::fitted_lights::Mode;
    ui.menu_button("Lighting", |ui| {
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
        if ui
            .add_enabled(!baking, egui::Button::new("Bake now"))
            .on_hover_text(
                "Saves, then bakes the area's lightmaps from its fitted lights as the view has them, \
                 as data/lights/bake.toml sets the module; the bake is shown when it is done",
            )
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
                    "Placed lights are saved to {}",
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
