//! `kjk detail-bake <module>`: an area's lightmaps made `SCALE` times
//! larger, with the detail the stored size cannot hold
//! (`docs/LIGHTING-PLAN.md`, step 10). It opens no window and runs on the
//! CPU. It writes one PNG for each lightmap into `--out/<area>`, a grey
//! picture of the detail alone into `--out/<area>/detail`, and a text
//! file. The game draws the PNGs in the lightmaps' place when
//! `KJK_DETAIL_BAKE` names that folder (`viewer::texture`).
//!
//! BioWare's lightmaps stay the base. Their colour and their brightness
//! are kept. The bake needs no light of its own, because the lights
//! BioWare baked from were not shipped (step 7).
//!
//! - **The texels.** Each lightmap keeps its coordinates and gets `SCALE`
//!   times the texels along each side. A new texel belongs to the triangle
//!   that holds its centre, or to the nearest triangle within
//!   `FOOTPRINT` texels, which is as far as the sampler reads past a
//!   triangle's edge.
//! - **What a texel gathers.** `rays × rays` rays leave the texel over the
//!   half of space in front of its triangle, more of them near the
//!   triangle's facing. A ray brings the stored lightmap's light from the
//!   room surface it meets. A surface nearer than `--near` blocks the room
//!   behind it and gives back only what its texture reflects.
//! - **The detail.** The gathered light over its own average across about
//!   one stored texel, on the same surface. So the detail is near 1 on
//!   average, and it darkens or brightens only what is smaller than a
//!   stored texel. `--width` widens the average, which keeps larger
//!   shadows too and changes the room more.
//! - **The new lightmap.** The stored lightmap, read between its texels as
//!   the GPU reads it, times the detail, in linear light.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use asset_odyssey::model::multiply;
use asset_odyssey::module::{AreaIr, ModuleIr};
use asset_odyssey::texture::{Encoding, TextureIr};
use bevy::math::{IVec3, Vec2, Vec3};
use fmt_odyssey::dxt;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

use crate::light_probes::{Level, Surface};
use crate::modules::{ResolverSource, module_names};
use crate::room_light_fit::{encoded, linear, parallel};
use crate::room_lights::{self, Owner, Placement, Reader};

/// How many new texels lie along one stored texel's side.
const SCALE: usize = 4;
/// How far outside its triangle (in new texels) a texel still belongs to
/// it. The sampler reads the four texels around a point, so a point on a
/// triangle's edge reads texels whose centres lie up to one texel outside.
const FOOTPRINT: f32 = 1.45;
/// How far a ray is followed (metres).
const REACH: f32 = 20.0;
/// How far a texel is lifted off its surface before its rays leave, and
/// how far a texel on a triangle's edge is moved in (metres).
const LIFT: f32 = 0.02;
const INWARD: f32 = 0.01;
/// A surface nearer than `NEAR_FROM` gives back only what its texture
/// reflects. One further than `Options::near` counts as the room's light
/// in that direction, in full. Between the two the share changes smoothly.
const NEAR_FROM: f32 = 0.25;
/// A texel with this share of its rays meeting the back of a surface is
/// inside a wall. It takes no detail.
const BACK_SHARE: f32 = 0.4;
/// The least and the most the detail changes a texel's light.
const LEAST: f32 = 0.4;
const MOST: f32 = 1.6;
/// The same under the fitted lights, whose shadows are deeper.
/// How many rings of texels inside a wall take their neighbours' light.
const FILL_RINGS: usize = 24;
/// A stored texel as bright as `GLOW_TO` (0 to 1, as stored) keeps its
/// stored value in a bake from lights, and one below `GLOW_FROM` keeps
/// none of it.
const GLOW_FROM: f32 = 0.35;
const GLOW_TO: f32 = 0.6;
/// A texel within `LAMP_NEAR` of a lamp painted into the lightmaps keeps
/// its stored value in a bake from lights, and one beyond `LAMP_FAR`
/// keeps none of it for that (metres).
const LAMP_NEAR: f32 = 0.6;
/// How bright a stored texel of such a lamp or of its pool of light is
/// (0 to 1, as stored).
const LAMP_BRIGHT: f32 = 0.3;
const LAMP_FAR: f32 = 2.0;
const LIGHTS_LEAST: f32 = 0.2;
const LIGHTS_MOST: f32 = 2.5;
/// The shortest and the longest distance the average reaches (metres).
const AVERAGE_LEAST: f32 = 0.15;
const AVERAGE_MOST: f32 = 2.0;
/// The side of a cell of the grid that finds a texel's neighbours (metres).
const CELL: f32 = 0.5;
/// Light this small is added to both sides of the detail's division, so a
/// black corner does not divide by nothing.
const FLOOR: f32 = 0.002;

/// What `kjk detail-bake` was asked for.
pub struct Options {
    /// `rays × rays` rays leave each texel.
    pub rays: usize,
    /// How much of the detail the new lightmap takes, from 0 to 1.
    pub strength: f32,
    /// How wide the average is, in stored texels.
    pub width: f32,
    /// Plain occlusion in the detail's place. A texel is darkened by the
    /// share of its rays that meet a surface nearer than `near`, whatever
    /// the stored lightmap holds there. Nothing is brightened, so the room
    /// gets darker as a whole.
    pub plain: bool,
    /// The distance (metres) beyond which a surface blocks nothing.
    pub near: f32,
}

pub fn run(
    discovery: &Discovery,
    module: &str,
    game: Option<&str>,
    options: &Options,
    out: &Path,
) -> ExitCode {
    let games = match game {
        None => vec![Game::Kotor1, Game::Kotor2],
        Some(key) => match Game::from_key(key).filter(|game| *game != Game::JediAcademy) {
            Some(game) => vec![game],
            None => {
                eprintln!("detail-bake: use k1 or tsl");
                return ExitCode::from(2);
            }
        },
    };
    let module = module.to_ascii_lowercase();
    for game in games {
        let Some(install) = discovery.get(game) else {
            continue;
        };
        let index = match DirIndex::of_install(install) {
            Ok(index) => index,
            Err(error) => {
                eprintln!("{}: {error}", game.key());
                return ExitCode::FAILURE;
            }
        };
        if !module_names(&index).contains(&module) {
            continue;
        }
        return match bake_module(game, &index, &module, options, out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("detail-bake: {error}");
                ExitCode::FAILURE
            }
        };
    }
    eprintln!("detail-bake: no install found has module {module}");
    ExitCode::FAILURE
}

/// The default folder: `target/detail-bake` under the current folder.
pub fn default_out() -> PathBuf {
    PathBuf::from("target").join("detail-bake")
}

fn bake_module(
    game: Game,
    index: &DirIndex,
    module: &str,
    options: &Options,
    out: &Path,
) -> Result<(), String> {
    let base = Resolver::base(game, index, TextureQuality::High).map_err(|e| e.to_string())?;
    let resolver = base
        .with_module(index, module)
        .map_err(|error| error.to_string())?;
    let built =
        ModuleIr::build(module, &ResolverSource(&resolver)).map_err(|error| format!("{error:?}"))?;
    let mut reader = Reader::new(&resolver, module);
    for area in &built.areas {
        let folder = out.join(&area.resref);
        let report = bake_area(&mut reader, &resolver, area, options, &folder, None)?;
        outln!("{report}");
        std::fs::write(folder.join("detail-bake.txt"), &report)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        outln!("wrote into {}", folder.display());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// The stored lightmaps.

/// A lightmap as stored: its colours and its alpha from 0 to 1, rows in
/// stored order.
struct Stored {
    name: String,
    width: usize,
    height: usize,
    texels: Vec<[f32; 4]>,
    clamp: bool,
    /// How many levels the stored texture has.
    levels: usize,
}

impl Stored {
    /// The value at `uv`, read between the four nearest texels as the GPU's
    /// sampler reads it.
    fn sample(&self, uv: Vec2) -> [f32; 4] {
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
            self.texels[(ty * w + tx) as usize]
        };
        let (x0, y0) = (x0 as i64, y0 as i64);
        let (a, b, c, d) = (
            texel(x0, y0),
            texel(x0 + 1, y0),
            texel(x0, y0 + 1),
            texel(x0 + 1, y0 + 1),
        );
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }

    /// The light at `uv` in linear light.
    fn light(&self, uv: Vec2) -> Vec3 {
        let [r, g, b, _] = self.sample(uv);
        Vec3::new(linear(r), linear(g), linear(b))
    }
}

fn read_stored(resolver: &Resolver, name: &str) -> Option<Stored> {
    let texture = TextureIr::from_texture(&resolver.texture(name)?, None)?;
    let (width, height) = (texture.width as usize, texture.height as usize);
    let data = texture.mips.first()?;
    let rgba = match texture.encoding {
        Encoding::Rgba8 => (data.len() >= width * height * 4).then(|| data.clone())?,
        Encoding::Bc1 | Encoding::Bc1Holes => dxt::decode(data, width, height, false)?,
        Encoding::Bc3 => dxt::decode(data, width, height, true)?,
    };
    let texels: Vec<[f32; 4]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .take(width * height)
        .map(|texel| texel.map(|v| f32::from(v) / 255.0))
        .collect();
    (texels.len() == width * height && width > 0 && height > 0).then_some(Stored {
        name: name.to_ascii_lowercase(),
        width,
        height,
        texels,
        clamp: texture.facts.clamp,
        levels: texture.mips.len(),
    })
}

/// The mean colour of a texture in linear light, from a small level.
fn albedo(resolver: &Resolver, name: &str) -> Option<Vec3> {
    let texture = TextureIr::from_texture(&resolver.texture(name)?, None)?;
    let size = |level: usize| {
        (
            (texture.width as usize >> level).max(1),
            (texture.height as usize >> level).max(1),
        )
    };
    let level = (0..texture.mips.len())
        .find(|&level| size(level).0.max(size(level).1) <= 16)
        .or(texture.mips.len().checked_sub(1))?;
    let (width, height) = size(level);
    let data = &texture.mips[level];
    let rgba = match texture.encoding {
        Encoding::Rgba8 => (data.len() >= width * height * 4).then(|| data.clone())?,
        Encoding::Bc1 | Encoding::Bc1Holes => dxt::decode(data, width, height, false)?,
        Encoding::Bc3 => dxt::decode(data, width, height, true)?,
    };
    let mut sum = Vec3::ZERO;
    let mut count = 0.0f32;
    for [r, g, b, _] in rgba.as_chunks::<4>().0.iter().take(width * height) {
        sum += Vec3::new(
            linear(f32::from(*r) / 255.0),
            linear(f32::from(*g) / 255.0),
            linear(f32::from(*b) / 255.0),
        );
        count += 1.0;
    }
    (count > 0.0).then(|| sum / count)
}

// ---------------------------------------------------------------------------
// The texels.

/// What a ray met, beside its corners in the `Level`.
struct Facts {
    normal: Vec3,
    albedo: Vec3,
    map: Option<usize>,
    uv: [Vec2; 3],
}

/// The triangle a new texel belongs to, while the triangles are read.
#[derive(Clone, Copy)]
struct Claim {
    /// How far outside the triangle the texel's centre lies (new texels).
    /// Zero inside.
    outside: f32,
    /// The texel's place on the triangle's plane.
    at: Vec3,
    /// The nearest place on the triangle itself.
    anchor: Vec3,
    normal: Vec3,
    /// The side of a stored texel on this triangle (metres).
    size: f32,
}

/// A new texel on a room's surface.
struct Point {
    /// Its place, lifted off the surface.
    at: Vec3,
    normal: Vec3,
    map: usize,
    texel: usize,
    /// Whether its centre lies on its triangle.
    inside: bool,
    size: f32,
}

/// What a texel gathered: its light, and whether it counts.
#[derive(Clone, Copy)]
struct Gathered {
    light: Vec3,
    counts: bool,
}

/// The nearest place to `p` on the triangle `[a, b, c]` (Ericson).
fn closest([a, b, c]: [Vec3; 3], p: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Two directions across `normal`. Triangles that face the same way get
/// the same two, so their texels cast the same rays and no seam shows
/// between them.
fn across(normal: Vec3) -> (Vec3, Vec3) {
    let axis = if normal.z.abs() < 0.9 {
        Vec3::Z
    } else {
        Vec3::X
    };
    let side = normal.cross(axis).normalize();
    (side, normal.cross(side))
}

/// `rays × rays` directions over the half of space around +Z, more of
/// them near +Z (weighted by the cosine), one in each cell of a grid.
fn directions(rays: usize) -> Vec<Vec3> {
    let mut all = Vec::with_capacity(rays * rays);
    for i in 0..rays {
        for j in 0..rays {
            let u1 = (i as f32 + 0.5) / rays as f32;
            let u2 = (j as f32 + 0.5) / rays as f32;
            let r = u1.sqrt();
            let phi = std::f32::consts::TAU * u2;
            all.push(Vec3::new(r * phi.cos(), r * phi.sin(), (1.0 - u1).sqrt()));
        }
    }
    all
}

fn smooth(from: f32, to: f32, v: f32) -> f32 {
    let t = ((v - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The light of the area's fitted lights at a place on a surface that
/// faces `normal`, in linear light (`room_light_fit::Lights`).
pub(crate) type LightAt<'l> = &'l (dyn Fn(Vec3, Vec3) -> Vec3 + Sync);

/// Bakes one area into `folder`. With `lights`, a texel's gathered light
/// is the fitted lights' light there, in the place of what its rays bring
/// from the stored lightmaps. The new lightmaps are then the stored ones
/// times that light over its own average, so the stored brightness stays
/// and the lights' shadows get their edges. `folder/relit` also gets the
/// fitted lights' light alone, with nothing of the stored lightmaps.
pub(crate) fn bake_area(
    reader: &mut Reader,
    resolver: &Resolver,
    area: &AreaIr,
    options: &Options,
    folder: &Path,
    lights: Option<LightAt>,
) -> Result<String, String> {
    let started = std::time::Instant::now();
    for sub in ["detail", "relit"] {
        if sub == "detail" || lights.is_some() {
            std::fs::create_dir_all(folder.join(sub))
                .map_err(|error| format!("{}: {error}", folder.display()))?;
        }
    }
    let placement = room_lights::place(reader, area);
    let Placement {
        objects, walkable, ..
    } = &placement;

    let mut albedos: HashMap<String, Vec3> = HashMap::new();
    let mut maps: Vec<Stored> = Vec::new();
    let mut map_of: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut claims: Vec<Vec<Option<Claim>>> = Vec::new();
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut facts: Vec<Facts> = Vec::new();
    for object in objects {
        let Owner::Room(room) = object.owner else {
            continue;
        };
        if !walkable[room] {
            continue;
        }
        let ir = &object.model.ir;
        for mesh in &ir.meshes {
            if mesh.skin.is_some() {
                continue;
            }
            let node = multiply(&object.frame, &ir.nodes[mesh.node].world);
            let world: Vec<Vec3> = (mesh.positions.iter())
                .map(|p| Vec3::from_array(room_lights::apply(&node, *p)))
                .collect();
            let map = match mesh.lightmap.as_deref() {
                Some(name) if mesh.uv2.len() == mesh.positions.len() => {
                    *map_of.entry(name.to_ascii_lowercase()).or_insert_with(|| {
                        let stored = read_stored(resolver, name)?;
                        claims.push(vec![None; stored.width * stored.height * SCALE * SCALE]);
                        maps.push(stored);
                        Some(maps.len() - 1)
                    })
                }
                _ => None,
            };
            let opaque = mesh.transparency_hint == 0
                && mesh.alpha >= 0.99
                && (mesh.texture.is_some() || mesh.lightmap.is_some());
            let colour = mesh.texture.as_deref().map_or(Vec3::splat(0.5), |name| {
                *albedos
                    .entry(name.to_ascii_lowercase())
                    .or_insert_with(|| albedo(resolver, name).unwrap_or(Vec3::splat(0.5)))
            });
            for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
                let corner = |i: u32| world.get(i as usize).copied();
                let (Some(pa), Some(pb), Some(pc)) = (corner(a), corner(b), corner(c)) else {
                    continue;
                };
                let front = (pb - pa).cross(pc - pa);
                if front.length() < 1e-8 {
                    continue;
                }
                let normal = front.normalize();
                let uv2 = |i: u32| mesh.uv2.get(i as usize).map(|uv| Vec2::from_array(*uv));
                if opaque {
                    surfaces.push(Surface {
                        corners: [pa, pb, pc],
                        uv: [Vec2::ZERO; 3],
                        lightmap: None,
                        texture: None,
                        texture_uv: [Vec2::ZERO; 3],
                        glow: Vec3::ZERO,
                    });
                    facts.push(Facts {
                        normal,
                        albedo: colour,
                        map,
                        uv: [a, b, c].map(|i| uv2(i).unwrap_or(Vec2::ZERO)),
                    });
                }
                let Some(map) = map else {
                    continue;
                };
                let stored = &maps[map];
                let (w, h) = (stored.width * SCALE, stored.height * SCALE);
                // The corners in new texels.
                let uv = [a, b, c].map(|i| {
                    let uv = mesh.uv2[i as usize];
                    [uv[0] * w as f32, uv[1] * h as f32]
                });
                let twice = (uv[1][0] - uv[0][0]) * (uv[2][1] - uv[0][1])
                    - (uv[2][0] - uv[0][0]) * (uv[1][1] - uv[0][1]);
                if twice.abs() < 1e-6 {
                    continue;
                }
                let sign = twice.signum();
                let low = |axis: usize| uv.iter().map(|p| p[axis]).fold(f32::MAX, f32::min);
                let high = |axis: usize| uv.iter().map(|p| p[axis]).fold(f32::MIN, f32::max);
                if (high(0) - low(0)) * (high(1) - low(1)) > 4.0 * (w * h) as f32 {
                    continue;
                }
                let corners = [pa, pb, pc];
                let middle = (pa + pb + pc) / 3.0;
                // A stored texel's side on this triangle: the triangle's
                // area in the room over its area on the stored lightmap.
                let size = (front.length() / (twice.abs() / (SCALE * SCALE) as f32)).sqrt();
                let rows = (low(1) - 0.5 - FOOTPRINT).ceil() as i64
                    ..=(high(1) - 0.5 + FOOTPRINT).floor() as i64;
                for y in rows {
                    let columns = (low(0) - 0.5 - FOOTPRINT).ceil() as i64
                        ..=(high(0) - 0.5 + FOOTPRINT).floor() as i64;
                    for x in columns {
                        let centre = [x as f32 + 0.5, y as f32 + 0.5];
                        let edges: [f32; 3] = std::array::from_fn(|k| {
                            sign * room_lights::edge(uv[k], uv[(k + 1) % 3], centre)
                        });
                        let outside = edges.iter().fold(0.0f32, |most, e| most.max(-e));
                        if outside > FOOTPRINT {
                            continue;
                        }
                        let texel =
                            (y.rem_euclid(h as i64) as usize) * w + x.rem_euclid(w as i64) as usize;
                        if claims[map][texel].is_some_and(|claim| claim.outside <= outside) {
                            continue;
                        }
                        // The weights of the three corners at the centre.
                        // They add up to 1, and one is below 0 outside.
                        let weights: [f32; 3] = std::array::from_fn(|k| {
                            let (p, q) = (uv[(k + 1) % 3], uv[(k + 2) % 3]);
                            ((q[0] - p[0]) * (centre[1] - p[1])
                                - (q[1] - p[1]) * (centre[0] - p[0]))
                                / twice
                        });
                        let at: Vec3 = (0..3).map(|k| corners[k] * weights[k]).sum();
                        let anchor = if outside > 0.0 {
                            let edge = closest(corners, at);
                            let inward = middle - edge;
                            edge + inward * (INWARD / inward.length().max(INWARD))
                        } else {
                            at
                        };
                        claims[map][texel] = Some(Claim {
                            outside,
                            at,
                            anchor,
                            normal,
                            size,
                        });
                    }
                }
            }
        }
    }
    if maps.is_empty() {
        return Err(format!("{}: no lightmap on a room", area.resref));
    }
    let (least, most) = surfaces.iter().flat_map(|s| s.corners).fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), p| (low.min(p), high.max(p)),
    );
    let level = Level::new(&surfaces, (least, most));

    // The texels' places. A texel outside its triangle keeps its place on
    // the triangle's plane when nothing lies between the triangle and that
    // place. That is so where the next triangle carries the surface on.
    // Where a wall stands there, the texel moves back onto its triangle.
    let claimed: Vec<(usize, usize, Claim)> = claims
        .iter()
        .enumerate()
        .flat_map(|(map, claims)| {
            (claims.iter().enumerate())
                .filter_map(move |(texel, claim)| claim.map(|claim| (map, texel, claim)))
        })
        .collect();
    drop(claims);
    let points: Vec<Point> = parallel(claimed.len(), |index| {
        let (map, texel, claim) = claimed[index];
        let lift = claim.normal * LIFT;
        let mut at = claim.at;
        if claim.outside > 0.0 {
            let way = claim.at - claim.anchor;
            let far = way.length();
            if far < 1e-4
                || level
                    .first_hit(claim.anchor + lift, way / far, far + LIFT)
                    .is_some()
            {
                at = claim.anchor;
            }
        }
        Point {
            at: at + lift,
            normal: claim.normal,
            map,
            texel,
            inside: claim.outside <= 0.0,
            size: claim.size,
        }
    });
    drop(claimed);

    // The light of a ray that meets nothing: the mean of the area's
    // lightmaps.
    let open = {
        let mut sum = Vec3::ZERO;
        let mut count = 0.0f32;
        for map in &maps {
            for [r, g, b, _] in &map.texels {
                sum += Vec3::new(linear(*r), linear(*g), linear(*b));
                count += 1.0;
            }
        }
        sum / count.max(1.0)
    };

    // What each texel gathers.
    let rays = directions(options.rays.max(1));
    let gathered: Vec<Gathered> = parallel(points.len(), |index| {
        let point = &points[index];
        let (side, up) = across(point.normal);
        let mut sum = Vec3::ZERO;
        let mut backs = 0usize;
        for ray in &rays {
            let direction = side * ray.x + up * ray.y + point.normal * ray.z;
            let Some((surface, t, u, v)) = level.first_hit_at(point.at, direction, REACH) else {
                sum += if options.plain && lights.is_none() {
                    Vec3::ONE
                } else {
                    open
                };
                continue;
            };
            let fact = &facts[surface];
            if fact.normal.dot(direction) >= 0.0 {
                backs += 1;
                continue;
            }
            let far = smooth(NEAR_FROM.min(options.near * 0.1), options.near, t);
            if options.plain && lights.is_none() {
                sum += Vec3::splat(far);
                continue;
            }
            let light = fact.map.map_or(open, |map| {
                maps[map].light(fact.uv[0] * (1.0 - u - v) + fact.uv[1] * u + fact.uv[2] * v)
            });
            sum += light * fact.albedo.lerp(Vec3::ONE, far);
        }
        Gathered {
            light: sum / rays.len() as f32,
            counts: (backs as f32) < BACK_SHARE * rays.len() as f32,
        }
    });

    let gathered: Vec<Gathered> = match lights {
        None => gathered,
        Some(lights) => parallel(points.len(), |index| Gathered {
            light: lights(points[index].at, points[index].normal),
            counts: gathered[index].counts,
        }),
    };
    let plain = options.plain && lights.is_none();
    let (least, most) = if lights.is_some() {
        (LIGHTS_LEAST, LIGHTS_MOST)
    } else {
        (LEAST, MOST)
    };

    // Each texel's average: the gathered light of the texels on the same
    // surface around it, as far as `width` stored texels.
    let mut cells: HashMap<IVec3, Vec<u32>> = HashMap::new();
    for (index, point) in points.iter().enumerate() {
        if point.inside && gathered[index].counts {
            cells
                .entry((point.at / CELL).floor().as_ivec3())
                .or_default()
                .push(index as u32);
        }
    }
    let detail: Vec<Vec3> = parallel(points.len(), |index| {
        let point = &points[index];
        let own = gathered[index];
        if !own.counts {
            return Vec3::ONE;
        }
        if plain {
            return own.light.clamp(Vec3::splat(LEAST), Vec3::ONE);
        }
        let reach = (point.size * options.width).clamp(AVERAGE_LEAST, AVERAGE_MOST);
        let (low, high) = (
            ((point.at - Vec3::splat(reach)) / CELL).floor().as_ivec3(),
            ((point.at + Vec3::splat(reach)) / CELL).floor().as_ivec3(),
        );
        let mut sum = Vec3::ZERO;
        let mut total = 0.0f32;
        for x in low.x..=high.x {
            for y in low.y..=high.y {
                for z in low.z..=high.z {
                    for &other in cells.get(&IVec3::new(x, y, z)).into_iter().flatten() {
                        let near = &points[other as usize];
                        let way = near.at - point.at;
                        let d = way.length();
                        if d >= reach
                            || near.normal.dot(point.normal) < 0.7
                            || way.dot(point.normal).abs() > 0.25 * reach
                        {
                            continue;
                        }
                        let fall = 1.0 - (d / reach) * (d / reach);
                        // A texel stands for its own patch of the surface.
                        let weight = fall * fall * near.size * near.size;
                        sum += gathered[other as usize].light * weight;
                        total += weight;
                    }
                }
            }
        }
        if total <= 0.0 {
            return Vec3::ONE;
        }
        let average = sum / total;
        ((own.light + FLOOR) / (average + FLOOR)).clamp(Vec3::splat(least), Vec3::splat(most))
    });

    // How much of its stored value each texel keeps in a bake from lights,
    // by how near it is to a lamp painted into the lightmaps. The light
    // such a lamp gives is painted beside it, and no fitted light has its
    // shape.
    let near_lamp: Vec<f32> = if lights.is_some() {
        let stored_luma = |point: &Point| {
            let map = &maps[point.map];
            let w = map.width * SCALE;
            let uv = Vec2::new(
                ((point.texel % w) as f32 + 0.5) / w as f32,
                ((point.texel / w) as f32 + 0.5) / (map.height * SCALE) as f32,
            );
            let [r, g, b, _] = map.sample(uv);
            0.2126 * r + 0.7152 * g + 0.0722 * b
        };
        let mut lamps: HashMap<IVec3, Vec<Vec3>> = HashMap::new();
        for point in &points {
            if point.inside && stored_luma(point) >= LAMP_BRIGHT {
                lamps
                    .entry((point.at / CELL).floor().as_ivec3())
                    .or_default()
                    .push(point.at);
            }
        }
        parallel(points.len(), |index| {
            let at = points[index].at;
            let (low, high) = (
                ((at - Vec3::splat(LAMP_FAR)) / CELL).floor().as_ivec3(),
                ((at + Vec3::splat(LAMP_FAR)) / CELL).floor().as_ivec3(),
            );
            let mut nearest = f32::MAX;
            for x in low.x..=high.x {
                for y in low.y..=high.y {
                    for z in low.z..=high.z {
                        for lamp in lamps.get(&IVec3::new(x, y, z)).into_iter().flatten() {
                            nearest = nearest.min((*lamp - at).length());
                        }
                    }
                }
            }
            1.0 - smooth(LAMP_NEAR, LAMP_FAR, nearest)
        })
    } else {
        Vec::new()
    };

    // The new lightmaps.
    let mut of_map: Vec<Vec<Option<u32>>> = (maps.iter())
        .map(|map| vec![None; map.width * map.height * SCALE * SCALE])
        .collect();
    for (index, point) in points.iter().enumerate() {
        of_map[point.map][point.texel] = Some(index as u32);
    }
    for (map, texels) in maps.iter().zip(&of_map) {
        let (w, h) = (map.width * SCALE, map.height * SCALE);
        let mut picture = image::RgbaImage::new(w as u32, h as u32);
        let mut grey = image::RgbImage::from_pixel(w as u32, h as u32, image::Rgb([48, 48, 96]));
        let mut relit = image::RgbaImage::new(w as u32, h as u32);
        // The lights' light of each texel that counts. A texel inside a
        // wall (under a table's foot) takes the mean of its neighbours
        // that have a light, one ring at a time. Left with the stored
        // value, it shows as a block beside the texels around it.
        let mut lit: Vec<Option<Vec3>> = texels
            .iter()
            .map(|texel| {
                texel
                    .filter(|&index| gathered[index as usize].counts)
                    .map(|index| gathered[index as usize].light)
            })
            .collect();
        if lights.is_some() {
            for _ in 0..FILL_RINGS {
                let before = lit.clone();
                let mut any = false;
                for y in 0..h {
                    for x in 0..w {
                        if before[y * w + x].is_some() || texels[y * w + x].is_none() {
                            continue;
                        }
                        let (mut sum, mut count) = (Vec3::ZERO, 0.0f32);
                        for (dx, dy) in [
                            (-1i64, -1i64),
                            (0, -1),
                            (1, -1),
                            (-1, 0),
                            (1, 0),
                            (-1, 1),
                            (0, 1),
                            (1, 1),
                        ] {
                            let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                            if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                                continue;
                            }
                            if let Some(light) = before[ny as usize * w + nx as usize] {
                                sum += light;
                                count += 1.0;
                            }
                        }
                        if count > 0.0 {
                            lit[y * w + x] = Some(sum / count);
                            any = true;
                        }
                    }
                }
                if !any {
                    break;
                }
            }
        }
        for y in 0..h {
            for x in 0..w {
                let uv = Vec2::new((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                let [r, g, b, a] = map.sample(uv);
                let mut rgb = [r, g, b];
                if let Some(index) = texels[y * w + x] {
                    let change = Vec3::ONE + (detail[index as usize] - Vec3::ONE) * options.strength;
                    for (k, value) in rgb.iter_mut().enumerate() {
                        *value = encoded(linear(*value) * change[k]);
                    }
                    let shade = detail[index as usize].to_array().map(|v| to_byte(v * 0.5));
                    grey.put_pixel(x as u32, y as u32, image::Rgb(shade));
                }
                // A stored texel this bright is a lamp painted into the
                // lightmap, and it keeps its stored value.
                let glow = smooth(GLOW_FROM, GLOW_TO, 0.2126 * r + 0.7152 * g + 0.0722 * b);
                let glow = texels[y * w + x]
                    .and_then(|index| near_lamp.get(index as usize))
                    .map_or(glow, |near| glow.max(*near));
                let alone = match lit[y * w + x] {
                    Some(light) => {
                        let light = light.to_array().map(encoded);
                        std::array::from_fn(|k| light[k] + ([r, g, b][k] - light[k]) * glow)
                    }
                    None => [r, g, b],
                };
                let [lr, lg, lb] = alone.map(to_byte);
                relit.put_pixel(x as u32, y as u32, image::Rgba([lr, lg, lb, to_byte(a)]));
                let [r, g, b] = rgb.map(to_byte);
                picture.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, to_byte(a)]));
            }
        }
        let path = folder.join(format!("{}.png", map.name));
        picture
            .save(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        let path = folder.join("detail").join(format!("{}.png", map.name));
        grey.save(&path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        if lights.is_some() {
            let path = folder.join("relit").join(format!("{}.png", map.name));
            relit
                .save(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }

    // The report.
    let counted: Vec<f32> = {
        let mut all: Vec<f32> = (points.iter().zip(&detail).zip(&gathered))
            .filter(|((point, _), gathered)| point.inside && gathered.counts)
            .map(|((_, detail), _)| (detail.x + detail.y + detail.z) / 3.0)
            .collect();
        all.sort_by(f32::total_cmp);
        all
    };
    let at = |share: f32| {
        counted
            .get(((counted.len() as f32 - 1.0) * share) as usize)
            .copied()
            .unwrap_or(1.0)
    };
    let inside = points.iter().filter(|point| point.inside).count();
    let dropped = gathered.iter().filter(|g| !g.counts).count();
    let mut report = String::new();
    let _ = writeln!(report, "{}", area.resref);
    let _ = writeln!(
        report,
        "  {} lightmaps, {} stored texels in all, {} levels at most",
        maps.len(),
        maps.iter().map(|m| m.width * m.height).sum::<usize>(),
        maps.iter().map(|m| m.levels).max().unwrap_or(0)
    );
    let _ = writeln!(
        report,
        "  {} room triangles block rays",
        surfaces.len()
    );
    let _ = writeln!(
        report,
        "  {} new texels on a surface, {} of them with their centre on a triangle",
        points.len(),
        inside
    );
    let _ = writeln!(
        report,
        "  {} new texels are inside a wall and take no detail",
        dropped
    );
    let _ = writeln!(
        report,
        "  {} rays a texel, strength {}, average over {} stored texels, near {} m{}",
        rays.len(),
        options.strength,
        options.width,
        options.near,
        if lights.is_some() {
            ", from the fitted lights"
        } else if options.plain {
            ", plain occlusion"
        } else {
            ""
        }
    );
    let _ = writeln!(
        report,
        "  the detail: 1st percentile {:.3}, 10th {:.3}, median {:.3}, 90th {:.3}, 99th {:.3}",
        at(0.01),
        at(0.10),
        at(0.50),
        at(0.90),
        at(0.99)
    );
    let _ = write!(report, "  {:.0} seconds", started.elapsed().as_secs_f64());
    Ok(report)
}

fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}
