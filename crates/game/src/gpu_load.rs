//! How busy the GPU is. This module measures the GPU's time for each frame
//! and the render thread's frame time. The GPU load check (`prepasses`)
//! compares the two.
//!
//! The GPU's time is measured with two timestamps a frame. Each timestamp
//! is written at the end of a compute pass of one thread. The first pass
//! does nothing. It is in a command buffer queued before every other
//! command buffer of the frame. The second pass reads one texel of each
//! view's main texture, the texture the frame's main and post-processing
//! passes draw into. It is in a command buffer queued after the frame's
//! rendering. The texel read ties the second pass to the rendering. The
//! GPU starts it only once the passes that draw into the main texture have
//! ended. The second command buffer also copies both values to a buffer
//! that the CPU reads back one to three frames later (`SLOTS`). The cost is
//! two passes of one thread, a 16-byte copy and one read back a frame.
//! Bevy's own render diagnostics (`KJK_GPU_TIMES=1`, `console::gpu_times`)
//! time each pass and count fragments. They stay off by default.
//!
//! A frame's GPU time is the time from its first timestamp to its second.
//! The GPU can still be busy with the frame before when the first pass
//! runs. The frame's time then starts at the end of the frame before, so
//! no GPU time counts twice. On Vulkan and DirectX 12, a pass's timestamps
//! wait for the work queued before it, so this never happens there.
//!
//! On Apple's GPUs (Metal), a timestamp can be taken only at the boundary
//! of a pass that does some work. A timestamp written between passes reads
//! 0 there. This is why Bevy's per-pass timings read 0 on a Mac, and why
//! this module does not reuse them. Apple's GPUs also run work from
//! successive command buffers side by side when no resource ties them
//! together. An earlier version of this module ended the frame with a pass
//! that read nothing. On a Mac, that pass ran before the frame's rendering
//! had ended, and its readings ranged from 0.8 ms to 29 ms within one
//! fight. The texel read is what makes the Mac's readings measure the
//! frame. Some of a Mac's samples were also not written at all, so the
//! slot still held an older value. The checks in `read_back` drop such
//! readings on every device.
//!
//! The render thread's frame time is the time from the start of one
//! frame's rendering to the start of the next. It includes the time the
//! render thread waits for the main thread's next frame or for the GPU. So
//! when the GPU's time comes close to it, the GPU is what holds the frame
//! rate back.
//!
//! A device without timestamp queries (`TIMESTAMP_QUERY`) measures
//! nothing. `Measured::timed` then says so.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::prelude::*;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, MapMode, TextureSampleType,
    TextureUsages, WgpuFeatures,
};
use bevy::render::renderer::{
    PendingCommandBuffers, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue,
};
use bevy::render::view::ViewTarget;
use bevy::render::{Render, RenderApp, RenderSystems};

const SLOTS: usize = 3;

/// The shader of the passes that write the timestamps. Each runs one
/// thread. `begin` does nothing. `end` reads one texel of a view's main
/// texture, so it waits for the passes that draw into it.
const SHADER: &str = "
@group(0) @binding(0) var frame: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> sink: vec4<f32>;
@compute @workgroup_size(1) fn begin() {}
@compute @workgroup_size(1) fn end() { sink = textureLoad(frame, vec2<i32>(0, 0), 0); }
";

/// The bytes of the buffer the `end` pass writes its texel into.
const SINK_BYTES: u64 = 16;

const MAX_FRAMES_SPANNED: f64 = 2.0;

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
    last_render_ms: f64,
    /// The second timestamp of the last reading kept. A frame's GPU time
    /// starts no earlier than this. It is kept across `take`.
    last_end: u64,
}

impl GpuLoad {
    pub(crate) fn take(&self) -> Measured {
        let Ok(mut measured) = self.0.lock() else {
            return Measured::default();
        };
        let kept = Measured {
            timed: measured.timed,
            last_render_ms: measured.last_render_ms,
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
}

#[derive(Resource, Default)]
struct FrameStamps {
    /// The device has timestamp queries, checked once.
    timed: Option<bool>,
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
                Pipelines {
                    begin: pipeline("begin"),
                    end: pipeline("end"),
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
        });
        Some(self.slots.len() - 1)
    }
}

fn frame_begins(
    mut stamps: ResMut<FrameStamps>,
    load: Res<GpuLoad>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let now = Instant::now();
    if let Some(started) = stamps.started.replace(now) {
        let ms = now.duration_since(started).as_secs_f64() * 1000.0;
        load.with(|measured| {
            measured.render_ms += ms;
            measured.render_frames += 1;
            measured.last_render_ms = ms;
        });
    }
    let timed = *stamps
        .timed
        .get_or_insert_with(|| device.features().contains(WgpuFeatures::TIMESTAMP_QUERY));
    load.with(|measured| measured.timed = timed);
    stamps.current = None;
    if !timed {
        return;
    }
    let Some(index) = stamps.free_slot(&device) else {
        return;
    };
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

/// Writes the second timestamp after the frame's rendering, and copies
/// both where the CPU can read them. The pass reads one texel of each
/// view's main texture, so it waits for the passes that draw into them.
fn frame_ends(
    stamps: Res<FrameStamps>,
    device: Res<RenderDevice>,
    views: Query<&ViewTarget>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let (Some(index), Some(pipelines)) = (stamps.current, &stamps.pipelines) else {
        return;
    };
    let slot = &stamps.slots[index];
    let bind_groups: Vec<wgpu::BindGroup> = views
        .iter()
        .filter(|view| loadable(view.main_texture()))
        .map(|view| {
            device
                .wgpu_device()
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("gpu_load"),
                    layout: &pipelines.end.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(view.main_texture_view()),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: pipelines.sink.as_entire_binding(),
                        },
                    ],
                })
        })
        .collect();
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("gpu_load_end"),
    });
    let mut pass = stamp_pass(&mut encoder, &slot.set, 1);
    if bind_groups.is_empty() {
        pass.set_pipeline(&pipelines.begin);
        pass.dispatch_workgroups(1, 1, 1);
    } else {
        pass.set_pipeline(&pipelines.end);
        for bind_group in &bind_groups {
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

/// A texture the `end` pass can read: single-sampled, bound as a texture,
/// and of a format a `texture_2d<f32>` reads with a filtering sampler.
fn loadable(texture: &wgpu::Texture) -> bool {
    texture.sample_count() == 1
        && texture.usage().contains(TextureUsages::TEXTURE_BINDING)
        && texture.format().sample_type(None, None)
            == Some(TextureSampleType::Float { filterable: true })
}

/// Begins a compute pass that writes timestamp `index` at its end.
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

/// Once the frame is submitted, reads back the slot it wrote. On Apple's
/// GPUs a pass's sample is sometimes not written, and the slot then reads
/// the timestamp it held before. A sample can also land late, after its
/// frame's copy, and then shows up in the slot's next reading. A reading is
/// kept only when both timestamps changed since the slot's last reading and
/// the second is after the first. The frame's time starts at its first
/// timestamp, or at the second timestamp of the frame before when that is
/// later. It must also be shorter than `MAX_FRAMES_SPANNED` render frames,
/// since a longer one spans frames.
fn read_back(stamps: Res<FrameStamps>, load: Res<GpuLoad>, queue: Res<RenderQueue>) {
    let period_ns = f64::from(queue.get_timestamp_period());
    for slot in &stamps.slots {
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
        slot.read.slice(..).map_async(MapMode::Read, move |result| {
            if result.is_ok() {
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
                if was[0] != begin && was[1] != end && end > begin {
                    load.with(|measured| {
                        let start = begin.max(measured.last_end);
                        if end <= start {
                            return;
                        }
                        measured.last_end = end;
                        let ms = (end - start) as f64 * period_ns / 1.0e6;
                        let longest = MAX_FRAMES_SPANNED * measured.last_render_ms;
                        if measured.last_render_ms > 0.0 && ms < longest {
                            measured.gpu_ms.push(ms);
                        }
                    });
                }
            }
            state.store(FREE, Ordering::Release);
        });
    }
}
