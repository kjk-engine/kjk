//! `kjk detail-bake <module> --lights <file>`: an area's lightmaps baked
//! anew from the game's fitted lights, at `SCALE` times their stored size
//! (`docs/LIGHTING-PLAN.md`, steps 10 and 10b). It opens no window. The
//! file of lights is one that `KJK_FITTED_DUMP` wrote. The graphics card
//! bakes the lights with their shadows and their bounced light
//! (`gpu::bake_lights`). The bake writes set `a`, which mixes in the stored
//! lightmaps and is never shipped, and set `b`, the lamps alone, which the
//! lighting download ships. It writes one PNG for each lightmap into
//! `--out/<area>/a` and `--out/<area>/b`, and a text file. It stamps each
//! set with the fingerprint of the files it was made from
//! (`bake_fingerprint`). `tools/light-kinds/bake.sh` and
//! `tools/light-kinds/canon/canon.sh` run it.
//!
//! This file finds the texels and reads the lights on the CPU, and writes
//! the sets. A bake on the CPU alone and a detail bake without lights were
//! dropped on 24 November 2026 (`docs/LIGHTING-LOG.md`).

use std::collections::{BTreeMap, HashMap};
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
use crate::room_light_fit::{linear, parallel};
use crate::room_lights::{self, Owner, Placement, Reader};

mod gpu;
pub(crate) use gpu::{GpuOptions, Sun};

/// How many new texels lie along one stored texel's side.
const SCALE: usize = 4;
/// The most new texels to a stored texel's side, and the longest side of a
/// lightmap the bake writes, under `KJK_BAKE_TEXEL`.
const SCALE_MOST: usize = 32;
const SIDE_MOST: usize = 2048;

/// The new texels to a stored texel's side for a lightmap whose stored
/// texel covers `metres` of its surfaces, under `KJK_BAKE_TEXEL`: the
/// longest new texel it asks for, in metres. A coarse lightmap gets more,
/// so a lamp's pool keeps its shape. The floor of tar_m03aa has a stored
/// texel about 3 metres across, and a fire's pool showed as hard squares
/// (22 November 2026). Without the variable every lightmap takes `SCALE`.
fn scale_for(metres: f32, side: usize) -> usize {
    let Some(target) = std::env::var("KJK_BAKE_TEXEL").ok().and_then(|v| v.parse::<f32>().ok()) else {
        return SCALE;
    };
    let mut scale = SCALE;
    while scale < SCALE_MOST && metres / scale as f32 > target && side * scale * 2 <= SIDE_MOST {
        scale *= 2;
    }
    scale
}
/// The most bytes the graphics card's bake binds of one buffer: wgpu's
/// largest storage binding on the RTX 4070 Ti SUPER that bakes the
/// download, under 2 GiB.
const BINDING_MOST: usize = 2_147_483_644;
/// The bytes of one new texel in the bake's buffer of light (`field_a` and
/// `field_b` in `gpu.rs`), and of one place in its buffers of places and of
/// results.
const FIELD_BYTES: usize = 16;
const PLACE_BYTES: usize = 48;

/// Lowers the scale of an area's largest lightmaps when its new texels do
/// not fit one buffer of the graphics card (`BINDING_MOST`). The scale of
/// every lightmap with the most new texels to a side is halved, again and
/// again, until each new texel could be a place and the places would still
/// fit one buffer. Lightmaps of one size keep one scale, so the walls of a
/// room are not sharper on one lightmap than on the next. An area whose
/// texels fit keeps every scale, so no other area's bake changes.
/// The K1 Community Patch gives room m44ad_01a 16 lightmaps, 15 of them
/// 1024 texels to a side. At `SCALE` they took 4.1 GB of new texels, and
/// the bake failed (23 November 2026). Halved once, they would still be 62
/// million texels, so they take one new texel to each stored texel.
fn fit_field(area: &str, maps: &mut [Stored]) {
    let texels = |maps: &[Stored]| -> usize { (maps.iter()).map(|map| map.width * map.height * map.scale * map.scale).sum() };
    let before = texels(maps);
    if before * FIELD_BYTES <= BINDING_MOST {
        return;
    }
    let side = |map: &Stored| map.width.max(map.height) * map.scale;
    while texels(maps) * PLACE_BYTES > BINDING_MOST {
        let Some(longest) = maps.iter().filter(|map| map.scale > 1).map(side).max() else {
            break;
        };
        for map in maps.iter_mut().filter(|map| map.scale > 1 && side(map) == longest) {
            map.scale /= 2;
        }
    }
    eprintln!(
        "{area}: the lightmaps' {before} new texels do not fit one buffer of the graphics card, so the largest lightmaps take fewer, {} in all",
        texels(maps)
    );
}

/// How far outside its triangle (in new texels) a texel still belongs to
/// it. The sampler reads the four texels around a point, so a point on a
/// triangle's edge reads texels whose centres lie up to one texel outside.
const FOOTPRINT: f32 = 1.45;
/// How far a texel is lifted off its surface before its rays leave, and
/// how far a texel on a triangle's edge is moved in (metres).
const LIFT: f32 = 0.02;
const INWARD: f32 = 0.01;
/// How many rings of texels inside a wall take their neighbours' light.
const FILL_RINGS: usize = 24;

pub fn run(
    discovery: &Discovery,
    module: &str,
    game: Option<&str>,
    lights: &Path,
    options: &GpuOptions,
    out: &Path,
) -> ExitCode {
    let module = module.to_ascii_lowercase();
    let (game, index) = match install_with(discovery, &module, game) {
        Ok(found) => found,
        Err(error) => {
            eprintln!("detail-bake: {error}");
            return ExitCode::FAILURE;
        }
    };
    match bake_module(game, &index, &module, lights, options, out) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("detail-bake: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The default folder: `target/detail-bake` under the current folder.
pub fn default_out() -> PathBuf {
    PathBuf::from("target").join("detail-bake")
}

fn bake_module(
    game: Game,
    index: &DirIndex,
    module: &str,
    lights: &Path,
    options: &GpuOptions,
    out: &Path,
) -> Result<(), String> {
    let base = Resolver::base(game, index, TextureQuality::High).map_err(|e| e.to_string())?;
    let resolver = base
        .with_module(index, module)
        .map_err(|error| error.to_string())?;
    let built =
        ModuleIr::build(module, &ResolverSource(&resolver)).map_err(|error| format!("{error:?}"))?;
    let mut reader = Reader::new(&resolver, module);
    // A light that reaches farther than `FAR_LIGHT` is no lamp of a room.
    let (far, fitted): (Vec<Fitted>, Vec<Fitted>) =
        read_fitted(lights)?.into_iter().partition(|light| light.radius > FAR_LIGHT);
    let _ = SWITCHED.set(read_switched(lights)?);
    for area in &built.areas {
        let folder = out.join(&area.resref);
        std::fs::create_dir_all(&folder).map_err(|error| format!("{}: {error}", folder.display()))?;
        let report = gpu::bake_lights((&mut reader, &resolver, game), area, (&fitted, &far), options, &folder)?;
        outln!("{report}");
        std::fs::write(folder.join("detail-bake.txt"), &report)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
        // Each set the bake wrote gets the fingerprint of the files it was
        // made from, so that the game shows it only over those files. Each
        // set's exact lines name the lightmaps of its own PNGs.
        let current = kjk_game::bake_fingerprint::current(&resolver, area);
        for set in [folder.clone(), folder.join("a"), folder.join("b")] {
            if holds_lightmaps(&set) {
                stamp_set(&set, &current)?;
            }
        }
        outln!("wrote into {}", folder.display());
    }
    Ok(())
}

/// Writes into the set `set` its fingerprint over the files `current`,
/// with the exact lines of its own PNGs. Names the PNGs that no room mesh
/// draws, such as a lightmap baked only for the grass under a walkmesh.
fn stamp_set(set: &Path, current: &kjk_game::bake_fingerprint::Current) -> Result<(), String> {
    let pngs = kjk_game::bake_fingerprint::set_lightmaps(set);
    let (print, undrawn) = kjk_game::bake_fingerprint::stamp(current, &pngs);
    if !undrawn.is_empty() {
        outln!(
            "{}: no room mesh draws these lightmaps, so they are stamped as drawn by no triangles: {}",
            set.display(),
            undrawn.join(", ")
        );
    }
    kjk_game::bake_fingerprint::write(set, &print)
}

/// Whether folder `set` holds a PNG, as a set of baked lightmaps does.
fn holds_lightmaps(set: &Path) -> bool {
    std::fs::read_dir(set).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().extension().is_some_and(|extension| extension == "png"))
    })
}

/// The install of `game` (or of either KOTOR game) that has module
/// `module`, indexed.
fn install_with(discovery: &Discovery, module: &str, game: Option<&str>) -> Result<(Game, DirIndex), String> {
    let games = match game {
        None => vec![Game::Kotor1, Game::Kotor2],
        Some(key) => match Game::from_key(key).filter(|game| *game != Game::JediAcademy) {
            Some(game) => vec![game],
            None => return Err("use k1 or tsl".to_string()),
        },
    };
    for game in games {
        let Some(install) = discovery.get(game) else {
            continue;
        };
        let index = DirIndex::of_install(install).map_err(|error| format!("{}: {error}", game.key()))?;
        if module_names(&index).contains(&module.to_string()) {
            return Ok((game, index));
        }
    }
    Err(format!("no install found has module {module}"))
}

/// `kjk detail-bake <module> --fingerprint` and `--stamp <set>`: prints the
/// fingerprint of each of the module's areas in the files the game reads
/// now (`kjk_game::bake_fingerprint`). With `stamp` it also writes it into
/// that set of lightmaps baked earlier. The set is taken to be of the
/// module's only area, or of the area that its folder or the folder above
/// it is named after.
pub fn fingerprint(discovery: &Discovery, module: &str, game: Option<&str>, stamp: Option<&Path>) -> ExitCode {
    match print_or_stamp(discovery, &module.to_ascii_lowercase(), game, stamp) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("detail-bake: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_or_stamp(discovery: &Discovery, module: &str, game: Option<&str>, stamp: Option<&Path>) -> Result<(), String> {
    let (game, index) = install_with(discovery, module, game)?;
    let base = Resolver::base(game, &index, TextureQuality::High).map_err(|e| e.to_string())?;
    let resolver = base.with_module(&index, module).map_err(|error| error.to_string())?;
    let built = ModuleIr::build(module, &ResolverSource(&resolver)).map_err(|error| format!("{error:?}"))?;
    let prints: Vec<kjk_game::bake_fingerprint::Current> =
        (built.areas.iter()).map(|area| kjk_game::bake_fingerprint::current(&resolver, area)).collect();
    for kjk_game::bake_fingerprint::Current { print, .. } in &prints {
        outln!("area {} format {} rooms {} placeables {}", print.area, print.format, print.rooms, print.placeables);
    }
    let Some(set) = stamp else {
        return Ok(());
    };
    if !holds_lightmaps(set) {
        return Err(format!("{} holds no baked lightmaps", set.display()));
    }
    let named = |path: Option<&Path>| {
        let name = path?.file_name()?.to_str()?.to_ascii_lowercase();
        prints.iter().find(|current| current.print.area == name)
    };
    let print = match prints.as_slice() {
        [only] => only,
        _ => named(Some(set))
            .or_else(|| named(set.parent()))
            .ok_or_else(|| format!("{} is not named after one of the module's areas", set.display()))?,
    };
    stamp_set(set, print)?;
    outln!("wrote {}", set.join(kjk_game::bake_fingerprint::FILE).display());
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
    /// How many new texels the bake writes to each side of a stored texel
    /// (`SCALE`, or more under `KJK_BAKE_TEXEL`).
    scale: usize,
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
        scale: SCALE,
    })
}

/// The mean colour of a texture in linear light, from a small level, and
/// the mean of its alpha.
fn albedo(resolver: &Resolver, name: &str) -> Option<(Vec3, f32)> {
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
    let mut alpha = 0.0f32;
    let mut count = 0.0f32;
    for [r, g, b, a] in rgba.as_chunks::<4>().0.iter().take(width * height) {
        sum += Vec3::new(
            linear(f32::from(*r) / 255.0),
            linear(f32::from(*g) / 255.0),
            linear(f32::from(*b) / 255.0),
        );
        alpha += f32::from(*a) / 255.0;
        count += 1.0;
    }
    (count > 0.0).then(|| (sum / count, alpha / count))
}

/// A mesh with a lightmap whose texture's alpha has a mean under this is a
/// layer the game draws over another by its alpha: wet sand, foam or a
/// shore's water. It casts no shadow and walls no texel in. As a solid
/// sheet the sand layers of unk_m41ac walled in the shore's bottom, and the
/// bake left hard black shapes there (22 November 2026). A mesh with no
/// lightmap, such as leaves, still casts its shadow.
const LAYER_ALPHA: f32 = 0.9;

// ---------------------------------------------------------------------------
// The texels.

/// What a ray met, beside its corners in the `Level`.
struct Facts {
    normal: Vec3,
    albedo: Vec3,
    map: Option<usize>,
    uv: [Vec2; 3],
    /// The light the surface gives off by itself, in linear light: its
    /// texture's mean colour times its own glow.
    glow: Vec3,
    /// The mesh it belongs to, by its number in `Texels::names`.
    mesh: u32,
}

/// A placeable with a mesh the bake may light
/// (`kjk_game::placeable_light::bends`), as placed in the area.
pub(crate) struct Still {
    /// Its number among the area's instances.
    pub(crate) object: usize,
    /// Whether the bake may light each of its meshes. A mesh that a clip
    /// moves is lit where it rests.
    pub(crate) lit: Vec<bool>,
    /// Its model's name, in lower case.
    pub(crate) name: String,
    pub(crate) model: std::rc::Rc<room_lights::Loaded>,
    pub(crate) frame: asset_odyssey::model::Matrix,
    /// Where it stands (area space).
    pub(crate) at: Vec3,
}

impl Still {
    /// The corners of `mesh` in the area.
    pub(crate) fn corners(&self, mesh: &asset_odyssey::model::MeshIr) -> Vec<Vec3> {
        let node = multiply(&self.frame, &self.model.ir.nodes[mesh.node].world);
        (mesh.positions.iter())
            .map(|p| Vec3::from_array(room_lights::apply(&node, *p)))
            .collect()
    }

    /// The normals of `mesh` in the area, one for each corner. A mesh that
    /// stores none takes the mean of its triangles' normals at each corner.
    pub(crate) fn normals(&self, mesh: &asset_odyssey::model::MeshIr) -> Vec<Vec3> {
        let node = multiply(&self.frame, &self.model.ir.nodes[mesh.node].world);
        if mesh.normals.len() == mesh.positions.len() {
            return (mesh.positions.iter().zip(&mesh.normals))
                .map(|(p, n)| {
                    let tip = [p[0] + n[0], p[1] + n[1], p[2] + n[2]];
                    (Vec3::from_array(room_lights::apply(&node, tip))
                        - Vec3::from_array(room_lights::apply(&node, *p)))
                    .normalize_or_zero()
                })
                .collect();
        }
        let corners = self.corners(mesh);
        let mut sums = vec![Vec3::ZERO; corners.len()];
        for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
            let corner = |i: u32| corners.get(i as usize).copied();
            if let (Some(pa), Some(pb), Some(pc)) = (corner(a), corner(b), corner(c)) {
                let front = (pb - pa).cross(pc - pa);
                for i in [a, b, c] {
                    sums[i as usize] += front;
                }
            }
        }
        sums.into_iter().map(Vec3::normalize_or_zero).collect()
    }
}

/// The placeables of `area` with a mesh the bake may light, as the game
/// finds them (`kjk_game::placeable_light`). Each model's report says how
/// many of its meshes keep still, and why each other one moves.
pub(crate) fn still_placeables(reader: &mut Reader, resolver: &Resolver, area: &AreaIr, game: Game) -> Vec<Still> {
    let mut cache: BTreeMap<String, Option<asset_odyssey::model::ModelIr>> = BTreeMap::new();
    let mut lit_of: HashMap<String, Vec<bool>> = HashMap::new();
    let mut found = Vec::new();
    for (object, instance) in area.instances.iter().enumerate() {
        if instance.kind != asset_odyssey::module::Kind::Placeable {
            continue;
        }
        let Some(name) = instance.model.as_deref().map(str::to_ascii_lowercase) else {
            continue;
        };
        let Some(model) = reader.model(&name) else {
            continue;
        };
        let lit = lit_of.entry(name.clone()).or_insert_with(|| {
            let clips = kjk_game::placeable_light::clips(&mut cache, resolver, game, &model.ir);
            let motions = kjk_game::placeable_light::motions(&model.ir, &clips);
            let lit = kjk_game::placeable_light::lit_meshes(&model.ir);
            let why: std::collections::BTreeSet<&str> = motions.iter().flatten().map(String::as_str).collect();
            outln!(
                "  placeable {name}: {} of {} meshes keep still, and the bake lights {}{}",
                motions.iter().filter(|motion| motion.is_none()).count(),
                motions.len(),
                lit.iter().filter(|lit| **lit).count(),
                if why.is_empty() { String::new() } else { format!(": {}", why.into_iter().collect::<Vec<_>>().join("; ")) }
            );
            lit
        });
        if lit.iter().any(|lit| *lit) {
            found.push(Still {
                object,
                lit: lit.clone(),
                name,
                model,
                frame: room_lights::frame(instance.position, instance.facing),
                at: Vec3::from_array(instance.position),
            });
        }
    }
    found
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
    /// The mesh's own normal at the texel, as the shader reads it between
    /// the corners. A mesh that stores no normals has none.
    shading: Option<Vec3>,
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
    shading: Option<Vec3>,
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

/// The room surfaces of an area and the new texels on them, as the bake
/// reads them.
struct Texels {
    /// The stored lightmaps of the rooms with a walkable face.
    maps: Vec<Stored>,
    /// The opaque triangles of those rooms, which block rays, and what
    /// each one is.
    surfaces: Vec<Surface>,
    facts: Vec<Facts>,
    /// For each lightmap, the triangle each new texel belongs to.
    claims: Vec<Vec<Option<Claim>>>,
    /// The lightmaps of rooms with no walkable face (a sky, a backdrop).
    /// They are not baked.
    unwalked: Vec<String>,
    /// The meshes' names (its node, then its texture), for the probe of the
    /// bake on the graphics card.
    names: Vec<String>,
    /// How many new texels lie under the area's grass and on no drawn mesh
    /// (`grass_texels`).
    grass: usize,
}

/// The box around every corner of `surfaces`.
fn bounds(surfaces: &[Surface]) -> (Vec3, Vec3) {
    surfaces.iter().flat_map(|s| s.corners).fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), p| (low.min(p), high.max(p)),
    )
}

/// Reads an area's rooms into `Texels`.
///
/// With `every_room`, a room with no walkable face is read too where one of
/// its meshes has a lightmap. Each house of Anchorhead is such a room, and so
/// is much of its ground. Left out, a house cast no shadow, and the game
/// showed its stored lightmap beside the baked ground. The owner said of
/// that on 19 November 2026: "this area is very strange and seems to have
/// large squares of ground in different light/colors". A bake with a sun
/// reads every room. A bake without one leaves them out as before: there
/// such a room is a backdrop outside a window.
///
/// The placeables of `occluders` block rays as well. Their triangles have no
/// lightmap, so a bounce ray that meets one takes the light at the
/// triangle's middle, as it does from a room's mesh with no lightmap.
fn texels(
    reader: &mut Reader,
    resolver: &Resolver,
    area: &AreaIr,
    every_room: bool,
    occluders: &[&Still],
) -> Result<Texels, String> {
    let placement = room_lights::place(reader, area);
    let Placement {
        objects, walkable, ..
    } = &placement;

    let mut unwalked: Vec<String> = Vec::new();
    let mut albedos: HashMap<String, (Vec3, f32)> = HashMap::new();
    let mut maps: Vec<Stored> = Vec::new();
    let mut map_of: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut claims: Vec<Vec<Option<Claim>>> = Vec::new();
    // Each lightmap's surface in square metres and in stored texels, and the
    // triangles to give texels once its scale is known.
    let mut densities: Vec<(f32, f32)> = Vec::new();
    #[allow(clippy::type_complexity)]
    let mut pending: Vec<(usize, [Vec3; 3], [[f32; 2]; 3], Option<[Vec3; 3]>)> = Vec::new();
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut facts: Vec<Facts> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for object in objects {
        let Owner::Room(room) = object.owner else {
            continue;
        };
        let lightmapped = || object.model.ir.meshes.iter().any(|mesh| mesh.lightmap.is_some());
        if !walkable[room] && !(every_room && lightmapped()) {
            unwalked.extend(
                (object.model.ir.meshes.iter())
                    .filter_map(|mesh| mesh.lightmap.as_deref())
                    .map(str::to_ascii_lowercase),
            );
            continue;
        }
        let ir = &object.model.ir;
        for (index, mesh) in ir.meshes.iter().enumerate() {
            if mesh.skin.is_some() {
                continue;
            }
            // Of a room with no walkable face only the meshes with a
            // lightmap are read. Its other meshes are the sky and the far
            // land, which must not shade the area.
            if !walkable[room] && mesh.lightmap.is_none() {
                continue;
            }
            let node = multiply(&object.frame, &ir.nodes[mesh.node].world);
            let world: Vec<Vec3> = (mesh.positions.iter())
                .map(|p| Vec3::from_array(room_lights::apply(&node, *p)))
                .collect();
            // The mesh's normals in the area, where it stores one for each
            // corner.
            let normals: Option<Vec<Vec3>> = (mesh.normals.len() == mesh.positions.len())
                .then(|| {
                    (mesh.positions.iter().zip(&mesh.normals))
                        .map(|(p, n)| {
                            let tip = [p[0] + n[0], p[1] + n[1], p[2] + n[2]];
                            (Vec3::from_array(room_lights::apply(&node, tip))
                                - Vec3::from_array(room_lights::apply(&node, *p)))
                            .normalize_or_zero()
                        })
                        .collect()
                });
            // A lamp that the game switches on glows too, on its tubes
            // alone (`SWITCHED_TUBES`).
            let switched = (SWITCHED.get())
                .and_then(|switched| switched.get(&(ir.name.to_ascii_lowercase(), index)))
                .map_or(0.0, |share| share * SWITCHED_TUBES);
            let glow = (Vec3::from_array(mesh.self_illum).max(Vec3::ZERO))
                .max(Vec3::splat(switched))
                .powf(2.2);
            names.push(format!(
                "{} ({})",
                ir.nodes[mesh.node].name,
                mesh.texture.as_deref().unwrap_or("no texture")
            ));
            let mesh_number = (names.len() - 1) as u32;
            let map = match mesh.lightmap.as_deref() {
                Some(name) if mesh.uv2.len() == mesh.positions.len() => {
                    *map_of.entry(name.to_ascii_lowercase()).or_insert_with(|| {
                        let stored = read_stored(resolver, name)?;
                        claims.push(Vec::new());
                        densities.push((0.0, 0.0));
                        maps.push(stored);
                        Some(maps.len() - 1)
                    })
                }
                _ => None,
            };
            let (colour, coverage) = mesh.texture.as_deref().map_or((Vec3::splat(0.5), 1.0), |name| {
                *albedos
                    .entry(name.to_ascii_lowercase())
                    .or_insert_with(|| albedo(resolver, name).unwrap_or((Vec3::splat(0.5), 1.0)))
            });
            let layer = mesh.lightmap.is_some() && coverage < LAYER_ALPHA;
            let opaque = mesh.transparency_hint == 0
                && mesh.alpha >= 0.99
                && !layer
                && (mesh.texture.is_some() || mesh.lightmap.is_some());
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
                        glow: colour * glow,
                        mesh: mesh_number,
                    });
                }
                let Some(map) = map else {
                    continue;
                };
                let stored = &maps[map];
                let corner_normals = normals
                    .as_ref()
                    .map(|normals| [a, b, c].map(|i| normals[i as usize]));
                let uv: [[f32; 2]; 3] = [a, b, c].map(|i| mesh.uv2[i as usize]);
                let twice = (uv[1][0] - uv[0][0]) * (uv[2][1] - uv[0][1])
                    - (uv[2][0] - uv[0][0]) * (uv[1][1] - uv[0][1]);
                let density = &mut densities[map];
                density.0 += (pb - pa).cross(pc - pa).length();
                density.1 += twice.abs() * (stored.width * stored.height) as f32;
                pending.push((map, [pa, pb, pc], uv, corner_normals));
            }
        }
    }
    for still in occluders {
        let ir = &still.model.ir;
        // A mesh that moves blocks rays at rest, unless a skin or its
        // dangles move it, since its place is then not known.
        for mesh in ir.meshes.iter().filter(|mesh| mesh.skin.is_none() && mesh.dangly.is_none()) {
            let (colour, coverage) = mesh.texture.as_deref().map_or((Vec3::splat(0.5), 1.0), |name| {
                *albedos
                    .entry(name.to_ascii_lowercase())
                    .or_insert_with(|| albedo(resolver, name).unwrap_or((Vec3::splat(0.5), 1.0)))
            });
            let opaque = mesh.transparency_hint == 0
                && mesh.alpha >= 0.99
                && coverage >= LAYER_ALPHA
                && mesh.texture.is_some();
            if !opaque {
                continue;
            }
            names.push(format!(
                "{} ({})",
                ir.nodes[mesh.node].name,
                mesh.texture.as_deref().unwrap_or("no texture")
            ));
            let mesh_number = (names.len() - 1) as u32;
            let glow = Vec3::from_array(mesh.self_illum).max(Vec3::ZERO).powf(2.2);
            let world = still.corners(mesh);
            for &[a, b, c] in mesh.indices.as_chunks::<3>().0 {
                let corner = |i: u32| world.get(i as usize).copied();
                let (Some(pa), Some(pb), Some(pc)) = (corner(a), corner(b), corner(c)) else {
                    continue;
                };
                let front = (pb - pa).cross(pc - pa);
                if front.length() < 1e-8 {
                    continue;
                }
                surfaces.push(Surface {
                    corners: [pa, pb, pc],
                    uv: [Vec2::ZERO; 3],
                    lightmap: None,
                    texture: None,
                    texture_uv: [Vec2::ZERO; 3],
                    glow: Vec3::ZERO,
                });
                facts.push(Facts {
                    normal: front.normalize(),
                    albedo: colour,
                    map: None,
                    uv: [Vec2::ZERO; 3],
                    glow: colour * glow,
                    mesh: mesh_number,
                });
            }
        }
    }
    // Each lightmap's scale, from how many metres its stored texel covers
    // (`scale_for`), and then its triangles' texels.
    for (map, stored) in maps.iter_mut().enumerate() {
        let (world, texels) = densities.get(map).copied().unwrap_or((0.0, 0.0));
        let metres = if texels > 0.0 { (world / texels).sqrt() } else { 0.0 };
        stored.scale = scale_for(metres, stored.width.max(stored.height));
    }
    fit_field(&area.resref, &mut maps);
    for (map, stored) in maps.iter().enumerate() {
        claims[map] = vec![None; stored.width * stored.height * stored.scale * stored.scale];
    }
    for (map, corners, uv, normals) in pending {
        let stored = &maps[map];
        claim_triangle(
            &mut claims[map],
            (stored.width * stored.scale, stored.height * stored.scale),
            stored.scale,
            corners,
            uv,
            normals,
        );
    }
    let grass = grass_texels(resolver, area, objects, walkable, &mut maps, &mut map_of, &mut claims);
    if maps.is_empty() {
        return Err(format!("{}: no lightmap on a room", area.resref));
    }
    unwalked.sort();
    unwalked.dedup();
    unwalked.retain(|name| !map_of.contains_key(name));
    Ok(Texels {
        maps,
        surfaces,
        facts,
        claims,
        unwalked,
        names,
        grass,
    })
}

/// Gives the new texels that the triangle `corners` covers on its lightmap
/// to that triangle. `claims` holds the lightmap's new texels, `w` by `h`
/// of them. `uv2` is each corner's place on the lightmap, from 0 to 1.
/// `normals` is the mesh's own normal at each corner, where it stores them.
/// A texel that a triangle nearer to its centre has claimed is left alone.
fn claim_triangle(
    claims: &mut [Option<Claim>],
    (w, h): (usize, usize),
    scale: usize,
    corners: [Vec3; 3],
    uv2: [[f32; 2]; 3],
    normals: Option<[Vec3; 3]>,
) {
    let [pa, pb, pc] = corners;
    let front = (pb - pa).cross(pc - pa);
    if front.length() < 1e-8 {
        return;
    }
    let normal = front.normalize();
    // The corners in new texels.
    let uv = uv2.map(|uv| [uv[0] * w as f32, uv[1] * h as f32]);
    let twice =
        (uv[1][0] - uv[0][0]) * (uv[2][1] - uv[0][1]) - (uv[2][0] - uv[0][0]) * (uv[1][1] - uv[0][1]);
    if twice.abs() < 1e-6 {
        return;
    }
    let sign = twice.signum();
    let low = |axis: usize| uv.iter().map(|p| p[axis]).fold(f32::MAX, f32::min);
    let high = |axis: usize| uv.iter().map(|p| p[axis]).fold(f32::MIN, f32::max);
    if (high(0) - low(0)) * (high(1) - low(1)) > 4.0 * (w * h) as f32 {
        return;
    }
    let middle = (pa + pb + pc) / 3.0;
    // A stored texel's side on this triangle: the triangle's area in the
    // room over its area on the stored lightmap.
    let size = (front.length() / (twice.abs() / (scale * scale) as f32)).sqrt();
    let rows = (low(1) - 0.5 - FOOTPRINT).ceil() as i64..=(high(1) - 0.5 + FOOTPRINT).floor() as i64;
    for y in rows {
        let columns =
            (low(0) - 0.5 - FOOTPRINT).ceil() as i64..=(high(0) - 0.5 + FOOTPRINT).floor() as i64;
        for x in columns {
            let centre = [x as f32 + 0.5, y as f32 + 0.5];
            let edges: [f32; 3] =
                std::array::from_fn(|k| sign * room_lights::edge(uv[k], uv[(k + 1) % 3], centre));
            let outside = edges.iter().fold(0.0f32, |most, e| most.max(-e));
            if outside > FOOTPRINT {
                continue;
            }
            let texel = (y.rem_euclid(h as i64) as usize) * w + x.rem_euclid(w as i64) as usize;
            if claims[texel].is_some_and(|claim| claim.outside <= outside) {
                continue;
            }
            // The weights of the three corners at the centre. They add up
            // to 1, and one is below 0 outside.
            let weights: [f32; 3] = std::array::from_fn(|k| {
                let (p, q) = (uv[(k + 1) % 3], uv[(k + 2) % 3]);
                ((q[0] - p[0]) * (centre[1] - p[1]) - (q[1] - p[1]) * (centre[0] - p[0])) / twice
            });
            let at: Vec3 = (0..3).map(|k| corners[k] * weights[k]).sum();
            let anchor = if outside > 0.0 {
                let edge = closest(corners, at);
                let inward = middle - edge;
                edge + inward * (INWARD / inward.length().max(INWARD))
            } else {
                at
            };
            // The mesh's normal between the corners, with the weights kept
            // on the triangle.
            let shading = normals.and_then(|normals| {
                let kept = weights.map(|w| w.max(0.0));
                let total: f32 = kept.iter().sum();
                let sum: Vec3 = (0..3).map(|k| normals[k] * kept[k]).sum();
                (total > 0.0).then(|| sum.normalize_or_zero())
            });
            claims[texel] = Some(Claim {
                outside,
                at,
                anchor,
                normal,
                size,
                shading: shading.filter(|n| *n != Vec3::ZERO),
            });
        }
    }
}

/// How far over its walkmesh face a grass texel gathers its light (metres).
/// A walkmesh lies on the drawn ground or a little under it, and a texel
/// under the ground would gather nothing.
const GRASS_LIFT: f32 = 0.1;

/// Gives the texels under the area's grass to the walkmesh faces the grass
/// grows on, and says how many they are.
///
/// The game lights each tuft of grass by the lightmap of its room's
/// walkmesh, at the walkmesh's own place on it (`asset_odyssey::grass`).
/// The walkmesh is not drawn. Where no drawn mesh covers those texels, a
/// bake that fills only the drawn meshes' texels leaves them black, and the
/// grass is then black on lit ground. The owner reported it in the
/// Dantooine enclave's first courtyard on 19 November 2026: "grass is black
/// here".
///
/// A texel that a drawn mesh covers stays that mesh's. The walkmesh blocks
/// no ray.
fn grass_texels(
    resolver: &Resolver,
    area: &AreaIr,
    objects: &[room_lights::Object],
    walkable: &[bool],
    maps: &mut Vec<Stored>,
    map_of: &mut BTreeMap<String, Option<usize>>,
    claims: &mut Vec<Vec<Option<Claim>>>,
) -> usize {
    if area.grass.is_none() {
        return 0;
    }
    let grassy = collision_odyssey::Surfaces::read(&ResolverSource(resolver))
        .map_or_else(Vec::new, |surfaces| surfaces.grass());
    if grassy.is_empty() {
        return 0;
    }
    let mut under: BTreeMap<usize, Vec<Option<Claim>>> = BTreeMap::new();
    for object in objects {
        let Owner::Room(room) = object.owner else {
            continue;
        };
        if !walkable[room] {
            continue;
        }
        let Ok(Some(model)) = crate::viewer::parsed(resolver, &area.rooms[room].model) else {
            continue;
        };
        let faces = asset_odyssey::grass::faces(&model, &grassy);
        let Some(name) = faces.lightmap.as_deref().filter(|_| !faces.faces.is_empty()) else {
            continue;
        };
        let Some(map) = *map_of.entry(name.to_ascii_lowercase()).or_insert_with(|| {
            let stored = read_stored(resolver, name)?;
            claims.push(vec![None; stored.width * stored.height * stored.scale * stored.scale]);
            maps.push(stored);
            Some(maps.len() - 1)
        }) else {
            continue;
        };
        let size = (maps[map].width * maps[map].scale, maps[map].height * maps[map].scale);
        let theirs = under.entry(map).or_insert_with(|| vec![None; size.0 * size.1]);
        for face in &faces.faces {
            let mut corners = face
                .corners
                .map(|corner| Vec3::from_array(room_lights::apply(&object.frame, corner)));
            let mut uv2 = face.lightmap;
            // A face that looks down is turned over, so its texels gather
            // the light from above.
            if (corners[1] - corners[0]).cross(corners[2] - corners[0]).z < 0.0 {
                corners.swap(1, 2);
                uv2.swap(1, 2);
            }
            let up = (corners[1] - corners[0]).cross(corners[2] - corners[0]).normalize_or_zero();
            claim_triangle(theirs, size, maps[map].scale, corners.map(|corner| corner + up * GRASS_LIFT), uv2, None);
        }
    }
    let mut taken = 0;
    for (map, theirs) in under {
        for (texel, claim) in theirs.into_iter().enumerate() {
            if claim.is_some() && claims[map][texel].is_none() {
                claims[map][texel] = claim;
                taken += 1;
            }
        }
    }
    taken
}

/// The new texels on the surfaces, from the triangles they belong to.
fn points(claims: Vec<Vec<Option<Claim>>>, level: &Level) -> Vec<Point> {
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
    parallel(claimed.len(), |index| {
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
            shading: claim.shading,
        }
    })
}

fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

// ---------------------------------------------------------------------------
// The game's fitted lights (`--lights`).
//
// The game writes its fitted lights to a file when `KJK_FITTED_DUMP` names
// one (`fitted_lights::dump`). The bake reads the lines of that file that
// start with `fitted` and `switched`. The live trial shows a room's surface
// at `((stored / 2)^2.2 + (0.8 × S)^2.2)^(1/2.2)`, where `S` is the fitted
// lights' sum (`odyssey.wgsl`, `fitted_lights`). Set `a` keeps that formula
// with shadows and bounced light, and set `b` leaves the stored lightmap
// out (`gpu.rs`).
//
// Both sets are stored at 1 / `GAIN` of their level, so a level above 1 beside a
// lamp fits in the PNG. The game multiplies them back under
// `KJK_FITTED_LIGHTS=baked` (`BAKED_GAIN` in `odyssey.wgsl`).

/// The level a baked lightmap is stored at is its level over this.
const GAIN: f32 = 2.0;
/// How much of the fitted lights' sum the live trial adds
/// (`FIGHT_GAIN_ENHANCED` in `odyssey.wgsl`).
const LIVE_GAIN: f32 = 0.8;
/// The curve the live trial's formula adds light on.
const GAMMA: f32 = 2.2;
/// A ray toward a light stops this far short of it (metres), so the
/// light's own fitting, which stands 0.25 metres behind it, casts no
/// shadow on it.
const SHORT: f32 = 0.35;
/// The side of a cell of the grid that finds the lights near a place
/// (metres).
const LIGHT_CELL: f32 = 2.0;
/// How many rays test whether a texel is inside a wall, squared.
const WALL_RAYS: usize = 4;

/// A light that reaches farther than this, in metres, is left out of the
/// bake's lamps. Anchorhead's rooms hold three hidden lights a kilometre
/// off that reach 5,000 and 18,000 metres: the games' own suns, which light
/// the characters. The grid that finds the lights near a place has a cell
/// for every 2 metres of a light's reach, and those three filled 21 GB
/// before the bake had lit a texel. An area's sun comes from its `sun` in
/// `data/lights/bake.toml`, and the report prints the way to each such
/// light to help write it.
const FAR_LIGHT: f32 = 500.0;

/// A fitted light as the game's shader reads it (`fitted_lights::Light`).
struct Fitted {
    at: Vec3,
    colour: Vec3,
    radius: f32,
    way: Vec3,
    half: f32,
    aim: Vec3,
    cone: f32,
}

/// The rooms' lamps that the game switches on although the files do not set
/// them to glow (`fitted_lights::switched_on`), from the `switched` lines of
/// the file that `--lights` names: how much each shines, by its model's
/// name in lower case and the number of its mesh.
static SWITCHED: std::sync::OnceLock<HashMap<(String, usize), f32>> = std::sync::OnceLock::new();
/// How much of a switched lamp's mesh glows: its tubes do, and its housing
/// does not. The game shows the glow on the mesh's bright texels alone.
const SWITCHED_TUBES: f32 = 0.5;

/// The `switched` lines of a file that `KJK_FITTED_DUMP` wrote.
fn read_switched(path: &Path) -> Result<HashMap<(String, usize), f32>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut switched = HashMap::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("switched ") else {
            continue;
        };
        let bad = || format!("{}: {line}: not a model, a mesh and a share", path.display());
        let mut words = rest.split_whitespace();
        let (Some(model), Some(mesh), Some(share)) = (words.next(), words.next(), words.next())
        else {
            return Err(bad());
        };
        let mesh: usize = mesh.parse().map_err(|_| bad())?;
        let share: f32 = share.parse().map_err(|_| bad())?;
        switched.insert((model.to_ascii_lowercase(), mesh), share);
    }
    Ok(switched)
}

/// The `fitted` lines of a file that `KJK_FITTED_DUMP` wrote.
fn read_fitted(path: &Path) -> Result<Vec<Fitted>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut lights = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.strip_prefix("fitted ") else {
            continue;
        };
        let v: Vec<f32> = rest
            .split_whitespace()
            .map(str::parse)
            .collect::<Result<_, _>>()
            .map_err(|error| format!("{}: {line}: {error}", path.display()))?;
        if v.len() != 15 {
            return Err(format!("{}: {line}: not 15 numbers", path.display()));
        }
        lights.push(Fitted {
            at: Vec3::new(v[0], v[1], v[2]),
            colour: Vec3::new(v[3], v[4], v[5]),
            radius: v[6],
            way: Vec3::new(v[7], v[8], v[9]).normalize_or_zero(),
            half: v[10],
            aim: Vec3::new(v[11], v[12], v[13]).normalize_or_zero(),
            cone: v[14],
        });
    }
    if lights.is_empty() {
        return Err(format!("{}: no line starts with fitted", path.display()));
    }
    Ok(lights)
}

impl Fitted {
    /// The nearest place of the light to `at`: on a strip, the nearest of
    /// its line.
    fn nearest(&self, at: Vec3) -> Vec3 {
        let away = self.at - at;
        at + away + self.way * (-away.dot(self.way)).clamp(-self.half, self.half)
    }

    /// What the light gives a surface at `at` whose normal is `normal`
    /// (none for a mesh without normals), as `odyssey.wgsl`'s
    /// `fitted_lights` works it out for one light.
    fn gives(&self, at: Vec3, normal: Option<Vec3>) -> Vec3 {
        let to = self.nearest(at) - at;
        let distance = to.length();
        if distance >= self.radius {
            return Vec3::ZERO;
        }
        let toward = to / distance.max(1e-4);
        let facing = normal.map_or(1.0, |normal| 0.3 + 0.7 * normal.dot(toward).max(0.0));
        let cone = if self.cone > 0.0 {
            0.33 + 0.67 * smooth(self.cone - 0.5, self.cone + 0.1, -self.aim.dot(toward))
        } else {
            1.0
        };
        self.colour * (room_lights_falloff(distance, self.radius) * facing * cone)
    }
}

/// How much of a light is left at a distance (`fitted_lights::falloff`).
fn room_lights_falloff(distance: f32, radius: f32) -> f32 {
    if distance >= radius {
        return 0.0;
    }
    let edge = (1.0 - (distance / radius).powi(4)).max(0.0);
    edge * edge / (1.0 + (distance / (0.45 * radius)).powi(2))
}

/// The lights near each cell of `LIGHT_CELL` metres.
struct LightGrid<'l> {
    lights: &'l [Fitted],
    cells: HashMap<IVec3, Vec<u32>>,
}

impl<'l> LightGrid<'l> {
    fn new(lights: &'l [Fitted]) -> LightGrid<'l> {
        let mut cells: HashMap<IVec3, Vec<u32>> = HashMap::new();
        for (index, light) in lights.iter().enumerate() {
            let reach = Vec3::splat(light.radius + light.half);
            let low = ((light.at - reach) / LIGHT_CELL).floor().as_ivec3();
            let high = ((light.at + reach) / LIGHT_CELL).floor().as_ivec3();
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
        LightGrid { lights, cells }
    }
}

/// The live trial's added light as linear light on its curve:
/// `(0.8 × S)^2.2`.
fn direct(sum: Vec3) -> Vec3 {
    (sum * LIVE_GAIN).max(Vec3::ZERO).powf(GAMMA)
}

/// A light on the curve as a stored level.
fn stored_level(light: Vec3) -> Vec3 {
    light.max(Vec3::ZERO).powf(1.0 / GAMMA)
}

/// A value for each new texel of each lightmap, where it has one.
type Field = Vec<Vec<Option<Vec3>>>;

/// Gives each texel without a value the mean of its neighbours that have
/// one, a ring at a time, `FILL_RINGS` times at most.
fn fill(field: &mut [Option<Vec3>], w: usize, h: usize) {
    for _ in 0..FILL_RINGS {
        let before = field.to_vec();
        let mut any = false;
        for y in 0..h {
            for x in 0..w {
                if before[y * w + x].is_some() {
                    continue;
                }
                let (mut sum, mut count) = (Vec3::ZERO, 0.0f32);
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                        if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                            continue;
                        }
                        if let Some(value) = before[ny as usize * w + nx as usize] {
                            sum += value;
                            count += 1.0;
                        }
                    }
                }
                if count > 0.0 {
                    field[y * w + x] = Some(sum / count);
                    any = true;
                }
            }
        }
        if !any {
            break;
        }
    }
}

/// The field of `values`, one for each texel, kept where the texel counts
/// and filled in around.
fn field_of(maps: &[Stored], points: &[Point], counts: &[bool], values: &[Vec3]) -> Field {
    let mut field: Field = (maps.iter())
        .map(|map| vec![None; map.width * map.height * map.scale * map.scale])
        .collect();
    for (index, point) in points.iter().enumerate() {
        if counts[index] {
            field[point.map][point.texel] = Some(values[index]);
        }
    }
    for (map, values) in maps.iter().zip(&mut field) {
        fill(values, map.width * map.scale, map.height * map.scale);
    }
    field
}

/// Writes each set of shown levels into `folder/<name>` as PNG lightmaps,
/// at 1 / `GAIN` of their level. A set that keeps the stored lightmap gives
/// a texel no triangle reads half its stored value, and writes a sky's or a
/// backdrop's lightmap at half its stored value. A set without it reads no
/// stored texel.
///
/// `sunlit` is the share of the area's suns that reaches each texel, from 0
/// to 1, where the area has a sun. A set that keeps no stored texel holds it
/// as its alpha, and the game's shadows read it there (`room_shadow` in
/// `odyssey.wgsl`). Without it the alpha is 1.
#[allow(clippy::type_complexity)]
fn write_sets(
    resolver: &Resolver,
    (maps, points, counts): (&[Stored], &[Point], &[bool]),
    unwalked: &[String],
    folder: &Path,
    sets: &[(&str, &[Vec3], bool)],
    sunlit: Option<&[f32]>,
    stored_backdrop: bool,
) -> Result<(), String> {
    let sunlit = sunlit.map(|sunlit| {
        let values: Vec<Vec3> = sunlit.iter().map(|share| Vec3::splat(*share)).collect();
        field_of(maps, points, counts, &values)
    });
    for &(name, shown, keep_stored) in sets {
        let out = folder.join(name);
        std::fs::create_dir_all(&out).map_err(|error| format!("{}: {error}", out.display()))?;
        let field = field_of(maps, points, counts, shown);
        for (number, (map, values)) in maps.iter().zip(&field).enumerate() {
            let (w, h) = (map.width * map.scale, map.height * map.scale);
            let mut picture = image::RgbaImage::new(w as u32, h as u32);
            for y in 0..h {
                for x in 0..w {
                    let uv = Vec2::new((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                    let [r, g, b, a] = map.sample(uv);
                    // A texel no triangle reads keeps half the stored light
                    // in `a` and none in `b`.
                    let value = values[y * w + x].unwrap_or(if keep_stored {
                        Vec3::new(r, g, b) / 2.0
                    } else {
                        Vec3::ZERO
                    });
                    let [r, g, b] = (value / GAIN).to_array().map(to_byte);
                    let a = if keep_stored {
                        to_byte(a)
                    } else {
                        let share = sunlit.as_ref().and_then(|field| field[number][y * w + x]);
                        to_byte(share.map_or(1.0, |share| share.x))
                    };
                    picture.put_pixel(x as u32, y as u32, image::Rgba([r, g, b, a]));
                }
            }
            let path = out.join(format!("{}.png", map.name));
            picture
                .save(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
        // A sky's or a backdrop's lightmap is not baked. In a set that keeps
        // the stored lightmap it shows half its stored light, as the live
        // trial shows it. The lamps' set leaves it out, and the game then
        // shows the stored lightmap at twice its light. With
        // `stored_backdrop` the lamps' set shows it at its stored light, as
        // the game shows it today. On Taris's walkways the buildings and
        // trees glared at twice their light while the walkway, lit by its
        // lamps alone, stayed dark. Elsewhere the owner walked the brighter
        // backdrop, and it stays.
        //
        // The lamps' set holds no picture of such a lightmap, because it
        // would be a copy of the game's own. It names the lightmap in
        // `stored.txt`, and the game makes the same picture from the
        // player's stored lightmap (`stored_fill::backdrop`).
        if !keep_stored {
            let stored_file = out.join(kjk_game::stored_fill::STORED_FILE);
            let _ = std::fs::remove_file(&stored_file);
            if stored_backdrop {
                let named: Vec<&str> = (unwalked.iter())
                    .filter(|name| read_stored(resolver, name).is_some())
                    .map(String::as_str)
                    .collect();
                // A set baked earlier into the same folder may hold their
                // pictures.
                for name in &named {
                    let _ = std::fs::remove_file(out.join(format!("{name}.png")));
                }
                std::fs::write(&stored_file, kjk_game::stored_fill::stored_text(named))
                    .map_err(|error| format!("{}: {error}", stored_file.display()))?;
            }
            continue;
        }
        for name in unwalked {
            let Some(stored) = read_stored(resolver, name) else {
                continue;
            };
            let mut picture = image::RgbaImage::new(stored.width as u32, stored.height as u32);
            for (i, [r, g, b, a]) in stored.texels.iter().enumerate() {
                let [r, g, b] = [r, g, b].map(|v| to_byte(v / 2.0 / GAIN));
                picture.put_pixel(
                    (i % stored.width) as u32,
                    (i / stored.width) as u32,
                    image::Rgba([r, g, b, to_byte(*a)]),
                );
            }
            let path = out.join(format!("{name}.png"));
            picture
                .save(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
        }
    }
    Ok(())
}
