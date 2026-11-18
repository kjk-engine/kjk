//! The GPU's time is taken by two timestamps a frame, each at the end of a
//! one-thread compute pass, queued before and after all of the frame's
//! rendering. Apple's GPUs take timestamps only at the boundaries of a pass
//! that does work, and they run successive command buffers side by side. So
//! on Apple's GPUs the second pass reads one texel of each view's main
//! texture, which makes it wait for the passes that draw into it.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::prelude::*;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, MapMode, TextureSampleType,
    TextureUsages, TextureViewId, WgpuFeatures,
};
use bevy::render::renderer::{
    PendingCommandBuffers, RenderAdapterInfo, RenderDevice, RenderGraph, RenderGraphSystems,
    RenderQueue,
};
use bevy::render::view::ViewTarget;
use bevy::render::{Render, RenderApp, RenderSystems};

/// A frame is stamped only while a slot is free, and a slot is freed only
/// when its reading comes back, several frames later. With 3 slots, a Mac
/// at 170 fps left about 40% of its frames unstamped.
const SLOTS: usize = 8;

const SHADER: &str = "
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> sink: vec4<f32>;
@compute @workgroup_size(1) fn begin() {}
@compute @workgroup_size(1) fn end() { sink = textureLoad(frame, vec2<i32>(0, 0), 0); }
";

const SINK_BYTES: u64 = 16;

const STAMP_BYTES: u64 = 16;

const FREE: u8 = 0;
const RECORDED: u8 = 1;
const READING: u8 = 2;

#[derive(Resource, Clone, Default)]
pub(crate) struct GpuLoad(Arc<Mutex<Measured>>);

#[derive(Default, Clone, Debug)]
pub(crate) struct Measured {
    pub timed: bool,
    pub gpu_ms: Vec<f64>,
    pub render_ms: f64,
    pub render_frames: u32,
    pub lost: Lost,
    /// The second timestamp of the last reading kept. A frame's GPU time
    /// starts no earlier than this. It is kept across `take`.
    last_end: u64,
}

/// The frames that gave no GPU reading, by reason.
#[derive(Default, Clone, Copy, Debug, PartialEq)]
pub(crate) struct Lost {
    /// No slot was free, or a Mac's frame had no main texture to read.
    pub unstamped: u32,
    /// A timestamp was not written in time, the frame ends no later than
    /// the frame before, or the read back failed.
    pub stale: u32,
    /// The reading was longer than the wall time the frame had, so its two
    /// timestamps do not belong together.
    pub impossible: u32,
    /// The GPU load check dropped the reading because it was longer than
    /// two of its window's mean render frames.
    pub too_long: u32,
}

impl Lost {
    pub(crate) fn add(&mut self, other: Lost) {
        self.unstamped += other.unstamped;
        self.stale += other.stale;
        self.impossible += other.impossible;
        self.too_long += other.too_long;
    }
}

#[cfg(feature = "probes")]
impl Measured {
    /// A window's worth of frames for the GPU load check's probes.
    pub(crate) fn probe(gpu_ms: &[f64], render_ms: f64) -> Measured {
        Measured {
            timed: true,
            gpu_ms: gpu_ms.to_vec(),
            render_ms: render_ms * gpu_ms.len().max(1) as f64,
            render_frames: gpu_ms.len().max(1) as u32,
            ..Measured::default()
        }
    }
}

impl GpuLoad {
    pub(crate) fn take(&self) -> Measured {
        let Ok(mut measured) = self.0.lock() else {
            return Measured::default();
        };
        let kept = Measured {
            timed: measured.timed,
            last_end: measured.last_end,
            ..Measured::default()
        };
        std::mem::replace(&mut *measured, kept)
    }

    fn with(&self, change: impl FnOnce(&mut Measured)) {
        if let Ok(mut measured) = self.0.lock() {
            change(&mut measured);
        }
    }
}

pub(crate) struct GpuLoadPlugin;

impl Plugin for GpuLoadPlugin {
    fn build(&self, app: &mut App) {
        let load = GpuLoad::default();
        app.insert_resource(load.clone());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(load)
            .init_resource::<FrameStamps>()
            .add_systems(Render, read_back.in_set(RenderSystems::Cleanup))
            .add_systems(
                RenderGraph,
                (
                    frame_begins.in_set(RenderGraphSystems::Begin),
                    frame_ends
                        .after(RenderGraphSystems::Render)
                        .before(RenderGraphSystems::Submit),
                ),
            );
    }
}

struct Slot {
    set: wgpu::QuerySet,
    resolved: Buffer,
    read: Buffer,
    state: Arc<AtomicU8>,
    last: Arc<[AtomicU64; 2]>,
    /// The number of the frame that last wrote this slot. Slots are read
    /// back in the order of their frames.
    frame: u64,
    /// When the render thread began that frame.
    began: Instant,
}

#[derive(Resource, Default)]
struct FrameStamps {
    timed: Option<bool>,
    /// The device is Apple's (Metal), so the `end` pass must read the
    /// views' main textures to wait for the frame's rendering.
    tie: bool,
    /// The frames that wrote timestamps so far.
    frames: u64,
    slots: Vec<Slot>,
    current: Option<usize>,
    started: Option<Instant>,
    pipelines: Option<Pipelines>,
}

/// The pipelines of the `begin` and `end` passes, and the buffer `end`
/// writes into.
#[derive(Clone)]
struct Pipelines {
    begin: wgpu::ComputePipeline,
    end: wgpu::ComputePipeline,
    end_layout: wgpu::BindGroupLayout,
    sink: Buffer,
}

impl FrameStamps {
    fn pipelines(&mut self, device: &RenderDevice) -> Pipelines {
        self.pipelines
            .get_or_insert_with(|| {
                let module =
                    device.create_and_validate_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("gpu_load"),
                        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
                    });
                let pipeline = |entry_point| {
                    device
                        .wgpu_device()
                        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                            label: Some("gpu_load"),
                            layout: None,
                            module: &module,
                            entry_point: Some(entry_point),
                            compilation_options: default(),
                            cache: None,
                        })
                };
                let end = pipeline("end");
                Pipelines {
                    begin: pipeline("begin"),
                    end_layout: end.get_bind_group_layout(0),
                    end,
                    sink: device.create_buffer(&BufferDescriptor {
                        label: Some("gpu_load_sink"),
                        size: SINK_BYTES,
                        usage: BufferUsages::STORAGE,
                        mapped_at_creation: false,
                    }),
                }
            })
            .clone()
    }

    fn free_slot(&mut self, device: &RenderDevice) -> Option<usize> {
        if let Some(index) =
            (self.slots.iter()).position(|slot| slot.state.load(Ordering::Acquire) == FREE)
        {
            return Some(index);
        }
        if self.slots.len() >= SLOTS {
            return None;
        }
        self.slots.push(Slot {
            set: device
                .wgpu_device()
                .create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("gpu_load"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 2,
                }),
            resolved: device.create_buffer(&BufferDescriptor {
                label: Some("gpu_load_resolved"),
                size: STAMP_BYTES,
                usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            read: device.create_buffer(&BufferDescriptor {
                label: Some("gpu_load_read"),
                size: STAMP_BYTES,
                usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            state: Arc::new(AtomicU8::new(FREE)),
            last: Arc::new([AtomicU64::new(0), AtomicU64::new(0)]),
            frame: 0,
            began: Instant::now(),
        });
        Some(self.slots.len() - 1)
    }
}

fn frame_begins(
    mut stamps: ResMut<FrameStamps>,
    load: Res<GpuLoad>,
    device: Res<RenderDevice>,
    adapter: Res<RenderAdapterInfo>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let now = Instant::now();
    if let Some(started) = stamps.started.replace(now) {
        let ms = now.duration_since(started).as_secs_f64() * 1000.0;
        load.with(|measured| {
            measured.render_ms += ms;
            measured.render_frames += 1;
        });
    }
    if stamps.timed.is_none() {
        stamps.timed = Some(device.features().contains(WgpuFeatures::TIMESTAMP_QUERY));
        stamps.tie = adapter.backend == wgpu::Backend::Metal;
    }
    let timed = stamps.timed == Some(true);
    load.with(|measured| measured.timed = timed);
    stamps.current = None;
    if !timed {
        return;
    }
    let Some(index) = stamps.free_slot(&device) else {
        load.with(|measured| measured.lost.unstamped += 1);
        return;
    };
    stamps.slots[index].began = now;
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("gpu_load_begin"),
    });
    let pipelines = stamps.pipelines(&device);
    let mut pass = stamp_pass(&mut encoder, &stamps.slots[index].set, 0);
    pass.set_pipeline(&pipelines.begin);
    pass.dispatch_workgroups(1, 1, 1);
    drop(pass);
    pending.push([encoder.finish()]);
    stamps.current = Some(index);
}

fn frame_ends(
    mut stamps: ResMut<FrameStamps>,
    load: Res<GpuLoad>,
    device: Res<RenderDevice>,
    views: Query<&ViewTarget>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let (Some(index), Some(pipelines)) = (stamps.current, stamps.pipelines.clone()) else {
        return;
    };
    let mut bind_groups: Vec<(TextureViewId, wgpu::BindGroup)> = Vec::new();
    if stamps.tie {
        for view in &views {
            let texture = view.main_texture_view();
            if !loadable(view.main_texture())
                || bind_groups.iter().any(|(id, _)| *id == texture.id())
            {
                continue;
            }
            let bind_group = device
                .wgpu_device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("gpu_load"),
                    layout: &pipelines.end_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(texture),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: pipelines.sink.as_entire_binding(),
                        },
                    ],
                });
            bind_groups.push((texture.id(), bind_group));
        }
        if bind_groups.is_empty() {
            stamps.slots[index].state.store(FREE, Ordering::Release);
            load.with(|measured| measured.lost.unstamped += 1);
            return;
        }
    }
    stamps.frames += 1;
    let frame = stamps.frames;
    let slot = &mut stamps.slots[index];
    slot.frame = frame;
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("gpu_load_end"),
    });
    let mut pass = stamp_pass(&mut encoder, &slot.set, 1);
    if bind_groups.is_empty() {
        pass.set_pipeline(&pipelines.begin);
        pass.dispatch_workgroups(1, 1, 1);
    } else {
        pass.set_pipeline(&pipelines.end);
        for (_, bind_group) in &bind_groups {
            pass.set_bind_group(0, bind_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
    }
    drop(pass);
    encoder.resolve_query_set(&slot.set, 0..2, &slot.resolved, 0);
    encoder.copy_buffer_to_buffer(&slot.resolved, 0, &slot.read, 0, STAMP_BYTES);
    pending.push([encoder.finish()]);
    slot.state.store(RECORDED, Ordering::Release);
}

/// Whether the `end` pass can read a texture. The texture must be
/// single-sampled and bound as a texture. Its format must give filterable
/// floats, since the pass's derived layout expects them.
fn loadable(texture: &wgpu::Texture) -> bool {
    texture.sample_count() == 1
        && texture.usage().contains(TextureUsages::TEXTURE_BINDING)
        && texture.format().sample_type(None, None)
            == Some(TextureSampleType::Float { filterable: true })
}

fn stamp_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    set: &'a wgpu::QuerySet,
    index: u32,
) -> wgpu::ComputePass<'a> {
    encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("gpu_load"),
        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
            query_set: set,
            beginning_of_pass_write_index: None,
            end_of_pass_write_index: Some(index),
        }),
    })
}

/// On Apple's GPUs a sample is sometimes not written or lands late, so a
/// reading is kept only when both timestamps changed and are in order.
/// A dropped reading leaves `last_end` alone, so one bad timestamp cannot
/// make the frames after it look stale.
fn read_back(stamps: Res<FrameStamps>, load: Res<GpuLoad>, queue: Res<RenderQueue>) {
    let period_ns = f64::from(queue.get_timestamp_period());
    let mut slots: Vec<&Slot> = stamps.slots.iter().collect();
    slots.sort_by_key(|slot| slot.frame);
    for slot in slots {
        if slot
            .state
            .compare_exchange(RECORDED, READING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            continue;
        }
        let read = slot.read.clone();
        let state = slot.state.clone();
        let last = slot.last.clone();
        let load = load.clone();
        let began = slot.began;
        slot.read.slice(..).map_async(MapMode::Read, move |result| {
            if result.is_err() {
                load.with(|measured| measured.lost.stale += 1);
            } else {
                let [begin, end] = {
                    let data = read.slice(..).get_mapped_range();
                    let at = |offset: usize| {
                        u64::from_le_bytes(data[offset..offset + 8].try_into().expect("8 bytes"))
                    };
                    [at(0), at(8)]
                };
                read.unmap();
                let was = [
                    last[0].swap(begin, Ordering::Relaxed),
                    last[1].swap(end, Ordering::Relaxed),
                ];
                let wall_ms = began.elapsed().as_secs_f64() * 1000.0;
                load.with(|measured| {
                    let reading = match was[0] != begin && was[1] != end {
                        true => reading([begin, end], measured.last_end, period_ns, wall_ms),
                        false => Reading::Stale,
                    };
                    match reading {
                        Reading::Kept(ms) => {
                            measured.last_end = end;
                            measured.gpu_ms.push(ms);
                        }
                        Reading::Stale => measured.lost.stale += 1,
                        Reading::Impossible => measured.lost.impossible += 1,
                    }
                });
            }
            state.store(FREE, Ordering::Release);
        });
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Reading {
    /// The frame's GPU ms.
    Kept(f64),
    Stale,
    Impossible,
}

/// A frame's GPU time from its two timestamps. It starts no earlier than
/// the end of the last frame kept (`last_end`). It cannot be longer than
/// the wall time from the start of the frame's rendering to its read back
/// (`wall_ms`), since the GPU did all of the frame's work within it.
pub(crate) fn reading(
    [begin, end]: [u64; 2],
    last_end: u64,
    period_ns: f64,
    wall_ms: f64,
) -> Reading {
    let start = begin.max(last_end);
    if end <= start {
        return Reading::Stale;
    }
    let ms = (end - start) as f64 * period_ns / 1.0e6;
    if ms > wall_ms {
        return Reading::Impossible;
    }
    Reading::Kept(ms)
}
