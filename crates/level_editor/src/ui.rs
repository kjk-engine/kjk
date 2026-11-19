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
use level_edit::{CollisionBox, Edit, Kind, Prop, Target};
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
    Placeables,
    Creatures,
    Props,
    Shapes,
}

/// Collision boxes ready to place: a name, and a size in metres.
const SHAPES: [(&str, [f32; 3]); 5] = [
    ("Ledge", [2.0, 1.0, 0.3]),
    ("Platform", [4.0, 4.0, 0.3]),
    ("Wall", [3.0, 0.2, 3.0]),
    ("Pillar", [1.0, 1.0, 3.0]),
    ("Step", [1.0, 1.0, 0.5]),
];

/// What the picker hands over: an asset by kind and name, or a shape.
#[derive(Debug, Clone, PartialEq)]
enum Asset {
    Template(AssetKind, String),
    Shape(usize),
}

/// The window's own state, apart from the project's.
#[derive(Resource)]
pub struct UiState {
    pub tool: Tool,
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
}

impl Default for UiState {
    fn default() -> UiState {
        UiState {
            tool: Tool::Move,
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
        }
    }
}

/// Adds the editor to the game's app: its state, egui, and the systems.
pub fn install(app: &mut App, state: EditorState) {
    app.insert_resource(state)
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
        .add_systems(EguiPrimaryContextPass, panels);
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
            state.editor.selected = state::pick(state, &scene.0, ray);
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
        Target::Object(..) => Vec3::ONE,
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
                Target::Object(..) => Vec::new(),
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
        (Tool::Rotate, _) => Some(EnumSet::only(GizmoMode::RotateZ)),
        (Tool::Scale, Target::Prop(_)) => Some(EnumSet::only(GizmoMode::ScaleUniform)),
        (Tool::Scale, Target::Box(_)) => {
            Some(GizmoMode::ScaleX | GizmoMode::ScaleY | GizmoMode::ScaleZ)
        }
        (Tool::Scale, Target::Object(..)) => None,
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
fn place(state: &mut EditorState, asset: &Asset, at: [f32; 3]) {
    match asset {
        Asset::Template(AssetKind::Placeable, name) => state.apply(Edit::AddObject {
            kind: Kind::Placeable,
            template: name.clone(),
            position: at,
            facing: 0.0,
        }),
        Asset::Template(AssetKind::Creature, name) => state.apply(Edit::AddObject {
            kind: Kind::Creature,
            template: name.clone(),
            position: at,
            facing: 0.0,
        }),
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
        Asset::Shape(at_shape) => {
            let (stem, size) = SHAPES[*at_shape];
            let name = state
                .editor
                .project
                .kjk
                .free_name(&stem.to_ascii_lowercase());
            state.apply(Edit::AddBox(CollisionBox {
                name,
                centre: [at[0], at[1], at[2] + size[2] * 0.5],
                size,
                yaw: 0.0,
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
            ["tab", "placeables"] => ui.tab = Tab::Placeables,
            ["tab", "creatures"] => ui.tab = Tab::Creatures,
            ["tab", "props"] => ui.tab = Tab::Props,
            ["tab", "shapes"] => ui.tab = Tab::Shapes,
            ["search", rest @ ..] => ui.asset_search = rest.join(" "),
            ["find", rest @ ..] => ui.outliner_search = rest.join(" "),
            ["snap", on @ ("on" | "off")] => ui.snapping = *on == "on",
            _ => state.say(format!(
                "edit ui {}: takes tool, tab, search, find or snap",
                words.join(" ")
            )),
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
    // The buttons' commands, run as their keys run them.
    if let Some(command) = state.pending_command.take() {
        state::run(state, &[command.as_str()], &cameras);
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
        place(state, &asset, at);
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
        .default_size(280.0)
        .show(root, |ui| {
            ui.heading("Details");
            let Some(target) = state.editor.selected else {
                ui.label("Nothing is selected. Click an object in the view or the outliner.");
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
                    ui.end_row();
                    match target {
                        Target::Prop(at) => {
                            if let Some(prop) = state.editor.project.kjk.props.get(at).cloned() {
                                let mut scale = prop.scale;
                                ui.label("Scale");
                                let response = ui.add(
                                    egui::DragValue::new(&mut scale)
                                        .range(
                                            level_edit::area::SCALES.0..=level_edit::area::SCALES.1,
                                        )
                                        .speed(0.01),
                                );
                                if response.changed() {
                                    edits.push(Edit::Scale(at, scale));
                                }
                                dragging |= response.dragged();
                                released |= response.drag_stopped() || response.lost_focus();
                                ui.end_row();
                            }
                        }
                        Target::Box(at) => {
                            if let Some(shape) = state.editor.project.kjk.boxes.get(at).cloned() {
                                let mut size = shape.size;
                                ui.label("Size");
                                ui.horizontal(|ui| {
                                    let mut sized = false;
                                    for (axis, label) in ["X", "Y", "Z"].iter().enumerate() {
                                        let response = ui.add(
                                            egui::DragValue::new(&mut size[axis])
                                                .prefix(format!("{label} "))
                                                .range(level_edit::area::SMALLEST_SIDE..=100.0)
                                                .speed(0.01)
                                                .suffix(" m"),
                                        );
                                        sized |= response.changed();
                                        dragging |= response.dragged();
                                        released |=
                                            response.drag_stopped() || response.lost_focus();
                                    }
                                    if sized {
                                        edits.push(Edit::Resize(at, size));
                                    }
                                });
                                ui.end_row();
                            }
                        }
                        Target::Object(..) => {}
                    }
                });
            });
            for edit in edits {
                drag_edit(state, ui_state, edit);
            }
            if released
                || (!dragging && !ui_state.gizmo.is_focused() && !ctx.egui_is_using_pointer())
            {
                ui_state.dragging = false;
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
                if ui.button("Frame").on_hover_text("F").clicked() {
                    frame_selection(state, camera_request, cameras);
                }
            });
            messages(ui, state);
        });
}

fn messages(ui: &mut egui::Ui, state: &EditorState) {
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
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (Tab::Placeables, "Placeables"),
                    (Tab::Creatures, "Creatures"),
                    (Tab::Props, "Jedi Academy props"),
                    (Tab::Shapes, "Collision shapes"),
                ] {
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
                Tab::Shapes => SHAPES
                    .iter()
                    .enumerate()
                    .map(|(at, (name, size))| {
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
            let width = ui.available_width();
            let per_row = ((width / (TILE + 12.0)).floor() as usize).max(1);
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
                place(state, &asset, at);
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
                place(state, &asset, at);
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
