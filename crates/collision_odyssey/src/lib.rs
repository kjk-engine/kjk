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
//!   ceilings the walkmesh does not cover have material 0.
//! - Doors: the three door walkmeshes (`<model>0/1/2.dwk`: closed, open one
//!   way, open the other) as brushes owned by the door, solid in state 0,
//!   1 and 2 respectively; an open one that is only a placeholder sheet is
//!   left out ([`placeholder`]). Placeables: `<model>.pwk`, solid in state 0 (in
//!   place). Both at the instance's position and bearing; the entity is the
//!   instance's index in the area's instance list.
//! - A walkmesh face's contents come from `surfacemat.2da`: see
//!   [`Surfaces::contents`].
//! - Walls along the walkmesh's open edges, as `WALK_CLIP` ([`edges`]),
//!   and round each placeable's footprint (its walkmesh's non-walkable
//!   faces that face up), as `WALK_CLIP` owned by the placeable, solid in
//!   state 0 ([`footprints`]).
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
    /// Walls along the walkmesh's open edges ([`edges`]).
    pub edge_walls: usize,
    /// Walls round placeables' footprints ([`footprints`]).
    pub footprint_walls: usize,
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
    /// The walls round its footprint ([`footprints`]).
    pub footprint_walls: usize,
    /// Its footprint too small for walls round it, a post at its middle
    /// instead ([`footprints::walls`]).
    pub post: bool,
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
            .filter(|entity| entity.footprint_walls > 0 && !entity.post)
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

/// How far a drawn triangle may sit from a walkmesh face and still be the
/// same surface, metres along its normal.
const SAME_SURFACE: f32 = 0.1;

/// How an area's collision is built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildOptions {
    /// Walls along the walkmesh's open edges; `None` for none.
    pub edge_walls: Option<EdgeWalls>,
}

impl Default for BuildOptions {
    fn default() -> BuildOptions {
        BuildOptions {
            edge_walls: Some(EdgeWalls {
                height: edges::HEIGHT,
            }),
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
    for room in &area.rooms {
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
            walk_builder.triangle(corners, Tag::level(Contents::SOLID, face.material as u16));
        }
    }
    let walk_level = walk_builder.build();
    for &(corners, material) in &walk_faces {
        if surfaces.walkable(material) {
            walkable.push((corners, material));
        }
        let tag = Tag::level(surfaces.contents(material), material as u16);
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

    let mut additive: BTreeMap<String, bool> = BTreeMap::new();
    for room in &area.rooms {
        let Some(model) = loader.model(&room.model) else {
            stats.rooms_without_model.push(room.model.clone());
            continue;
        };
        for mesh in &model.meshes {
            let world = &model.nodes[mesh.node].world;
            let place = |p: [f32; 3]| -> [f32; 3] {
                std::array::from_fn(|r| {
                    world[r][0] * p[0]
                        + world[r][1] * p[1]
                        + world[r][2] * p[2]
                        + world[r][3]
                        + room.position[r]
                })
            };
            let positions: Vec<[f32; 3]> = mesh.positions.iter().map(|&p| place(p)).collect();
            // A see-through mesh is drawn, never met (a light beam's,
            // a hologram's: the Sith academy's beam over Uthar boxed him
            // in, out of every player's reach); KOTOR meets walkmeshes
            // only, and the walkmesh's edges keep bodies in. One drawn
            // translucent by its alpha is see-through too (Khoonda's hall:
            // the skylights' light shafts, `danfx_clouds` at alpha 0.6,
            // stood the player in a box at the battle's start).
            // So is one whose texture adds its light to what lies behind
            // (TXI `blending additive`): Korriban's secret tomb hangs a
            // cloud of light (`beam21`, `korfx_clouds`) along the minefield
            // in Dxun's vision, which stood the player off every mine.
            let additive = mesh.texture.as_deref().is_some_and(|texture| {
                *additive
                    .entry(texture.to_string())
                    .or_insert_with(|| loader.additive(texture))
            });
            if mesh.transparency_hint != 0 || mesh.alpha < 1.0 || additive {
                *stats
                    .render_skipped
                    .entry("see-through".into())
                    .or_insert(0) += mesh.indices.len() / 3;
                continue;
            }
            for index in mesh.indices.as_chunks::<3>().0 {
                let corner = |k: usize| positions.get(index[k] as usize).copied();
                let (Some(a), Some(b), Some(c)) = (corner(0), corner(1), corner(2)) else {
                    *stats
                        .render_skipped
                        .entry("index out of range".into())
                        .or_insert(0) += 1;
                    continue;
                };
                let corners = [a, b, c];
                let material = classify(&walk_level, &corners);
                if material.is_some() {
                    stats.render_classified += 1;
                }
                let material = material.unwrap_or(0);
                let tag = Tag::level(surfaces.contents(material), material as u16);
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
        }
    }

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
        let mut footprint_walls = 0;
        let mut post = false;
        if origin == Origin::Placeable
            && let Some(walls) = &options.edge_walls
        {
            let tag = Tag {
                contents: Contents::WALK_CLIP,
                surface: collision::Surface { material: 0 },
                entity: Some(entity),
                states: 1,
            };
            for wall in footprints::walls(&footprint, &walk_level, surfaces, walls) {
                post |= matches!(wall, collision::clip_file::ClipShape::Box { .. });
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
        });
    }

    if let Some(walls) = &options.edge_walls {
        let tag = Tag::level(Contents::WALK_CLIP, 0);
        for wall in edges::walls(&walkable, &walk_level, surfaces, walls) {
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

/// The material of the walkmesh face a drawn triangle lies on: a point
/// trace along the triangle's normal through its centroid, within
/// [`SAME_SURFACE`], that meets a face turned the same way.
fn classify(walkmesh: &Level, corners: &[[f32; 3]; 3]) -> Option<u32> {
    use collision::CollisionWorld;
    let n = normal(corners)?;
    let c: [f32; 3] =
        std::array::from_fn(|i| (corners[0][i] + corners[1][i] + corners[2][i]) / 3.0);
    let from: [f32; 3] = std::array::from_fn(|i| c[i] + n[i] * SAME_SURFACE);
    let to: [f32; 3] = std::array::from_fn(|i| c[i] - n[i] * SAME_SURFACE);
    let trace = walkmesh.trace(from, to, &Extents::POINT, Contents::SOLID);
    let facing = trace.plane.normal;
    let same = (facing[0] * n[0] + facing[1] * n[1] + facing[2] * n[2]).abs() > 0.9;
    (trace.fraction < 1.0 && same).then_some(u32::from(trace.surface.material))
}
