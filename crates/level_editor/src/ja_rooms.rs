//! The editor's rooms tool (`kjk-level-editor ja LEVEL`): a Jedi Academy
//! level cut into rooms by hand, as a KOTOR area is made of rooms
//! (`docs/LEVEL-EDIT.md`, "Jedi Academy levels").
//!
//! The level opens as one room. A click on the view places a point where
//! the click meets the level. The points make a shape, drawn in the view
//! as they are placed. A placed point is dragged to move it, and a click
//! on a line of the shape places a further point there. A button then gives everything inside the shape to
//! a new room, or to the selected one (`asset_ja::level_rooms`). Every
//! change is saved at once into the project (`level_edit::ja_rooms`), and
//! Undo takes the last one back.
//!
//! The level is drawn lit by its own stored light and coloured by room, as
//! one mesh whose triangles read their colour from a small palette texture,
//! a texel a room (`asset_ja::level::Palette`). A cut divides the triangles
//! it crosses, so the mesh and the light atlas are built anew after each
//! cut and put in the old ones' place. Selecting a room only writes the
//! palette anew.

use std::collections::BTreeMap;
use std::path::PathBuf;

use asset_ja::level::{Paint, Palette};
use asset_ja::level_rooms::{Cut, MOST_POINTS, RoomCuts, Shape, Solid};
use asset_odyssey::texture::{Encoding, TextureFacts, TextureIr};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use fmt_ja::bsp::Bsp;
use fmt_ja::md3::Md3;
use kjk_game::scene::{FlyCamera, FlyControls, SceneContent};
use kjk_game::viewer::LoadedScene;
use kjk_game::viewer_ja::Files;

/// How far a new shape reaches above and below its points and out from its
/// faces, in metres, until the user sets them.
const REACH_UP: f32 = 8.0;
const REACH_DOWN: f32 = 1.0;
const MARGIN: f32 = 0.1;

/// The rooms tool's state.
#[derive(Resource)]
pub struct Rooms {
    /// The level's path in Jedi Academy's files, and its file's digest.
    level: String,
    digest: String,
    project: String,
    folder: PathBuf,
    bsp: Bsp,
    /// The level's static models, each read once.
    models: BTreeMap<String, Option<Md3>>,
    /// The level's collision, to find where a click in the view lands.
    collision: collision::Level,
    palette: Palette,
    rooms: RoomCuts,
    /// The rooms before each change, the oldest first.
    undo: Vec<RoomCuts>,
    /// The pieces drawn in each room.
    drawn: Vec<usize>,
    /// The points placed for the next cut, and its shape's reach.
    points: Vec<[f32; 3]>,
    up: f32,
    down: f32,
    margin: f32,
    /// The shape the points make, and what it was made from.
    shape: Option<Solid>,
    shape_of: Option<Shape>,
    selected: Option<usize>,
    /// The point being dragged, and how far it lies from where the drag's
    /// first ray met the point's own height (x and y), so the point does
    /// not jump to the cursor.
    dragging: Option<(usize, [f32; 2])>,
    /// The scene's first door model: the textured level's doors, shown
    /// with it.
    first_door: usize,
    /// The cut whose shape the placed points are, while it is adjusted.
    adjusting: Option<usize>,
    /// The level is shown with its textures, the rooms as outlines, in
    /// place of coloured by room.
    textured: bool,
    rename: String,
    /// The mesh is to be built anew, the palette written anew, and which
    /// model is shown set anew.
    rebuild: bool,
    repaint: bool,
    reshow: bool,
    message: String,
    fonts_set: bool,
}

/// A room's colour, from its name, so a room keeps its colour while others
/// change. A name that ends in a number (`room 12`) takes its hue from the
/// number, each a golden ratio's turn on from the one before, so rooms
/// numbered close together are told apart. Any other name takes its hue
/// from its letters. One room in three is darker and one paler.
fn colour(name: &str) -> [f32; 3] {
    let digits = name.len() - name.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    let number = name[name.len() - digits..]
        .parse::<u32>()
        .unwrap_or_else(|_| {
            name.bytes().fold(0x811c_9dc5u32, |hash, byte| {
                (hash ^ u32::from(byte)).wrapping_mul(0x0100_0193)
            })
        });
    let hue = (f64::from(number) * 0.618_033_988_75).fract() as f32 * 6.0;
    let (depth, bright) = [(0.75, 1.0), (0.85, 0.75), (0.5, 1.0)][(number % 3) as usize];
    let part = |shift: f32| {
        let k = (shift + hue) % 6.0;
        bright * (1.0 - depth * k.min(4.0 - k).clamp(0.0, 1.0))
    };
    [part(5.0), part(3.0), part(1.0)]
}

/// The colour of the rest of the level: grey, so the rooms cut from it
/// stand out.
const REST_COLOUR: [f32; 3] = [0.62; 3];

impl Rooms {
    /// Opens `level` for the project `project` (the level's name when left
    /// out): the level read, cut as the project keeps it, and drawn by its
    /// rooms.
    pub fn open(level: &str, project: Option<&str>) -> Result<(Rooms, LoadedScene), String> {
        let discovery = kjk_game::discover();
        let stack = kjk_game::viewer_ja::stack(&discovery)?;
        let files = Files::new(&stack);
        // The level as the viewer draws it, with its textures: the view's
        // second model, shown in place of the rooms' colours on asking.
        let textured = kjk_game::viewer_ja::level_parts(&discovery, level)?;
        let path = textured.path.clone();
        let digest = textured.digest.clone();
        let bsp = textured.bsp.clone();
        let mut models = BTreeMap::new();
        for entity in &bsp.entities {
            if entity.class() == "misc_model_static"
                && let Some(model) = entity.get("model").map(fmt_ja::game_path)
            {
                models
                    .entry(model.clone())
                    .or_insert_with(|| fmt_ja::md3::parse(&files.read(&model)?).ok());
            }
        }
        let default = path
            .trim_start_matches("maps/")
            .trim_end_matches(".bsp")
            .replace('/', "-");
        let project = project.unwrap_or(&default).to_string();
        let folder = level_edit::folder_of(&project, |var| std::env::var(var).ok())?;
        let kept = level_edit::ja_rooms::JaRooms::read(&folder, &path, &digest)?;
        let (rooms, message) = match kept {
            Some(kept) => (
                RoomCuts {
                    names: std::iter::once(kept.rest).chain(kept.rooms).collect(),
                    cuts: (kept.cuts.into_iter())
                        .map(|cut| Cut {
                            room: cut.room + 1,
                            shape: Shape {
                                points: cut.points,
                                up: cut.up,
                                down: cut.down,
                                margin: cut.margin,
                            },
                        })
                        .collect(),
                },
                format!("Opened the rooms the project {project} keeps."),
            ),
            None => (
                RoomCuts::default(),
                "The level is one room. Click the view to place the points of a shape.".to_string(),
            ),
        };
        let mut rooms = Rooms {
            collision: asset_ja::level_collision::collision(&bsp).level,
            palette: Palette::of(&path),
            level: path,
            digest,
            project,
            folder,
            bsp,
            models,
            rooms,
            undo: Vec::new(),
            drawn: Vec::new(),
            points: Vec::new(),
            up: REACH_UP,
            down: REACH_DOWN,
            margin: MARGIN,
            shape: None,
            shape_of: None,
            selected: None,
            dragging: None,
            adjusting: None,
            first_door: usize::MAX,
            textured: false,
            rename: String::new(),
            rebuild: false,
            repaint: false,
            reshow: true,
            message,
            fonts_set: false,
        };
        let level = rooms.built();
        rooms.drawn = level.tally.rooms.clone();
        let mut textures = textured.textures;
        textures.insert(asset_ja::level::light_name(&rooms.view_name()), level.light);
        textures.insert(rooms.palette.name.clone(), rooms.palette_texture());
        let notes = vec![format!("{} rooms", rooms.rooms.names.len())];
        let mut scene = kjk_game::viewer_ja::level_scene(
            rooms.level.clone(),
            level.model,
            textured.sky,
            textured.doors,
            textures,
            level.start,
            notes,
        );
        // The textured level is the second model, after the rooms' and
        // before the sky's (`reshow`).
        let mut second = scene.models[0].clone();
        second.ir = textured.model;
        scene.models.insert(1, second);
        for door in &mut scene.doors {
            door.model += 1;
        }
        rooms.first_door = scene.doors.first().map_or(usize::MAX, |door| door.model);
        scene.missing = textured.missing;
        Ok((rooms, scene))
    }

    /// The name of the level as the rooms colour it, apart from the level
    /// itself, so the two have light atlases of their own.
    fn view_name(&self) -> String {
        format!("{}#rooms", self.level)
    }

    /// The level as its rooms cut it: one mesh, and its light atlas.
    fn built(&self) -> asset_ja::level::Level {
        let cutter = self.rooms.cutter();
        let md3 = |model: &str| self.models.get(model).cloned().flatten();
        asset_ja::level::level(
            &self.view_name(),
            &self.bsp,
            &md3,
            &Paint::Rooms(&cutter, &self.palette),
        )
    }

    /// The palette as the rooms colour it. With a room selected, the others
    /// are dimmed.
    fn palette_texture(&self) -> TextureIr {
        let side = Palette::SIDE;
        let mut rgba = vec![255u8; side * side * 4];
        for (index, name) in self.rooms.names.iter().enumerate().take(Palette::ROOMS) {
            let own = if index == 0 {
                REST_COLOUR
            } else {
                colour(name)
            };
            let shown = match self.selected {
                Some(selected) if selected != index => own.map(|part| part * 0.3),
                _ => own,
            };
            rgba[index * 4..index * 4 + 3].copy_from_slice(&shown.map(|part| (part * 255.0) as u8));
        }
        TextureIr {
            width: side as u32,
            height: side as u32,
            encoding: Encoding::Rgba8,
            mips: vec![rgba],
            cube: false,
            frames: 1,
            facts: TextureFacts::default(),
            faces: Vec::new(),
            frame_mips: Vec::new(),
        }
    }

    /// Writes the rooms into the project, and says so if it fails.
    fn save(&mut self) {
        let kept = level_edit::ja_rooms::JaRooms {
            level: self.level.clone(),
            digest: self.digest.clone(),
            rest: self.rooms.names[0].clone(),
            rooms: self.rooms.names[1..].to_vec(),
            cuts: (self.rooms.cuts.iter())
                .map(|cut| level_edit::ja_rooms::Cut {
                    room: cut.room - 1,
                    points: cut.shape.points.clone(),
                    up: cut.shape.up,
                    down: cut.shape.down,
                    margin: cut.shape.margin,
                })
                .collect(),
        };
        if let Err(error) = kept.save(&self.folder) {
            self.message = format!("The rooms were not saved. {error}");
        }
    }

    /// Makes one change to the rooms, kept for Undo and saved.
    fn change(&mut self, change: impl FnOnce(&mut Rooms) -> String) {
        let before = self.rooms.clone();
        self.message = change(self);
        if self.rooms != before {
            self.undo.push(before);
            self.save();
            self.rebuild = true;
        }
        self.repaint = true;
    }

    /// A room by what the user calls it: its number in the list, from 1,
    /// or its name.
    fn room(&self, word: &str) -> Option<usize> {
        (word.parse::<usize>().ok())
            .and_then(|number| number.checked_sub(1))
            .filter(|index| *index < self.rooms.names.len())
            .or_else(|| self.rooms.names.iter().position(|name| name == word))
    }

    /// The shape the placed points make, with its reach.
    fn placed(&self) -> Shape {
        Shape {
            points: self.points.clone(),
            up: self.up,
            down: self.down,
            margin: self.margin,
        }
    }

    /// Keeps `shape` the solid of the points as they are now.
    fn refresh_shape(&mut self) {
        let placed = self.placed();
        if self.shape_of.as_ref() != Some(&placed) {
            self.shape = placed.solid();
            self.shape_of = Some(placed);
        }
    }

    fn place(&mut self, at: [f32; 3]) {
        if self.points.len() >= MOST_POINTS {
            self.message = format!("A shape takes at most {MOST_POINTS} points.");
        } else {
            self.points.push(at);
            self.message = format!("{} points placed.", self.points.len());
        }
    }

    /// Places a new point between points `a` and `b`, `share` of the way
    /// from `a`. Between two points that a line of the shape joins, the new
    /// point lies on the shape, which stays as it is until the point is
    /// dragged.
    fn insert(&mut self, a: usize, b: usize, share: f32) -> Option<usize> {
        if self.points.len() >= MOST_POINTS {
            self.message = format!("A shape takes at most {MOST_POINTS} points.");
            return None;
        }
        let (from, to) = (self.points[a], self.points[b]);
        let share = share.clamp(0.0, 1.0);
        self.points.push(std::array::from_fn(|axis| {
            from[axis] + (to[axis] - from[axis]) * share
        }));
        self.message =
            "Added a point on the shape's line. Drag it to change the shape.".to_string();
        Some(self.points.len() - 1)
    }

    fn select(&mut self, room: Option<usize>) {
        self.selected = room;
        if let Some(room) = room {
            self.rename = self.rooms.names[room].clone();
        }
        self.repaint = true;
    }

    /// Gives everything inside the placed shape to `room`, or to a new
    /// room named `name` (the lowest `room N` not taken when left out).
    fn cut(&mut self, room: Option<usize>, name: Option<String>) {
        let shape = self.placed();
        if shape.solid().is_none() {
            self.message = "The points have no inside yet. Place more points, or set how far \
                            the shape reaches up."
                .to_string();
            return;
        }
        if room == Some(0) {
            self.message = "The rest of the level takes no cut. Delete a room to give its \
                            space back."
                .to_string();
            return;
        }
        if room.is_none() && self.rooms.names.len() >= Palette::ROOMS {
            self.message = format!("A level takes at most {} rooms.", Palette::ROOMS);
            return;
        }
        if let Some(name) = &name
            && self.rooms.names.contains(name)
        {
            self.message = format!("Another room is named {name}.");
            return;
        }
        self.change(|rooms| {
            let (room, said) = match room {
                Some(room) => (
                    room,
                    format!("Added the shape to {}.", rooms.rooms.names[room]),
                ),
                None => {
                    let name = name.unwrap_or_else(|| {
                        (1..)
                            .map(|number| format!("room {number}"))
                            .find(|name| !rooms.rooms.names.contains(name))
                            .expect("a number is free")
                    });
                    rooms.rooms.names.push(name.clone());
                    (
                        rooms.rooms.names.len() - 1,
                        format!("Cut the new room {name}."),
                    )
                }
            };
            rooms.rooms.cuts.push(Cut { room, shape });
            rooms.points.clear();
            rooms.select(Some(room));
            said
        });
    }

    /// Places the points of cut `cut` to be adjusted, with its reach. The
    /// cut keeps its old shape until the adjusted one is applied.
    fn adjust(&mut self, cut: usize) {
        let Some(shape) = self.rooms.cuts.get(cut).map(|cut| cut.shape.clone()) else {
            self.message = "There is no such shape.".to_string();
            return;
        };
        self.points = shape.points;
        self.up = shape.up;
        self.down = shape.down;
        self.margin = shape.margin;
        self.adjusting = Some(cut);
        let room = self.rooms.cuts[cut].room;
        self.select(Some(room));
        self.message = format!(
            "Adjusting a shape of {}. Drag its points, then apply.",
            self.rooms.names[room]
        );
    }

    /// Gives the adjusted shape to the cut it was taken from.
    fn apply(&mut self) {
        let Some(cut) = self.adjusting else {
            self.message = "No shape is being adjusted.".to_string();
            return;
        };
        let shape = self.placed();
        if shape.solid().is_none() {
            self.message = "The points have no inside. Place more points, or set how far the \
                            shape reaches up."
                .to_string();
            return;
        }
        self.change(|rooms| {
            rooms.rooms.cuts[cut].shape = shape;
            rooms.points.clear();
            rooms.adjusting = None;
            let room = rooms.rooms.cuts[cut].room;
            format!("Changed a shape of {}.", rooms.rooms.names[room])
        });
    }

    /// Stops adjusting a shape, which keeps the shape it had.
    fn stop_adjusting(&mut self) {
        if self.adjusting.take().is_some() {
            self.points.clear();
            self.message = "The shape was left as it was.".to_string();
        }
    }

    /// Takes cut `cut` out: what it held goes back to the rooms cut before
    /// it. Its room stays, with its other shapes.
    fn remove_cut(&mut self, cut: usize) {
        if cut >= self.rooms.cuts.len() {
            self.message = "There is no such shape.".to_string();
            return;
        }
        self.adjusting = None;
        self.change(|rooms| {
            let removed = rooms.rooms.cuts.remove(cut);
            format!("Took a shape out of {}.", rooms.rooms.names[removed.room])
        });
    }

    /// The cut that is shape `number` (from 1) of room `room`, in the order
    /// the room's shapes were made.
    fn cut_of(&self, room: usize, number: usize) -> Option<usize> {
        (self.rooms.cuts.iter().enumerate())
            .filter(|(_, cut)| cut.room == room)
            .nth(number.checked_sub(1)?)
            .map(|(index, _)| index)
    }

    fn delete(&mut self, room: usize) {
        self.adjusting = None;
        self.change(|rooms| {
            let name = rooms.rooms.names[room].clone();
            if rooms.rooms.remove(room) {
                rooms.selected = None;
                format!("Deleted {name}. Its space went back to what it was cut from.")
            } else {
                "The rest of the level cannot be deleted.".to_string()
            }
        });
    }

    fn name(&mut self, room: usize, name: &str) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.message = "A room's name cannot be empty.".to_string();
        } else if (self.rooms.names.iter().enumerate())
            .any(|(other, taken)| other != room && *taken == name)
        {
            self.message = format!("Another room is named {name}.");
        } else {
            self.change(|rooms| {
                rooms.rooms.names[room] = name.clone();
                format!("Named the room {name}.")
            });
            // A name changes no triangle, only the room's colour.
            self.rebuild = false;
        }
    }

    fn undo(&mut self) {
        match self.undo.pop() {
            Some(before) => {
                self.rooms = before;
                self.selected = None;
                self.adjusting = None;
                self.points.clear();
                self.save();
                self.rebuild = true;
                self.message = "Took the last change back.".to_string();
            }
            None => self.message = "There is no change to take back.".to_string(),
        }
        self.repaint = true;
    }

    /// One `edit …` command, for checks and for an agent.
    fn command(&mut self, words: &[&str]) {
        let number = |word: &str| word.parse::<f32>().ok();
        let length = |rooms: &mut Rooms, word: &str, set: fn(&mut Rooms, f32)| match number(word) {
            Some(metres) if metres >= 0.0 => set(rooms, metres),
            _ => rooms.message = format!("{word} is not a length in metres."),
        };
        match words {
            ["point", x, y, z] => match [number(x), number(y), number(z)] {
                [Some(x), Some(y), Some(z)] => self.place([x, y, z]),
                _ => self.message = "edit point takes X Y Z in metres.".to_string(),
            },
            ["points", "clear"] => self.points.clear(),
            ["move", point, x, y, z] => {
                let index = (point.parse::<usize>().ok())
                    .and_then(|number| number.checked_sub(1))
                    .filter(|index| *index < self.points.len());
                match (index, [number(x), number(y), number(z)]) {
                    (Some(index), [Some(x), Some(y), Some(z)]) => self.points[index] = [x, y, z],
                    _ => self.message = "edit move takes a point's number and X Y Z.".to_string(),
                }
            }
            ["edge", a, b] | ["edge", a, b, _] => {
                let share = words.get(3).and_then(|word| number(word)).unwrap_or(0.5);
                let point = |word: &str| {
                    (word.parse::<usize>().ok())
                        .and_then(|number| number.checked_sub(1))
                        .filter(|index| *index < self.points.len())
                };
                match (point(a), point(b)) {
                    (Some(a), Some(b)) if a != b => {
                        self.insert(a, b, share);
                    }
                    _ => self.message = "edit edge takes two points' numbers.".to_string(),
                }
            }
            ["points", "undo"] => {
                self.points.pop();
            }
            ["up", metres] => length(self, metres, |rooms, metres| rooms.up = metres),
            ["down", metres] => length(self, metres, |rooms, metres| rooms.down = metres),
            ["margin", metres] => length(self, metres, |rooms, metres| rooms.margin = metres),
            ["cut", name @ ..] => {
                self.cut(None, (!name.is_empty()).then(|| name.join(" ")));
            }
            ["add", room] => match self.room(room) {
                Some(room) => self.cut(Some(room), None),
                None => self.message = format!("{room} is not a room."),
            },
            ["delete", room] => match self.room(room) {
                Some(room) => self.delete(room),
                None => self.message = format!("{room} is not a room."),
            },
            ["rename", room, name @ ..] if !name.is_empty() => match self.room(room) {
                Some(room) => self.name(room, &name.join(" ")),
                None => self.message = format!("{room} is not a room."),
            },
            ["select"] => self.select(None),
            ["select", room] => match self.room(room) {
                Some(room) => self.select(Some(room)),
                None => self.message = format!("{room} is not a room."),
            },
            ["undo"] => self.undo(),
            ["adjust", room, number] | ["unshape", room, number] => {
                let cut = (self.room(room))
                    .zip(number.parse::<usize>().ok())
                    .and_then(|(room, number)| self.cut_of(room, number));
                match (cut, words[0]) {
                    (Some(cut), "adjust") => self.adjust(cut),
                    (Some(cut), _) => self.remove_cut(cut),
                    (None, _) => {
                        self.message = format!("{room} has no shape {number}.");
                    }
                }
            }
            ["apply"] => self.apply(),
            ["cancel"] => self.stop_adjusting(),
            ["view", "textures"] => {
                self.textured = true;
                self.reshow = true;
            }
            ["view", "rooms"] => {
                self.textured = false;
                self.reshow = true;
            }
            _ => self.message = format!("edit {}: not a rooms command", words.join(" ")),
        }
    }
}

/// Adds the rooms tool to the game's app.
pub fn install(app: &mut App, rooms: Rooms) {
    app.insert_resource(rooms)
        .insert_resource(kjk_game::rolloff::LightingMode::Enhanced)
        .insert_resource(FlyControls {
            keys_need_look: true,
            paused: false,
            // The level editor's fly keys (`keys`).
            keys: Some(crate::keys::fly_keys(
                &crate::keys::Keys::load(&mut |problem| bevy::log::warn!("{problem}")).map,
            )),
        })
        .insert_resource(bevy_egui::EguiGlobalSettings {
            auto_create_primary_context: false,
            ..default()
        })
        .add_plugins(EguiPlugin::default())
        .add_systems(Startup, crate::ui::egui_writes_gamma)
        .add_systems(
            Update,
            (crate::ui::attach_egui, commands, rebuild, repaint, reshow).chain(),
        )
        .add_systems(EguiPrimaryContextPass, window)
        .add_systems(Update, publish);
}

fn commands(mut rooms: ResMut<Rooms>, mut requests: ResMut<console::EditRequests>) {
    for words in std::mem::take(&mut requests.0) {
        let words: Vec<&str> = words.iter().map(String::as_str).collect();
        rooms.command(&words);
    }
}

/// After a cut: the level's mesh and its light atlas built anew, and put
/// in the place of the ones drawn.
fn rebuild(
    mut rooms: ResMut<Rooms>,
    scene: Res<SceneContent>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    models: Query<&kjk_game::scene::SceneModel>,
    children: Query<&Children>,
    drawn: Query<&Mesh3d>,
) {
    if !rooms.rebuild {
        return;
    }
    // The level is the scene's first model, and its one mesh hangs from
    // the model's one node, once the scene has drawn it.
    let node = (models.iter())
        .find(|model| model.index == 0)
        .and_then(|model| model.nodes.first().copied());
    let hung = node.and_then(|node| children.get(node).ok());
    let mut found = (hung.into_iter())
        .flat_map(|hung| hung.iter())
        .filter_map(|child| drawn.get(child).ok());
    let (Some(mesh), None) = (found.next(), found.next()) else {
        return;
    };
    let level = rooms.built();
    let Some(part) = level.model.meshes.first() else {
        return;
    };
    let _ = meshes.insert(mesh.id(), kjk_game::scene::level_mesh(part));
    (scene.0.images).replace(
        &asset_ja::level::light_name(&rooms.view_name()),
        &level.light,
        &mut images,
    );
    rooms.drawn = level.tally.rooms;
    rooms.rebuild = false;
    rooms.repaint = true;
}

/// Shows the level coloured by room (the scene's first model) or with its
/// textures (its second), as asked.
fn reshow(
    mut rooms: ResMut<Rooms>,
    models: Query<(Entity, &kjk_game::scene::SceneModel)>,
    mut commands: Commands,
) {
    if !rooms.reshow {
        return;
    }
    let mut found = 0;
    for (entity, model) in &models {
        // The rooms' level is the first model. The textured level is the
        // second, and its doors are the last ones. The sky is shown always.
        let shown = match model.index {
            0 => !rooms.textured,
            1 => rooms.textured,
            index if index >= rooms.first_door => rooms.textured,
            _ => continue,
        };
        commands.entity(entity).insert(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        found += 1;
    }
    // The models are there once the scene has drawn its first frame.
    if found >= 2 {
        rooms.reshow = false;
    }
}

/// Writes the palette anew after a change.
fn repaint(mut rooms: ResMut<Rooms>, scene: Res<SceneContent>, mut images: ResMut<Assets<Image>>) {
    if !rooms.repaint {
        return;
    }
    let texture = rooms.palette_texture();
    // The image is there once the scene has drawn its first frame.
    if (scene.0.images).replace(&rooms.palette.name, &texture, &mut images) {
        rooms.repaint = false;
    }
}

/// Where a place of the level (metres) is in the view, if it is before
/// the camera.
fn on_screen(
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    at: [f32; 3],
) -> Option<egui::Pos2> {
    let (camera, eye) = cameras.iter().next()?;
    let world = kjk_game::scene::game_to_bevy() * Vec3::from_array(at);
    let at = camera.world_to_viewport(eye, world).ok()?;
    Some(egui::pos2(at.x, at.y))
}

fn window(
    mut contexts: EguiContexts,
    mut rooms: ResMut<Rooms>,
    mut controls: ResMut<FlyControls>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let ctx = ctx.clone();
    let rooms = &mut *rooms;
    if !rooms.fonts_set {
        rooms.fonts_set = true;
        if let Ok(fonts) = save_editor::model::fonts::system() {
            crate::ui::use_fonts(&ctx, fonts);
        }
    }
    controls.paused = ctx.egui_wants_keyboard_input();
    rooms.refresh_shape();
    let mut root = egui::Ui::new(
        ctx.clone(),
        "ja rooms".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );
    egui::Panel::left("rooms")
        .resizable(true)
        .default_size(340.0)
        .show(&mut root, |ui| panel(ui, rooms));

    // What is left of the window is the view.
    let view = root.allocate_rect(
        root.available_rect_before_wrap(),
        egui::Sense::click_and_drag(),
    );
    let pointer = view.interact_pointer_pos().or(view.hover_pos());
    let ray = pointer.and_then(|at| crate::state::ray_at(&cameras, Vec2::new(at.x, at.y)));
    // What the pointer is over: a placed point, else a line of the shape.
    let over_point = pointer.and_then(|at| point_under(&cameras, &rooms.points, at));
    let over_line = match (over_point, pointer, ray) {
        (None, Some(at), Some(ray)) => line_under(&cameras, rooms, at, ray),
        _ => None,
    };
    let (shift, alt, picking) = ctx.input(|input| {
        (
            input.modifiers.shift,
            input.modifiers.alt,
            input.modifiers.command,
        )
    });

    // A drag that starts on a point moves it. One that starts on a line
    // places a point there first, and moves that.
    if view.drag_started_by(egui::PointerButton::Primary) {
        let grabbed =
            over_point.or_else(|| over_line.and_then(|(a, b, share)| rooms.insert(a, b, share)));
        rooms.dragging = grabbed.map(|index| {
            let at = rooms.points[index];
            let met = ray.and_then(|ray| at_height(ray, at[2])).unwrap_or(at);
            (index, [at[0] - met[0], at[1] - met[1]])
        });
    }
    if let Some((index, held)) = rooms.dragging
        && view.dragged_by(egui::PointerButton::Primary)
        && let Some(ray) = ray
    {
        let at = rooms.points[index];
        if shift {
            // Up and down: as far as the pointer went, at the point's
            // distance from the eye.
            let above = [at[0], at[1], at[2] + 1.0];
            let metre = (on_screen(&cameras, at).zip(on_screen(&cameras, above)))
                .map(|(low, high)| low.distance(high));
            if let Some(metre) = metre.filter(|metre| *metre > 1.0) {
                rooms.points[index][2] -= view.drag_delta().y / metre;
            }
        } else if alt {
            // Stuck to the level: where the pointer meets it.
            if let Some(met) = level_under(&rooms.collision, ray) {
                rooms.points[index] = met;
            }
        } else if let Some(met) = at_height(ray, at[2]) {
            // Level with itself: where the pointer meets the point's own
            // height.
            rooms.points[index] = [met[0] + held[0], met[1] + held[1], at[2]];
        }
    }
    if view.drag_stopped() {
        rooms.dragging = None;
    }

    // A click on a line of the shape places a point there. A click
    // anywhere else places one where it meets the level. With Ctrl held,
    // it selects the room there.
    if view.clicked()
        && let Some((origin, direction)) = ray
    {
        if over_point.is_some() {
            rooms.message = "Drag the point to move it.".to_string();
        } else if let Some((a, b, share)) = over_line.filter(|_| !picking) {
            rooms.insert(a, b, share);
        } else {
            match level_under(&rooms.collision, (origin, direction)) {
                None => rooms.message = "The click met nothing of the level.".to_string(),
                Some(met) if picking => {
                    // A little before the wall the click met, where the
                    // room is.
                    let before = Vec3::from_array(met) - direction.normalize_or_zero() * 0.1;
                    let room = rooms.rooms.cutter().room_at(before.to_array());
                    rooms.select(Some(room));
                }
                Some(met) => rooms.place(met),
            }
        }
    }

    // The points and the shape they make, drawn over the view. The point
    // or the line under the pointer is drawn stronger.
    let painter = ctx
        .layer_painter(egui::LayerId::background())
        .with_clip_rect(view.rect);
    let yellow = egui::Color32::from_rgb(255, 220, 60);
    // The rooms' shapes: with the textures, every room's in its colour;
    // coloured by room, the selected room's only. The selected room's are
    // drawn stronger. A shape being adjusted is drawn as the points make it.
    for (index, cut) in rooms.rooms.cuts.iter().enumerate() {
        let chosen = rooms.selected == Some(cut.room);
        if (!rooms.textured && !chosen) || rooms.adjusting == Some(index) {
            continue;
        }
        let Some(solid) = cut.shape.solid() else {
            continue;
        };
        let [r, g, b] = colour(&rooms.rooms.names[cut.room]).map(|part| (part * 255.0) as u8);
        let tint = egui::Color32::from_rgb(r, g, b);
        let width = if chosen { 2.5 } else { 1.0 };
        for (&(a, b), &shallow) in solid.edges.iter().zip(&solid.shallow) {
            if shallow {
                continue;
            }
            let ends = (
                on_screen(&cameras, solid.corners[a]),
                on_screen(&cameras, solid.corners[b]),
            );
            if let (Some(a), Some(b)) = ends {
                painter.line_segment([a, b], egui::Stroke::new(width, tint));
            }
        }
    }
    if let Some(shape) = &rooms.shape {
        for (&(a, b), &shallow) in shape.edges.iter().zip(&shape.shallow) {
            let ends = (
                on_screen(&cameras, shape.corners[a]),
                on_screen(&cameras, shape.corners[b]),
            );
            let points = (shape.corner_points[a], shape.corner_points[b]);
            let under = !shallow
                && over_line
                    .is_some_and(|(from, to, _)| points == (from, to) || points == (to, from));
            // A shallow edge, across a top or a bottom that is not flat,
            // is drawn faintly, so the shape's outline stands out.
            let stroke = if shallow {
                egui::Stroke::new(1.0, yellow.gamma_multiply(0.22))
            } else {
                egui::Stroke::new(if under { 3.0 } else { 1.5 }, yellow)
            };
            if let (Some(a), Some(b)) = ends {
                painter.line_segment([a, b], stroke);
            }
        }
    }
    let held = rooms.dragging.map(|(index, _)| index);
    for (index, point) in rooms.points.iter().enumerate() {
        if let Some(at) = on_screen(&cameras, *point) {
            let strong = held == Some(index) || (held.is_none() && over_point == Some(index));
            let size = if strong { 6.5 } else { 3.5 };
            painter.circle_filled(at, size + 1.5, egui::Color32::BLACK);
            painter.circle_filled(at, size, yellow);
        }
    }
}

/// How near the pointer must be to a point or to a line of the shape to be
/// over it, in pixels.
const NEAR_POINT: f32 = 10.0;
const NEAR_LINE: f32 = 6.0;

/// Where a ray from the camera meets the height `z` (metres), if it does
/// within 2 km before the camera.
fn at_height((origin, direction): (Vec3, Vec3), z: f32) -> Option<[f32; 3]> {
    if direction.z.abs() < 1e-5 {
        return None;
    }
    let along = (z - origin.z) / direction.z;
    (along > 0.0 && along * direction.length() < 2000.0)
        .then(|| (origin + direction * along).to_array())
}

/// Where a ray from the camera first meets the level, within 500 m.
fn level_under(level: &collision::Level, (origin, direction): (Vec3, Vec3)) -> Option<[f32; 3]> {
    use collision::CollisionWorld;
    let end = origin + direction.normalize_or_zero() * 500.0;
    let trace = level.trace(
        origin.to_array(),
        end.to_array(),
        &collision::Extents::POINT,
        collision::Contents::PLAYER_SOLID,
    );
    (trace.fraction < 1.0 && !trace.start_solid).then_some(trace.end)
}

/// The placed point the pointer is over: the nearest within `NEAR_POINT`.
fn point_under(
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    points: &[[f32; 3]],
    pointer: egui::Pos2,
) -> Option<usize> {
    (points.iter().enumerate())
        .filter_map(|(index, point)| Some((index, on_screen(cameras, *point)?.distance(pointer))))
        .filter(|(_, away)| *away <= NEAR_POINT)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(index, _)| index)
}

/// The line of the shape the pointer is over, as the two points of the
/// shape it joins and how far along it the pointer is, from the first: the
/// nearest line within `NEAR_LINE`. A line that stands on one point (from
/// the point lowered to the point raised) joins no two points and is
/// passed over, and so is a shallow line.
fn line_under(
    cameras: &Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    rooms: &Rooms,
    pointer: egui::Pos2,
    (origin, direction): (Vec3, Vec3),
) -> Option<(usize, usize, f32)> {
    let shape = rooms.shape.as_ref()?;
    (shape.edges.iter().zip(&shape.shallow))
        .filter_map(|(&(a, b), &shallow)| {
            let (from, to) = (shape.corner_points[a], shape.corner_points[b]);
            // A shallow line takes no point: it runs across the shape, not
            // along its outline.
            if from == to || shallow {
                return None;
            }
            let ends = (
                on_screen(cameras, shape.corners[a])?,
                on_screen(cameras, shape.corners[b])?,
            );
            // How far the pointer is from the line on the screen.
            let along = ends.1 - ends.0;
            let length = along.length_sq();
            let share = if length > 0.0 {
                ((pointer - ends.0).dot(along) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let away = (ends.0 + along * share).distance(pointer);
            // How far along the line, in the level, the pointer's ray
            // passes nearest: the screen's share is not the level's, since
            // the far end of a line is drawn shorter.
            let (start, end) = (
                Vec3::from_array(shape.corners[a]),
                Vec3::from_array(shape.corners[b]),
            );
            let (line, ray) = (end - start, direction.normalize_or_zero());
            let between = start - origin;
            let (ll, lr, lb, rb) = (
                line.dot(line),
                line.dot(ray),
                line.dot(between),
                ray.dot(between),
            );
            let facing = ll - lr * lr;
            let share = if facing.abs() > 1e-6 {
                ((lr * rb - lb) / facing).clamp(0.05, 0.95)
            } else {
                share
            };
            Some((from, to, share, away))
        })
        .filter(|(.., away)| *away <= NEAR_LINE)
        .min_by(|a, b| a.3.total_cmp(&b.3))
        .map(|(from, to, share, _)| (from, to, share))
}

/// The tool's panel.
fn panel(ui: &mut egui::Ui, rooms: &mut Rooms) {
    ui.heading("Rooms");
    ui.label(format!(
        "{}, in the project {}. {}.",
        rooms.level,
        rooms.project,
        match rooms.rooms.names.len() {
            1 => "It is one room".to_string(),
            count => format!("It has {count} rooms"),
        }
    ));
    ui.separator();
    ui.label("A new shape");
    ui.label(format!(
        "Click the view to place a point. {} of {MOST_POINTS} points are placed.",
        rooms.points.len()
    ));
    ui.label("Drag a point to move it. A click on a line of the shape places a point there.")
        .on_hover_text(
            "A dragged point stays at its own height. With Shift held it goes up and down. \
         With Alt held it sticks to the level under the pointer.",
        );
    fn metres(value: &mut f32) -> egui::DragValue<'_> {
        egui::DragValue::new(value)
            .range(0.0..=500.0)
            .speed(0.1)
            .suffix(" m")
    }
    egui::Grid::new("reach").num_columns(2).show(ui, |ui| {
        ui.label("Reach up")
            .on_hover_text("How far the shape reaches above each point.");
        ui.add(metres(&mut rooms.up));
        ui.end_row();
        ui.label("Reach down")
            .on_hover_text("How far the shape reaches below each point.");
        ui.add(metres(&mut rooms.down));
        ui.end_row();
        ui.label("Margin")
            .on_hover_text("How far every face of the shape is moved out, so a wall the points were clicked on is inside.");
        ui.add(metres(&mut rooms.margin));
        ui.end_row();
    });
    let has_inside = rooms.shape.is_some();
    if !rooms.points.is_empty() && !has_inside {
        ui.label("The points have no inside yet.");
    }
    if rooms.adjusting.is_some() {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(has_inside, egui::Button::new("Apply to the shape"))
                .on_hover_text("The room's shape becomes the one the points make now.")
                .clicked()
            {
                rooms.apply();
            }
            if ui
                .button("Cancel")
                .on_hover_text("The shape stays as it was.")
                .clicked()
            {
                rooms.stop_adjusting();
            }
        });
    }
    ui.add_enabled_ui(rooms.adjusting.is_none(), |ui| {
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(has_inside, egui::Button::new("Cut a new room"))
                .on_hover_text("Everything inside the shape becomes a new room.")
                .clicked()
            {
                rooms.cut(None, None);
            }
            let target = rooms.selected.filter(|room| *room != 0);
            if ui
                .add_enabled(
                    has_inside && target.is_some(),
                    egui::Button::new("Add to the selected room"),
                )
                .on_hover_text("Everything inside the shape joins the selected room.")
                .clicked()
            {
                rooms.cut(target, None);
            }
        })
    });
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(
                !rooms.points.is_empty(),
                egui::Button::new("Remove the last point"),
            )
            .clicked()
        {
            rooms.points.pop();
        }
        if ui
            .add_enabled(
                !rooms.points.is_empty(),
                egui::Button::new("Clear the points"),
            )
            .clicked()
        {
            rooms.points.clear();
        }
    });
    ui.separator();
    ui.horizontal(|ui| {
        let target = rooms.selected.filter(|room| *room != 0);
        if ui
            .add_enabled(target.is_some(), egui::Button::new("Delete the room"))
            .on_hover_text("The room's space goes back to what it was cut from.")
            .clicked()
            && let Some(room) = target
        {
            rooms.delete(room);
        }
        if ui
            .add_enabled(!rooms.undo.is_empty(), egui::Button::new("Undo"))
            .clicked()
        {
            rooms.undo();
        }
    });
    if let Some(room) = rooms.selected {
        ui.horizontal(|ui| {
            ui.label("Name");
            let field = ui.text_edit_singleline(&mut rooms.rename);
            if field.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                let name = rooms.rename.clone();
                rooms.name(room, &name);
            }
        });
    }
    // The selected room's shapes, each to adjust or take out.
    let shapes: Vec<usize> = (rooms.rooms.cuts.iter().enumerate())
        .filter(|(_, cut)| Some(cut.room) == rooms.selected && cut.room != 0)
        .map(|(index, _)| index)
        .collect();
    let mut asked: Option<(usize, bool)> = None;
    for (number, &cut) in shapes.iter().enumerate() {
        ui.horizontal(|ui| {
            let points = rooms.rooms.cuts[cut].shape.points.len();
            ui.label(format!("Shape {}, {points} points", number + 1));
            let free = rooms.adjusting.is_none();
            if ui
                .add_enabled(free, egui::Button::new("Adjust"))
                .on_hover_text("Places the shape's points again, to drag them.")
                .clicked()
            {
                asked = Some((cut, true));
            }
            if ui
                .add_enabled(free, egui::Button::new("Take out"))
                .on_hover_text("What the shape held goes back to what it was cut from.")
                .clicked()
            {
                asked = Some((cut, false));
            }
        });
    }
    match asked {
        Some((cut, true)) => rooms.adjust(cut),
        Some((cut, false)) => rooms.remove_cut(cut),
        None => {}
    }
    ui.label(&rooms.message);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Show");
        let before = rooms.textured;
        ui.selectable_value(&mut rooms.textured, false, "Rooms in colour");
        ui.selectable_value(&mut rooms.textured, true, "Textures");
        if rooms.textured != before {
            rooms.reshow = true;
        }
    });
    ui.label("Ctrl and a click on the view selects the room there.");
    let mut clicked = None;
    egui::ScrollArea::vertical().show(ui, |ui| {
        for (index, name) in rooms.rooms.names.iter().enumerate() {
            ui.horizontal(|ui| {
                let (swatch, _) =
                    ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
                let own = if index == 0 {
                    REST_COLOUR
                } else {
                    colour(name)
                };
                let [r, g, b] = own.map(|part| (part * 255.0) as u8);
                ui.painter()
                    .rect_filled(swatch, 2.0, egui::Color32::from_rgb(r, g, b));
                let cuts = (rooms.rooms.cuts.iter())
                    .filter(|cut| cut.room == index)
                    .count();
                let pieces = rooms.drawn.get(index).copied().unwrap_or(0);
                let text = if index == 0 {
                    format!("{}. {name}   {pieces} triangles", index + 1)
                } else {
                    format!("{}. {name}   {pieces} triangles, {cuts} shapes", index + 1)
                };
                if ui
                    .selectable_label(rooms.selected == Some(index), text)
                    .clicked()
                {
                    clicked = Some(index);
                }
            });
        }
    });
    if let Some(index) = clicked {
        let again = rooms.selected == Some(index);
        rooms.select((!again).then_some(index));
    }
}

/// The rooms tool's facts for `dump`.
fn publish(rooms: Res<Rooms>, mut facts: ResMut<console::DumpFacts>) {
    if !facts.wanted() {
        return;
    }
    facts.0.insert(
        "ja_rooms".into(),
        serde_json::json!({
            "level": rooms.level,
            "project": rooms.project,
            "rooms": rooms.rooms.names.iter().enumerate().map(|(index, name)| {
                serde_json::json!({
                    "name": name,
                    "triangles": rooms.drawn.get(index).copied().unwrap_or(0),
                    "shapes": rooms.rooms.cuts.iter().filter(|cut| cut.room == index).count(),
                })
            }).collect::<Vec<_>>(),
            "points": rooms.points,
            "shape": rooms.placed().solid().is_some(),
            "selected": rooms.selected.map(|room| room + 1),
            "undo": rooms.undo.len(),
            "adjusting": rooms.adjusting.map(|cut| cut + 1),
            "textured": rooms.textured,
            "drawn": !rooms.rebuild && !rooms.repaint,
            "file": level_edit::ja_rooms::path(&rooms.folder, &rooms.level),
            "message": rooms.message,
        }),
    );
}
