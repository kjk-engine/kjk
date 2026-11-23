//! The bake from the fitted lights on the graphics card (`kjk detail-bake
//! --lights <file> --gpu`). It opens its own Vulkan or Metal device with wgpu's ray
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
    FAR_LIGHT, Field, Fitted, GAIN, GAMMA, LIFT, LIGHT_CELL, LightGrid, Point, SHORT, Still, Stored, Texels,
    WALL_RAYS, bounds, direct, directions, field_of, points, still_placeables, stored_level, texels,
    write_sets,
};
use crate::light_probes::Level;
use crate::room_lights::Reader;

/// How long the bake waits for the graphics card to finish one submission
/// before it fails. The owner's Windows card does a whole bake in about 13
/// seconds in all, and one submission of `CHUNK` places takes well under a
/// second, so a wait of 30 seconds means the card is stuck.
const WAIT_LIMIT: std::time::Duration = std::time::Duration::from_secs(30);

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
/// mean on the Endar Spire, measured on 18 November 2026. It is one fixed
/// number, not read from the stored lightmaps.
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
    /// The light a room takes where its lamps cannot reach its level is the
    /// room's own stored light, scaled, and not an even one. A room with no
    /// lamp that the level art shows was lit by lamps the games never
    /// shipped, and its stored lightmaps are all that is left of them. The
    /// even light made such rooms flat and grey, and no light left them
    /// black. The owner said of the Korriban tomb on 20 November 2026: "it
    /// seems to be some light they have baked in there from the ceiling",
    /// "We should make that one stronger and work with that".
    pub fill_stored: bool,
    /// Each texel of `b` shows at least this share of its stored linear
    /// light, as `floor.txt` asks of the game. A room may reach its level by
    /// its bright lightmaps alone, and a wall far from every lamp stayed
    /// black: the hall of stunt_18 showed a mean of 4 where the game shows
    /// 47 (22 November 2026). The owner asked for such a wall to be lit, on
    /// the darker side of the original. A module asks for it in
    /// `data/lights/bake.toml` with `floor`.
    pub floor: Option<f32>,
    /// Every room that has a lightmapped mesh is baked, as under a sun,
    /// not only the rooms with a walkable face. Manaan's sea floor lies in
    /// rooms no one walks on, and the pole lamps by the walkway lit none of
    /// it (21 November 2026).
    pub every_room: bool,
    /// A room with no walkable face shows its stored light in `b`, and not
    /// twice it. Taris's buildings beside a walkway glared at twice their
    /// light (owner, 21 November 2026).
    pub stored_backdrop: bool,
    /// The least a room's lamps are scaled by in `b`, where it is not
    /// `B_LEAST`. A ship's dark rooms have lamps that are too bright at
    /// half their strength.
    pub least: Option<f32>,
    /// The most a room's lamps are scaled by in `b`, where it is not
    /// `B_MOST`. A room's level counts its bright texels most, and where
    /// its lamps do not reach those, the most scale lights the rest of the
    /// room far above today. The Ebon Hawk's hold came out at 70 against
    /// today's 19 so.
    pub most: Option<f32>,
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
    /// The placeables' meshes that have no skin and do not dangle
    /// (`kjk_game::placeable_light::bends`) block the rays where they rest.
    /// A placeable that holds a fitted light does not. Without this option
    /// the bake is the same as before to the last pixel.
    pub placeables: bool,
    /// Each vertex of each such mesh is a place of the passes, lit as a
    /// texel is, and set `b` gets `placeables.txt` (`write_placeables`).
    pub placeable_light: bool,
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
            backends: wgpu::Backends::VULKAN | wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: None,
        }))
        .map_err(|error| format!("no Vulkan or Metal graphics card: {error}"))?;
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

    /// Waits for the graphics card to finish `step` and fails with the first
    /// error wgpu reported. After `WAIT_LIMIT` the bake prints why and ends
    /// the process at once. wgpu panics when it drops a device whose work
    /// never finished, and that panic would hide the message.
    fn wait(&self, step: &str) -> Result<(), String> {
        let started = Instant::now();
        match self.device.poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(WAIT_LIMIT),
        }) {
            Ok(_) => {}
            Err(wgpu::PollError::Timeout) => {
                eprintln!(
                    "detail-bake: the graphics card did not finish {step} in {} seconds. A shader that never ends, \
                     or a command buffer that the card stopped, keeps wgpu waiting for ever on Metal.",
                    WAIT_LIMIT.as_secs()
                );
                std::process::exit(1);
            }
            Err(error) => return Err(format!("waiting for the graphics card to finish {step}: {error}")),
        }
        if std::env::var_os("KJK_BAKE_LAPS").is_some() {
            eprintln!("  the graphics card finished {step} in {:.2} s", started.elapsed().as_secs_f32());
        }
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
        self.wait("reading the results back")?;
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

fn mode_name(mode: u32) -> &'static str {
    match mode {
        MODE_DIRECT_OLD => "old direct light",
        MODE_DIRECT_SOFT => "soft direct light",
        MODE_BOUNCE_OLD => "old bounce",
        MODE_BOUNCE_NEW => "bounce",
        MODE_SKY => "sun and sky",
        _ => "unknown",
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

    /// `run_toward` over the places `from` to `from + count`.
    #[allow(clippy::too_many_arguments)]
    fn run_from(
        &self,
        mode: u32,
        (from, count): (usize, usize),
        samples: u32,
        rays: u32,
        seed: u32,
        toward: Vec3,
        width: f32,
    ) -> Result<(), String> {
        for first in (from..from + count).step_by(CHUNK) {
            let size = CHUNK.min(from + count - first);
            self.dispatch(mode, (first, size, from + count), (samples, rays, seed), toward, width)?;
        }
        Ok(())
    }

    /// `run`, with the way to the sun for `MODE_SKY` and half the sun's
    /// width. A width of 0 is the shader's own, with every ray from the
    /// texel's middle.
    #[allow(clippy::too_many_arguments)]
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
            self.dispatch(mode, (first, size, count), (samples, rays, seed), toward, width)?;
        }
        Ok(())
    }

    /// One chunk of a pass: `size` places from `first`, of the `end` the
    /// pass works on.
    fn dispatch(
        &self,
        mode: u32,
        (first, size, end): (usize, usize, usize),
        (samples, rays, seed): (u32, u32, u32),
        toward: Vec3,
        width: f32,
    ) -> Result<(), String> {
        {
            let count = end;
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
            self.gpu.wait(&format!(
                "the {} pass over places {first} to {} of {count}",
                mode_name(mode),
                first + size
            ))?;
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
    (reader, resolver, game): (&mut Reader, &Resolver, vfs::Game),
    area: &AreaIr,
    (lights, far): (&[Fitted], &[Fitted]),
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
    // The placeables with a mesh the bake may light. Those that hold a
    // fitted light within `LAMP_CLEAR` of their box block no ray: a lamp's
    // own housing would shut in the light the game fitted to it. These lights
    // are the placeable's own (`own_lights`). Its vertices take them as if
    // they stood no nearer than `OWN_LIGHT_LEAST` (`OWN_LIGHT` in
    // `gpu.wgsl`), and its own meshes shade them (`own_seen`). Without the
    // shade, the light fitted to a terminal's screen in danm13 lit the whole
    // terminal a bright blue, even at a least distance of half a metre.
    // Without its own light at all, the glowing table of danm13 was darker
    // than on the probes (23 November 2026).
    let still: Vec<Still> = if options.placeables {
        still_placeables(reader, resolver, area, game)
    } else {
        Vec::new()
    };
    let own_lights: Vec<Vec<u32>> = (still.iter())
        .map(|still| {
            let corners: Vec<Vec3> = (still.model.ir.meshes.iter()).flat_map(|mesh| still.corners(mesh)).collect();
            let low = corners.iter().fold(Vec3::splat(f32::MAX), |low, p| low.min(*p)) - Vec3::splat(LAMP_CLEAR);
            let high = corners.iter().fold(Vec3::splat(f32::MIN), |high, p| high.max(*p)) + Vec3::splat(LAMP_CLEAR);
            (0..lights.len() as u32)
                .filter(|&k| lights[k as usize].at.cmpge(low).all() && lights[k as usize].at.cmple(high).all())
                .collect()
        })
        .collect();
    let occluders: Vec<&Still> = (still.iter().zip(&own_lights))
        .filter(|(_, own)| own.is_empty())
        .map(|(still, _)| still)
        .collect();
    let Texels {
        maps,
        surfaces,
        facts,
        claims,
        unwalked,
        names,
        grass,
    } = texels(reader, resolver, area, options.sun.is_some() || options.every_room, &occluders)?;
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
            side: point.size / maps[point.map].scale as f32,
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
    // The probes' places, six to a probe, one facing each of `PROBE_AXES`.
    // They follow the triangles' middles, and every pass that lights them
    // runs over them alone (`run_from`).
    let probes = if options.old { Vec::new() } else { probe_places(reader, area) };
    let probe_first = places.len();
    places.extend(probes.iter().flat_map(|(_, at)| {
        PROBE_AXES.map(|axis| Place {
            at: *at,
            normal: axis,
            shading: Some(axis),
            side: 0.0,
        })
    }));
    // The vertices' places, one for each vertex of each mesh the bake may
    // light (`vertex_places`). They follow the probes' places, and every pass
    // that lights the probes lights them too.
    let vertices = if options.placeable_light && !options.old {
        vertex_places(area, reader, &still)
    } else {
        Vertices::default()
    };
    places.extend(vertices.places.iter().map(|(at, normal)| Place {
        at: *at,
        normal: *normal,
        shading: Some(*normal),
        side: 0.0,
    }));
    let probe_range = (probe_first, 6 * probes.len() + vertices.places.len());
    // Where each probe's place and each vertex stands, for the fill it takes
    // from the stored light beside it (`GpuOptions::fill_stored`).
    let probe_at: Vec<Vec3> = places[probe_first..].iter().map(|place| place.at).collect();
    // Each place's lightmap, for its room's scale and how far it is under a
    // roof: six to a probe, then one to a vertex.
    let map_of_room = |room: &str| maps.iter().position(|map| room_name(map).eq_ignore_ascii_case(room));
    let probe_maps: Vec<Option<usize>> = (probes.iter())
        .flat_map(|(room, _)| [map_of_room(room); 6])
        .chain(vertices.rooms.iter().map(|room| room.as_deref().and_then(map_of_room)))
        .collect();

    // The light grid's cells that hold a place, each with its list of lights.
    // Cell 0 is empty. A vertex of a placeable that holds lights of its own
    // takes a cell of its own placeable, whose list marks those lights with
    // `OWN_LIGHT`.
    let vertex_first = probe_first + 6 * probes.len();
    let seen = own_seen(&vertices, &still, &own_lights, lights);
    let mut cell_of: HashMap<(IVec3, usize, u32), u32> = HashMap::new();
    let mut cells: Vec<u32> = vec![0, 0];
    let mut cell_lights: Vec<u32> = Vec::new();
    let place_cells: Vec<u32> = (places.iter().enumerate())
        .map(|(index, place)| {
            let key = (place.at / LIGHT_CELL).floor().as_ivec3();
            let Some(listed) = grid.cells.get(&key) else {
                return 0;
            };
            let vertex = (index.checked_sub(vertex_first)).filter(|&k| !own_lights[vertices.owners[k]].is_empty());
            let owner = vertex.map(|k| vertices.owners[k]);
            let sees = vertex.map_or(0, |k| seen[k]);
            *cell_of.entry((key, owner.unwrap_or(usize::MAX), sees)).or_insert_with(|| {
                let start = cell_lights.len();
                let own: &[u32] = owner.map_or(&[], |owner| &own_lights[owner]);
                for &light in listed {
                    match own.iter().position(|&mine| mine == light) {
                        None => cell_lights.push(light),
                        Some(bit) if bit < 32 && sees & (1 << bit) != 0 => cell_lights.push(light | OWN_LIGHT),
                        Some(_) => {}
                    }
                }
                cells.extend([start as u32, (cell_lights.len() - start) as u32]);
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
        let (w, h) = (map.width * map.scale, map.height * map.scale);
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
    gpu.wait("building the acceleration structure")?;

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
    gpu.wait("making the compute pipeline")?;
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
    let first_counts: Vec<bool> = (0..places.len()).map(|i| first[2 * i][3] > 0.5).collect();
    let mut shadowed: Vec<Vec3> = (0..places.len())
        .map(|i| Vec3::from_slice(&first[2 * i]))
        .collect();
    let open: Vec<Vec3> = (0..places.len())
        .map(|i| Vec3::from_slice(&first[2 * i + 1]))
        .collect();
    drop(first);
    lap("direct light".into(), &mut times);
    // What each texel is, for the pictures of `KJK_BAKE_DEBUG`, before the
    // shadows are smoothed (`debug_pictures`).
    let kinds: Option<Vec<[u8; 3]>> = std::env::var_os("KJK_BAKE_DEBUG").map(|_| {
        (0..count)
            .map(|i| texel_kind(&points[i], counts[i], open[i], shadowed[i]))
            .collect()
    });
    // The probe's texels as the graphics card lit them, and the lamp places
    // of the lights that reach them most.
    if let Some((at, radius)) = options.probe {
        for (i, point) in points.iter().enumerate() {
            if point.inside && point.at.distance(at) <= radius {
                outln!(
                    "  gpu texel {} of {} at {:.3}, normal {:.2}: without shadows {:.3}, with shadows {:.3}, counts {}",
                    point.texel,
                    maps[point.map].name,
                    point.at,
                    point.normal,
                    luma(open[i]),
                    luma(shadowed[i]),
                    counts[i]
                );
            }
        }
    }
    if !options.old {
        smooth_shadows(&maps, &points, &counts, &mut shadowed, &open);
        lap("smoothing the shadows".into(), &mut times);
    }
    let direct_light: Vec<Vec3> = shadowed[..count].iter().map(|sum| direct(*sum)).collect();
    let probe_direct: Vec<Vec3> = (shadowed[probe_first..probe_first + probe_range.1].iter())
        .map(|sum| direct(*sum))
        .collect();
    // Whether each vertex's place counts: a place inside a wall or under the
    // floor takes the light of its neighbours (`fill_vertices`).
    // A vertex under a smaller placeable that rests on its own does not count
    // either (`covered_vertices`).
    let covered = covered_vertices(&vertices, &still, &own_lights);
    let vertex_counts: Vec<bool> = (0..vertices.places.len())
        .map(|k| first_counts[probe_first + 6 * probes.len() + k] && !covered[k])
        .collect();
    let unshadowed: Vec<Vec3> = open[..count].iter().map(|sum| direct(*sum)).collect();
    let tri_direct: Vec<[f32; 4]> = shadowed[count..probe_first]
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
    // The sun's and the sky's light on the probes' places.
    let probe_sun_and_sky = |toward: Vec3, samples: u32, rays: u32, width: f32| -> Result<Vec<[f32; 2]>, String> {
        passes.run_from(MODE_SKY, probe_range, samples, rays, 21, toward, width)?;
        let read = gpu.read(&passes.results)?;
        Ok((0..probe_range.1).map(|j| [read[2 * (probe_first + j)][0], read[2 * (probe_first + j)][1]]).collect())
    };
    let mut probe_sky = vec![Vec3::ZERO; probe_range.1];
    // The first sun's direct light on each probe's place, which `probe_sky`
    // holds, and the sun's way and linear colour. The game adds it live
    // under `KJK_PROBE_SUN=on` (`write_probes_sun`).
    let mut probe_sun = vec![Vec3::ZERO; probe_range.1];
    let mut sun_of_probes: Option<(Vec3, Vec3)> = None;
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
            if probe_range.1 > 0 {
                let seen = probe_sun_and_sky(sun.toward, sun_rays, SKY_RAYS, width)?;
                let second: Vec<Vec3> = match sun.second {
                    Some((toward, colour)) => (probe_sun_and_sky(toward, sun_rays, 0, width)?.iter())
                        .map(|[sun, _]| colour.powf(GAMMA) * *sun)
                        .collect(),
                    None => vec![Vec3::ZERO; probe_range.1],
                };
                for (j, light) in probe_sky.iter_mut().enumerate() {
                    let [sun_seen, sky_seen] = seen[j];
                    let roof = probe_maps[j].map_or(1.0, |map| roofed[map]);
                    let open = ((sky_seen - OPENING_FROM) / (OPENING_TO - OPENING_FROM)).clamp(0.0, 1.0);
                    let open = open * open * (3.0 - 2.0 * open);
                    let in_a_courtyard = (on_skylight * OPENING_GAIN).lerp(of_sky, open) * sky_seen;
                    let under_a_roof = on_skylight * (ROOF_GAIN * sky_seen).min(1.0);
                    *light = of_sun * sun_seen + second[j] + in_a_courtyard.lerp(under_a_roof, roof);
                    probe_sun[j] = of_sun * sun_seen;
                }
                sun_of_probes = Some((sun.toward, of_sun));
            }
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
    // What the probes' places gather from field `b`, and from the glow.
    let probe_gather = |b: &[Vec3], seed: u32| -> Result<Vec<[Vec3; 2]>, String> {
        if probe_range.1 == 0 {
            return Ok(Vec::new());
        }
        let field_b = flat(&field_of(&maps, &points, &counts, b));
        passes.write_fields(&field_b, &field_b);
        passes.run_from(MODE_BOUNCE_NEW, probe_range, 0, BOUNCE_RAYS, seed, Vec3::Z, 0.0)?;
        let read = gpu.read(&passes.results)?;
        Ok((0..probe_range.1)
            .map(|j| [1, 2].map(|k| Vec3::from_slice(&read[3 * (probe_first + j) + k])))
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
    // The lamps' light on the probes: direct, and what the texels lit by
    // them and their bounces send back.
    let probe_lamps: Vec<Vec3> = {
        let from_b: Vec<Vec3> = (0..count).map(|i| direct_light[i] + bounce_b[i]).collect();
        let gathered = probe_gather(&from_b, 40)?;
        (0..probe_range.1)
            .map(|j| probe_direct[j] + gathered.get(j).map_or(Vec3::ZERO, |[b, glow]| *b + *glow))
            .collect()
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
        let from: Vec<Vec3> = (0..count).map(|i| sky_light[i] + bounced[i]).collect();
        // The glow was gathered with the lamps' light already.
        let gathered = probe_gather(&from, 42)?;
        for (light, [b, _]) in probe_sky.iter_mut().zip(gathered) {
            *light += b;
        }
        from
    } else {
        sky_light
    };

    // Each texel's stored lightmap, read between texels as the GPU reads it.
    let stored_at = |point: &Point| {
        let map = &maps[point.map];
        let w = map.width * map.scale;
        let uv = bevy::math::Vec2::new(
            ((point.texel % w) as f32 + 0.5) / w as f32,
            ((point.texel / w) as f32 + 0.5) / (map.height * map.scale) as f32,
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
    // `b`: the lamps alone. No texel of the stored lightmaps is copied into
    // it. Each room's scale still takes one number from them: the room's
    // target level comes from its stored level, and with `--weigh` each
    // texel counts in that level by its stored brightness (`lit_weights`).
    let lamps_alone: Vec<Vec3> = (0..count).map(|i| direct_light[i] + bounce_b[i]).collect();
    let nothing = vec![Vec3::ZERO; count];
    // `written_b` is what set `b` holds on disk, and `fills_b` is how much
    // of its stored light each room takes when the game loads it
    // (`GpuOptions::fill_stored`).
    //
    // `probe_light` and `probe_linear` are the probes' and the vertices'
    // light as the files hold it, after and before the curve. Where a place
    // takes its fill from the stored light, the files hold its light without
    // that fill, and `probe_stored` names the stored texel the game reads the
    // fill from when it loads the file.
    let (rooms_b, shown_b, written_b, fills_b, probe_light, probe_linear, probe_stored) = if options.old {
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
        (rooms, shown.clone(), shown, std::collections::BTreeMap::new(), Vec::new(), Vec::new(), Vec::new())
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
        let b_most = options.most.unwrap_or(B_MOST);
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
            Some((options.least.unwrap_or(B_LEAST), b_most)),
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
        // What the fill shows at each texel for a fill of 1: the same
        // everywhere, or the room's stored light with a mean of 1 over the
        // room (`GpuOptions::fill_stored`).
        let mut pattern = vec![Vec3::ONE; count];
        let mut pattern_mean: HashMap<String, f32> = HashMap::new();
        if options.fill_stored {
            for (room, texels) in &texels_of_room {
                let stored: Vec<Vec3> = (texels.iter())
                    .map(|&i| stored_at(&points[i]).powf(GAMMA))
                    .collect();
                let mean = stored.iter().map(|s| luma(*s)).sum::<f32>() / texels.len() as f32;
                if mean > 1e-6 {
                    pattern_mean.insert(room.clone(), mean);
                    for (&i, s) in texels.iter().zip(&stored) {
                        pattern[i] = *s / mean;
                    }
                }
            }
        }
        let fill_most = if options.fill_stored { 4.0f32 } else { 1.0 };
        let mut even_of_room: HashMap<String, (f32, f32)> = HashMap::new();
        for room in &rooms.rooms {
            if options.no_even || room.scale < b_most || room.after >= room.target {
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
                            * luma(eased_level(sun_light[i] + lamps_alone[i] * scale + pattern[i] * fill))
                    })
                    .sum::<f32>()
                    / weight
            };
            let (mut low, mut high) = (0.0f32, b_most);
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
                let (mut low, mut high) = (fill, fill_most);
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
                eased_level(sun_light[i] + lamps_alone[i] * scale + pattern[i] * fill)
            })
            .collect();
        // The files hold nothing of the stored lightmaps. A room's fill
        // from its stored light is one number in `fill.txt`, and the game
        // adds the stored light of the player's own files when it loads
        // the lightmap (`detail_baked` in `crates/game/src/viewer.rs`).
        // The owner asked on 21 November 2026: "do we need to hold bioware
        // lightmap pixels? or could we load them from the game files
        // instead".
        let mut fills: std::collections::BTreeMap<String, f32> = Default::default();
        for (room, (fill, _)) in &even_of_room {
            if let Some(mean) = pattern_mean.get(room) {
                fills.insert(room.clone(), fill / mean);
            }
        }
        let written: Vec<Vec3> = (0..count)
            .map(|i| {
                let map = points[i].map;
                if fills.contains_key(&room_name(&maps[map])) {
                    let scale = even[map].map_or(rooms.scale[map], |(_, scale)| scale);
                    eased_level(sun_light[i] + lamps_alone[i] * scale)
                } else {
                    shown[i]
                }
            })
            .collect();
        // What the fill shows at each probe's place and each vertex: the
        // pattern of the room's texel nearest it. The fill of a room from its
        // stored light is the room's stored light times a number, so a texel
        // in a dark corner takes less of it than one under a lamp. A vertex
        // took the fill at the room's mean before, and in 411dxn the foot
        // locker and the table were 4.01 and 3.48 of their floor, against
        // 2.45 and 1.98 today.
        let nearest = nearest_texels(&maps, &points, &counted, &probe_at, &probe_maps);
        let probe_pattern = places_pattern(&pattern, &nearest, options.fill_stored);
        // Each probe's light as its room shows it: the sun and the sky, the
        // lamps at the room's scale, and the room's even light. The linear
        // light before the curve is kept for the live sun
        // (`write_probes_sun`).
        let probe_linear: Vec<Vec3> = (0..probe_range.1)
            .map(|j| {
                let (fill, scale) = probe_maps[j].map_or((0.0, 1.0), |map| even[map].unwrap_or((0.0, rooms.scale[map])));
                probe_sky[j] + probe_lamps[j] * scale + probe_pattern[j] * fill
            })
            .collect();
        let probe_light: Vec<Vec3> = probe_linear.iter().map(|light| eased_level(*light)).collect();
        // `KJK_VERTEX_PARTS=<placeable>`: each part of the light on that
        // placeable's vertices, by the way they face, against the texels
        // around it that face the same way.
        if let Some(name) = std::env::var_os("KJK_VERTEX_PARTS").and_then(|name| name.into_string().ok()) {
            let axis_of = |n: Vec3| -> usize {
                let a = n.abs();
                if a.z >= a.x && a.z >= a.y { if n.z > 0.0 { 4 } else { 5 } } else if a.x >= a.y { if n.x > 0.0 { 0 } else { 1 } } else if n.y > 0.0 { 2 } else { 3 }
            };
            let axes = ["+x", "-x", "+y", "-y", "+z", "-z"];
            let v3 = |v: Vec3| format!("{:.4} {:.4} {:.4}", v.x, v.y, v.z);
            for (owner, placeable) in still.iter().enumerate().filter(|(_, s)| name.split(',').any(|n| n == s.name)) {
                let mine: Vec<usize> = (0..vertices.places.len()).filter(|&k| vertices.owners[k] == owner && vertex_counts[k]).collect();
                let low = mine.iter().fold(Vec3::splat(f32::MAX), |low, &k| low.min(vertices.places[k].0));
                let high = mine.iter().fold(Vec3::splat(f32::MIN), |high, &k| high.max(vertices.places[k].0));
                outln!("  parts of {} at {} (the vertices that count): box {} to {}", placeable.name, v3(placeable.at), v3(low), v3(high));
                #[allow(clippy::needless_range_loop)]
                for axis in 0..6 {
                    let group: Vec<usize> = mine.iter().copied().filter(|&k| axis_of(vertices.places[k].1) == axis).collect();
                    if group.is_empty() {
                        continue;
                    }
                    let n = group.len() as f32;
                    let sum = |f: &dyn Fn(usize) -> Vec3| group.iter().map(|&k| f(6 * probes.len() + k)).sum::<Vec3>() / n;
                    let (fill_scale, room) = (probe_maps[6 * probes.len() + group[0]])
                        .map_or(((0.0, 1.0), String::new()), |map| (even[map].unwrap_or((0.0, rooms.scale[map])), room_name(&maps[map])));
                    outln!(
                        "    vertices {} ({}), room {room}, fill {:.4}, scale {:.3}: direct {}, bounce {}, sky {}, fill part {}, linear {}, after the curve {}",
                        axes[axis], group.len(), fill_scale.0, fill_scale.1,
                        v3(sum(&|j| probe_direct[j] * fill_scale.1)), v3(sum(&|j| (probe_lamps[j] - probe_direct[j]) * fill_scale.1)), v3(sum(&|j| probe_sky[j])),
                        v3(sum(&|j| probe_pattern[j] * fill_scale.0)), v3(sum(&|j| probe_linear[j])), v3(sum(&|j| probe_light[j]))
                    );
                    for &k in group.iter().step_by((group.len() / 4).max(1)).take(4) {
                        let j = 6 * probes.len() + k;
                        let texel = nearest[j].map_or("none".to_string(), |i| {
                            format!("{} at {} facing {}, {:.2} m off, pattern {}", maps[points[i].map].name, v3(points[i].at), v3(points[i].normal), points[i].at.distance(vertices.places[k].0), v3(pattern[i]))
                        });
                        outln!(
                            "      vertex at {}: direct {}, bounce {}, fill part {}, after the curve {}; nearest texel {texel}",
                            v3(vertices.places[k].0), v3(probe_direct[j] * fill_scale.1), v3((probe_lamps[j] - probe_direct[j]) * fill_scale.1), v3(probe_pattern[j] * fill_scale.0), v3(probe_light[j])
                        );
                    }
                }
                // The counted texels within a metre and a half of its box, by the way they face.
                let near: Vec<usize> = (counted.iter().copied())
                    .filter(|&i| points[i].at.clamp(low, high).distance(points[i].at) < 1.5)
                    .collect();
                #[allow(clippy::needless_range_loop)]
                for axis in 0..6 {
                    let group: Vec<usize> = near.iter().copied().filter(|&i| axis_of(points[i].normal) == axis).collect();
                    if group.is_empty() {
                        continue;
                    }
                    let n = group.len() as f32;
                    let sum = |f: &dyn Fn(usize) -> Vec3| group.iter().map(|&i| f(i)).sum::<Vec3>() / n;
                    let mut by_room: HashMap<String, usize> = HashMap::new();
                    for &i in &group {
                        *by_room.entry(room_name(&maps[points[i].map])).or_default() += 1;
                    }
                    let fs = |i: usize| even[points[i].map].unwrap_or((0.0, rooms.scale[points[i].map]));
                    outln!(
                        "    texels {} ({}, rooms {:?}): direct {}, bounce {}, sky {}, fill part {}, after the curve {}, stored {}",
                        axes[axis], group.len(), by_room,
                        v3(sum(&|i| direct_light[i] * fs(i).1)), v3(sum(&|i| bounce_b[i] * fs(i).1)), v3(sum(&|i| sun_light[i])),
                        v3(sum(&|i| pattern[i] * fs(i).0)), v3(sum(&|i| shown[i])), v3(sum(&|i| stored_at(&points[i])))
                    );
                }
            }
        }
        // The files hold nothing of the stored lightmaps here either. A
        // probe or a vertex whose room takes its fill from its stored light
        // is written without that fill, with the place of the stored texel
        // it was read from. The game reads that texel of the player's own
        // lightmap and adds the fill as it adds it to the room's texels
        // (`fill.txt`). Where the bake writes a floor (`floor.txt`), every
        // place with a texel near gets the reference, so that the game can
        // hold it to the floor as it holds the room's texels.
        let probe_stored: Vec<Option<usize>> = (0..probe_range.1)
            .map(|j| {
                let map = probe_maps[j]?;
                let filled = fills.contains_key(&room_name(&maps[map]));
                nearest[j].filter(|_| filled || options.floor.is_some())
            })
            .collect();
        let probe_linear: Vec<Vec3> = (0..probe_range.1)
            .map(|j| {
                let in_files = probe_maps[j].is_some_and(|map| fills.contains_key(&room_name(&maps[map])));
                if in_files && probe_stored[j].is_some() {
                    let scale = probe_maps[j].map_or(1.0, |map| even[map].map_or(rooms.scale[map], |(_, scale)| scale));
                    probe_sky[j] + probe_lamps[j] * scale
                } else {
                    probe_linear[j]
                }
            })
            .collect();
        let probe_light: Vec<Vec3> = probe_linear.iter().map(|light| eased_level(*light)).collect();
        (rooms, shown, written, fills, probe_light, probe_linear, probe_stored)
    };
    lap("levels".into(), &mut times);

    write_sets(
        resolver,
        &maps,
        &points,
        &counts,
        &unwalked,
        folder,
        &[("a", &shown_a[..], true), ("b", &written_b[..], false)],
        sunlit.as_deref(),
        options.stored_backdrop,
    )?;
    // A set baked earlier into the same folder may hold a `fill.txt` or a
    // `floor.txt` that this bake does not ask for.
    for name in ["fill.txt", "floor.txt", "probes.txt", PROBES_SUN, kjk_game::placeable_light::FILE] {
        let _ = std::fs::remove_file(folder.join("b").join(name));
    }
    if !fills_b.is_empty() {
        let mut text = String::from(
            "# How much of its stored light each lightmap takes when the game loads it:\n\
             # the lightmap's name and the number its stored linear light is times.\n",
        );
        for map in &maps {
            if let Some(fill) = fills_b.get(&room_name(map)) {
                let _ = writeln!(text, "{} {fill:.6}", map.name);
            }
        }
        let path = folder.join("b").join("fill.txt");
        std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    // The floor (`GpuOptions::floor`): one share for every lightmap of set
    // `b`, which the game holds each texel to.
    if let Some(floor) = options.floor.filter(|_| !options.old) {
        let mut text = String::from(
            "# The least share of its stored linear light each texel of a lightmap shows when the game loads it:
             # the lightmap's name and the share.
",
        );
        for map in &maps {
            let _ = writeln!(text, "{} {floor:.3}", map.name);
        }
        let path = folder.join("b").join("floor.txt");
        std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    if std::env::var_os("KJK_PROBE_CHECK").is_some() && !probe_light.is_empty() {
        // Each probe's light from above against the upward texels under it.
        let mut ratios: Vec<f32> = Vec::new();
        for (k, (_, at)) in probes.iter().enumerate() {
            let under: Vec<usize> = (0..count)
                .filter(|&i| counts[i] && points[i].normal.z > 0.9)
                .filter(|&i| points[i].at.truncate().distance(at.truncate()) < 0.3 && (at.z - points[i].at.z - PROBE_HEIGHT).abs() < 0.3)
                .collect();
            if under.is_empty() {
                continue;
            }
            let floor = under.iter().map(|&i| luma(written_b[i])).sum::<f32>() / under.len() as f32;
            if floor > 0.02 {
                ratios.push(luma(probe_light[6 * k + 2]) / floor);
            }
        }
        ratios.sort_by(f32::total_cmp);
        let at = |q: f32| ratios.get(((ratios.len() as f32 - 1.0) * q) as usize).copied().unwrap_or(0.0);
        eprintln!("probe up over floor texel under it, {} probes: 10% {:.2}, median {:.2}, 90% {:.2}", ratios.len(), at(0.1), at(0.5), at(0.9));
    }
    // The probes and the vertices are written as a lightmap's texels are:
    // at their level over `GAIN`, which the game multiplies back. The trial
    // of 22 November 2026 wrote the probes at their whole level, and the
    // game showed them at twice their light.
    let placeable_count = (area.instances.iter())
        .filter(|instance| instance.kind == asset_odyssey::module::Kind::Placeable)
        .count();
    let refs = |texels: &[Option<usize>]| stored_refs(&maps, &points, texels);
    if !probes.is_empty() && !probe_light.is_empty() {
        let light: Vec<Vec3> = probe_light[..6 * probes.len()].iter().map(|light| *light / GAIN).collect();
        // A probe's six places stand at one point and share one texel.
        let stored: Vec<Option<usize>> = (0..probes.len()).map(|k| probe_stored[6 * k]).collect();
        write_probes(&folder.join("b").join("probes.txt"), &probes, &light, &refs(&stored))?;
        if let Some((toward, colour)) = sun_of_probes {
            let unsunned: Vec<Vec3> = (0..6 * probes.len()).map(|j| probe_linear[j] - probe_sun[j]).collect();
            write_probes_sun(
                &folder.join("b").join(PROBES_SUN),
                &probes,
                (&unsunned, &probe_sun),
                &refs(&stored),
                toward,
                colour,
            )?;
        }
    }
    if !vertices.places.is_empty() && !probe_light.is_empty() {
        let mut light: Vec<Vec3> = probe_light[6 * probes.len()..].iter().map(|light| *light / GAIN).collect();
        let mut stored: Vec<Option<usize>> = probe_stored[6 * probes.len()..].to_vec();
        let filled = fill_vertices(&vertices, &vertex_counts, &mut light, &mut stored);
        outln!(
            "  placeables: {} of {} have a mesh the bake lights, {} of them block rays, and {} hold lights of their own",
            still.len(),
            placeable_count,
            occluders.len(),
            own_lights.iter().filter(|own| !own.is_empty()).count(),
        );
        for (owner, (still, own)) in still.iter().zip(&own_lights).enumerate().filter(|(_, (_, own))| !own.is_empty()) {
            let mine: Vec<u32> = (vertices.owners.iter().zip(&seen))
                .filter(|(of, _)| **of == owner)
                .map(|(_, sees)| *sees)
                .collect();
            outln!(
                "    {} at {:.2} {:.2} {:.2} holds {} fitted lights of its own, and {} of its {} vertices see one of them past its own meshes",
                still.name,
                still.at.x,
                still.at.y,
                still.at.z,
                own.len(),
                mine.iter().filter(|sees| **sees != 0).count(),
                mine.len(),
            );
        }
        outln!(
            "  placeables: {} vertices lit, {} before the long triangles were split, {} of them under a smaller placeable",
            light.len(),
            vertices.before,
            covered.iter().filter(|covered| **covered).count(),
        );
        outln!(
            "  placeables: {} vertices inside a surface or under a placeable: {} filled from their neighbours, {} from the nearest vertex, {} left unfilled",
            vertex_counts.iter().filter(|counts| !**counts).count(),
            filled.rings,
            filled.nearest,
            filled.none
        );
        outln!(
            "  placeables: {} vertices take their room's fill or floor from a stored texel when the game loads them",
            stored.iter().filter(|stored| stored.is_some()).count()
        );
        write_placeables(
            &folder.join("b").join(kjk_game::placeable_light::FILE),
            &area.resref,
            &vertices,
            &light,
            &refs(&stored),
        )?;
    } else if options.placeables {
        outln!(
            "  placeables: {} of {} have a mesh the bake may light, {} of them block rays",
            still.len(),
            placeable_count,
            occluders.len()
        );
    }
    if let Some(kinds) = &kinds {
        debug_pictures(folder, &maps, &points, kinds, &|i| stored_at(&points[i]), &shown_b)?;
    }
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
        "  {} lightmaps, {} lightmaps of rooms with no walkable face kept {}",
        maps.len(),
        unwalked.len(),
        if options.stored_backdrop { "as stored" } else { "at half" }
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

/// A placeable that holds a fitted light this near its box blocks no ray
/// (metres).
const LAMP_CLEAR: f32 = 0.3;
/// The mark on a light in a cell's list that is the vertex's placeable's own
/// (`OWN_LIGHT` in `gpu.wgsl`).
const OWN_LIGHT: u32 = 1 << 31;
/// How far a vertex's place is moved toward the middle of the triangles
/// around it, at the most (metres), and at the most as a share of the way
/// there. A place at a crate's corner then sits on its own face and not on
/// the edge with the floor.
const VERTEX_INWARD: f32 = 0.03;
const VERTEX_INWARD_SHARE: f32 = 0.25;
/// How many rings of neighbours a vertex inside a surface takes its light
/// from, at the most.
const VERTEX_RINGS: usize = 8;

/// The vertices of the placeables' meshes that the bake lights, as places of
/// the passes.
#[derive(Default)]
struct Vertices {
    /// Each vertex's place and the way it faces, in order: mesh by mesh,
    /// placeable by placeable.
    places: Vec<(Vec3, Vec3)>,
    /// The room each vertex stands in, by its model's name.
    rooms: Vec<Option<String>>,
    /// The placeable each vertex belongs to, by its number in the list
    /// `vertex_places` was given.
    owners: Vec<usize>,
    /// Each mesh: the placeable's number among the area's instances, its
    /// model's name, the mesh's number, where its vertices start in
    /// `places`, and its triangles.
    meshes: Vec<VertexMesh>,
    /// How many vertices the meshes had before their long triangles were
    /// split (`kjk_game::placeable_light::subdivide`).
    before: usize,
}

/// A mesh of `Vertices`: the placeable's number among the area's
/// instances, its model's name, the mesh's number, where its vertices start
/// in `Vertices::places`, and its triangles.
type VertexMesh = (usize, String, usize, usize, Vec<[u32; 3]>);

/// Each vertex of each mesh of `still` that the bake lights as a place:
/// moved toward the middle of its triangles by `VERTEX_INWARD` at the most,
/// lifted by `LIFT` along its normal, and facing its normal. A mesh that a
/// clip moves is taken where it rests. A vertex stands in the room of its
/// placeable's origin.
fn vertex_places(area: &AreaIr, reader: &mut Reader, still: &[Still]) -> Vertices {
    let placement = crate::room_lights::place(reader, area);
    let mut vertices = Vertices::default();
    for (owner, placeable) in still.iter().enumerate() {
        let lifted = placeable.at + Vec3::Z * 0.5;
        let room = crate::room_lights::room_at(&placement.walk_boxes, lifted.to_array())
            .map(|room| area.rooms[room].model.to_ascii_lowercase());
        for (number, mesh) in placeable.model.ir.meshes.iter().enumerate() {
            if !placeable.lit[number] {
                continue;
            }
            // The game draws the mesh with its long triangles split, and the
            // bake lights the vertices of the split mesh.
            vertices.before += mesh.positions.len();
            let divided = kjk_game::placeable_light::subdivide(mesh);
            let mesh = &divided;
            let corners = placeable.corners(mesh);
            let normals = placeable.normals(mesh);
            let triangles: Vec<[u32; 3]> = (mesh.indices.as_chunks::<3>().0.iter())
                .filter(|triangle| triangle.iter().all(|&i| (i as usize) < corners.len()))
                .copied()
                .collect();
            // The middle of the triangles around each vertex.
            let mut middles = vec![(Vec3::ZERO, 0.0f32); corners.len()];
            for triangle in &triangles {
                let middle = triangle.iter().map(|&i| corners[i as usize]).sum::<Vec3>() / 3.0;
                for &i in triangle {
                    middles[i as usize].0 += middle;
                    middles[i as usize].1 += 1.0;
                }
            }
            let start = vertices.places.len();
            for (k, corner) in corners.iter().enumerate() {
                let (sum, weight) = middles[k];
                let mut at = *corner;
                if weight > 0.0 {
                    let toward = sum / weight - *corner;
                    let length = toward.length();
                    if length > 1e-6 {
                        at += toward * ((VERTEX_INWARD_SHARE * length).min(VERTEX_INWARD) / length);
                    }
                }
                let normal = normals[k];
                vertices.places.push((at + normal * LIFT, if normal == Vec3::ZERO { Vec3::Z } else { normal }));
                vertices.rooms.push(room.clone());
                vertices.owners.push(owner);
            }
            vertices.meshes.push((placeable.object, placeable.name.clone(), number, start, triangles));
        }
    }
    vertices
}

/// Which of its placeable's own lights (`own_lights`) each vertex sees past
/// the placeable's own meshes, one bit for each light in the order of its
/// list. A vertex of a placeable with no light of its own has no bits.
///
/// A placeable that holds a light blocks no ray in the passes, so this is
/// the only shadow its own meshes cast from its own light. The terminal of
/// danm13 has its light in front of its screen. Without this test the light
/// reached the terminal's post and the back of its panel, and the whole
/// terminal came out bright blue. Meshes that glow are the lamp's glass and
/// cast no shadow, as in the room's bake (`lamps`). A light shut inside the
/// placeable's meshes reaches none of its vertices, as before its own lights
/// were let in.
fn own_seen(vertices: &Vertices, still: &[Still], own_lights: &[Vec<u32>], lights: &[Fitted]) -> Vec<u32> {
    let blockers: Vec<Vec<[Vec3; 3]>> = (still.iter().zip(own_lights))
        .map(|(still, own)| {
            if own.is_empty() {
                return Vec::new();
            }
            (still.model.ir.meshes.iter())
                .filter(|mesh| {
                    mesh.skin.is_none()
                        && mesh.dangly.is_none()
                        && mesh.transparency_hint == 0
                        && mesh.alpha >= 0.99
                        && mesh.self_illum.iter().all(|value| *value <= 0.0)
                })
                .flat_map(|mesh| {
                    let world = still.corners(mesh);
                    (mesh.indices.as_chunks::<3>().0.iter())
                        .filter_map(|&[a, b, c]| {
                            let corner = |i: u32| world.get(i as usize).copied();
                            Some([corner(a)?, corner(b)?, corner(c)?])
                        })
                        .collect::<Vec<_>>()
                })
                .collect()
        })
        .collect();
    (vertices.places.iter().zip(&vertices.owners))
        .map(|((at, _), &owner)| {
            let mut seen = 0u32;
            for (bit, &number) in own_lights[owner].iter().enumerate().take(32) {
                let light = &lights[number as usize];
                let goal = light.at + light.way * (*at - light.at).dot(light.way).clamp(-light.half, light.half);
                let way = goal - *at;
                let far = way.length() - OWN_SHADOW_SHORT;
                if far <= 0.0 || !blockers[owner].iter().any(|corners| meets(*at, way.normalize(), far, corners)) {
                    seen |= 1 << bit;
                }
            }
            seen
        })
        .collect()
}

/// How far short of its own light a vertex's shadow ray stops (metres).
const OWN_SHADOW_SHORT: f32 = 0.02;

/// Whether a ray from `from` along `toward` meets the triangle `corners`
/// beyond a millimetre and within `far`.
fn meets(from: Vec3, toward: Vec3, far: f32, corners: &[Vec3; 3]) -> bool {
    let [a, b, c] = *corners;
    let (ab, ac) = (b - a, c - a);
    let p = toward.cross(ac);
    let det = ab.dot(p);
    if det.abs() < 1e-9 {
        return false;
    }
    let inverse = 1.0 / det;
    let s = from - a;
    let u = s.dot(p) * inverse;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = s.cross(ab);
    let v = toward.dot(q) * inverse;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = ac.dot(q) * inverse;
    t > 1e-3 && t < far
}

/// How far beyond a placeable's box a vertex still counts as under it in
/// `covered_vertices` (metres). A vertex's place is lifted by `LIFT` off its
/// surface.
const COVER_MARGIN: f32 = 0.03;

/// Which vertices of `vertices` stand inside the box of a smaller placeable
/// of `still` that blocks rays, such as the food on one of 203tel's oval
/// tables. Such a vertex is hidden under that placeable, and the rays from
/// it meet the placeable almost all round, so the bake makes it nearly
/// black. Blended across the triangles around it, its dark spread far past
/// the food, so the middle of the table came out dark. Such a vertex takes
/// the light of the vertices around it that are seen (`fill_vertices`).
///
/// The vertices beside the smaller placeable still count, and they keep the
/// shade it casts. The room's texels are not changed, so the floor keeps its
/// dark ring around a crate.
fn covered_vertices(vertices: &Vertices, still: &[Still], own_lights: &[Vec<u32>]) -> Vec<bool> {
    // Each placeable's box over its meshes that block rays, and its volume.
    let boxes: Vec<Option<(Vec3, Vec3, f32)>> = (still.iter().zip(own_lights))
        .map(|(still, own)| {
            if !own.is_empty() {
                return None;
            }
            let corners: Vec<Vec3> = (still.model.ir.meshes.iter())
                .filter(|mesh| mesh.skin.is_none() && mesh.dangly.is_none())
                .flat_map(|mesh| still.corners(mesh))
                .collect();
            if corners.is_empty() {
                return None;
            }
            let low = corners.iter().fold(Vec3::splat(f32::MAX), |low, p| low.min(*p));
            let high = corners.iter().fold(Vec3::splat(f32::MIN), |high, p| high.max(*p));
            let size = (high - low).max(Vec3::splat(0.01));
            Some((low, high, size.x * size.y * size.z))
        })
        .collect();
    let volume_of = |owner: usize| {
        let corners: Vec<Vec3> = still[owner].model.ir.meshes.iter().flat_map(|mesh| still[owner].corners(mesh)).collect();
        let low = corners.iter().fold(Vec3::splat(f32::MAX), |low, p| low.min(*p));
        let high = corners.iter().fold(Vec3::splat(f32::MIN), |high, p| high.max(*p));
        let size = (high - low).max(Vec3::splat(0.01));
        size.x * size.y * size.z
    };
    let volumes: Vec<f32> = (0..still.len()).map(volume_of).collect();
    (vertices.places.iter().zip(&vertices.owners))
        .map(|((at, _), &owner)| {
            boxes.iter().enumerate().any(|(other, found)| {
                found.is_some_and(|(low, high, volume)| {
                    other != owner
                        && volume < volumes[owner]
                        && at.cmpge(low - Vec3::splat(COVER_MARGIN)).all()
                        && at.cmple(high + Vec3::splat(COVER_MARGIN)).all()
                })
            })
        })
        .collect()
}

/// How much a neighbour that faces another way counts in `fill_vertices`,
/// against 1 for one that faces the same way.
const VERTEX_ASKEW: f32 = 0.05;

/// How much the light of a vertex facing `from` counts toward one facing
/// `to` in `fill_vertices`. A corner of a crate's top takes the light of the
/// top beside it before that of the side under it.
fn facing_weight(to: Vec3, from: Vec3) -> f32 {
    let along = to.dot(from).max(0.0);
    VERTEX_ASKEW + along * along
}

/// How many vertices `fill_vertices` filled, by the way it found their light.
#[derive(Default)]
struct Filled {
    /// From neighbours on the same mesh.
    rings: usize,
    /// From the nearest vertex of the same placeable whose place counts.
    nearest: usize,
    /// Left unfilled: the placeable has no vertex whose place counts.
    none: usize,
}

/// Gives each vertex whose place does not count (inside a wall or under the
/// floor) the light of the vertices near it whose places do.
///
/// First it takes the mean light of its neighbours on its mesh, a ring at a
/// time, `VERTEX_RINGS` times at most. A vertex is a neighbour of another
/// when a triangle joins them or when they stand at the same place, as the
/// corners of a crate's faces do. A neighbour counts by how closely it faces
/// the same way (`facing_weight`).
///
/// A vertex that no ring reaches takes the light of the nearest vertex of
/// its placeable whose place counts, among those that face its way when
/// there are any. Without it such a vertex kept the light the passes gave
/// its place inside the surface. A vertex is left as it is only when no
/// vertex of its placeable counts.
///
/// A filled vertex takes the stored texel of its fill (`stored`) from the
/// neighbour that counts most, or from the nearest vertex it took its light
/// from. The light it takes holds no fill, so the game adds that texel's
/// fill to the mean of its neighbours' light, where the bake had the mean
/// of their light with the fill. The two differ a little where the
/// neighbours' fills differ.
fn fill_vertices(vertices: &Vertices, counts: &[bool], light: &mut [Vec3], stored: &mut [Option<usize>]) -> Filled {
    let mut filled = Filled::default();
    let mut known: Vec<bool> = counts.to_vec();
    for (m, (_, _, _, start, triangles)) in vertices.meshes.iter().enumerate() {
        let end = (vertices.meshes.get(m + 1)).map_or(vertices.places.len(), |(_, _, _, next, _)| *next);
        let size = end - start;
        let mut next: Vec<Vec<usize>> = vec![Vec::new(); size];
        for triangle in triangles {
            for a in 0..3 {
                for b in 0..3 {
                    if a != b {
                        next[triangle[a] as usize].push(triangle[b] as usize);
                    }
                }
            }
        }
        let mut same: HashMap<[i32; 3], Vec<usize>> = HashMap::new();
        for k in 0..size {
            let key = (vertices.places[start + k].0 * 100.0).round().as_ivec3().to_array();
            same.entry(key).or_default().push(k);
        }
        for group in same.values() {
            for &k in group {
                next[k].extend(group.iter().filter(|&&other| other != k));
            }
        }
        for _ in 0..VERTEX_RINGS {
            let before: Vec<bool> = known[*start..end].to_vec();
            let mut any = false;
            for k in (0..size).filter(|&k| !before[k]) {
                let facing = vertices.places[start + k].1;
                let (sum, weight) = (next[k].iter().filter(|&&other| before[other]))
                    .map(|&other| {
                        let weight = facing_weight(facing, vertices.places[start + other].1);
                        (light[start + other] * weight, weight)
                    })
                    .fold((Vec3::ZERO, 0.0), |(sum, total), (light, weight)| (sum + light, total + weight));
                if weight > 0.0 {
                    light[start + k] = sum / weight;
                    // The stored texel of the fill is the one of the
                    // neighbour that counts most, of the nearest number
                    // among equals.
                    let heaviest = (next[k].iter().copied().filter(|&other| before[other]))
                        .map(|other| (facing_weight(facing, vertices.places[start + other].1), other))
                        .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
                    if let Some((_, other)) = heaviest {
                        stored[start + k] = stored[start + other];
                    }
                    known[start + k] = true;
                    filled.rings += 1;
                    any = true;
                }
            }
            if !any {
                break;
            }
        }
    }
    // The vertices no ring reached, from the nearest that count on their
    // placeable.
    let mut by_owner: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, owner) in vertices.owners.iter().enumerate() {
        by_owner.entry(*owner).or_default().push(k);
    }
    for members in by_owner.values() {
        let sources: Vec<usize> = members.iter().copied().filter(|&k| counts[k]).collect();
        for &k in members.iter().filter(|&&k| !known[k]) {
            let (at, facing) = vertices.places[k];
            let nearest = |facing_only: bool| {
                (sources.iter().copied())
                    .filter(|&other| !facing_only || facing.dot(vertices.places[other].1) > 0.0)
                    .min_by(|&a, &b| {
                        at.distance_squared(vertices.places[a].0)
                            .total_cmp(&at.distance_squared(vertices.places[b].0))
                    })
            };
            match nearest(true).or_else(|| nearest(false)) {
                Some(other) => {
                    light[k] = light[other];
                    stored[k] = stored[other];
                    filled.nearest += 1;
                }
                None => filled.none += 1,
            }
        }
    }
    filled
}

/// The stored texels that a bake's file names for the game to read a
/// place's fill from (`kjk_game::stored_fill`).
struct StoredRefs {
    /// Each lightmap the file names, with the width and height of the
    /// bake's grid of texels on it.
    maps: Vec<(String, usize, usize)>,
    /// Each place's lightmap, by its number in `maps`, and its texel in
    /// that grid, row by row. `None` for a place that takes no fill from
    /// the stored light.
    at: Vec<Option<(usize, usize)>>,
}

impl StoredRefs {
    /// The file's lines that name its lightmaps, after its format.
    fn header(&self, text: &mut String) {
        let _ = writeln!(text, "format {}", kjk_game::bake_fingerprint::FORMAT);
        for (name, width, height) in &self.maps {
            let _ = writeln!(text, "{}", kjk_game::stored_fill::map_line(name, *width, *height));
        }
    }

    /// The word that names place `k`'s stored texel, with a space before
    /// it, or nothing.
    fn word(&self, k: usize) -> String {
        self.at[k].map_or(String::new(), |(map, texel)| {
            format!(" {}", kjk_game::stored_fill::word(map, texel))
        })
    }

    /// `word`, or the short word `stored_fill::SAME` when place `k` names
    /// the same texel as place `before`.
    fn word_after(&self, k: usize, before: Option<usize>) -> String {
        match (self.at[k], before.and_then(|before| self.at[before])) {
            (Some(now), Some(then)) if now == then => format!(" {}", kjk_game::stored_fill::SAME),
            _ => self.word(k),
        }
    }
}

/// The lightmaps and texels of `texels`, each a texel of `points` or none.
fn stored_refs(maps: &[Stored], points: &[Point], texels: &[Option<usize>]) -> StoredRefs {
    let mut numbers: std::collections::BTreeMap<usize, usize> = Default::default();
    let mut refs = StoredRefs { maps: Vec::new(), at: Vec::with_capacity(texels.len()) };
    for texel in texels {
        refs.at.push(texel.map(|i| {
            let map = points[i].map;
            let number = *numbers.entry(map).or_insert_with(|| {
                let stored = &maps[map];
                refs.maps.push((stored.name.clone(), stored.width * stored.scale, stored.height * stored.scale));
                refs.maps.len() - 1
            });
            (number, points[i].texel)
        }));
    }
    refs
}

/// Writes the placeables' light to `path` (`kjk_game::placeable_light::read`
/// reads it): a line for each mesh with its placeable's number among the
/// area's instances, its model, the mesh's number, how many vertices it has,
/// and the light of each vertex at the level a lightmap of set `b` stores.
/// A vertex that takes its room's fill from the stored light holds its
/// light without it, and the word after its light names the stored texel
/// the game reads the fill from (`stored`).
fn write_placeables(
    path: &Path,
    area: &str,
    vertices: &Vertices,
    light: &[Vec3],
    stored: &StoredRefs,
) -> Result<(), String> {
    let mut text = String::from(
        "# The light on each vertex of each placeable's mesh that the bake lights (crates/game/src/placeable_light.rs):\n\
         # mesh, the placeable's number among the area's instances, its model, the mesh's number, how many\n\
         # vertices it has, and each vertex's r g b at the level the lightmaps of this set store.\n\
         # The light holds nothing of the game's own lightmaps. After a vertex's r g b, @<map>:<texel> names the\n\
         # texel of a stored lightmap listed under `map` (its name, and the width and height of the bake's grid),\n\
         # whose light the game adds as fill.txt and floor.txt ask (crates/game/src/stored_fill.rs). A lone @\n\
         # names the texel of the vertex before it on the line.\n",
    );
    let _ = writeln!(text, "area {area}");
    stored.header(&mut text);
    for (k, (object, model, number, start, _)) in vertices.meshes.iter().enumerate() {
        let end = vertices.meshes.get(k + 1).map_or(light.len(), |(_, _, _, next, _)| *next);
        let _ = write!(text, "mesh {object} {model} {number} {}", end - start);
        for (offset, value) in light[*start..end].iter().enumerate() {
            let word = stored.word_after(start + offset, offset.checked_sub(1).map(|before| start + before));
            let _ = write!(text, " {:.4} {:.4} {:.4}{word}", value.x, value.y, value.z);
        }
        text.push('\n');
    }
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The ways a probe's six places face, in the area's space (z up), in the
/// order of the game's probes (`AXES` in `crates/game/src/light_probes.rs`):
/// east, west, up, down, south, north.
const PROBE_AXES: [Vec3; 6] = [
    Vec3::X,
    Vec3::NEG_X,
    Vec3::Z,
    Vec3::NEG_Z,
    Vec3::NEG_Y,
    Vec3::Y,
];
/// How far apart the probes stand, how high over the walkable floor, and
/// how far apart two storeys must be to each have one, in metres, as the
/// game's (`probe_places` in `crates/game/src/light_probes.rs`).
const PROBE_SPACING: f32 = 1.0;
const PROBE_HEIGHT: f32 = 1.0;
const PROBE_STOREY: f32 = 1.5;

/// Where the area's probes stand, with the name of each one's room: on a
/// grid `PROBE_SPACING` apart, `PROBE_HEIGHT` over each walkable face of a
/// room's walkmesh, as the game places its own.
fn probe_places(reader: &Reader, area: &AreaIr) -> Vec<(String, Vec3)> {
    let mut places = Vec::new();
    for room in &area.rooms {
        let floor = reader.walkable_faces(&room.model);
        let mut heights: std::collections::BTreeMap<(i32, i32), Vec<f32>> = Default::default();
        for face in &floor {
            let [a, b, c] = face.map(Vec3::from_array);
            let (low, high) = (a.min(b).min(c), a.max(b).max(c));
            for gx in (low.x / PROBE_SPACING).ceil() as i32..=(high.x / PROBE_SPACING).floor() as i32 {
                for gy in (low.y / PROBE_SPACING).ceil() as i32..=(high.y / PROBE_SPACING).floor() as i32 {
                    let (x, y) = (gx as f32 * PROBE_SPACING, gy as f32 * PROBE_SPACING);
                    if let Some(z) = height_at([a, b, c], x, y) {
                        heights.entry((gx, gy)).or_default().push(z);
                    }
                }
            }
        }
        for ((gx, gy), mut at) in heights {
            at.sort_by(f32::total_cmp);
            at.dedup_by(|a, b| (*a - *b).abs() < PROBE_STOREY);
            let (x, y) = (gx as f32 * PROBE_SPACING, gy as f32 * PROBE_SPACING);
            places.extend(at.into_iter().map(|z| (room.model.to_ascii_lowercase(), Vec3::new(x, y, z + PROBE_HEIGHT))));
        }
    }
    places
}

/// The height of triangle `[a, b, c]` over the point `x`, `y`, or `None`
/// when the point is not over it.
fn height_at([a, b, c]: [Vec3; 3], x: f32, y: f32) -> Option<f32> {
    let d = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
    if d.abs() < 1e-9 {
        return None;
    }
    let u = ((b.y - c.y) * (x - c.x) + (c.x - b.x) * (y - c.y)) / d;
    let v = ((c.y - a.y) * (x - c.x) + (a.x - c.x) * (y - c.y)) / d;
    let w = 1.0 - u - v;
    (u >= -1e-4 && v >= -1e-4 && w >= -1e-4).then_some(u * a.z + v * b.z + w * c.z)
}

/// Writes the probes' light to `path`: a line for each probe with its
/// room, its place and the light on each of its six sides, in the order of
/// `PROBE_AXES`, at the level a lightmap of set `b` stores. A probe that
/// takes its room's fill from the stored light holds its light without it,
/// and the line ends with the stored texel the game reads the fill from
/// (`stored`).
fn write_probes(path: &Path, probes: &[(String, Vec3)], light: &[Vec3], stored: &StoredRefs) -> Result<(), String> {
    let mut text = String::from(
        "# The light on each probe, as the game's probes hold it (crates/game/src/light_probes.rs):\n\
         # its room, its place (x y z), and the light on a surface facing east, west, up, down,\n\
         # south and north, each r g b at the level the lightmaps of this set store.\n\
         # The light holds nothing of the game's own lightmaps. A last word @<map>:<texel> names the texel\n\
         # of a stored lightmap listed under `map`, whose light the game adds to the six sides as fill.txt\n\
         # and floor.txt ask (crates/game/src/stored_fill.rs).\n",
    );
    stored.header(&mut text);
    for (k, (room, at)) in probes.iter().enumerate() {
        let _ = write!(text, "{room} {:.3} {:.3} {:.3}", at.x, at.y, at.z);
        for side in &light[6 * k..6 * k + 6] {
            let _ = write!(text, " {:.4} {:.4} {:.4}", side.x, side.y, side.z);
        }
        text.push_str(&stored.word(k));
        text.push('\n');
    }
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The file of a bake's set that holds the probes for the live sun
/// (`write_probes_sun`).
const PROBES_SUN: &str = "probes-sun.txt";

/// Writes the probes for the live sun to `path`, which the game reads under
/// `KJK_PROBE_SUN=on` (`baked_probes` in `crates/game/src/viewer.rs`).
///
/// The first line gives the sun's way (Bevy's space, toward the sun), its
/// linear colour, and the bake's curve (`eased_level`): its gamma, its knee
/// and how far it lets light go over the knee. Each further line gives a
/// probe's room, its place, the share of the sun that reaches it, and the
/// linear light on its six sides without the sun's direct light, before the
/// curve. The sky, the fitted lights at the room's scale, the room's fill
/// and every bounce, the sun's among them, stay in it.
///
/// The shader adds the sun's colour times how squarely a surface faces the
/// sun times the share, and then puts the sum through the same curve. A
/// room's scale is for its lamps alone, so the sun takes none.
///
/// The share is the sun's light on the probe's sides that face the sun over
/// what those sides would take with nothing in the way. The pass gives each
/// side the share of `SUN_RAYS` rays that reach the sun times how squarely
/// it faces it, so the sum over the sides is a mean of their shares weighed
/// by how squarely each faces the sun.
///
/// A probe that takes its room's fill from the stored light holds its light
/// without it, and the line ends with the stored texel the game reads the
/// fill from (`stored`), as in `write_probes`.
fn write_probes_sun(
    path: &Path,
    probes: &[(String, Vec3)],
    (light, sun): (&[Vec3], &[Vec3]),
    stored: &StoredRefs,
    toward: Vec3,
    colour: Vec3,
) -> Result<(), String> {
    // The area's space (z up) to Bevy's (y up), as `PROBE_AXES` map to the
    // game's `AXES`: east is x, up is y and south is z.
    let way = Vec3::new(toward.x, toward.z, -toward.y);
    let facing: f32 = PROBE_AXES.iter().map(|axis| axis.dot(toward).max(0.0)).sum();
    let strength = luma(colour).max(1e-6);
    let mut text = String::from(
        "# The probes for the live sun (crates/launcher/src/detail_bake/gpu.rs, write_probes_sun).\n\
         # sun: its way in Bevy's space (x y z), its linear colour (r g b), and the curve's gamma, knee and most over the knee.\n\
         # Then each probe: its room, its place (x y z), the share of the sun that reaches it, and the\n\
         # linear light without the sun on a surface facing east, west, up, down, south and north.\n\
         # The light holds nothing of the game's own lightmaps. A last word @<map>:<texel> names the texel\n\
         # of a stored lightmap listed under `map`, whose linear light the game adds as fill.txt and\n\
         # floor.txt ask (crates/game/src/stored_fill.rs).\n",
    );
    stored.header(&mut text);
    let _ = writeln!(
        text,
        "sun {:.5} {:.5} {:.5} {:.5} {:.5} {:.5} {GAMMA} {B_KNEE} {B_OVER}",
        way.x, way.y, way.z, colour.x, colour.y, colour.z
    );
    for (k, (room, at)) in probes.iter().enumerate() {
        let reached: f32 = sun[6 * k..6 * k + 6].iter().map(|light| luma(*light)).sum();
        let share = (reached / (strength * facing).max(1e-6)).clamp(0.0, 1.0);
        let _ = write!(text, "{room} {:.3} {:.3} {:.3} {share:.4}", at.x, at.y, at.z);
        for side in &light[6 * k..6 * k + 6] {
            let side = side.max(Vec3::ZERO);
            let _ = write!(text, " {:.5} {:.5} {:.5}", side.x, side.y, side.z);
        }
        text.push_str(&stored.word(k));
        text.push('\n');
    }
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The fill's pattern (`GpuOptions::fill_stored`) at each probe's place and
/// each vertex: that of the counted texel of the same room nearest it
/// (`nearest`). A place with no such texel, or a bake without the stored
/// fill, takes 1, which is the pattern's mean over its room.
fn places_pattern(pattern: &[Vec3], nearest: &[Option<usize>], fill_stored: bool) -> Vec<Vec3> {
    if !fill_stored {
        return vec![Vec3::ONE; nearest.len()];
    }
    (nearest.iter())
        .map(|nearest| nearest.map_or(Vec3::ONE, |i| pattern[i]))
        .collect()
}

/// The counted texel of the same room nearest each of `at`, within 4
/// metres.
fn nearest_texels(
    maps: &[Stored],
    points: &[Point],
    counted: &[usize],
    at: &[Vec3],
    map_of: &[Option<usize>],
) -> Vec<Option<usize>> {
    const CELL: f32 = 0.5;
    const RINGS: i32 = 8;
    let rooms: Vec<String> = maps.iter().map(room_name).collect();
    let mut grid: HashMap<IVec3, Vec<usize>> = HashMap::new();
    for &i in counted {
        grid.entry((points[i].at / CELL).floor().as_ivec3()).or_default().push(i);
    }
    (at.iter().zip(map_of))
        .map(|(at, map)| {
            let map = (*map)?;
            let room = &rooms[map];
            let key = (*at / CELL).floor().as_ivec3();
            let mut best: Option<(f32, usize)> = None;
            for ring in 0..=RINGS {
                for dx in -ring..=ring {
                    for dy in -ring..=ring {
                        for dz in -ring..=ring {
                            if dx.abs().max(dy.abs()).max(dz.abs()) != ring {
                                continue;
                            }
                            let Some(near) = grid.get(&(key + IVec3::new(dx, dy, dz))) else {
                                continue;
                            };
                            for &i in near {
                                if rooms[points[i].map] != *room {
                                    continue;
                                }
                                let apart = points[i].at.distance_squared(*at);
                                if best.is_none_or(|(least, _)| apart < least) {
                                    best = Some((apart, i));
                                }
                            }
                        }
                    }
                }
                // A texel of the next ring can be nearer than one found in
                // this ring's corner, so the search runs one ring on.
                if let Some((least, _)) = best
                    && least.sqrt() < ring as f32 * CELL
                {
                    break;
                }
            }
            best.map(|(_, i)| i)
        })
        .collect()
}

fn luma(c: Vec3) -> f32 {
    0.2126 * c.x + 0.7152 * c.y + 0.0722 * c.z
}

/// Smooths the share of the lights each texel sees over its neighbours on
/// the same surface, as far as `SMOOTH_RINGS` texels around it
/// (`Places::mean`). A neighbour counts when it faces the same way and lies
/// on the same plane.
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
    let places = Places::new(points, counts);
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
        if !counts[index] {
            return shadowed[index];
        }
        match places.mean(maps, points, index, SMOOTH_RINGS, &share) {
            Some(mean) => open[index] * mean,
            None => shadowed[index],
        }
    });
    shadowed[..points.len()].copy_from_slice(&smoothed);
}

/// The colour that tells what a texel is in the pictures of
/// `KJK_BAKE_DEBUG`. Red is a texel inside a wall: too many of its rays meet
/// the back of a surface, and it does not count. Blue is a texel whose
/// centre lies off its triangle, on the ring the bake adds round a patch.
/// Magenta is a texel whose mesh normal faces away from its triangle.
/// Dark cyan is a texel that no light faces. Grey is the share of its
/// lights' light that reaches it past every shadow, white where all does.
fn texel_kind(point: &Point, counts: bool, open: Vec3, shadowed: Vec3) -> [u8; 3] {
    if !counts {
        return [220, 0, 0];
    }
    if !point.inside {
        return [0, 60, 230];
    }
    if point.shading.is_some_and(|shading| shading.dot(point.normal) < 0.0) {
        return [220, 0, 220];
    }
    if luma(open) < 1e-6 {
        return [0, 70, 70];
    }
    let share = (luma(shadowed) / luma(open)).clamp(0.0, 1.0);
    let grey = (40.0 + 215.0 * share) as u8;
    [grey, grey, grey]
}

/// Writes `debug/<lightmap>.png` into `folder` for `KJK_BAKE_DEBUG`, each
/// three pictures wide at the bake's own texels: what each texel is
/// (`texel_kind`, black where no triangle claims it), the stored lightmap,
/// and the lamps' set `b` as the bake shows it. `debug/texels.txt` gives
/// each lightmap's side of a stored texel in metres and how many texels
/// are of each kind.
fn debug_pictures(
    folder: &Path,
    maps: &[Stored],
    points: &[Point],
    kinds: &[[u8; 3]],
    stored: &dyn Fn(usize) -> Vec3,
    shown: &[Vec3],
) -> Result<(), String> {
    let out = folder.join("debug");
    std::fs::create_dir_all(&out).map_err(|error| format!("{}: {error}", out.display()))?;
    let mut text = String::from(
        "# lightmap, stored texel side in metres (median), texels counted, inside a wall, off the triangle, normal away, unlit\n",
    );
    for (number, map) in maps.iter().enumerate() {
        let (w, h) = (map.width * map.scale, map.height * map.scale);
        let mut picture = image::RgbImage::new((3 * w) as u32, h as u32);
        let mut sizes = Vec::new();
        let mut tally = [0usize; 5];
        for (i, point) in points.iter().enumerate().filter(|(_, point)| point.map == number) {
            let (x, y) = ((point.texel % w) as u32, (point.texel / w) as u32);
            let kind = kinds[i];
            picture.put_pixel(x, y, image::Rgb(kind));
            let s = stored(i).to_array().map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
            picture.put_pixel(x + w as u32, y, image::Rgb(s));
            let b = shown[i].to_array().map(|v| (v.clamp(0.0, 1.0) * 255.0) as u8);
            picture.put_pixel(x + 2 * w as u32, y, image::Rgb(b));
            sizes.push(point.size);
            tally[match kind {
                [220, 0, 0] => 1,
                [0, 60, 230] => 2,
                [220, 0, 220] => 3,
                [0, 70, 70] => 4,
                _ => 0,
            }] += 1;
        }
        sizes.sort_by(f32::total_cmp);
        let side = sizes.get(sizes.len() / 2).copied().unwrap_or(0.0);
        let _ = writeln!(
            text,
            "{} {side:.3} {} {} {} {} {}",
            map.name, tally[0], tally[1], tally[2], tally[3], tally[4]
        );
        let path = out.join(format!("{}.png", map.name));
        picture.save(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    let path = out.join("texels.txt");
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))
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
            let (w, h) = ((stored.width * stored.scale) as i64, (stored.height * stored.scale) as i64);
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
/// surface as far as `BOUNCE_RINGS` texels around it (`Places::mean`),
/// weighted to fall off with distance.
fn smooth_values(maps: &[Stored], points: &[Point], counts: &[bool], values: &[Vec3]) -> Vec<Vec3> {
    let places = Places::new(points, counts);
    super::parallel(points.len(), |index| {
        if !counts[index] {
            return values[index];
        }
        places
            .mean(maps, points, index, BOUNCE_RINGS, &|other| values[other])
            .unwrap_or(values[index])
    })
}

/// The texels that count, by their place in the area, so a smoothing can
/// gather a texel's neighbours on the same surface across the patches of
/// its lightmap and across lightmaps (`Places::mean`). Smoothed by their
/// neighbours in the lightmap, the walkway of tar_m03aa, whose floor tiles
/// each have their own small patch, took one level of bounced light for
/// each tile, and the tiles met at hard straight edges (22 November 2026).
struct Places {
    cells: HashMap<IVec3, Vec<u32>>,
}

/// The side of a cell of `Places` (metres).
const PLACE_CELL: f32 = 0.5;
/// The widest a smoothing reaches (metres).
const PLACE_REACH: f32 = 1.0;

impl Places {
    fn new(points: &[Point], counts: &[bool]) -> Places {
        let mut cells: HashMap<IVec3, Vec<u32>> = HashMap::new();
        for (index, point) in points.iter().enumerate().filter(|(i, _)| counts[*i]) {
            let cell = (point.at / PLACE_CELL).floor().as_ivec3();
            cells.entry(cell).or_default().push(index as u32);
        }
        Places { cells }
    }

    /// The mean of `value` over the texels near texel `index` that face the
    /// same way and lie on its plane, each weighted by its distance. The
    /// width is `rings` texels at the bake's usual 4 to a stored texel, as
    /// the smoothing in the lightmap had, and at most `PLACE_REACH`.
    fn mean(
        &self,
        maps: &[Stored],
        points: &[Point],
        index: usize,
        rings: i64,
        value: &dyn Fn(usize) -> Vec3,
    ) -> Option<Vec3> {
        let point = &points[index];
        let map = &maps[point.map];
        let near = (point.size / map.scale as f32 * 0.5).max(0.01);
        let reach = (rings as f32 * point.size / 4.0).clamp(1e-3, PLACE_REACH);
        let low = ((point.at - Vec3::splat(reach)) / PLACE_CELL).floor().as_ivec3();
        let high = ((point.at + Vec3::splat(reach)) / PLACE_CELL).floor().as_ivec3();
        let mut sum = Vec3::ZERO;
        let mut total = 0.0f32;
        for x in low.x..=high.x {
            for y in low.y..=high.y {
                for z in low.z..=high.z {
                    for &other in self.cells.get(&IVec3::new(x, y, z)).into_iter().flatten() {
                        let them = &points[other as usize];
                        let way = them.at - point.at;
                        let d2 = way.length_squared();
                        if d2 > reach * reach
                            || them.normal.dot(point.normal) < 0.9
                            || way.dot(point.normal).abs() > near
                        {
                            continue;
                        }
                        let weight = (-d2 / (reach * reach)).exp();
                        sum += value(other as usize) * weight;
                        total += weight;
                    }
                }
            }
        }
        (total > 0.0).then(|| sum / total)
    }
}
