//! The level editor (`kjk edit`, `docs/LEVEL-EDIT.md`). It draws a KOTOR
//! area as `kjk module` draws it, with the free camera. Every object the
//! area places gets a coloured marker and a label, and a panel shows the
//! selected object and the keys. The project's data and its edits live in
//! `level_edit`. This plugin only draws them and turns keys, clicks and
//! `edit` console commands into edits.
//!
//! The scene is drawn again from the project after edits (`redraw`): the
//! area's resolver with the edited GIT over it, the module's scene made
//! from it as `kjk module` makes it, then the Jedi Academy props, the
//! collision boxes and the markers added. While an object is being moved,
//! its marker and its model move at once, and the redraw waits until the
//! edits stop for a moment.

use std::collections::BTreeMap;
use std::path::PathBuf;

use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr};
use asset_odyssey::module::ModuleIr;
use asset_odyssey::texture::{Encoding, TextureFacts, TextureIr};
use bevy::input::keyboard::KeyCode;
use bevy::prelude::*;
use console::{ConsoleQueue, DumpFacts};
use fmt_odyssey::ResType;
use level_edit::editor::{ADDABLE, EDITABLE};
use level_edit::{CollisionBox, Edit, Editor, Entry, Kind, Prop, Target};
use vfs::Game;
use vfs::resolver::{Layer, Location, Rank, Resolver};

use crate::project_props::{self, PropModel};
use crate::scene::{FlyCamera, SceneContent, SceneModel, game_to_bevy};
use crate::viewer::{LoadedScene, Placed, ResolverSource};

/// How many frames without an edit before the scene is drawn again.
const SETTLE_FRAMES: u32 = 12;
/// How far one press of an arrow key moves the selection, metres.
const STEP_M: f32 = 0.25;
/// How far one press of `[` or `]` turns it, degrees.
const TURN_DEG: f32 = 15.0;
/// How many labels are shown at once: those of the nearest objects.
const LABELS: usize = 24;
/// How far a marker sits above its object's origin, metres.
const MARKER_RISE: f32 = 0.35;
/// A marker's radius, metres; also how near a click must pass to pick it.
const MARKER_R: f32 = 0.22;
/// The size a new collision box starts at, metres: a ledge.
const NEW_BOX: [f32; 3] = [2.0, 1.0, 0.3];
/// How far ahead of the camera something is placed when nothing is under
/// the screen's centre, metres.
const PLACE_AHEAD_M: f32 = 4.0;

/// Everything the editor starts from, made by `kjk edit` before the window
/// opens.
pub struct EditSetup {
    /// The project as its folder holds it, or as it was just made.
    pub project: level_edit::Project,
    pub game: Game,
    /// The install's resolver with the module's archives and the project's
    /// folder (`vfs::project`) in it.
    pub resolver: Resolver,
    /// Jedi Academy's archives, for props; `None` without the game.
    pub ja: Option<vfs::ja::JaStack>,
    /// The install's placeable and creature templates, and Jedi Academy's
    /// map objects.
    pub placeables: Vec<String>,
    pub creatures: Vec<String>,
    pub models: Vec<String>,
    /// The kjk that `F5` and `edit audit` start (this program).
    pub program: PathBuf,
}

/// Which list the browser shows, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Browse {
    Off,
    Placeables,
    Creatures,
    Props,
}

/// What a scene's drawing placed for each thing the project holds.
#[derive(Default)]
struct Drawn {
    /// Each target's own model roots (not attached ones) and its marker.
    things: Vec<(Target, Vec<usize>, Option<usize>)>,
    /// The collision overlay's model, when drawn.
    overlay: Option<usize>,
}

/// The editor's state.
#[derive(Resource)]
pub struct EditorState {
    setup: EditSetup,
    /// The edits are made here.
    pub editor: Editor,
    drawn: Drawn,
    built: u64,
    /// Frames since the last edit.
    quiet: u32,
    prop_models: BTreeMap<String, Result<PropModel, String>>,
    collision: Option<(u64, collision_odyssey::ModuleCollision)>,
    show_collision: bool,
    shown_collision: bool,
    browse: Browse,
    cursor: usize,
    /// The selection follows the point under the pointer (`G`).
    grabbing: bool,
    /// A click `edit pick` asks for, at a point of the view (pixels).
    pending_pick: Option<Vec2>,
    messages: Vec<String>,
    audit: Vec<String>,
    redraws: u32,
}

/// Turns the editor on: `setup`'s scene is drawn, and the keys, clicks and
/// `edit` commands edit it.
pub struct EditorPlugin;

/// The setup, handed to the app once (`runtime::Scene::Edit`).
#[derive(Resource)]
pub struct PendingSetup(pub std::sync::Mutex<Option<EditSetup>>);

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<console::EditRequests>()
            .add_systems(PreStartup, start)
            .add_systems(Startup, spawn_panel)
            // Before `Update`, where the scene spawns what `SceneContent`
            // holds: in `Update` a new scene could be spawned a frame after
            // the systems that bind to its models saw it change.
            .add_systems(PreUpdate, redraw)
            .add_systems(
                Update,
                (
                    commands_from_console.after(ConsoleQueue),
                    keys,
                    click,
                    grab,
                    follow,
                    panel_camera,
                    panel,
                    labels,
                    publish,
                )
                    .chain(),
            );
    }
}

fn start(world: &mut World) {
    let Some(setup) = world
        .get_resource::<PendingSetup>()
        .and_then(|pending| pending.0.lock().ok()?.take())
    else {
        return;
    };
    let editor = Editor::new(setup.project.clone());
    let mut state = EditorState {
        setup,
        editor,
        drawn: Drawn::default(),
        built: u64::MAX,
        quiet: SETTLE_FRAMES,
        prop_models: BTreeMap::new(),
        collision: None,
        show_collision: false,
        shown_collision: false,
        browse: Browse::Off,
        cursor: 0,
        grabbing: false,
        pending_pick: None,
        messages: Vec::new(),
        audit: Vec::new(),
        redraws: 0,
    };
    match draw(&mut state) {
        Ok(scene) => {
            world.insert_resource(SceneContent(Box::new(scene)));
        }
        Err(why) => state.say(format!("the area does not draw: {why}")),
    }
    world.insert_resource(state);
}

impl EditorState {
    fn say(&mut self, message: impl Into<String>) {
        let message = message.into();
        info!(target: "editor", "{message}");
        self.messages.push(message);
        let over = self.messages.len().saturating_sub(8);
        self.messages.drain(..over);
    }

    fn apply(&mut self, edit: Edit) {
        if let Err(why) = self.editor.apply(edit) {
            self.say(why);
        }
        self.quiet = 0;
    }

    fn area_index(ir: &ModuleIr, area: &str) -> Option<usize> {
        ir.areas
            .iter()
            .position(|candidate| candidate.resref.eq_ignore_ascii_case(area))
    }

    /// The resolver with the edited GIT over the project's folder.
    fn resolver(&self) -> Resolver {
        let git = self.editor.project.git.write();
        let mut resources = BTreeMap::new();
        let size = git.len() as u64;
        resources.insert(
            (self.editor.project.area.clone(), ResType::GIT),
            Location {
                path: PathBuf::from("editor"),
                offset: 0,
                size,
                held: Some(git.into()),
            },
        );
        self.setup.resolver.with_layer(Layer {
            name: "editor".into(),
            rank: Rank::Directory,
            resources,
        })
    }

    /// The edited area's collision, with the project's boxes and solid
    /// props, built again when the project changed since.
    fn collision(&mut self) -> Option<&collision_odyssey::ModuleCollision> {
        let changes = self.editor.changes();
        if self.collision.as_ref().is_none_or(|(at, _)| *at != changes) {
            let resolver = self.resolver();
            let (added, _) =
                level_edit::active::collision_of(&self.editor.project, self.setup.ja.as_ref());
            let game = self.setup.game.key();
            let module = self.editor.project.module.clone();
            let clip = match collision_odyssey::built_in_clip(game, &module) {
                Ok(clip) => clip,
                Err(why) => {
                    self.say(why);
                    None
                }
            };
            let clip = level_edit::active::with_added(clip, &added);
            match collision_odyssey::build_module(
                &ResolverSource(&resolver),
                &module,
                Some(&clip),
                &collision_odyssey::BuildOptions::default(),
            ) {
                Ok(mut built) => {
                    built.set_doors(collision_odyssey::Doors::Openable);
                    self.collision = Some((changes, built));
                }
                Err(why) => {
                    self.say(format!("the collision does not build: {why}"));
                    return None;
                }
            }
        }
        self.collision.as_ref().map(|(_, built)| built)
    }

    /// The tag an entry goes by: its own, else its template's.
    fn tag_of(&self, entry: &Entry) -> String {
        if !entry.tag.is_empty() {
            return entry.tag.clone();
        }
        let (_, restype) = entry.kind.list_and_type();
        self.setup
            .resolver
            .read(&entry.template, restype)
            .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
            .and_then(|gff| {
                gff.root
                    .bytes("Tag")
                    .map(|tag| String::from_utf8_lossy(tag).into_owned())
            })
            .unwrap_or_else(String::new)
    }

    fn list(&self) -> &[String] {
        match self.browse {
            Browse::Off => &[],
            Browse::Placeables => &self.setup.placeables,
            Browse::Creatures => &self.setup.creatures,
            Browse::Props => &self.setup.models,
        }
    }
}

/// A flat colour as a 4 by 4 texture.
fn colour_texture(rgba: [u8; 4]) -> TextureIr {
    TextureIr {
        width: 4,
        height: 4,
        encoding: Encoding::Rgba8,
        mips: vec![rgba.repeat(16)],
        cube: false,
        frames: 1,
        facts: TextureFacts {
            alpha: rgba[3] < 255,
            two_sided: true,
            ..TextureFacts::default()
        },
        faces: Vec::new(),
        frame_mips: Vec::new(),
    }
}

/// A model of one node and one mesh of `triangles` in its own space,
/// coloured by `texture`.
fn flat_model(name: &str, triangles: &[[[f32; 3]; 3]], texture: &str, glow: bool) -> ModelIr {
    let mut positions = Vec::with_capacity(triangles.len() * 3);
    let mut normals = Vec::with_capacity(triangles.len() * 3);
    for [a, b, c] in triangles {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let n = Vec3::from_array([
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ])
        .normalize_or(Vec3::Z)
        .to_array();
        positions.extend([*a, *b, *c]);
        normals.extend([n; 3]);
    }
    let count = positions.len();
    ModelIr {
        name: name.to_string(),
        supermodel: None,
        nodes: vec![NodeIr {
            name: name.to_string(),
            parent: None,
            translation: [0.0; 3],
            rotation: [0.0, 0.0, 0.0, 1.0],
            world: IDENTITY,
            animated: false,
        }],
        meshes: vec![MeshIr {
            node: 0,
            positions,
            normals,
            uv1: vec![[0.5, 0.5]; count],
            uv2: Vec::new(),
            indices: (0..count as u32).collect(),
            texture: Some(texture.to_string()),
            lightmap: None,
            transparency_hint: 0,
            skinned: false,
            shadow: false,
            self_illum: if glow { [1.0; 3] } else { [0.0; 3] },
            alpha: 1.0,
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            uv_scroll: None,
            skin: None,
            dangly: None,
        }],
        lights: Vec::new(),
        emitters: Vec::new(),
        animations: Vec::new(),
        animation_scale: 1.0,
        affected_by_fog: false,
        refusals: Vec::new(),
        folded: Vec::new(),
    }
}

/// An octahedron of radius `r` about the origin: a marker.
fn diamond(r: f32) -> Vec<[[f32; 3]; 3]> {
    let points = [
        [r, 0.0, 0.0],
        [-r, 0.0, 0.0],
        [0.0, r, 0.0],
        [0.0, -r, 0.0],
        [0.0, 0.0, r * 1.6],
        [0.0, 0.0, -r * 1.6],
    ];
    let mut out = Vec::new();
    for (a, b) in [(0, 2), (2, 1), (1, 3), (3, 0)] {
        out.push([points[a], points[b], points[4]]);
        out.push([points[b], points[a], points[5]]);
    }
    out
}

/// The marker colour of each kind.
fn kind_colour(kind: Kind) -> (&'static str, [u8; 4]) {
    match kind {
        Kind::Creature => ("kjk-editor-creature", [255, 80, 80, 255]),
        Kind::Door => ("kjk-editor-door", [170, 120, 255, 255]),
        Kind::Placeable => ("kjk-editor-placeable", [255, 190, 60, 255]),
        Kind::Trigger => ("kjk-editor-trigger", [80, 220, 255, 255]),
        Kind::Waypoint => ("kjk-editor-waypoint", [90, 255, 120, 255]),
        Kind::Sound => ("kjk-editor-sound", [255, 120, 220, 255]),
        Kind::Encounter => ("kjk-editor-encounter", [255, 255, 255, 255]),
        Kind::Store => ("kjk-editor-store", [255, 255, 120, 255]),
    }
}

const PROP_MARKER: (&str, [u8; 4]) = ("kjk-editor-prop", [120, 200, 255, 255]);
const BOX_FILL: (&str, [u8; 4]) = ("kjk-editor-box", [60, 230, 255, 110]);
const WALK_FILL: (&str, [u8; 4]) = ("kjk-editor-walkable", [60, 255, 90, 90]);
const SOLID_FILL: (&str, [u8; 4]) = ("kjk-editor-solid", [255, 130, 60, 60]);
const WALL_FILL: (&str, [u8; 4]) = ("kjk-editor-wall", [90, 120, 255, 80]);

/// The scene of the project as it is now.
fn draw(state: &mut EditorState) -> Result<LoadedScene, String> {
    let started = std::time::Instant::now();
    let resolver = state.resolver();
    let module = state.editor.project.module.clone();
    let ir = ModuleIr::build(&module, &ResolverSource(&resolver))
        .map_err(|refusal| format!("{module}: {refusal:?}"))?;
    let area = EditorState::area_index(&ir, &state.editor.project.area).ok_or(format!(
        "{module} has no area {}",
        state.editor.project.area
    ))?;
    let mut scene = crate::viewer::module_scene(&resolver, state.setup.game, &ir, area)?;
    let mut drawn = Drawn::default();

    // The objects' models: a door's or placeable's by its object number,
    // a creature's body. The module reader leaves out a creature it cannot
    // make, so entries are matched to objects in order, kind by kind, by
    // template.
    let instances = &ir.areas[area].instances;
    let entries = state.editor.entries();
    let mut models_of: Vec<Vec<usize>> = vec![Vec::new(); entries.len()];
    let mut next = 0usize;
    for (object, instance) in instances.iter().enumerate() {
        let found = (next..entries.len()).find(|&at| {
            entries[at].kind == instance.kind
                && entries[at]
                    .template
                    .eq_ignore_ascii_case(&instance.template)
        });
        let Some(at) = found else { continue };
        next = at + 1;
        if let Some(model) = scene.instance_models.get(&(object as u32)) {
            models_of[at].push(*model);
        }
        for view in scene
            .creature_views
            .iter()
            .filter(|view| view.object == object as u32)
        {
            models_of[at].push(view.body);
        }
    }

    // Props.
    let files = state.setup.ja.as_ref().map(crate::viewer_ja::Files::base);
    let props = state.editor.project.kjk.props.clone();
    let (prop_models, notes) =
        project_props::add(&mut scene, files.as_ref(), &mut state.prop_models, &props);
    drop(files);
    for note in notes {
        state.say(note);
    }

    // Markers and boxes.
    let mut textures: Vec<(&str, [u8; 4])> = vec![PROP_MARKER, BOX_FILL];
    textures.extend(Kind::ALL.map(kind_colour));
    for (entry, models) in entries.iter().zip(models_of) {
        let (texture, _) = kind_colour(entry.kind);
        let marker = scene.models.len();
        scene.models.push(Placed::at(
            flat_model("marker", &diamond(MARKER_R), texture, true),
            entry.position,
        ));
        drawn.things.push((
            Target::Object(entry.kind, entry.index),
            models,
            Some(marker),
        ));
    }
    for (at, prop) in props.iter().enumerate() {
        let marker = scene.models.len();
        scene.models.push(Placed::at(
            flat_model("marker", &diamond(MARKER_R), PROP_MARKER.0, true),
            prop.position,
        ));
        drawn.things.push((
            Target::Prop(at),
            prop_models[at].into_iter().collect(),
            Some(marker),
        ));
    }
    for (at, shape) in state.editor.project.kjk.boxes.iter().enumerate() {
        let local = CollisionBox {
            centre: [0.0; 3],
            yaw: 0.0,
            ..shape.clone()
        };
        let model = scene.models.len();
        scene.models.push(Placed {
            facing: shape.yaw.to_radians(),
            ..Placed::at(
                flat_model("box", &local.triangles(), BOX_FILL.0, true),
                shape.centre,
            )
        });
        drawn.things.push((Target::Box(at), vec![model], None));
    }

    // The collision overlay.
    if state.show_collision {
        textures.extend([WALK_FILL, SOLID_FILL, WALL_FILL]);
        if let Some(built) = state.collision() {
            let mut walk = Vec::new();
            let mut solid = Vec::new();
            let mut walls = Vec::new();
            let lift = |t: &[[f32; 3]; 3]| t.map(|[x, y, z]| [x, y, z + 0.02]);
            for triangle in &built.collision.triangles {
                match triangle.origin {
                    collision_odyssey::Origin::Walkmesh
                        if built.surfaces.walkable(triangle.material) =>
                    {
                        walk.push(lift(&triangle.corners));
                    }
                    collision_odyssey::Origin::EdgeWall => walls.push(triangle.corners),
                    _ => solid.push(triangle.corners),
                }
            }
            let mut overlay = flat_model("collision", &walk, WALK_FILL.0, true);
            for (triangles, texture) in [(solid, SOLID_FILL.0), (walls, WALL_FILL.0)] {
                let mut part = flat_model("collision", &triangles, texture, true);
                overlay.meshes.append(&mut part.meshes);
            }
            drawn.overlay = Some(scene.models.len());
            scene.models.push(Placed::at(overlay, [0.0; 3]));
        }
    }
    for (name, rgba) in textures {
        scene
            .textures
            .insert(name.to_string(), colour_texture(rgba));
    }
    state.drawn = drawn;
    state.built = state.editor.changes();
    state.shown_collision = state.show_collision;
    state.redraws += 1;
    info!(
        target: "editor",
        "drew {module} in {:.0} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
    Ok(scene)
}

/// The scene drawn again once the edits have stopped for a moment.
fn redraw(mut commands: Commands, mut state: ResMut<EditorState>) {
    state.quiet = state.quiet.saturating_add(1);
    let stale =
        state.built != state.editor.changes() || state.shown_collision != state.show_collision;
    if !stale || state.quiet < SETTLE_FRAMES || state.grabbing {
        return;
    }
    match draw(&mut state) {
        Ok(scene) => commands.insert_resource(SceneContent(Box::new(scene))),
        Err(why) => {
            state.say(format!("the area does not draw: {why}"));
            state.built = state.editor.changes();
        }
    }
}

/// Each drawn thing's models and marker moved to where the project has it
/// now: an edit shows before the scene is drawn again.
fn follow(
    state: Res<EditorState>,
    scene: Option<Res<SceneContent>>,
    mut roots: Query<(&SceneModel, &mut Transform)>,
) {
    let Some(scene) = scene else { return };
    let mut wanted: BTreeMap<usize, Mat4> = BTreeMap::new();
    for (target, models, marker) in &state.drawn.things {
        let Some((position, facing)) = state.editor.placement(*target) else {
            continue;
        };
        for &model in models {
            let Some(placed) = scene.0.models.get(model) else {
                continue;
            };
            if placed.hook.is_some() {
                continue;
            }
            wanted.insert(model, root(placed, position, facing));
        }
        if let Some(marker) = *marker {
            let rise = match target {
                Target::Object(Kind::Creature, _) => 2.0,
                _ => MARKER_RISE,
            };
            let grown = if state.editor.selected == Some(*target) {
                1.8
            } else {
                1.0
            };
            wanted.insert(
                marker,
                Mat4::from_scale_rotation_translation(
                    Vec3::splat(grown),
                    Quat::IDENTITY,
                    Vec3::from_array(position) + Vec3::Z * rise,
                ),
            );
        }
    }
    for (model, mut transform) in &mut roots {
        if let Some(matrix) = wanted.get(&model.index) {
            let next = Transform::from_matrix(*matrix);
            if *transform != next {
                *transform = next;
            }
        }
    }
}

/// Where a placed model's root sits when its object stands at `position`
/// facing `facing` degrees: as `scene::placement` places it.
fn root(placed: &Placed, position: [f32; 3], facing: f32) -> Mat4 {
    let frame = Mat4::from_rotation_translation(
        Quat::from_rotation_z(facing.to_radians()),
        Vec3::from_array(position),
    );
    match &placed.attach {
        Some(attach) => frame * crate::scene::to_mat4(attach),
        None => frame,
    }
}

/// A ray from the camera through a point of its view (pixels), in game
/// space.
fn ray_at(
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    point: Vec2,
) -> Option<(Vec3, Vec3)> {
    let (camera, transform) = cameras.iter().next()?;
    let ray = camera.viewport_to_world(transform, point).ok()?;
    let to_game = game_to_bevy().inverse();
    Some((to_game * ray.origin, to_game * Vec3::from(ray.direction)))
}

/// The centre of the camera's view, pixels.
fn view_centre(cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>) -> Option<Vec2> {
    let (camera, _) = cameras.iter().next()?;
    camera.logical_viewport_size().map(|size| size / 2.0)
}

/// Where a ray first meets the level, if within 200 m.
fn ground(state: &mut EditorState, (origin, direction): (Vec3, Vec3)) -> Option<[f32; 3]> {
    use collision::CollisionWorld;
    let end = origin + direction.normalize_or_zero() * 200.0;
    let built = state.collision()?;
    let trace = built.collision.level.trace(
        origin.to_array(),
        end.to_array(),
        &collision::Extents::POINT,
        collision::Contents::PLAYER_SOLID,
    );
    (trace.fraction < 1.0 && !trace.start_solid).then_some(trace.end)
}

/// Where a new thing goes: the level under the screen's centre, else a
/// few metres ahead of the camera.
fn place_point(
    state: &mut EditorState,
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> [f32; 3] {
    let Some(ray) = view_centre(cameras).and_then(|centre| ray_at(cameras, centre)) else {
        return [0.0; 3];
    };
    ground(state, ray)
        .unwrap_or_else(|| (ray.0 + ray.1.normalize_or_zero() * PLACE_AHEAD_M).to_array())
}

/// The nearest drawn thing a ray passes through: its marker, or its model's
/// bounds.
fn pick(
    state: &EditorState,
    scene: &LoadedScene,
    (origin, direction): (Vec3, Vec3),
) -> Option<Target> {
    let direction = direction.normalize_or_zero();
    let mut best: Option<(f32, Target)> = None;
    let mut offer = |distance: f32, target: Target| {
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((distance, target));
        }
    };
    for (target, models, marker) in &state.drawn.things {
        let Some((position, facing)) = state.editor.placement(*target) else {
            continue;
        };
        if marker.is_some() {
            let rise = match target {
                Target::Object(Kind::Creature, _) => 2.0,
                _ => MARKER_RISE,
            };
            let centre = Vec3::from_array(position) + Vec3::Z * rise;
            let along = (centre - origin).dot(direction);
            let off = (origin + direction * along - centre).length();
            if along > 0.0 && off < MARKER_R * 1.5 {
                offer(along, *target);
            }
        }
        for &model in models {
            let Some((low, high)) = scene.models.get(model).and_then(crate::scene::local_bounds)
            else {
                continue;
            };
            // The ray in the model's own space.
            let frame = Mat4::from_rotation_translation(
                Quat::from_rotation_z(facing.to_radians()),
                Vec3::from_array(position),
            )
            .inverse();
            let o = frame.transform_point3(origin);
            let d = frame.transform_vector3(direction);
            let mut near = 0.0f32;
            let mut far = f32::INFINITY;
            let mut hit = true;
            for axis in 0..3 {
                if d[axis].abs() < 1e-6 {
                    if o[axis] < low[axis] || o[axis] > high[axis] {
                        hit = false;
                    }
                    continue;
                }
                let a = (low[axis] - o[axis]) / d[axis];
                let b = (high[axis] - o[axis]) / d[axis];
                near = near.max(a.min(b));
                far = far.min(a.max(b));
            }
            if hit && near <= far && far > 0.0 {
                offer(near, *target);
            }
        }
    }
    best.map(|(_, target)| target)
}

/// A left click selects what is under the pointer, or puts the grabbed
/// selection down.
fn click(
    mut state: ResMut<EditorState>,
    scene: Option<Res<SceneContent>>,
    buttons: Option<Res<ButtonInput<MouseButton>>>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let Some(scene) = scene else { return };
    let state = &mut *state;
    if let Some(point) = state.pending_pick.take() {
        match ray_at(&cameras, point) {
            Some(ray) => {
                state.editor.selected = pick(state, &scene.0, ray);
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
        return;
    }
    let Some(buttons) = buttons else { return };
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    if state.grabbing {
        state.grabbing = false;
        return;
    }
    let Some(cursor) = windows.iter().next().and_then(Window::cursor_position) else {
        return;
    };
    let Some(ray) = ray_at(&cameras, cursor) else {
        return;
    };
    state.editor.selected = pick(state, &scene.0, ray);
}

/// While grabbing, the selection stands where the pointer meets the level.
fn grab(
    mut state: ResMut<EditorState>,
    windows: Query<&Window>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let state = &mut *state;
    if !state.grabbing {
        return;
    }
    let Some(target) = state.editor.selected else {
        state.grabbing = false;
        return;
    };
    let Some(cursor) = windows.iter().next().and_then(Window::cursor_position) else {
        return;
    };
    let Some(ray) = ray_at(&cameras, cursor) else {
        return;
    };
    // The grabbed thing's own collision is left as it was: it is drawn
    // again when it is put down.
    if let Some(mut point) = ground(state, ray) {
        if let Target::Box(at) = target
            && let Some(shape) = state.editor.project.kjk.boxes.get(at)
        {
            point[2] += shape.size[2] * 0.5;
        }
        if state.editor.placement(target).map(|(p, _)| p) != Some(point) {
            let changes = state.editor.changes();
            state.apply(Edit::Move(target, point));
            // Grabbing moves many times: the collision is not built again
            // for each.
            if let Some((at, _)) = &mut state.collision
                && *at == changes
            {
                *at = state.editor.changes();
            }
        }
    }
}

/// The keys (`docs/LEVEL-EDIT.md` lists them).
fn keys(
    mut state: ResMut<EditorState>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    flies: Query<&Transform, With<FlyCamera>>,
) {
    let Some(keys) = keys else { return };
    let state = &mut *state;
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let pressed = |key| keys.just_pressed(key);
    if ctrl {
        if pressed(KeyCode::KeyZ) {
            run(state, &["undo"], &cameras);
        } else if pressed(KeyCode::KeyY) {
            run(state, &["redo"], &cameras);
        } else if pressed(KeyCode::KeyS) {
            run(state, &["save"], &cameras);
        } else if pressed(KeyCode::KeyD) {
            run(state, &["duplicate"], &cameras);
        }
        return;
    }
    if pressed(KeyCode::Tab) {
        state.browse = match state.browse {
            Browse::Off => Browse::Placeables,
            Browse::Placeables => Browse::Creatures,
            Browse::Creatures => Browse::Props,
            Browse::Props => Browse::Off,
        };
        state.cursor = 0;
        return;
    }
    if pressed(KeyCode::F2) {
        state.show_collision = !state.show_collision;
        state.quiet = SETTLE_FRAMES;
    }
    if pressed(KeyCode::F5) {
        run(state, &["play"], &cameras);
    }
    if pressed(KeyCode::F6) {
        run(state, &["audit"], &cameras);
    }
    if pressed(KeyCode::KeyB) {
        run(state, &["box"], &cameras);
    }
    if pressed(KeyCode::KeyG) && state.editor.selected.is_some() {
        state.grabbing = !state.grabbing;
    }
    if pressed(KeyCode::Escape) {
        state.grabbing = false;
        state.browse = Browse::Off;
    }
    if pressed(KeyCode::Delete) {
        run(state, &["delete"], &cameras);
    }
    if state.browse != Browse::Off {
        let count = state.list().len();
        let step = |state: &mut EditorState, by: isize| {
            if count > 0 {
                state.cursor = (state.cursor as isize + by).rem_euclid(count as isize) as usize;
            }
        };
        if pressed(KeyCode::ArrowDown) {
            step(state, 1);
        } else if pressed(KeyCode::ArrowUp) {
            step(state, -1);
        } else if pressed(KeyCode::PageDown) {
            step(state, 10);
        } else if pressed(KeyCode::PageUp) {
            step(state, -10);
        } else if pressed(KeyCode::Enter)
            && let Some(name) = state.list().get(state.cursor).cloned()
        {
            match state.browse {
                Browse::Placeables => run(state, &["add", "placeable", &name], &cameras),
                Browse::Creatures => run(state, &["add", "creature", &name], &cameras),
                _ => run(state, &["prop", &name], &cameras),
            }
        }
        return;
    }
    let Some(target) = state.editor.selected else {
        return;
    };
    let Some((position, facing)) = state.editor.placement(target) else {
        return;
    };
    let yaw = flies.iter().next().map_or(0.0, |transform| {
        let forward = game_to_bevy().inverse() * transform.forward().as_vec3();
        forward.y.atan2(forward.x)
    });
    let step = if shift { STEP_M * 4.0 } else { STEP_M };
    let forward = Vec3::new(yaw.cos(), yaw.sin(), 0.0) * step;
    let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.0) * step;
    let mut moved = Vec3::ZERO;
    for (key, by) in [
        (KeyCode::ArrowUp, forward),
        (KeyCode::ArrowDown, -forward),
        (KeyCode::ArrowRight, right),
        (KeyCode::ArrowLeft, -right),
        (KeyCode::PageUp, Vec3::Z * step),
        (KeyCode::PageDown, -Vec3::Z * step),
    ] {
        if pressed(key) {
            moved += by;
        }
    }
    if moved != Vec3::ZERO {
        let to = (Vec3::from_array(position) + moved).to_array();
        state.apply(Edit::Move(target, to));
    }
    if pressed(KeyCode::BracketLeft) {
        state.apply(Edit::Turn(target, facing + TURN_DEG));
    } else if pressed(KeyCode::BracketRight) {
        state.apply(Edit::Turn(target, facing - TURN_DEG));
    }
    match target {
        Target::Prop(at) => {
            let scale = state
                .editor
                .project
                .kjk
                .props
                .get(at)
                .map_or(1.0, |p| p.scale);
            if pressed(KeyCode::Equal) {
                state.apply(Edit::Scale(at, scale * 1.1));
            } else if pressed(KeyCode::Minus) {
                state.apply(Edit::Scale(at, scale / 1.1));
            }
        }
        Target::Box(at) => {
            let Some(size) = state.editor.project.kjk.boxes.get(at).map(|b| b.size) else {
                return;
            };
            for (key, axis, by) in [
                (KeyCode::Digit1, 0, -STEP_M),
                (KeyCode::Digit2, 0, STEP_M),
                (KeyCode::Digit3, 1, -STEP_M),
                (KeyCode::Digit4, 1, STEP_M),
                (KeyCode::Digit5, 2, -STEP_M),
                (KeyCode::Digit6, 2, STEP_M),
            ] {
                if pressed(key) {
                    let mut size = size;
                    size[axis] = (size[axis] + by).max(level_edit::area::SMALLEST_SIDE);
                    state.apply(Edit::Resize(at, size));
                }
            }
        }
        Target::Object(..) => {}
    }
}

fn number(word: &str) -> Result<f32, String> {
    word.parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or(format!("{word:?} is not a number"))
}

fn point(words: &[&str]) -> Result<[f32; 3], String> {
    match words {
        [x, y, z] => Ok([number(x)?, number(y)?, number(z)?]),
        _ => Err("a point is three numbers: x y z".into()),
    }
}

fn kind_named(word: &str) -> Option<Kind> {
    match word {
        "placeable" => Some(Kind::Placeable),
        "creature" => Some(Kind::Creature),
        _ => None,
    }
}

/// Runs one `edit` command (`docs/LEVEL-EDIT.md`). Keys run the same
/// commands.
fn run(
    state: &mut EditorState,
    words: &[&str],
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    if let Err(why) = command(state, words, cameras) {
        state.say(format!("edit {}: {why}", words.join(" ")));
    }
}

fn selected(state: &EditorState) -> Result<Target, String> {
    state.editor.selected.ok_or("nothing is selected".into())
}

fn command(
    state: &mut EditorState,
    words: &[&str],
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Result<(), String> {
    match words {
        ["select", "prop", n] => {
            let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
            if at >= state.editor.project.kjk.props.len() {
                return Err(format!("there is no prop {at}"));
            }
            state.editor.selected = Some(Target::Prop(at));
        }
        ["select", "box", n] => {
            let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
            if at >= state.editor.project.kjk.boxes.len() {
                return Err(format!("there is no box {at}"));
            }
            state.editor.selected = Some(Target::Box(at));
        }
        ["select", "none"] | ["deselect"] => state.editor.selected = None,
        ["select", tag] => {
            let tags: Vec<(Entry, String)> = state
                .editor
                .entries()
                .into_iter()
                .map(|entry| {
                    let tag = state.tag_of(&entry);
                    (entry, tag)
                })
                .collect();
            let found = tags
                .iter()
                .find(|(_, own)| own.eq_ignore_ascii_case(tag))
                .ok_or(format!("no object is tagged {tag}"))?;
            state.editor.selected = Some(Target::Object(found.0.kind, found.0.index));
        }
        ["move", rest @ ..] => {
            let target = selected(state)?;
            state.apply(Edit::Move(target, point(rest)?));
        }
        ["nudge", rest @ ..] => {
            let target = selected(state)?;
            let by = point(rest)?;
            let (at, _) = state
                .editor
                .placement(target)
                .ok_or("the selection is gone")?;
            state.apply(Edit::Move(target, [0, 1, 2].map(|i| at[i] + by[i])));
        }
        ["turn", degrees] => {
            let target = selected(state)?;
            state.apply(Edit::Turn(target, number(degrees)?));
        }
        ["scale", scale] => match selected(state)? {
            Target::Prop(at) => state.apply(Edit::Scale(at, number(scale)?)),
            _ => return Err("only a prop has a scale".into()),
        },
        ["size", rest @ ..] => match selected(state)? {
            Target::Box(at) => state.apply(Edit::Resize(at, point(rest)?)),
            _ => return Err("only a box has a size".into()),
        },
        ["duplicate"] => {
            let target = selected(state)?;
            state.apply(Edit::Duplicate(target));
        }
        ["delete"] => {
            let target = selected(state)?;
            state.apply(Edit::Delete(target));
        }
        ["undo"] => {
            if !state.editor.undo() {
                return Err("nothing to undo".into());
            }
            state.quiet = 0;
        }
        ["redo"] => {
            if !state.editor.redo() {
                return Err("nothing to redo".into());
            }
            state.quiet = 0;
        }
        ["add", kind, template, rest @ ..] => {
            let kind = kind_named(kind).ok_or("add takes placeable or creature")?;
            let list = if kind == Kind::Placeable {
                &state.setup.placeables
            } else {
                &state.setup.creatures
            };
            let template = template.to_ascii_lowercase();
            if !list.contains(&template) {
                return Err(format!("the install has no {kind:?} template {template}"));
            }
            debug_assert!(ADDABLE.contains(&kind));
            let position = match rest {
                [] => place_point(state, cameras),
                rest => point(rest)?,
            };
            state.apply(Edit::AddObject {
                kind,
                template,
                position,
                facing: 0.0,
            });
        }
        ["prop", model, rest @ ..] => {
            let model = fmt_ja::game_path(model);
            if !state.setup.models.contains(&model) {
                let known = state
                    .setup
                    .ja
                    .as_ref()
                    .is_some_and(|stack| stack.base().contains_key(model.as_str()));
                if !known {
                    return Err(format!("Jedi Academy's archives have no {model}"));
                }
            }
            let (position, scale) = match rest {
                [] => (place_point(state, cameras), 1.0),
                [x, y, z] => (point(&[x, y, z])?, 1.0),
                [x, y, z, scale] => (point(&[x, y, z])?, number(scale)?),
                _ => return Err("prop takes a model, then x y z and a scale".into()),
            };
            let name = state.editor.project.kjk.free_name("prop");
            state.apply(Edit::AddProp(Prop {
                name,
                model,
                position,
                yaw: 0.0,
                scale,
                solid: true,
            }));
        }
        ["box", rest @ ..] => {
            let (centre, size, yaw) = match rest {
                [] => {
                    let mut at = place_point(state, cameras);
                    at[2] += NEW_BOX[2] * 0.5;
                    (at, NEW_BOX, 0.0)
                }
                [cx, cy, cz, sx, sy, sz] => (point(&[cx, cy, cz])?, point(&[sx, sy, sz])?, 0.0),
                [cx, cy, cz, sx, sy, sz, yaw] => {
                    (point(&[cx, cy, cz])?, point(&[sx, sy, sz])?, number(yaw)?)
                }
                _ => return Err("box takes a centre, a size and a yaw".into()),
            };
            let name = state.editor.project.kjk.free_name("box");
            state.apply(Edit::AddBox(CollisionBox {
                name,
                centre,
                size,
                yaw,
            }));
        }
        ["collision", on @ ("on" | "off")] => {
            state.show_collision = *on == "on";
            state.quiet = SETTLE_FRAMES;
        }
        ["save"] => {
            let made_by = format!("kjk {}", env!("KJK_GIT_SHA"));
            let warnings: Vec<String> = state
                .editor
                .project
                .problems()
                .into_iter()
                .filter(|problem| !problem.blocks)
                .map(|problem| problem.what)
                .collect();
            for warning in warnings {
                state.say(warning);
            }
            let written = state.editor.save(&made_by)?;
            state.say(format!(
                "saved {} files to {}",
                written.len(),
                state.editor.project.folder.display()
            ));
        }
        ["play"] => {
            command(state, &["save"], cameras)?;
            let project = state.editor.project.clone();
            let mut play = std::process::Command::new(&state.setup.program);
            play.args([
                "--project",
                &project.name,
                "play",
                "--stage",
                &project.module,
                "--class",
                "jedi",
            ]);
            play.spawn()
                .map_err(|error| format!("kjk play did not start: {error}"))?;
            state.say(format!("kjk play started on {}", project.module));
        }
        ["audit"] => {
            command(state, &["save"], cameras)?;
            let project = state.editor.project.clone();
            let output = std::process::Command::new(&state.setup.program)
                .args([
                    "--project",
                    &project.name,
                    "collision",
                    "audit",
                    &project.game,
                    &project.module,
                ])
                .output()
                .map_err(|error| format!("kjk collision audit did not start: {error}"))?;
            let text = String::from_utf8_lossy(&output.stdout);
            state.audit = text
                .lines()
                .filter(|line| !line.trim().is_empty())
                .map(str::to_string)
                .collect();
            let escapes = state
                .audit
                .iter()
                .filter(|line| line.contains("escape"))
                .count();
            state.say(format!(
                "the containment audit wrote {} lines, {escapes} of them about escapes",
                state.audit.len()
            ));
        }
        _ => return Err("not an edit command (docs/LEVEL-EDIT.md lists them)".into()),
    }
    Ok(())
}

/// `edit` commands from `--cmds`, in order.
fn commands_from_console(
    mut state: ResMut<EditorState>,
    mut requests: ResMut<console::EditRequests>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let state = &mut *state;
    for words in std::mem::take(&mut requests.0) {
        if let ["pick", x, y] = words.iter().map(String::as_str).collect::<Vec<_>>()[..] {
            let (Ok(x), Ok(y)) = (number(x), number(y)) else {
                state.say("edit pick takes a point of the view in pixels");
                continue;
            };
            state.pending_pick = Some(Vec2::new(x, y));
            continue;
        }
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        run(state, &words, &cameras);
    }
}

#[derive(Component)]
struct PanelText;

#[derive(Component)]
struct Label(usize);

/// The panel's and the labels' root: the whole view.
#[derive(Component)]
struct PanelRoot;

fn spawn_panel(mut commands: Commands) {
    commands
        .spawn((
            PanelRoot,
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
        ))
        .with_children(|root| {
            for at in 0..LABELS {
                root.spawn((
                    Label(at),
                    Text::new(""),
                    TextFont::from_font_size(12.0),
                    TextColor(Color::WHITE),
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                    Visibility::Hidden,
                ));
            }
            root.spawn((
                PanelText,
                Text::new(""),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb(0.95, 0.95, 0.85)),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(8),
                    top: px(8),
                    max_width: px(620),
                    padding: UiRect::all(px(8)),
                    ..default()
                },
            ));
        });
}

/// The panel drawn by the scene's camera: a headless run's camera draws
/// into an image, which no camera's UI reaches by default.
fn panel_camera(
    mut commands: Commands,
    roots: Query<(Entity, Option<&UiTargetCamera>), With<PanelRoot>>,
    cameras: Query<Entity, With<FlyCamera>>,
) {
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    for (root, target) in &roots {
        if target.is_none_or(|target| target.entity() != camera) {
            commands.entity(root).insert(UiTargetCamera(camera));
        }
    }
}

fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Creature => "creature",
        Kind::Door => "door",
        Kind::Placeable => "placeable",
        Kind::Trigger => "trigger",
        Kind::Waypoint => "waypoint",
        Kind::Sound => "sound",
        Kind::Encounter => "encounter",
        Kind::Store => "store",
    }
}

/// What the panel and a dump say about a target.
fn describe(state: &EditorState, target: Target) -> serde_json::Value {
    let placement = state.editor.placement(target);
    let (position, facing) = placement.unwrap_or(([0.0; 3], 0.0));
    match target {
        Target::Object(kind, index) => {
            let entry = state.editor.entry(kind, index);
            serde_json::json!({
                "kind": kind_name(kind),
                "index": index,
                "template": entry.as_ref().map(|e| e.template.clone()),
                "tag": entry.as_ref().map(|e| state.tag_of(e)),
                "position": position,
                "facing": facing,
                "editable": EDITABLE.contains(&kind),
            })
        }
        Target::Prop(at) => {
            let prop = state.editor.project.kjk.props.get(at);
            serde_json::json!({
                "kind": "prop",
                "index": at,
                "name": prop.map(|p| p.name.clone()),
                "model": prop.map(|p| p.model.clone()),
                "scale": prop.map(|p| p.scale),
                "solid": prop.map(|p| p.solid),
                "position": position,
                "facing": facing,
            })
        }
        Target::Box(at) => {
            let shape = state.editor.project.kjk.boxes.get(at);
            serde_json::json!({
                "kind": "box",
                "index": at,
                "name": shape.map(|b| b.name.clone()),
                "size": shape.map(|b| b.size),
                "position": position,
                "facing": facing,
            })
        }
    }
}

fn panel(state: Res<EditorState>, mut texts: Query<&mut Text, With<PanelText>>) {
    let project = &state.editor.project;
    let mut lines = vec![format!(
        "kjk level editor: project {} ({} {}){}",
        project.name,
        project.game,
        project.module,
        if state.editor.unsaved() {
            ", not saved"
        } else {
            ""
        }
    )];
    match state.editor.selected {
        Some(target) => {
            let facts = describe(&state, target);
            let position = state
                .editor
                .placement(target)
                .map_or([0.0; 3], |(position, _)| position);
            let facing = state.editor.placement(target).map_or(0.0, |(_, f)| f);
            lines.push(format!(
                "Selected: {} {}  tag {}  at {:.2}, {:.2}, {:.2}  facing {:.0} degrees{}",
                facts["kind"].as_str().unwrap_or("?"),
                facts["template"]
                    .as_str()
                    .or(facts["model"].as_str())
                    .or(facts["name"].as_str())
                    .unwrap_or(""),
                facts["tag"].as_str().unwrap_or("-"),
                position[0],
                position[1],
                position[2],
                facing,
                if state.grabbing { "  (grabbed)" } else { "" }
            ));
        }
        None => lines.push("Nothing is selected. Click an object or its marker.".into()),
    }
    if state.browse != Browse::Off {
        let list = state.list();
        lines.push(format!(
            "{:?}: {} of {} (arrows to pick, Enter to place at the screen's centre, Tab for the next list)",
            state.browse,
            state.cursor + 1,
            list.len()
        ));
        let first = state.cursor.saturating_sub(4);
        for (at, name) in list.iter().enumerate().skip(first).take(9) {
            let mark = if at == state.cursor { ">" } else { " " };
            lines.push(format!("{mark} {name}"));
        }
    } else {
        lines.push(
            "Fly: right mouse, WASD, Q and E.  Move: arrows, PageUp/PageDown, G to grab.  Turn: [ ].  \
             Ctrl+D copy, Delete, Ctrl+Z undo, Ctrl+Y redo, Ctrl+S save.  Tab: templates and props.  \
             B: collision box (1 to 6 size it), - and = scale a prop.  F2 collision, F5 play, F6 audit."
                .into(),
        );
    }
    lines.extend(state.messages.iter().rev().take(4).cloned());
    lines.extend(state.audit.iter().take(6).cloned());
    let text = lines.join("\n");
    for mut shown in &mut texts {
        if shown.0 != text {
            shown.0 = text.clone();
        }
    }
}

/// Labels over the nearest objects in view.
fn labels(
    state: Res<EditorState>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    mut labels: Query<(&Label, &mut Text, &mut Node, &mut Visibility)>,
) {
    let Some((camera, eye)) = cameras.iter().next() else {
        return;
    };
    let eye_game = game_to_bevy().inverse() * eye.translation();
    let mut near: Vec<(f32, String, Vec3)> = Vec::new();
    for entry in state.editor.entries() {
        let at = Vec3::from_array(entry.position);
        let tag = if entry.tag.is_empty() {
            entry.template.clone()
        } else {
            entry.tag.clone()
        };
        near.push((
            at.distance(eye_game),
            format!("{} {tag}", kind_name(entry.kind)),
            at,
        ));
    }
    for prop in &state.editor.project.kjk.props {
        let at = Vec3::from_array(prop.position);
        near.push((at.distance(eye_game), format!("prop {}", prop.name), at));
    }
    for shape in &state.editor.project.kjk.boxes {
        let at = Vec3::from_array(shape.centre);
        near.push((at.distance(eye_game), format!("box {}", shape.name), at));
    }
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let shown: Vec<(String, Vec2)> = near
        .into_iter()
        .filter_map(|(_, text, at)| {
            let world = game_to_bevy() * (at + Vec3::Z * 0.8);
            let point = camera.world_to_viewport(eye, world).ok()?;
            Some((text, point))
        })
        .take(LABELS)
        .collect();
    for (label, mut text, mut node, mut visibility) in &mut labels {
        match shown.get(label.0) {
            Some((value, point)) => {
                if text.0 != *value {
                    text.0 = value.clone();
                }
                node.left = px(point.x);
                node.top = px(point.y);
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

/// The editor's facts for `dump`.
fn publish(state: Res<EditorState>, mut facts: ResMut<DumpFacts>) {
    if !facts.wanted() {
        return;
    }
    let project = &state.editor.project;
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let entries = state.editor.entries();
    for entry in &entries {
        *counts.entry(kind_name(entry.kind)).or_insert(0) += 1;
    }
    let objects: Vec<serde_json::Value> = entries
        .iter()
        .map(|entry| {
            serde_json::json!({
                "kind": kind_name(entry.kind),
                "index": entry.index,
                "template": entry.template,
                "tag": state.tag_of(entry),
                "position": entry.position,
                "facing": entry.facing,
            })
        })
        .collect();
    facts.0.insert(
        "editor".into(),
        serde_json::json!({
            "project": project.name,
            "folder": project.folder,
            "game": project.game,
            "module": project.module,
            "area": project.area,
            "unsaved": state.editor.unsaved(),
            "undo": state.editor.undo_depth(),
            "redo": state.editor.redo_depth(),
            "counts": counts,
            "objects": objects,
            "selected": state.editor.selected.map(|target| describe(&state, target)),
            "props": (0..project.kjk.props.len()).map(|at| describe(&state, Target::Prop(at))).collect::<Vec<_>>(),
            "boxes": (0..project.kjk.boxes.len()).map(|at| describe(&state, Target::Box(at))).collect::<Vec<_>>(),
            "drawn_props": state.drawn.things.iter().filter(|(t, models, _)| matches!(t, Target::Prop(_)) && !models.is_empty()).count(),
            "collision_overlay": state.drawn.overlay.is_some(),
            "templates": { "placeables": state.setup.placeables.len(), "creatures": state.setup.creatures.len(), "ja_models": state.setup.models.len() },
            "messages": state.messages,
            "audit": state.audit,
            "redraws": state.redraws,
            "problems": project.problems().into_iter().map(|p| p.what).collect::<Vec<_>>(),
        }),
    );
}
