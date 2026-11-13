//! Light probes: characters, placeables and doors lit by the room around
//! them under the options' Enhanced lighting (`docs/LIGHTING-PLAN.md`,
//! step 5). Classic lighting does not change.
//!
//! A probe is a point about 1 m above a room's walkable floor. Probes sit on
//! a grid `SPACING` apart, room by room. A probe holds six colours, the
//! light arriving on a surface that faces east, west, up, down, south or
//! north. The shader weighs the six by a surface's normal.
//!
//! The probes are made when an area loads (`bake`), from the rooms'
//! lightmaps alone. A lightmap texel on a wall that faces east already is
//! the light arriving on a surface facing east at that wall. So for each
//! of its six directions a probe casts a few short rays the other way, and
//! a ray that meets a lightmapped surface facing the probe's direction
//! takes that surface's lightmap colour. The ray toward the floor finds the
//! floor's lightmap, the light from above. A direction whose rays meet
//! nothing takes the mean of the directions that did. An area with no
//! lightmapped room mesh gets no probes.
//!
//! Each frame `light_objects` gives every placed model that is not a room
//! the blend of the probes nearest it in the room it stands in, never
//! across rooms. Each lamp of the shader's list is then taken out of that
//! light by the share the shader adds for it, because the lightmaps already
//! hold the lamps' light. The result eases over `EASE_SECONDS` and goes to
//! the shader in `AREA_PROBES`, one entry per model, which a mesh finds by
//! the model in its `MeshTag` (`shadows::OWNER_TAG`). In the shader the
//! probe light replaces the area's flat ambient (`odyssey.wgsl`).
//!
//! `KJK_PROBES=off` leaves Enhanced lighting without probe light.
//! `KJK_PROBES=only` shows each probed model in its probe light alone, with
//! no lamps and no texture.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use asset_odyssey::texture::{Encoding, TextureIr};
use bevy::asset::uuid_handle;
use bevy::mesh::MeshTag;
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;
use bevy::render::storage::ShaderBuffer;
use console::DumpFacts;
use fmt_odyssey::dxt;

use crate::material::{self, AreaLights, GpuLight, LIGHT_COMBAT, LIGHT_DYNAMIC_ALL};
use crate::scene::{SceneContent, SceneModel};
use crate::viewer::{Placed, RoomView};

/// The probes' light, one entry per placed model (`ProbeList`), which
/// every material reads.
pub const AREA_PROBES: Handle<ShaderBuffer> = uuid_handle!("2f6c9e41-7a3d-4b58-9e07-c4d1a8b3f526");

/// How many models the shader's list has an entry for. A model numbered
/// this or higher keeps the area's flat ambient. Our chosen cap.
pub const PROBE_SLOTS: usize = 1024;

/// How far apart the probes stand on a room's floor (metres).
const SPACING: f32 = 1.0;
/// How high above the floor a probe stands (metres).
const PROBE_HEIGHT: f32 = 1.0;
/// How far a probe's rays reach (metres).
const RAY_REACH: f32 = 6.0;
/// How far a probe's four side rays lean from its middle ray (radians).
const RAY_LEAN: f32 = 0.6;
/// A ray takes a surface's lightmap only when the surface faces the
/// probe's direction at least this much (the cosine between them).
const FACING_LEAST: f32 = 0.3;
/// The side of a cell of the level's triangle grid (metres).
const CELL: f32 = 2.0;
/// A lightmap is read at its first mip level no wider or taller than this
/// (texels). The light is soft, and a small level reads fast.
const MIP_SIDE: usize = 128;
/// How far from a model a probe still lights it (metres, across the
/// floor), and how far above or below it.
const BLEND_REACH: f32 = 1.6;
const BLEND_STOREY: f32 = 1.5;
/// Where a model's light is read: this high above its origin (metres).
/// A character's origin is at its feet.
const SAMPLE_HEIGHT: f32 = 1.0;
/// How long a model's light takes to settle on a new value (seconds). So
/// walking through a doorway does not pop.
const EASE_SECONDS: f32 = 0.25;
/// A model that moved less than this (metres) keeps its light.
const MOVED: f32 = 0.05;

/// The six directions a probe holds light for, in Bevy's space, in the
/// shader's order: east, west, up, down, south, north.
const AXES: [Vec3; 6] = [
    Vec3::X,
    Vec3::NEG_X,
    Vec3::Y,
    Vec3::NEG_Y,
    Vec3::Z,
    Vec3::NEG_Z,
];

/// A point of Bevy's space in the game's (z up). The inverse of
/// `scene::game_to_bevy`.
fn to_game(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y)
}

/// What `KJK_PROBES` asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    On,
    /// Enhanced lighting without probe light.
    Off,
    /// The probed models in their probe light alone.
    Only,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::On => "on",
            Mode::Off => "off",
            Mode::Only => "only",
        }
    }
}

/// `KJK_PROBES`, read once.
pub fn mode() -> Mode {
    static MODE: OnceLock<Mode> = OnceLock::new();
    *MODE.get_or_init(|| match std::env::var("KJK_PROBES").as_deref() {
        Ok("off") => Mode::Off,
        Ok("only") => Mode::Only,
        _ => Mode::On,
    })
}

/// One probe: where it stands (game space), the light arriving from each
/// of `AXES`, and whether any of its rays met a lightmap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Probe {
    pub at: Vec3,
    pub light: [Vec3; 6],
    pub lit: bool,
}

/// One room's probes, and a grid of them across the floor to find the
/// ones near a point.
#[derive(Default)]
pub struct RoomProbes {
    pub probes: Vec<Probe>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

fn grid_key(at: Vec3) -> (i32, i32) {
    (
        (at.x / SPACING).floor() as i32,
        (at.y / SPACING).floor() as i32,
    )
}

impl RoomProbes {
    fn new(probes: Vec<Probe>) -> RoomProbes {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (index, probe) in probes.iter().enumerate() {
            grid.entry(grid_key(probe.at))
                .or_default()
                .push(index as u32);
        }
        RoomProbes { probes, grid }
    }

    /// The light at `at` (game space, at the height a model's light is
    /// read): the probes with light within `BLEND_REACH` across and
    /// `BLEND_STOREY` up or down, each weighed by how near it is. `None`
    /// when no such probe is near.
    pub fn blend(&self, at: Vec3) -> Option<[Vec3; 6]> {
        let (x, y) = grid_key(at);
        let span = (BLEND_REACH / SPACING).ceil() as i32;
        let mut sum = [Vec3::ZERO; 6];
        let mut weight = 0.0;
        for dx in -span..=span {
            for dy in -span..=span {
                let Some(near) = self.grid.get(&(x + dx, y + dy)) else {
                    continue;
                };
                for &index in near {
                    let probe = &self.probes[index as usize];
                    let across = probe.at.truncate().distance(at.truncate());
                    if !probe.lit
                        || across >= BLEND_REACH
                        || (probe.at.z - at.z).abs() >= BLEND_STOREY
                    {
                        continue;
                    }
                    let w = (1.0 - across / BLEND_REACH).powi(2).max(1e-4);
                    for (sum, light) in sum.iter_mut().zip(probe.light) {
                        *sum += light * w;
                    }
                    weight += w;
                }
            }
        }
        (weight > 0.0).then(|| sum.map(|light| light / weight))
    }
}

/// Why a model keeps the area's flat ambient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fallback {
    /// The area has no probes (no lightmapped room mesh).
    NoProbes,
    /// The model stands in no room.
    Outside,
    /// No probe with light is near it, in its room or a room linked with it.
    NoneNear,
}

/// An area's probes, made when it loads (`bake`).
#[derive(Default)]
pub struct AreaProbes {
    /// One entry per room, as `LoadedScene::rooms`. Empty for an area with
    /// no probes.
    pub rooms: Vec<RoomProbes>,
    /// How long the bake took (ms).
    pub bake_ms: f64,
    /// The room triangles the rays met: lightmapped ones, and the other
    /// opaque ones, which only block.
    pub lightmapped: usize,
    pub blocking: usize,
}

impl AreaProbes {
    pub fn is_empty(&self) -> bool {
        self.rooms.iter().all(|room| room.probes.is_empty())
    }

    /// The light at `at` (game space) in room `room`. When the room has no
    /// probe near, the rooms linked with it are tried in turn
    /// (`fight_rooms::linked`). The light of one room never blends with
    /// another's, so it does not leak through a wall.
    pub fn light_at(
        &self,
        rooms: &[RoomView],
        room: Option<usize>,
        at: Vec3,
    ) -> Result<[Vec3; 6], Fallback> {
        if self.is_empty() {
            return Err(Fallback::NoProbes);
        }
        let room = room.ok_or(Fallback::Outside)?;
        if let Some(light) = self.rooms.get(room).and_then(|probes| probes.blend(at)) {
            return Ok(light);
        }
        (0..rooms.len().min(self.rooms.len()))
            .filter(|&other| other != room && crate::fight_rooms::linked(rooms, room, other))
            .find_map(|other| self.rooms[other].blend(at))
            .ok_or(Fallback::NoneNear)
    }
}

/// A lightmap's texels at a small mip level, as stored (gamma-encoded),
/// for the bake.
pub struct Pixels {
    width: usize,
    height: usize,
    rgb: Vec<Vec3>,
    clamp: bool,
}

impl Pixels {
    /// One texel of `colour`.
    pub fn flat(colour: Vec3) -> Pixels {
        Pixels {
            width: 1,
            height: 1,
            rgb: vec![colour],
            clamp: false,
        }
    }

    /// `texture`'s first level no larger than `MIP_SIDE`, decoded. `None`
    /// when that level does not decode.
    fn of(texture: &TextureIr) -> Option<Pixels> {
        let size = |level: usize| {
            (
                (texture.width as usize >> level).max(1),
                (texture.height as usize >> level).max(1),
            )
        };
        let level = (0..texture.mips.len())
            .find(|&level| {
                let (w, h) = size(level);
                w.max(h) <= MIP_SIDE
            })
            .or(texture.mips.len().checked_sub(1))?;
        let (width, height) = size(level);
        let data = &texture.mips[level];
        let rgba = match texture.encoding {
            Encoding::Rgba8 => (data.len() >= width * height * 4).then(|| data.clone())?,
            Encoding::Bc1 | Encoding::Bc1Holes => dxt::decode(data, width, height, false)?,
            Encoding::Bc3 => dxt::decode(data, width, height, true)?,
        };
        let rgb = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .take(width * height)
            .map(|[r, g, b, _]| Vec3::new(f32::from(*r), f32::from(*g), f32::from(*b)) / 255.0)
            .collect::<Vec<_>>();
        (rgb.len() == width * height).then_some(Pixels {
            width,
            height,
            rgb,
            clamp: texture.facts.clamp,
        })
    }

    /// The colour at `uv`, read between the four nearest texels, as the
    /// GPU's sampler reads it (rows in stored order, repeating unless the
    /// texture clamps).
    fn sample(&self, uv: Vec2) -> Vec3 {
        let x = uv.x * self.width as f32 - 0.5;
        let y = uv.y * self.height as f32 - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let texel = |tx: i64, ty: i64| {
            let (w, h) = (self.width as i64, self.height as i64);
            let (tx, ty) = if self.clamp {
                (tx.clamp(0, w - 1), ty.clamp(0, h - 1))
            } else {
                (tx.rem_euclid(w), ty.rem_euclid(h))
            };
            self.rgb[(ty * w + tx) as usize]
        };
        let (x0, y0) = (x0 as i64, y0 as i64);
        let top = texel(x0, y0).lerp(texel(x0 + 1, y0), fx);
        let bottom = texel(x0, y0 + 1).lerp(texel(x0 + 1, y0 + 1), fx);
        top.lerp(bottom, fy)
    }
}

/// A room triangle the probes' rays may meet (game space): its corners,
/// its lightmap's texture coordinates, and its lightmap (an index into the
/// bake's `Pixels`). One without a lightmap only blocks rays.
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub corners: [Vec3; 3],
    pub uv: [Vec2; 3],
    pub lightmap: Option<usize>,
}

/// Where a ray met a surface: which one, how far along the ray, and the
/// point's place in the triangle.
struct Hit {
    surface: u32,
    t: f32,
    u: f32,
    v: f32,
}

/// The level's surfaces, each with the way its front faces, in a grid of
/// cells to find the ones along a ray.
struct Level<'s> {
    surfaces: &'s [Surface],
    /// Each triangle's front (`(b − a) × (c − a)`, as the games wind a
    /// face drawn from its front), taken from its corners: some models
    /// store no normals.
    facing: Vec<Vec3>,
    cells: HashMap<IVec3, Vec<u32>>,
}

fn cell_of(at: Vec3) -> IVec3 {
    (at / CELL).floor().as_ivec3()
}

impl<'s> Level<'s> {
    /// The grid of `surfaces`, over the box from `least` to `most` alone:
    /// no ray reaches past it. A backdrop's triangles span hundreds of
    /// metres (the space outside the Ebon Hawk's cockpit), and the cells
    /// they would fill outside the box cost seconds.
    fn new(surfaces: &'s [Surface], (least, most): (Vec3, Vec3)) -> Level<'s> {
        let mut cells: HashMap<IVec3, Vec<u32>> = HashMap::new();
        let mut facing = Vec::with_capacity(surfaces.len());
        for (index, surface) in surfaces.iter().enumerate() {
            let [a, b, c] = surface.corners;
            facing.push((b - a).cross(c - a).normalize_or_zero());
            let (low, high) = (a.min(b).min(c), a.max(b).max(c));
            if low.cmpgt(most).any() || high.cmplt(least).any() {
                continue;
            }
            let (low, high) = (cell_of(low.max(least)), cell_of(high.min(most)));
            for x in low.x..=high.x {
                for y in low.y..=high.y {
                    for z in low.z..=high.z {
                        cells
                            .entry(IVec3::new(x, y, z))
                            .or_default()
                            .push(index as u32);
                    }
                }
            }
        }
        Level {
            surfaces,
            facing,
            cells,
        }
    }

    /// The nearest surface the ray from `from` along `direction` (a unit
    /// vector) meets within `reach`. It walks the grid cell by cell along
    /// the ray and stops once a hit lies inside the cell it has reached.
    fn cast(&self, from: Vec3, direction: Vec3, reach: f32) -> Option<Hit> {
        let mut cell = cell_of(from);
        let step = IVec3::new(
            direction.x.partial_cmp(&0.0).map_or(0, |o| o as i32),
            direction.y.partial_cmp(&0.0).map_or(0, |o| o as i32),
            direction.z.partial_cmp(&0.0).map_or(0, |o| o as i32),
        );
        let boundary = |axis: usize| {
            let edge = (cell[axis] + i32::from(step[axis] > 0)) as f32 * CELL;
            if step[axis] == 0 {
                f32::INFINITY
            } else {
                (edge - from[axis]) / direction[axis]
            }
        };
        let mut next = Vec3::new(boundary(0), boundary(1), boundary(2));
        let delta = Vec3::new(
            CELL / direction.x.abs(),
            CELL / direction.y.abs(),
            CELL / direction.z.abs(),
        );
        let mut best: Option<Hit> = None;
        loop {
            for &index in self.cells.get(&cell).into_iter().flatten() {
                if let Some((t, u, v)) =
                    ray_hit_uv(from, direction, &self.surfaces[index as usize].corners)
                    && t <= reach
                    && best.as_ref().is_none_or(|hit| t < hit.t)
                {
                    best = Some(Hit {
                        surface: index,
                        t,
                        u,
                        v,
                    });
                }
            }
            let exit = next.min_element();
            if best.as_ref().is_some_and(|hit| hit.t <= exit) || exit > reach {
                return best;
            }
            let axis = if next.x == exit {
                0
            } else if next.y == exit {
                1
            } else {
                2
            };
            cell[axis] += step[axis];
            next[axis] += delta[axis];
        }
    }
}

/// Where along `direction` from `origin` the ray meets `triangle`, and the
/// point's weights for its second and third corners (Möller–Trumbore), on
/// either face. As `shadows::ray_hit`, with the weights.
fn ray_hit_uv(origin: Vec3, direction: Vec3, [a, b, c]: &[Vec3; 3]) -> Option<(f32, f32, f32)> {
    let (ab, ac) = (*b - *a, *c - *a);
    let p = direction.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let to_origin = origin - *a;
    let u = to_origin.dot(p) / det;
    let q = to_origin.cross(ab);
    let v = direction.dot(q) / det;
    let t = ac.dot(q) / det;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 1e-4).then_some((t, u, v))
}

/// The light arriving at `at` (game space) from each of `AXES`.
fn light_probe(level: &Level, maps: &[Pixels], at: Vec3) -> Probe {
    let mut light = [Vec3::ZERO; 6];
    let mut found = [false; 6];
    for (slot, axis) in AXES.iter().enumerate() {
        // The surfaces that face this way lie the other way.
        let facing = to_game(*axis);
        let cast = -facing;
        let (side, across) = cast.any_orthonormal_pair();
        let (lean_cos, lean_sin) = (RAY_LEAN.cos(), RAY_LEAN.sin());
        let rays = [
            (cast, 1.0),
            (cast * lean_cos + side * lean_sin, lean_cos),
            (cast * lean_cos - side * lean_sin, lean_cos),
            (cast * lean_cos + across * lean_sin, lean_cos),
            (cast * lean_cos - across * lean_sin, lean_cos),
        ];
        let mut sum = Vec3::ZERO;
        let mut weight = 0.0;
        for (direction, lean) in rays {
            let Some(hit) = level.cast(at, direction, RAY_REACH) else {
                continue;
            };
            let surface = &level.surfaces[hit.surface as usize];
            let Some(map) = surface.lightmap else {
                continue;
            };
            if level.facing[hit.surface as usize].dot(facing) < FACING_LEAST {
                continue;
            }
            let [a, b, c] = surface.uv;
            let uv = a * (1.0 - hit.u - hit.v) + b * hit.u + c * hit.v;
            let w = lean / (1.0 + hit.t * 0.5);
            sum += maps[map].sample(uv) * w;
            weight += w;
        }
        if weight > 0.0 {
            light[slot] = sum / weight;
            found[slot] = true;
        }
    }
    let hits = found.iter().filter(|found| **found).count();
    if hits > 0 {
        let mean = light
            .iter()
            .zip(found)
            .filter(|(_, found)| *found)
            .map(|(light, _)| *light)
            .sum::<Vec3>()
            / hits as f32;
        for (light, found) in light.iter_mut().zip(found) {
            if !found {
                *light = mean;
            }
        }
    }
    Probe {
        at,
        light,
        lit: hits > 0,
    }
}

/// Where a room's probes stand (game space): on a grid `SPACING` apart,
/// `PROBE_HEIGHT` above each walkable floor under a grid point. A floor
/// small enough to hold no grid point gets one probe over the middle of
/// its largest triangle.
fn probe_places(floor: &[[[f32; 3]; 3]]) -> Vec<Vec3> {
    let corners = floor
        .iter()
        .flatten()
        .map(|corner| Vec3::from_array(*corner));
    let Some((low, high)) = corners.fold(None, |span: Option<(Vec3, Vec3)>, corner| {
        Some(span.map_or((corner, corner), |(low, high)| {
            (low.min(corner), high.max(corner))
        }))
    }) else {
        return Vec::new();
    };
    let mut places = Vec::new();
    let (x0, x1) = (
        (low.x / SPACING).ceil() as i32,
        (high.x / SPACING).floor() as i32,
    );
    let (y0, y1) = (
        (low.y / SPACING).ceil() as i32,
        (high.y / SPACING).floor() as i32,
    );
    for gx in x0..=x1 {
        for gy in y0..=y1 {
            let (x, y) = (gx as f32 * SPACING, gy as f32 * SPACING);
            let mut heights: Vec<f32> = floor
                .iter()
                .filter_map(|[a, b, c]| crate::scene::height_at(*a, *b, *c, x, y))
                .collect();
            heights.sort_by(f32::total_cmp);
            heights.dedup_by(|a, b| (*a - *b).abs() < BLEND_STOREY);
            places.extend(
                heights
                    .into_iter()
                    .map(|z| Vec3::new(x, y, z + PROBE_HEIGHT)),
            );
        }
    }
    if places.is_empty() {
        let area = |[a, b, c]: &[[f32; 3]; 3]| {
            let [a, b, c] = [a, b, c].map(|p| Vec3::from_array(*p));
            (b - a).cross(c - a).length()
        };
        if let Some(largest) = floor.iter().max_by(|a, b| area(a).total_cmp(&area(b))) {
            let middle = largest.iter().map(|p| Vec3::from_array(*p)).sum::<Vec3>() / 3.0;
            places.push(middle + Vec3::Z * PROBE_HEIGHT);
        }
    }
    places
}

/// The probes of rooms whose walkable floors are `floors`, from the level's
/// `surfaces` and their lightmaps `maps`. The probes are shared among the
/// machine's threads.
pub fn bake_rooms(
    floors: &[&[[[f32; 3]; 3]]],
    surfaces: &[Surface],
    maps: &[Pixels],
) -> Vec<RoomProbes> {
    let places: Vec<(usize, Vec3)> = floors
        .iter()
        .enumerate()
        .flat_map(|(room, floor)| probe_places(floor).into_iter().map(move |at| (room, at)))
        .collect();
    let Some(reached) = places
        .iter()
        .fold(None, |span: Option<(Vec3, Vec3)>, (_, at)| {
            Some(span.map_or((*at, *at), |(low, high)| (low.min(*at), high.max(*at))))
        })
    else {
        return floors.iter().map(|_| RoomProbes::default()).collect();
    };
    let margin = Vec3::splat(RAY_REACH + CELL);
    let level = Level::new(surfaces, (reached.0 - margin, reached.1 + margin));
    const CHUNK: usize = 64;
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(places.len().div_ceil(CHUNK))
        .max(1);
    let taken = AtomicUsize::new(0);
    let mut lit: Vec<(usize, Probe)> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let first = taken.fetch_add(CHUNK, Ordering::Relaxed);
                        if first >= places.len() {
                            return out;
                        }
                        for &(room, at) in &places[first..(first + CHUNK).min(places.len())] {
                            out.push((room, light_probe(&level, maps, at)));
                        }
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("a probe's thread does not panic"))
            .collect()
    });
    lit.sort_by(|a, b| a.0.cmp(&b.0));
    let mut rooms: Vec<Vec<Probe>> = vec![Vec::new(); floors.len()];
    for (room, probe) in lit {
        rooms[room].push(probe);
    }
    rooms.into_iter().map(RoomProbes::new).collect()
}

/// An area's probes, from its rooms' models among `models`, their floors,
/// and the lightmaps among `textures`. A room mesh counts as lightmapped by
/// the spawner's rule (`scene`): its lightmap loaded and one lightmap
/// coordinate per vertex. Other opaque room meshes only block rays; a
/// see-through one, or one the spawner does not draw, is left out. An area
/// with no lightmapped room mesh (the Dune Sea) gets no probes.
pub fn bake(
    models: &[Placed],
    rooms: &[RoomView],
    textures: &BTreeMap<String, TextureIr>,
) -> AreaProbes {
    let started = Instant::now();
    let mut maps: Vec<Pixels> = Vec::new();
    let mut map_of: HashMap<&str, Option<usize>> = HashMap::new();
    let mut surfaces = Vec::new();
    for room in rooms {
        let Some(placed) = room.model.and_then(|model| models.get(model)) else {
            continue;
        };
        let frame = crate::scene::placement(placed);
        for mesh in placed.ir.meshes.iter().filter(|mesh| mesh.skin.is_none()) {
            let loaded = |name: &Option<String>| {
                name.as_deref()
                    .and_then(|name| textures.get_key_value(name))
            };
            let diffuse = loaded(&mesh.texture);
            let lightmap = loaded(&mesh.lightmap);
            if diffuse.is_none() && lightmap.is_none() {
                continue;
            }
            let facts: Vec<_> = [diffuse, lightmap]
                .into_iter()
                .flatten()
                .map(|(_, texture)| &texture.facts)
                .collect();
            let drawn = material::surface(
                (mesh.transparency_hint, false),
                mesh.alpha,
                (diffuse.map(|(_, texture)| &texture.facts), &facts),
            );
            if !drawn.is_opaque() {
                continue;
            }
            let map = lightmap
                .filter(|_| mesh.uv2.len() == mesh.positions.len())
                .and_then(|(name, texture)| {
                    *map_of.entry(name.as_str()).or_insert_with(|| {
                        Pixels::of(texture).map(|pixels| {
                            maps.push(pixels);
                            maps.len() - 1
                        })
                    })
                });
            let node = frame * crate::scene::to_mat4(&placed.ir.nodes[mesh.node].world);
            let corner = |index: u32| {
                let at =
                    node.transform_point3(Vec3::from_array(*mesh.positions.get(index as usize)?));
                let uv = map.map_or(Vec2::ZERO, |_| Vec2::from_array(mesh.uv2[index as usize]));
                Some((at, uv))
            };
            for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
                let (Some(a), Some(b), Some(c)) = (corner(a), corner(b), corner(c)) else {
                    continue;
                };
                surfaces.push(Surface {
                    corners: [a.0, b.0, c.0],
                    uv: [a.1, b.1, c.1],
                    lightmap: map,
                });
            }
        }
    }
    let lightmapped = surfaces.iter().filter(|s| s.lightmap.is_some()).count();
    let blocking = surfaces.len() - lightmapped;
    let rooms = if lightmapped == 0 {
        Vec::new()
    } else {
        let floors: Vec<&[[[f32; 3]; 3]]> = rooms.iter().map(|room| &room.floor[..]).collect();
        bake_rooms(&floors, &surfaces, &maps)
    };
    AreaProbes {
        rooms,
        bake_ms: started.elapsed().as_secs_f64() * 1000.0,
        lightmapped,
        blocking,
    }
}

fn smoothstep(low: f32, high: f32, x: f32) -> f32 {
    let t = ((x - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `light` with each lamp's share taken out, never below black. A lamp
/// that darkens (a negative colour) takes nothing out. A lamp's
/// share is what `odyssey.wgsl` adds for it to a surface facing each of
/// `AXES`, read at `at` for a mesh whose origin is `origin` (both Bevy
/// space). A static mesh takes only the lamps meant for everything, as in
/// the shader. A fight's light is in no lightmap and is not taken out.
pub fn without_lamps(
    mut light: [Vec3; 6],
    lamps: &[GpuLight],
    (origin, at): (Vec3, Vec3),
    is_static: bool,
) -> [Vec3; 6] {
    for lamp in lamps {
        if lamp.kind.z == LIGHT_COMBAT || (is_static && lamp.kind.y != LIGHT_DYNAMIC_ALL) {
            continue;
        }
        let place = lamp.position_radius.truncate();
        let radius = lamp.position_radius.w;
        let reach = 1.0 - smoothstep(0.9 * radius, radius, place.distance(origin));
        if reach <= 0.0 {
            continue;
        }
        let to_lamp = place - at;
        let distance = to_lamp.length();
        let direction = to_lamp / distance.max(1e-4);
        let falloff = (radius * radius) / ((radius + distance) * (radius + distance));
        // A lamp with a negative colour or multiplier darkens what it
        // lights. Its share is not added back: the probes' light never
        // grows past what the lightmaps hold.
        let strength = (lamp.color_multiplier.truncate()
            * (reach * lamp.color_multiplier.w * falloff))
            .max(Vec3::ZERO);
        for (light, axis) in light.iter_mut().zip(AXES) {
            let facing = if lamp.kind.x != 0 {
                1.0
            } else {
                axis.dot(direction).max(0.0)
            };
            *light = (*light - strength * facing).max(Vec3::ZERO);
        }
    }
    light
}

/// One placed model's light as the shader reads it: the six colours of
/// `AXES`, and in the first's w 1 when it holds light, else 0.
#[derive(Clone, Copy, Debug, Default, PartialEq, ShaderType)]
pub struct GpuProbe {
    pub light: [Vec4; 6],
}

/// What `AREA_PROBES` holds: an entry for each model by its number, and
/// `mode` 1 under `KJK_PROBES=only`.
#[derive(Clone, Debug, PartialEq, ShaderType)]
pub struct ProbeList {
    pub mode: u32,
    pub entries: [GpuProbe; PROBE_SLOTS],
}

impl Default for ProbeList {
    fn default() -> ProbeList {
        ProbeList {
            mode: 0,
            entries: [GpuProbe::default(); PROBE_SLOTS],
        }
    }
}

/// A placed model's light as `light_objects` keeps it.
#[derive(Clone, Copy, Default)]
struct Entry {
    /// Where its origin stood when its light was last worked out (Bevy).
    at: Vec3,
    /// The probes' light before the lamps are taken out, for the dump.
    raw: [Vec3; 6],
    target: [Vec3; 6],
    now: [Vec3; 6],
    valid: bool,
    worked: bool,
    seen: bool,
    why: Option<Fallback>,
}

/// What `light_objects` keeps between frames.
#[derive(Default)]
struct ProbeState {
    entries: Vec<Entry>,
    lamps: Vec<GpuLight>,
    /// Whether the shader's list holds any light now.
    shown: bool,
    /// Whether each of the scene's models is a room's.
    is_room: Vec<bool>,
    /// The meshes' own ambient colours, for the dump.
    mesh_ambient: serde_json::Value,
    /// Models numbered past `PROBE_SLOTS` this frame.
    past_slots: usize,
}

impl ProbeState {
    fn new(models: &[Placed], rooms: &[RoomView]) -> ProbeState {
        let mut is_room = vec![false; models.len()];
        for model in rooms.iter().filter_map(|room| room.model) {
            if let Some(room) = is_room.get_mut(model) {
                *room = true;
            }
        }
        let luma = |c: [f32; 3]| 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
        let ambient: Vec<f32> = models
            .iter()
            .zip(&is_room)
            .filter(|(_, room)| !**room)
            .flat_map(|(placed, _)| placed.ir.meshes.iter().map(|mesh| luma(mesh.ambient)))
            .collect();
        let round = |x: f32| (x * 1000.0).round() / 1000.0;
        let mesh_ambient = serde_json::json!({
            "meshes": ambient.len(),
            "least": round(ambient.iter().copied().fold(f32::INFINITY, f32::min)),
            "mean": round(ambient.iter().sum::<f32>() / ambient.len().max(1) as f32),
            "most": round(ambient.iter().copied().fold(0.0, f32::max)),
            "black": ambient.iter().filter(|a| **a < 1e-3).count(),
        });
        ProbeState {
            is_room,
            mesh_ambient,
            ..default()
        }
    }
}

pub struct LightProbesPlugin;

impl Plugin for LightProbesPlugin {
    fn build(&self, app: &mut App) {
        let _ = app
            .world_mut()
            .resource_mut::<Assets<ShaderBuffer>>()
            .insert(&AREA_PROBES, ShaderBuffer::from(ProbeList::default()));
        app.add_systems(
            PostUpdate,
            (own_armed_meshes, light_objects)
                .after(bevy::transform::TransformSystems::Propagate)
                .after(crate::shadows::Uploaded),
        );
    }
}

/// A mesh spawned outside the scene's models whose model is found from
/// where it hangs: a weapon armed during play (`armory`). `own_armed_meshes`
/// gives it the number of the body it hangs on.
#[derive(Component)]
pub(crate) struct Unowned;

/// Each `Unowned` mesh carries the number of the body it hangs on in its
/// tag (`scene::owner_tag`), so it takes the body's probe light and casts
/// no shadow of the body on itself. A weapon not yet hung keeps none.
fn own_armed_meshes(
    meshes: Query<(Entity, Option<&MeshTag>), With<Unowned>>,
    parents: Query<&ChildOf>,
    models: Query<&SceneModel>,
    scene: Option<Res<SceneContent>>,
    mut commands: Commands,
) {
    let Some(scene) = scene else { return };
    for (mesh, tag) in &meshes {
        let mut at = mesh;
        let mut owner = None;
        for _ in 0..64 {
            let Ok(parent) = parents.get(at) else { break };
            at = parent.parent();
            if let Ok(model) = models.get(at)
                && let Some(placed) = scene.0.models.get(model.index)
            {
                owner = Some(placed.hook.map_or(model.index, |(body, _)| body));
                break;
            }
        }
        let want = owner.map(|owner| MeshTag(crate::shadows::OWNER_TAG | owner as u32));
        if want.as_ref().map(|tag| tag.0) != tag.map(|tag| tag.0) {
            match want {
                Some(want) => commands.entity(mesh).insert(want),
                None => commands.entity(mesh).remove::<MeshTag>(),
            };
        }
    }
}

/// Gives each placed model that is not a room the light of the probes
/// where it stands, with the lamps of the shader's list taken out, and
/// puts the list up (`AREA_PROBES`). A head or a weapon hung on a body is
/// lit by the body's entry. A model is worked out again only when it moves
/// or the lamps change. Only under Enhanced lighting, unless
/// `KJK_PROBES=off`.
#[allow(clippy::too_many_arguments)]
fn light_objects(
    placed: Query<(&SceneModel, &GlobalTransform)>,
    scene: Option<Res<SceneContent>>,
    (culling, scene_lights, area): (
        Res<crate::scene::Culling>,
        Res<crate::scene::SceneLights>,
        Res<AreaLights>,
    ),
    time: Res<Time>,
    cameras: Query<&GlobalTransform, With<crate::scene::FlyCamera>>,
    mut state: Local<ProbeState>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
    mut facts: ResMut<DumpFacts>,
) {
    let Some(scene) = scene else { return };
    let loaded = &scene.0;
    // A new scene's models take their numbers afresh: the old list goes.
    let mut changed = false;
    if scene.is_changed() {
        changed = state.shown;
        *state = ProbeState::new(&loaded.models, &loaded.rooms);
    }
    let mode = mode();
    let on = area.0.enhanced != 0 && mode != Mode::Off && !loaded.probes.is_empty();
    if !on {
        if state.shown {
            state.entries.clear();
            state.shown = false;
            changed = true;
        }
    } else {
        // The lamps of the shader's list that are the scene's own, which
        // the lightmaps hold. A carried or a fight's light is not baked.
        let count = (area.0.count as usize).min(area.0.lights.len());
        let lamps: Vec<GpuLight> = scene_lights
            .active
            .iter()
            .flatten()
            .zip(&area.0.lights[..count])
            .filter(|(index, light)| {
                **index < scene_lights.all.len() && light.kind.z != LIGHT_COMBAT
            })
            .map(|(_, light)| *light)
            .collect();
        let lamps_changed = lamps != state.lamps;
        state.lamps = lamps;
        let state = &mut *state;
        state.past_slots = 0;
        for entry in &mut state.entries {
            entry.seen = false;
        }
        for (model, at) in &placed {
            let index = model.index;
            let Some(model_placed) = loaded.models.get(index) else {
                continue;
            };
            if state.is_room.get(index).copied().unwrap_or(false) || model_placed.hook.is_some() {
                continue;
            }
            if index >= PROBE_SLOTS {
                state.past_slots += 1;
                continue;
            }
            if state.entries.len() <= index {
                state.entries.resize(index + 1, Entry::default());
            }
            let entry = &mut state.entries[index];
            entry.seen = true;
            let origin = at.translation();
            if entry.worked && !lamps_changed && origin.distance(entry.at) <= MOVED {
                continue;
            }
            entry.worked = true;
            entry.at = origin;
            let sample = origin + Vec3::Y * SAMPLE_HEIGHT;
            let room = culling.fight.room_of(&loaded.rooms, to_game(origin));
            match loaded.probes.light_at(&loaded.rooms, room, to_game(sample)) {
                Ok(light) => {
                    entry.raw = light;
                    entry.target = without_lamps(
                        light,
                        &state.lamps,
                        (origin, sample),
                        model_placed.is_static,
                    );
                    if !entry.valid {
                        entry.now = entry.target;
                        entry.valid = true;
                        changed = true;
                    }
                    entry.why = None;
                }
                Err(why) => {
                    changed |= entry.valid;
                    entry.valid = false;
                    entry.why = Some(why);
                }
            }
        }
        let ease = 1.0 - (-time.delta_secs() * 4.0 / EASE_SECONDS).exp();
        for entry in &mut state.entries {
            if !entry.seen {
                changed |= entry.valid;
                *entry = Entry::default();
                continue;
            }
            if !entry.valid || entry.now == entry.target {
                continue;
            }
            for (now, target) in entry.now.iter_mut().zip(entry.target) {
                *now = if now.distance(target) < 1e-3 {
                    target
                } else {
                    now.lerp(target, ease)
                };
            }
            changed = true;
        }
        state.shown |= state.entries.iter().any(|entry| entry.valid);
    }
    if changed {
        let mut list = Box::new(ProbeList {
            mode: u32::from(mode == Mode::Only),
            ..default()
        });
        for (slot, entry) in list.entries.iter_mut().zip(&state.entries) {
            if entry.valid {
                slot.light = std::array::from_fn(|axis| {
                    entry.now[axis].extend(if axis == 0 { 1.0 } else { 0.0 })
                });
            }
        }
        if let Some(mut buffer) = buffers.get_mut(&AREA_PROBES) {
            buffer.set_data(*list);
        }
    }
    if facts.wanted() {
        let eye = cameras
            .single()
            .map_or(Vec3::ZERO, GlobalTransform::translation);
        let probes = &loaded.probes;
        let all: usize = probes.rooms.iter().map(|room| room.probes.len()).sum();
        let lit: usize = probes
            .rooms
            .iter()
            .flat_map(|room| &room.probes)
            .filter(|probe| probe.lit)
            .count();
        let why = |want: Fallback| {
            state
                .entries
                .iter()
                .filter(|entry| entry.seen && entry.why == Some(want))
                .count()
        };
        facts.0.insert(
            "light_probes".into(),
            serde_json::json!({
                "mode": mode.name(),
                "on": on,
                "probes": all,
                "with_light": lit,
                "rooms_with_probes": probes.rooms.iter().filter(|room| !room.probes.is_empty()).count(),
                "bake_ms": (probes.bake_ms * 10.0).round() / 10.0,
                "triangles": { "lightmapped": probes.lightmapped, "blocking": probes.blocking },
                "objects_probed": state.entries.iter().filter(|entry| entry.seen && entry.valid).count(),
                "kept_flat_ambient": {
                    "no_probes": why(Fallback::NoProbes),
                    "outside_rooms": why(Fallback::Outside),
                    "no_probe_near": why(Fallback::NoneNear),
                    "past_slots": state.past_slots,
                },
                "lamps_taken_out": state.lamps.len(),
                // The six sides of the probed models that the lamps took to
                // black, of all their sides.
                "sides_taken_to_black": [
                    state.entries.iter().filter(|entry| entry.seen && entry.valid)
                        .flat_map(|entry| entry.raw.iter().zip(&entry.target))
                        .filter(|(raw, lit)| raw.max_element() > 0.0 && lit.max_element() <= 0.0)
                        .count(),
                    6 * state.entries.iter().filter(|entry| entry.seen && entry.valid).count(),
                ],
                "flat_ambient": (Vec3::from_array(loaded.ambient) * 255.0).round().to_array(),
                "mesh_ambient": state.mesh_ambient,
                "models": model_facts(&state.entries, &loaded.models),
                "nearest": nearest_facts(&state.entries, eye, (&loaded.models, &state.lamps)),
            }),
        );
    }
}

/// Each probed model, for the dump: its name, where it stands (game space,
/// metres), and how bright its light is from above and on its four sides
/// (Rec. 601 luma, in steps of 255), with the lamps taken out.
fn model_facts(entries: &[Entry], models: &[Placed]) -> serde_json::Value {
    let luma = |c: Vec3| (c.dot(Vec3::new(0.299, 0.587, 0.114)) * 255.0).round();
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.seen && entry.valid)
        .map(|(model, entry)| {
            let at = to_game(entry.at);
            let sides = [0, 1, 4, 5].map(|side| luma(entry.target[side]));
            serde_json::json!({
                "model": model,
                "name": models.get(model).map(|placed| placed.ir.name.clone()),
                "at": (at * 10.0).round().to_array().map(|x| x / 10.0),
                "up": luma(entry.target[2]),
                "sides": sides,
            })
        })
        .collect()
}

/// The three probed models nearest the camera, for the dump: each one's
/// number, the probes' light, and that light with the lamps taken out, in
/// steps of 255, in `AXES`'s order.
fn nearest_facts(
    entries: &[Entry],
    eye: Vec3,
    (models, lamps): (&[Placed], &[GpuLight]),
) -> serde_json::Value {
    let mut near: Vec<(usize, &Entry)> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.seen && entry.valid)
        .collect();
    near.sort_by(|a, b| a.1.at.distance(eye).total_cmp(&b.1.at.distance(eye)));
    let steps = |light: &[Vec3; 6]| {
        light
            .iter()
            .map(|c| (*c * 255.0).round().to_array())
            .collect::<Vec<_>>()
    };
    near.iter()
        .take(3)
        .map(|(model, entry)| {
            let round = |v: Vec3| (v * 100.0).round().to_array().map(|x| x / 100.0);
            // The lamps nearest the model, where they stand from its origin
            // (Bevy space, metres), with their radius.
            let mut near_lamps: Vec<&GpuLight> = lamps.iter().collect();
            near_lamps.sort_by(|a, b| {
                let apart = |l: &GpuLight| l.position_radius.truncate().distance(entry.at);
                apart(a).total_cmp(&apart(b))
            });
            serde_json::json!({
                "model": model,
                "name": models.get(*model).map(|placed| placed.ir.name.clone()),
                "at": round(entry.at),
                "lamps": near_lamps.iter().take(3).map(|lamp| {
                    let off = lamp.position_radius.truncate() - entry.at;
                    serde_json::json!({ "off": round(off), "radius": lamp.position_radius.w, "kind": lamp.kind.to_array() })
                }).collect::<Vec<_>>(),
                "probes": steps(&entry.raw),
                "without_lamps": steps(&entry.target),
            })
        })
        .collect()
}

#[cfg(feature = "probes")]
pub mod probes;
