//! A Jedi Academy level (`fmt_ja::bsp`) as a model the scene draws, lit by
//! the level's own stored light.
//!
//! The level becomes one model of one node, in metres (`collision::UNIT`),
//! with a mesh for each shader the level draws with. Every mesh is lit
//! through one texture, the level's light atlas ([`Level::light`]), by its
//! second texture coordinates.
//!
//! - **A surface with a lightmap** keeps its place on its lightmap. The
//!   atlas holds the level's lightmaps side by side, [`PAGES_ACROSS`] to a
//!   row.
//! - **A surface lit by its vertices** has no lightmap, or one kept as a
//!   picture beside the level, which is not read yet. Each of its
//!   triangles gets a block of two by two texels under the lightmaps: one
//!   texel for each corner's colour, and a fourth that continues them
//!   (the second plus the third less the first). The sampler's blend of
//!   the four is then the blend of the three colours across the triangle.
//! - **A curved surface** is a grid of control points, every three by three
//!   of them a patch (a quadratic Bézier surface). It is cut into flat
//!   strips, as many as keep the strips within [`PATCH_TOLERANCE`] of the
//!   curve.
//!
//! - **A static model** (`misc_model_static`) is a model of its own that the
//!   level places by an entity, with an origin, angles and a scale. The
//!   level's file does not hold it. It is lit as the game lights it: by the
//!   level's light grid at its origin (`light_grid`), each vertex by how
//!   much it faces the directed light. Its triangles are then lit by their
//!   vertices, as above. The light is read one unit and the entity's
//!   `zoffset` above the origin.
//!
//! Left out: the sky's surfaces, surfaces a shader marks as never drawn,
//! volumes of fog, and lens flares. A moving part (a door, a lift) is drawn
//! where it rests. Only the level's steady light is read, not its three
//! further light styles.

use std::collections::BTreeMap;

use asset_odyssey::anim::{AnimationIr, NodeTrack};
use asset_odyssey::model::{IDENTITY, MeshIr, ModelIr, NodeIr};
use asset_odyssey::texture::{Encoding, TextureFacts, TextureIr};
use fmt_ja::bsp::{
    Bsp, CONTENTS_FOG, Entity, LIGHTMAP_SIDE, SURFACE_NODRAW, SURFACE_SKY, Surface, SurfaceKind,
    Vertex,
};
use fmt_ja::md3::Md3;

use crate::light_grid::{ModelLight, light_at};

/// Lightmaps side by side in a row of the light atlas.
pub const PAGES_ACROSS: usize = 8;

/// How far a curved surface's flat strips may lie from the curve, in units.
/// Jedi Academy's renderer cuts a curve by its distance from the viewer, down
/// to 4 units (`r_subdivisions`). A model that is cut once is cut finer.
pub const PATCH_TOLERANCE: f32 = 2.0;

/// The most strips one patch is cut into along a side.
const PATCH_STRIPS: usize = 8;

/// The side of a vertex-lit triangle's block in the atlas, in texels.
const BLOCK: usize = 2;

/// A level ready for the scene.
pub struct Level {
    pub model: ModelIr,
    /// The texture every mesh names as its lightmap ([`light_name`]).
    pub light: TextureIr,
    /// Where a player starts (metres, at the feet) and the way they face
    /// (x, y), from the level's `info_player_start`, else its first
    /// `info_player_deathmatch`.
    pub start: Option<([f32; 3], [f32; 2])>,
    /// The lightmaps kept beside the level that the meshes name, by
    /// number. Each is the picture `beside_path`, and is named among the
    /// scene's textures as `beside_name`.
    pub beside: Vec<u32>,
    /// The shaders of the level's sky surfaces, which are not drawn: the
    /// sky is drawn as a box round the viewer (`sky`).
    pub sky: Vec<String>,
    /// The level's doors (`movers::doors`), each a model of its own that
    /// moves, in the order of their numbers. A level drawn by its rooms
    /// keeps its doors in its own model, and has none here.
    pub doors: Vec<DoorModel>,
    pub tally: Tally,
}

/// A door as the scene draws it: its model, a node for each of its parts,
/// and the clips that open and close it.
#[derive(Debug, Clone)]
pub struct DoorModel {
    pub model: ModelIr,
    pub opening: AnimationIr,
    pub closing: AnimationIr,
}

/// The name of door `door`'s model, of the level `level`.
pub fn door_name(level: &str, door: usize) -> String {
    format!("{level}#door{door}")
}

/// A clip that moves each part of a door from `from` to `to` of its way
/// in `seconds`.
fn door_clip(
    name: &str,
    model: &str,
    door: &crate::movers::Mover,
    (from, to): (f32, f32),
) -> AnimationIr {
    let seconds = door.seconds.max(0.01);
    AnimationIr {
        name: name.to_string(),
        length: seconds,
        transition: 0.0,
        root: None,
        tracks: (door.parts.iter().enumerate())
            .map(|(part, (_, moves))| {
                let at = |share: f32| moves.map(|part| part * share * collision::UNIT);
                NodeTrack {
                    node: format!("{model}_{part}"),
                    positions: vec![(0.0, at(from)), (seconds, at(to))],
                    orientations: Vec::new(),
                }
            })
            .collect(),
        settings: Vec::new(),
        events: Vec::new(),
    }
}

/// The picture of lightmap `page` kept beside the level at `path`
/// (`maps/duel_carbon.bsp` keeps `maps/duel_carbon/lm_0004.tga`). A newer
/// compiler keeps a level's lightmaps so when they are larger than the
/// file's own 128 by 128 texels.
pub fn beside_path(path: &str, page: u32) -> String {
    format!("{}/lm_{page:04}.tga", path.trim_end_matches(".bsp"))
}

/// The name of lightmap `page` kept beside the level `level` among the
/// scene's textures.
pub fn beside_name(level: &str, page: u32) -> String {
    format!("{level}#lm_{page:04}")
}

/// What a level's conversion drew and left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    /// Surfaces drawn with a lightmap, and lit by their vertices.
    pub lightmapped: usize,
    pub vertex_lit: usize,
    /// Surfaces lit by a lightmap kept as a picture beside the level
    /// (`beside_path`), counted apart from `lightmapped`. A level drawn by
    /// its rooms lights them by their vertices instead, as `vertex_lit`.
    pub kept_beside: usize,
    /// Curved surfaces among them.
    pub patches: usize,
    pub triangles: usize,
    /// Surfaces left out: the sky, never drawn, fog, flares and kinds not
    /// known.
    pub sky: usize,
    pub not_drawn: usize,
    pub fog: usize,
    pub flares: usize,
    pub unknown: usize,
    /// Static models placed, and their triangles (counted in `triangles`
    /// too).
    pub static_models: usize,
    pub static_triangles: usize,
    /// Static models left out: the entity names no model or no origin, or
    /// the model was not found.
    pub static_missing: usize,
    /// The pieces drawn in each room, of a level painted by its rooms
    /// (`Paint::Rooms`); empty otherwise.
    pub rooms: Vec<usize>,
}

/// The name of a level's light atlas among the scene's textures.
pub fn light_name(level: &str) -> String {
    format!("{level}#light")
}

/// One mesh being gathered: the surfaces of one shader.
#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uv1: Vec<[f32; 2]>,
    /// In atlas texels, turned into coordinates once the atlas has its size.
    uv2: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Batch {
    /// A vertex, lit at `texel` of the atlas.
    fn push(&mut self, corner: &Corner, texel: [f32; 2]) -> u32 {
        self.positions
            .push(corner.position.map(|part| part * collision::UNIT));
        self.normals.push(corner.normal);
        self.uv1.push(corner.st);
        self.uv2.push(texel);
        (self.positions.len() - 1) as u32
    }
}

/// A mesh being gathered: the door and part it belongs to (none for the
/// level's own), its shader, and the lightmap kept beside the level it is
/// lit by (none for the light atlas).
type Key = (Option<(usize, usize)>, String, Option<u32>);

/// A vertex in the level: its place (units), its normal, its texture
/// coordinates and its light as a colour.
struct Corner {
    position: [f32; 3],
    normal: [f32; 3],
    st: [f32; 2],
    color: [u8; 3],
}

impl Corner {
    fn of(vertex: &Vertex, offset: [f32; 3]) -> Corner {
        let [r, g, b, _] = vertex.color[0];
        Corner {
            position: std::array::from_fn(|axis| vertex.position[axis] + offset[axis]),
            normal: vertex.normal,
            st: vertex.st,
            color: [r, g, b],
        }
    }
}

/// The blocks of the light atlas that hold the vertex-lit triangles'
/// colours, under the lightmaps.
struct Blocks {
    /// The atlas row the first block starts at.
    top: usize,
    /// Blocks in a row of the atlas.
    across: usize,
    /// Each triangle's four texels, in the order it was met.
    texels: Vec<[[u8; 3]; 4]>,
}

impl Blocks {
    /// A triangle lit by its corners' colours, added to `batch`.
    fn triangle(&mut self, batch: &mut Batch, corners: [Corner; 3]) {
        let [a, b, c] = [corners[0].color, corners[1].color, corners[2].color];
        let fourth = std::array::from_fn(|channel| {
            (i32::from(b[channel]) + i32::from(c[channel]) - i32::from(a[channel])).clamp(0, 255)
                as u8
        });
        let block = self.texels.len();
        self.texels.push([a, b, c, fourth]);
        let left = (block % self.across * BLOCK) as f32 + 0.5;
        let top = (self.top + block / self.across * BLOCK) as f32 + 0.5;
        let texels = [[left, top], [left + 1.0, top], [left, top + 1.0]];
        for (corner, texel) in corners.iter().zip(texels) {
            let index = batch.push(corner, texel);
            batch.indices.push(index);
        }
    }
}

/// A surface's vertices and triangles, a curved one cut into strips. The
/// triangles wind counter-clockwise seen from the front, as the scene
/// draws them.
pub(crate) fn triangles(bsp: &Bsp, surface: &Surface) -> (Vec<Vertex>, Vec<[u32; 3]>) {
    let vertices = bsp.surface_vertices(surface);
    if surface.kind == SurfaceKind::Patch {
        return patch(vertices, surface.patch.0 as usize, surface.patch.1 as usize);
    }
    // The file winds a triangle clockwise seen from its front, as Quake 3
    // does.
    let triangles = bsp
        .surface_indices(surface)
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| [t[0], t[2], t[1]])
        .collect();
    (vertices.to_vec(), triangles)
}

/// The numbers of a vertex that a curve blends.
const BLENDED: usize = 14;

fn numbers(vertex: &Vertex) -> [f32; BLENDED] {
    let [p, n] = [vertex.position, vertex.normal];
    let [st, light] = [vertex.st, vertex.lightmap_st[0]];
    let c = vertex.color[0].map(f32::from);
    [
        p[0], p[1], p[2], st[0], st[1], light[0], light[1], n[0], n[1], n[2], c[0], c[1], c[2],
        c[3],
    ]
}

fn vertex_of(v: [f32; BLENDED], like: &Vertex) -> Vertex {
    let length = (v[7] * v[7] + v[8] * v[8] + v[9] * v[9]).sqrt();
    let normal = if length > 0.0 {
        [v[7] / length, v[8] / length, v[9] / length]
    } else {
        like.normal
    };
    let mut vertex = *like;
    vertex.position = [v[0], v[1], v[2]];
    vertex.st = [v[3], v[4]];
    vertex.lightmap_st[0] = [v[5], v[6]];
    vertex.normal = normal;
    vertex.color[0] = [v[10], v[11], v[12], v[13]].map(|c| c.round().clamp(0.0, 255.0) as u8);
    vertex
}

/// A quadratic Bézier curve through three control points, at `t`.
fn curve(a: [f32; BLENDED], b: [f32; BLENDED], c: [f32; BLENDED], t: f32) -> [f32; BLENDED] {
    let (wa, wb, wc) = ((1.0 - t) * (1.0 - t), 2.0 * t * (1.0 - t), t * t);
    std::array::from_fn(|i| a[i] * wa + b[i] * wb + c[i] * wc)
}

/// The strips that keep a curve through `a`, `b`, `c` within the tolerance.
/// A quadratic curve cut into `n` chords lies at most a quarter of
/// |a − 2b + c|, over n², from them.
fn strips(a: &Vertex, b: &Vertex, c: &Vertex) -> usize {
    let bend: f32 = (0..3)
        .map(|axis| a.position[axis] - 2.0 * b.position[axis] + c.position[axis])
        .map(|d| d * d)
        .sum::<f32>()
        .sqrt();
    ((bend / (4.0 * PATCH_TOLERANCE)).sqrt().ceil() as usize).clamp(1, PATCH_STRIPS)
}

/// A curved surface of `width` by `height` control points, cut into
/// triangles. Every patch of it is cut into the same number of strips each
/// way, so the patches meet without gaps.
fn patch(control: &[Vertex], width: usize, height: usize) -> (Vec<Vertex>, Vec<[u32; 3]>) {
    let at = |column: usize, row: usize| &control[row * width + column];
    let (across, down) = ((width - 1) / 2, (height - 1) / 2);
    let mut cut = (1, 1);
    for row in 0..height {
        for p in 0..across {
            let n = strips(at(2 * p, row), at(2 * p + 1, row), at(2 * p + 2, row));
            cut.0 = cut.0.max(n);
        }
    }
    for column in 0..width {
        for p in 0..down {
            let n = strips(
                at(column, 2 * p),
                at(column, 2 * p + 1),
                at(column, 2 * p + 2),
            );
            cut.1 = cut.1.max(n);
        }
    }
    // A point of the cut grid: the patch it is in and how far along it.
    let along = |index: usize, patches: usize, cut: usize| {
        let patch = (index / cut).min(patches - 1);
        (patch, (index - patch * cut) as f32 / cut as f32)
    };
    let (columns, rows) = (across * cut.0 + 1, down * cut.1 + 1);
    let mut vertices = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        let (py, ty) = along(row, down, cut.1);
        for column in 0..columns {
            let (px, tx) = along(column, across, cut.0);
            let [a, b, c] = [0, 1, 2].map(|k| {
                let [l, m, r] = [0, 1, 2].map(|i| numbers(at(2 * px + i, 2 * py + k)));
                curve(l, m, r, tx)
            });
            vertices.push(vertex_of(curve(a, b, c, ty), at(2 * px, 2 * py)));
        }
    }
    let mut triangles = Vec::with_capacity((columns - 1) * (rows - 1) * 2);
    for row in 0..rows - 1 {
        for column in 0..columns - 1 {
            let corner = (row * columns + column) as u32;
            let (right, below) = (corner + 1, corner + columns as u32);
            for triangle in [[corner, below, right], [right, below, below + 1]] {
                triangles.push(facing(&vertices, triangle));
            }
        }
    }
    (vertices, triangles)
}

/// `triangle` wound counter-clockwise seen from the side its vertices'
/// normals point to.
fn facing(vertices: &[Vertex], triangle: [u32; 3]) -> [u32; 3] {
    let [a, b, c] = triangle.map(|index| &vertices[index as usize]);
    let edge = |to: &Vertex| -> [f32; 3] {
        std::array::from_fn(|axis| to.position[axis] - a.position[axis])
    };
    let (u, v) = (edge(b), edge(c));
    let face = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let toward: f32 = (0..3)
        .map(|axis| face[axis] * (a.normal[axis] + b.normal[axis] + c.normal[axis]))
        .sum();
    if toward < 0.0 {
        [triangle[0], triangle[2], triangle[1]]
    } else {
        triangle
    }
}

/// Where each moving part rests: the `origin` of the entity that stands
/// for it. A part built round an origin keeps its vertices relative to it.
pub(crate) fn rests(bsp: &Bsp) -> BTreeMap<usize, [f32; 3]> {
    bsp.entities
        .iter()
        .filter_map(|entity| Some((entity.model()?, entity.vector("origin")?)))
        .collect()
}

fn start(bsp: &Bsp) -> Option<([f32; 3], [f32; 2])> {
    let of = |class: &str| bsp.entities.iter().find(|entity| entity.class() == class);
    let entity = of("info_player_start").or_else(|| of("info_player_deathmatch"))?;
    let origin = entity.vector("origin")?;
    let yaw = (entity.get("angle"))
        .and_then(|angle| angle.trim().parse::<f32>().ok())
        .unwrap_or(0.0)
        .to_radians();
    let feet = [origin[0], origin[1], origin[2] + collision::player::SOLE_Z];
    Some((feet.map(|v| v * collision::UNIT), [yaw.cos(), yaw.sin()]))
}

/// A static model's entity: where it stands (units), its three axes
/// (forward, left and up), each as long as the model's scale along it, and
/// where its light is read.
struct Stand {
    origin: [f32; 3],
    axes: [[f32; 3]; 3],
    light_at: [f32; 3],
}

impl Stand {
    /// From the entity's `origin`, its `angles` (pitch, yaw and roll, in
    /// degrees) or its `angle` (yaw alone), its `modelscale` (else its
    /// `modelscale_vec`, else 1) and its `zoffset`.
    fn of(entity: &Entity) -> Option<Stand> {
        let number = |key: &str| entity.get(key)?.trim().parse::<f32>().ok();
        let origin = entity.vector("origin")?;
        let yaw = number("angle").unwrap_or(0.0);
        let angles = entity.vector("angles").unwrap_or([0.0, yaw, 0.0]);
        let scale = match number("modelscale") {
            Some(scale) if scale != 0.0 => [scale; 3],
            _ => entity.vector("modelscale_vec").unwrap_or([1.0; 3]),
        };
        let (forward, right, up) = collision::trig::angle_vectors(angles);
        let axes = [forward, right.map(|part| -part), up];
        Some(Stand {
            origin,
            axes: std::array::from_fn(|axis| axes[axis].map(|part| part * scale[axis])),
            light_at: [
                origin[0],
                origin[1],
                origin[2] + number("zoffset").unwrap_or(0.0) + 1.0,
            ],
        })
    }

    /// A vector of the model along the level's axes.
    fn turned(&self, v: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|part| {
            v[0] * self.axes[0][part] + v[1] * self.axes[1][part] + v[2] * self.axes[2][part]
        })
    }
}

/// A triangle as the pieces a paint makes of it.
type Pieces<'a> = dyn Fn([Corner; 3]) -> Vec<[Corner; 3]> + 'a;

/// One static model's triangles, lit by `light`, added to the batches of
/// its shaders (or to the batch `paint` names, when it names one), as the
/// pieces `paint`'s function makes of each. A surface named as a tag
/// (`tag_...`) is left out. Gives the model's triangles.
fn place(
    md3: &Md3,
    stand: &Stand,
    light: &ModelLight,
    paint: Option<(Option<&str>, &Pieces)>,
    batches: &mut BTreeMap<Key, Batch>,
    blocks: &mut Blocks,
) -> usize {
    let mut added = 0;
    for surface in &md3.surfaces {
        let Some(shader) = (surface.shaders.first()).filter(|shader| !shader.is_empty()) else {
            continue;
        };
        if surface.name.to_ascii_lowercase().starts_with("tag_") {
            continue;
        }
        let key = (paint.and_then(|(name, _)| name))
            .map_or_else(|| fmt_ja::game_path(shader), str::to_string);
        let batch = batches.entry((None, key, None)).or_default();
        let corner = |index: u32| {
            let at = index as usize;
            let position = stand.turned(surface.positions[at]);
            let normal = stand.turned(surface.normals[at]);
            let length = normal.iter().map(|part| part * part).sum::<f32>().sqrt();
            Corner {
                position: std::array::from_fn(|axis| stand.origin[axis] + position[axis]),
                normal: if length > 0.0 {
                    normal.map(|part| part / length)
                } else {
                    normal
                },
                st: surface.uvs[at],
                color: light.shade(normal),
            }
        };
        // The file winds a triangle clockwise seen from its front.
        for &[a, b, c] in &surface.triangles {
            let corners = [corner(a), corner(c), corner(b)];
            match paint {
                Some((_, pieces)) => {
                    for piece in pieces(corners) {
                        blocks.triangle(batch, piece);
                    }
                }
                None => blocks.triangle(batch, corners),
            }
            added += 1;
        }
    }
    added
}

/// Why a surface is not drawn.
enum LeftOut {
    Flare,
    Unknown,
    Sky,
    NotDrawn,
    Fog,
}

fn left_out(bsp: &Bsp, surface: &Surface) -> Option<LeftOut> {
    let shader = &bsp.shaders[surface.shader as usize];
    match surface.kind {
        SurfaceKind::Flare => Some(LeftOut::Flare),
        SurfaceKind::Other(_) => Some(LeftOut::Unknown),
        _ if shader.surface_flags & SURFACE_SKY != 0 => Some(LeftOut::Sky),
        _ if shader.surface_flags & SURFACE_NODRAW != 0 => Some(LeftOut::NotDrawn),
        _ if shader.content_flags & CONTENTS_FOG != 0 => Some(LeftOut::Fog),
        _ => None,
    }
}

/// A texture with a texel for each room of a level, which a level drawn
/// by its rooms ([`Paint::Rooms`]) is coloured through. Whoever sets a
/// texel's colour colours that room, with no mesh changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// The palette's name among the scene's textures.
    pub name: String,
}

impl Palette {
    /// The palette's width and height in texels.
    pub const SIDE: usize = 16;
    /// The most rooms a palette colours.
    pub const ROOMS: usize = Palette::SIDE * Palette::SIDE;

    /// The palette of the level `level`.
    pub fn of(level: &str) -> Palette {
        Palette {
            name: format!("{level}#rooms"),
        }
    }

    /// The middle of `room`'s texel, as texture coordinates. Rooms are
    /// counted along the rows.
    fn st(&self, room: usize) -> [f32; 2] {
        let texel = room.min(Palette::ROOMS - 1);
        let side = Palette::SIDE as f32;
        [
            ((texel % Palette::SIDE) as f32 + 0.5) / side,
            ((texel / Palette::SIDE) as f32 + 0.5) / side,
        ]
    }
}

/// What a level's meshes are drawn with.
pub enum Paint<'a> {
    /// Each surface's own shader.
    Shaders,
    /// One mesh, coloured through a palette by room. Every triangle, a
    /// static model's too, is cut where a room's shape crosses it
    /// (`level_rooms::Cutter::pieces`), and each piece takes its room's
    /// texel.
    Rooms(&'a crate::level_rooms::Cutter, &'a Palette),
    /// One room alone (its number among the rooms), with each surface's
    /// own shader: what the room is drawn as in a map (`room`). Every
    /// triangle is cut as for `Rooms`, and only the pieces in the room are
    /// kept. Its doors are kept where they rest.
    Room(&'a crate::level_rooms::Cutter, usize),
}

/// The corner at `share` of a triangle's three corners.
fn blend(corners: &[Corner; 3], share: [f32; 3]) -> Corner {
    let mix = |pick: &dyn Fn(&Corner) -> [f32; 3]| -> [f32; 3] {
        let [a, b, c] = [pick(&corners[0]), pick(&corners[1]), pick(&corners[2])];
        std::array::from_fn(|axis| share[0] * a[axis] + share[1] * b[axis] + share[2] * c[axis])
    };
    let normal = mix(&|corner| corner.normal);
    let length = normal.iter().map(|part| part * part).sum::<f32>().sqrt();
    Corner {
        position: mix(&|corner| corner.position),
        normal: if length > 0.0 {
            normal.map(|part| part / length)
        } else {
            corners[0].normal
        },
        st: {
            let [a, b, c] = [corners[0].st, corners[1].st, corners[2].st];
            std::array::from_fn(|axis| share[0] * a[axis] + share[1] * b[axis] + share[2] * c[axis])
        },
        color: mix(&|corner| corner.color.map(f32::from)).map(|part| part.round() as u8),
    }
}

/// `bsp` as a model named `name`, with its light atlas. `md3` reads a
/// static model by its path in the game's files.
pub fn level(name: &str, bsp: &Bsp, md3: &dyn Fn(&str) -> Option<Md3>, paint: &Paint) -> Level {
    let mut tally = Tally::default();
    // The cuts, the palette the rooms are coloured through, and the one
    // room kept.
    let (cutter, palette, only) = match paint {
        Paint::Shaders => (None, None, None),
        Paint::Rooms(cutter, palette) => (Some(*cutter), Some(*palette), None),
        Paint::Room(cutter, room) => (Some(*cutter), None, Some(*room)),
    };
    // The triangles drawn in each room.
    let room_triangles = std::cell::RefCell::new(Vec::<usize>::new());
    // A triangle as its rooms have it: the pieces the cuts make of it
    // (only the one room's, when one is kept), each with its room's texel
    // for its texture coordinates when the rooms are coloured, and with
    // the places on the light atlas of its corners when it has them.
    type Piece = ([Corner; 3], Option<[[f32; 2]; 3]>);
    let cut = |corners: [Corner; 3], texels: Option<[[f32; 2]; 3]>| -> Vec<Piece> {
        let Some(cutter) = cutter else {
            return vec![(corners, texels)];
        };
        let metres = [0, 1, 2].map(|at| corners[at].position.map(|part| part * collision::UNIT));
        (cutter.pieces(metres).into_iter())
            .filter(|(room, _)| only.is_none_or(|only| *room == only))
            .map(|(room, shares)| {
                let mut counts = room_triangles.borrow_mut();
                if counts.len() <= room {
                    counts.resize(room + 1, 0);
                }
                counts[room] += 1;
                let piece = shares.map(|share| {
                    let mut corner = blend(&corners, share);
                    if let Some(palette) = palette {
                        corner.st = palette.st(room);
                    }
                    corner
                });
                let places = texels.map(|texels| {
                    shares.map(|share| {
                        std::array::from_fn(|axis| {
                            share[0] * texels[0][axis]
                                + share[1] * texels[1][axis]
                                + share[2] * texels[2][axis]
                        })
                    })
                });
                (piece, places)
            })
            .collect()
    };
    let pages = bsp.lightmaps.len();
    let width = PAGES_ACROSS * LIGHTMAP_SIDE;
    let mut blocks = Blocks {
        top: pages.div_ceil(PAGES_ACROSS) * LIGHTMAP_SIDE,
        across: width / BLOCK,
        texels: Vec::new(),
    };
    let mut batches: BTreeMap<Key, Batch> = BTreeMap::new();
    // The doors, and the door and part of each of the level's models that
    // is part of one. A level drawn by its rooms keeps them in its own
    // model.
    let movers = crate::movers::movers(bsp);
    let doors = crate::movers::doors(&movers);
    let parts = match cutter {
        Some(_) => BTreeMap::new(),
        None => crate::movers::door_parts(&movers),
    };
    let mut beside = std::collections::BTreeSet::new();
    let mut sky = std::collections::BTreeSet::new();
    let rests = rests(bsp);

    for (index, model) in bsp.models.iter().enumerate() {
        let offset = if index == 0 {
            [0.0; 3]
        } else {
            rests.get(&index).copied().unwrap_or([0.0; 3])
        };
        let owner = parts.get(&index).copied();
        let (first, count) = model.surfaces;
        for surface in &bsp.surfaces[first as usize..(first + count) as usize] {
            let shader = &bsp.shaders[surface.shader as usize];
            let left_out = match left_out(bsp, surface) {
                Some(LeftOut::Flare) => Some(&mut tally.flares),
                Some(LeftOut::Unknown) => Some(&mut tally.unknown),
                Some(LeftOut::Sky) => {
                    sky.insert(fmt_ja::game_path(&shader.name));
                    Some(&mut tally.sky)
                }
                Some(LeftOut::NotDrawn) => Some(&mut tally.not_drawn),
                Some(LeftOut::Fog) => Some(&mut tally.fog),
                None => None,
            };
            if let Some(count) = left_out {
                *count += 1;
                continue;
            }
            let (vertices, triangles) = triangles(bsp, surface);
            if triangles.is_empty() {
                continue;
            }
            tally.triangles += triangles.len();
            tally.patches += usize::from(surface.kind == SurfaceKind::Patch);
            let key = palette.map_or_else(|| fmt_ja::game_path(&shader.name), |p| p.name.clone());
            let stored = surface.lightmaps[0].filter(|page| (*page as usize) < pages);
            // A lightmap the file does not hold is a picture beside it, and
            // the surface's places on it are already its own.
            let kept = (surface.lightmaps[0])
                .filter(|page| (*page as usize) >= pages && palette.is_none());
            if let Some(page) = kept {
                tally.kept_beside += 1;
                let batch = batches.entry((owner, key, Some(page))).or_default();
                if cutter.is_some() {
                    for triangle in &triangles {
                        let own = triangle.map(|index| &vertices[index as usize]);
                        let corners = own.map(|vertex| Corner::of(vertex, offset));
                        let places = own.map(|vertex| vertex.lightmap_st[0]);
                        for (piece, places) in cut(corners, Some(places)) {
                            let places = places.expect("a lightmapped piece has places");
                            beside.insert(page);
                            for (corner, place) in piece.iter().zip(places) {
                                let index = batch.push(corner, place);
                                batch.indices.push(index);
                            }
                        }
                    }
                } else {
                    beside.insert(page);
                    let base = batch.positions.len() as u32;
                    for vertex in &vertices {
                        batch.push(&Corner::of(vertex, offset), vertex.lightmap_st[0]);
                    }
                    (batch.indices).extend(triangles.iter().flatten().map(|index| base + index));
                }
                continue;
            }
            let batch = batches.entry((owner, key, None)).or_default();
            match stored {
                Some(page) => {
                    tally.lightmapped += 1;
                    let page = page as usize;
                    let corner = [page % PAGES_ACROSS, page / PAGES_ACROSS]
                        .map(|cell| (cell * LIGHTMAP_SIDE) as f32);
                    let texel = |vertex: &Vertex| {
                        let [s, t] = vertex.lightmap_st[0];
                        let side = LIGHTMAP_SIDE as f32;
                        [corner[0] + s * side, corner[1] + t * side]
                    };
                    if cutter.is_some() {
                        // Two pieces of a surface may be in two rooms, so
                        // each has corners of its own.
                        for triangle in &triangles {
                            let own = triangle.map(|index| &vertices[index as usize]);
                            let corners = own.map(|vertex| Corner::of(vertex, offset));
                            for (piece, places) in cut(corners, Some(own.map(texel))) {
                                let places = places.expect("a lightmapped piece has places");
                                for (corner, place) in piece.iter().zip(places) {
                                    let index = batch.push(corner, place);
                                    batch.indices.push(index);
                                }
                            }
                        }
                    } else {
                        let base = batch.positions.len() as u32;
                        for vertex in &vertices {
                            batch.push(&Corner::of(vertex, offset), texel(vertex));
                        }
                        (batch.indices)
                            .extend(triangles.iter().flatten().map(|index| base + index));
                    }
                }
                None => {
                    tally.vertex_lit += 1;
                    for triangle in &triangles {
                        let corners = triangle.map(|index| &vertices[index as usize]);
                        let corners = corners.map(|vertex| Corner::of(vertex, offset));
                        for (piece, _) in cut(corners, None) {
                            blocks.triangle(batch, piece);
                        }
                    }
                }
            }
        }
    }

    // Each static model is read once, however often the level places it.
    let mut read: BTreeMap<String, Option<Md3>> = BTreeMap::new();
    for entity in &bsp.entities {
        if entity.class() != "misc_model_static" {
            continue;
        }
        let path = entity.get("model").map(fmt_ja::game_path);
        let model = path.and_then(|path| {
            read.entry(path.clone())
                .or_insert_with(|| md3(&path))
                .as_ref()
        });
        let Some((model, stand)) = model.zip(Stand::of(entity)) else {
            tally.static_missing += 1;
            continue;
        };
        let light = light_at(&bsp.light_grid, stand.light_at);
        let pieces = |corners: [Corner; 3]| -> Vec<[Corner; 3]> {
            (cut(corners, None).into_iter())
                .map(|(piece, _)| piece)
                .collect()
        };
        let paint = cutter.map(|_| {
            (
                palette.map(|palette| palette.name.as_str()),
                &pieces as &Pieces,
            )
        });
        let added = place(model, &stand, &light, paint, &mut batches, &mut blocks);
        tally.static_models += 1;
        tally.static_triangles += added;
        tally.triangles += added;
    }

    let (blocks_top, blocks_across) = (blocks.top, blocks.across);
    let blocks = blocks.texels;
    let height = (blocks_top + blocks.len().div_ceil(blocks_across) * BLOCK).max(1);
    let mut rgba = vec![255u8; width * height * 4];
    for (page, texels) in bsp.lightmaps.iter().enumerate() {
        let (left, top) = (
            page % PAGES_ACROSS * LIGHTMAP_SIDE,
            page / PAGES_ACROSS * LIGHTMAP_SIDE,
        );
        for (row, line) in texels
            .as_chunks::<{ LIGHTMAP_SIDE * 3 }>()
            .0
            .iter()
            .enumerate()
        {
            let at = ((top + row) * width + left) * 4;
            for (texel, rgb) in line.as_chunks::<3>().0.iter().enumerate() {
                rgba[at + texel * 4..at + texel * 4 + 3].copy_from_slice(rgb);
            }
        }
    }
    for (block, colours) in blocks.iter().enumerate() {
        let (left, top) = (
            block % blocks_across * BLOCK,
            blocks_top + block / blocks_across * BLOCK,
        );
        for (corner, rgb) in colours.iter().enumerate() {
            let at = ((top + corner / BLOCK) * width + left + corner % BLOCK) * 4;
            rgba[at..at + 3].copy_from_slice(rgb);
        }
    }

    let lightmap = light_name(name);
    let mut meshes = Vec::new();
    let mut door_meshes: Vec<Vec<MeshIr>> = vec![Vec::new(); doors.len()];
    for ((owner, shader, page), batch) in batches {
        let mesh = MeshIr {
            node: owner.map_or(0, |(_, part)| part + 1),
            positions: batch.positions,
            normals: batch.normals,
            uv1: batch.uv1,
            uv2: match page {
                Some(_) => batch.uv2,
                None => (batch.uv2.into_iter())
                    .map(|[x, y]| [x / width as f32, y / height as f32])
                    .collect(),
            },
            indices: batch.indices,
            texture: Some(shader),
            lightmap: Some(page.map_or_else(|| lightmap.clone(), |page| beside_name(name, page))),
            transparency_hint: 0,
            skinned: false,
            shadow: true,
            self_illum: [0.0; 3],
            alpha: 1.0,
            ambient: [1.0; 3],
            diffuse: [1.0; 3],
            uv_scroll: None,
            skin: None,
            dangly: None,
        };
        match owner {
            Some((door, _)) => door_meshes[door].push(mesh),
            None => meshes.push(mesh),
        }
    }
    let node = |name: String, parent: Option<usize>| NodeIr {
        name,
        parent,
        translation: [0.0; 3],
        rotation: [0.0, 0.0, 0.0, 1.0],
        world: IDENTITY,
        animated: true,
    };
    let door_models = (doors.iter().zip(door_meshes).enumerate())
        .map(|(number, (door, meshes))| {
            let model = door_name(name, number);
            DoorModel {
                opening: door_clip("opening1", &model, door, (0.0, 1.0)),
                closing: door_clip("closing1", &model, door, (1.0, 0.0)),
                model: ModelIr {
                    name: model.clone(),
                    supermodel: None,
                    nodes: std::iter::once(node(model.clone(), None))
                        .chain(
                            (0..door.parts.len())
                                .map(|part| node(format!("{model}_{part}"), Some(0))),
                        )
                        .collect(),
                    meshes,
                    lights: Vec::new(),
                    emitters: Vec::new(),
                    animations: Vec::new(),
                    animation_scale: 1.0,
                    affected_by_fog: true,
                    refusals: Vec::new(),
                    folded: Vec::new(),
                },
            }
        })
        .collect();
    Level {
        model: ModelIr {
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
            meshes,
            lights: Vec::new(),
            emitters: Vec::new(),
            animations: Vec::new(),
            animation_scale: 1.0,
            affected_by_fog: true,
            refusals: Vec::new(),
            folded: Vec::new(),
        },
        light: TextureIr {
            width: width as u32,
            height: height as u32,
            encoding: Encoding::Rgba8,
            mips: vec![rgba],
            cube: false,
            frames: 1,
            facts: TextureFacts::default(),
            faces: Vec::new(),
            frame_mips: Vec::new(),
        },
        start: start(bsp),
        beside: beside.into_iter().collect(),
        sky: sky.into_iter().collect(),
        doors: door_models,
        tally: Tally {
            rooms: room_triangles.into_inner(),
            ..tally
        },
    }
}
