//! The bake from the fitted lights on the graphics card (`kjk detail-bake
//! --lights <file> --gpu`). It opens its own Vulkan device with wgpu's ray
//! queries, builds one acceleration structure from the rooms' blocking
//! triangles, and runs its passes as compute shaders (`gpu.wgsl`). The
//! texels and the lights are found on the CPU as the CPU bake finds them.
//!
//! With `--old` it does what the CPU bake does, ray for ray, so the two can
//! be compared. Without it, it makes the bake the owner judges:
//!
//! - **Soft shadows.** Each light casts `SAMPLES` rays a texel, spread over
//!   its lamp. A strip's rays spread along its length. A light that shines
//!   one way spreads them over a disc of `LAMP` metres across that way, and
//!   any other light over a ball of that size. The share a texel sees is
//!   then smoothed over its neighbours on the same surface
//!   (`smooth_shadows`), so a shadow's edge shows no steps of the texels.
//! - **More bounces.** The light is gathered `BOUNCES` times, each time from
//!   the direct light and the last bounce together.
//! - **One level for each room.** Each room's light from the lights is then
//!   scaled so that its mean stored level is that of the live look
//!   (`room_levels`).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use asset_odyssey::module::AreaIr;
use bevy::math::{IVec3, Vec3};
use vfs::resolver::Resolver;
use wgpu::util::DeviceExt as _;

use super::{
    FAR_LIGHT, Field, Fitted, GAMMA, LIFT, LIGHT_CELL, LightGrid, Point, SCALE, SHORT, Stored, Texels,
    WALL_RAYS, bounds, direct, directions, field_of, points, stored_level, texels, write_sets,
};
use crate::light_probes::Level;
use crate::room_lights::Reader;

/// How many shadow rays each light casts from a texel in the new bake.
const SAMPLES: u32 = 64;
/// The radius of a lamp that has no glass of its own, and twice the radius
/// across such a strip (metres). The soft shadows spread over it.
const LAMP: f32 = 0.15;
/// How far from a light's lamp a glowing triangle may lie and still be part
/// of its glass (metres). The lamp is the light's place moved back by `OFF`
/// against the way it shines, along the strip's length for a strip.
const LAMP_REACH: f32 = 0.6;
/// How near its lamp the nearest glowing triangle must lie to be the lamp's
/// glass (metres).
const LAMP_NEAR: f32 = 0.3;
/// How far a fitted light stands off its lamp (`fitted_lights::OFF`).
const OFF: f32 = 0.25;
/// How far a place on a lamp's glass is lifted off it, toward the light,
/// and how far short of it a shadow ray stops (metres).
const GLASS_LIFT: f32 = 0.01;
const GLASS_SHORT: f32 = 0.03;
/// How many times the new bake gathers bounced light.
const BOUNCES: usize = 3;
/// How many rays a texel casts for each bounce in the new bake. It is a
/// square, so the rays lie on a grid.
const BOUNCE_RAYS: u32 = 256;
/// How many texels one dispatch works on. A dispatch must end well within
/// the two seconds Windows gives the graphics card before it resets it.
const CHUNK: usize = 1 << 16;
/// How many rings of neighbouring texels the shadows are smoothed over.
const SMOOTH_RINGS: i64 = 2;
/// How many rings of neighbouring texels each bounce is smoothed over.
const BOUNCE_RINGS: i64 = 4;
/// The least and the most a room's light from the lights is scaled by.
const ROOM_LEAST: f32 = 0.8;
const ROOM_MOST: f32 = 2.0;
/// The mean stored level `b` aims at over the whole area: the live look's
/// mean on the Endar Spire, measured on 18 November 2026. It is a number,
/// so `b` reads nothing of the stored lightmaps.
const B_MEAN: f32 = 0.484;
/// `b`'s stored level is eased above `B_KNEE`, and reaches `B_KNEE +
/// B_OVER` at the most (`eased_level`).
const B_KNEE: f32 = 0.8;
const B_OVER: f32 = 0.45;
/// The least and the most a room's light in `b` is scaled by.
const B_LEAST: f32 = 0.5;
const B_MOST: f32 = 6.0;
/// Where a room's lamps do not reach its level in `b`, an even light shows
/// this share of the level.
const B_FILL_SHARE: f32 = 0.4;

const MODE_DIRECT_OLD: u32 = 0;
const MODE_DIRECT_SOFT: u32 = 1;
const MODE_BOUNCE_OLD: u32 = 2;
const MODE_BOUNCE_NEW: u32 = 3;
const MODE_SKY: u32 = 4;
/// How many rays a texel casts toward the sun and over itself at the sky.
const SUN_RAYS: u32 = 16;
/// How many rays a texel casts toward a sun that has a width of its own.
/// It is a square number: the rays start from a grid over the texel.
const SHARP_SUN_RAYS: u32 = 64;
const SKY_RAYS: u32 = 64;
/// A texel that sees less than `OPENING_FROM` of the sky sees it through an
/// opening in a roof. The opening then shines as a skylight does: with the
/// light that falls on it from the sun and the sky. A texel that sees more
/// than `OPENING_TO` is under the open sky and takes the sky's own light.
/// The owner walked the Dantooine enclave on 18 November 2026 and wrote of
/// the training room's opening: "have light shine down from here across the
/// whole opening. and make the room darker".
const OPENING_FROM: f32 = 0.06;
const OPENING_TO: f32 = 0.2;
/// How many times that light an opening shines with. A floor sees only a
/// few hundredths of the sky through a slit in a roof, and at 1 the room
/// under it is near black. At 4 the training room's floor under its slit
/// is about as bright as the stored lightmaps have it (0.57).
const OPENING_GAIN: f32 = 4.0;
/// A room is under a roof when less than `ROOFED_FROM` of its upward texels
/// lie under the open sky, and it is a courtyard when more than `ROOFED_TO`
/// do. In the Dantooine enclave the rooms under a roof have 0.05 at the
/// most and the two courtyards have 0.74 and 0.82.
const ROOFED_FROM: f32 = 0.1;
const ROOFED_TO: f32 = 0.3;
/// How many times the light that falls on it an opening shines with in a
/// room under a roof. All of the room's daylight comes through the opening
/// there. The light on a texel stops at the light that falls on the open
/// ground, so a texel that sees much of the sky is no brighter than the
/// ground outside. A courtyard keeps `OPENING_GAIN` and the rule above.
/// The owner walked the enclave on 19 November 2026 with every room at
/// `OPENING_GAIN` and wrote of the training room "shine down some more
/// light here", and of the council chamber "shine down even more light here
/// from the skylight (opening in the roof)".
const ROOF_GAIN: f32 = 8.0;
/// A texel is under the open sky when this share of its sky rays is free.
/// `--sun-fit` reads only such texels.
const FIT_OPEN: f32 = 0.3;

/// What the bake on the graphics card was asked for.
pub(crate) struct GpuOptions {
    /// `rays × rays` bounce rays a texel, in the CPU bake's way (`old`).
    pub rays: usize,
    /// Do what the CPU bake does, ray for ray.
    pub old: bool,
    /// Print what the texels near this place see of each light, and what
    /// blocks it, on the CPU (metres: x, y, z and a radius).
    pub probe: Option<(Vec3, f32)>,
    /// A file of stated levels for `b`: one line for each room, its name and
    /// the mean stored level its texels should have. A room it does not
    /// list takes the level `b` works out for itself.
    pub levels: Option<std::path::PathBuf>,
    /// The area's sun and sky, where it has them.
    pub sun: Option<Sun>,
    /// Print the ways to the sun that fit the stored lightmaps best.
    pub sun_fit: bool,
    /// No room takes the even light. A room that its lamps, the sun and the
    /// sky cannot bring to its level stays darker. The owner said of the
    /// enclave on 18 November 2026: "we shouldnt have so much artificial
    /// light generated for the rooms. we want them naturally dark in the
    /// sides and then have the skylight come down in the middle".
    pub no_even: bool,
    /// A room's level is its level where the game lights it today: each
    /// texel counts by its stored level, in the room's target and in what
    /// the lamps are scaled to. A bake that reaches a room's plain mean is
    /// darker than today where a player looks. It spreads its light into
    /// the corners that the stored lightmaps leave black, and the floors
    /// and walls that they light take less. The owner walked the Dantooine
    /// enclave on 18 November 2026 and said: "its too dark overall
    /// everywhere".
    pub weigh: bool,
    /// The meshes that are skylights, by their node's name. A skylight
    /// sends the light that falls on it from the sun and the sky down into
    /// its room, evenly to every side. The Dantooine enclave's training room
    /// and council chamber have a plain ceiling with no lightmap, which the
    /// game draws at full white. The stored lightmaps under it are as
    /// bright as the courtyard's are under the open sky.
    pub skylights: Vec<String>,
}

/// An area's sun and sky for set `b`. The games' files hold neither. The
/// owner walked the Dantooine enclave's first bake on 18 November 2026 and
/// said: "without the sun its all very dark" and "we should add the sun".
#[derive(Clone, Copy)]
pub(crate) struct Sun {
    /// The way to the sun, a unit vector in game space.
    pub toward: Vec3,
    /// The stored level a surface that faces the sun squarely shows of it.
    pub colour: Vec3,
    /// The stored level a surface that sees the whole sky shows of it.
    pub sky: Vec3,
    /// A second sun, where the area has two: the way to it and its colour.
    /// Tatooine's sky shows two suns. The owner saw Anchorhead baked with
    /// one sun between them on 19 November 2026 and said: "it doesnt look
    /// like shadows are aligned with the actual suns".
    pub second: Option<(Vec3, Vec3)>,
    /// Half the width of each sun as the bake sees it (radians), where the
    /// area gives one. Its shadows are then sharp: a texel casts
    /// `SHARP_SUN_RAYS` rays from places spread over its own square, so an
    /// edge that crosses the texel shades it by the share it covers. Without
    /// one the sun is `SUN_WIDTH` wide in the shader and every ray starts at
    /// the texel's middle. The owner walked Anchorhead on 19 November 2026
    /// and said of its shadows: "why is the shadow from this wall so
    /// scattered at the edge?", "shadow here looks blocky like a staircase"
    /// and "why is no shadow cast from that tower?".
    pub width: Option<f32>,
}

impl Sun {
    /// The light that falls on a level surface under the open sky, linear.
    fn on_level_ground(&self) -> Vec3 {
        let second = self
            .second
            .map_or(Vec3::ZERO, |(toward, colour)| colour.powf(GAMMA) * toward.z.max(0.0));
        self.colour.powf(GAMMA) * self.toward.z.max(0.0) + second + self.sky.powf(GAMMA)
    }
}

/// The graphics card, opened for the bake.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    name: String,
    /// The errors wgpu reported. A pass that ends with one fails the bake.
    errors: Arc<Mutex<Vec<String>>>,
}

impl Gpu {
    fn open() -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|error| format!("no Vulkan graphics card: {error}"))?;
        let info = adapter.get_info();
        let features = wgpu::Features::EXPERIMENTAL_RAY_QUERY;
        if !adapter.features().contains(features) {
            return Err(format!("{} has no ray queries under wgpu", info.name));
        }
        let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("detail_bake"),
            required_features: features,
            required_limits: adapter.limits(),
            // SAFETY: the ray queries are an experimental feature of wgpu.
            // The game's Ultra shadows use the same feature on this device.
            experimental_features: unsafe { wgpu::ExperimentalFeatures::enabled() },
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|error| format!("{}: {error}", info.name))?;
        let errors: Arc<Mutex<Vec<String>>> = Arc::default();
        let kept = errors.clone();
        device.on_uncaptured_error(Arc::new(move |error: wgpu::Error| {
            kept.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(error.to_string());
        }));
        let kept = errors.clone();
        device.set_device_lost_callback(move |reason, message| {
            kept.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(format!("device lost ({reason:?}): {message}"));
        });
        Ok(Gpu {
            device,
            queue,
            name: format!("{} ({:?}, driver {})", info.name, info.backend, info.driver_info),
            errors,
        })
    }

    /// Waits for the graphics card and fails with the first error wgpu
    /// reported.
    fn wait(&self) -> Result<(), String> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|error| format!("waiting for the graphics card: {error}"))?;
        let errors = self
            .errors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match errors.first() {
            Some(error) => Err(format!("wgpu: {error}")),
            None => Ok(()),
        }
    }

    fn buffer(&self, label: &str, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        // A binding may not be empty, so an empty list gets one blank value.
        let blank = [0u8; 16];
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: if bytes.is_empty() { &blank } else { bytes },
                usage,
            })
    }

    fn storage(&self, label: &str, bytes: &[u8]) -> wgpu::Buffer {
        self.buffer(
            label,
            bytes,
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        )
    }

    /// The contents of `buffer` as values of four floats.
    fn read(&self, buffer: &wgpu::Buffer) -> Result<Vec<[f32; 4]>, String> {
        let size = buffer.size();
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("detail_bake_read"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        self.queue.submit([encoder.finish()]);
        let mapped: Arc<Mutex<Option<Result<(), wgpu::BufferAsyncError>>>> = Arc::default();
        let kept = mapped.clone();
        staging.map_async(wgpu::MapMode::Read, .., move |result| {
            *kept.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
        });
        self.wait()?;
        match mapped
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            Some(Ok(())) => {}
            Some(Err(error)) => return Err(format!("reading a buffer back: {error}")),
            None => return Err("reading a buffer back: it was never mapped".into()),
        }
        let values = {
            let view = staging.get_mapped_range(..);
            view.as_chunks::<16>()
                .0
                .iter()
                .map(|value| {
                    std::array::from_fn(|k| {
                        f32::from_le_bytes([
                            value[4 * k],
                            value[4 * k + 1],
                            value[4 * k + 2],
                            value[4 * k + 3],
                        ])
                    })
                })
                .collect()
        };
        staging.unmap();
        Ok(values)
    }
}

fn float_bytes(values: &[[f32; 4]]) -> Vec<u8> {
    values
        .iter()
        .flatten()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn word_bytes(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|value| value.to_le_bytes()).collect()
}

fn four(v: Vec3, w: f32) -> [f32; 4] {
    [v.x, v.y, v.z, w]
}

/// A place the direct pass works on: a new texel, or the middle of a room
/// triangle.
struct Place {
    at: Vec3,
    normal: Vec3,
    shading: Option<Vec3>,
    /// A new texel's side in the room (metres), and 0 for a triangle's
    /// middle.
    side: f32,
}

/// The compute passes and their buffers.
struct Passes<'g> {
    gpu: &'g Gpu,
    pipeline: wgpu::ComputePipeline,
    group: wgpu::BindGroup,
    params: wgpu::Buffer,
    field_a: wgpu::Buffer,
    field_b: wgpu::Buffer,
    tri_light: wgpu::Buffer,
    results: wgpu::Buffer,
}

impl Passes<'_> {
    /// Runs one pass over the first `count` places, a chunk at a time.
    fn run(&self, mode: u32, count: usize, samples: u32, rays: u32, seed: u32) -> Result<(), String> {
        self.run_toward(mode, count, samples, rays, seed, Vec3::Z, 0.0)
    }

    /// `run`, with the way to the sun for `MODE_SKY` and half the sun's
    /// width. A width of 0 is the shader's own, with every ray from the
    /// texel's middle.
    fn run_toward(
        &self,
        mode: u32,
        count: usize,
        samples: u32,
        rays: u32,
        seed: u32,
        toward: Vec3,
        width: f32,
    ) -> Result<(), String> {
        for first in (0..count).step_by(CHUNK) {
            let size = CHUNK.min(count - first);
            let mut params = word_bytes(&[
                first as u32,
                size as u32,
                mode,
                samples,
                rays,
                seed,
            ]);
            params.extend(SHORT.to_le_bytes());
            params.extend(LIFT.to_le_bytes());
            params.extend(four(toward, width).iter().flat_map(|value| value.to_le_bytes()));
            self.gpu.queue.write_buffer(&self.params, 0, &params);
            let mut encoder = self
                .gpu
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("detail_bake"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.group, &[]);
                pass.dispatch_workgroups(size.div_ceil(64) as u32, 1, 1);
            }
            self.gpu.queue.submit([encoder.finish()]);
            self.gpu.wait()?;
        }
        Ok(())
    }

    fn write_fields(&self, a: &[[f32; 4]], b: &[[f32; 4]]) {
        self.gpu.queue.write_buffer(&self.field_a, 0, &float_bytes(a));
        self.gpu.queue.write_buffer(&self.field_b, 0, &float_bytes(b));
    }
}

/// A field laid out as the shader reads it: each lightmap's new texels one
/// after the other, `w` 1 where a texel has a value.
fn flat(field: &Field) -> Vec<[f32; 4]> {
    field
        .iter()
        .flatten()
        .map(|value| value.map_or([0.0; 4], |v| four(v, 1.0)))
        .collect()
}

/// Bakes one area from the fitted lights in `lights` into `folder/a` and
/// `folder/b` on the graphics card.
pub(super) fn bake_lights(
    reader: &mut Reader,
    resolver: &Resolver,
    area: &AreaIr,
    lights: &[Fitted],
    far: &[Fitted],
    options: &GpuOptions,
    folder: &Path,
) -> Result<String, String> {
    let started = Instant::now();
    let mut times: Vec<(String, f64)> = Vec::new();
    let mut last = Instant::now();
    let mut lap = |name: String, times: &mut Vec<(String, f64)>| {
        if std::env::var_os("KJK_BAKE_LAPS").is_some() {
            eprintln!("{name}: {:.1} seconds", last.elapsed().as_secs_f64());
        }
        times.push((name, last.elapsed().as_secs_f64()));
        last = Instant::now();
    };
    let Texels {
        maps,
        surfaces,
        facts,
        claims,
        unwalked,
        names,
        grass,
    } = texels(reader, resolver, area, options.sun.is_some())?;
    if std::env::var_os("KJK_BAKE_LAPS").is_some() {
        eprintln!("{} triangles, bounds {:?}", surfaces.len(), bounds(&surfaces));
    }
    let level = Level::new(&surfaces, bounds(&surfaces));
    let points = points(claims, &level);
    let grid = LightGrid::new(lights);
    lap("texels on the CPU".into(), &mut times);

    if let Some((at, radius)) = options.probe {
        probe(&maps, &facts, &names, &level, &points, lights, at, radius);
    }

    let losses = losses(&facts, &level, &points, &grid);
    lap("where the shadows take the light".into(), &mut times);
    let lamps = lamps(lights, &surfaces, &facts);
    lap("the lamps' glass".into(), &mut times);

    let gpu = Gpu::open()?;
    lap("opening the graphics card".into(), &mut times);

    // The places of the direct pass: the texels, then the triangles' middles.
    let mut places: Vec<Place> = (points.iter())
        .map(|point| Place {
            at: point.at,
            normal: point.normal,
            shading: point.shading,
            side: point.size / SCALE as f32,
        })
        .collect();
    places.extend(surfaces.iter().zip(&facts).map(|(surface, fact)| {
        let middle = surface.corners.iter().sum::<Vec3>() / 3.0;
        Place {
            at: middle + fact.normal * LIFT,
            normal: fact.normal,
            shading: Some(fact.normal),
            side: 0.0,
        }
    }));

    // The light grid's cells that hold a place, each with its list of lights.
    // Cell 0 is empty.
    let mut cell_of: HashMap<IVec3, u32> = HashMap::new();
    let mut cells: Vec<u32> = vec![0, 0];
    let mut cell_lights: Vec<u32> = Vec::new();
    let place_cells: Vec<u32> = (places.iter())
        .map(|place| {
            let key = (place.at / LIGHT_CELL).floor().as_ivec3();
            let Some(listed) = grid.cells.get(&key) else {
                return 0;
            };
            *cell_of.entry(key).or_insert_with(|| {
                cells.extend([cell_lights.len() as u32, listed.len() as u32]);
                cell_lights.extend(listed);
                (cells.len() / 2 - 1) as u32
            })
        })
        .collect();

    // The buffers.
    let place_values: Vec<[f32; 4]> = (places.iter().zip(&place_cells))
        .flat_map(|(place, cell)| {
            [
                four(place.at, f32::from_bits(*cell)),
                four(place.normal, if place.shading.is_some() { 1.0 } else { 0.0 }),
                four(place.shading.unwrap_or(Vec3::ZERO), place.side),
            ]
        })
        .collect();
    let light_values: Vec<[f32; 4]> = (lights.iter())
        .flat_map(|light| {
            [
                four(light.at, light.radius),
                four(light.colour, light.half),
                four(light.way, light.cone),
                four(light.aim, 0.0),
            ]
        })
        .collect();
    let tri_values: Vec<[f32; 4]> = (facts.iter())
        .flat_map(|fact| {
            [
                four(fact.normal, fact.map.map_or(-1.0, |map| map as f32)),
                four(fact.albedo, 0.0),
                four(fact.glow, 0.0),
                [fact.uv[0].x, fact.uv[0].y, fact.uv[1].x, fact.uv[1].y],
                [fact.uv[2].x, fact.uv[2].y, 0.0, 0.0],
            ]
        })
        .collect();
    let mut map_words: Vec<u32> = Vec::new();
    let mut field_size = 0usize;
    for map in &maps {
        let (w, h) = (map.width * SCALE, map.height * SCALE);
        map_words.extend([field_size as u32, w as u32, h as u32, u32::from(map.clamp)]);
        field_size += w * h;
    }
    let ray_values = |rays: &[Vec3]| -> Vec<[f32; 4]> { rays.iter().map(|d| four(*d, 0.0)).collect() };
    let bounce_dirs = directions(options.rays.max(1));
    let wall_dirs = directions(WALL_RAYS);

    // The acceleration structure: two bottom levels, each in its own
    // instance. The first holds the triangles that cast shadows, the second
    // the lamps' glass, which casts none on the lights (`lamps`). Bounce rays
    // meet both. `ids` gives each triangle's number in `facts`, and an
    // instance's custom data is where its triangles start in `ids`.
    let skylight: Vec<bool> = (facts.iter())
        .map(|fact| {
            let node = names[fact.mesh as usize].split(" (").next();
            options.skylights.iter().any(|name| node == Some(name.as_str()))
        })
        .collect();
    let (blocking, glass): (Vec<u32>, Vec<u32>) =
        (0..surfaces.len() as u32).partition(|&t| !lamps.glass[t as usize]);
    let mut ids = blocking.clone();
    ids.extend(&glass);
    let parts: Vec<(&[u32], u32, u8)> = [
        (&blocking[..], 0, 1u8),
        (&glass[..], blocking.len() as u32, 2u8),
    ]
    .into_iter()
    .filter(|(part, _, _)| !part.is_empty())
    .collect();
    let built: Vec<(
        wgpu::Buffer,
        wgpu::BlasTriangleGeometrySizeDescriptor,
        wgpu::Blas,
        u32,
        u8,
    )> = parts
        .iter()
        .map(|(part, start, mask)| {
            let corners: Vec<u8> = (part.iter())
                .flat_map(|&t| surfaces[t as usize].corners)
                .flat_map(|corner| corner.to_array())
                .flat_map(|value| value.to_le_bytes())
                .collect();
            let buffer = gpu.buffer("detail_bake_corners", &corners, wgpu::BufferUsages::BLAS_INPUT);
            let size = wgpu::BlasTriangleGeometrySizeDescriptor {
                vertex_format: wgpu::VertexFormat::Float32x3,
                vertex_count: (part.len() * 3) as u32,
                index_format: None,
                index_count: None,
                flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
            };
            let blas = gpu.device.create_blas(
                &wgpu::CreateBlasDescriptor {
                    label: Some("detail_bake"),
                    flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
                    update_mode: wgpu::AccelerationStructureUpdateMode::Build,
                },
                wgpu::BlasGeometrySizeDescriptors::Triangles {
                    descriptors: vec![size.clone()],
                },
            );
            (buffer, size, blas, *start, *mask)
        })
        .collect();
    let mut tlas = gpu.device.create_tlas(&wgpu::CreateTlasDescriptor {
        label: Some("detail_bake"),
        max_instances: built.len() as u32,
        flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
        update_mode: wgpu::AccelerationStructureUpdateMode::Build,
    });
    for (slot, (_, _, blas, start, mask)) in built.iter().enumerate() {
        tlas[slot] = Some(wgpu::TlasInstance::new(
            blas,
            [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            *start,
            *mask,
        ));
    }
    let entries: Vec<wgpu::BlasBuildEntry> = (built.iter())
        .map(|(buffer, size, blas, _, _)| wgpu::BlasBuildEntry {
            blas,
            geometry: wgpu::BlasGeometries::TriangleGeometries(vec![wgpu::BlasTriangleGeometry {
                size,
                vertex_buffer: buffer,
                first_vertex: 0,
                vertex_stride: 12,
                index_buffer: None,
                first_index: None,
                transform_buffer: None,
                transform_buffer_offset: None,
            }]),
        })
        .collect();
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.build_acceleration_structures(&entries, [&tlas]);
    gpu.queue.submit([encoder.finish()]);
    gpu.wait()?;

    let blank_field = vec![[0.0f32; 4]; field_size];
    let results = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("detail_bake_results"),
        size: (3 * places.len() * 16) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let tris = gpu.storage("detail_bake_tris", &float_bytes(&tri_values));
    let place_buffer = gpu.storage("detail_bake_places", &float_bytes(&place_values));
    let light_buffer = gpu.storage("detail_bake_lights", &float_bytes(&light_values));
    let cell_buffer = gpu.storage("detail_bake_cells", &word_bytes(&cells));
    let cell_light_buffer = gpu.storage("detail_bake_cell_lights", &word_bytes(&cell_lights));
    let params = gpu.buffer(
        "detail_bake_params",
        &[0u8; 48],
        wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    );
    let dir_buffer = gpu.storage("detail_bake_dirs", &float_bytes(&ray_values(&bounce_dirs)));
    let wall_buffer = gpu.storage("detail_bake_wall_dirs", &float_bytes(&ray_values(&wall_dirs)));
    let map_buffer = gpu.storage("detail_bake_maps", &word_bytes(&map_words));
    let field_a = gpu.storage("detail_bake_field_a", &float_bytes(&blank_field));
    let field_b = gpu.storage("detail_bake_field_b", &float_bytes(&blank_field));
    let tri_light = gpu.storage(
        "detail_bake_tri_light",
        &float_bytes(&vec![[0.0f32; 4]; facts.len()]),
    );
    let lamp_buffer = gpu.storage("detail_bake_lamp_points", &float_bytes(&lamps.points));
    let id_buffer = gpu.storage("detail_bake_ids", &word_bytes(&ids));

    let storage = |binding: u32, read_only: bool| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let mut layout_entries = vec![wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::AccelerationStructure {
            vertex_return: false,
        },
        count: None,
    }];
    layout_entries.extend((1..=5).map(|binding| storage(binding, true)));
    layout_entries.push(wgpu::BindGroupLayoutEntry {
        binding: 6,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    });
    layout_entries.extend((7..=12).map(|binding| storage(binding, true)));
    layout_entries.push(storage(13, false));
    layout_entries.extend((14..=15).map(|binding| storage(binding, true)));
    let layout = gpu
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("detail_bake"),
            entries: &layout_entries,
        });
    let bound: [(u32, &wgpu::Buffer); 15] = [
        (1, &tris),
        (2, &place_buffer),
        (3, &light_buffer),
        (4, &cell_buffer),
        (5, &cell_light_buffer),
        (6, &params),
        (7, &dir_buffer),
        (8, &wall_buffer),
        (9, &map_buffer),
        (10, &field_a),
        (11, &field_b),
        (12, &tri_light),
        (13, &results),
        (14, &lamp_buffer),
        (15, &id_buffer),
    ];
    let mut group_entries = vec![wgpu::BindGroupEntry {
        binding: 0,
        resource: tlas.as_binding(),
    }];
    group_entries.extend(bound.iter().map(|(binding, buffer)| wgpu::BindGroupEntry {
        binding: *binding,
        resource: buffer.as_entire_binding(),
    }));
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("detail_bake"),
        layout: &layout,
        entries: &group_entries,
    });
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("detail_bake"),
            source: wgpu::ShaderSource::Wgsl(include_str!("gpu.wgsl").into()),
        });
    let pipeline_layout = gpu
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("detail_bake"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
    let pipeline = gpu
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("detail_bake"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            cache: None,
        });
    gpu.wait()?;
    let passes = Passes {
        gpu: &gpu,
        pipeline,
        group,
        params,
        field_a,
        field_b,
        tri_light,
        results,
    };
    lap("acceleration structure and buffers".into(), &mut times);

    // The direct light, with shadows and without.
    let mode = if options.old {
        MODE_DIRECT_OLD
    } else {
        MODE_DIRECT_SOFT
    };
    passes.run(mode, places.len(), SAMPLES, 0, 1)?;
    let first = gpu.read(&passes.results)?;
    let count = points.len();
    let counts: Vec<bool> = (0..count).map(|i| first[2 * i][3] > 0.5).collect();
    let mut shadowed: Vec<Vec3> = (0..places.len())
        .map(|i| Vec3::from_slice(&first[2 * i]))
        .collect();
    let open: Vec<Vec3> = (0..places.len())
        .map(|i| Vec3::from_slice(&first[2 * i + 1]))
        .collect();
    drop(first);
    lap("direct light".into(), &mut times);
    // The probe's texels as the graphics card lit them, and the lamp places
    // of the lights that reach them most.
    if let Some((at, radius)) = options.probe {
        for (i, point) in points.iter().enumerate() {
            if point.inside && point.at.distance(at) <= radius {
                outln!(
                    "  gpu texel {} of {} at {:.3}: without shadows {:.3}, with shadows {:.3}",
                    point.texel,
                    maps[point.map].name,
                    point.at,
                    luma(open[i]),
                    luma(shadowed[i])
                );
            }
        }
    }
    if !options.old {
        smooth_shadows(&maps, &points, &counts, &mut shadowed, &open);
        lap("smoothing the shadows".into(), &mut times);
    }
    let direct_light: Vec<Vec3> = shadowed[..count].iter().map(|sum| direct(*sum)).collect();
    let unshadowed: Vec<Vec3> = open[..count].iter().map(|sum| direct(*sum)).collect();
    let tri_direct: Vec<[f32; 4]> = shadowed[count..]
        .iter()
        .map(|sum| four(direct(*sum), 1.0))
        .collect();
    gpu.queue
        .write_buffer(&passes.tri_light, 0, &float_bytes(&tri_direct));

    // The sun's and the sky's light on each texel, for `b` alone. It is
    // nothing in an area without a sun. It is kept apart from the lamps'
    // light, because a room's scale is for its lamps and not for the sun.
    let sun_and_sky = |toward: Vec3, samples: u32, rays: u32, width: f32| -> Result<Vec<[f32; 2]>, String> {
        passes.run_toward(MODE_SKY, count, samples, rays, 20, toward, width)?;
        let read = gpu.read(&passes.results)?;
        Ok((0..count).map(|i| [read[2 * i][0], read[2 * i][1]]).collect())
    };
    // The share of the area's suns that reaches each texel, for the alpha of
    // set `b`. The pass gives a sun's share times how squarely the texel
    // faces it, so that is divided out. The owner walked Anchorhead on 19
    // November 2026 and said: "characters are casting shadows in the shade.
    // not sure if that is correct, should they really do that? what light
    // are they blocking?". The game's shadows read the share and take none
    // of a sun's light where the sun does not reach.
    let reached = |seen: &[[f32; 2]], toward: Vec3| -> Vec<f32> {
        (0..count)
            .map(|i| {
                let facing = points[i].shading.unwrap_or(points[i].normal).dot(toward);
                if facing > 1e-3 { (seen[i][0] / facing).clamp(0.0, 1.0) } else { 0.0 }
            })
            .collect()
    };
    let mut sunlit: Option<Vec<f32>> = None;
    let sky_light: Vec<Vec3> = match &options.sun {
        Some(sun) => {
            let (sun_rays, width) = sun.width.map_or((SUN_RAYS, 0.0), |width| (SHARP_SUN_RAYS, width));
            let seen = sun_and_sky(sun.toward, sun_rays, SKY_RAYS, width)?;
            let mut shares = reached(&seen, sun.toward);
            // The second sun's light on each texel.
            let of_second: Vec<Vec3> = match sun.second {
                Some((toward, colour)) => {
                    let seen = sun_and_sky(toward, sun_rays, 0, width)?;
                    for (share, second) in shares.iter_mut().zip(reached(&seen, toward)) {
                        *share = (*share + second) / 2.0;
                    }
                    seen.iter().map(|[sun, _]| colour.powf(GAMMA) * *sun).collect()
                }
                None => vec![Vec3::ZERO; count],
            };
            sunlit = Some(shares);
            // The sky's light changes slowly over a surface, so the noise
            // of its random rays is smoothed away.
            let sky = smooth_values(
                &maps,
                &points,
                &counts,
                &seen.iter().map(|[_, sky]| Vec3::splat(*sky)).collect::<Vec<_>>(),
            );
            let (of_sun, of_sky) = (sun.colour.powf(GAMMA), sun.sky.powf(GAMMA));
            // The share of each room's upward texels that lie under the open
            // sky, and from it how far the room is under a roof.
            let mut floors: HashMap<String, (f32, f32)> = HashMap::new();
            for i in (0..count).filter(|&i| points[i].inside && counts[i] && points[i].normal.z > 0.5) {
                let floor = floors.entry(room_name(&maps[points[i].map])).or_default();
                floor.0 += 1.0;
                if sky[i].x > OPENING_TO {
                    floor.1 += 1.0;
                }
            }
            let roofed: Vec<f32> = (maps.iter())
                .map(|map| {
                    let (all, open) = floors.get(&room_name(map)).copied().unwrap_or((0.0, 0.0));
                    let share = ((open / all.max(1.0) - ROOFED_FROM) / (ROOFED_TO - ROOFED_FROM)).clamp(0.0, 1.0);
                    1.0 - share * share * (3.0 - 2.0 * share)
                })
                .collect();
            let mut listed: Vec<(String, f32, f32)> = (maps.iter().zip(&roofed))
                .map(|(map, roofed)| {
                    let name = room_name(map);
                    let (all, open) = floors.get(&name).copied().unwrap_or((0.0, 0.0));
                    (name, open / all.max(1.0), *roofed)
                })
                .collect();
            listed.sort_by(|a, b| a.0.cmp(&b.0));
            listed.dedup_by(|a, b| a.0 == b.0);
            outln!("  each room: the share of its upward texels under the open sky, and how far it is under a roof");
            for (name, share, roofed) in listed {
                outln!("    {name}: {share:.3}, {roofed:.2}");
            }
            // A skylight's own texels show the light that falls on it from
            // above. A texel is a skylight's when it lies on one of its
            // triangles and faces the triangle's way.
            let on_skylight = sun.on_level_ground();
            let panes: Vec<usize> = (0..facts.len()).filter(|&t| skylight[t]).collect();
            let pane_maps: std::collections::HashSet<usize> = panes.iter().filter_map(|&t| facts[t].map).collect();
            let light = (0..count)
                .map(|i| {
                    let point = &points[i];
                    let on_pane = pane_maps.contains(&point.map)
                        && panes.iter().any(|&t| {
                            facts[t].normal.dot(point.normal) > 0.9
                                && super::closest(surfaces[t].corners, point.at).distance(point.at) < 4.0 * LIFT
                        });
                    if on_pane {
                        on_skylight
                    } else {
                        let open = ((sky[i].x - OPENING_FROM) / (OPENING_TO - OPENING_FROM)).clamp(0.0, 1.0);
                        let open = open * open * (3.0 - 2.0 * open);
                        let in_a_courtyard = (on_skylight * OPENING_GAIN).lerp(of_sky, open) * sky[i].x;
                        let under_a_roof = on_skylight * (ROOF_GAIN * sky[i].x).min(1.0);
                        of_sun * seen[i][0]
                            + of_second[i]
                            + in_a_courtyard.lerp(under_a_roof, roofed[point.map])
                    }
                })
                .collect();
            lap("the sun and the sky".into(), &mut times);
            light
        }
        None => vec![Vec3::ZERO; count],
    };

    // The bounces. Each pass gathers from field a and field b at once.
    let gather = |a: &[Vec3], b: &[Vec3], mode: u32, seed: u32| -> Result<Vec<[Vec3; 3]>, String> {
        let field_a = flat(&field_of(&maps, &points, &counts, a));
        let field_b = flat(&field_of(&maps, &points, &counts, b));
        passes.write_fields(&field_a, &field_b);
        passes.run(mode, count, 0, BOUNCE_RAYS, seed)?;
        let read = gpu.read(&passes.results)?;
        Ok((0..count)
            .map(|i| {
                [0, 1, 2].map(|k| Vec3::from_slice(&read[3 * i + k]))
            })
            .collect())
    };
    let (bounce_a, bounce_b) = if options.old {
        // The CPU bake's bounces: one for `a`, and for `b` a second from the
        // first with the glow.
        let first = gather(&direct_light, &direct_light, MODE_BOUNCE_OLD, 2)?;
        lap("first bounce".into(), &mut times);
        let bounce_a: Vec<Vec3> = first.iter().map(|[a, _, _]| *a).collect();
        let once: Vec<Vec3> = (0..count)
            .map(|i| direct_light[i] + first[i][0] + first[i][2])
            .collect();
        let second = gather(&once, &once, MODE_BOUNCE_OLD, 3)?;
        lap("second bounce".into(), &mut times);
        let bounce_b: Vec<Vec3> = second.iter().map(|[_, b, glow]| *b + *glow).collect();
        (bounce_a, bounce_b)
    } else {
        let mut bounce_a = vec![Vec3::ZERO; count];
        let mut bounce_b = vec![Vec3::ZERO; count];
        for pass in 0..BOUNCES {
            let from_a: Vec<Vec3> = (0..count).map(|i| direct_light[i] + bounce_a[i]).collect();
            let from_b: Vec<Vec3> = (0..count).map(|i| direct_light[i] + bounce_b[i]).collect();
            let gathered = gather(&from_a, &from_b, MODE_BOUNCE_NEW, 10 + pass as u32)?;
            // Bounced light changes slowly over a surface, so the noise of
            // its random rays is smoothed away over the texels around.
            bounce_a = smooth_values(
                &maps,
                &points,
                &counts,
                &gathered.iter().map(|[a, _, _]| *a).collect::<Vec<_>>(),
            );
            bounce_b = smooth_values(
                &maps,
                &points,
                &counts,
                &gathered.iter().map(|[_, b, glow]| *b + *glow).collect::<Vec<_>>(),
            );
            lap(format!("bounce {}", pass + 1), &mut times);
        }
        (bounce_a, bounce_b)
    };
    // The sun's and the sky's light with its own bounces.
    let sun_light: Vec<Vec3> = if let Some(sun) = &options.sun {
        // What a bounce ray takes from a skylight is the light that falls
        // on it from above, whatever its lightmap holds (`w` is 2). The
        // shader multiplies it by the triangle's colour, so it is divided
        // by that here. Any other triangle without a lightmap gives nothing.
        let on_skylight = sun.on_level_ground();
        let tri_sun: Vec<[f32; 4]> = (facts.iter().zip(&skylight))
            .map(|(fact, &skylight)| {
                let sent = if skylight { on_skylight / fact.albedo.max(Vec3::splat(0.05)) } else { Vec3::ZERO };
                four(sent, if skylight { 2.0 } else { 1.0 })
            })
            .collect();
        gpu.queue
            .write_buffer(&passes.tri_light, 0, &float_bytes(&tri_sun));
        let mut bounced = vec![Vec3::ZERO; count];
        for pass in 0..BOUNCES {
            let from: Vec<Vec3> = (0..count).map(|i| sky_light[i] + bounced[i]).collect();
            let gathered = gather(&from, &from, MODE_BOUNCE_NEW, 30 + pass as u32)?;
            bounced = smooth_values(
                &maps,
                &points,
                &counts,
                &gathered.iter().map(|[_, b, _]| *b).collect::<Vec<_>>(),
            );
            lap(format!("the sun's bounce {}", pass + 1), &mut times);
        }
        (0..count).map(|i| sky_light[i] + bounced[i]).collect()
    } else {
        sky_light
    };

    // Each texel's stored lightmap, read between texels as the GPU reads it.
    let stored_at = |point: &Point| {
        let map = &maps[point.map];
        let w = map.width * SCALE;
        let uv = bevy::math::Vec2::new(
            ((point.texel % w) as f32 + 0.5) / w as f32,
            ((point.texel / w) as f32 + 0.5) / (map.height * SCALE) as f32,
        );
        let [r, g, b, _] = map.sample(uv);
        Vec3::new(r, g, b)
    };
    if options.sun_fit {
        // The ways to the sun under which the texels under the open sky are
        // bright where the stored lightmaps are bright. Only the one way is
        // taken from the stored lightmaps, for the area's entry in
        // `data/lights/bake.toml`.
        let open: Vec<usize> = {
            let seen = sun_and_sky(Vec3::Z, 0, SKY_RAYS, 0.0)?;
            (0..count)
                .filter(|&i| points[i].inside && counts[i] && seen[i][1] >= FIT_OPEN)
                .collect()
        };
        let stored: Vec<Vec3> = open.iter().map(|&i| stored_at(&points[i])).collect();
        let mean_stored = stored.iter().map(|c| luma(*c)).sum::<f32>() / open.len().max(1) as f32;
        let mut tried: Vec<(f32, Vec3, Vec3, Vec3)> = Vec::new();
        for elevation in (10..=80).step_by(10) {
            for turn in (0..360).step_by(15) {
                let (up, around) = ((elevation as f32).to_radians(), (turn as f32).to_radians());
                let toward = Vec3::new(up.cos() * around.cos(), up.cos() * around.sin(), up.sin());
                let seen = sun_and_sky(toward, 1, 0, 0.0)?;
                let mean_seen = open.iter().map(|&i| seen[i][0]).sum::<f32>() / open.len().max(1) as f32;
                let (mut both, mut of_seen, mut of_stored) = (0.0f32, 0.0f32, 0.0f32);
                let (mut lit, mut lit_texels, mut shaded, mut shaded_texels) = (Vec3::ZERO, 0, Vec3::ZERO, 0);
                for (k, &i) in open.iter().enumerate() {
                    let (a, b) = (seen[i][0] - mean_seen, luma(stored[k]) - mean_stored);
                    both += a * b;
                    of_seen += a * a;
                    of_stored += b * b;
                    if seen[i][0] > 0.5 {
                        lit += stored[k];
                        lit_texels += 1;
                    } else if seen[i][0] <= 0.0 {
                        shaded += stored[k];
                        shaded_texels += 1;
                    }
                }
                tried.push((
                    both / (of_seen * of_stored).sqrt().max(1e-9),
                    toward,
                    lit / lit_texels.max(1) as f32,
                    shaded / shaded_texels.max(1) as f32,
                ));
            }
        }
        tried.sort_by(|a, b| b.0.total_cmp(&a.0));
        outln!("  the sun's way over {} texels under the open sky, the best fits first", open.len());
        for (fit, toward, lit, shaded) in tried.iter().take(8) {
            outln!(
                "    toward {:.3} {:.3} {:.3}: fit {fit:.3}, stored colour in the sun {:.3} {:.3} {:.3}, out of it {:.3} {:.3} {:.3}",
                toward.x, toward.y, toward.z, lit.x, lit.y, lit.z, shaded.x, shaded.y, shaded.z
            );
        }
        // What stands between each room's floor and the sky: the meshes a
        // ray straight up from its upward texels meets first. A bright
        // ceiling that the stored lightmaps treat as a skylight shows here.
        let mut over: std::collections::BTreeMap<String, (usize, HashMap<String, usize>, f32)> = Default::default();
        for (number, i) in (0..count).filter(|&i| points[i].inside && counts[i]).enumerate() {
            if number % 16 != 0 || points[i].normal.z < 0.7 {
                continue;
            }
            let entry = over.entry(room_name(&maps[points[i].map])).or_default();
            entry.0 += 1;
            entry.2 += luma(stored_at(&points[i]));
            let met = match level.first_hit(points[i].at, Vec3::Z, 400.0) {
                Some((surface, _)) => names[facts[surface].mesh as usize].clone(),
                None => "the sky".to_string(),
            };
            *entry.1.entry(met).or_default() += 1;
        }
        outln!("  what is over each room's upward texels, with their mean stored level");
        for (room, (texels, met, stored)) in &over {
            let mut met: Vec<(&String, &usize)> = met.iter().collect();
            met.sort_by(|a, b| b.1.cmp(a.1));
            let listed: Vec<String> = (met.iter().take(5))
                .map(|(name, count)| format!("{name} {:.0}%", 100.0 * **count as f32 / *texels as f32))
                .collect();
            outln!("    {room}: {:.3}; {}", stored / *texels as f32, listed.join(", "));
        }
        lap("fitting the sun".into(), &mut times);
    }
    let base: Vec<Vec3> = (points.iter())
        .map(|point| (stored_at(point) / 2.0).powf(GAMMA))
        .collect();
    let added: Vec<Vec3> = (0..count).map(|i| direct_light[i] + bounce_a[i]).collect();
    let shown_live: Vec<Vec3> = (0..count)
        .map(|i| stored_level(base[i] + unshadowed[i]))
        .collect();
    let counted: Vec<usize> = (0..count)
        .filter(|&i| points[i].inside && counts[i])
        .collect();
    let mean = |values: &dyn Fn(usize) -> Vec3| {
        counted.iter().map(|&i| luma(values(i))).sum::<f32>() / counted.len().max(1) as f32
    };
    // What each texel counts for in its room's level (`GpuOptions::weigh`).
    let even_weights = vec![1.0f32; count];
    let lit_weights: Vec<f32> = if options.weigh {
        points[..count].iter().map(|point| luma(stored_at(point))).collect()
    } else {
        even_weights.clone()
    };
    // `a`: each room's light from the lights scaled to the live look's level
    // in that room.
    let rooms_a = room_levels(
        &maps,
        &points,
        &counted,
        &even_weights,
        &base,
        &added,
        &|texels: &[usize]| {
            texels.iter().map(|&i| luma(shown_live[i])).sum::<f32>() / texels.len() as f32
        },
        if options.old { None } else { Some((ROOM_LEAST, ROOM_MOST)) },
        &stored_level,
    );
    let shown_a: Vec<Vec3> = (0..count)
        .map(|i| stored_level(base[i] + added[i] * rooms_a.scale[points[i].map]))
        .collect();
    // `b`: the lamps alone. It reads nothing of the stored lightmaps' texels.
    let lamps_alone: Vec<Vec3> = (0..count).map(|i| direct_light[i] + bounce_b[i]).collect();
    let nothing = vec![Vec3::ZERO; count];
    let (rooms_b, shown_b) = if options.old {
        // The CPU bake's way: one scale for the area, so that its mean is
        // `a`'s.
        let mean_a = mean(&|i| shown_a[i]);
        let (mut low, mut high) = (1e-3f32, 1e3f32);
        for _ in 0..40 {
            let middle = (low * high).sqrt();
            if mean(&|i| stored_level(lamps_alone[i] * middle)) < mean_a {
                low = middle;
            } else {
                high = middle;
            }
        }
        let scale = (low * high).sqrt();
        let lamps_scaled: Vec<Vec3> = lamps_alone.iter().map(|l| *l * scale).collect();
        let rooms = room_levels(
            &maps,
            &points,
            &counted,
            &even_weights,
            &nothing,
            &lamps_scaled,
            &|_| 0.0,
            None,
            &stored_level,
        );
        let shown: Vec<Vec3> = lamps_scaled.iter().map(|l| stored_level(*l)).collect();
        (rooms, shown)
    } else {
        // Each room's target is the live formula without its stored half:
        // the room's mean level of the lights alone, without shadows. One
        // factor for the whole area then raises those targets so that
        // their mean over the texels is `B_MEAN`.
        let without_stored = |texels: &[usize]| {
            texels.iter().map(|&i| luma(stored_level(unshadowed[i]))).sum::<f32>()
                / texels.len() as f32
        };
        let factor = B_MEAN / mean(&|i| stored_level(unshadowed[i])).max(1e-6);
        let stated = read_levels(options.levels.as_deref())?;
        let room_of = |texels: &[usize]| texels.first().map(|&i| room_name(&maps[points[i].map]));
        let rooms = room_levels(
            &maps,
            &points,
            &counted,
            &lit_weights,
            &sun_light,
            &lamps_alone,
            &|texels: &[usize]| {
                room_of(texels)
                    .and_then(|room| stated.get(&room).copied())
                    .unwrap_or_else(|| without_stored(texels) * factor)
            },
            Some((B_LEAST, B_MOST)),
            &eased_level,
        );
        // A room whose lamps do not reach its level at the most scale takes
        // an even light on every texel. Without it a floor far from every
        // lamp is black. The even light alone shows `B_FILL_SHARE` of the
        // room's level, and the lamps are scaled again to give the rest.
        let mut texels_of_room: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
        for &i in &counted {
            texels_of_room
                .entry(room_name(&maps[points[i].map]))
                .or_default()
                .push(i);
        }
        let mut even_of_room: HashMap<String, (f32, f32)> = HashMap::new();
        for room in &rooms.rooms {
            if options.no_even || room.scale < B_MOST || room.after >= room.target {
                continue;
            }
            let (mut low, mut high) = (0.0f32, 1.0f32);
            for _ in 0..30 {
                let middle = 0.5 * (low + high);
                if luma(eased_level(Vec3::splat(middle))) < B_FILL_SHARE * room.target {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            let mut fill = 0.5 * (low + high);
            let texels = &texels_of_room[&room.name];
            let weight = texels.iter().map(|&i| lit_weights[i]).sum::<f32>().max(1e-9);
            let mean_of = |fill: f32, scale: f32| {
                texels
                    .iter()
                    .map(|&i| {
                        lit_weights[i]
                            * luma(eased_level(sun_light[i] + lamps_alone[i] * scale + Vec3::splat(fill)))
                    })
                    .sum::<f32>()
                    / weight
            };
            let (mut low, mut high) = (0.0f32, B_MOST);
            for _ in 0..30 {
                let middle = 0.5 * (low + high);
                if mean_of(fill, middle) < room.target {
                    low = middle;
                } else {
                    high = middle;
                }
            }
            let scale = 0.5 * (low + high);
            // Where the lamps at their most scale still leave the room under
            // its level, the even light gives the rest. Such a room is lit
            // by the sky or by a skylight, which the bake does not have. The
            // owner walked the Dantooine enclave on 18 November 2026 and
            // said: "its too dark in the jedi masters area. i guess the sun
            // isnt hitting it or something? its too dark overall everywhere".
            if mean_of(fill, scale) < room.target - 1e-4 {
                let (mut low, mut high) = (fill, 1.0f32);
                for _ in 0..30 {
                    let middle = 0.5 * (low + high);
                    if mean_of(middle, scale) < room.target {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                fill = 0.5 * (low + high);
            }
            outln!(
                "    {} takes an even light of {fill:.4} (linear) and its lamps x{scale:.3}: its level is {:.4}",
                room.name,
                mean_of(fill, scale)
            );
            even_of_room.insert(room.name.clone(), (fill, scale));
        }
        let even: Vec<Option<(f32, f32)>> = (maps.iter())
            .map(|map| even_of_room.get(&room_name(map)).copied())
            .collect();
        let shown: Vec<Vec3> = (0..count)
            .map(|i| {
                let map = points[i].map;
                let (fill, scale) = even[map].unwrap_or((0.0, rooms.scale[map]));
                eased_level(sun_light[i] + lamps_alone[i] * scale + Vec3::splat(fill))
            })
            .collect();
        (rooms, shown)
    };
    lap("levels".into(), &mut times);

    write_sets(
        resolver,
        &maps,
        &points,
        &counts,
        &unwalked,
        folder,
        &[("a", &shown_a[..], true), ("b", &shown_b[..], false)],
        sunlit.as_deref(),
    )?;
    lap("pictures".into(), &mut times);

    // The report.
    let mean_abs = |a: &[Vec3], b: &[Vec3]| {
        counted
            .iter()
            .map(|&i| (luma(a[i]) - luma(b[i])).abs())
            .sum::<f32>()
            / counted.len().max(1) as f32
    };
    let mut report = String::new();
    let _ = writeln!(
        report,
        "{} from {} fitted lights, on {}",
        area.resref,
        lights.len(),
        gpu.name
    );
    let _ = writeln!(
        report,
        "  {} lightmaps, {} lightmaps of rooms with no walkable face kept at half",
        maps.len(),
        unwalked.len()
    );
    let _ = writeln!(report, "  {} room triangles block rays", surfaces.len());
    if !far.is_empty() {
        let _ = writeln!(
            report,
            "  {} lights reach farther than {FAR_LIGHT} metres and are no lamps; the way to each from the area's middle",
            far.len()
        );
        let (low, high) = bounds(&surfaces);
        for light in far {
            let way = (light.at - (low + high) / 2.0).normalize_or_zero();
            let _ = writeln!(
                report,
                "    toward {:.3} {:.3} {:.3}, colour {:.3} {:.3} {:.3}, reach {:.0}",
                way.x, way.y, way.z, light.colour.x, light.colour.y, light.colour.z, light.radius
            );
        }
    }
    if grass > 0 {
        let _ = writeln!(
            report,
            "  {grass} new texels lie under the grass and on no drawn mesh"
        );
    }
    let _ = writeln!(
        report,
        "  {} new texels on a surface, {} on a triangle and not inside a wall",
        points.len(),
        counted.len()
    );
    if options.old {
        let _ = writeln!(
            report,
            "  the CPU bake's way: one shadow ray a light (five on a strip), {} rays a bounce, one bounce for a and two for b",
            bounce_dirs.len()
        );
    } else {
        let _ = writeln!(
            report,
            "  {SAMPLES} shadow rays a light over its lamp, shadows smoothed over {SMOOTH_RINGS} rings of texels, {BOUNCES} bounces of {BOUNCE_RAYS} rays"
        );
        let _ = writeln!(
            report,
            "  {} of {} lights cast their shadows from their lamp's glass, {} glass triangles cast no shadow on the lights",
            lamps.on_glass,
            lights.len(),
            glass.len()
        );
    }
    let _ = writeln!(
        report,
        "  mean stored level: live {:.4}, a {:.4}, b {:.4}",
        mean(&|i| shown_live[i]),
        mean(&|i| shown_a[i]),
        mean(&|i| shown_b[i])
    );
    let _ = writeln!(
        report,
        "  mean of the lights' added light (linear): without shadows {:.4}, with shadows {:.4}, a's bounces {:.4}, b's bounces and glow {:.4}",
        mean(&|i| unshadowed[i]),
        mean(&|i| direct_light[i]),
        mean(&|i| bounce_a[i]),
        mean(&|i| bounce_b[i])
    );
    let _ = writeln!(
        report,
        "  mean difference live to a {:.4}, live to b {:.4}",
        mean_abs(&shown_live, &shown_a),
        mean_abs(&shown_live, &shown_b)
    );
    let _ = writeln!(
        report,
        "  where the lights' light goes, over {} texels and one ray to each light's nearest place: {:.1}% reaches the texel, {:.1}% is behind the texel's own surface, {:.1}% is blocked by a glowing surface, {:.1}% by another surface",
        losses.texels,
        100.0 * losses.reached,
        100.0 * losses.behind,
        100.0 * losses.glowing,
        100.0 * losses.other
    );
    let _ = writeln!(
        report,
        "  each room: texels, live; a before its scale, a, its scale; b's target, b before its scale, b, its scale"
    );
    for (room, b) in rooms_a.rooms.iter().zip(&rooms_b.rooms) {
        let _ = writeln!(
            report,
            "    {}: {}, live {:.4}; a {:.4} {:.4} x{:.3}; b {:.4} {:.4} {:.4} x{:.3}",
            room.name,
            room.texels,
            room.target,
            room.before,
            room.after,
            room.scale,
            b.target,
            b.before,
            b.after,
            b.scale
        );
    }
    // Each room's level in the game today: the mean of its stored
    // lightmaps over the same texels. Only this one number for each room is
    // reported, for a levels file to aim at. No texel of `b` reads it.
    // `lit` is the level where the game lights the room: the same mean
    // with each texel counted by its own stored level (`GpuOptions::weigh`).
    let mut original: std::collections::BTreeMap<String, (Vec3, usize, f32, f32)> = Default::default();
    for &i in &counted {
        let room = (original.entry(room_name(&maps[points[i].map]))).or_insert((Vec3::ZERO, 0, 0.0, 0.0));
        let stored = stored_at(&points[i]);
        room.0 += stored;
        room.1 += 1;
        room.2 += luma(stored) * luma(stored);
        room.3 += luma(stored);
    }
    let _ = writeln!(
        report,
        "  each room's original level, the mean of its stored lightmaps, their mean colour, and its level where it is lit"
    );
    for (name, (sum, texels, squares, lumas)) in &original {
        let colour = *sum / *texels as f32;
        let _ = writeln!(
            report,
            "    {name}: original {:.4}, colour {:.3} {:.3} {:.3}, lit {:.4}",
            luma(colour),
            colour.x,
            colour.y,
            colour.z,
            squares / lumas.max(1e-9)
        );
    }
    for (name, seconds) in &times {
        let _ = writeln!(report, "  {name}: {seconds:.1} seconds");
    }
    let _ = write!(report, "  {:.1} seconds in all", started.elapsed().as_secs_f64());
    Ok(report)
}

fn luma(c: Vec3) -> f32 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

/// Smooths the share of the lights each texel sees over its neighbours on
/// the same surface, `SMOOTH_RINGS` texels around it in its lightmap. A
/// neighbour counts when it faces the same way and lies on the same plane.
/// The light without shadows is kept as it is, so a pool of light keeps its
/// shape and only a shadow's edge is softened. `shadowed` and `open` hold
/// the texels first; the triangles' middles after them are left alone.
fn smooth_shadows(
    maps: &[Stored],
    points: &[Point],
    counts: &[bool],
    shadowed: &mut [Vec3],
    open: &[Vec3],
) {
    let mut of_map: Vec<Vec<Option<u32>>> = (maps.iter())
        .map(|map| vec![None; map.width * map.height * SCALE * SCALE])
        .collect();
    for (index, point) in points.iter().enumerate() {
        if counts[index] {
            of_map[point.map][point.texel] = Some(index as u32);
        }
    }
    let share = |i: usize| -> Vec3 {
        let o = open[i];
        let s = shadowed[i];
        Vec3::new(
            if o.x > 1e-6 { s.x / o.x } else { 1.0 },
            if o.y > 1e-6 { s.y / o.y } else { 1.0 },
            if o.z > 1e-6 { s.z / o.z } else { 1.0 },
        )
    };
    let smoothed: Vec<Vec3> = super::parallel(points.len(), |index| {
        let point = &points[index];
        if !counts[index] {
            return shadowed[index];
        }
        let map = &maps[point.map];
        let (w, h) = ((map.width * SCALE) as i64, (map.height * SCALE) as i64);
        let (x, y) = ((point.texel as i64) % w, (point.texel as i64) / w);
        // The plane must agree to within half a texel's side.
        let near = (point.size / SCALE as f32 * 0.5).max(0.01);
        let mut sum = Vec3::ZERO;
        let mut total = 0.0f32;
        for dy in -SMOOTH_RINGS..=SMOOTH_RINGS {
            for dx in -SMOOTH_RINGS..=SMOOTH_RINGS {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let Some(other) = of_map[point.map][(ny * w + nx) as usize] else {
                    continue;
                };
                let other = other as usize;
                let them = &points[other];
                if them.normal.dot(point.normal) < 0.9
                    || (them.at - point.at).dot(point.normal).abs() > near
                {
                    continue;
                }
                let d2 = (dx * dx + dy * dy) as f32;
                let weight = (-d2 / (SMOOTH_RINGS * SMOOTH_RINGS) as f32).exp();
                sum += share(other) * weight;
                total += weight;
            }
        }
        if total <= 0.0 {
            return shadowed[index];
        }
        open[index] * (sum / total)
    });
    shadowed[..points.len()].copy_from_slice(&smoothed);
}

/// `b`'s stored level of a light: `stored_level`, eased above `B_KNEE` so
/// that it never passes `B_KNEE + B_OVER`. A surface beside a lamp takes
/// the lamp's light at its strongest and the room's scale on top, and
/// would show white. Its colour keeps its hue.
fn eased_level(light: Vec3) -> Vec3 {
    let (knee, most) = (B_KNEE, B_OVER);
    let level = stored_level(light);
    let brightness = luma(level);
    if brightness <= knee {
        return level;
    }
    let over = brightness - knee;
    level * ((knee + over * most / (most + over)) / brightness)
}

/// Each room's level, and the scale of each lightmap's light from the
/// lights.
struct RoomLevels {
    rooms: Vec<RoomLevel>,
    /// The scale for each lightmap.
    scale: Vec<f32>,
}

struct RoomLevel {
    name: String,
    texels: usize,
    target: f32,
    before: f32,
    after: f32,
    scale: f32,
}

/// The room a lightmap belongs to: its name before `_lm`.
fn room_name(map: &Stored) -> String {
    match map.name.rfind("_lm") {
        Some(at) => map.name[..at].to_string(),
        None => map.name.clone(),
    }
}

/// The stated levels of `GpuOptions::levels`, by room.
fn read_levels(path: Option<&Path>) -> Result<HashMap<String, f32>, String> {
    let Some(path) = path else {
        return Ok(HashMap::new());
    };
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut levels = HashMap::new();
    for line in text.lines().map(str::trim).filter(|line| !line.is_empty() && !line.starts_with('#')) {
        let mut words = line.split_whitespace();
        let (Some(room), Some(level)) = (words.next(), words.next().and_then(|w| w.parse().ok()))
        else {
            return Err(format!("{}: {line}: give a room and a level", path.display()));
        };
        levels.insert(room.to_ascii_lowercase(), level);
    }
    Ok(levels)
}

/// Scales each room's light from the lights (`added`) so that the room's
/// mean stored level is its `target`, within the `range`. A room is the
/// lightmaps that share its name before `_lm`. With no range, every scale
/// is 1 and only the levels are reported.
#[allow(clippy::too_many_arguments)]
fn room_levels(
    maps: &[Stored],
    points: &[Point],
    counted: &[usize],
    weights: &[f32],
    base: &[Vec3],
    added: &[Vec3],
    target: &dyn Fn(&[usize]) -> f32,
    range: Option<(f32, f32)>,
    level: &dyn Fn(Vec3) -> Vec3,
) -> RoomLevels {
    let mut by_room: std::collections::BTreeMap<String, Vec<usize>> = Default::default();
    for &i in counted {
        by_room
            .entry(room_name(&maps[points[i].map]))
            .or_default()
            .push(i);
    }
    let mut scale_of_room: HashMap<String, f32> = HashMap::new();
    let mut rooms = Vec::new();
    for (name, texels) in by_room {
        let weight = texels.iter().map(|&i| weights[i]).sum::<f32>().max(1e-9);
        let mean = |k: f32| {
            texels
                .iter()
                .map(|&i| weights[i] * luma(level(base[i] + added[i] * k)))
                .sum::<f32>()
                / weight
        };
        let live_mean = target(&texels);
        let scale = if let Some((least, most)) = range {
            let (mut low, mut high) = (least, most);
            if mean(high) <= live_mean {
                high
            } else if mean(low) >= live_mean {
                low
            } else {
                for _ in 0..30 {
                    let middle = 0.5 * (low + high);
                    if mean(middle) < live_mean {
                        low = middle;
                    } else {
                        high = middle;
                    }
                }
                0.5 * (low + high)
            }
        } else {
            1.0
        };
        rooms.push(RoomLevel {
            name: name.clone(),
            texels: texels.len(),
            target: live_mean,
            before: mean(1.0),
            after: mean(scale),
            scale,
        });
        scale_of_room.insert(name, scale);
    }
    let scale = (maps.iter())
        .map(|map| scale_of_room.get(&room_name(map)).copied().unwrap_or(1.0))
        .collect();
    RoomLevels { rooms, scale }
}

/// Prints, for the texels within `radius` of `at`, what each light gives
/// them and whether a ray from the texel reaches the light's nearest place,
/// with the mesh that blocks it.
#[allow(clippy::too_many_arguments)]
fn probe(
    maps: &[Stored],
    facts: &[super::Facts],
    names: &[String],
    level: &Level,
    points: &[Point],
    lights: &[Fitted],
    at: Vec3,
    radius: f32,
) {
    let mut near: Vec<(f32, usize)> = (points.iter().enumerate())
        .filter(|(_, point)| point.inside)
        .map(|(i, point)| (point.at.distance(at), i))
        .filter(|(d, _)| *d <= radius)
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    outln!("probe at {at} within {radius} m: {} texels", near.len());
    // What the six rays along the axes from the probe's place meet, and
    // where the texel that place reads in its lightmap lies. A texel that
    // lies far from where the ray met its surface is shared by two places
    // of the lightmap's layout.
    let claimed: HashMap<(usize, usize), usize> = (points.iter().enumerate())
        .map(|(i, point)| ((point.map, point.texel), i))
        .collect();
    for direction in [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z] {
        let Some((surface, t, u, v)) = level.first_hit_at(at, direction, 5.0) else {
            continue;
        };
        let fact = &facts[surface];
        let hit = at + direction * t;
        let read = fact.map.map(|map| {
            let stored = &maps[map];
            let uv = fact.uv[0] * (1.0 - u - v) + fact.uv[1] * u + fact.uv[2] * v;
            let (w, h) = ((stored.width * SCALE) as i64, (stored.height * SCALE) as i64);
            let x = ((uv.x * w as f32).floor() as i64).rem_euclid(w);
            let y = ((uv.y * h as f32).floor() as i64).rem_euclid(h);
            (map, (y * w + x) as usize)
        });
        let owner = read.and_then(|key| claimed.get(&key));
        outln!(
            "  ray {direction} meets {} at {hit:.3} ({t:.2} m), lightmap {}, texel {}, whose place is {}",
            names[fact.mesh as usize],
            read.map_or("none".to_string(), |(map, _)| maps[map].name.clone()),
            read.map_or(0, |(_, texel)| texel),
            owner.map_or("nowhere".to_string(), |&i| format!(
                "{:.3}, {:.2} m from the hit",
                points[i].at,
                points[i].at.distance(hit)
            )),
        );
    }
    // One line for each texel: what the lights give it, and how much of that
    // reaches it past the rooms' surfaces.
    for &(_, i) in &near {
        let point = &points[i];
        let (mut given, mut reached) = (0.0f32, 0.0f32);
        for light in lights {
            let gives = luma(light.gives(point.at, point.shading));
            if gives < 1e-4 {
                continue;
            }
            given += gives;
            let way = light.nearest(point.at) - point.at;
            let far = way.length();
            if far <= SHORT + LIFT || level.first_hit(point.at, way / far, far - SHORT).is_none() {
                reached += gives;
            }
        }
        let mesh = level
            .first_hit(point.at, -point.normal, 0.1)
            .map_or("?", |(surface, _)| names[facts[surface].mesh as usize].as_str());
        outln!(
            "  texel {} of {} at {:.3} normal {:.2} on {mesh}: gives {given:.3}, reaches {reached:.3}",
            point.texel,
            maps[point.map].name,
            point.at,
            point.normal,
        );
    }
    // Every few texels in full, so the list spreads over the whole radius.
    let step = (near.len() / 16).max(1);
    for &(_, i) in near.iter().step_by(step).take(16) {
        let point = &points[i];
        outln!(
            "texel {} of {} at {:.3} normal {:.2} shading {:?}",
            point.texel,
            maps[point.map].name,
            point.at,
            point.normal,
            point.shading.map(|n| format!("{n:.2}")),
        );
        // The mesh under the texel: the nearest triangle straight behind it.
        if let Some((surface, t)) = level.first_hit(point.at, -point.normal, 0.1) {
            outln!("  on {} ({t:.3} m behind)", names[facts[surface].mesh as usize]);
        }
        for (index, light) in lights.iter().enumerate() {
            let given = light.gives(point.at, point.shading);
            if given.max_element() < 1e-3 {
                continue;
            }
            let target = light.nearest(point.at);
            let way = target - point.at;
            let far = way.length();
            let facing = point.normal.dot(way / far.max(1e-4));
            let blocker = level.first_hit(point.at, way / far.max(1e-4), far);
            outln!(
                "  light {index} at {:.2} colour {:.3} radius {} half {:.2} aim {:.2} cone {}: gives {:.4}, {far:.2} m away, normal against it {facing:.2}, {}",
                light.at,
                light.colour,
                light.radius,
                light.half,
                light.aim,
                light.cone,
                luma(given),
                match blocker {
                    Some((surface, t)) => format!(
                        "blocked at {t:.2} m by {}{}",
                        names[facts[surface].mesh as usize],
                        if t > far - SHORT { " (within the last 0.35 m, ignored)" } else { "" }
                    ),
                    None => "reached".into(),
                }
            );
        }
    }
}

/// Where the lights' light goes on a sample of the texels, as shares of
/// the light they give without shadows.
struct Losses {
    texels: usize,
    reached: f32,
    /// The light comes from behind the texel's own surface.
    behind: f32,
    /// A glowing surface, a lamp's glass, blocks the ray.
    glowing: f32,
    /// Any other surface blocks the ray.
    other: f32,
}

/// Measures `Losses` on every 64th texel, on the CPU, with one ray to each
/// light's nearest place as the CPU bake casts it.
fn losses(facts: &[super::Facts], level: &Level, points: &[Point], grid: &LightGrid) -> Losses {
    let sample: Vec<usize> = (0..points.len()).step_by(64).filter(|&i| points[i].inside).collect();
    let each: Vec<[f32; 4]> = super::parallel(sample.len(), |k| {
        let point = &points[sample[k]];
        let cell = (point.at / LIGHT_CELL).floor().as_ivec3();
        let mut shares = [0.0f32; 4];
        for &index in grid.cells.get(&cell).into_iter().flatten() {
            let light = &grid.lights[index as usize];
            let given = luma(light.gives(point.at, point.shading));
            if given < 1e-4 {
                continue;
            }
            let way = light.nearest(point.at) - point.at;
            let far = way.length();
            let toward = way / far.max(1e-4);
            let slot = if point.normal.dot(toward) <= 0.0 {
                1
            } else if far <= SHORT + LIFT {
                0
            } else {
                match level.first_hit(point.at, toward, far - SHORT) {
                    None => 0,
                    Some((surface, _)) if facts[surface].glow.max_element() > 0.0 => 2,
                    Some(_) => 3,
                }
            };
            shares[slot] += given;
        }
        shares
    });
    let total: [f32; 4] = each.iter().fold([0.0; 4], |sum, s| std::array::from_fn(|k| sum[k] + s[k]));
    let all = total.iter().sum::<f32>().max(1e-9);
    Losses {
        texels: sample.len(),
        reached: total[0] / all,
        behind: total[1] / all,
        glowing: total[2] / all,
        other: total[3] / all,
    }
}

/// The places on each light's lamp that its shadow rays aim at, and which
/// triangles are a lamp's glass.
struct Lamps {
    /// `SAMPLES` places for each light, with how far short of each a shadow
    /// ray stops in `w`.
    points: Vec<[f32; 4]>,
    /// Whether each triangle is the glass of a lamp. The glass glows, and
    /// it casts no shadow on the lights: it is where their light comes from.
    glass: Vec<bool>,
    /// How many lights have glass.
    on_glass: usize,
}

/// A small generator of random numbers from 0 to 1, the same on every run.
struct Random(u64);

impl Random {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 40) as f32) / (1u64 << 24) as f32
    }
}

/// The distance from `p` to the segment from `a` to `b`.
fn to_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() > 1e-12 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance(a + ab * t)
}

/// Finds each light's lamp and the places on it. A light's glass is the
/// glowing mesh nearest its lamp, within `LAMP_NEAR`: those of its triangles
/// whose corners all lie within `LAMP_REACH` of the lamp.
/// The places are spread over the glass by area, each lifted toward the
/// light. A light with no glass takes places on a strip along its length,
/// on a disc across the way it shines, or in a ball, all `LAMP` across, and
/// its rays stop `SHORT` short of them as the CPU bake's do.
fn lamps(
    lights: &[Fitted],
    surfaces: &[crate::light_probes::Surface],
    facts: &[super::Facts],
) -> Lamps {
    let glowing: Vec<usize> = (0..facts.len())
        .filter(|&t| facts[t].glow.max_element() > 0.0)
        .collect();
    let each: Vec<(Vec<[f32; 4]>, Vec<usize>)> = super::parallel(lights.len(), |index| {
        let light = &lights[index];
        let back = light.aim * OFF;
        let (a, b) = (
            light.at - light.way * light.half - back,
            light.at + light.way * light.half - back,
        );
        // The glowing triangle nearest the lamp, and the triangles of its
        // mesh near the lamp. A glowing strip that runs past the lamp is
        // another mesh, so it is left out.
        let gap = |t: usize| {
            let [p, q, r] = surfaces[t].corners;
            [p, q, r, (p + q + r) / 3.0]
                .iter()
                .map(|corner| to_segment(*corner, a, b))
                .fold(f32::MAX, f32::min)
        };
        let nearest = (glowing.iter().copied())
            .map(|t| (gap(t), t))
            .filter(|(d, _)| *d <= LAMP_NEAR)
            .min_by(|x, y| x.0.total_cmp(&y.0));
        let mine: Vec<usize> = match nearest {
            None => Vec::new(),
            Some((_, first)) => (glowing.iter().copied())
                .filter(|&t| {
                    facts[t].mesh == facts[first].mesh
                        && (surfaces[t].corners.iter())
                            .all(|corner| to_segment(*corner, a, b) <= LAMP_REACH)
                })
                .collect(),
        };
        let mut random = Random(index as u64 * 0x9e37_79b9 + 17);
        let count = SAMPLES as usize;
        let mut places = Vec::with_capacity(count);
        if !mine.is_empty() {
            let areas: Vec<f32> = (mine.iter())
                .map(|&t| {
                    let [p, q, r] = surfaces[t].corners;
                    0.5 * (q - p).cross(r - p).length()
                })
                .collect();
            let total: f32 = areas.iter().sum();
            for k in 0..count {
                // One place in each of `count` even parts of the glass's area.
                let pick = (k as f32 + random.next()) / count as f32 * total;
                let mut sum = 0.0;
                let mut chosen = mine.len() - 1;
                for (n, area) in areas.iter().enumerate() {
                    sum += area;
                    if pick <= sum {
                        chosen = n;
                        break;
                    }
                }
                let t = mine[chosen];
                let [p, q, r] = surfaces[t].corners;
                let (mut u, mut v) = (random.next(), random.next());
                if u + v > 1.0 {
                    (u, v) = (1.0 - u, 1.0 - v);
                }
                let place = p + (q - p) * u + (r - p) * v;
                let normal = facts[t].normal;
                let toward = if normal.dot(light.at - place) >= 0.0 {
                    normal
                } else {
                    -normal
                };
                places.push(four(place + toward * GLASS_LIFT, GLASS_SHORT));
            }
        } else {
            for k in 0..count {
                let along = (k as f32 + random.next()) / count as f32;
                let turn = std::f32::consts::TAU * random.next();
                let place = if light.half > 0.0 {
                    let (side, up) = super::across(light.way);
                    let off = 0.35 * LAMP * random.next().sqrt();
                    light.at
                        + light.way * ((2.0 * along - 1.0) * light.half)
                        + (side * turn.cos() + up * turn.sin()) * off
                } else if light.aim.length_squared() > 0.5 {
                    let (side, up) = super::across(light.aim);
                    light.at + (side * turn.cos() + up * turn.sin()) * (LAMP * along.sqrt())
                } else {
                    let z = 2.0 * along - 1.0;
                    let ring = (1.0 - z * z).max(0.0).sqrt();
                    light.at
                        + Vec3::new(ring * turn.cos(), ring * turn.sin(), z)
                            * (LAMP * random.next().cbrt())
                };
                places.push(four(place, SHORT));
            }
        }
        (places, mine)
    });
    let mut glass = vec![false; facts.len()];
    let mut points = Vec::with_capacity(lights.len() * SAMPLES as usize);
    let mut on_glass = 0;
    for (places, mine) in each {
        points.extend(places);
        on_glass += usize::from(!mine.is_empty());
        for t in mine {
            glass[t] = true;
        }
    }
    Lamps {
        points,
        glass,
        on_glass,
    }
}

/// The values of the texels, each the mean of its neighbours on the same
/// surface within `BOUNCE_RINGS` texels in its lightmap, weighted to fall
/// off with distance.
fn smooth_values(maps: &[Stored], points: &[Point], counts: &[bool], values: &[Vec3]) -> Vec<Vec3> {
    let mut of_map: Vec<Vec<Option<u32>>> = (maps.iter())
        .map(|map| vec![None; map.width * map.height * SCALE * SCALE])
        .collect();
    for (index, point) in points.iter().enumerate() {
        if counts[index] {
            of_map[point.map][point.texel] = Some(index as u32);
        }
    }
    super::parallel(points.len(), |index| {
        let point = &points[index];
        if !counts[index] {
            return values[index];
        }
        let map = &maps[point.map];
        let (w, h) = ((map.width * SCALE) as i64, (map.height * SCALE) as i64);
        let (x, y) = ((point.texel as i64) % w, (point.texel as i64) / w);
        let near = (point.size / SCALE as f32 * 0.5).max(0.01);
        let mut sum = Vec3::ZERO;
        let mut total = 0.0f32;
        for dy in -BOUNCE_RINGS..=BOUNCE_RINGS {
            for dx in -BOUNCE_RINGS..=BOUNCE_RINGS {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let Some(other) = of_map[point.map][(ny * w + nx) as usize] else {
                    continue;
                };
                let them = &points[other as usize];
                if them.normal.dot(point.normal) < 0.9
                    || (them.at - point.at).dot(point.normal).abs() > near
                {
                    continue;
                }
                let d2 = (dx * dx + dy * dy) as f32;
                let weight = (-d2 / (BOUNCE_RINGS * BOUNCE_RINGS) as f32).exp();
                sum += values[other as usize] * weight;
                total += weight;
            }
        }
        if total > 0.0 { sum / total } else { values[index] }
    })
}
