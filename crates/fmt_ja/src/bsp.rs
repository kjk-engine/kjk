//! Jedi Academy levels (`RBSP`, version 1): a compiled map, as `maps/*.bsp`
//! in the game's archives.
//!
//! The file starts with its ident, its version and 18 lumps, each a byte
//! offset and a length. A lump is an array of fixed-size records, but for
//! the entities (text) and the visibility (a header and bit rows). Every
//! size below was checked against the 61 levels of a Steam install
//! (`kjk formats ja`).
//!
//! | lump | record | bytes |
//! |---|---|---|
//! | 0 | entities: `{ "key" "value" … }` blocks, as text | |
//! | 1 | shader: name (64), surface flags, content flags | 72 |
//! | 2 | plane: normal, distance | 16 |
//! | 3 | node: plane, two children, bounds (whole units) | 36 |
//! | 4 | leaf: cluster, area, bounds, its surfaces and brushes | 48 |
//! | 5, 6 | a leaf's surfaces, a leaf's brushes: one index each | 4 |
//! | 7 | model: bounds, its surfaces, its brushes | 40 |
//! | 8 | brush: its sides, shader | 12 |
//! | 9 | brush side: plane, shader, draw surface | 12 |
//! | 10 | vertex: place, texture st, four lightmap st, normal, four colours | 80 |
//! | 11 | index: a vertex of its surface, counted from the surface's first | 4 |
//! | 12 | fog (not read) | 72 |
//! | 13 | surface (below) | 148 |
//! | 14 | lightmap: 128 by 128 texels of RGB | 49152 |
//! | 15 | light grid point: four ambient and four directed colours (RGB), four styles, a direction | 30 |
//! | 17 | light grid cell: the point it takes its light from | 2 |
//! | 16 | visibility: cluster count, bytes a row, then a row of bits a cluster | |
//!
//! A surface: shader, fog, kind, first vertex and vertex count, first
//! index and index count, four lightmap styles and four vertex styles (a
//! byte each), four lightmap numbers, four lightmap x and y, the lightmap's
//! width and height, its origin and three vectors, and a patch's width and
//! height in control points.
//!
//! The light grid lights what moves through the level and the models placed
//! in it: a box of cells over the world, each naming one of the grid's
//! points ([`LightGrid`]).
//!
//! Jedi Academy keeps up to four light styles a surface, for lights that
//! flicker or switch. The first is the level's steady light. A lightmap
//! number below zero means the surface has no lightmap and is lit by its
//! vertices' colours. A level made with a newer compiler may keep its
//! lightmaps as pictures beside it and none in the file. Its surfaces then
//! name lightmaps the file does not hold.

use std::fmt;

const LUMPS: usize = 18;
const HEADER: usize = 8 + LUMPS * 8;
const NAME: usize = 64;

/// A lightmap's side, in texels.
pub const LIGHTMAP_SIDE: usize = 128;
/// A lightmap's bytes: its texels, three bytes each.
pub const LIGHTMAP_BYTES: usize = LIGHTMAP_SIDE * LIGHTMAP_SIDE * 3;
/// The light styles a surface and a vertex keep.
pub const STYLES: usize = 4;

/// The light style of a slot that is not used.
pub const STYLE_NONE: u8 = 255;
/// A light grid cell's size where the level's `worldspawn` gives no
/// `gridsize`.
pub const GRID_SIZE: [f32; 3] = [64.0, 64.0, 128.0];

/// A shader's surface flag: the sky. Its surfaces are where the sky shows,
/// not walls.
pub const SHOWS_SKY: u32 = 0x2000;
/// A shader's surface flag: never drawn (clips, triggers, caulk).
pub const NEVER_DRAWN: u32 = 0x20_0000;
/// A shader's content flag: solid to everything.
pub const STOPS_ALL: u32 = 0x1;
/// A shader's content flag: blocks players and creatures, and nothing else.
pub const STOPS_BODIES: u32 = 0x10;
/// A shader's content flag: a volume of fog.
pub const HOLDS_FOG: u32 = 0x8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BspError {
    Signature,
    Version(i32),
    Truncated(&'static str),
    Bad(String),
}

impl fmt::Display for BspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BspError::Signature => write!(f, "bsp: not a Jedi Academy level (RBSP)"),
            BspError::Version(version) => write!(f, "bsp: version {version}, expected 1"),
            BspError::Truncated(what) => write!(f, "bsp: truncated {what}"),
            BspError::Bad(what) => write!(f, "bsp: {what}"),
        }
    }
}

impl std::error::Error for BspError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shader {
    pub name: String,
    pub surface_flags: u32,
    pub content_flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    pub normal: [f32; 3],
    pub dist: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub plane: u32,
    /// The side in front of the plane, then the side behind it. A value
    /// below zero is a leaf: leaf `-(value + 1)`.
    pub children: [i32; 2],
    pub min: [i32; 3],
    pub max: [i32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Leaf {
    /// Its row in the visibility; below zero for a leaf inside a solid.
    pub cluster: i32,
    /// The part of the level it is in, closed off from the others by doors.
    pub area: i32,
    pub min: [i32; 3],
    pub max: [i32; 3],
    /// Its span of `Bsp::leaf_surfaces`: first and count.
    pub surfaces: (u32, u32),
    /// Its span of `Bsp::leaf_brushes`: first and count.
    pub brushes: (u32, u32),
}

/// The world (the first model) or a part that moves: a door, a lift.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Model {
    pub min: [f32; 3],
    pub max: [f32; 3],
    /// Its span of `Bsp::surfaces`: first and count.
    pub surfaces: (u32, u32),
    /// Its span of `Bsp::brushes`: first and count.
    pub brushes: (u32, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Brush {
    /// Its span of `Bsp::brush_sides`: first and count.
    pub sides: (u32, u32),
    pub shader: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrushSide {
    pub plane: u32,
    pub shader: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub st: [f32; 2],
    /// Where it lies on its surface's lightmap, for each light style.
    pub lightmap_st: [[f32; 2]; STYLES],
    pub normal: [f32; 3],
    /// Its light as a colour (RGBA), for each light style.
    pub color: [[u8; 4]; STYLES],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    /// A flat face.
    Planar,
    /// A curved face: a grid of control points, `patch` wide and high.
    Patch,
    /// Triangles of any shape: a model the compiler put into the level.
    Triangles,
    /// A lens flare's place. It has no triangles.
    Flare,
    /// A kind this reader does not know.
    Other(i32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Surface {
    pub shader: u32,
    pub kind: SurfaceKind,
    /// Its span of `Bsp::vertices`: first and count.
    pub vertices: (u32, u32),
    /// Its span of `Bsp::indices`: first and count. An index counts from
    /// the surface's first vertex.
    pub indices: (u32, u32),
    /// The lightmap of each light style; `None` where the style has none.
    /// A number past `Bsp::lightmaps` names a picture beside the level
    /// (`maps/<level>/lm_0000.tga`), where a newer compiler keeps
    /// lightmaps larger than the file's own.
    pub lightmaps: [Option<u32>; STYLES],
    /// A patch's control points: across and down. Zero for other kinds.
    pub patch: (u32, u32),
}

/// Which clusters see which: a row of bits a cluster.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Visibility {
    pub clusters: usize,
    pub row_bytes: usize,
    pub rows: Vec<u8>,
}

impl Visibility {
    /// Whether a viewer in cluster `from` may see cluster `to`. A level
    /// compiled without visibility has no rows, and everything is seen.
    pub fn sees(&self, from: usize, to: usize) -> bool {
        if self.rows.is_empty() {
            return true;
        }
        // A cluster the table has no row or no bit for sees nothing and is
        // seen from nowhere. Without this, a `to` past the row's end read a
        // bit of the next cluster's row.
        if from >= self.clusters || to >= self.clusters || to / 8 >= self.row_bytes {
            return false;
        }
        self.rows
            .get(from * self.row_bytes + to / 8)
            .is_some_and(|byte| byte & (1 << (to % 8)) != 0)
    }
}

/// One point of the light grid: the light that comes from all round
/// (`ambient`) and the light that comes from one side (`direct`), for each
/// light style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPoint {
    pub ambient: [[u8; 3]; STYLES],
    pub direct: [[u8; 3]; STYLES],
    /// `STYLE_NONE` in the first slot marks a point inside a wall.
    pub styles: [u8; STYLES],
    /// Where the directed light comes from, as two angles of 256 steps to
    /// the full turn: the angle from +Z, then the angle about Z from +X.
    pub from: [u8; 2],
}

/// The light grid: cells of `size` units, `bounds` of them along each
/// axis, the first with its corner at `origin`. The cell at (x, y, z) is
/// `cells[x + bounds[0] * (y + bounds[1] * z)]`, and names one of `points`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LightGrid {
    pub size: [f32; 3],
    pub origin: [f32; 3],
    pub bounds: [usize; 3],
    pub cells: Vec<u16>,
    pub points: Vec<GridPoint>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Bsp {
    pub entities: Vec<Entity>,
    pub shaders: Vec<Shader>,
    pub planes: Vec<Plane>,
    pub nodes: Vec<Node>,
    pub leafs: Vec<Leaf>,
    pub leaf_surfaces: Vec<u32>,
    pub leaf_brushes: Vec<u32>,
    pub models: Vec<Model>,
    pub brushes: Vec<Brush>,
    pub brush_sides: Vec<BrushSide>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub surfaces: Vec<Surface>,
    /// Each lightmap's texels: `LIGHTMAP_BYTES` of RGB, the first row
    /// first.
    pub lightmaps: Vec<Vec<u8>>,
    pub visibility: Visibility,
    /// Empty where the level has none, or where its cells are not as many
    /// as its world's size asks for (the game then lights nothing by it).
    pub light_grid: LightGrid,
}

/// One block of the entity text: its keys and values, in the file's order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entity(pub Vec<(String, String)>);

impl Entity {
    /// The value of `key`, compared without case.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    }

    pub fn class(&self) -> &str {
        self.get("classname").unwrap_or("")
    }

    /// A value of three numbers (`origin`), if it has one.
    pub fn vector(&self, key: &str) -> Option<[f32; 3]> {
        let mut numbers = self.get(key)?.split_whitespace().map(str::parse::<f32>);
        let vector = [
            numbers.next()?.ok()?,
            numbers.next()?.ok()?,
            numbers.next()?.ok()?,
        ];
        Some(vector)
    }

    /// The moving part it stands for (`model` is `*3` for model 3).
    pub fn model(&self) -> Option<usize> {
        self.get("model")?.strip_prefix('*')?.parse().ok()
    }
}

/// The entity text's blocks. Text outside a block and a key with no value
/// are left out.
fn entities(text: &[u8]) -> Vec<Entity> {
    let text = String::from_utf8_lossy(text);
    let mut out = Vec::new();
    let mut open: Option<Entity> = None;
    let mut key: Option<String> = None;
    let mut rest = text.as_ref();
    while let Some(at) = rest.find(['{', '}', '"']) {
        let found = rest.as_bytes()[at];
        rest = &rest[at + 1..];
        match found {
            b'{' => {
                open = Some(Entity::default());
                key = None;
            }
            b'}' => {
                out.extend(open.take());
                key = None;
            }
            _ => {
                let end = rest.find('"').unwrap_or(rest.len());
                let word = rest[..end].to_string();
                rest = rest.get(end + 1..).unwrap_or("");
                match (open.as_mut(), key.take()) {
                    (Some(entity), Some(key)) => entity.0.push((key, word)),
                    (Some(_), None) => key = Some(word),
                    (None, _) => {}
                }
            }
        }
    }
    out
}

/// A cursor over one record.
struct Record<'a>(&'a [u8]);

impl Record<'_> {
    fn bytes<const N: usize>(&mut self) -> [u8; N] {
        let (head, tail) = self.0.split_at(N);
        self.0 = tail;
        head.try_into().expect("N bytes")
    }
    fn i32(&mut self) -> i32 {
        i32::from_le_bytes(self.bytes())
    }
    fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.bytes())
    }
    fn f32(&mut self) -> f32 {
        f32::from_le_bytes(self.bytes())
    }
    fn i32s<const N: usize>(&mut self) -> [i32; N] {
        std::array::from_fn(|_| self.i32())
    }
    fn f32s<const N: usize>(&mut self) -> [f32; N] {
        std::array::from_fn(|_| self.f32())
    }
    fn skip(&mut self, bytes: usize) {
        self.0 = &self.0[bytes..];
    }
}

/// A lump's bytes, which must hold whole records of `size` bytes.
fn lump<'a>(
    bytes: &'a [u8],
    index: usize,
    size: usize,
    what: &'static str,
) -> Result<&'a [u8], BspError> {
    let at = 8 + index * 8;
    let field = |at: usize| {
        let raw: [u8; 4] = bytes[at..at + 4].try_into().expect("4 bytes");
        usize::try_from(i32::from_le_bytes(raw)).map_err(|_| BspError::Bad(format!("{what} < 0")))
    };
    let (offset, len) = (field(at)?, field(at + 4)?);
    let data = offset
        .checked_add(len)
        .and_then(|end| bytes.get(offset..end))
        .ok_or(BspError::Truncated(what))?;
    if len % size != 0 {
        return Err(BspError::Bad(format!(
            "{what}: {len} bytes are not whole records of {size}"
        )));
    }
    Ok(data)
}

/// Each record of a lump, read by `read`.
fn records<T>(
    bytes: &[u8],
    index: usize,
    size: usize,
    what: &'static str,
    read: impl Fn(&mut Record) -> T,
) -> Result<Vec<T>, BspError> {
    Ok(lump(bytes, index, size, what)?
        .chunks_exact(size)
        .map(|record| read(&mut Record(record)))
        .collect())
}

/// A span (first, count) as the file has it, both at least zero.
fn span(first: i32, count: i32) -> (u32, u32) {
    (first.max(0) as u32, count.max(0) as u32)
}

/// Whether a span lies within `len` items.
fn within((first, count): (u32, u32), len: usize) -> bool {
    (first as usize)
        .checked_add(count as usize)
        .is_some_and(|end| end <= len)
}

pub fn parse(bytes: &[u8]) -> Result<Bsp, BspError> {
    if bytes.len() < HEADER {
        return Err(BspError::Truncated("header"));
    }
    if &bytes[..4] != b"RBSP" {
        return Err(BspError::Signature);
    }
    let version = i32::from_le_bytes(bytes[4..8].try_into().expect("4 bytes"));
    if version != 1 {
        return Err(BspError::Version(version));
    }

    let shaders = records(bytes, 1, 72, "shaders", |r| {
        let name: [u8; NAME] = r.bytes();
        let end = name.iter().position(|&b| b == 0).unwrap_or(NAME);
        Shader {
            name: String::from_utf8_lossy(&name[..end]).into_owned(),
            surface_flags: r.u32(),
            content_flags: r.u32(),
        }
    })?;
    let planes = records(bytes, 2, 16, "planes", |r| Plane {
        normal: r.f32s(),
        dist: r.f32(),
    })?;
    let nodes = records(bytes, 3, 36, "nodes", |r| Node {
        plane: r.u32(),
        children: r.i32s(),
        min: r.i32s(),
        max: r.i32s(),
    })?;
    let leafs = records(bytes, 4, 48, "leafs", |r| Leaf {
        cluster: r.i32(),
        area: r.i32(),
        min: r.i32s(),
        max: r.i32s(),
        surfaces: span(r.i32(), r.i32()),
        brushes: span(r.i32(), r.i32()),
    })?;
    let leaf_surfaces = records(bytes, 5, 4, "leaf surfaces", |r| r.u32())?;
    let leaf_brushes = records(bytes, 6, 4, "leaf brushes", |r| r.u32())?;
    let models = records(bytes, 7, 40, "models", |r| Model {
        min: r.f32s(),
        max: r.f32s(),
        surfaces: span(r.i32(), r.i32()),
        brushes: span(r.i32(), r.i32()),
    })?;
    let brushes = records(bytes, 8, 12, "brushes", |r| Brush {
        sides: span(r.i32(), r.i32()),
        shader: r.u32(),
    })?;
    let brush_sides = records(bytes, 9, 12, "brush sides", |r| BrushSide {
        plane: r.u32(),
        shader: r.u32(),
    })?;
    let vertices = records(bytes, 10, 80, "vertices", |r| Vertex {
        position: r.f32s(),
        st: r.f32s(),
        lightmap_st: std::array::from_fn(|_| r.f32s()),
        normal: r.f32s(),
        color: std::array::from_fn(|_| r.bytes()),
    })?;
    let indices = records(bytes, 11, 4, "indices", |r| r.u32())?;
    let surfaces = records(bytes, 13, 148, "surfaces", |r| {
        let shader = r.u32();
        r.skip(4);
        let kind = match r.i32() {
            1 => SurfaceKind::Planar,
            2 => SurfaceKind::Patch,
            3 => SurfaceKind::Triangles,
            4 => SurfaceKind::Flare,
            other => SurfaceKind::Other(other),
        };
        let vertices = span(r.i32(), r.i32());
        let indices = span(r.i32(), r.i32());
        r.skip(2 * STYLES);
        let lightmaps = r.i32s::<STYLES>().map(|number| u32::try_from(number).ok());
        r.skip(8 * STYLES + 8 + 12 + 36);
        let patch = span(r.i32(), r.i32());
        Surface {
            shader,
            kind,
            vertices,
            indices,
            lightmaps,
            patch: if kind == SurfaceKind::Patch {
                patch
            } else {
                (0, 0)
            },
        }
    })?;
    let lightmaps: Vec<Vec<u8>> = lump(bytes, 14, LIGHTMAP_BYTES, "lightmaps")?
        .as_chunks::<LIGHTMAP_BYTES>()
        .0
        .iter()
        .map(|texels| texels.to_vec())
        .collect();
    let visibility = match lump(bytes, 16, 1, "visibility")? {
        [] => Visibility::default(),
        data => {
            let header = data.get(..8).ok_or(BspError::Truncated("visibility"))?;
            let field = |at: usize| {
                usize::try_from(i32::from_le_bytes(
                    header[at..at + 4].try_into().expect("4 bytes"),
                ))
                .map_err(|_| BspError::Bad("visibility: a count < 0".into()))
            };
            let (clusters, row_bytes) = (field(0)?, field(4)?);
            let rows = clusters
                .checked_mul(row_bytes)
                .and_then(|len| data.get(8..8 + len))
                .ok_or(BspError::Truncated("visibility rows"))?;
            Visibility {
                clusters,
                row_bytes,
                rows: rows.to_vec(),
            }
        }
    };

    let entities = entities(lump(bytes, 0, 1, "entities")?);
    let points = records(bytes, 15, 30, "light grid", |r| GridPoint {
        ambient: std::array::from_fn(|_| r.bytes()),
        direct: std::array::from_fn(|_| r.bytes()),
        styles: r.bytes(),
        from: r.bytes(),
    })?;
    let cells = records(bytes, 17, 2, "light grid cells", |r| {
        u16::from_le_bytes(r.bytes())
    })?;
    let light_grid = light_grid(&entities, models.first(), cells, points);

    let bsp = Bsp {
        entities,
        shaders,
        planes,
        nodes,
        leafs,
        leaf_surfaces,
        leaf_brushes,
        models,
        brushes,
        brush_sides,
        vertices,
        indices,
        surfaces,
        lightmaps,
        visibility,
        light_grid,
    };
    bsp.checked()?;
    Ok(bsp)
}

/// The light grid over the world: its cells start at the first multiple of
/// their size inside the world's bounds and end at the last.
///
/// Gives the grid's first point and how many cells it has along each axis,
/// for cells of `size` in a world from `min` to `max`. The last point less
/// the first, in cells, is a whole number, but not always when it is worked
/// out: with cells 33.3 wide, 71 comes out as 70.99999. It is rounded to
/// the nearest whole number. Cut off, the count was one short, it no longer
/// matched the level's cells, and the level lost its whole grid.
pub fn grid_layout(size: [f32; 3], min: [f32; 3], max: [f32; 3]) -> ([f32; 3], [usize; 3]) {
    let origin: [f32; 3] = std::array::from_fn(|axis| size[axis] * (min[axis] / size[axis]).ceil());
    let bounds: [usize; 3] = std::array::from_fn(|axis| {
        let last = size[axis] * (max[axis] / size[axis]).floor();
        (((last - origin[axis]) / size[axis]).round() as i64 + 1).max(0) as usize
    });
    (origin, bounds)
}

/// The level's light grid, from its cells and its points, laid out by
/// `grid_layout`. The size is the
/// `worldspawn` entity's `gridsize`, else `GRID_SIZE`. A grid whose cells
/// are not as many as that asks for, or which names a point it lacks, is
/// left empty.
fn light_grid(
    entities: &[Entity],
    world: Option<&Model>,
    cells: Vec<u16>,
    points: Vec<GridPoint>,
) -> LightGrid {
    let Some(world) = world else {
        return LightGrid::default();
    };
    let size = (entities.iter())
        .find(|entity| entity.class() == "worldspawn")
        .and_then(|entity| entity.vector("gridsize"))
        // A size that is no number, or without end, is none.
        .filter(|size| size.iter().all(|side| side.is_finite() && *side > 0.0))
        .unwrap_or(GRID_SIZE);
    let (origin, bounds) = grid_layout(size, world.min, world.max);
    let wanted = bounds
        .iter()
        .try_fold(1usize, |all, side| all.checked_mul(*side));
    if wanted != Some(cells.len()) || cells.iter().any(|&cell| cell as usize >= points.len()) {
        return LightGrid::default();
    }
    LightGrid {
        size,
        origin,
        bounds,
        cells,
        points,
    }
}

impl Bsp {
    /// Refuses a level whose records point outside its arrays, so what
    /// reads a parsed level may index without checking. `parse` gives only
    /// levels that pass.
    pub fn checked(&self) -> Result<(), BspError> {
        let bad = |what: String| Err(BspError::Bad(what));
        for (at, surface) in self.surfaces.iter().enumerate() {
            if surface.shader as usize >= self.shaders.len() {
                return bad(format!("surface {at} names shader {}", surface.shader));
            }
            if !within(surface.vertices, self.vertices.len())
                || !within(surface.indices, self.indices.len())
            {
                return bad(format!("surface {at} reaches past the vertices or indices"));
            }
            let (first, count) = surface.indices;
            let own = &self.indices[first as usize..(first + count) as usize];
            if own.iter().any(|&index| index >= surface.vertices.1) {
                return bad(format!("surface {at} indexes past its vertices"));
            }
            if surface.kind == SurfaceKind::Patch {
                let (width, height) = surface.patch;
                let grid = (width as usize).checked_mul(height as usize);
                if width < 3
                    || height < 3
                    || width % 2 == 0
                    || height % 2 == 0
                    || grid != Some(surface.vertices.1 as usize)
                {
                    return bad(format!(
                        "surface {at} is a patch of {width} by {height} over {} vertices",
                        surface.vertices.1
                    ));
                }
            }
        }
        for (at, model) in self.models.iter().enumerate() {
            if !within(model.surfaces, self.surfaces.len())
                || !within(model.brushes, self.brushes.len())
            {
                return bad(format!("model {at} reaches past the surfaces or brushes"));
            }
        }
        for (at, leaf) in self.leafs.iter().enumerate() {
            if !within(leaf.surfaces, self.leaf_surfaces.len())
                || !within(leaf.brushes, self.leaf_brushes.len())
            {
                return bad(format!("leaf {at} reaches past the leafs' lists"));
            }
        }
        if (self.leaf_surfaces.iter()).any(|&surface| surface as usize >= self.surfaces.len()) {
            return bad("a leaf names a surface the level lacks".into());
        }
        if (self.leaf_brushes.iter()).any(|&brush| brush as usize >= self.brushes.len()) {
            return bad("a leaf names a brush the level lacks".into());
        }
        for (at, brush) in self.brushes.iter().enumerate() {
            if !within(brush.sides, self.brush_sides.len()) {
                return bad(format!("brush {at} reaches past the brush sides"));
            }
            // A brush's shader and a side's, as a surface's above. They
            // were the two the check left out.
            if brush.shader as usize >= self.shaders.len() {
                return bad(format!("brush {at} names shader {}", brush.shader));
            }
        }
        if (self.brush_sides.iter()).any(|side| side.plane as usize >= self.planes.len()) {
            return bad("a brush side names a plane the level lacks".into());
        }
        if (self.brush_sides.iter()).any(|side| side.shader as usize >= self.shaders.len()) {
            return bad("a brush side names a shader the level lacks".into());
        }
        for (at, node) in self.nodes.iter().enumerate() {
            let child_ok = |child: i32| match usize::try_from(child) {
                Ok(node) => node < self.nodes.len(),
                Err(_) => ((-(i64::from(child)) - 1) as usize) < self.leafs.len(),
            };
            if node.plane as usize >= self.planes.len() || !node.children.into_iter().all(child_ok)
            {
                return bad(format!(
                    "node {at} names a plane or a child the level lacks"
                ));
            }
        }
        Ok(())
    }

    /// The surfaces of model `index`.
    pub fn model_surfaces(&self, index: usize) -> &[Surface] {
        self.models.get(index).map_or(&[], |model| {
            let (first, count) = model.surfaces;
            &self.surfaces[first as usize..(first + count) as usize]
        })
    }

    /// A surface's vertices.
    pub fn surface_vertices(&self, surface: &Surface) -> &[Vertex] {
        let (first, count) = surface.vertices;
        &self.vertices[first as usize..(first + count) as usize]
    }

    /// A surface's indices, each counted from its first vertex.
    pub fn surface_indices(&self, surface: &Surface) -> &[u32] {
        let (first, count) = surface.indices;
        &self.indices[first as usize..(first + count) as usize]
    }
}
