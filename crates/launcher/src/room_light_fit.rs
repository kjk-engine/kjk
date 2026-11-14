//! `kjk room-light-fit <module>`: lights fitted to one area's lightmaps
//! (`docs/LIGHTING-PLAN.md`, step 7c). It opens no window and runs on the
//! CPU. It writes a JSON file, a text file and two pictures for each area
//! of the module into `--out`.
//!
//! BioWare baked the lightmaps from lights that were not shipped. This verb
//! asks how much of a room's baked light the files' own lights explain.
//!
//! - **The candidates.** Every lamp of the area (rooms, doors, placeables
//!   and creatures), each labelled by its owner, its dynamic type, and its
//!   ambient-only and shadow flags. Every glowing mesh that looks like a
//!   light by the rule of `kjk room-lights`, as one rectangle or one sphere
//!   fitted to the bright texels of its texture. A lamp at least
//!   `SUN_RADIUS` wide, which is the sun. One flat term, the same light on
//!   every texel.
//! - **The samples.** Every lightmap texel whose centre lies on a triangle
//!   of a room with a walkable face. The texel's place and facing come from
//!   that triangle through the mesh's second coordinates (`uv2`). Its value
//!   is decoded from the stored sRGB to linear light, and the fit works in
//!   linear light.
//! - **The solve.** Each candidate's light on each sample at strength 1,
//!   with a ray against the room surfaces for what blocks it. Then one
//!   non-negative least-squares solve for each colour channel, so each
//!   candidate gets its own colour.
//! - **The falloff.** Six versions run: no falloff, 1/d and 1/d², each
//!   alone and each ended at the lamp's radius.
//! - **The honesty checks.** One face in `HELD_OUT` is held out of a second
//!   fit and scored alone. `CONTROLS` control fits move every candidate to
//!   a random place in the rooms and solve again.
//! - **One bounce.** For the best falloff, each candidate's light also
//!   reaches a sample after one bounce off a room surface, coloured by the
//!   surface's texture.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::rc::Rc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use asset_odyssey::model::multiply;
use asset_odyssey::module::{AreaIr, ModuleIr};
use asset_odyssey::texture::{Encoding, TextureIr};
use bevy::math::{Vec2, Vec3};
use fmt_odyssey::dxt;
use serde::Serialize;
use vfs::resolver::{Resolver, TextureQuality};
use vfs::{DirIndex, Discovery, Game};

use crate::light_probes::{Level, Surface};
use crate::modules::{ResolverSource, module_names};
use crate::room_lights::{self, Owner, Placement, Reader};

/// How far a lamp's light is followed (metres). The sun's has no end.
const REACH: f32 = 30.0;
/// The distance below which 1/d and 1/d² stop growing (metres).
const NEAR_END: f32 = 0.25;
/// A surface this close to a lamp does not block its light (metres). A
/// lamp often sits inside its own fitting's mesh.
const LAMP_SKIP: f32 = 0.25;
/// How far a sample is lifted off its surface, and moved in toward its
/// triangle's middle (metres). Most samples sit on a face's corner, where
/// two walls meet.
const LIFT: f32 = 0.02;
const INWARD: f32 = 0.05;
/// How far along its ray a sample's ray starts (metres).
const RAY_START: f32 = 0.05;
/// The far end of a glowing panel's light, which has no radius (metres).
/// It is the median radius of the shadow-flagged room lamps of both games.
const PANEL_RADIUS: f32 = 6.0;
/// A lamp this wide (metres) is the sun (`shadows::SUN_RADIUS`).
const SUN_RADIUS: f32 = 100.0;
/// A texel of a glowing texture brighter than this (0 to 1, as stored)
/// gives light (`room_lights::BRIGHT`).
const BRIGHT: f32 = 0.75;
/// The rays a sample casts for one bounce, and how far they reach.
const BOUNCE_RAYS: usize = 32;
const BOUNCE_REACH: f32 = 20.0;
/// How far from where a bounce ray lands its sample may be (metres).
const BOUNCE_NEAR: f32 = 1.0;
/// How near (metres) the samples are whose mean predicts a sample, for
/// `AreaFit::neighbours_r2`.
const NEIGHBOURS: f32 = 1.0;
/// How near (metres) the samples are that `seen` counts.
const SEEN_NEAR: f32 = 10.0;
/// The control fits, each with its own random places.
const CONTROLS: u64 = 3;
/// One face in this many is held out of the held-out fit.
const HELD_OUT: u64 = 4;
/// Which candidates a ray tests for what blocks their light. A lamp without
/// the shadow flag may have cast no shadow in the bake either.
const RAYS: [&str; 3] = [
    "every candidate",
    "the shadow-flagged lamps and the panels",
    "none",
];

fn rays_for(mode: usize, candidate: &Candidate) -> bool {
    match mode {
        0 => true,
        1 => candidate.group == Group::Panel || candidate.shadow,
        _ => false,
    }
}

/// The falloffs, in the order a sample's light holds them.
const FALLOFFS: [&str; 6] = [
    "none",
    "1/d",
    "1/d²",
    "none, ends at the radius",
    "1/d, ends at the radius",
    "1/d², ends at the radius",
];

/// `every_glow` takes every glowing mesh of a room or a placeable as a
/// panel, not only those that look like a light.
pub fn run(
    discovery: &Discovery,
    module: &str,
    game: Option<&str>,
    every_glow: bool,
    out: &Path,
) -> ExitCode {
    let games = match game {
        None => vec![Game::Kotor1, Game::Kotor2],
        Some(key) => match Game::from_key(key).filter(|game| *game != Game::JediAcademy) {
            Some(game) => vec![game],
            None => {
                eprintln!("room-light-fit: use k1 or tsl");
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
        return match fit_module(game, &index, &module, every_glow, out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("room-light-fit: {error}");
                ExitCode::FAILURE
            }
        };
    }
    eprintln!("room-light-fit: no install found has module {module}");
    ExitCode::FAILURE
}

/// The default folder: `target/room-lights/fit` under the current folder.
pub fn default_out() -> PathBuf {
    room_lights::default_out().join("fit")
}

fn fit_module(
    game: Game,
    index: &DirIndex,
    module: &str,
    every_glow: bool,
    out: &Path,
) -> Result<(), String> {
    let base = Resolver::base(game, index, TextureQuality::High).map_err(|e| e.to_string())?;
    let resolver = base
        .with_module(index, module)
        .map_err(|error| error.to_string())?;
    let built =
        ModuleIr::build(module, &ResolverSource(&resolver)).map_err(|error| format!("{error:?}"))?;
    let mut reader = Reader::new(&resolver, module);
    std::fs::create_dir_all(out).map_err(|error| format!("{}: {error}", out.display()))?;
    for area in &built.areas {
        let started = std::time::Instant::now();
        let fit = fit_area(&mut reader, &resolver, area, (game, module, every_glow), out)?;
        outln!(
            "{}: {} samples, {} candidates; best falloff {}, R² {:.3}, held out {:.3}, control {:.3} ({:.0} s)",
            area.resref,
            fit.samples,
            fit.candidates.len(),
            fit.best_falloff,
            fit.falloffs[fit.best].all.r2,
            fit.falloffs[fit.best].held_out_r2,
            fit.falloffs[fit.best].control_r2,
            started.elapsed().as_secs_f64()
        );
    }
    outln!("wrote into {}", out.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Textures.

/// A texture's colours as stored (0 to 1), at its first level no wider or
/// taller than `side`.
struct Rgb {
    width: usize,
    height: usize,
    texels: Vec<[f32; 3]>,
}

impl Rgb {
    /// The texel at `uv`, nearest, repeating.
    fn at(&self, uv: [f32; 2]) -> [f32; 3] {
        let x = ((uv[0] * self.width as f32).floor() as i64).rem_euclid(self.width as i64);
        let y = ((uv[1] * self.height as f32).floor() as i64).rem_euclid(self.height as i64);
        self.texels[y as usize * self.width + x as usize]
    }
}

fn read_rgb(resolver: &Resolver, name: &str, side: usize) -> Option<Rgb> {
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
    let texels: Vec<[f32; 3]> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .take(width * height)
        .map(|[r, g, b, _]| [*r, *g, *b].map(|v| f32::from(v) / 255.0))
        .collect();
    (texels.len() == width * height && width > 0 && height > 0).then_some(Rgb {
        width,
        height,
        texels,
    })
}

/// sRGB as stored to linear light, and back.
fn linear(v: f32) -> f32 {
    if v <= 0.040_45 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn encoded(v: f32) -> f32 {
    let v = v.max(0.0);
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

// ---------------------------------------------------------------------------
// Samples and candidates.

/// A lightmap texel on a room's surface.
struct Sample {
    /// Its place (area space), lifted off the surface.
    at: Vec3,
    normal: Vec3,
    /// Its lightmap value in linear light.
    target: [f32; 3],
    room: usize,
    map: usize,
    texel: usize,
    held_out: bool,
}

/// A lightmap of the area: its texels as stored.
struct Map {
    name: String,
    width: usize,
    height: usize,
    stored: Vec<[f32; 3]>,
    taken: Vec<bool>,
}

/// What a ray met, beside its corners in the `Level`.
struct SurfaceFacts {
    normal: Vec3,
    /// The mean colour of its texture in linear light.
    albedo: [f32; 3],
    lightmapped: bool,
}

#[derive(Clone, Copy)]
enum Shape {
    Point(Vec3),
    /// A panel: its middle, its two half sides and the way it faces.
    Rect {
        centre: Vec3,
        half: [Vec3; 2],
        normal: Vec3,
    },
    Sphere {
        centre: Vec3,
        radius: f32,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum Group {
    Lamp,
    Panel,
    Sun,
}

#[derive(Clone)]
struct Candidate {
    kind: String,
    name: String,
    room: Option<String>,
    shape: Shape,
    /// The far end of its light (metres), 0 for none.
    radius: f32,
    /// An ambient-only lamp: no facing and no ray.
    ambient: bool,
    group: Group,
    /// A direct, shadow-flagged type 2 lamp of a room.
    shadow_room_lamp: bool,
    /// A lamp with the shadow flag.
    shadow: bool,
    /// Whether a ray tests what blocks its light (`RAYS`).
    rays: bool,
    /// Its colour as the files give it.
    colour: [f32; 3],
}

impl Candidate {
    fn sun(&self) -> bool {
        self.group == Group::Sun
    }

    fn centre(&self) -> Vec3 {
        match self.shape {
            Shape::Point(at) => at,
            Shape::Rect { centre, .. } | Shape::Sphere { centre, .. } => centre,
        }
    }

    /// The points its light leaves from: each one's place, the way it faces
    /// (none for a point), its weight, and how near it a surface does not
    /// block it.
    fn emitters(&self) -> Vec<(Vec3, Option<Vec3>, f32, f32)> {
        match self.shape {
            Shape::Point(at) => vec![(at, None, 1.0, LAMP_SKIP)],
            Shape::Sphere { centre, radius } => {
                vec![(centre, None, 1.0, LAMP_SKIP.max(radius + 0.05))]
            }
            Shape::Rect {
                centre,
                half,
                normal,
            } => [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)]
                .into_iter()
                .map(|(a, b)| {
                    (
                        centre + half[0] * a + half[1] * b + normal * 0.03,
                        Some(normal),
                        0.25,
                        0.1,
                    )
                })
                .collect(),
        }
    }

    /// Its light on `sample` at strength 1 under each of the `FALLOFFS`, or
    /// `None` when none reaches it.
    fn light_on(
        &self,
        emitters: &[(Vec3, Option<Vec3>, f32, f32)],
        sample: &Sample,
        level: &Level,
    ) -> Option<[f32; 6]> {
        let mut light = [0.0f32; 6];
        let mut any = false;
        for &(place, facing, weight, skip) in emitters {
            let to = place - sample.at;
            let d = to.length();
            if d < 1e-3 || (!self.sun() && d > REACH) {
                continue;
            }
            let direction = to / d;
            let cos_r = if self.ambient {
                1.0
            } else {
                sample.normal.dot(direction)
            };
            if cos_r <= 0.0 {
                continue;
            }
            let cos_e = facing.map_or(1.0, |n| -direction.dot(n));
            if cos_e <= 0.0 {
                continue;
            }
            if !self.ambient && self.rays {
                let reach = d - RAY_START - skip;
                if reach > 0.0
                    && level
                        .first_hit(sample.at + direction * RAY_START, direction, reach)
                        .is_some()
                {
                    continue;
                }
            }
            let g = weight * cos_r * cos_e;
            let near = d.max(NEAR_END);
            let base = if self.sun() {
                [1.0; 3]
            } else {
                [1.0, 1.0 / near, 1.0 / (near * near)]
            };
            let window = if self.sun() || self.radius <= 0.0 {
                1.0
            } else {
                let q = d / self.radius;
                if q >= 1.0 {
                    0.0
                } else {
                    (1.0 - q * q) * (1.0 - q * q)
                }
            };
            for k in 0..3 {
                light[k] += g * base[k];
                light[k + 3] += g * base[k] * window;
            }
            any = true;
        }
        any.then_some(light)
    }
}

/// For each candidate, of the samples within `SEEN_NEAR` of its middle
/// that face it, the share its rays reach (by its first emitter).
fn seen(candidates: &[Candidate], samples: &[Sample], level: &Level) -> Vec<Option<f64>> {
    parallel(candidates.len(), |j| {
        let candidate = &candidates[j];
        if candidate.sun() || candidate.ambient {
            return None;
        }
        let (place, facing, _, skip) = candidate.emitters()[0];
        let (mut facing_count, mut reached) = (0usize, 0usize);
        for sample in samples {
            let to = place - sample.at;
            let d = to.length();
            if !(1e-3..=SEEN_NEAR).contains(&d) {
                continue;
            }
            let direction = to / d;
            if sample.normal.dot(direction) <= 0.0
                || facing.is_some_and(|n| -direction.dot(n) <= 0.0)
            {
                continue;
            }
            facing_count += 1;
            let reach = d - RAY_START - skip;
            if reach <= 0.0
                || level
                    .first_hit(sample.at + direction * RAY_START, direction, reach)
                    .is_none()
            {
                reached += 1;
            }
        }
        (facing_count > 0).then(|| reached as f64 / facing_count as f64)
    })
}

fn lamp_kind(owner: &str, dynamic_type: u32, ambient: bool, shadow: bool) -> String {
    let how = if ambient {
        "ambient only"
    } else if shadow {
        "shadow"
    } else {
        "no shadow"
    };
    format!("{owner} lamp, type {dynamic_type}, {how}")
}

/// The eigenvalues and eigenvectors of a symmetric 3 by 3 matrix, largest
/// first (Jacobi rotations).
fn eigen(mut a: [[f32; 3]; 3]) -> [(f32, Vec3); 3] {
    let mut v = [[1.0f32, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-12 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for row in &mut a {
                let (akp, akq) = (row[p], row[q]);
                row[p] = c * akp - s * akq;
                row[q] = s * akp + c * akq;
            }
            let (row_p, row_q) = (a[p], a[q]);
            a[p] = std::array::from_fn(|k| c * row_p[k] - s * row_q[k]);
            a[q] = std::array::from_fn(|k| s * row_p[k] + c * row_q[k]);
            for row in &mut v {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    let mut out: [(f32, Vec3); 3] =
        std::array::from_fn(|k| (a[k][k], Vec3::new(v[0][k], v[1][k], v[2][k])));
    out.sort_by(|x, y| y.0.total_cmp(&x.0));
    out
}

// ---------------------------------------------------------------------------
// The design: each candidate's light on each sample.

/// Row `s` holds the candidates whose light reaches sample `s`, by column
/// (the candidate's index plus 1, as column 0 is the flat term), under each
/// falloff.
struct Design {
    offsets: Vec<usize>,
    entries: Vec<(u32, [f32; 6])>,
}

impl Design {
    fn row(&self, sample: usize) -> &[(u32, [f32; 6])] {
        &self.entries[self.offsets[sample]..self.offsets[sample + 1]]
    }
}

fn threads() -> usize {
    std::thread::available_parallelism()
        .map_or(4, usize::from)
        .max(1)
}

/// Runs `work` on blocks of `count` items across the machine's threads and
/// returns each item's result in order.
fn parallel<T: Send>(count: usize, work: impl Fn(usize) -> T + Sync) -> Vec<T> {
    const BLOCK: usize = 256;
    let taken = AtomicUsize::new(0);
    let done: Mutex<Vec<(usize, Vec<T>)>> = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..threads() {
            scope.spawn(|| {
                loop {
                    let first = taken.fetch_add(BLOCK, Ordering::Relaxed);
                    if first >= count {
                        return;
                    }
                    let block: Vec<T> = (first..(first + BLOCK).min(count)).map(&work).collect();
                    done.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .push((first, block));
                }
            });
        }
    });
    let mut done = done
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    done.sort_by_key(|(first, _)| *first);
    done.into_iter().flat_map(|(_, block)| block).collect()
}

fn design(samples: &[Sample], candidates: &[Candidate], level: &Level) -> Design {
    let emitters: Vec<_> = candidates.iter().map(Candidate::emitters).collect();
    let rows = parallel(samples.len(), |s| {
        let sample = &samples[s];
        let mut row = Vec::new();
        for (index, candidate) in candidates.iter().enumerate() {
            if !candidate.sun() && (candidate.centre() - sample.at).length() > REACH + 2.0 {
                continue;
            }
            if let Some(light) = candidate.light_on(&emitters[index], sample, level) {
                row.push((index as u32 + 1, light));
            }
        }
        row
    });
    let mut offsets = Vec::with_capacity(rows.len() + 1);
    let mut entries = Vec::new();
    offsets.push(0);
    for row in rows {
        entries.extend(row);
        offsets.push(entries.len());
    }
    Design { offsets, entries }
}

// ---------------------------------------------------------------------------
// The solve.

/// One sample's row for one solve: (column, light per channel), without
/// the flat term. A column may repeat, and its values then add up.
type Row = Vec<(u32, [f32; 3])>;

/// The normal equations of one solve: the Gram matrix of the columns and
/// each channel's right-hand side, over the samples `take` allows.
struct Normal {
    n: usize,
    gram: [Vec<f64>; 3],
    rhs: [Vec<f64>; 3],
    /// The sum of the targets' squares, per channel.
    yy: [f64; 3],
}

fn normal_eq(
    samples: &[Sample],
    n: usize,
    take: &dyn Fn(usize) -> bool,
    fill: &(dyn Fn(usize, &mut Row) + Sync),
    shared: bool,
) -> Normal {
    let channels = if shared { 1 } else { 3 };
    let mut gram: [Vec<f64>; 3] = std::array::from_fn(|c| {
        if c < channels {
            vec![0.0; n * n]
        } else {
            Vec::new()
        }
    });
    let mut rhs: [Vec<f64>; 3] = std::array::from_fn(|_| vec![0.0; n]);
    let mut yy = [0.0f64; 3];
    let mut row: Row = Vec::new();
    for (s, sample) in samples.iter().enumerate() {
        if !take(s) {
            continue;
        }
        row.clear();
        row.push((0, [1.0; 3]));
        fill(s, &mut row);
        for c in 0..3 {
            let y = f64::from(sample.target[c]);
            yy[c] += y * y;
            for &(j, v) in &row {
                rhs[c][j as usize] += f64::from(v[c]) * y;
            }
        }
        for (c, gram) in gram.iter_mut().enumerate().take(channels) {
            for &(i, vi) in &row {
                let vi = f64::from(vi[c]);
                let line = &mut gram[i as usize * n..(i as usize + 1) * n];
                for &(j, vj) in &row {
                    line[j as usize] += vi * f64::from(vj[c]);
                }
            }
        }
    }
    if shared {
        gram[1] = gram[0].clone();
        gram[2] = gram[0].clone();
    }
    Normal { n, gram, rhs, yy }
}

/// Non-negative least squares on the normal equations by cyclic coordinate
/// descent, over the columns `cols` alone. The others stay 0.
fn nnls(gram: &[f64], n: usize, rhs: &[f64], cols: &[usize], yy: f64) -> Vec<f64> {
    let mut x = vec![0.0f64; cols.len()];
    let mut grad: Vec<f64> = cols.iter().map(|&j| -rhs[j]).collect();
    let tolerance = 1e-13 * yy.max(1e-30);
    for _ in 0..20_000 {
        let mut biggest = 0.0f64;
        for i in 0..cols.len() {
            let j = cols[i];
            let gjj = gram[j * n + j];
            if gjj <= 0.0 {
                continue;
            }
            let new = (x[i] - grad[i] / gjj).max(0.0);
            let delta = new - x[i];
            if delta != 0.0 {
                x[i] = new;
                let column = &gram[j * n..(j + 1) * n];
                for (k, &jk) in cols.iter().enumerate() {
                    grad[k] += delta * column[jk];
                }
                biggest = biggest.max(delta * delta * gjj);
            }
        }
        if biggest < tolerance {
            break;
        }
    }
    let mut full = vec![0.0; n];
    for (i, &j) in cols.iter().enumerate() {
        full[j] = x[i];
    }
    full
}

/// The strengths of every column, per channel.
type Strengths = [Vec<f64>; 3];

fn solve(normal: &Normal, cols: &[usize]) -> Strengths {
    std::array::from_fn(|c| {
        nnls(
            &normal.gram[c],
            normal.n,
            &normal.rhs[c],
            cols,
            normal.yy[c],
        )
    })
}

/// How well a fit matches the lightmap over some samples, in linear light
/// with the three channels together.
#[derive(Serialize, Clone, Copy, Default)]
struct Score {
    samples: usize,
    /// 1 minus the squared error over the squared spread around each
    /// channel's mean. The flat term alone scores 0.
    r2: f64,
    /// The root of the squared error over the root of the squared target.
    error_left: f64,
    /// The placed candidates' light over the lightmap's light.
    placed_share: f64,
    /// The flat term's light over the lightmap's light.
    flat_share: f64,
    /// The part of `placed_share` that arrives by a bounce.
    bounce_share: f64,
}

/// A fit scored over some samples, with each sample's fitted light and each
/// column's light summed over the samples and channels.
struct Scored {
    score: Score,
    predicted: Vec<[f32; 3]>,
    contribution: Vec<f64>,
}

/// Scores the strengths `x` over the samples `take` allows.
fn score(
    samples: &[Sample],
    x: &Strengths,
    take: &dyn Fn(usize) -> bool,
    fill: &dyn Fn(usize, &mut Row),
) -> Scored {
    let n = x[0].len();
    let mut predicted = vec![[0.0f32; 3]; samples.len()];
    let mut contribution = vec![0.0f64; n];
    let (mut sum, mut count) = ([0.0f64; 3], 0usize);
    for (s, sample) in samples.iter().enumerate() {
        if take(s) {
            count += 1;
            for (sum, target) in sum.iter_mut().zip(sample.target) {
                *sum += f64::from(target);
            }
        }
    }
    let mean = sum.map(|v| v / count.max(1) as f64);
    let (mut sse, mut sst, mut yy, mut ysum) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    let (mut placed, mut flat) = (0.0f64, 0.0f64);
    let mut row: Row = Vec::new();
    for (s, sample) in samples.iter().enumerate() {
        if !take(s) {
            continue;
        }
        row.clear();
        fill(s, &mut row);
        for c in 0..3 {
            let mut p = x[c][0];
            flat += x[c][0];
            for &(j, v) in &row {
                let light = x[c][j as usize] * f64::from(v[c]);
                p += light;
                placed += light;
                contribution[j as usize] += light;
            }
            contribution[0] += x[c][0];
            let y = f64::from(sample.target[c]);
            predicted[s][c] = p as f32;
            sse += (y - p) * (y - p);
            sst += (y - mean[c]) * (y - mean[c]);
            yy += y * y;
            ysum += y;
        }
    }
    let ysum = ysum.max(1e-30);
    Scored {
        score: Score {
            samples: count,
            r2: if sst > 0.0 { 1.0 - sse / sst } else { 0.0 },
            error_left: (sse / yy.max(1e-30)).sqrt(),
            placed_share: placed / ysum,
            flat_share: flat / ysum,
            bounce_share: 0.0,
        },
        predicted,
        contribution,
    }
}

/// A sample's row under falloff `k` of `design`.
fn fill_of(design: &Design, k: usize) -> impl Fn(usize, &mut Row) + Sync + '_ {
    move |s, row| row.extend(design.row(s).iter().map(|&(j, v)| (j, [v[k]; 3])))
}

/// A sample's row under falloff `k` of `design`, with its bounce `rows`.
fn with_bounce<'d>(
    design: &'d Design,
    k: usize,
    rows: &'d [Row],
) -> impl Fn(usize, &mut Row) + Sync + 'd {
    move |s, row| {
        row.extend(design.row(s).iter().map(|&(j, v)| (j, [v[k]; 3])));
        row.extend_from_slice(&rows[s]);
    }
}

// ---------------------------------------------------------------------------
// What the verb writes.

#[derive(Serialize)]
struct FalloffFit {
    /// Which candidates' light a ray tests (`RAYS`).
    rays: &'static str,
    falloff: &'static str,
    /// Fitted on every sample and scored on every sample.
    all: Score,
    /// Fitted on three faces in four, scored on the fourth.
    held_out_r2: f64,
    /// The mean of the control fits, each scored on every sample.
    control_r2: f64,
    control_placed_share: f64,
}

#[derive(Serialize)]
struct CandidateFit {
    kind: String,
    name: String,
    room: Option<String>,
    shape: &'static str,
    at: [f32; 3],
    /// For a panel, its size (metres) and the way it faces.
    size: Option<[f32; 2]>,
    facing: Option<[f32; 3]>,
    radius: f32,
    colour: [f32; 3],
    /// The fitted strength per channel under the best falloff, direct.
    strength: [f64; 3],
    /// Its light over the lightmap's light, under the best falloff.
    share: f64,
    /// Of the samples within `SEEN_NEAR` that face it, the share its rays
    /// reach. `None` for the sun and ambient-only lamps.
    seen: Option<f64>,
}

#[derive(Serialize)]
struct KindFit {
    kind: String,
    candidates: usize,
    with_strength: usize,
    share: f64,
    /// The median of its candidates' `CandidateFit::seen`.
    seen_median: Option<f64>,
}

#[derive(Serialize)]
struct RoomFit {
    room: String,
    samples: usize,
    /// The mean of its lightmap texels as stored (gamma encoded, 0 to 1).
    stored_mean: f32,
    placed_share: f64,
    flat_share: f64,
    error_left: f64,
    r2: f64,
}

#[derive(Serialize)]
struct AreaFit {
    game: &'static str,
    module: String,
    area: String,
    samples: usize,
    held_out_samples: usize,
    rooms_sampled: usize,
    lightmaps: usize,
    lamps: usize,
    glowing_meshes: usize,
    /// Every glowing mesh was taken as a panel (`--every-glow`).
    every_glow: bool,
    panels: usize,
    suns: usize,
    falloffs: Vec<FalloffFit>,
    best: usize,
    best_falloff: &'static str,
    best_rays: &'static str,
    /// Fits on parts of the candidates under the best falloff: (which, R²).
    subsets: Vec<(String, f64)>,
    /// The best falloff with one bounce: fitted and scored on every sample,
    /// and the held-out score.
    bounce: Score,
    bounce_held_out_r2: f64,
    bounce_control_r2: f64,
    kinds: Vec<KindFit>,
    /// The fit by the way the samples face: (facing, samples, lightmap
    /// mean, fit mean), linear.
    facings: Vec<(&'static str, usize, f64, f64)>,
    /// R² of each sample predicted by the mean of the other samples within
    /// `NEIGHBOURS` that face the same way. No light that changes smoothly
    /// over a metre can do much better.
    neighbours_r2: f64,
    rooms: Vec<RoomFit>,
    candidates: Vec<CandidateFit>,
    /// Each lightmap's place in the lightmaps' picture: name, x, y, width
    /// and height of its target panel.
    sheet: Vec<SheetPlace>,
}

// ---------------------------------------------------------------------------
// One area.

fn hash(values: &[u64]) -> u64 {
    let mut h = 0x9e37_79b9_7f4a_7c15u64;
    for &v in values {
        h ^= v.wrapping_add(0x9e37_79b9_7f4a_7c15)
            .wrapping_add(h << 6)
            .wrapping_add(h >> 2);
        h = h.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        h ^= h >> 31;
    }
    h
}

/// A number from 0 to 1 for `values`.
fn unit(values: &[u64]) -> f32 {
    (hash(values) >> 40) as f32 / (1u64 << 24) as f32
}

#[allow(clippy::too_many_lines)]
fn fit_area(
    reader: &mut Reader,
    resolver: &Resolver,
    area: &AreaIr,
    (game, module, every_glow): (Game, &str, bool),
    out: &Path,
) -> Result<AreaFit, String> {
    let placement = room_lights::place(reader, area);
    let Placement {
        objects,
        walkable,
        walk_boxes,
        ..
    } = &placement;

    let mut textures: HashMap<String, Option<Rc<Rgb>>> = HashMap::new();
    let mut texture = |name: &str, side: usize| -> Option<Rc<Rgb>> {
        textures
            .entry(format!("{name}@{side}"))
            .or_insert_with(|| read_rgb(resolver, name, side).map(Rc::new))
            .clone()
    };

    // The room surfaces the rays meet, and the samples.
    let mut maps: Vec<Map> = Vec::new();
    let mut map_of: BTreeMap<String, Option<usize>> = BTreeMap::new();
    let mut surfaces: Vec<Surface> = Vec::new();
    let mut facts: Vec<SurfaceFacts> = Vec::new();
    let mut samples: Vec<Sample> = Vec::new();
    for (index, object) in objects.iter().enumerate() {
        let Owner::Room(room) = object.owner else {
            continue;
        };
        if !walkable[room] {
            continue;
        }
        let ir = &object.model.ir;
        for (mesh_index, mesh) in ir.meshes.iter().enumerate() {
            if mesh.skin.is_some() {
                continue;
            }
            let node = multiply(&object.frame, &ir.nodes[mesh.node].world);
            let world: Vec<Vec3> = (mesh.positions.iter())
                .map(|p| Vec3::from_array(room_lights::apply(&node, *p)))
                .collect();
            let map = match mesh.lightmap.as_deref() {
                Some(name) if mesh.uv2.len() == mesh.positions.len() => {
                    *map_of.entry(name.to_string()).or_insert_with(|| {
                        let rgb = read_rgb(resolver, name, usize::MAX)?;
                        maps.push(Map {
                            name: name.to_string(),
                            width: rgb.width,
                            height: rgb.height,
                            taken: vec![false; rgb.texels.len()],
                            stored: rgb.texels,
                        });
                        Some(maps.len() - 1)
                    })
                }
                _ => None,
            };
            let opaque = mesh.transparency_hint == 0
                && mesh.alpha >= 0.99
                && (mesh.texture.is_some() || mesh.lightmap.is_some());
            let albedo = mesh
                .texture
                .as_deref()
                .and_then(|name| texture(name, 16))
                .map_or([0.5; 3], |rgb| {
                    let mut sum = [0.0f32; 3];
                    for t in &rgb.texels {
                        for c in 0..3 {
                            sum[c] += linear(t[c]);
                        }
                    }
                    sum.map(|v| v / rgb.texels.len() as f32)
                });
            for (t, &[a, b, c]) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
                let corner = |i: u32| world.get(i as usize).copied();
                let (Some(pa), Some(pb), Some(pc)) = (corner(a), corner(b), corner(c)) else {
                    continue;
                };
                let front = (pb - pa).cross(pc - pa);
                if front.length() < 1e-8 {
                    continue;
                }
                let normal = front.normalize();
                if opaque {
                    surfaces.push(Surface {
                        corners: [pa, pb, pc],
                        uv: [Vec2::ZERO; 3],
                        lightmap: None,
                        texture: None,
                        texture_uv: [Vec2::ZERO; 3],
                        glow: Vec3::ZERO,
                    });
                    facts.push(SurfaceFacts {
                        normal,
                        albedo,
                        lightmapped: map.is_some(),
                    });
                }
                let Some(map) = map else {
                    continue;
                };
                let m = &mut maps[map];
                let (w, h) = (m.width, m.height);
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
                let middle = (pa + pb + pc) / 3.0;
                let corners = [pa, pb, pc];
                for y in (low(1) - 0.5).ceil() as i64..=(high(1) - 0.5).floor() as i64 {
                    for x in (low(0) - 0.5).ceil() as i64..=(high(0) - 0.5).floor() as i64 {
                        let centre = [x as f32 + 0.5, y as f32 + 0.5];
                        if !room_lights::inside(&uv, sign, centre, -0.01) {
                            continue;
                        }
                        let texel = (y.rem_euclid(h as i64) as usize) * w
                            + x.rem_euclid(w as i64) as usize;
                        if m.taken[texel] {
                            continue;
                        }
                        m.taken[texel] = true;
                        let weights: [f32; 3] = std::array::from_fn(|k| {
                            sign * room_lights::edge(uv[(k + 1) % 3], uv[(k + 2) % 3], centre)
                        });
                        let total: f32 = weights.iter().sum();
                        let place = (0..3).map(|k| corners[k] * weights[k]).sum::<Vec3>() / total;
                        let inward = middle - place;
                        let place = place
                            + inward * (INWARD / inward.length().max(INWARD))
                            + normal * LIFT;
                        samples.push(Sample {
                            at: place,
                            normal,
                            target: m.stored[texel].map(linear),
                            room,
                            map,
                            texel,
                            held_out: hash(&[index as u64, mesh_index as u64, t as u64 / 2])
                                .is_multiple_of(HELD_OUT),
                        });
                    }
                }
            }
        }
    }
    if samples.is_empty() {
        return Err(format!("{}: no lightmap texel on a room", area.resref));
    }
    let (least, most) = samples.iter().fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(low, high), s| (low.min(s.at), high.max(s.at)),
    );
    let margin = Vec3::splat(REACH + 2.0);
    let level = Level::new(&surfaces, (least - margin, most + margin));

    // The candidates: the lamps.
    let room_name = |room: usize| area.rooms[room].model.clone();
    let mut candidates: Vec<Candidate> = Vec::new();
    let mut lamps = 0usize;
    let mut suns = 0usize;
    for object in objects {
        let ir = &object.model.ir;
        for light in &ir.lights {
            let world = multiply(&object.frame, &ir.nodes[light.node].world);
            let at = Vec3::new(world[0][3], world[1][3], world[2][3]);
            let room = match object.owner {
                Owner::Room(room) => Some(room),
                _ => room_lights::room_at(walk_boxes, at.to_array()),
            };
            let sun = light.radius >= SUN_RADIUS;
            if sun {
                suns += 1;
            } else {
                lamps += 1;
            }
            candidates.push(Candidate {
                kind: if sun {
                    "sun".to_string()
                } else {
                    lamp_kind(
                        object.owner.name(),
                        light.dynamic_type,
                        light.ambient_only,
                        light.shadow,
                    )
                },
                name: format!("{} {}", ir.name, ir.nodes[light.node].name),
                room: room.map(room_name),
                shape: Shape::Point(at),
                radius: light.radius,
                ambient: light.ambient_only,
                group: if sun { Group::Sun } else { Group::Lamp },
                shadow_room_lamp: matches!(object.owner, Owner::Room(_))
                    && light.dynamic_type == 2
                    && light.shadow
                    && !light.ambient_only,
                shadow: light.shadow,
                rays: true,
                colour: light.color.map(|v| v * light.multiplier),
            });
        }
    }

    // The glowing surfaces that look like lights, by the survey's rule.
    let mut glowing = 0usize;
    for object in objects {
        if matches!(object.owner, Owner::Creature | Owner::Door) {
            continue;
        }
        let ir = &object.model.ir;
        for mesh in &ir.meshes {
            if !mesh.self_illum.iter().any(|&v| v > 0.0) || mesh.skin.is_some() {
                continue;
            }
            glowing += 1;
            let node = multiply(&object.frame, &ir.nodes[mesh.node].world);
            let world: Vec<Vec3> = (mesh.positions.iter())
                .map(|p| Vec3::from_array(room_lights::apply(&node, *p)))
                .collect();
            let triangles: Vec<[usize; 3]> = (mesh.indices.as_chunks::<3>().0.iter())
                .map(|t| t.map(|i| i as usize))
                .filter(|t| t.iter().all(|&i| i < world.len()))
                .collect();
            let size = |t: &[usize; 3]| {
                0.5 * (world[t[1]] - world[t[0]])
                    .cross(world[t[2]] - world[t[0]])
                    .length()
            };
            let area_m2: f32 = triangles.iter().map(size).sum();
            let centre = if area_m2 > 0.0 {
                triangles
                    .iter()
                    .map(|t| (world[t[0]] + world[t[1]] + world[t[2]]) / 3.0 * size(t))
                    .sum::<Vec3>()
                    / area_m2
            } else {
                Vec3::new(node[0][3], node[1][3], node[2][3])
            };
            let room = match object.owner {
                Owner::Room(room) => Some(room),
                _ => room_lights::room_at(walk_boxes, centre.to_array()),
            };
            let lightmapped = mesh.lightmap.as_deref().is_some_and(|name| {
                mesh.uv2.len() == mesh.positions.len() && reader.lightmap(name).is_some()
            });
            let bright_share = mesh
                .texture
                .as_deref()
                .and_then(|name| reader.bright_share(name));
            let backdrop = room.is_some_and(|room| !walkable[room]) || area_m2 > room_lights::SKY_M2;
            let lamp_like = !backdrop
                && (every_glow
                    || lightmapped || bright_share.is_some_and(|s| s >= room_lights::BRIGHT_SHARE));
            if !lamp_like || area_m2 <= 0.0 {
                continue;
            }
            // The bright points of its surface: 16 points in each triangle,
            // each weighed by its share of the triangle's area.
            let rgb = (mesh.uv1.len() == mesh.positions.len())
                .then(|| mesh.texture.as_deref().and_then(|name| texture(name, 64)))
                .flatten();
            let mut points: Vec<(Vec3, Vec3, f32, [f32; 3])> = Vec::new();
            let mut all: Vec<(Vec3, Vec3, f32, [f32; 3])> = Vec::new();
            for t in &triangles {
                let a = size(t);
                if a <= 0.0 {
                    continue;
                }
                let normal = (world[t[1]] - world[t[0]])
                    .cross(world[t[2]] - world[t[0]])
                    .normalize();
                for i in 0..4 {
                    for j in 0..(4 - i) {
                        for flip in [false, true] {
                            if flip && i + j >= 3 {
                                continue;
                            }
                            let (u, v) = if flip {
                                ((i as f32 + 2.0 / 3.0) / 4.0, (j as f32 + 2.0 / 3.0) / 4.0)
                            } else {
                                ((i as f32 + 1.0 / 3.0) / 4.0, (j as f32 + 1.0 / 3.0) / 4.0)
                            };
                            let weights = [1.0 - u - v, u, v];
                            let at = (0..3).map(|k| world[t[k]] * weights[k]).sum::<Vec3>();
                            let colour = rgb.as_ref().map_or([1.0; 3], |rgb| {
                                let uv: [f32; 2] = std::array::from_fn(|axis| {
                                    (0..3).map(|k| mesh.uv1[t[k]][axis] * weights[k]).sum()
                                });
                                rgb.at(uv)
                            });
                            let point = (at, normal, a / 16.0, colour);
                            all.push(point);
                            if luma(colour) > BRIGHT {
                                points.push(point);
                            }
                        }
                    }
                }
            }
            if points.is_empty() {
                points = all;
            }
            let weight: f32 = points.iter().map(|p| p.2).sum();
            if weight <= 0.0 {
                continue;
            }
            let mean = points.iter().map(|p| p.0 * p.2).sum::<Vec3>() / weight;
            let facing = points.iter().map(|p| p.1 * p.2).sum::<Vec3>() / weight;
            let mut cov = [[0.0f32; 3]; 3];
            for p in &points {
                let d = p.0 - mean;
                for r in 0..3 {
                    for c in 0..3 {
                        cov[r][c] += p.2 * d[r] * d[c] / weight;
                    }
                }
            }
            let axes = eigen(cov);
            let sigma = axes.map(|(value, _)| value.max(0.0).sqrt());
            let mut colour = [0.0f32; 3];
            for p in &points {
                for (colour, stored) in colour.iter_mut().zip(p.3) {
                    *colour += p.2 * linear(stored) / weight;
                }
            }
            let colour: [f32; 3] = std::array::from_fn(|c| colour[c] * mesh.self_illum[c]);
            let flat = facing.length() > 0.5 && sigma[2] < 0.3 * sigma[0].max(1e-4);
            let shape = if flat {
                let normal = if axes[2].1.dot(facing) >= 0.0 {
                    axes[2].1
                } else {
                    -axes[2].1
                };
                let half = 3f32.sqrt();
                Shape::Rect {
                    centre: mean,
                    half: [
                        axes[0].1 * (half * sigma[0]).max(0.05),
                        axes[1].1 * (half * sigma[1]).max(0.05),
                    ],
                    normal,
                }
            } else {
                Shape::Sphere {
                    centre: mean,
                    radius: (sigma[0] * sigma[0] + sigma[1] * sigma[1] + sigma[2] * sigma[2])
                        .sqrt(),
                }
            };
            candidates.push(Candidate {
                kind: format!("panel of a {}", object.owner.name()),
                name: format!(
                    "{} {} ({})",
                    ir.name,
                    ir.nodes[mesh.node].name,
                    mesh.texture.as_deref().unwrap_or("no texture")
                ),
                room: room.map(room_name),
                shape,
                radius: PANEL_RADIUS,
                ambient: false,
                group: Group::Panel,
                shadow_room_lamp: false,
                shadow: false,
                rays: true,
                colour,
            });
        }
    }
    let panels = candidates.iter().filter(|c| c.group == Group::Panel).count();
    outln!(
        "{}: {} samples on {} lightmaps, {} surfaces; {} lamps, {} panels of {} glowing meshes, {} suns",
        area.resref,
        samples.len(),
        maps.len(),
        surfaces.len(),
        lamps,
        panels,
        glowing,
        suns
    );

    // Direct light under each falloff.
    let n = candidates.len() + 1;
    let all_cols: Vec<usize> = (0..n).collect();
    let every = |_: usize| true;
    let train = |s: usize| !samples[s].held_out;
    let test = |s: usize| samples[s].held_out;
    let mut falloffs: Vec<FalloffFit> = Vec::new();
    // The best mode's design, its first control and its fits, kept.
    let mut kept: Option<(usize, Design, Design, Vec<Strengths>)> = None;
    let mut best = 0usize;
    for (mode, &rays) in RAYS.iter().enumerate() {
        for candidate in &mut candidates {
            candidate.rays = rays_for(mode, candidate);
        }
        let main = design(&samples, &candidates, &level);
        let mut controls: Vec<Design> = Vec::new();
        for seed in 0..CONTROLS {
            let moved = scatter(&candidates, &samples, &level, seed);
            controls.push(design(&samples, &moved, &level));
        }
        let mut fits: Vec<Strengths> = Vec::new();
        for (k, &falloff) in FALLOFFS.iter().enumerate() {
            let fill = fill_of(&main, k);
            let normal = normal_eq(&samples, n, &every, &fill, true);
            let x = solve(&normal, &all_cols);
            let all = score(&samples, &x, &every, &fill).score;
            let held = normal_eq(&samples, n, &train, &fill, true);
            let held_x = solve(&held, &all_cols);
            let held_out_r2 = score(&samples, &held_x, &test, &fill).score.r2;
            let (mut control_r2, mut control_placed) = (0.0, 0.0);
            for control in &controls {
                let fill = fill_of(control, k);
                let normal = normal_eq(&samples, n, &every, &fill, true);
                let x = solve(&normal, &all_cols);
                let scored = score(&samples, &x, &every, &fill).score;
                control_r2 += scored.r2 / CONTROLS as f64;
                control_placed += scored.placed_share / CONTROLS as f64;
            }
            falloffs.push(FalloffFit {
                rays,
                falloff,
                all,
                held_out_r2,
                control_r2,
                control_placed_share: control_placed,
            });
            fits.push(x);
        }
        let mode_best = (mode * FALLOFFS.len()..falloffs.len())
            .max_by(|a, b| falloffs[*a].held_out_r2.total_cmp(&falloffs[*b].held_out_r2))
            .unwrap_or(0);
        if kept.is_none() || falloffs[mode_best].held_out_r2 > falloffs[best].held_out_r2 {
            best = mode_best;
            let first = controls.swap_remove(0);
            kept = Some((mode, main, first, fits));
        }
    }
    let Some((best_mode, main, first_control, fits)) = kept else {
        return Err("no fit".to_string());
    };
    for candidate in &mut candidates {
        candidate.rays = rays_for(best_mode, candidate);
    }
    let best_k = best % FALLOFFS.len();
    let fill = fill_of(&main, best_k);
    let x = &fits[best_k];
    let scored = score(&samples, x, &every, &fill);

    // Parts of the candidates.
    let normal = normal_eq(&samples, n, &every, &fill, true);
    let pick = |keep: &dyn Fn(&Candidate) -> bool| -> Vec<usize> {
        std::iter::once(0)
            .chain(
                (candidates.iter().enumerate())
                    .filter(|(_, c)| keep(c))
                    .map(|(j, _)| j + 1),
            )
            .collect()
    };
    let mut subsets = Vec::new();
    for (label, cols) in [
        ("the flat term alone", pick(&|_| false)),
        ("the flat term and every lamp", pick(&|c| c.group != Group::Panel)),
        (
            "the flat term and the shadow-flagged type 2 room lamps",
            pick(&|c| c.shadow_room_lamp),
        ),
        ("the flat term and the panels", pick(&|c| c.group == Group::Panel)),
        ("every candidate", all_cols.clone()),
        (
            "every candidate without the flat term",
            pick(&|_| true).into_iter().skip(1).collect(),
        ),
    ] {
        let x = solve(&normal, &cols);
        subsets.push((
            label.to_string(),
            score(&samples, &x, &every, &fill).score.r2,
        ));
    }
    // A flat term for each room, in columns after the candidates' (the
    // area's flat term stays in, and the solve shares the light among
    // them).
    {
        let rooms = area.rooms.len();
        let wide = n + rooms;
        let room_fill = |s: usize, row: &mut Row| {
            fill(s, row);
            row.push(((n + samples[s].room) as u32, [1.0; 3]));
        };
        let normal = normal_eq(&samples, wide, &every, &room_fill, true);
        let room_cols: Vec<usize> = std::iter::once(0).chain(n..wide).collect();
        let all: Vec<usize> = (0..wide).collect();
        for (label, cols) in [
            ("a flat term for each room alone", room_cols),
            ("a flat term for each room and every candidate", all),
        ] {
            let x = solve(&normal, &cols);
            subsets.push((
                label.to_string(),
                score(&samples, &x, &every, &room_fill).score.r2,
            ));
        }
    }

    // One bounce under the best falloff.
    let bounce_rows = bounce(&samples, &facts, &level, &main, best_k, n);
    let bounce_fill = with_bounce(&main, best_k, &bounce_rows);
    let bounce_normal = normal_eq(&samples, n, &every, &bounce_fill, false);
    let bounce_x = solve(&bounce_normal, &all_cols);
    let bounced = {
        // The bounce's own share, from the bounce rows alone.
        let mut total = score(&samples, &bounce_x, &every, &bounce_fill).score;
        let mut share = 0.0;
        let mut ysum = 0.0;
        for (s, sample) in samples.iter().enumerate() {
            for c in 0..3 {
                ysum += f64::from(sample.target[c]);
                for &(j, v) in &bounce_rows[s] {
                    share += bounce_x[c][j as usize] * f64::from(v[c]);
                }
            }
        }
        total.bounce_share = share / ysum.max(1e-30);
        total
    };
    let bounce_held = normal_eq(&samples, n, &train, &bounce_fill, false);
    let bounce_held_x = solve(&bounce_held, &all_cols);
    let bounce_held_out_r2 = score(&samples, &bounce_held_x, &test, &bounce_fill)
        .score
        .r2;
    // One control with a bounce, from the first control's places.
    let bounce_control_r2 = {
        let control_bounce = bounce(&samples, &facts, &level, &first_control, best_k, n);
        let fill = with_bounce(&first_control, best_k, &control_bounce);
        let normal = normal_eq(&samples, n, &every, &fill, false);
        let x = solve(&normal, &all_cols);
        score(&samples, &x, &every, &fill).score.r2
    };

    // The kinds and the candidates under the best falloff.
    let ysum: f64 = samples
        .iter()
        .map(|s| s.target.iter().map(|&v| f64::from(v)).sum::<f64>())
        .sum::<f64>()
        .max(1e-30);
    let mut kinds: BTreeMap<String, KindFit> = BTreeMap::new();
    let mut candidate_fits = Vec::new();
    let seen = seen(&candidates, &samples, &level);
    let mut seen_by_kind: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for (j, candidate) in candidates.iter().enumerate() {
        let strength = [x[0][j + 1], x[1][j + 1], x[2][j + 1]];
        let share = scored.contribution[j + 1] / ysum;
        let kind = kinds
            .entry(candidate.kind.clone())
            .or_insert_with(|| KindFit {
                kind: candidate.kind.clone(),
                candidates: 0,
                with_strength: 0,
                share: 0.0,
                seen_median: None,
            });
        if let Some(seen) = seen[j] {
            seen_by_kind
                .entry(candidate.kind.clone())
                .or_default()
                .push(seen);
        }
        kind.candidates += 1;
        kind.with_strength += usize::from(strength.iter().any(|&v| v > 0.0));
        kind.share += share;
        let (shape, size, facing) = match candidate.shape {
            Shape::Point(_) => ("point", None, None),
            Shape::Sphere { radius, .. } => ("sphere", Some([radius * 2.0; 2]), None),
            Shape::Rect { half, normal, .. } => (
                "rectangle",
                Some([half[0].length() * 2.0, half[1].length() * 2.0]),
                Some(normal.to_array()),
            ),
        };
        candidate_fits.push(CandidateFit {
            kind: candidate.kind.clone(),
            name: candidate.name.clone(),
            room: candidate.room.clone(),
            shape,
            at: candidate.centre().to_array(),
            size,
            facing,
            radius: candidate.radius,
            colour: candidate.colour,
            strength,
            share,
            seen: seen[j],
        });
    }
    for (kind, mut values) in seen_by_kind {
        values.sort_by(f64::total_cmp);
        if let Some(fit) = kinds.get_mut(&kind) {
            fit.seen_median = values.get(values.len() / 2).copied();
        }
    }
    kinds.insert(
        "flat term".to_string(),
        KindFit {
            kind: "flat term".to_string(),
            candidates: 1,
            with_strength: usize::from((0..3).any(|c| x[c][0] > 0.0)),
            share: scored.contribution[0] / ysum,
            seen_median: None,
        },
    );

    // The fit by facing, and the neighbours' bound.
    let mut facings = Vec::new();
    for (label, test) in [
        ("up", (|n: Vec3| n.z > 0.7) as fn(Vec3) -> bool),
        ("down", |n: Vec3| n.z < -0.7),
        ("sideways", |n: Vec3| n.z.abs() <= 0.7),
    ] {
        let chosen: Vec<usize> = (0..samples.len())
            .filter(|&s| test(samples[s].normal))
            .collect();
        let mean = |f: &dyn Fn(usize) -> [f32; 3]| {
            chosen
                .iter()
                .map(|&s| f64::from(luma(f(s))))
                .sum::<f64>()
                / chosen.len().max(1) as f64
        };
        facings.push((
            label,
            chosen.len(),
            mean(&|s| samples[s].target),
            mean(&|s| scored.predicted[s]),
        ));
    }
    let neighbours_r2 = neighbours(&samples);

    // The rooms.
    let mut rooms = Vec::new();
    for (room, layout) in area.rooms.iter().enumerate() {
        let in_room = |s: usize| samples[s].room == room;
        let count = (0..samples.len()).filter(|&s| in_room(s)).count();
        if count == 0 {
            continue;
        }
        let room_score = score(&samples, x, &in_room, &fill).score;
        let stored_mean = samples
            .iter()
            .filter(|s| s.room == room)
            .map(|s| luma(maps[s.map].stored[s.texel]))
            .sum::<f32>()
            / count as f32;
        rooms.push(RoomFit {
            room: layout.model.clone(),
            samples: count,
            stored_mean,
            placed_share: room_score.placed_share,
            flat_share: room_score.flat_share,
            error_left: room_score.error_left,
            r2: room_score.r2,
        });
    }

    // The pictures.
    let stem = if every_glow {
        format!("{}-every-glow", area.resref.to_ascii_lowercase())
    } else {
        area.resref.to_ascii_lowercase()
    };
    let sheet = lightmap_sheet(
        &maps,
        &samples,
        &scored.predicted,
        &out.join(format!("{stem}-lightmaps.png")),
    )?;
    top_down(
        &samples,
        &scored.predicted,
        &candidates,
        x,
        &out.join(format!("{stem}-top.png")),
    )?;

    let fit = AreaFit {
        game: game.key(),
        module: module.to_string(),
        area: area.resref.clone(),
        samples: samples.len(),
        held_out_samples: samples.iter().filter(|s| s.held_out).count(),
        rooms_sampled: rooms.len(),
        lightmaps: maps.len(),
        lamps,
        glowing_meshes: glowing,
        every_glow,
        panels,
        suns,
        best,
        best_falloff: FALLOFFS[best_k],
        best_rays: RAYS[best_mode],
        falloffs,
        subsets,
        bounce: bounced,
        bounce_held_out_r2,
        bounce_control_r2,
        kinds: kinds.into_values().collect(),
        facings,
        neighbours_r2,
        rooms,
        candidates: candidate_fits,
        sheet,
    };
    let json = serde_json::to_string_pretty(&fit).map_err(|error| error.to_string())?;
    std::fs::write(out.join(format!("{stem}.json")), json).map_err(|e| e.to_string())?;
    std::fs::write(out.join(format!("{stem}.txt")), report(&fit)).map_err(|e| e.to_string())?;
    Ok(fit)
}

/// R² of each sample predicted by the mean of the other samples within
/// `NEIGHBOURS` whose facing is within 25 degrees of its own.
fn neighbours(samples: &[Sample]) -> f64 {
    let mut cells: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
    for (s, sample) in samples.iter().enumerate() {
        cells
            .entry(sample.at.to_array().map(|v| v.floor() as i32))
            .or_default()
            .push(s as u32);
    }
    let predicted = parallel(samples.len(), |s| {
        let sample = &samples[s];
        let cell = sample.at.to_array().map(|v| v.floor() as i32);
        let (mut sum, mut count) = ([0.0f32; 3], 0usize);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let key = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                    for &o in cells.get(&key).map_or(&[][..], Vec::as_slice) {
                        let other = &samples[o as usize];
                        if o as usize == s
                            || other.normal.dot(sample.normal) < 0.9
                            || (other.at - sample.at).length() > NEIGHBOURS
                        {
                            continue;
                        }
                        for (sum, target) in sum.iter_mut().zip(other.target) {
                            *sum += target;
                        }
                        count += 1;
                    }
                }
            }
        }
        (count > 0).then(|| sum.map(|v| v / count as f32))
    });
    let mut mean = [0.0f64; 3];
    let mut count = 0usize;
    for (s, p) in predicted.iter().enumerate() {
        if p.is_some() {
            count += 1;
            for (mean, target) in mean.iter_mut().zip(samples[s].target) {
                *mean += f64::from(target);
            }
        }
    }
    let mean = mean.map(|v| v / count.max(1) as f64);
    let (mut sse, mut sst) = (0.0f64, 0.0f64);
    for (s, p) in predicted.iter().enumerate() {
        let Some(p) = p else { continue };
        for c in 0..3 {
            let y = f64::from(samples[s].target[c]);
            sse += (y - f64::from(p[c])).powi(2);
            sst += (y - mean[c]).powi(2);
        }
    }
    if sst > 0.0 { 1.0 - sse / sst } else { 0.0 }
}

/// The candidates moved to random places in the rooms, for a control fit.
/// A lamp floats 0.3 to 2.5 m off a random sample's surface, never through
/// a wall. A panel lies on a random sample's surface and faces out of it. A
/// sphere becomes a point there. Radius, kind and flags stay.
fn scatter(candidates: &[Candidate], samples: &[Sample], level: &Level, seed: u64) -> Vec<Candidate> {
    candidates
        .iter()
        .enumerate()
        .map(|(j, candidate)| {
            let mut moved = candidate.clone();
            if candidate.sun() {
                return moved;
            }
            let pick = (hash(&[seed, j as u64, 1]) % samples.len() as u64) as usize;
            let sample = &samples[pick];
            moved.shape = match candidate.shape {
                Shape::Rect { half, .. } => {
                    let (side, _) = sample.normal.any_orthonormal_pair();
                    let across = sample.normal.cross(side);
                    Shape::Rect {
                        centre: sample.at + sample.normal * 0.03,
                        half: [side * half[0].length(), across * half[1].length()],
                        normal: sample.normal,
                    }
                }
                Shape::Point(_) | Shape::Sphere { .. } => {
                    let lift = 0.3 + 2.2 * unit(&[seed, j as u64, 2]);
                    let lift = level
                        .first_hit(sample.at, sample.normal, lift)
                        .map_or(lift, |(_, t)| t * 0.5);
                    Shape::Point(sample.at + sample.normal * lift)
                }
            };
            moved
        })
        .collect()
}

/// Each sample's light after one bounce, from the direct light of `design`
/// under falloff `k`: `BOUNCE_RAYS` rays spread by the cosine over the
/// sample's side. Each ray that meets the front of a lightmapped surface
/// takes the direct light of the nearest sample there, times the surface's
/// colour. With rays spread by the cosine, the mean of what they bring is
/// the light arriving.
fn bounce(
    samples: &[Sample],
    facts: &[SurfaceFacts],
    level: &Level,
    design: &Design,
    k: usize,
    n: usize,
) -> Vec<Row> {
    let mut cells: HashMap<[i32; 3], Vec<u32>> = HashMap::new();
    for (s, sample) in samples.iter().enumerate() {
        cells
            .entry(sample.at.to_array().map(|v| v.floor() as i32))
            .or_default()
            .push(s as u32);
    }
    let nearest = |at: Vec3, facing: Vec3| -> Option<usize> {
        let cell = at.to_array().map(|v| v.floor() as i32);
        let mut best: Option<(f32, usize)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let key = [cell[0] + dx, cell[1] + dy, cell[2] + dz];
                    for &s in cells.get(&key).map_or(&[][..], Vec::as_slice) {
                        let sample = &samples[s as usize];
                        if sample.normal.dot(facing) < 0.7 {
                            continue;
                        }
                        let d = (sample.at - at).length();
                        if d <= BOUNCE_NEAR && best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, s as usize));
                        }
                    }
                }
            }
        }
        best.map(|(_, s)| s)
    };
    parallel(samples.len(), |s| {
        let sample = &samples[s];
        let (side, across) = sample.normal.any_orthonormal_pair();
        let mut sum: HashMap<u32, [f32; 3]> = HashMap::new();
        for ray in 0..BOUNCE_RAYS {
            let (u1, u2) = (
                unit(&[s as u64, ray as u64, 7]),
                unit(&[s as u64, ray as u64, 8]),
            );
            let r = u1.sqrt();
            let phi = std::f32::consts::TAU * u2;
            let direction = (side * (r * phi.cos())
                + across * (r * phi.sin())
                + sample.normal * (1.0 - u1).max(0.0).sqrt())
            .normalize();
            let Some((surface, t)) =
                level.first_hit(sample.at + direction * RAY_START, direction, BOUNCE_REACH)
            else {
                continue;
            };
            let fact = &facts[surface];
            if !fact.lightmapped || fact.normal.dot(direction) >= 0.0 {
                continue;
            }
            let at = sample.at + direction * (RAY_START + t);
            let Some(hit) = nearest(at, fact.normal) else {
                continue;
            };
            for &(j, v) in design.row(hit) {
                let entry = sum.entry(j).or_insert([0.0; 3]);
                for (entry, albedo) in entry.iter_mut().zip(fact.albedo) {
                    *entry += v[k] * albedo / BOUNCE_RAYS as f32;
                }
            }
        }
        let mut row: Row = sum.into_iter().filter(|(j, _)| (*j as usize) < n).collect();
        row.sort_by_key(|(j, _)| *j);
        row
    })
}

// ---------------------------------------------------------------------------
// The pictures.

/// The difference's colour: warm where the lightmap is brighter than the
/// fit (a light the fit lacks), blue where it is darker.
fn difference_colour(target: [f32; 3], fitted: [f32; 3], scale: f32) -> [u8; 3] {
    let r = luma(target) - luma(fitted);
    let t = (r.abs() / scale).clamp(0.0, 1.0);
    let byte = |v: f32| (v * 255.0).round() as u8;
    if r >= 0.0 {
        [byte(t), byte(0.85 * t), byte(0.4 * t)]
    } else {
        [byte(0.25 * t), byte(0.5 * t), byte(t)]
    }
}

fn to_bytes(c: [f32; 3]) -> [u8; 3] {
    c.map(|v| (encoded(v).clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A lightmap's place in the lightmaps' picture: its name, and the x, y,
/// width and height of its first panel.
type SheetPlace = (String, u32, u32, u32, u32);

/// Every lightmap of the area as three panels: the lightmap, the fitted
/// light, and their difference. Texels no sample covers are dark grey in
/// the fit and the difference. Returns each lightmap's place.
fn lightmap_sheet(
    maps: &[Map],
    samples: &[Sample],
    predicted: &[[f32; 3]],
    path: &Path,
) -> Result<Vec<SheetPlace>, String> {
    const WIDTH: u32 = 1800;
    const GAP: u32 = 6;
    let scale_of = |m: &Map| (128 / m.width.max(m.height)).max(1) as u32;
    let mut fitted: Vec<Vec<Option<[f32; 3]>>> =
        maps.iter().map(|m| vec![None; m.stored.len()]).collect();
    for (s, sample) in samples.iter().enumerate() {
        fitted[sample.map][sample.texel] = Some(predicted[s]);
    }
    let mean = samples.iter().map(|s| luma(s.target)).sum::<f32>() / samples.len() as f32;
    // Shelves, left to right.
    let mut places = Vec::new();
    let (mut x, mut y, mut shelf) = (GAP, GAP, 0u32);
    for m in maps {
        let s = scale_of(m);
        let (w, h) = (m.width as u32 * s, m.height as u32 * s);
        let cell = 3 * w + 2 * 2 + GAP;
        if x + cell > WIDTH && x > GAP {
            x = GAP;
            y += shelf + GAP;
            shelf = 0;
        }
        places.push((x, y, w, h, s));
        x += cell;
        shelf = shelf.max(h);
    }
    let height = y + shelf + GAP;
    let mut image = image::RgbImage::from_pixel(WIDTH, height.max(1), image::Rgb([48, 48, 56]));
    let mut sheet = Vec::new();
    for (m, (map, &(x0, y0, w, h, s))) in maps.iter().zip(&places).enumerate() {
        sheet.push((map.name.clone(), x0, y0, w, h));
        for py in 0..h {
            for px in 0..w {
                let texel = (py / s) as usize * map.width + (px / s) as usize;
                let stored = map.stored[texel].map(|v| (v * 255.0).round() as u8);
                image.put_pixel(x0 + px, y0 + py, image::Rgb(stored));
                let (fit, diff) = match fitted[m][texel] {
                    Some(f) => (
                        to_bytes(f),
                        difference_colour(map.stored[texel].map(linear), f, mean),
                    ),
                    None => ([24; 3], [24; 3]),
                };
                image.put_pixel(x0 + w + 2 + px, y0 + py, image::Rgb(fit));
                image.put_pixel(x0 + 2 * (w + 2) + px, y0 + py, image::Rgb(diff));
            }
        }
    }
    image
        .save(path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(sheet)
}

/// The floors from above: the lightmap, the fit and the difference side by
/// side. A sample faces up when its normal's height is above 0.7. A higher
/// floor is drawn over a lower one. The difference marks each lamp (a
/// square) and each panel (a ring), green with a fitted strength and grey
/// without.
fn top_down(
    samples: &[Sample],
    predicted: &[[f32; 3]],
    candidates: &[Candidate],
    x: &Strengths,
    path: &Path,
) -> Result<(), String> {
    let mut floor: Vec<usize> = (0..samples.len())
        .filter(|&s| samples[s].normal.z > 0.7)
        .collect();
    if floor.is_empty() {
        return Ok(());
    }
    floor.sort_by(|a, b| samples[*a].at.z.total_cmp(&samples[*b].at.z));
    let (low, high) = floor.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(low, high), &s| {
            let p = samples[s].at.truncate();
            (low.min(p), high.max(p))
        },
    );
    let extent = (high - low).max(Vec2::splat(1.0));
    let per_metre = (900.0 / extent.x.max(extent.y)).min(24.0);
    let (w, h) = (
        (extent.x * per_metre) as u32 + 8,
        (extent.y * per_metre) as u32 + 8,
    );
    let mean = samples.iter().map(|s| luma(s.target)).sum::<f32>() / samples.len() as f32;
    let mut image = image::RgbImage::from_pixel(3 * w + 8, h, image::Rgb([48, 48, 56]));
    let dot = (per_metre * 0.35).ceil().max(1.0) as i64;
    let pixel = |p: Vec2| {
        let q = (p - low) * per_metre;
        (q.x as i64 + 4, (h as i64 - 5) - q.y as i64)
    };
    let paint = |image: &mut image::RgbImage, panel: u32, (px, py): (i64, i64), c: [u8; 3]| {
        if px >= 0 && py >= 0 && (px as u32) < w && (py as u32) < h {
            image.put_pixel(panel * (w + 4) + px as u32, py as u32, image::Rgb(c));
        }
    };
    for &s in &floor {
        let sample = &samples[s];
        let (px, py) = pixel(sample.at.truncate());
        let colours = [
            to_bytes(sample.target),
            to_bytes(predicted[s]),
            difference_colour(sample.target, predicted[s], mean),
        ];
        for dy in -dot..=dot {
            for dx in -dot..=dot {
                for (panel, colour) in colours.iter().enumerate() {
                    paint(&mut image, panel as u32, (px + dx, py + dy), *colour);
                }
            }
        }
    }
    for (j, candidate) in candidates.iter().enumerate() {
        if candidate.sun() {
            continue;
        }
        let strong = (0..3).any(|c| x[c][j + 1] > 0.0);
        let colour = if strong { [60, 230, 90] } else { [150, 150, 150] };
        let (px, py) = pixel(candidate.centre().truncate());
        for dy in -3i64..=3 {
            for dx in -3i64..=3 {
                let ring = dx.abs().max(dy.abs()) == 3;
                let mark = match candidate.group {
                    Group::Panel => ring,
                    _ => dx.abs() <= 2 && dy.abs() <= 2,
                };
                if mark {
                    paint(&mut image, 2, (px + dx, py + dy), colour);
                }
            }
        }
    }
    image
        .save(path)
        .map_err(|error| format!("{}: {error}", path.display()))
}

// ---------------------------------------------------------------------------
// The text.

fn report(fit: &AreaFit) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# Lights fitted to {} ({}, {})", fit.area, fit.module, fit.game);
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "{} lightmap texels in {} rooms on {} lightmaps. {} of them are held out. {} lamps, {} panels (of {} glowing meshes), {} suns and one flat term.",
        fit.samples,
        fit.rooms_sampled,
        fit.lightmaps,
        fit.held_out_samples,
        fit.lamps,
        fit.panels,
        fit.glowing_meshes,
        fit.suns
    );
    if fit.every_glow {
        let _ = writeln!(
            out,
            "Every glowing mesh of a room or a placeable is a panel here, not only those that look like a light."
        );
    }
    let _ = writeln!(
        out,
        "The fit is in linear light, one solve per colour channel. R² is 1 minus the squared error over the squared spread around the mean, so the flat term alone scores 0. A share is light over the lightmap's light. The error left is the root of the squared error over the root of the squared lightmap."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "## Direct light by rays and falloff");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Rays for | Falloff | R² | R² on held-out faces | R² of the controls | Placed lights' share | Flat term's share | Placed share in the controls | Error left |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|");
    for f in &fit.falloffs {
        let _ = writeln!(
            out,
            "| {} | {} | {:.3} | {:.3} | {:.3} | {:.0}% | {:.0}% | {:.0}% | {:.2} |",
            f.rays,
            f.falloff,
            f.all.r2,
            f.held_out_r2,
            f.control_r2,
            100.0 * f.all.placed_share,
            100.0 * f.all.flat_share,
            100.0 * f.control_placed_share,
            f.all.error_left
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "The best on held-out faces is falloff {} with rays for {}. The sections below use it.",
        fit.best_falloff, fit.best_rays
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "## Parts of the candidates, best falloff");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Candidates | R² |");
    let _ = writeln!(out, "|---|---|");
    for (label, r2) in &fit.subsets {
        let _ = writeln!(out, "| {label} | {r2:.3} |");
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "## One bounce, best falloff");
    let _ = writeln!(out);
    let b = &fit.bounce;
    let _ = writeln!(
        out,
        "R² {:.3}, on held-out faces {:.3}, one control {:.3}. Placed lights' share {:.0}%, of which {:.0}% of the lightmap's light arrives by the bounce. Flat term's share {:.0}%. Error left {:.2}.",
        b.r2,
        fit.bounce_held_out_r2,
        fit.bounce_control_r2,
        100.0 * b.placed_share,
        100.0 * b.bounce_share,
        100.0 * b.flat_share,
        b.error_left
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "## Kinds, best falloff, direct");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Kind | Candidates | With a strength | Share of the light | Seen, median |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|");
    let mut kinds: Vec<&KindFit> = fit.kinds.iter().collect();
    kinds.sort_by(|a, b| b.share.total_cmp(&a.share));
    for k in kinds {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {:.1}% | {} |",
            k.kind,
            k.candidates,
            k.with_strength,
            100.0 * k.share,
            k.seen_median
                .map_or("-".to_string(), |v| format!("{:.0}%", 100.0 * v))
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Seen is the share of the samples within {SEEN_NEAR} m that face a candidate which its rays reach."
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "## The strongest candidates, best falloff, direct");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Candidate | Kind | Room | Shape | Share | Fitted colour (linear) | Listed colour |");
    let _ = writeln!(out, "|---|---|---|---|---|---|---|");
    let mut strongest: Vec<&CandidateFit> = fit.candidates.iter().filter(|c| c.share > 0.0).collect();
    strongest.sort_by(|a, b| b.share.total_cmp(&a.share));
    for c in strongest.iter().take(25) {
        let total: f64 = c.strength.iter().sum::<f64>().max(1e-30);
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {:.1}% | {:.2} {:.2} {:.2} | {:.2} {:.2} {:.2} |",
            c.name,
            c.kind,
            c.room.as_deref().unwrap_or("-"),
            c.shape,
            100.0 * c.share,
            c.strength[0] / total,
            c.strength[1] / total,
            c.strength[2] / total,
            c.colour[0],
            c.colour[1],
            c.colour[2]
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "## By facing, best falloff, direct");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Facing | Samples | Lightmap mean (linear luma) | Fit mean |");
    let _ = writeln!(out, "|---|---|---|---|");
    for (label, count, target, fitted) in &fit.facings {
        let _ = writeln!(out, "| {label} | {count} | {target:.3} | {fitted:.3} |");
    }
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Each texel predicted by the mean of the texels within {NEIGHBOURS} m that face the same way scores R² {:.3}. No light that changes smoothly over a metre can do much better.",
        fit.neighbours_r2
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "## Rooms, best falloff, direct");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "| Room | Samples | Lightmap mean (stored) | Placed lights' share | Flat term's share | Error left | R² in the room |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|");
    for r in &fit.rooms {
        let _ = writeln!(
            out,
            "| {} | {} | {:.2} | {:.0}% | {:.0}% | {:.2} | {:.2} |",
            r.room,
            r.samples,
            r.stored_mean,
            100.0 * r.placed_share,
            100.0 * r.flat_share,
            r.error_left,
            r.r2
        );
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "## The lightmaps' picture");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "Each lightmap has three panels: the lightmap as stored, the fit, and the difference. The difference is warm where the lightmap is brighter than the fit and blue where it is darker. Full colour is a difference as large as the mean texel. The place given is the top left corner of the first panel."
    );
    let _ = writeln!(out);
    for (name, x, y, w, h) in &fit.sheet {
        let _ = writeln!(out, "- {name} at {x}, {y}, {w} by {h}");
    }
    out
}
