//! `kjk room-lights [k1|tsl|all]`: a survey of the rooms' own light in every
//! area of both games (`docs/LIGHTING-PLAN.md`, step 7a and 7d). It opens no
//! window. It writes `room-lights.json` and `room-lights.txt` into `--out`.
//!
//! - **The lamps.** Every light node of every room, door, placeable and
//!   creature model placed in an area. Each gives its dynamic type, its
//!   ambient-only and shadow flags, its priority, its radius, and the room
//!   it sits in. A room's lamp is in its own room. Any other lamp is in the
//!   smallest room box that holds it.
//! - **The glowing surfaces.** Every mesh of a room, door or placeable whose
//!   self-illumination colour is above black. Each gives its area in square
//!   metres, its texture and whether it has a lightmap. A glowing surface
//!   looks like a light when it is lightmapped, or when a tenth of its
//!   texture is bright. A door's never does, and neither does a sky's.
//! - **Near the bake.** For each lamp and each glowing surface, the mean
//!   brightness of the area's lightmap texels within `NEAR` metres that face
//!   it, over the mean of all the area's lightmap texels. A light the bake
//!   used should sit in a bright patch. Brightness is read as stored, gamma
//!   encoded.
//! - **The layouts.** For each lightmapped room mesh, its texels per metre,
//!   the share of its triangles whose lightmap area overlaps another's, and
//!   its islands. An island is a set of triangles joined on the lightmap.
//!   For each island, whether its footprint (the texels bilinear filtering
//!   reads for it) shares a texel with an unrelated island, and how far the
//!   nearest unrelated island is in the coordinates. An unrelated island is
//!   one that does not meet this one on the surface. For each lightmap, the
//!   share no footprint holds. The lightmap's coordinates are the mesh's
//!   second set (`uv2`), as the renderer reads them.
//! - **Rooms.** A room with no walkable face in its walkmesh is a sky, a
//!   backdrop or a cutscene set. The tables leave such rooms out, and the
//!   JSON marks them `backdrop`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use asset_odyssey::model::{Matrix, ModelIr, multiply};
use asset_odyssey::module::{AreaIr, Kind, ModuleIr};
use asset_odyssey::texture::{Encoding, TextureIr};
use fmt_odyssey::dxt;
use fmt_odyssey::ResType;
use fmt_odyssey::bwm::Walkmesh;
use fmt_odyssey::mdl::Node;
use serde::Serialize;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

use crate::modules::{ResolverSource, module_names};
use crate::viewer;

/// How far from a lamp or a glowing surface its lightmap texels are read
/// (metres).
const NEAR: f32 = 2.0;
/// How far around an island's edge another island is looked for (texels).
const GAP_REACH: i64 = 8;
/// How far the gap between islands is looked for in the lightmap's
/// coordinates (texels).
const COORD_REACH: f32 = 2.0;
/// A texel centre this close to a triangle's edge (texels) is in neither
/// triangle, so two triangles that share an edge do not count as sharing
/// texels.
const EDGE: f32 = 1e-3;
/// A glowing mesh larger than this (square metres) is a sky or a backdrop.
pub(crate) const SKY_M2: f32 = 2000.0;
/// A texel of a diffuse texture brighter than this (0 to 1, as stored)
/// counts as bright.
const BRIGHT: f32 = 0.75;
/// A glowing surface whose texture has at least this share of bright
/// texels looks like a light: a panel, a lamp or a screen.
pub(crate) const BRIGHT_SHARE: f32 = 0.1;
/// How far outside a room's box a lamp may sit and still be in the room
/// (metres).
const BOX_SLACK: f32 = 0.25;

pub fn run(discovery: &Discovery, game: &str, out: &Path) -> ExitCode {
    let games = match game {
        "all" => vec![Game::Kotor1, Game::Kotor2],
        key => match Game::from_key(key).filter(|game| *game != Game::JediAcademy) {
            Some(game) => vec![game],
            None => {
                eprintln!("room-lights: use k1, tsl or all");
                return ExitCode::from(2);
            }
        },
    };
    let mut surveys = Vec::new();
    for game in games {
        let Some(install) = discovery.get(game) else {
            eprintln!("{}: not found (set {})", game.key(), game.env_var());
            return ExitCode::FAILURE;
        };
        match survey_game(game, install) {
            Ok(survey) => surveys.push(survey),
            Err(error) => {
                eprintln!("{}: {error}", game.key());
                return ExitCode::FAILURE;
            }
        }
    }
    if let Err(error) = std::fs::create_dir_all(out) {
        eprintln!("{}: {error}", out.display());
        return ExitCode::FAILURE;
    }
    let json = out.join("room-lights.json");
    let text = out.join("room-lights.txt");
    let written = serde_json::to_string_pretty(&surveys)
        .map_err(|error| error.to_string())
        .and_then(|body| std::fs::write(&json, body).map_err(|error| error.to_string()))
        .and_then(|()| {
            std::fs::write(&text, report(&surveys)).map_err(|error| error.to_string())
        });
    if let Err(error) = written {
        eprintln!("room-lights: {error}");
        return ExitCode::FAILURE;
    }
    for survey in &surveys {
        outln!(
            "{}: {} modules, {} areas, {} refused",
            survey.game,
            survey.modules,
            survey.areas.len(),
            survey.refused.len()
        );
    }
    outln!("wrote {} and {}", json.display(), text.display());
    ExitCode::SUCCESS
}

// ---------------------------------------------------------------------------
// What the survey writes.

#[derive(Serialize)]
struct GameSurvey {
    game: &'static str,
    modules: usize,
    /// Modules or models that did not load, and why.
    refused: Vec<String>,
    areas: Vec<AreaSurvey>,
}

#[derive(Serialize, Default)]
struct AreaSurvey {
    module: String,
    area: String,
    rooms: Vec<RoomFacts>,
    lamps: Vec<Lamp>,
    glows: Vec<Glow>,
    meshes: Vec<LayoutMesh>,
    lightmaps: Vec<LightmapFacts>,
    /// Room models the layout names that the install lacks.
    missing_rooms: Vec<String>,
    /// The mean brightness of every lightmap texel of the area (0 to 1).
    lightmap_mean: Option<f32>,
}

#[derive(Serialize, Clone)]
struct RoomFacts {
    name: String,
    /// A room with no walkable face: a sky, a backdrop or a cutscene set.
    backdrop: bool,
    lightmapped: bool,
    lamps: usize,
    /// Lamps that are not ambient-only.
    direct_lamps: usize,
    glows: usize,
    glow_m2: f32,
    /// Its glowing surfaces that look like lights (`Glow::lamp_like`).
    lamp_like_glows: usize,
}

#[derive(Serialize, Clone)]
struct Lamp {
    /// room, door, placeable or creature.
    owner: &'static str,
    model: String,
    node: String,
    dynamic_type: u32,
    ambient_only: bool,
    shadow: bool,
    affect_dynamic: bool,
    flare: bool,
    priority: u32,
    radius: f32,
    multiplier: f32,
    color: [f32; 3],
    position: [f32; 3],
    room: Option<String>,
    /// A room's lamp: whether it is inside its own room's box.
    inside_own_room: Option<bool>,
    /// Height above the floor of the room's box (metres).
    above_floor: Option<f32>,
    /// Mean lightmap brightness near it over the area's mean.
    near_ratio: Option<f32>,
    near_texels: usize,
}

#[derive(Serialize, Clone)]
struct Glow {
    owner: &'static str,
    model: String,
    node: String,
    texture: Option<String>,
    self_illum: [f32; 3],
    area_m2: f32,
    lightmapped: bool,
    /// The share of its texture's texels brighter than `BRIGHT`.
    bright_share: Option<f32>,
    /// It looks like a light: a mesh of a room or a placeable (not a
    /// door), lightmapped, or with at least `BRIGHT_SHARE` of its texture
    /// bright, and no larger than `SKY_M2`.
    lamp_like: bool,
    /// A mesh of a room with no walkable face (`RoomFacts::backdrop`), or one
    /// larger than `SKY_M2`.
    backdrop: bool,
    room: Option<String>,
    near_ratio: Option<f32>,
    near_texels: usize,
}

#[derive(Serialize, Clone)]
struct LayoutMesh {
    room: String,
    /// A mesh of a room with no walkable face (`RoomFacts::backdrop`).
    backdrop: bool,
    node: String,
    lightmap: String,
    triangles: usize,
    world_m2: f32,
    texels_per_metre: f32,
    /// The area-weighted 90th over 10th percentile of its triangles'
    /// texels per metre: 1 is even.
    density_spread: f32,
    /// Triangles that share a texel with another triangle.
    overlap_share: f32,
    /// Triangles whose lightmap coordinates have no area.
    collapsed_share: f32,
    /// Triangles smaller than a texel.
    small_share: f32,
    islands: usize,
    /// Its islands whose corners all sit on texel centres.
    islands_centred: usize,
    /// Its islands no more than 1 texel across either way: lit by the 4
    /// texels at their corners alone.
    islands_one_texel_wide: usize,
    /// Its islands whose footprint (the texels bilinear filtering reads
    /// for it) shares a texel with an island it does not meet on the
    /// surface.
    islands_bleeding: usize,
    /// Its islands that share no texel with such an island, but whose
    /// footprint has such an island's in a neighbouring texel.
    islands_spare_none: usize,
    /// Its islands less than 1/8 texel from such an island in the
    /// lightmap's coordinates, less than 0.9 texels, and between 0.9 and
    /// 1.1 texels.
    islands_touching: usize,
    islands_under_one: usize,
    islands_about_one: usize,
    /// The smallest gap in the coordinates between one of its islands and
    /// such an island (texels); `None` where none is within `COORD_REACH`.
    min_coord_gap: Option<f32>,
    /// The share of its lightmap no island's footprint holds.
    lightmap_unused: f32,
}

#[derive(Serialize, Clone)]
struct LightmapFacts {
    name: String,
    width: usize,
    height: usize,
    meshes: usize,
    unused_share: f32,
    /// Triangles with a lightmap coordinate outside 0 to 1.
    outside_share: f32,
}

// ---------------------------------------------------------------------------
// The walk.

fn survey_game(game: Game, install: &vfs::Install) -> Result<GameSurvey, String> {
    let index = DirIndex::of_install(install).map_err(|error| error.to_string())?;
    let base = Resolver::base(game, &index, TextureQuality::High).map_err(|e| e.to_string())?;
    let names = module_names(&index);
    let taken = AtomicUsize::new(0);
    let results = Mutex::new(Vec::new());
    let threads = std::thread::available_parallelism()
        .map_or(4, usize::from)
        .clamp(1, 8);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let at = taken.fetch_add(1, Ordering::Relaxed);
                    let Some(name) = names.get(at) else { return };
                    let surveyed = survey_module(&base, &index, name);
                    results
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((at, surveyed));
                }
            });
        }
    });
    let mut results = results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    results.sort_by_key(|(at, _)| *at);
    let mut survey = GameSurvey {
        game: game.key(),
        modules: names.len(),
        refused: Vec::new(),
        areas: Vec::new(),
    };
    let mut seen = BTreeSet::new();
    for (_, (areas, refused)) in results {
        survey.refused.extend(refused);
        for area in areas {
            if seen.insert(area.area.clone()) {
                survey.areas.push(area);
            }
        }
    }
    Ok(survey)
}

/// A model as the survey reads it: its IR, each light's header fields the
/// IR leaves out (priority, affect dynamic, flare), and its rest box.
pub(crate) struct Loaded {
    pub(crate) ir: ModelIr,
    pub(crate) extra: Vec<(u32, bool, bool)>,
    pub(crate) bounds: Option<[f32; 6]>,
}

fn light_extras(node: &Node, out: &mut Vec<(u32, bool, bool)>) {
    if let Some(light) = &node.light {
        out.push((light.priority, light.affect_dynamic, light.flare));
    }
    for child in &node.children {
        light_extras(child, out);
    }
}

/// A lightmap's size and its texels' brightness (rows in stored order, as
/// the renderer samples them).
pub(crate) struct Lightmap {
    width: usize,
    height: usize,
    brightness: Vec<f32>,
}

/// A texture's brightness at its first level no wider or taller than
/// `side` (its smallest level when none is).
fn read_brightness(resolver: &Resolver, name: &str, side: usize) -> Option<Lightmap> {
    let texture = TextureIr::from_texture(&resolver.texture(name)?, None)?;
    let size = |level: usize| {
        (
            (texture.width as usize >> level).max(1),
            (texture.height as usize >> level).max(1),
        )
    };
    let level = (0..texture.mips.len())
        .find(|&level| size(level).0.max(size(level).1) <= side)
        .or(texture.mips.len().checked_sub(1))?;
    let (width, height) = size(level);
    let data = &texture.mips[level];
    let rgba = match texture.encoding {
        Encoding::Rgba8 => (data.len() >= width * height * 4).then(|| data.clone())?,
        Encoding::Bc1 | Encoding::Bc1Holes => dxt::decode(data, width, height, false)?,
        Encoding::Bc3 => dxt::decode(data, width, height, true)?,
    };
    let brightness: Vec<f32> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .take(width * height)
        .map(|[r, g, b, _]| {
            (0.2126 * f32::from(*r) + 0.7152 * f32::from(*g) + 0.0722 * f32::from(*b)) / 255.0
        })
        .collect();
    (brightness.len() == width * height && width > 0 && height > 0).then_some(Lightmap {
        width,
        height,
        brightness,
    })
}

/// What a module's areas share: its models and lightmaps, read once.
pub(crate) struct Reader<'r> {
    pub(crate) resolver: &'r Resolver,
    module: String,
    models: BTreeMap<String, Option<Rc<Loaded>>>,
    lightmaps: BTreeMap<String, Option<Rc<Lightmap>>>,
    /// Each diffuse texture's share of bright texels (`bright_share`).
    bright: BTreeMap<String, Option<f32>>,
    refused: Vec<String>,
    /// The rows of `surfacemat.2da` one can walk on.
    walk: Vec<bool>,
}

impl<'r> Reader<'r> {
    pub(crate) fn new(resolver: &'r Resolver, module: &str) -> Reader<'r> {
        Reader {
            resolver,
            module: module.to_string(),
            models: BTreeMap::new(),
            lightmaps: BTreeMap::new(),
            bright: BTreeMap::new(),
            refused: Vec::new(),
            walk: resolver.two_da("surfacemat").map_or_else(Vec::new, |table| {
                collision_odyssey::Surfaces::from_2da(&table)
                    .rows
                    .iter()
                    .map(|row| row.walk)
                    .collect()
            }),
        }
    }

    pub(crate) fn model(&mut self, name: &str) -> Option<Rc<Loaded>> {
        if let Some(found) = self.models.get(name) {
            return found.clone();
        }
        let loaded = match viewer::parsed(self.resolver, name) {
            Ok(Some(parsed)) => {
                let ir = ModelIr::from_model(&parsed);
                let mut extra = Vec::new();
                light_extras(&parsed.root, &mut extra);
                if extra.len() != ir.lights.len() {
                    extra = vec![(0, false, false); ir.lights.len()];
                }
                let bounds = ir.rest_bounds();
                Some(Rc::new(Loaded { ir, extra, bounds }))
            }
            Ok(None) => None,
            Err(error) => {
                self.refused
                    .push(format!("{}: model {name}: {error}", self.module));
                None
            }
        };
        self.models.insert(name.to_string(), loaded.clone());
        loaded
    }

    pub(crate) fn lightmap(&mut self, name: &str) -> Option<Rc<Lightmap>> {
        let resolver = self.resolver;
        self.lightmaps
            .entry(name.to_string())
            .or_insert_with(|| read_brightness(resolver, name, usize::MAX).map(Rc::new))
            .clone()
    }

    /// The share of texture `name`'s texels brighter than `BRIGHT`, read
    /// at 64 texels a side at most.
    pub(crate) fn bright_share(&mut self, name: &str) -> Option<f32> {
        let resolver = self.resolver;
        *self.bright.entry(name.to_string()).or_insert_with(|| {
            let texture = read_brightness(resolver, name, 64)?;
            let bright = texture.brightness.iter().filter(|&&b| b > BRIGHT).count();
            Some(bright as f32 / texture.brightness.len() as f32)
        })
    }

    /// Whether room `name`'s walkmesh has a face one can walk on. A room
    /// without one is a sky, a backdrop or a cutscene set.
    fn walkable(&self, name: &str) -> bool {
        let Some(bytes) = self.resolver.read(name, ResType::WOK) else {
            return false;
        };
        let Ok(walkmesh) = Walkmesh::parse(&bytes) else {
            return false;
        };
        walkmesh.faces.iter().any(|face| {
            self.walk
                .get(face.material as usize)
                .copied()
                .unwrap_or(false)
        })
    }
}

fn survey_module(
    base: &Resolver,
    index: &DirIndex,
    name: &str,
) -> (Vec<AreaSurvey>, Vec<String>) {
    let resolver = match base.with_module(index, name) {
        Ok(resolver) => resolver,
        Err(error) => return (Vec::new(), vec![format!("{name}: {error}")]),
    };
    let module = match ModuleIr::build(name, &ResolverSource(&resolver)) {
        Ok(module) => module,
        Err(error) => return (Vec::new(), vec![format!("{name}: {error:?}")]),
    };
    let mut reader = Reader::new(&resolver, name);
    let areas = module
        .areas
        .iter()
        .map(|area| survey_area(&mut reader, area))
        .collect();
    (areas, reader.refused)
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Owner {
    Room(usize),
    Door,
    Placeable,
    Creature,
}

impl Owner {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Owner::Room(_) => "room",
            Owner::Door => "door",
            Owner::Placeable => "placeable",
            Owner::Creature => "creature",
        }
    }
}

pub(crate) struct Object {
    pub(crate) owner: Owner,
    pub(crate) model: Rc<Loaded>,
    pub(crate) frame: Matrix,
}

fn frame(offset: [f32; 3], facing: f32) -> Matrix {
    let (sin, cos) = facing.sin_cos();
    [
        [cos, -sin, 0.0, offset[0]],
        [sin, cos, 0.0, offset[1]],
        [0.0, 0.0, 1.0, offset[2]],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

pub(crate) fn apply(m: &Matrix, p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|row| m[row][0] * p[0] + m[row][1] * p[1] + m[row][2] * p[2] + m[row][3])
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

/// A room's box in the area (min x, y, z, max).
fn room_box(object: &Object) -> Option<[f32; 6]> {
    let b = object.model.bounds?;
    let low = apply(&object.frame, [b[0], b[1], b[2]]);
    let high = apply(&object.frame, [b[3], b[4], b[5]]);
    Some([low[0], low[1], low[2], high[0], high[1], high[2]])
}

fn in_box(b: &[f32; 6], p: [f32; 3]) -> bool {
    (0..3).all(|axis| p[axis] >= b[axis] - BOX_SLACK && p[axis] <= b[axis + 3] + BOX_SLACK)
}

/// The smallest room box that holds `p`.
pub(crate) fn room_at(boxes: &[Option<[f32; 6]>], p: [f32; 3]) -> Option<usize> {
    boxes
        .iter()
        .enumerate()
        .filter_map(|(room, b)| b.filter(|b| in_box(b, p)).map(|b| (room, b)))
        .min_by(|(_, a), (_, b)| volume(a).total_cmp(&volume(b)))
        .map(|(room, _)| room)
}

fn volume(b: &[f32; 6]) -> f32 {
    (b[3] - b[0]) * (b[4] - b[1]) * (b[5] - b[2])
}

/// A lightmap texel on a room's surface: where it is (area space), which
/// way the surface faces, how bright it is, and which object's mesh it is.
struct Sample {
    at: [f32; 3],
    normal: [f32; 3],
    brightness: f32,
    mesh: (usize, usize),
}

/// The samples in cells of 1 metre.
struct Grid {
    samples: Vec<Sample>,
    cells: HashMap<[i32; 3], Vec<u32>>,
}

impl Grid {
    fn new(samples: Vec<Sample>) -> Grid {
        let mut cells: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
        for (index, sample) in samples.iter().enumerate() {
            cells
                .entry(sample.at.map(|v| v.floor() as i32))
                .or_default()
                .push(index as u32);
        }
        Grid { samples, cells }
    }

    /// The mean brightness of the samples within `NEAR` of `at` that face
    /// it, leaving out mesh `skip`, and how many there are.
    fn near(&self, at: [f32; 3], skip: Option<(usize, usize)>) -> (f32, usize) {
        let reach = NEAR.ceil() as i32;
        let cell = at.map(|v| v.floor() as i32);
        let (mut sum, mut count) = (0.0, 0);
        for dx in -reach..=reach {
            for dy in -reach..=reach {
                for dz in -reach..=reach {
                    let key = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                    for &index in self.cells.get(&key).map_or(&[][..], Vec::as_slice) {
                        let sample = &self.samples[index as usize];
                        if Some(sample.mesh) == skip {
                            continue;
                        }
                        let to = sub(at, sample.at);
                        if length(to) <= NEAR && dot(to, sample.normal) > 0.0 {
                            sum += sample.brightness;
                            count += 1;
                        }
                    }
                }
            }
        }
        (if count > 0 { sum / count as f32 } else { 0.0 }, count)
    }
}

/// An area's rooms, doors, placeables and creatures as placed, with each
/// room's box. Shared with `kjk room-light-fit` (`room_light_fit`).
pub(crate) struct Placement {
    pub(crate) objects: Vec<Object>,
    /// Whether each room has a walkable face.
    pub(crate) walkable: Vec<bool>,
    pub(crate) boxes: Vec<Option<[f32; 6]>>,
    /// The boxes a lamp or a placeable's glow may be found in. A sky's box
    /// holds the whole area, so only rooms with a walkable face count.
    pub(crate) walk_boxes: Vec<Option<[f32; 6]>>,
    /// Room models the layout names that the install lacks.
    pub(crate) missing_rooms: Vec<String>,
}

pub(crate) fn place(reader: &mut Reader, area: &AreaIr) -> Placement {
    let mut objects: Vec<Object> = Vec::new();
    let mut room_objects: Vec<Option<usize>> = Vec::new();
    let mut missing_rooms = Vec::new();
    let walkable: Vec<bool> = (area.rooms.iter())
        .map(|room| reader.walkable(&room.model))
        .collect();
    for (room, layout) in area.rooms.iter().enumerate() {
        match reader.model(&layout.model) {
            Some(model) => {
                room_objects.push(Some(objects.len()));
                objects.push(Object {
                    owner: Owner::Room(room),
                    model,
                    frame: frame(layout.position, 0.0),
                });
            }
            None => {
                missing_rooms.push(layout.model.clone());
                room_objects.push(None);
            }
        }
    }
    for instance in &area.instances {
        let (owner, model) = match instance.kind {
            Kind::Door => (Owner::Door, instance.model.clone()),
            Kind::Placeable => (Owner::Placeable, instance.model.clone()),
            Kind::Creature => (
                Owner::Creature,
                asset_odyssey::creature::resolve(
                    &ResolverSource(reader.resolver),
                    &instance.template,
                )
                .ok()
                .map(|look| look.body),
            ),
            _ => continue,
        };
        let Some(model) = model.and_then(|name| reader.model(&name)) else {
            continue;
        };
        objects.push(Object {
            owner,
            model,
            frame: frame(instance.position, instance.facing),
        });
    }
    let boxes: Vec<Option<[f32; 6]>> = room_objects
        .iter()
        .map(|object| object.and_then(|object| room_box(&objects[object])))
        .collect();
    let walk_boxes: Vec<Option<[f32; 6]>> = (boxes.iter().zip(&walkable))
        .map(|(b, room)| b.filter(|_| *room))
        .collect();
    Placement {
        objects,
        walkable,
        boxes,
        walk_boxes,
        missing_rooms,
    }
}

fn survey_area(reader: &mut Reader, area: &AreaIr) -> AreaSurvey {
    let mut survey = AreaSurvey {
        module: reader.module.clone(),
        area: area.resref.clone(),
        ..AreaSurvey::default()
    };
    let Placement {
        objects,
        walkable,
        boxes,
        walk_boxes,
        missing_rooms,
        ..
    } = place(reader, area);
    survey.missing_rooms = missing_rooms;

    // The lamps.
    for object in &objects {
        let ir = &object.model.ir;
        for (light, &(priority, affect_dynamic, flare)) in ir.lights.iter().zip(&object.model.extra)
        {
            let world = multiply(&object.frame, &ir.nodes[light.node].world);
            let position = [world[0][3], world[1][3], world[2][3]];
            let room = match object.owner {
                Owner::Room(room) => Some(room),
                _ => room_at(&walk_boxes, position),
            };
            let own_box = match object.owner {
                Owner::Room(room) => boxes[room],
                _ => None,
            };
            survey.lamps.push(Lamp {
                owner: object.owner.name(),
                model: ir.name.clone(),
                node: ir.nodes[light.node].name.clone(),
                dynamic_type: light.dynamic_type,
                ambient_only: light.ambient_only,
                shadow: light.shadow,
                affect_dynamic,
                flare,
                priority,
                radius: light.radius,
                multiplier: light.multiplier,
                color: light.color,
                position,
                room: room.map(|room| area.rooms[room].model.clone()),
                inside_own_room: own_box.map(|b| in_box(&b, position)),
                above_floor: room.and_then(|room| boxes[room]).map(|b| position[2] - b[2]),
                near_ratio: None,
                near_texels: 0,
            });
        }
    }

    // The glowing surfaces, and where each sits.
    let mut glow_meshes: Vec<((usize, usize), [f32; 3])> = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        if object.owner == Owner::Creature {
            continue;
        }
        let ir = &object.model.ir;
        for (mesh_index, mesh) in ir.meshes.iter().enumerate() {
            if !mesh.self_illum.iter().any(|&v| v > 0.0) || mesh.skin.is_some() {
                continue;
            }
            let node = multiply(&object.frame, &ir.nodes[mesh.node].world);
            let (mut area_m2, mut centre) = (0.0, [0.0; 3]);
            for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
                let corner = |i: u32| apply(&node, mesh.positions[i as usize]);
                let (a, b, c) = (corner(a), corner(b), corner(c));
                let size = 0.5 * length(cross(sub(b, a), sub(c, a)));
                area_m2 += size;
                for axis in 0..3 {
                    centre[axis] += size * (a[axis] + b[axis] + c[axis]) / 3.0;
                }
            }
            if area_m2 > 0.0 {
                centre = centre.map(|v| v / area_m2);
            } else {
                centre = [node[0][3], node[1][3], node[2][3]];
            }
            let room = match object.owner {
                Owner::Room(room) => Some(room),
                _ => room_at(&walk_boxes, centre),
            };
            let lightmapped = mesh.lightmap.as_deref().is_some_and(|name| {
                mesh.uv2.len() == mesh.positions.len() && reader.lightmap(name).is_some()
            });
            let bright_share = mesh.texture.as_deref().and_then(|name| reader.bright_share(name));
            let backdrop = room.is_some_and(|room| !walkable[room]) || area_m2 > SKY_M2;
            let lamp_like = !backdrop
                && object.owner != Owner::Door
                && (lightmapped || bright_share.is_some_and(|share| share >= BRIGHT_SHARE));
            glow_meshes.push(((index, mesh_index), centre));
            survey.glows.push(Glow {
                owner: object.owner.name(),
                model: ir.name.clone(),
                node: ir.nodes[mesh.node].name.clone(),
                texture: mesh.texture.clone(),
                self_illum: mesh.self_illum,
                area_m2,
                lightmapped,
                bright_share,
                lamp_like,
                backdrop,
                room: room.map(|room| area.rooms[room].model.clone()),
                near_ratio: None,
                near_texels: 0,
            });
        }
    }

    // The lightmapped room meshes, by lightmap.
    let mut by_lightmap: BTreeMap<String, Vec<(usize, usize)>> = BTreeMap::new();
    let mut lightmapped_rooms = vec![false; area.rooms.len()];
    for (index, object) in objects.iter().enumerate() {
        let Owner::Room(room) = object.owner else {
            continue;
        };
        for (mesh_index, mesh) in object.model.ir.meshes.iter().enumerate() {
            let Some(name) = mesh.lightmap.as_deref() else {
                continue;
            };
            if mesh.skin.is_some()
                || mesh.uv2.len() != mesh.positions.len()
                || reader.lightmap(name).is_none()
            {
                continue;
            }
            lightmapped_rooms[room] = true;
            by_lightmap
                .entry(name.to_string())
                .or_default()
                .push((index, mesh_index));
        }
    }
    let mut samples = Vec::new();
    for (name, meshes) in &by_lightmap {
        let Some(map) = reader.lightmap(name) else {
            continue;
        };
        let (facts, layouts) = layout(name, &map, &objects, meshes, &mut samples);
        for (mesh, (index, mesh_index)) in layouts.into_iter().zip(meshes) {
            let object = &objects[*index];
            let ir = &object.model.ir;
            let Owner::Room(room) = object.owner else {
                continue;
            };
            survey.meshes.push(LayoutMesh {
                room: area.rooms[room].model.clone(),
                backdrop: !walkable[room],
                node: ir.nodes[ir.meshes[*mesh_index].node].name.clone(),
                ..mesh
            });
        }
        survey.lightmaps.push(facts);
    }

    // Near the bake.
    if !samples.is_empty() {
        let mean = samples.iter().map(|s| s.brightness).sum::<f32>() / samples.len() as f32;
        survey.lightmap_mean = Some(mean);
        let grid = Grid::new(samples);
        for lamp in &mut survey.lamps {
            let (near, count) = grid.near(lamp.position, None);
            lamp.near_texels = count;
            lamp.near_ratio = (count > 0 && mean > 0.0).then(|| near / mean);
        }
        for (glow, (mesh, centre)) in survey.glows.iter_mut().zip(&glow_meshes) {
            let (near, count) = grid.near(*centre, Some(*mesh));
            glow.near_texels = count;
            glow.near_ratio = (count > 0 && mean > 0.0).then(|| near / mean);
        }
    }

    // The rooms.
    for (room, layout) in area.rooms.iter().enumerate() {
        let name = &layout.model;
        let lamps: Vec<&Lamp> = survey
            .lamps
            .iter()
            .filter(|lamp| lamp.room.as_ref() == Some(name))
            .collect();
        let glows: Vec<&Glow> = survey
            .glows
            .iter()
            .filter(|glow| glow.room.as_ref() == Some(name) && !glow.backdrop)
            .collect();
        survey.rooms.push(RoomFacts {
            name: name.clone(),
            backdrop: !walkable[room],
            lightmapped: lightmapped_rooms[room],
            lamps: lamps.len(),
            direct_lamps: lamps.iter().filter(|lamp| !lamp.ambient_only).count(),
            glows: glows.len(),
            glow_m2: glows.iter().map(|glow| glow.area_m2).sum(),
            lamp_like_glows: glows.iter().filter(|glow| glow.lamp_like).count(),
        });
    }
    survey
}

// ---------------------------------------------------------------------------
// The lightmap layouts (7d).

struct Triangle {
    /// Index into the lightmap's meshes.
    mesh: usize,
    island: usize,
    /// Lightmap coordinates in texels.
    uv: [[f32; 2]; 3],
    world: [[f32; 3]; 3],
}

fn find(parent: &mut [usize], mut at: usize) -> usize {
    while parent[at] != at {
        parent[at] = parent[parent[at]];
        at = parent[at];
    }
    at
}

/// The signed distance (texels) of `p` from the edge `a` to `b`, positive
/// on the left.
pub(crate) fn edge(a: [f32; 2], b: [f32; 2], p: [f32; 2]) -> f32 {
    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
    let size = (ex * ex + ey * ey).sqrt();
    if size == 0.0 {
        return 0.0;
    }
    (ex * (p[1] - a[1]) - ey * (p[0] - a[0])) / size
}

/// Whether `p` is inside triangle `uv` (wound `sign`) by more than
/// `margin` texels.
pub(crate) fn inside(uv: &[[f32; 2]; 3], sign: f32, p: [f32; 2], margin: f32) -> bool {
    (0..3).all(|k| sign * edge(uv[k], uv[(k + 1) % 3], p) > margin)
}

/// One lightmap's layout: the meshes `meshes` (object, mesh) that use it,
/// rasterised on its texels. Pushes a sample for each texel a triangle
/// holds first.
fn layout(
    name: &str,
    map: &Lightmap,
    objects: &[Object],
    meshes: &[(usize, usize)],
    samples: &mut Vec<Sample>,
) -> (LightmapFacts, Vec<LayoutMesh>) {
    let (width, height) = (map.width, map.height);
    let mut triangles: Vec<Triangle> = Vec::new();
    let mut world_area = vec![0.0f32; meshes.len()];
    let mut island_count = 0usize;
    let mut island_mesh: Vec<usize> = Vec::new();
    let mut outside = 0usize;
    for (slot, &(index, mesh_index)) in meshes.iter().enumerate() {
        let object = &objects[index];
        let ir = &object.model.ir;
        let mesh = &ir.meshes[mesh_index];
        let node = multiply(&object.frame, &ir.nodes[mesh.node].world);
        // Islands: vertices joined by a triangle, or by the same lightmap
        // coordinate.
        let mut parent: Vec<usize> = (0..mesh.uv2.len()).collect();
        let mut same: HashMap<[i64; 2], usize> = HashMap::new();
        for (vertex, uv) in mesh.uv2.iter().enumerate() {
            let key = uv.map(|v| (f64::from(v) * 1e5).round() as i64);
            let first = *same.entry(key).or_insert(vertex);
            let (a, b) = (find(&mut parent, first), find(&mut parent, vertex));
            parent[a] = b;
        }
        for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
            let (a, b, c) = (a as usize, b as usize, c as usize);
            let root = find(&mut parent, a);
            for other in [b, c] {
                let other = find(&mut parent, other);
                parent[other] = root;
            }
        }
        let mut island_of: HashMap<usize, usize> = HashMap::new();
        for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
            let root = find(&mut parent, a as usize);
            let island = *island_of.entry(root).or_insert_with(|| {
                island_count += 1;
                island_mesh.push(slot);
                island_count - 1
            });
            let corner = |i: u32| {
                let uv = mesh.uv2[i as usize];
                [uv[0] * width as f32, uv[1] * height as f32]
            };
            let uv = [corner(a), corner(b), corner(c)];
            if [a, b, c].iter().any(|&i| {
                let uv = mesh.uv2[i as usize];
                !(-1e-3..=1.001).contains(&uv[0]) || !(-1e-3..=1.001).contains(&uv[1])
            }) {
                outside += 1;
            }
            let world = [a, b, c].map(|i| apply(&node, mesh.positions[i as usize]));
            world_area[slot] += 0.5 * length(cross(sub(world[1], world[0]), sub(world[2], world[0])));
            triangles.push(Triangle {
                mesh: slot,
                island,
                uv,
                world,
            });
        }
    }
    let mut seams = Seams {
        corners: vec![BTreeSet::new(); island_count],
        known: HashMap::new(),
    };
    for triangle in &triangles {
        for corner in &triangle.world {
            seams.corners[triangle.island].insert(corner.map(|v| (v * 1000.0).round() as i64));
        }
    }

    // Two grids over the lightmap. The texel grid holds one sample of the
    // surface for each texel centre a triangle holds. The fine grid splits
    // each texel into `fine` by `fine` cells, so that a triangle smaller
    // than a texel still shows where it lies. Two triangles overlap where
    // they hold the same fine cell.
    const NONE: u32 = u32::MAX;
    let fine = (1024 / width.max(height)).clamp(8, 16);
    let (fine_w, fine_h) = (width * fine, height * fine);
    let mut sampled = vec![false; width * height];
    let mut cells = vec![NONE; fine_w * fine_h];
    let mut overlap = vec![false; triangles.len()];
    let mut collapsed = vec![false; triangles.len()];
    let mut small = vec![false; triangles.len()];
    for (t, triangle) in triangles.iter().enumerate() {
        let uv = &triangle.uv;
        let twice = (uv[1][0] - uv[0][0]) * (uv[2][1] - uv[0][1])
            - (uv[2][0] - uv[0][0]) * (uv[1][1] - uv[0][1]);
        if twice.abs() < 1e-6 {
            collapsed[t] = true;
            continue;
        }
        small[t] = twice.abs() * 0.5 < 1.0;
        let sign = twice.signum();
        let low = |axis: usize| uv.iter().map(|p| p[axis]).fold(f32::MAX, f32::min);
        let high = |axis: usize| uv.iter().map(|p| p[axis]).fold(f32::MIN, f32::max);
        // A triangle spread over more than four copies of its lightmap is
        // not rasterised.
        if (high(0) - low(0)) * (high(1) - low(1)) > 4.0 * (width * height) as f32 {
            continue;
        }
        let normal = {
            let w = &triangle.world;
            let n = cross(sub(w[1], w[0]), sub(w[2], w[0]));
            let size = length(n);
            if size > 0.0 {
                n.map(|v| v / size)
            } else {
                [0.0, 0.0, 1.0]
            }
        };
        // The fine cells.
        let scale = fine as f32;
        let fine_uv = uv.map(|p| [p[0] * scale, p[1] * scale]);
        let (fx0, fx1) = (
            (low(0) * scale).floor() as i64,
            (high(0) * scale).ceil() as i64,
        );
        let (fy0, fy1) = (
            (low(1) * scale).floor() as i64,
            (high(1) * scale).ceil() as i64,
        );
        let mut held = false;
        for y in fy0..fy1 {
            for x in fx0..fx1 {
                let centre = [x as f32 + 0.5, y as f32 + 0.5];
                if !inside(&fine_uv, sign, centre, EDGE * scale) {
                    continue;
                }
                held = true;
                let at = (y.rem_euclid(fine_h as i64) as usize) * fine_w
                    + x.rem_euclid(fine_w as i64) as usize;
                if cells[at] == NONE {
                    cells[at] = t as u32;
                } else if cells[at] != t as u32 {
                    overlap[t] = true;
                    overlap[cells[at] as usize] = true;
                }
            }
        }
        // A triangle that holds no fine cell centre takes the cell of its
        // first corner, so it still touches its texel.
        if !held {
            let x = (fine_uv[0][0].floor() as i64).rem_euclid(fine_w as i64) as usize;
            let y = (fine_uv[0][1].floor() as i64).rem_euclid(fine_h as i64) as usize;
            if cells[y * fine_w + x] == NONE {
                cells[y * fine_w + x] = t as u32;
            }
        }
        // The texel centres, for the samples.
        for y in low(1).floor() as i64..high(1).ceil() as i64 {
            for x in low(0).floor() as i64..high(0).ceil() as i64 {
                let centre = [x as f32 + 0.5, y as f32 + 0.5];
                if !inside(uv, sign, centre, EDGE) {
                    continue;
                }
                let at = (y.rem_euclid(height as i64) as usize) * width
                    + x.rem_euclid(width as i64) as usize;
                if sampled[at] {
                    continue;
                }
                sampled[at] = true;
                // The texel's place on the surface, by its barycentric
                // weights.
                let weights: [f32; 3] = std::array::from_fn(|k| {
                    sign * edge(uv[(k + 1) % 3], uv[(k + 2) % 3], centre)
                });
                let total: f32 = weights.iter().sum();
                let w = &triangle.world;
                let place = std::array::from_fn(|axis| {
                    (0..3).map(|k| weights[k] * w[k][axis]).sum::<f32>() / total
                });
                samples.push(Sample {
                    at: place,
                    normal,
                    brightness: map.brightness[at],
                    mesh: meshes[triangle.mesh],
                });
            }
        }
    }

    // Each island's footprint: the texels that bilinear filtering reads for
    // it. A fine cell at `p` reads the four texels around `p`. A texel
    // that two islands read, where the islands do not meet on the surface,
    // carries light from one onto the other.
    let mut footprint = vec![NONE; width * height];
    let mut bleeds = vec![false; island_count];
    let mut used = 0usize;
    for (at, &cell) in cells.iter().enumerate() {
        if cell == NONE {
            continue;
        }
        let island = triangles[cell as usize].island;
        let px = ((at % fine_w) as f32 + 0.5) / fine as f32 - 0.5;
        let py = ((at / fine_w) as f32 + 0.5) / fine as f32 - 0.5;
        let (x0, y0) = (px.floor() as i64, py.floor() as i64);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let texel = ((y0 + dy).rem_euclid(height as i64) as usize) * width
                + (x0 + dx).rem_euclid(width as i64) as usize;
            let first = footprint[texel];
            if first == NONE {
                footprint[texel] = island as u32;
                used += 1;
            } else if first as usize != island && !seams.adjacent(first as usize, island) {
                bleeds[island] = true;
                bleeds[first as usize] = true;
            }
        }
    }

    // The spare texels between footprints: from each footprint's edge
    // texels, the nearest texel of an island that does not meet it on the
    // surface (no wrapping). 0 is a neighbouring texel.
    let mut spare: Vec<Option<u32>> = vec![None; island_count];
    let island_at = |x: i64, y: i64| -> Option<usize> {
        if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
            return None;
        }
        let island = footprint[(y as usize) * width + x as usize];
        (island != NONE).then_some(island as usize)
    };
    for y in 0..height as i64 {
        for x in 0..width as i64 {
            let Some(island) = island_at(x, y) else {
                continue;
            };
            let best = spare[island].map_or(GAP_REACH, |gap| i64::from(gap) + 1);
            'rings: for ring in 1..=best.min(GAP_REACH) {
                let ring_texels = (-ring..=ring)
                    .flat_map(|d| [(d, -ring), (d, ring)])
                    .chain((1 - ring..ring).flat_map(|d| [(-ring, d), (ring, d)]));
                for (dx, dy) in ring_texels {
                    if let Some(other) = island_at(x + dx, y + dy)
                        && other != island
                        && !seams.adjacent(island, other)
                    {
                        let gap = (ring - 1) as u32;
                        for found in [island, other] {
                            let old = spare[found].unwrap_or(u32::MAX);
                            spare[found] = Some(old.min(gap));
                        }
                        break 'rings;
                    }
                }
            }
        }
    }

    // The gap in the coordinates themselves (texels, to a fraction): the
    // nearest triangle of an island that does not meet this one on the
    // surface, looked for up to `COORD_REACH` texels away. This gap grows
    // with the lightmap's size.
    let mut coord_gap: Vec<Option<f32>> = vec![None; island_count];
    let boxes: Vec<[f32; 4]> = triangles
        .iter()
        .map(|t| {
            let xs = t.uv.map(|p| p[0]);
            let ys = t.uv.map(|p| p[1]);
            [
                xs.iter().copied().fold(f32::MAX, f32::min),
                ys.iter().copied().fold(f32::MAX, f32::min),
                xs.iter().copied().fold(f32::MIN, f32::max),
                ys.iter().copied().fold(f32::MIN, f32::max),
            ]
        })
        .collect();
    let mut buckets: HashMap<[i64; 2], Vec<u32>> = HashMap::new();
    for (t, b) in boxes.iter().enumerate() {
        if (b[2] - b[0]) * (b[3] - b[1]) > (width * height) as f32 {
            continue;
        }
        for by in b[1].floor() as i64..=b[3].floor() as i64 {
            for bx in b[0].floor() as i64..=b[2].floor() as i64 {
                buckets.entry([bx, by]).or_default().push(t as u32);
            }
        }
    }
    let mut seen = vec![u32::MAX; triangles.len()];
    let reach = COORD_REACH.ceil() as i64;
    for (t, b) in boxes.iter().enumerate() {
        if (b[2] - b[0]) * (b[3] - b[1]) > (width * height) as f32 {
            continue;
        }
        let island = triangles[t].island;
        for by in b[1].floor() as i64 - reach..=b[3].floor() as i64 + reach {
            for bx in b[0].floor() as i64 - reach..=b[2].floor() as i64 + reach {
                for &u in buckets.get(&[bx, by]).map_or(&[][..], Vec::as_slice) {
                    let u = u as usize;
                    if u <= t || seen[u] == t as u32 {
                        continue;
                    }
                    seen[u] = t as u32;
                    let other = triangles[u].island;
                    if other == island {
                        continue;
                    }
                    let c = &boxes[u];
                    let apart = (c[0] - b[2]).max(b[0] - c[2]).max(c[1] - b[3]).max(b[1] - c[3]);
                    if apart > COORD_REACH {
                        continue;
                    }
                    if seams.adjacent(island, other) {
                        continue;
                    }
                    let distance = triangle_distance(&triangles[t].uv, &triangles[u].uv);
                    if distance <= COORD_REACH {
                        for found in [island, other] {
                            let old = coord_gap[found].unwrap_or(f32::MAX);
                            coord_gap[found] = Some(old.min(distance));
                        }
                    }
                }
            }
        }
    }

    // Whether an island's corners all sit on texel centres.
    let mut centred = vec![true; island_count];
    let mut extent = vec![[f32::MAX, f32::MAX, f32::MIN, f32::MIN]; island_count];
    for t in &triangles {
        let e = &mut extent[t.island];
        for p in &t.uv {
            e[0] = e[0].min(p[0]);
            e[1] = e[1].min(p[1]);
            e[2] = e[2].max(p[0]);
            e[3] = e[3].max(p[1]);
        }
        let on_centre = |v: f32| ((v - 0.5) - (v - 0.5).round()).abs() < 0.02;
        if !t.uv.iter().all(|p| on_centre(p[0]) && on_centre(p[1])) {
            centred[t.island] = false;
        }
    }

    let unused = 1.0 - used as f32 / (width * height) as f32;
    let facts = LightmapFacts {
        name: name.to_string(),
        width,
        height,
        meshes: meshes.len(),
        unused_share: unused,
        outside_share: outside as f32 / triangles.len().max(1) as f32,
    };
    let mut layouts = Vec::new();
    for (slot, &mesh_m2) in world_area.iter().enumerate() {
        let mine: Vec<usize> = (0..triangles.len())
            .filter(|&t| triangles[t].mesh == slot)
            .collect();
        let count = mine.len().max(1) as f32;
        let mut densities: Vec<(f32, f32)> = Vec::new();
        let mut texel_area = 0.0;
        for &t in &mine {
            let (uv, w) = (&triangles[t].uv, &triangles[t].world);
            let uv_area = 0.5
                * ((uv[1][0] - uv[0][0]) * (uv[2][1] - uv[0][1])
                    - (uv[2][0] - uv[0][0]) * (uv[1][1] - uv[0][1]))
                    .abs();
            let world = 0.5 * length(cross(sub(w[1], w[0]), sub(w[2], w[0])));
            texel_area += uv_area;
            if world > 1e-6 {
                densities.push(((uv_area / world).sqrt(), world));
            }
        }
        let islands: Vec<usize> = (0..island_count)
            .filter(|&island| island_mesh[island] == slot)
            .collect();
        let count_of = |test: &dyn Fn(usize) -> bool| islands.iter().filter(|&&i| test(i)).count();
        layouts.push(LayoutMesh {
            room: String::new(),
            backdrop: false,
            node: String::new(),
            lightmap: name.to_string(),
            triangles: mine.len(),
            world_m2: mesh_m2,
            texels_per_metre: if mesh_m2 > 0.0 {
                (texel_area / mesh_m2).sqrt()
            } else {
                0.0
            },
            density_spread: {
                let p10 = weighted_percentile(&mut densities.clone(), 0.1);
                let p90 = weighted_percentile(&mut densities, 0.9);
                if p10 > 0.0 { p90 / p10 } else { 0.0 }
            },
            overlap_share: mine.iter().filter(|&&t| overlap[t]).count() as f32 / count,
            collapsed_share: mine.iter().filter(|&&t| collapsed[t]).count() as f32 / count,
            small_share: mine.iter().filter(|&&t| small[t]).count() as f32 / count,
            islands: islands.len(),
            islands_centred: count_of(&|i| centred[i]),
            islands_one_texel_wide: count_of(&|i| {
                extent[i][2] - extent[i][0] < 1.01 && extent[i][3] - extent[i][1] < 1.01
            }),
            islands_bleeding: count_of(&|i| bleeds[i]),
            islands_spare_none: count_of(&|i| !bleeds[i] && spare[i] == Some(0)),
            islands_touching: count_of(&|i| coord_gap[i].is_some_and(|gap| gap < 0.125)),
            islands_under_one: count_of(&|i| coord_gap[i].is_some_and(|gap| gap < 0.9)),
            islands_about_one: count_of(&|i| {
                coord_gap[i].is_some_and(|gap| (0.9..1.1).contains(&gap))
            }),
            min_coord_gap: islands
                .iter()
                .filter_map(|&i| coord_gap[i])
                .fold(None, |best: Option<f32>, gap| Some(best.map_or(gap, |b| b.min(gap)))),
            lightmap_unused: unused,
        });
    }
    (facts, layouts)
}

/// The shortest distance between two triangles in the plane: 0 where they
/// overlap.
fn triangle_distance(a: &[[f32; 2]; 3], b: &[[f32; 2]; 3]) -> f32 {
    let inside = |tri: &[[f32; 2]; 3], p: [f32; 2]| {
        let side = |k: usize| {
            let (u, v) = (tri[k], tri[(k + 1) % 3]);
            (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])
        };
        let (s0, s1, s2) = (side(0), side(1), side(2));
        (s0 >= 0.0 && s1 >= 0.0 && s2 >= 0.0) || (s0 <= 0.0 && s1 <= 0.0 && s2 <= 0.0)
    };
    if a.iter().any(|&p| inside(b, p)) || b.iter().any(|&p| inside(a, p)) {
        return 0.0;
    }
    let to_segment = |p: [f32; 2], u: [f32; 2], v: [f32; 2]| {
        let (ex, ey) = (v[0] - u[0], v[1] - u[1]);
        let size = ex * ex + ey * ey;
        let t = if size > 0.0 {
            (((p[0] - u[0]) * ex + (p[1] - u[1]) * ey) / size).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (dx, dy) = (u[0] + t * ex - p[0], u[1] + t * ey - p[1]);
        (dx * dx + dy * dy).sqrt()
    };
    let crosses = |u: [f32; 2], v: [f32; 2], p: [f32; 2], q: [f32; 2]| {
        let side = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
            (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
        };
        side(u, v, p) * side(u, v, q) < 0.0 && side(p, q, u) * side(p, q, v) < 0.0
    };
    let mut best = f32::MAX;
    for i in 0..3 {
        let (u, v) = (a[i], a[(i + 1) % 3]);
        for j in 0..3 {
            let (p, q) = (b[j], b[(j + 1) % 3]);
            if crosses(u, v, p, q) {
                return 0.0;
            }
            best = best
                .min(to_segment(u, p, q))
                .min(to_segment(p, u, v));
        }
    }
    best
}

/// Which islands of a lightmap meet on the surface: two islands meet when
/// they share a corner (to the millimetre).
struct Seams {
    corners: Vec<BTreeSet<[i64; 3]>>,
    known: HashMap<(usize, usize), bool>,
}

impl Seams {
    fn adjacent(&mut self, a: usize, b: usize) -> bool {
        let key = (a.min(b), a.max(b));
        if let Some(&known) = self.known.get(&key) {
            return known;
        }
        let (small, large) = if self.corners[a].len() <= self.corners[b].len() {
            (&self.corners[a], &self.corners[b])
        } else {
            (&self.corners[b], &self.corners[a])
        };
        let meets = small.iter().any(|corner| large.contains(corner));
        self.known.insert(key, meets);
        meets
    }
}

/// The value at `share` of the total weight, from (value, weight) pairs.
fn weighted_percentile(values: &mut [(f32, f32)], share: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f32 = values.iter().map(|v| v.1).sum();
    let mut seen = 0.0;
    for &(value, weight) in values.iter() {
        seen += weight;
        if seen >= share * total {
            return value;
        }
    }
    values[values.len() - 1].0
}

fn percentile(values: &[f32], share: f32) -> f32 {
    let mut pairs: Vec<(f32, f32)> = values.iter().map(|&v| (v, 1.0)).collect();
    weighted_percentile(&mut pairs, share)
}

// ---------------------------------------------------------------------------
// The readable report.

fn report(surveys: &[GameSurvey]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "# Room lights survey\n");
    let _ = writeln!(
        out,
        "Brightness is read from the lightmaps as stored (gamma encoded). \
         A ratio is the mean brightness of the lightmap texels within {NEAR} m \
         that face a lamp or a glowing surface, over the mean of all the area's \
         lightmap texels.\n"
    );
    for survey in surveys {
        report_game(&mut out, survey);
    }
    out
}

fn share(part: usize, whole: usize) -> String {
    if whole == 0 {
        "-".into()
    } else {
        format!("{:.0}%", 100.0 * part as f32 / whole as f32)
    }
}

/// A lamp's owner, dynamic type, ambient-only flag and shadow flag.
type LampKind<'a> = (&'a str, u32, bool, bool);

/// A line of the layout table: its label, and how many of a mesh's islands
/// it counts.
type IslandRow = (&'static str, fn(&LayoutMesh) -> usize);

fn report_game(out: &mut String, survey: &GameSurvey) {
    use std::fmt::Write as _;
    let game = survey.game;
    // The tables leave out the rooms with no walkable face (skies, backdrops and
    // cutscene sets), their glowing meshes and their layouts.
    let backdrop_glows: Vec<&Glow> = (survey.areas.iter())
        .flat_map(|area| &area.glows)
        .filter(|glow| glow.backdrop)
        .collect();
    let backdrop_meshes = (survey.areas.iter())
        .flat_map(|area| &area.meshes)
        .filter(|mesh| mesh.backdrop)
        .count();
    let filtered: Vec<AreaSurvey> = (survey.areas.iter())
        .map(|area| {
            let meshes: Vec<LayoutMesh> =
                area.meshes.iter().filter(|mesh| !mesh.backdrop).cloned().collect();
            let names: BTreeSet<&str> = meshes.iter().map(|mesh| mesh.lightmap.as_str()).collect();
            AreaSurvey {
                module: area.module.clone(),
                area: area.area.clone(),
                rooms: area.rooms.iter().filter(|room| !room.backdrop).cloned().collect(),
                lamps: area.lamps.clone(),
                glows: area.glows.iter().filter(|glow| !glow.backdrop).cloned().collect(),
                lightmaps: (area.lightmaps.iter())
                    .filter(|map| names.contains(map.name.as_str()))
                    .cloned()
                    .collect(),
                meshes,
                missing_rooms: area.missing_rooms.clone(),
                lightmap_mean: area.lightmap_mean,
            }
        })
        .collect();
    let areas = &filtered;
    let rooms: Vec<&RoomFacts> = areas.iter().flat_map(|area| &area.rooms).collect();
    let lit: Vec<&&RoomFacts> = rooms.iter().filter(|room| room.lightmapped).collect();
    let lamps: Vec<&Lamp> = areas.iter().flat_map(|area| &area.lamps).collect();
    let glows: Vec<&Glow> = areas.iter().flat_map(|area| &area.glows).collect();
    let _ = writeln!(out, "## {game}\n");
    let _ = writeln!(
        out,
        "{} modules, {} areas, {} refused. {} rooms, {} with a lightmap. {} lamps. {} glowing meshes.\n",
        survey.modules,
        areas.len(),
        survey.refused.len(),
        rooms.len(),
        lit.len(),
        lamps.len(),
        glows.len()
    );
    let _ = writeln!(
        out,
        "The tables count only rooms with a walkable face. The rooms without one hold {} more glowing meshes ({:.0} m², mostly skies) and {backdrop_meshes} more lightmapped meshes.\n",
        backdrop_glows.len(),
        backdrop_glows.iter().map(|glow| glow.area_m2).sum::<f32>()
    );
    for line in &survey.refused {
        let _ = writeln!(out, "- refused {line}");
    }
    if !survey.refused.is_empty() {
        let _ = writeln!(out);
    }

    // Lamps by kind.
    let _ = writeln!(out, "### {game}: lamps by kind\n");
    let _ = writeln!(
        out,
        "| owner | type | ambient only | shadow | lamps | areas | radius p10 / median / p90 | priorities | in own room's box | height median | ratio median | ratio over 1.5 | no texel near |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    let mut kinds: BTreeMap<LampKind, Vec<(&Lamp, &str)>> = BTreeMap::new();
    for area in areas {
        for lamp in &area.lamps {
            kinds
                .entry((lamp.owner, lamp.dynamic_type, lamp.ambient_only, lamp.shadow))
                .or_default()
                .push((lamp, &area.area));
        }
    }
    for ((owner, kind, ambient, shadow), list) in &kinds {
        let radii: Vec<f32> = list.iter().map(|(lamp, _)| lamp.radius).collect();
        let mut priorities: BTreeMap<u32, usize> = BTreeMap::new();
        for (lamp, _) in list {
            *priorities.entry(lamp.priority).or_insert(0) += 1;
        }
        let priorities: Vec<String> = priorities
            .iter()
            .map(|(p, n)| format!("{p}:{n}"))
            .take(4)
            .collect();
        let own: Vec<bool> = list
            .iter()
            .filter_map(|(lamp, _)| lamp.inside_own_room)
            .collect();
        let heights: Vec<f32> = list.iter().filter_map(|(lamp, _)| lamp.above_floor).collect();
        let ratios: Vec<f32> = list.iter().filter_map(|(lamp, _)| lamp.near_ratio).collect();
        let area_count = list
            .iter()
            .map(|(_, area)| *area)
            .collect::<BTreeSet<_>>()
            .len();
        let _ = writeln!(
            out,
            "| {owner} | {kind} | {} | {} | {} | {area_count} | {:.1} / {:.1} / {:.1} | {} | {} | {} | {} | {} | {} |",
            if *ambient { "yes" } else { "no" },
            if *shadow { "yes" } else { "no" },
            list.len(),
            percentile(&radii, 0.1),
            percentile(&radii, 0.5),
            percentile(&radii, 0.9),
            priorities.join(" "),
            if own.is_empty() {
                "-".into()
            } else {
                share(own.iter().filter(|&&b| b).count(), own.len())
            },
            if heights.is_empty() {
                "-".into()
            } else {
                format!("{:.1} m", percentile(&heights, 0.5))
            },
            if ratios.is_empty() {
                "-".into()
            } else {
                format!("{:.2}", percentile(&ratios, 0.5))
            },
            share(ratios.iter().filter(|&&r| r > 1.5).count(), ratios.len()),
            share(list.len() - ratios.len(), list.len()),
        );
    }
    let _ = writeln!(out);

    // The glowing surfaces.
    let _ = writeln!(out, "### {game}: glowing surfaces\n");
    let _ = writeln!(
        out,
        "| owner | lightmapped | looks like a light | meshes | total m² | median m² | ratio median | ratio over 1.5 |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
    let mut glow_kinds: BTreeMap<(&str, bool, bool), Vec<&Glow>> = BTreeMap::new();
    for glow in &glows {
        glow_kinds
            .entry((glow.owner, glow.lightmapped, glow.lamp_like))
            .or_default()
            .push(glow);
    }
    for ((owner, lightmapped, lamp_like), list) in &glow_kinds {
        let sizes: Vec<f32> = list.iter().map(|glow| glow.area_m2).collect();
        let ratios: Vec<f32> = list.iter().filter_map(|glow| glow.near_ratio).collect();
        let _ = writeln!(
            out,
            "| {owner} | {} | {} | {} | {:.0} | {:.2} | {} | {} |",
            if *lightmapped { "yes" } else { "no" },
            if *lamp_like { "yes" } else { "no" },
            list.len(),
            sizes.iter().sum::<f32>(),
            percentile(&sizes, 0.5),
            if ratios.is_empty() {
                "-".into()
            } else {
                format!("{:.2}", percentile(&ratios, 0.5))
            },
            share(ratios.iter().filter(|&&r| r > 1.5).count(), ratios.len()),
        );
    }
    let _ = writeln!(out);

    // Rooms without lamps.
    let no_lamp: Vec<&&&RoomFacts> = lit.iter().filter(|room| room.lamps == 0).collect();
    let no_direct = lit.iter().filter(|room| room.direct_lamps == 0).count();
    let glow_no_lamp = no_lamp.iter().filter(|room| room.glows > 0).count();
    let dark = no_lamp.iter().filter(|room| room.glows == 0).count();
    let like_no_lamp = no_lamp.iter().filter(|room| room.lamp_like_glows > 0).count();
    let areas_like_no_lamp = areas
        .iter()
        .filter(|area| {
            area.rooms
                .iter()
                .any(|room| room.lightmapped && room.lamps == 0 && room.lamp_like_glows > 0)
        })
        .count();
    let areas_glow_no_lamp = areas
        .iter()
        .filter(|area| {
            area.rooms
                .iter()
                .any(|room| room.lightmapped && room.lamps == 0 && room.glows > 0)
        })
        .count();
    let areas_no_lamp = areas
        .iter()
        .filter(|area| {
            area.rooms
                .iter()
                .any(|room| room.lightmapped && room.lamps == 0)
        })
        .count();
    let areas_lit = areas
        .iter()
        .filter(|area| area.rooms.iter().any(|room| room.lightmapped))
        .count();
    let _ = writeln!(out, "### {game}: lightmapped rooms and their lights\n");
    let _ = writeln!(out, "| | rooms | share of lightmapped rooms | areas |");
    let _ = writeln!(out, "|---|---|---|---|");
    let _ = writeln!(
        out,
        "| lightmapped | {} | 100% | {areas_lit} |",
        lit.len()
    );
    let _ = writeln!(
        out,
        "| no lamp | {} | {} | {areas_no_lamp} |",
        no_lamp.len(),
        share(no_lamp.len(), lit.len())
    );
    let _ = writeln!(
        out,
        "| no lamp but an ambient-only one | {} | {} | |",
        no_direct - no_lamp.len(),
        share(no_direct - no_lamp.len(), lit.len())
    );
    let _ = writeln!(
        out,
        "| no lamp, a glowing surface | {glow_no_lamp} | {} | {areas_glow_no_lamp} |",
        share(glow_no_lamp, lit.len())
    );
    let _ = writeln!(
        out,
        "| no lamp, a glowing surface that looks like a light | {like_no_lamp} | {} | {areas_like_no_lamp} |",
        share(like_no_lamp, lit.len())
    );
    let _ = writeln!(
        out,
        "| no lamp, no glowing surface | {dark} | {} | |",
        share(dark, lit.len())
    );
    let _ = writeln!(out);

    // Textures that glow most.
    for (title, lamp_like) in [("all glowing surfaces", false), ("surfaces that look like a light", true)] {
        let mut textures: BTreeMap<&str, (f32, usize, BTreeSet<&str>)> = BTreeMap::new();
        for area in areas {
            for glow in area.glows.iter().filter(|glow| glow.lamp_like || !lamp_like) {
                let entry = textures
                    .entry(glow.texture.as_deref().unwrap_or("(none)"))
                    .or_default();
                entry.0 += glow.area_m2;
                entry.1 += 1;
                entry.2.insert(&area.area);
            }
        }
        let mut textures: Vec<_> = textures.into_iter().collect();
        textures.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
        let _ = writeln!(out, "### {game}: textures that glow most, by area, {title}
");
        let _ = writeln!(out, "| texture | m² | meshes | areas |");
        let _ = writeln!(out, "|---|---|---|---|");
        for (name, (m2, count, in_areas)) in textures.iter().take(12) {
            let _ = writeln!(out, "| {name} | {m2:.0} | {count} | {} |", in_areas.len());
        }
        let _ = writeln!(out);
    }

    // Areas.
    let mut ranked: Vec<&AreaSurvey> = areas.iter().collect();
    let dark_rooms = |area: &AreaSurvey| {
        area.rooms
            .iter()
            .filter(|room| room.lightmapped && room.lamps == 0)
            .count()
    };
    ranked.sort_by_key(|area| std::cmp::Reverse(dark_rooms(area)));
    let _ = writeln!(
        out,
        "### {game}: areas with the most lightmapped rooms without a lamp\n"
    );
    let _ = writeln!(
        out,
        "| area | rooms | lightmapped | no lamp | no lamp, a glow | no lamp, a glow like a light | no lamp, no glow | lamps | glows |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|");
    for area in ranked.iter().take(15) {
        let lit = area.rooms.iter().filter(|room| room.lightmapped);
        let (mut count, mut none, mut glowing, mut like, mut dark) = (0, 0, 0, 0, 0);
        for room in lit {
            count += 1;
            if room.lamps == 0 {
                none += 1;
                if room.lamp_like_glows > 0 {
                    like += 1;
                }
                if room.glows > 0 {
                    glowing += 1;
                } else {
                    dark += 1;
                }
            }
        }
        let _ = writeln!(
            out,
            "| {} | {} | {count} | {none} | {glowing} | {like} | {dark} | {} | {} |",
            area.area,
            area.rooms.len(),
            area.lamps.len(),
            area.glows.len()
        );
    }
    let _ = writeln!(out);

    // The layouts.
    let meshes: Vec<&LayoutMesh> = areas.iter().flat_map(|area| &area.meshes).collect();
    let maps: Vec<&LightmapFacts> = areas.iter().flat_map(|area| &area.lightmaps).collect();
    let mut density: Vec<(f32, f32)> = meshes
        .iter()
        .map(|mesh| (mesh.texels_per_metre, mesh.world_m2))
        .collect();
    let spreads: Vec<f32> = meshes
        .iter()
        .filter(|mesh| mesh.density_spread > 0.0)
        .map(|mesh| mesh.density_spread)
        .collect();
    let triangles: usize = meshes.iter().map(|mesh| mesh.triangles).sum();
    let count_of = |share: fn(&LayoutMesh) -> f32| -> f32 {
        meshes
            .iter()
            .map(|mesh| share(mesh) * mesh.triangles as f32)
            .sum::<f32>()
    };
    let overlapping = count_of(|mesh| mesh.overlap_share);
    let collapsed = count_of(|mesh| mesh.collapsed_share);
    let small = count_of(|mesh| mesh.small_share);
    let unused: Vec<f32> = maps.iter().map(|map| map.unused_share).collect();
    let sizes: Vec<f32> = maps
        .iter()
        .map(|map| (map.width * map.height) as f32)
        .collect();
    let _ = writeln!(out, "### {game}: lightmap layouts\n");
    let _ = writeln!(out, "| measure | value |");
    let _ = writeln!(out, "|---|---|");
    let _ = writeln!(
        out,
        "| lightmapped room meshes, lightmaps, triangles | {}, {}, {triangles} |",
        meshes.len(),
        maps.len()
    );
    let _ = writeln!(
        out,
        "| lightmap side, median (texels) | {:.0} |",
        percentile(&sizes, 0.5).sqrt()
    );
    let _ = writeln!(
        out,
        "| texels per metre, by surface area, p10 / median / p90 | {:.1} / {:.1} / {:.1} |",
        weighted_percentile(&mut density.clone(), 0.1),
        weighted_percentile(&mut density.clone(), 0.5),
        weighted_percentile(&mut density, 0.9)
    );
    let per_mesh: Vec<f32> = meshes.iter().map(|mesh| mesh.texels_per_metre).collect();
    let _ = writeln!(
        out,
        "| texels per metre, by mesh, p10 / median / p90 | {:.1} / {:.1} / {:.1} |",
        percentile(&per_mesh, 0.1),
        percentile(&per_mesh, 0.5),
        percentile(&per_mesh, 0.9)
    );
    let _ = writeln!(
        out,
        "| density spread inside a mesh (p90 over p10), median / p90 | {:.2} / {:.2} |",
        percentile(&spreads, 0.5),
        percentile(&spreads, 0.9)
    );
    let _ = writeln!(
        out,
        "| triangles whose lightmap area overlaps another triangle | {} |",
        share(overlapping.round() as usize, triangles)
    );
    let _ = writeln!(
        out,
        "| meshes with any overlapping triangle | {} |",
        share(
            meshes.iter().filter(|mesh| mesh.overlap_share > 0.0).count(),
            meshes.len()
        )
    );
    let _ = writeln!(
        out,
        "| triangles with no lightmap area | {} |",
        share(collapsed.round() as usize, triangles)
    );
    let _ = writeln!(
        out,
        "| triangles smaller than a texel | {} |",
        share(small.round() as usize, triangles)
    );
    let islands: usize = meshes.iter().map(|mesh| mesh.islands).sum();
    let _ = writeln!(
        out,
        "| islands, and islands per mesh, median | {islands}, {:.0} |",
        percentile(
            &meshes.iter().map(|mesh| mesh.islands as f32).collect::<Vec<_>>(),
            0.5
        )
    );
    let island_rows: [IslandRow; 7] = [
        ("islands whose corners all sit on texel centres", |mesh| mesh.islands_centred),
        ("islands no more than 1 texel across, lit by their 4 corner texels", |mesh| mesh.islands_one_texel_wide),
        ("islands whose footprint shares a texel with an unrelated island", |mesh| mesh.islands_bleeding),
        ("islands whose footprint has an unrelated one in the next texel, sharing none", |mesh| mesh.islands_spare_none),
        ("islands less than 1/8 texel from an unrelated island, in the coordinates", |mesh| mesh.islands_touching),
        ("islands less than 0.9 texels from an unrelated island, in the coordinates", |mesh| mesh.islands_under_one),
        ("islands 0.9 to 1.1 texels from an unrelated island, in the coordinates", |mesh| mesh.islands_about_one),
    ];
    for (label, count) in island_rows {
        let _ = writeln!(
            out,
            "| {label} | {} |",
            share(meshes.iter().map(|mesh| count(mesh)).sum(), islands)
        );
    }
    let _ = writeln!(
        out,
        "| lightmap left unused, p10 / median / p90 | {:.0}% / {:.0}% / {:.0}% |",
        100.0 * percentile(&unused, 0.1),
        100.0 * percentile(&unused, 0.5),
        100.0 * percentile(&unused, 0.9)
    );
    let _ = writeln!(
        out,
        "| lightmaps with a coordinate outside 0 to 1 | {} |",
        share(
            maps.iter().filter(|map| map.outside_share > 0.0).count(),
            maps.len()
        )
    );
    let _ = writeln!(out);

    let mut worst: Vec<(&AreaSurvey, usize, f32, f32, f32, f32, f32)> = areas
        .iter()
        .filter(|area| !area.meshes.is_empty())
        .map(|area| {
            let triangles: usize = area.meshes.iter().map(|mesh| mesh.triangles).sum();
            let islands: usize = area.meshes.iter().map(|mesh| mesh.islands).sum();
            let overlap: f32 = area
                .meshes
                .iter()
                .map(|mesh| mesh.overlap_share * mesh.triangles as f32)
                .sum::<f32>()
                / triangles.max(1) as f32;
            let island_share = |count: fn(&LayoutMesh) -> usize| {
                area.meshes.iter().map(count).sum::<usize>() as f32 / islands.max(1) as f32
            };
            let unused: Vec<f32> = area.lightmaps.iter().map(|map| map.unused_share).collect();
            let density: Vec<f32> = area.meshes.iter().map(|mesh| mesh.texels_per_metre).collect();
            (
                area,
                islands,
                overlap,
                island_share(|mesh| mesh.islands_bleeding),
                island_share(|mesh| mesh.islands_under_one),
                percentile(&unused, 0.5),
                percentile(&density, 0.5),
            )
        })
        .collect();
    worst.sort_by(|a, b| (b.2 + b.3 + b.4).total_cmp(&(a.2 + a.3 + a.4)));
    let _ = writeln!(out, "### {game}: areas with the worst layouts
");
    let _ = writeln!(
        out,
        "| area | meshes | islands | triangles overlapping | islands sharing a texel | islands under 0.9 texels apart | unused, median | texels per metre, median by mesh |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|");
    for (area, islands, overlap, bleeding, close, unused, density) in worst.iter().take(15) {
        let _ = writeln!(
            out,
            "| {} | {} | {islands} | {:.1}% | {:.1}% | {:.1}% | {:.0}% | {:.1} |",
            area.area,
            area.meshes.len(),
            100.0 * overlap,
            100.0 * bleeding,
            100.0 * close,
            100.0 * unused,
            density
        );
    }
    let _ = writeln!(out);
}

/// The default folder: `target/room-lights` under the current folder.
pub fn default_out() -> PathBuf {
    PathBuf::from("target").join("room-lights")
}
