//! The editor's state: the project being edited, the scene drawn from it,
//! the collision built from it, and the `edit` commands that change it.
//! The panels, the gizmo and the asset picker (`ui`) turn clicks and keys
//! into these commands and edits.
//!
//! The scene is drawn again from the project after edits (`redraw`): the
//! area's resolver with the edited GIT over it, the module's scene made
//! from it as `kjk module` makes it, then the Jedi Academy props, the
//! collision boxes and the markers added. While an object is being moved,
//! its marker and its model move at once (`follow`), and the redraw waits
//! until the edits stop for a moment.

use std::collections::BTreeMap;
use std::path::PathBuf;

use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr};
use asset_odyssey::module::ModuleIr;
use asset_odyssey::texture::{Encoding, TextureFacts, TextureIr};
use bevy::prelude::*;
use console::DumpFacts;
use fmt_odyssey::ResType;
use level_edit::editor::{ADDABLE, EDITABLE};
use level_edit::{Change, CollisionBox, Edit, Editor, Entry, Kind, Prop, Target};
use vfs::Game;
use vfs::resolver::{Layer, Location, Rank, Resolver};

use kjk_game::project_props::{self, PropModel};
use kjk_game::scene::{FlyCamera, SceneContent, SceneModel, game_to_bevy};
use kjk_game::viewer::{LoadedScene, Placed, ResolverSource};

/// How many frames without an edit before the scene is drawn again.
const SETTLE_FRAMES: u32 = 12;
/// How far one press of an arrow key moves the selection, metres.
pub(crate) const STEP_M: f32 = 0.25;
/// How far one press of `[` or `]` turns it, degrees.
pub(crate) const TURN_DEG: f32 = 15.0;
/// How far a marker sits above its object's origin, metres.
pub(crate) const MARKER_RISE: f32 = 0.35;
/// A marker's radius, metres; also how near a click must pass to pick it.
const MARKER_R: f32 = 0.22;
/// The size a new collision box starts at, metres: a ledge.
const NEW_BOX: [f32; 3] = [2.0, 1.0, 0.3];
/// How far ahead of the camera something is placed when nothing is under
/// the screen's centre, metres.
const PLACE_AHEAD_M: f32 = 4.0;

/// Everything the editor starts from, made before the window opens
/// (`main`).
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
    /// The install's compiled scripts and item templates, for the scripts
    /// and items an object's template names.
    pub scripts: Vec<String>,
    /// The game's routines, from its `nwscript.nss`, for the script viewer.
    pub catalog: Option<script_ncs::Catalog>,
    pub items: Vec<String>,
    /// The install's waypoint, trigger, sound and store templates.
    pub waypoints: Vec<String>,
    pub triggers: Vec<String>,
    pub sounds: Vec<String>,
    pub stores: Vec<String>,
    /// The install's rooms, for a new map (`LibraryRoom`).
    pub rooms: Vec<LibraryRoom>,
    /// The `kjk` that `edit play` and `edit audit` start: the one beside
    /// this program.
    pub program: PathBuf,
}

/// A room of the install that a new map can place: its model, where its
/// own area's layout places it, and that area.
#[derive(Debug, Clone, PartialEq)]
pub struct LibraryRoom {
    pub model: String,
    pub from: [f32; 3],
    pub area: String,
}

/// The files of a room's copy, by the copy's name, the room's model and
/// how far the room moved (as bits).
type MadeRooms = BTreeMap<(String, String, [u32; 3]), Vec<(String, ResType, Vec<u8>)>>;

/// What a scene's drawing placed for each thing the project holds.
#[derive(Default)]
pub(crate) struct Drawn {
    /// Each target's own model roots (not attached ones) and its marker.
    pub(crate) things: Vec<(Target, Vec<usize>, Option<usize>)>,
    /// The collision overlay's model, when drawn.
    pub(crate) overlay: Option<usize>,
}

/// The editor's state.
#[derive(Resource)]
pub struct EditorState {
    pub(crate) setup: EditSetup,
    /// The edits are made here.
    pub editor: Editor,
    pub(crate) drawn: Drawn,
    built: u64,
    /// Frames since the last edit.
    pub(crate) quiet: u32,
    pub(crate) prop_models: BTreeMap<String, Result<PropModel, String>>,
    /// The rooms' copies made so far (`level_edit::map::room_copy`).
    made_rooms: MadeRooms,
    collision: Option<(u64, collision_odyssey::ModuleCollision)>,
    pub(crate) show_collision: bool,
    shown_collision: bool,
    /// A gizmo is being dragged: the scene is not drawn again until it is
    /// let go.
    pub(crate) dragging: bool,
    /// A click `edit pick` asks for, at a point of the view (pixels).
    pub(crate) pending_pick: Option<Vec2>,
    /// A command a toolbar or details button asked for, run after the
    /// panels are laid out.
    pub(crate) pending_command: Option<String>,
    /// `edit ui ...` commands for the window's own state.
    pub(crate) pending_ui: Vec<Vec<String>>,
    pub(crate) messages: Vec<String>,
    pub(crate) audit: Vec<String>,
    redraws: u32,
}

impl EditorState {
    /// The state `setup` starts from, with its scene drawn.
    pub fn new(setup: EditSetup) -> Result<(EditorState, LoadedScene), String> {
        let editor = Editor::new(setup.project.clone());
        let mut state = EditorState {
            setup,
            editor,
            drawn: Drawn::default(),
            built: u64::MAX,
            quiet: SETTLE_FRAMES,
            prop_models: BTreeMap::new(),
            made_rooms: BTreeMap::new(),
            collision: None,
            show_collision: false,
            shown_collision: false,
            dragging: false,
            pending_pick: None,
            pending_command: None,
            pending_ui: Vec::new(),
            messages: Vec::new(),
            audit: Vec::new(),
            redraws: 0,
        };
        let scene = draw(&mut state)?;
        Ok((state, scene))
    }
}

/// Turns the editor on: `setup`'s scene is drawn, and the keys, clicks and
/// `edit` commands edit it.
impl EditorState {
    pub(crate) fn say(&mut self, message: impl Into<String>) {
        let message = message.into();
        info!(target: "editor", "{message}");
        self.messages.push(message);
        let over = self.messages.len().saturating_sub(8);
        self.messages.drain(..over);
    }

    pub(crate) fn apply(&mut self, edit: Edit) {
        if let Err(why) = self.editor.apply(edit) {
            self.say(why);
        }
        self.quiet = 0;
    }

    /// `apply`, joining the last edit's undo step (a drag).
    pub(crate) fn apply_continuing(&mut self, edit: Edit) {
        if let Err(why) = self.editor.apply_continuing(edit) {
            self.say(why);
        }
        self.quiet = 0;
    }

    fn area_index(ir: &ModuleIr, area: &str) -> Option<usize> {
        ir.areas
            .iter()
            .position(|candidate| candidate.resref.eq_ignore_ascii_case(area))
    }

    /// A new map's module as it is now (saved or not): its `module.ifo`,
    /// its area's ARE, layout and visibility, and its rooms' copies, made
    /// from the install's rooms.
    fn map_files(&mut self) -> Vec<(String, ResType, Vec<u8>)> {
        let project = &self.editor.project;
        let Some(map) = &project.map else {
            return Vec::new();
        };
        let (module, area) = (project.module.clone(), project.area.clone());
        let mut files = vec![
            (
                "module".to_string(),
                ResType::IFO,
                map.ifo(&module, &area).write(),
            ),
            (area.clone(), ResType::ARE, project.map_are(map).write()),
            (area.clone(), ResType::LYT, map.layout(&module).write()),
            (area.clone(), ResType::VIS, map.visibility(&module).write()),
        ];
        let rooms = map.rooms.clone();
        let mut problems = Vec::new();
        for (index, room) in rooms.iter().enumerate() {
            let name = level_edit::map::room_name(&module, index);
            let by: [f32; 3] = std::array::from_fn(|axis| room.position[axis] - room.from[axis]);
            let key = (name.clone(), room.model.clone(), by.map(f32::to_bits));
            if !self.made_rooms.contains_key(&key) {
                let resolver = &self.setup.resolver;
                let read = |name: &str, restype: ResType| resolver.read(name, restype);
                match level_edit::map::room_copy(room, &name, &read) {
                    Ok(made) => {
                        self.made_rooms.insert(key.clone(), made);
                    }
                    Err(why) => {
                        problems.push(why);
                        continue;
                    }
                }
            }
            files.extend(self.made_rooms[&key].iter().cloned());
        }
        for problem in problems {
            self.say(problem);
        }
        files
    }

    /// The resolver with the edited GIT, the project's templates and a new
    /// map's module as they are now (saved or not) over the project's
    /// folder.
    fn resolver(&mut self) -> Resolver {
        let held = |bytes: Vec<u8>| Location {
            path: PathBuf::from("editor"),
            offset: 0,
            size: bytes.len() as u64,
            held: Some(bytes.into()),
        };
        let mut resources = BTreeMap::new();
        resources.insert(
            (self.editor.project.area.clone(), ResType::GIT),
            held(self.editor.project.git.write()),
        );
        for (resref, made) in &self.editor.project.templates {
            resources.insert((resref.clone(), made.restype), held(made.gff.write()));
        }
        for (resref, restype, bytes) in self.map_files() {
            resources.insert((resref, restype), held(bytes));
        }
        self.setup.resolver.with_layer(Layer {
            name: "editor".into(),
            rank: Rank::Directory,
            resources,
        })
    }

    /// A template as the object would be made from it: the project's own
    /// (saved or not), else the install's.
    pub(crate) fn template_gff(
        &self,
        resref: &str,
        restype: ResType,
    ) -> Option<fmt_odyssey::gff::Gff> {
        if let Some(made) = self.editor.project.templates.get(resref) {
            return Some(made.gff.clone());
        }
        self.setup
            .resolver
            .read(resref, restype)
            .and_then(|bytes| fmt_odyssey::gff::Gff::parse(&bytes).ok())
    }

    /// What says what an object does: its template for a kind made from
    /// one (the project's copy, else the install's), its own GIT entry for
    /// a kind read whole from it (a door, a waypoint).
    pub(crate) fn behaviour_source(
        &self,
        kind: Kind,
        entry: &Entry,
    ) -> Option<fmt_odyssey::gff::Gff> {
        match level_edit::template::restype(kind) {
            Some(restype) => self.template_gff(&entry.template, restype),
            None => {
                level_edit::git::item(&self.editor.project.git, kind, entry.index).map(|item| {
                    fmt_odyssey::gff::Gff {
                        file_type: *b"GIT ",
                        root: item.clone(),
                    }
                })
            }
        }
    }

    /// Makes `change` in the template of `target` (`Editor::change_template`).
    pub(crate) fn change_template(&mut self, target: Target, change: Change) {
        let resolver = &self.setup.resolver;
        let read = |name: &str, restype: ResType| resolver.read(name, restype);
        let taken = |name: &str| {
            resolver.find(name, ResType::UTP).is_some()
                || resolver.find(name, ResType::UTC).is_some()
        };
        if let Err(why) = self
            .editor
            .change_template(target, &change, (&read, &taken))
        {
            self.say(why);
        }
        self.quiet = 0;
    }

    /// The edited area's collision, with the project's boxes and solid
    /// props, built again when the project changed since.
    pub(crate) fn collision(&mut self) -> Option<&collision_odyssey::ModuleCollision> {
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
    pub(crate) fn tag_of(&self, entry: &Entry) -> String {
        if !entry.tag.is_empty() {
            return entry.tag.clone();
        }
        let (_, restype) = entry.kind.list_and_type();
        self.template_gff(&entry.template, restype)
            .and_then(|gff| {
                gff.root
                    .bytes("Tag")
                    .map(|tag| String::from_utf8_lossy(tag).into_owned())
            })
            .unwrap_or_else(String::new)
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
pub(crate) fn kind_colour(kind: Kind) -> (&'static str, [u8; 4]) {
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
const ENTRY_MARKER: (&str, [u8; 4]) = ("kjk-editor-entry", [255, 230, 60, 255]);
const BOX_FILL: (&str, [u8; 4]) = ("kjk-editor-box", [60, 230, 255, 110]);
const TRIGGER_FILL: (&str, [u8; 4]) = ("kjk-editor-trigger-fill", [80, 220, 255, 70]);
const ENCOUNTER_FILL: (&str, [u8; 4]) = ("kjk-editor-encounter-fill", [255, 255, 255, 60]);
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
    let mut scene = kjk_game::viewer::module_scene(&resolver, state.setup.game, &ir, area)?;
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
    let files = state
        .setup
        .ja
        .as_ref()
        .map(kjk_game::viewer_ja::Files::base);
    let props = state.editor.project.kjk.props.clone();
    let (prop_models, notes) =
        project_props::add(&mut scene, files.as_ref(), &mut state.prop_models, &props);
    drop(files);
    for note in notes {
        state.say(note);
    }

    // Markers and boxes.
    let mut textures: Vec<(&str, [u8; 4])> =
        vec![PROP_MARKER, BOX_FILL, TRIGGER_FILL, ENCOUNTER_FILL];
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
    // A trigger's and an encounter's outline: a see-through floor of its
    // shape, a few centimetres up, moved with the object.
    for (entry_at, entry) in entries.iter().enumerate() {
        if !matches!(entry.kind, Kind::Trigger | Kind::Encounter) {
            continue;
        }
        let outline = level_edit::git::outline(&state.editor.project.git, entry.kind, entry.index);
        if outline.len() < 3 {
            continue;
        }
        let lift = |p: [f32; 3]| [p[0], p[1], p[2] + 0.05];
        let fan: Vec<[[f32; 3]; 3]> = (1..outline.len() - 1)
            .map(|at| [lift(outline[0]), lift(outline[at]), lift(outline[at + 1])])
            .collect();
        let texture = if entry.kind == Kind::Trigger {
            TRIGGER_FILL.0
        } else {
            ENCOUNTER_FILL.0
        };
        let model = scene.models.len();
        scene.models.push(Placed::at(
            flat_model("outline", &fan, texture, true),
            entry.position,
        ));
        if let Some((_, models, _)) = drawn.things.get_mut(entry_at) {
            models.push(model);
        }
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

    // A new map's rooms, in the layout's order, and its entry.
    if let Some(map) = &state.editor.project.map {
        for at in 0..map.rooms.len() {
            let models = scene
                .rooms
                .get(at)
                .and_then(|room| room.model)
                .into_iter()
                .collect();
            drawn.things.push((Target::Room(at), models, None));
        }
        let marker = scene.models.len();
        scene.models.push(Placed::at(
            flat_model("marker", &diamond(MARKER_R * 1.6), ENTRY_MARKER.0, true),
            map.entry,
        ));
        drawn.things.push((Target::Entry, Vec::new(), Some(marker)));
        textures.push(ENTRY_MARKER);
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
pub(crate) fn redraw(mut commands: Commands, mut state: ResMut<EditorState>) {
    state.quiet = state.quiet.saturating_add(1);
    let stale =
        state.built != state.editor.changes() || state.shown_collision != state.show_collision;
    if !stale || state.quiet < SETTLE_FRAMES || state.dragging {
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
pub(crate) fn follow(
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
pub(crate) fn root(placed: &Placed, position: [f32; 3], facing: f32) -> Mat4 {
    let frame = Mat4::from_rotation_translation(
        Quat::from_rotation_z(facing.to_radians()),
        Vec3::from_array(position),
    );
    match &placed.attach {
        Some(attach) => frame * kjk_game::scene::to_mat4(attach),
        None => frame,
    }
}

/// A ray from the camera through a point of its view (pixels), in game
/// space.
pub(crate) fn ray_at(
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    point: Vec2,
) -> Option<(Vec3, Vec3)> {
    let (camera, transform) = cameras.iter().next()?;
    let ray = camera.viewport_to_world(transform, point).ok()?;
    let to_game = game_to_bevy().inverse();
    Some((to_game * ray.origin, to_game * Vec3::from(ray.direction)))
}

/// The centre of the camera's view, pixels.
pub(crate) fn view_centre(
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) -> Option<Vec2> {
    let (camera, _) = cameras.iter().next()?;
    camera.logical_viewport_size().map(|size| size / 2.0)
}

/// Where a ray first meets the level, if within 200 m.
pub(crate) fn ground(
    state: &mut EditorState,
    (origin, direction): (Vec3, Vec3),
) -> Option<[f32; 3]> {
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
pub(crate) fn place_point(
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
/// bounds. A new map's room is picked last, when nothing else is: the room
/// whose model holds `floor`, where the ray meets the level, and of those
/// the smallest.
pub(crate) fn pick(
    state: &EditorState,
    scene: &LoadedScene,
    (origin, direction): (Vec3, Vec3),
    floor: Option<[f32; 3]>,
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
        if let Target::Room(_) = target {
            continue;
        }
        for &model in models {
            let Some((low, high)) = scene
                .models
                .get(model)
                .and_then(kjk_game::scene::local_bounds)
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
    if best.is_none()
        && let Some(floor) = floor
    {
        let floor = Vec3::from_array(floor);
        let mut smallest: Option<(f32, Target)> = None;
        for (target, models, _) in &state.drawn.things {
            let (Target::Room(_), Some((position, _))) = (target, state.editor.placement(*target))
            else {
                continue;
            };
            for &model in models {
                let Some((low, high)) = scene
                    .models
                    .get(model)
                    .and_then(kjk_game::scene::local_bounds)
                else {
                    continue;
                };
                let local = floor - Vec3::from_array(position);
                let slack = Vec3::splat(0.05);
                if local.cmpge(low - slack).all() && local.cmple(high + slack).all() {
                    let size = (high - low).x * (high - low).y;
                    if smallest.is_none_or(|(least, _)| size < least) {
                        smallest = Some((size, *target));
                    }
                }
            }
        }
        return smallest.map(|(_, target)| target);
    }
    best.map(|(_, target)| target)
}

/// Adds the install's room `model` to the new map so that its floor's
/// middle stands at `at` (`library` says where its own area places it).
pub(crate) fn add_room(state: &mut EditorState, model: &str, at: [f32; 3]) -> Result<(), String> {
    if state.editor.project.map.is_none() {
        return Err("only a new map has rooms (docs/LEVEL-EDIT.md)".into());
    }
    let found = state
        .setup
        .rooms
        .iter()
        .find(|room| room.model.eq_ignore_ascii_case(model))
        .cloned()
        .ok_or(format!("the install has no room {model}"))?;
    // The walkmesh's middle and floor, which stand where the room is put.
    let (middle, low) = state
        .setup
        .resolver
        .read(&found.model, ResType::WOK)
        .and_then(|bytes| fmt_odyssey::bwm::Walkmesh::parse(&bytes).ok())
        .and_then(|walkmesh| {
            let points = walkmesh
                .vertices
                .iter()
                .map(|point| Vec3::from_array(*point));
            kjk_game::scene::span(points)
        })
        .map_or(
            (Vec3::from_array(found.from), found.from[2]),
            |(low, high)| ((low + high) * 0.5, low.z),
        );
    let offset = middle - Vec3::from_array(found.from);
    let position = [
        at[0] - offset.x,
        at[1] - offset.y,
        at[2] - (low - found.from[2]),
    ];
    state.apply(Edit::AddRoom(level_edit::Room {
        model: found.model.clone(),
        from: found.from,
        position,
    }));
    Ok(())
}

/// A left click selects what is under the pointer, or puts the grabbed
/// selection down.
pub(crate) fn number(word: &str) -> Result<f32, String> {
    word.parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or(format!("{word:?} is not a number"))
}

pub(crate) fn point(words: &[&str]) -> Result<[f32; 3], String> {
    match words {
        [x, y, z] => Ok([number(x)?, number(y)?, number(z)?]),
        _ => Err("a point is three numbers: x y z".into()),
    }
}

fn kind_named(word: &str) -> Option<Kind> {
    match word {
        "placeable" => Some(Kind::Placeable),
        "creature" => Some(Kind::Creature),
        "waypoint" => Some(Kind::Waypoint),
        "trigger" => Some(Kind::Trigger),
        "sound" => Some(Kind::Sound),
        "store" => Some(Kind::Store),
        _ => None,
    }
}

impl EditSetup {
    /// The install's templates of a kind the editor adds.
    pub(crate) fn templates_of(&self, kind: Kind) -> &[String] {
        match kind {
            Kind::Placeable => &self.placeables,
            Kind::Creature => &self.creatures,
            Kind::Waypoint => &self.waypoints,
            Kind::Trigger => &self.triggers,
            Kind::Sound => &self.sounds,
            Kind::Store => &self.stores,
            Kind::Door | Kind::Encounter => &[],
        }
    }
}

/// Adds an object of `kind` made from the install's `template` at
/// `position`. A kind read whole from its GIT entry (a waypoint) takes the
/// template's own fields into its entry.
pub(crate) fn add_object(
    state: &mut EditorState,
    kind: Kind,
    template: &str,
    position: [f32; 3],
) -> Result<(), String> {
    let template = template.to_ascii_lowercase();
    if !ADDABLE.contains(&kind) {
        return Err(format!("a {} cannot be added", kind_name(kind)));
    }
    if state
        .setup
        .templates_of(kind)
        .binary_search(&template)
        .is_err()
    {
        return Err(format!(
            "the install has no {} template {template}",
            kind_name(kind)
        ));
    }
    let base = if level_edit::template::whole(kind) {
        let (_, restype) = kind.list_and_type();
        let gff = state
            .template_gff(&template, restype)
            .ok_or(format!("the template {template} does not read"))?;
        Some(gff.root)
    } else {
        None
    };
    state.apply(Edit::AddObject {
        kind,
        template,
        position,
        facing: 0.0,
        base,
    });
    Ok(())
}

/// Runs one `edit` command (`docs/LEVEL-EDIT.md`). Keys run the same
/// commands.
pub(crate) fn run(
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

pub(crate) fn command(
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
        ["select", "room", n] => {
            let at: usize = n.parse().map_err(|_| format!("{n:?} is not a number"))?;
            let rooms = state
                .editor
                .project
                .map
                .as_ref()
                .map_or(0, |map| map.rooms.len());
            if at >= rooms {
                return Err(format!("there is no room {at}"));
            }
            state.editor.selected = Some(Target::Room(at));
        }
        ["select", "entry"] => {
            if state.editor.project.map.is_none() {
                return Err("only a new map has an entry".into());
            }
            state.editor.selected = Some(Target::Entry);
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
        ["add", "room", model, rest @ ..] => {
            let at = match rest {
                [] => place_point(state, cameras),
                rest => point(rest)?,
            };
            add_room(state, model, at)?;
        }
        ["entry", rest @ ..] => {
            let at = match rest {
                [] => place_point(state, cameras),
                rest => point(rest)?,
            };
            state.apply(Edit::Move(Target::Entry, at));
        }
        ["add", kind, template, rest @ ..] => {
            let kind = kind_named(kind)
                .ok_or("add takes placeable, creature, waypoint, trigger, sound or store")?;
            let position = match rest {
                [] => place_point(state, cameras),
                rest => point(rest)?,
            };
            add_object(state, kind, template, position)?;
        }
        ["tag", tag] => {
            let target = selected(state)?;
            state.change_template(target, Change::Tag(tag.to_string()));
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
        ["script", label, script] => {
            let target = selected(state)?;
            let script = if *script == "-" { "" } else { script };
            state.change_template(
                target,
                Change::Script(label.to_string(), script.to_string()),
            );
        }
        [
            "flag",
            flag @ ("useable" | "inventory"),
            on @ ("on" | "off"),
        ] => {
            let target = selected(state)?;
            let label = if *flag == "useable" {
                "Useable"
            } else {
                "HasInventory"
            };
            state.change_template(target, Change::Flag(label.to_string(), *on == "on"));
        }
        ["item", "add", item] => {
            let target = selected(state)?;
            state.change_template(target, Change::AddItem(item.to_ascii_lowercase()));
        }
        ["item", "remove", at] => {
            let target = selected(state)?;
            let at: usize = at.parse().map_err(|_| format!("{at:?} is not a number"))?;
            state.change_template(target, Change::RemoveItem(at));
        }
        ["collision", on @ ("on" | "off")] => {
            state.show_collision = *on == "on";
            state.quiet = SETTLE_FRAMES;
        }
        ["save"] => {
            let made_by = format!("kjk {}", kjk_game::GIT_SHA);
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
            let resolver = state.setup.resolver.clone();
            let read = |name: &str, restype: ResType| resolver.read(name, restype);
            let written = state.editor.save_with(&made_by, &read)?;
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
            // Each reach's line ends "... N regions: E escapes, P perches, ...".
            let count = |line: &str, word: &str| {
                line.split(',').find_map(|part| {
                    part.trim()
                        .strip_suffix(word)?
                        .rsplit(' ')
                        .next()?
                        .parse::<usize>()
                        .ok()
                })
            };
            let reaches: Vec<String> = state
                .audit
                .iter()
                .filter(|line| {
                    let line = line.trim_start();
                    ["walk ", "jump ", "force "]
                        .iter()
                        .any(|reach| line.starts_with(reach))
                })
                .cloned()
                .collect();
            let escapes: usize = reaches
                .iter()
                .filter_map(|line| count(line, " escapes"))
                .max()
                .unwrap_or(0);
            let perches: usize = reaches
                .iter()
                .filter_map(|line| count(line, " perches"))
                .max()
                .unwrap_or(0);
            if !output.status.success() && reaches.is_empty() {
                return Err(format!(
                    "the audit failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
            state.say(format!(
                "audit: at the highest reach a body escapes the level in {escapes} regions and reaches {perches} perches off the walkmesh"
            ));
        }
        _ => return Err("not an edit command (docs/LEVEL-EDIT.md lists them)".into()),
    }
    Ok(())
}

/// `edit` commands from `--cmds`, in order.
pub(crate) fn commands_from_console(
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
        // The window's own state (`ui`): its tool, the picker's tab and
        // search, for scripted screenshots.
        if words.first().is_some_and(|word| word == "ui") {
            state.pending_ui.push(words[1..].to_vec());
            continue;
        }
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        run(state, &words, &cameras);
    }
}

pub(crate) fn kind_name(kind: Kind) -> &'static str {
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
pub(crate) fn describe(state: &EditorState, target: Target) -> serde_json::Value {
    let placement = state.editor.placement(target);
    let (position, facing) = placement.unwrap_or(([0.0; 3], 0.0));
    match target {
        Target::Object(kind, index) => {
            let entry = state.editor.entry(kind, index);
            // What its template makes it do.
            let behaviour = entry.as_ref().and_then(|entry| {
                let gff = state.behaviour_source(kind, entry)?;
                let own = state.editor.project.templates.get(&entry.template);
                let scripts: BTreeMap<String, String> = level_edit::template::scripts(&gff)
                    .into_iter()
                    .filter(|(_, script)| !script.is_empty())
                    .collect();
                Some(serde_json::json!({
                    "project_template": own.is_some(),
                    "copied_from": own.map(|made| made.from.clone()),
                    "scripts": scripts,
                    "useable": level_edit::template::flag(&gff, "Useable"),
                    "holds_items": level_edit::template::flag(&gff, "HasInventory"),
                    "items": level_edit::template::items(&gff),
                }))
            });
            serde_json::json!({
                "kind": kind_name(kind),
                "index": index,
                "template": entry.as_ref().map(|e| e.template.clone()),
                "tag": entry.as_ref().map(|e| state.tag_of(e)),
                "position": position,
                "facing": facing,
                "editable": EDITABLE.contains(&kind),
                "behaviour": behaviour,
            })
        }
        Target::Prop(at) => {
            let prop = state.editor.project.kjk.props.get(at);
            // Where its model's corners stand, lowest and highest.
            let bounds = prop.and_then(|prop| {
                let model = state.prop_models.get(&prop.model)?.as_ref().ok()?;
                let points = model.ir.meshes.iter().flat_map(|mesh| &mesh.positions);
                kjk_game::scene::span(points.map(|point| {
                    let ja = point.map(|value| value / collision::UNIT);
                    Vec3::from_array(prop.place(ja))
                }))
                .map(|(low, high)| [low.to_array(), high.to_array()])
            });
            serde_json::json!({
                "bounds": bounds,
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
        Target::Room(at) => {
            let project = &state.editor.project;
            let room = project.map.as_ref().and_then(|map| map.rooms.get(at));
            serde_json::json!({
                "kind": "room",
                "index": at,
                "name": level_edit::map::room_name(&project.module, at),
                "model": room.map(|room| room.model.clone()),
                "from": room.map(|room| room.from),
                "position": position,
                "facing": facing,
            })
        }
        Target::Entry => serde_json::json!({
            "kind": "entry",
            "position": position,
            "facing": facing,
        }),
    }
}

pub(crate) fn publish(state: Res<EditorState>, mut facts: ResMut<DumpFacts>) {
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
            "prop_models": state.prop_models.iter().map(|(path, read)| match read {
                Ok(model) => serde_json::json!({
                    "model": path,
                    "meshes": model.ir.meshes.iter().map(|mesh| mesh.texture.clone()).collect::<Vec<_>>(),
                    "textures": model.textures.iter().map(|(name, texture)| {
                        let texels = texture.mips.first().map_or(&[][..], Vec::as_slice);
                        let mean = texels.chunks(4).fold([0u64; 3], |sum, t| [sum[0] + u64::from(t[0]), sum[1] + u64::from(t[1]), sum[2] + u64::from(t[2])]);
                        let count = (texels.len() / 4).max(1) as u64;
                        serde_json::json!({"name": name, "size": [texture.width, texture.height], "mean": mean.map(|c| c / count)})
                    }).collect::<Vec<_>>(),
                }),
                Err(why) => serde_json::json!({"model": path, "error": why}),
            }).collect::<Vec<_>>(),
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
