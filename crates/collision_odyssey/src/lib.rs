//! KOTOR areas as collision levels (`collision::Level`).
//!
//! An area's collision is built from its files at load, through a
//! `Source`, so this crate does no I/O:
//!
//! - Rooms: every drawn triangle of each room model (the LYT places the
//!   model; its nodes' rest transforms place the meshes), solid from both
//!   sides, and every face of the room's walkmesh (`.wok`, already in world
//!   space). A drawn triangle takes the material of the walkmesh face it
//!   lies on, if any, so a trace reports the floor's material; walls and
//!   ceilings the walkmesh does not cover have material 0. Clutter low on
//!   the floor, which KOTOR's walker walks through, is left out
//!   ([`floor_clutter`]). A floor's
//!   ground (sand, snow, metal, …: sounds only) comes from its texture and
//!   material ([`ground`]), and a walkmesh face takes the ground of the
//!   floor drawn on it.
//! - Doors: the three door walkmeshes (`<model>0/1/2.dwk`: closed, open one
//!   way, open the other) as brushes owned by the door, solid in state 0,
//!   1 and 2 respectively; an open one that is only a placeholder sheet is
//!   left out ([`placeholder`]). Placeables: `<model>.pwk`, solid in state 0 (in
//!   place), and the model's drawn triangles, sorted by
//!   [`placeable_shape`]. Both at the instance's position and bearing; the
//!   entity is the instance's index in the area's instance list.
//! - A walkmesh face's contents come from `surfacemat.2da`: see
//!   [`Surfaces::contents`].
//! - Walls along the walkmesh's open edges, as `WALK_CLIP` ([`edges`]),
//!   and round the footprint of each placeable with no drawn shape (its
//!   walkmesh's non-walkable faces that face up), as `WALK_CLIP` owned by
//!   the placeable, solid in state 0 ([`footprints`]).
//! - Clip brushes from the module's override file, as `PLAYER_CLIP`.
//!
//! The walkmesh's walkable faces are kept apart too: they say where KOTOR
//! lets a character be, which the containment audit measures against.
//!
//! [`build_module`] does it for a module's entry area with the module's
//! built-in clip file ([`built_in_clip`]); [`set_doors`] opens its doors.

use std::collections::BTreeMap;

use asset_odyssey::model::ModelIr;
use asset_odyssey::module::{AreaIr, Direction, Kind, Source};
use collision::clip_file::ClipFile;
use collision::{Contents, Extents, Level, LevelBuilder, Tag, trig};
use fmt_odyssey::ResType;
use fmt_odyssey::bwm::Walkmesh;
use fmt_odyssey::mdl::Model;
use fmt_odyssey::twoda::TwoDa;

pub mod edges;
pub mod footprints;
pub mod ground;
mod modules;
pub mod overrides;
#[cfg(feature = "probes")]
#[allow(
    clippy::disallowed_methods,
    reason = "checks and traces set up their own cases on one machine"
)]
pub mod probes;

pub use edges::EdgeWalls;
pub use modules::{Doors, ModuleCollision, build_module, built_in_clip, set_doors};

/// A hash of the code that builds an area's collision and navigation (this
/// crate's and `collision`'s sources but their checks, the clip files):
/// stored bakes are keyed by it, so changed code never reads an old one.
pub const BUILDER: &str = env!("KJK_AREA_BUILDER");

/// A row of `surfacemat.2da`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfaceMat {
    pub label: String,
    /// Characters may stand on it.
    pub walk: bool,
    /// Walking checks it (a non-walkable one then blocks).
    pub walk_check: bool,
    /// It blocks sight.
    pub line_of_sight: bool,
    /// Grass grows on it.
    pub grass: bool,
}

/// The surface materials, by row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Surfaces {
    pub rows: Vec<SurfaceMat>,
}

impl Surfaces {
    /// `surfacemat.2da`'s rows, its flags read as reone reads a boolean
    /// (`TwoDa::flag`).
    pub fn from_2da(table: &TwoDa) -> Surfaces {
        Surfaces {
            rows: (0..table.cells.len())
                .map(|row| SurfaceMat {
                    label: table.text(row, "label").unwrap_or_else(String::new),
                    walk: table.flag(row, "walk"),
                    walk_check: table.flag(row, "walkcheck"),
                    line_of_sight: table.flag(row, "lineofsight"),
                    grass: table.flag(row, "grass"),
                })
                .collect(),
        }
    }

    pub fn read(source: &dyn Source) -> Option<Surfaces> {
        let bytes = source.read("surfacemat", ResType::TWO_DA)?;
        Some(Surfaces::from_2da(&TwoDa::parse(&bytes).ok()?))
    }

    pub fn walkable(&self, material: u32) -> bool {
        self.rows.get(material as usize).is_some_and(|row| row.walk)
    }

    pub fn label(&self, material: u32) -> &str {
        self.rows
            .get(material as usize)
            .map_or("?", |row| row.label.as_str())
    }

    /// Grass grows on each surface material, indexed by material.
    pub fn grass(&self) -> Vec<bool> {
        self.rows.iter().map(|row| row.grass).collect()
    }

    /// A walkmesh face's contents: solid, whatever its material. A face
    /// that is walkable is floor; one that is not (non-walk, obscuring,
    /// lava, pits, deep water) is still a surface of the level, marked
    /// `NONWALK` so no step lands on it (furniture beside walkable floor,
    /// within the half width the edge walls stand back). Material 0 (a
    /// drawn surface no walkmesh face covers) is plain solid.
    pub fn contents(&self, material: u32) -> Contents {
        if material == 0 || self.walkable(material) {
            Contents::SOLID
        } else {
            Contents::SOLID | Contents::NONWALK
        }
    }
}

/// Where a model's files are read from.
pub struct Loader<'a> {
    pub source: &'a dyn Source,
}

impl Loader<'_> {
    fn model(&self, name: &str) -> Option<ModelIr> {
        let mdl = self.source.read(name, ResType::MDL)?;
        let mdx = self
            .source
            .read(name, ResType::MDX)
            .unwrap_or_else(Vec::new);
        let model = Model::parse(&mdl, &mdx).ok()?;
        Some(ModelIr::from_model(&model))
    }

    /// Whether texture `name` blends additively (`blending additive`), by
    /// its loose TXI, else the one its TPC carries.
    fn additive(&self, name: &str) -> bool {
        let txi = self
            .source
            .read(name, ResType::TXI)
            .map(|bytes| fmt_odyssey::txi::Txi::parse(&bytes))
            .or_else(|| {
                let bytes = self.source.read(name, ResType::TPC)?;
                Some(fmt_odyssey::texture::Texture::parse_tpc(&bytes).ok()?.txi)
            });
        txi.is_some_and(|txi| {
            asset_odyssey::texture::TextureFacts::from_txi(&txi, false).blending
                == asset_odyssey::texture::Blending::Additive
        })
    }

    fn walkmesh(&self, name: &str, kind: ResType) -> Option<Walkmesh> {
        let bytes = self.source.read(name, kind)?;
        Walkmesh::parse(&bytes).ok()
    }
}

/// One source of triangles, counted.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Count {
    pub triangles: usize,
    /// Of those, steeper than JA's walkable slope (normal Z below 0.7).
    pub steep: usize,
}

impl Count {
    fn add(&mut self, corners: &[[f32; 3]; 3]) {
        self.triangles += 1;
        let normal = normal(corners);
        if normal.is_some_and(|n| n[2].abs() < collision::player::WALKABLE_NORMAL_Z) {
            self.steep += 1;
        }
    }
}

/// What went into a level.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildStats {
    pub rooms: usize,
    pub rooms_without_model: Vec<String>,
    pub rooms_without_walkmesh: Vec<String>,
    /// Walkmesh faces by material row.
    pub walkmesh_materials: BTreeMap<u32, Count>,
    pub render: Count,
    /// Drawn triangles that took a walkmesh face's material.
    pub render_classified: usize,
    /// Those triangles by the material they took and their texture: what
    /// each material's floors look like.
    pub floor_textures: BTreeMap<(u32, String), usize>,
    /// Drawn triangles left out, by why.
    pub render_skipped: BTreeMap<String, usize>,
    pub doors: usize,
    pub door_walkmeshes: usize,
    /// Open doors' walkmeshes left out as placeholders (see
    /// [`placeholder`]).
    pub door_placeholders: usize,
    pub placeables: usize,
    pub placeable_walkmeshes: usize,
    pub clip_brushes: usize,
    /// Non-walkable walkmesh faces left out by a clip file's `leave` boxes.
    pub walkmesh_left: usize,
    /// Walls along the walkmesh's open edges ([`edges`]).
    pub edge_walls: usize,
    /// Walls round placeables' footprints ([`footprints`]).
    pub footprint_walls: usize,
    /// Placeables' drawn triangles in the level ([`placeable_shape`]).
    pub placeable_shape: Count,
    /// Of those, the triangles of low bases kept as floor.
    pub placeable_bases: usize,
    /// Placeables' drawn triangles left out as clutter (small, low parts).
    pub placeable_clutter: usize,
    /// Placeables' drawn triangles left out by why (see-through).
    pub placeable_skipped: BTreeMap<String, usize>,
    /// Triangles with no area.
    pub degenerate: usize,
}

/// An entity whose brushes switch with its state.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityCollision {
    /// The instance's index in `AreaIr::instances`.
    pub entity: u32,
    pub kind: Kind,
    pub tag: String,
    pub model: String,
    /// The states it has brushes for (bit n for state n).
    pub states: u8,
    /// How high its state-0 walkmesh rises from its lowest corner, metres
    /// (a flat footprint's is nought).
    pub rise: f32,
    /// Its footprint: the state-0 walkmesh's non-walkable faces that face
    /// up, world space.
    pub footprint: Vec<[[f32; 3]; 3]>,
    /// The walls round its footprint in the level ([`footprints`]): for a
    /// placeable with a drawn shape, their upper parts alone.
    pub footprint_walls: usize,
    /// Its footprint too small for walls round it, a post at its middle
    /// instead ([`footprints::walls`]).
    pub post: bool,
    /// Its footprint has walls round it, or would have without a drawn
    /// shape: routes go round it ([`AreaCollision::nav_faces`]).
    pub walled: bool,
    /// Its drawn triangles in the level ([`placeable_shape`]).
    pub shape: usize,
    /// Its drawn triangles left out as clutter (small, low parts).
    pub shape_clutter: usize,
}

/// An area's collision.
#[derive(Debug, Clone)]
pub struct AreaCollision {
    pub level: Level,
    /// The walkmesh's walkable faces, world space, with their material:
    /// where KOTOR lets a character stand.
    pub walkable: Vec<([[f32; 3]; 3], u32)>,
    /// Doors' walkable faces in their open state (`<model>1.dwk`, else
    /// `2`): the floor of a doorway the rooms' walkmeshes leave out
    /// (Taris's sewers), which KOTOR walks when the door is open. Routes
    /// cross it (`AreaCollision::nav_faces`); the edge walls do not know it.
    pub doorways: Vec<[[f32; 3]; 3]>,
    /// Every triangle in the level, world space, with what it came from
    /// (for drawing the collision over the scene).
    pub triangles: Vec<SourceTriangle>,
    pub entities: Vec<EntityCollision>,
    pub stats: BuildStats,
}

impl AreaCollision {
    /// The faces a navigation mesh is baked from: the walkable walkmesh and
    /// the doorways' floors, cut along the walled placeables' footprints
    /// ([`footprints::cut`]); not a post's (a jar's), which a walker passes
    /// as it does a body: cut, a jar's slivers of floor in Korriban's
    /// academy turned a route back on itself at every plan.
    pub fn nav_faces(&self) -> Vec<[[f32; 3]; 3]> {
        let floor: Vec<[[f32; 3]; 3]> = self
            .walkable
            .iter()
            .map(|(corners, _)| *corners)
            .chain(self.doorways.iter().copied())
            .collect();
        let prints: Vec<[[f32; 3]; 3]> = self
            .entities
            .iter()
            .filter(|entity| entity.walled && !entity.post)
            .flat_map(|entity| entity.footprint.iter().copied())
            .collect();
        footprints::cut(&floor, &prints)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    RoomMesh,
    Walkmesh,
    Door,
    Placeable,
    /// The middle of a wall along an open walkmesh edge or round a
    /// placeable's footprint.
    EdgeWall,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SourceTriangle {
    pub corners: [[f32; 3]; 3],
    pub origin: Origin,
    pub material: u32,
}

fn normal(corners: &[[f32; 3]; 3]) -> Option<[f32; 3]> {
    let [a, b, c] = *corners;
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    (length > 1e-9).then(|| [n[0] / length, n[1] / length, n[2] / length])
}

/// A rigid placement: turned `(sin, cos)` about Z, then moved.
#[derive(Debug, Clone, Copy)]
struct Pose {
    position: [f32; 3],
    sin: f32,
    cos: f32,
}

/// The sine and cosine of a facing as stored: a bearing's, or the angle of
/// an orientation vector through `trig::atan2_deg` (never the platform's
/// `atan2`, so every machine builds the same level).
pub fn turn(direction: Direction) -> (f32, f32) {
    match direction {
        Direction::Bearing(radians) => trig::sin_cos(radians),
        Direction::Vector([x, y]) => trig::sin_cos_deg(trig::atan2_deg(y, x)),
    }
}

impl Pose {
    fn new(position: [f32; 3], direction: Direction) -> Pose {
        let (sin, cos) = turn(direction);
        Pose { position, sin, cos }
    }

    fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        [
            self.cos * p[0] - self.sin * p[1] + self.position[0],
            self.sin * p[0] + self.cos * p[1] + self.position[1],
            p[2] + self.position[2],
        ]
    }
}

/// A face's area seen from above.
fn flat_area(corners: &[[f32; 3]; 3]) -> f32 {
    let [a, b, c] = corners;
    ((b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])).abs() * 0.5
}

/// How far a drawn triangle may sit from a walkmesh face and still be the
/// same surface, metres along its normal.
const SAME_SURFACE: f32 = 0.1;

/// How an area's collision is built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildOptions {
    /// Walls along the walkmesh's open edges; `None` for none.
    pub edge_walls: Option<EdgeWalls>,
    /// Whether an edge wall ends at the top of furniture standing behind
    /// it indoors (`edges::furniture_top`), so a jump lands on the
    /// furniture; else every wall reaches its full height.
    pub furniture_tops: bool,
    /// Whether a placeable is solid where its model is drawn
    /// ([`placeable_shape`]), without walls round its footprint; else it
    /// is its walkmesh and the walls alone.
    pub placeable_shapes: bool,
}

impl BuildOptions {
    /// Whether placeables are built with their drawn shapes:
    /// `KJK_PLACEABLE_SHAPES=off` builds them as their walkmeshes and
    /// footprint walls alone, in play and in the `kjk collision` verbs, to
    /// compare.
    pub fn placeable_shapes_from_env() -> bool {
        std::env::var("KJK_PLACEABLE_SHAPES").map_or(true, |value| value != "off")
    }
}

impl Default for BuildOptions {
    fn default() -> BuildOptions {
        BuildOptions {
            edge_walls: Some(EdgeWalls {
                height: edges::HEIGHT,
            }),
            furniture_tops: true,
            placeable_shapes: true,
        }
    }
}

/// Each room's walkable walkmesh faces (world space), in the area's room
/// order; none for a room without a walkmesh: which room a place is in
/// (TSL's room Force ratings).
pub fn room_floors(loader: &Loader, area: &AreaIr, surfaces: &Surfaces) -> Vec<Vec<[[f32; 3]; 3]>> {
    area.rooms
        .iter()
        .map(|room| {
            loader
                .walkmesh(&room.model, ResType::WOK)
                .map(|walkmesh| {
                    walkmesh
                        .faces
                        .iter()
                        .filter(|face| surfaces.walkable(face.material))
                        .map(|face| face.vertices.map(|v| walkmesh.vertices[v as usize]))
                        .collect()
                })
                .unwrap_or_else(Vec::new)
        })
        .collect()
}

/// Builds an area's collision. `clip` adds the module's clip brushes.
pub fn build_area(
    loader: &Loader,
    area: &AreaIr,
    surfaces: &Surfaces,
    clip: Option<&ClipFile>,
    options: &BuildOptions,
) -> AreaCollision {
    let mut stats = BuildStats {
        rooms: area.rooms.len(),
        ..BuildStats::default()
    };
    let mut builder = LevelBuilder::new();
    let mut triangles = Vec::new();
    let mut walkable = Vec::new();

    // The walkmesh first: its faces classify the drawn triangles.
    let mut walk_builder = LevelBuilder::new();
    let mut walk_faces = Vec::new();
    // The room each walkmesh face is of, and each walkable one.
    let mut face_rooms = Vec::new();
    let mut walkable_rooms = Vec::new();
    for (index, room) in area.rooms.iter().enumerate() {
        let Some(walkmesh) = loader.walkmesh(&room.model, ResType::WOK) else {
            stats.rooms_without_walkmesh.push(room.model.clone());
            continue;
        };
        for face in &walkmesh.faces {
            let corners = face.vertices.map(|v| walkmesh.vertices[v as usize]);
            stats
                .walkmesh_materials
                .entry(face.material)
                .or_insert_with(Count::default)
                .add(&corners);
            walk_faces.push((corners, face.material));
            face_rooms.push(index);
            walk_builder.triangle(corners, Tag::level(Contents::SOLID, face.material as u16));
        }
    }
    let walk_level = walk_builder.build();
    // The rooms' drawn triangles, each with the walkmesh material it lies
    // on and the ground its texture shows there.
    let mut additive = BTreeMap::new();
    let drawn: Vec<([[f32; 3]; 3], Option<u32>, collision::Ground)> =
        drawn_triangles(loader, area, &mut additive, &mut stats)
            .into_iter()
            .map(|(corners, texture)| {
                let material = classify(&walk_level, &corners);
                let ground = material.map_or(collision::Ground::Unknown, |material| {
                    stats.render_classified += 1;
                    let ground = ground::of(material, &texture);
                    *stats.floor_textures.entry((material, texture)).or_insert(0) += 1;
                    ground
                });
                (corners, material, ground)
            })
            .collect();
    // The floors drawn, by their ground: what each walkmesh face looks like.
    let mut look_builder = LevelBuilder::new();
    for &(corners, material, ground) in &drawn {
        if let Some(material) = material {
            look_builder.triangle(
                corners,
                Tag::level(Contents::SOLID, material as u16).on(ground),
            );
        }
    }
    let look_level = look_builder.build();
    for (&(corners, material), &room) in walk_faces.iter().zip(&face_rooms) {
        if surfaces.walkable(material) {
            walkable.push((corners, material));
            walkable_rooms.push(room);
        }
        let ground = lies_on(&look_level, &corners)
            .map(|surface| surface.ground)
            .filter(|&ground| ground != collision::Ground::Unknown)
            .unwrap_or_else(|| ground::of(material, ""));
        let tag = Tag::level(surfaces.contents(material), material as u16).on(ground);
        if clip.is_some_and(|clip| left_out(clip, &corners, surfaces.walkable(material))) {
            stats.walkmesh_left += 1;
            continue;
        }
        if builder.triangle(corners, tag) {
            triangles.push(SourceTriangle {
                corners,
                origin: Origin::Walkmesh,
                material,
            });
        } else {
            stats.degenerate += 1;
        }
    }
    for (corners, material, ground) in drawn {
        if material.is_none() && floor_clutter(&walk_level, surfaces, &corners) {
            *stats
                .render_skipped
                .entry("floor clutter".into())
                .or_insert(0) += 1;
            continue;
        }
        let material = material.unwrap_or(0);
        let tag = Tag::level(surfaces.contents(material), material as u16).on(ground);
        if builder.triangle(corners, tag) {
            stats.render.add(&corners);
            triangles.push(SourceTriangle {
                corners,
                origin: Origin::RoomMesh,
                material,
            });
        } else {
            stats.degenerate += 1;
        }
    }

    // The rooms' solid alone (no door, placeable or wall yet): what
    // furniture the edge walls find behind them. None where the module's
    // clip file keeps its walls whole.
    let whole_walls = clip.is_some_and(|clip| clip.whole_walls);
    let rooms = (options.edge_walls.is_some() && options.furniture_tops && !whole_walls)
        .then(|| builder.clone().build());
    // Each walkable face's room indoors or not: by how much of the room's
    // walkable floor the rooms' solid roofs.
    let indoors: Vec<bool> = rooms.as_ref().map_or_else(Vec::new, |rooms| {
        let mut floor = vec![0.0f32; area.rooms.len()];
        let mut roofed = vec![0.0f32; area.rooms.len()];
        for ((corners, _), &room) in walkable.iter().zip(&walkable_rooms) {
            let size = flat_area(corners);
            floor[room] += size;
            if edges::roofed(rooms, corners) {
                roofed[room] += size;
            }
        }
        walkable_rooms
            .iter()
            .map(|&room| edges::indoors(roofed[room], floor[room]))
            .collect()
    });

    let mut entities = Vec::new();
    let mut doorways = Vec::new();
    for (index, instance) in area.instances.iter().enumerate() {
        let (walkmeshes, origin): (Vec<(u8, Walkmesh)>, Origin) = match instance.kind {
            Kind::Door => {
                stats.doors += 1;
                let Some(model) = &instance.model else {
                    continue;
                };
                let found = (0..3u8)
                    .filter_map(|state| {
                        loader
                            .walkmesh(&format!("{model}{state}"), ResType::DWK)
                            .map(|walkmesh| (state, walkmesh))
                    })
                    .collect();
                (found, Origin::Door)
            }
            Kind::Placeable => {
                stats.placeables += 1;
                let Some(model) = &instance.model else {
                    continue;
                };
                let found = loader
                    .walkmesh(model, ResType::PWK)
                    .map(|walkmesh| (0u8, walkmesh))
                    .into_iter()
                    .collect();
                (found, Origin::Placeable)
            }
            _ => continue,
        };
        if walkmeshes.is_empty() {
            continue;
        }
        let pose = Pose::new(instance.position, instance.direction);
        let entity = index as u32;
        // The doorway's floor: the first open state that is not a sheet.
        if origin == Origin::Door
            && let Some((_, open)) = walkmeshes
                .iter()
                .find(|(state, walkmesh)| *state > 0 && !placeholder(walkmesh))
        {
            doorways.extend(
                open.faces
                    .iter()
                    .filter(|face| surfaces.walkable(face.material))
                    .map(|face| face.vertices.map(|v| pose.apply(open.vertices[v as usize]))),
            );
        }
        let mut states = 0u8;
        let mut rise = 0.0;
        let mut footprint = Vec::new();
        for (state, walkmesh) in &walkmeshes {
            if *state == 0 {
                (rise, footprint) = footprint_of(walkmesh, &pose, surfaces);
            }
            match origin {
                Origin::Door => stats.door_walkmeshes += 1,
                _ => stats.placeable_walkmeshes += 1,
            }
            if *state > 0 && placeholder(walkmesh) {
                stats.door_placeholders += 1;
                continue;
            }
            states |= 1 << state;
            for face in &walkmesh.faces {
                let corners = face
                    .vertices
                    .map(|v| pose.apply(walkmesh.vertices[v as usize]));
                let tag = Tag {
                    contents: surfaces.contents(face.material),
                    surface: collision::Surface {
                        material: face.material as u16,
                        ground: ground::of(face.material, ""),
                    },
                    entity: Some(entity),
                    states: 1 << state,
                };
                if builder.triangle(corners, tag) {
                    triangles.push(SourceTriangle {
                        corners,
                        origin,
                        material: face.material,
                    });
                } else {
                    stats.degenerate += 1;
                }
            }
        }
        // The placeable's drawn shape, in state 0 as its walkmesh is.
        let mut shape = Shape::default();
        if origin == Origin::Placeable
            && options.placeable_shapes
            && let Some(model) = instance
                .model
                .as_deref()
                .and_then(|name| loader.model(name))
        {
            let drawn = model_triangles(
                loader,
                &model,
                |p| pose.apply(p),
                PLACEABLE_SEE_THROUGH_HINT,
                &mut additive,
                &mut stats.placeable_skipped,
            );
            shape = placeable_shape(drawn, &walk_level, surfaces, (pose.sin, pose.cos));
            stats.placeable_clutter += shape.left_out;
        }
        let mut shape_triangles = 0;
        for (corners, texture, base) in &shape.solid {
            let tag = Tag {
                contents: if *base {
                    Contents::SHAPE
                } else {
                    Contents::SHAPE | Contents::NONWALK
                },
                surface: collision::Surface {
                    material: 0,
                    ground: ground::of(0, texture),
                },
                entity: Some(entity),
                states: 1,
            };
            if builder.triangle(*corners, tag) {
                shape_triangles += 1;
                stats.placeable_shape.add(corners);
                stats.placeable_bases += usize::from(*base);
                triangles.push(SourceTriangle {
                    corners: *corners,
                    origin,
                    material: 0,
                });
            } else {
                stats.degenerate += 1;
            }
        }
        // A placeable drawn keeps only the upper part of the walls round
        // its footprint, from a standing player and a plain jump's rise
        // (`edges::SLOT`) over its highest drawn point. Its shape stops a
        // walker (`sim::motion` refuses a step onto its top), and its
        // clutter lets one by. The upper part holds in some Force jumps
        // from its top (K1 unk_m43aa's flags and beds).
        let drawn = shape_triangles > 0 || shape.left_out > 0;
        let shape_top = drawn.then(|| {
            shape
                .solid
                .iter()
                .flat_map(|(corners, _, _)| corners.iter().map(|c| c[2]))
                .fold(instance.position[2], f32::max)
        });
        let mut footprint_walls = 0;
        let mut post = false;
        let mut walled = false;
        if origin == Origin::Placeable
            && let Some(walls) = &options.edge_walls
        {
            let tag = Tag {
                contents: Contents::WALK_CLIP,
                surface: collision::Surface {
                    material: 0,
                    ..Default::default()
                },
                entity: Some(entity),
                states: 1,
            };
            for wall in footprints::walls(&footprint, &walk_level, surfaces, walls) {
                post |= matches!(wall, collision::clip_file::ClipShape::Box { .. });
                let wall = match shape_top {
                    Some(top) => {
                        walled = true;
                        match above(wall, top + edges::SLOT) {
                            Some(wall) => wall,
                            None => continue,
                        }
                    }
                    None => wall,
                };
                if wall.add_to(&mut builder, tag) {
                    footprint_walls += 1;
                    triangles.extend(edges::middle(&wall).map(|corners| SourceTriangle {
                        corners,
                        origin: Origin::EdgeWall,
                        material: 0,
                    }));
                }
            }
            stats.footprint_walls += footprint_walls;
        }
        walled |= footprint_walls > 0;
        entities.push(EntityCollision {
            entity,
            kind: instance.kind,
            tag: String::from_utf8_lossy(&instance.tag).into_owned(),
            model: instance.model.clone().unwrap_or_else(String::new),
            states,
            rise,
            footprint,
            footprint_walls,
            post,
            walled,
            shape: shape_triangles,
            shape_clutter: shape.left_out,
        });
    }

    if let Some(walls) = &options.edge_walls {
        let tag = Tag::level(Contents::WALK_CLIP, 0);
        let furniture = rooms.as_ref().map(|rooms| edges::Furniture {
            rooms,
            indoors: &indoors,
        });
        for wall in edges::walls(&walkable, &walk_level, furniture, surfaces, walls) {
            if wall.add_to(&mut builder, tag) {
                stats.edge_walls += 1;
                triangles.extend(edges::middle(&wall).map(|corners| SourceTriangle {
                    corners,
                    origin: Origin::EdgeWall,
                    material: 0,
                }));
            }
        }
    }
    if let Some(clip) = clip {
        stats.clip_brushes = clip.add_to(&mut builder);
    }
    AreaCollision {
        level: builder.build(),
        walkable,
        doorways,
        triangles,
        entities,
        stats,
    }
}

/// Whether a clip file's `leave` boxes leave a walkmesh face out of the
/// level: a non-walkable face standing steep (a wall's, its normal's Z
/// under a half) with all three corners in one box. Such a face the drawn
/// room does not have stood as an invisible wall (the Sith base on Taris,
/// K1 tar_m09aa: one 6.8 m high across a console).
pub fn left_out(clip: &ClipFile, corners: &[[f32; 3]; 3], walkable: bool) -> bool {
    !walkable
        && normal(corners).is_some_and(|n| n[2].abs() < 0.5)
        && clip
            .leaves
            .iter()
            .any(|region| corners.iter().all(|&corner| region.contains(corner)))
}

/// How high a walkmesh placed at `pose` rises from its lowest corner, and
/// its footprint: its non-walkable faces that face up (a downward look
/// meets them, as KOTOR's walker's does), world space.
fn footprint_of(
    walkmesh: &Walkmesh,
    pose: &Pose,
    surfaces: &Surfaces,
) -> (f32, Vec<[[f32; 3]; 3]>) {
    let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut footprint = Vec::new();
    for face in &walkmesh.faces {
        let corners = face
            .vertices
            .map(|v| pose.apply(walkmesh.vertices[v as usize]));
        for corner in &corners {
            low = low.min(corner[2]);
            high = high.max(corner[2]);
        }
        let up =
            normal(&corners).is_some_and(|n| n[2].abs() >= collision::player::WALKABLE_NORMAL_Z);
        if up && !surfaces.walkable(face.material) {
            footprint.push(corners);
        }
    }
    ((high - low).max(0.0), footprint)
}

/// Whether an open door's walkmesh is a placeholder: no thicker than a
/// centimetre across either horizontal axis, a sheet standing edge-on to
/// the way through. KOTOR's walker is a point, which such a sheet never
/// stops; most doors' open walkmeshes are one (49 of K1's 52 door models,
/// several in the middle of the doorway), and a player's box would snag
/// on it. Left out, an open door stops nothing, as in KOTOR.
pub fn placeholder(walkmesh: &Walkmesh) -> bool {
    let mut min = [f32::INFINITY; 2];
    let mut max = [f32::NEG_INFINITY; 2];
    for v in &walkmesh.vertices {
        for i in 0..2 {
            min[i] = min[i].min(v[i]);
            max[i] = max[i].max(v[i]);
        }
    }
    (0..2).any(|i| max[i] - min[i] < 0.01)
}

/// Every drawn triangle of the area's rooms (world space) with its
/// texture (lowercase; "" for none), in the rooms' and meshes' order. A
/// see-through mesh is drawn, never met (a light beam's, a hologram's: the
/// Sith academy's beam over Uthar boxed him in, out of every player's
/// reach); KOTOR meets walkmeshes only, and the walkmesh's edges keep
/// bodies in. One drawn translucent by its alpha is see-through too
/// (Khoonda's hall: the skylights' light shafts, `danfx_clouds` at alpha
/// 0.6, stood the player in a box at the battle's start). So is one whose
/// texture adds its light to what lies behind (TXI `blending additive`):
/// Korriban's secret tomb hangs a cloud of light (`beam21`, `korfx_clouds`)
/// along the minefield in Dxun's vision, which stood the player off every
/// mine.
fn drawn_triangles(
    loader: &Loader,
    area: &AreaIr,
    additive: &mut BTreeMap<String, bool>,
    stats: &mut BuildStats,
) -> Vec<([[f32; 3]; 3], String)> {
    let mut drawn = Vec::new();
    for room in &area.rooms {
        let Some(model) = loader.model(&room.model) else {
            stats.rooms_without_model.push(room.model.clone());
            continue;
        };
        let place = |p: [f32; 3]| -> [f32; 3] { std::array::from_fn(|r| p[r] + room.position[r]) };
        drawn.extend(model_triangles(
            loader,
            &model,
            place,
            ROOM_SEE_THROUGH_HINT,
            additive,
            &mut stats.render_skipped,
        ));
    }
    drawn
}

/// Every drawn triangle of `model` at its rest pose, moved by `place` from
/// the model's space into the world, with its texture (lowercase; "" for
/// none), in the meshes' order. See-through meshes are left out, and
/// counted in `skipped` ([`drawn_triangles`] says which): those drawn
/// translucent by their alpha, those whose texture blends additively, and
/// those whose transparency hint is `see_through_hint` or more. `additive`
/// keeps what each texture's blending was found to be.
fn model_triangles(
    loader: &Loader,
    model: &ModelIr,
    place: impl Fn([f32; 3]) -> [f32; 3],
    see_through_hint: u32,
    additive: &mut BTreeMap<String, bool>,
    skipped: &mut BTreeMap<String, usize>,
) -> Vec<([[f32; 3]; 3], String)> {
    let mut drawn = Vec::new();
    for mesh in &model.meshes {
        let world = &model.nodes[mesh.node].world;
        let positions: Vec<[f32; 3]> = mesh
            .positions
            .iter()
            .map(|&p| {
                place(std::array::from_fn(|r| {
                    world[r][0] * p[0] + world[r][1] * p[1] + world[r][2] * p[2] + world[r][3]
                }))
            })
            .collect();
        let additive = mesh.texture.as_deref().is_some_and(|texture| {
            *additive
                .entry(texture.to_string())
                .or_insert_with(|| loader.additive(texture))
        });
        if mesh.transparency_hint >= see_through_hint || mesh.alpha < 1.0 || additive {
            *skipped.entry("see-through".into()).or_insert(0) += mesh.indices.len() / 3;
            continue;
        }
        let texture = mesh.texture.as_deref().unwrap_or("").to_ascii_lowercase();
        for index in mesh.indices.as_chunks::<3>().0 {
            let corner = |k: usize| positions.get(index[k] as usize).copied();
            let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else {
                *skipped.entry("index out of range".into()).or_insert(0) += 1;
                continue;
            };
            drawn.push(([a, b, c], texture.clone()));
        }
    }
    drawn
}

/// Whether a drawn triangle is clutter on the floor, which KOTOR's walker
/// walks through: some part of it over walkable floor, and none more than
/// a step from the walkmesh right under it or, where the walkmesh has a
/// hole (under a console), more than a step over that floor (the Ebon
/// Hawk's pipes, a console's sloped skirt and its sides reaching past the
/// walkmesh's edge onto the floor). Solid, the player's box rode up onto
/// every pipe and was shoved up a skirt's slope, or the slanted top of its
/// side; left out, the walkmesh is what bodies meet there (the floor, a
/// console's own non-walkable faces), and the edge walls keep them off the
/// floor's edge. Floor drawn flat at the floor's height is kept: nothing
/// to snag on, and what lies or rolls there has it under it.
fn floor_clutter(walkmesh: &Level, surfaces: &Surfaces, corners: &[[f32; 3]; 3]) -> bool {
    use collision::CollisionWorld;
    let step = collision::player::STEP_HEIGHT * collision::UNIT;
    let middle: [f32; 3] =
        std::array::from_fn(|i| (corners[0][i] + corners[1][i] + corners[2][i]) / 3.0);
    // The corners drawn in a centimetre, so a corner on a seam between two
    // faces still finds one; and the middle.
    let inward = |p: [f32; 3]| -> [f32; 3] {
        let d: [f32; 3] = std::array::from_fn(|i| middle[i] - p[i]);
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let t = if length > CLUTTER_INSET {
            CLUTTER_INSET / length
        } else {
            1.0
        };
        std::array::from_fn(|i| p[i] + d[i] * t)
    };
    let points = [
        inward(corners[0]),
        inward(corners[1]),
        inward(corners[2]),
        middle,
    ];
    // The highest walkable floor under a point, and the highest point over
    // a hole.
    let mut floor = f32::NEG_INFINITY;
    let mut over_hole = f32::NEG_INFINITY;
    for p in &points {
        // From a step over the point (a corner under the floor, a pipe's
        // lower half, still finds the floor over it) down to a step under it.
        let from = [p[0], p[1], p[2] + step];
        let to = [p[0], p[1], p[2] - step];
        let hit = walkmesh.trace(from, to, &Extents::POINT, Contents::SOLID);
        if hit.start_solid {
            return false;
        }
        if hit.fraction >= 1.0 {
            over_hole = over_hole.max(p[2]);
        } else if hit.plane.normal[2] >= collision::player::WALKABLE_NORMAL_Z
            && surfaces.walkable(u32::from(hit.surface.material))
        {
            floor = floor.max(hit.end[2]);
        }
    }
    let flat_on_floor = normal(corners).is_some_and(|n| n[2] >= FLAT)
        && corners.iter().all(|c| (c[2] - floor).abs() <= SAME_SURFACE);
    floor.is_finite() && over_hole <= floor + step && !flat_on_floor
}

/// A placeable's drawn triangles as they go into the level
/// ([`placeable_shape`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shape {
    /// The triangles kept, world space, with their texture and whether they
    /// are floor a walker steps onto (part of a low, wide base).
    pub solid: Vec<([[f32; 3]; 3], String, bool)>,
    /// Triangles of small, low parts left out, as floor clutter is.
    pub left_out: usize,
}

/// A placeable's drawn triangles (`drawn`, world space), sorted by how a
/// body meets them. Heights are over the walkable floor under a triangle's
/// or a part's lowest corner ([`floor_under`]), and sizes are along the
/// placeable's own axes (`turn` being its facing's sine and cosine).
///
/// - A triangle is low when it rises no more than a step. Low triangles
///   that touch make a base when their up-facing ones span the player's
///   box both ways. A base is floor: a walker steps onto it and off it, as
///   onto the ring at a cage's foot, whose posts rise from it.
/// - Triangles that touch, whatever their height, are one part. A part no
///   higher than [`CLUTTER_HEIGHT`] and smaller from above than
///   [`CLUTTER_AREA`] is left out, as floor clutter is: each piece of a
///   pile of parts.
/// - Every other triangle is solid and `NONWALK`. A walker is stopped by
///   it, and only a jump puts a body on top of it (a footlocker, a
///   cylinder, an environment suit).
pub fn placeable_shape(
    drawn: Vec<([[f32; 3]; 3], String)>,
    walkmesh: &Level,
    surfaces: &Surfaces,
    turn: (f32, f32),
) -> Shape {
    let step = collision::player::STEP_HEIGHT * collision::UNIT;
    let width = 2.0 * collision::player::HALF_WIDTH * collision::UNIT;
    let lowest = |corners: &[[f32; 3]; 3]| {
        *corners
            .iter()
            .min_by(|a, b| a[2].total_cmp(&b[2]))
            .expect("three corners")
    };
    let highest = |corners: &[[f32; 3]; 3]| {
        corners
            .iter()
            .map(|c| c[2])
            .fold(f32::NEG_INFINITY, f32::max)
    };
    let low: Vec<bool> = drawn
        .iter()
        .map(|(corners, _)| {
            floor_under(walkmesh, surfaces, lowest(corners))
                .is_some_and(|floor| highest(corners) - floor <= step)
        })
        .collect();
    let bases = touching_parts(&drawn, &low);
    let parts = touching_parts(&drawn, &vec![true; drawn.len()]);
    let mut base_tops: BTreeMap<usize, Span> = BTreeMap::new();
    let mut part_sizes: BTreeMap<usize, (Span, [f32; 3], f32)> = BTreeMap::new();
    for (index, (corners, _)) in drawn.iter().enumerate() {
        let up = normal(corners).is_some_and(|n| n[2] >= collision::player::WALKABLE_NORMAL_Z);
        if low[index] && up {
            base_tops
                .entry(bases[index])
                .or_insert_with(|| Span::new(turn))
                .grow(corners);
        }
        let (span, low_corner, high) = part_sizes.entry(parts[index]).or_insert_with(|| {
            (
                Span::new(turn),
                [0.0, 0.0, f32::INFINITY],
                f32::NEG_INFINITY,
            )
        });
        span.grow(corners);
        let corner = lowest(corners);
        if corner[2] < low_corner[2] {
            *low_corner = corner;
        }
        *high = high.max(highest(corners));
    }
    let base = |index: usize| {
        low[index]
            && base_tops
                .get(&bases[index])
                .is_some_and(|top| top.size().iter().all(|&side| side >= width))
    };
    let clutter: BTreeMap<usize, bool> = part_sizes
        .iter()
        .map(|(&part, (span, low_corner, high))| {
            let [x, y] = span.size();
            let short = floor_under(walkmesh, surfaces, *low_corner)
                .is_some_and(|floor| high - floor <= CLUTTER_HEIGHT);
            (part, short && x * y < CLUTTER_AREA)
        })
        .collect();
    let mut shape = Shape::default();
    for (index, (corners, texture)) in drawn.into_iter().enumerate() {
        if base(index) {
            shape.solid.push((corners, texture, true));
        } else if clutter[&parts[index]] {
            shape.left_out += 1;
        } else {
            shape.solid.push((corners, texture, false));
        }
    }
    shape
}

/// The extent of triangles along a placeable's two axes.
struct Span {
    /// The placeable's facing: sine and cosine.
    turn: (f32, f32),
    /// The lowest and highest place along each axis.
    range: [[f32; 2]; 2],
}

impl Span {
    fn new(turn: (f32, f32)) -> Span {
        Span {
            turn,
            range: [[f32::INFINITY, f32::NEG_INFINITY]; 2],
        }
    }

    fn grow(&mut self, corners: &[[f32; 3]; 3]) {
        let (sin, cos) = self.turn;
        for c in corners {
            for (axis, at) in [c[0] * cos + c[1] * sin, c[1] * cos - c[0] * sin]
                .into_iter()
                .enumerate()
            {
                let range = &mut self.range[axis];
                *range = [range[0].min(at), range[1].max(at)];
            }
        }
    }

    fn size(&self) -> [f32; 2] {
        self.range.map(|[low, high]| (high - low).max(0.0))
    }
}

/// The highest part of a placeable left out as clutter can stand over the
/// floor: two steps. The tallest piece of a pile of parts in the Sith base
/// on Taris stands 0.63 m; a plasteel cylinder there stands 0.83 m.
const CLUTTER_HEIGHT: f32 = 2.0 * collision::player::STEP_HEIGHT * collision::UNIT;

/// A part of a placeable left out as clutter covers less than this from
/// above: half of the player's box. The largest piece of a pile of parts
/// covers 0.15 m², an environment suit's smaller half 0.32 m².
const CLUTTER_AREA: f32 = 2.0
    * collision::player::HALF_WIDTH
    * collision::player::HALF_WIDTH
    * collision::UNIT
    * collision::UNIT;

/// The walkable floor under `p`, from a step over it down to
/// [`CLUTTER_HEIGHT`] under it (a part's lowest corner may float over the
/// floor).
fn floor_under(walkmesh: &Level, surfaces: &Surfaces, p: [f32; 3]) -> Option<f32> {
    use collision::CollisionWorld;
    let step = collision::player::STEP_HEIGHT * collision::UNIT;
    let hit = walkmesh.trace(
        [p[0], p[1], p[2] + step],
        [p[0], p[1], p[2] - CLUTTER_HEIGHT],
        &Extents::POINT,
        Contents::SOLID,
    );
    (hit.fraction < 1.0
        && !hit.start_solid
        && hit.plane.normal[2] >= collision::player::WALKABLE_NORMAL_Z
        && surfaces.walkable(u32::from(hit.surface.material)))
    .then_some(hit.end[2])
}

/// For each triangle of `drawn` marked in `members`, the part it is in:
/// the lowest index of the members it touches through shared corners
/// (welded to [`edges::WELD`]), directly or through others. A triangle not
/// marked is a part of its own.
fn touching_parts(drawn: &[([[f32; 3]; 3], String)], members: &[bool]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..drawn.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut first: BTreeMap<[i64; 3], usize> = BTreeMap::new();
    for (index, (corners, _)) in drawn.iter().enumerate() {
        if !members[index] {
            continue;
        }
        for corner in corners {
            let key = corner.map(|v| (v / edges::WELD).round() as i64);
            let other = *first.entry(key).or_insert(index);
            let (a, b) = (root(&mut parent, index), root(&mut parent, other));
            parent[a.max(b)] = a.min(b);
        }
    }
    (0..drawn.len()).map(|i| root(&mut parent, i)).collect()
}

/// A room's mesh with any transparency hint is see-through
/// ([`drawn_triangles`]).
const ROOM_SEE_THROUGH_HINT: u32 = 1;

/// A placeable's mesh is see-through from a transparency hint of 2: the
/// renderer blends a mesh with a hint above 1 and draws one with hint 1 by
/// its texture's alpha (`game::material::surface`). The base and posts of
/// the cages in the Sith base on Taris carry hint 1 and are drawn solid;
/// their force fields carry 2, or alpha 0.35.
const PLACEABLE_SEE_THROUGH_HINT: u32 = 2;

/// The part of a footprint wall from `bottom` up; `None` when none is
/// left.
fn above(
    wall: collision::clip_file::ClipShape,
    bottom: f32,
) -> Option<collision::clip_file::ClipShape> {
    use collision::clip_file::ClipShape;
    match wall {
        ClipShape::Wall {
            from,
            to,
            z,
            thickness,
        } => (z[1] > bottom).then_some(ClipShape::Wall {
            from,
            to,
            z: [z[0].max(bottom), z[1]],
            thickness,
        }),
        ClipShape::Box { min, max } => (max[2] > bottom).then_some(ClipShape::Box {
            min: [min[0], min[1], min[2].max(bottom)],
            max,
        }),
    }
}

/// A drawn triangle whose normal is this near straight up is flat.
const FLAT: f32 = 0.99;

/// How far in from a drawn triangle's corners [`floor_clutter`] looks for
/// the floor under it, metres.
const CLUTTER_INSET: f32 = 0.01;

/// The material of the walkmesh face a drawn triangle lies on.
fn classify(walkmesh: &Level, corners: &[[f32; 3]; 3]) -> Option<u32> {
    lies_on(walkmesh, corners).map(|surface| u32::from(surface.material))
}

/// The surface of `level` a triangle lies on: a point trace along its
/// normal through its centroid, within [`SAME_SURFACE`], that meets a face
/// turned the same way.
fn lies_on(level: &Level, corners: &[[f32; 3]; 3]) -> Option<collision::Surface> {
    use collision::CollisionWorld;
    let n = normal(corners)?;
    let c: [f32; 3] =
        std::array::from_fn(|i| (corners[0][i] + corners[1][i] + corners[2][i]) / 3.0);
    let from: [f32; 3] = std::array::from_fn(|i| c[i] + n[i] * SAME_SURFACE);
    let to: [f32; 3] = std::array::from_fn(|i| c[i] - n[i] * SAME_SURFACE);
    let trace = level.trace(from, to, &Extents::POINT, Contents::SOLID);
    let facing = trace.plane.normal;
    let same = (facing[0] * n[0] + facing[1] * n[1] + facing[2] * n[2]).abs() > 0.9;
    (trace.fraction < 1.0 && same).then_some(trace.surface)
}
