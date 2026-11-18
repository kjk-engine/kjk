//! How busy the GPU is. This module measures the GPU's time for each frame
//! and the render thread's frame time. The GPU load check (`prepasses`)
//! compares the two.
//!
//! The GPU's time is measured with two timestamps a frame. Each timestamp
//! is written at the end of a compute pass of one thread that does nothing.
//! The first pass is in a command buffer queued before every other command
//! buffer of the frame. The second pass is in a command buffer queued after
//! the frame's rendering. So every pass of every camera and every shadow
//! view runs between the two timestamps. The second command buffer also
//! copies both values to a buffer that the CPU reads back one to three
//! frames later (`SLOTS`). The cost is two passes of one thread, a 16-byte
//! copy and one read back a frame. Bevy's own render diagnostics
//! (`KJK_GPU_TIMES=1`, `console::gpu_times`) time each pass and count
//! fragments. They stay off by default.
//!
//! Apple's GPUs (Metal) are not timed. They take timestamps only at the
//! boundaries of a pass that does some work. A timestamp written between
//! passes, or by an empty pass, reads 0 there. This is why Bevy's per-pass
//! timings read 0 on a Mac, and why this module does not reuse them. With
//! the one-thread passes, a Mac's readings came back, but they did not
//! measure the frame. In an arena fight at 3024x1964, the middle reading of
//! each quarter second ranged from 0.8 ms to 29 ms. Apple's GPUs run work
//! from successive command buffers side by side when no resource ties them
//! together, so the two passes do not enclose the frame's rendering. On
//! Vulkan and DirectX 12, a pass's timestamps wait for the work before it.
//! Some of a Mac's samples were also not written at all, so the slot still
//! held an older value. The checks in `read_back` drop such readings on
//! every device.
//!
//! The render thread's frame time is the time from the start of one
//! frame's rendering to the start of the next. It includes the time the
//! render thread waits for the main thread's next frame or for the GPU. So
//! when the GPU's time comes close to it, the GPU is what holds the frame
//! rate back.
//!
//! A device without timestamp queries (`TIMESTAMP_QUERY`), or Apple's,
//! measures nothing. `Measured::timed` then says so.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::prelude::*;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, MapMode, WgpuFeatures,
};
use bevy::render::renderer::{
    PendingCommandBuffers, RenderAdapterInfo, RenderDevice, RenderGraph, RenderGraphSystems,
    RenderQueue,
};
use bevy::render::{Render, RenderApp, RenderSystems};

/// How many frames of timestamps can be in flight at once.
const SLOTS: usize = 3;

/// The shader of the passes that write the timestamps. It runs one thread
/// that does nothing.
const SHADER: &str = "@compute @workgroup_size(1) fn main() {}";

/// A reading longer than this many of the render thread's frames spans
/// several frames, so it is dropped. A frame's GPU time can be longer than
/// its render frame only while frames queue up behind the GPU.
const MAX_FRAMES_SPANNED: f64 = 2.0;

/// The bytes of a slot's two timestamps.
const STAMP_BYTES: u64 = 16;

/// A slot that no frame uses.
const FREE: u8 = 0;
/// A slot whose timestamps this frame wrote and that waits to be read.
const RECORDED: u8 = 1;
/// A slot whose timestamps are being read back.
const READING: u8 = 2;

/// The frames measured since the main world last took them, shared by the
/// render thread, which fills it, and the main thread, which takes it.
#[derive(Resource, Clone, Default)]
pub(crate) struct GpuLoad(Arc<Mutex<Measured>>);

/// What was measured since the last `take`.
#[derive(Default, Clone, Debug)]
pub(crate) struct Measured {
    /// The device can time the GPU's frames.
    pub timed: bool,
    /// The GPU's ms of each frame whose reading was kept.
    pub gpu_ms: Vec<f64>,
    /// The render thread's frame times added up, and how many there were.
    pub render_ms: f64,
    pub render_frames: u32,
    /// The render thread's last frame time. It is kept across `take`.
    last_render_ms: f64,
}

impl GpuLoad {
    /// The frames measured since the last call, and a fresh start.
    pub(crate) fn take(&self) -> Measured {
        let Ok(mut measured) = self.0.lock() else {
            return Measured::default();
        };
        let kept = Measured {
            timed: measured.timed,
            last_render_ms: measured.last_render_ms,
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

/// One frame's two timestamps and the buffers they go through.
struct Slot {
    set: wgpu::QuerySet,
    resolved: Buffer,
    read: Buffer,
    state: Arc<AtomicU8>,
    /// The two timestamps this slot last read back.
    last: Arc<[AtomicU64; 2]>,
}

/// The render world's side of the measurement. It holds the slots, the slot
/// this frame writes, and when the last frame's rendering started.
#[derive(Resource, Default)]
struct FrameStamps {
    /// The device has timestamp queries and is not Apple's, checked once.
    timed: Option<bool>,
    slots: Vec<Slot>,
    current: Option<usize>,
    started: Option<Instant>,
    pipeline: Option<wgpu::ComputePipeline>,
}

impl FrameStamps {
    fn pipeline(&mut self, device: &RenderDevice) -> wgpu::ComputePipeline {
        self.pipeline
            .get_or_insert_with(|| {
                let module =
                    device.create_and_validate_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some("gpu_load"),
                        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
                    });
                device
                    .wgpu_device()
                    .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some("gpu_load"),
                        layout: None,
                        module: &module,
                        entry_point: Some("main"),
                        compilation_options: default(),
                        cache: None,
                    })
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

/// Notes the render thread's frame time, and writes the first timestamp
/// in a command buffer queued before every other one of the frame.
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
            measured.last_render_ms = ms;
        });
    }
    let timed = *stamps.timed.get_or_insert_with(|| {
        device.features().contains(WgpuFeatures::TIMESTAMP_QUERY)
            && adapter.backend != wgpu::Backend::Metal
    });
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
    let pipeline = stamps.pipeline(&device);
    stamp(&mut encoder, &pipeline, &stamps.slots[index].set, 0);
    pending.push([encoder.finish()]);
    stamps.current = Some(index);
}

/// Writes the second timestamp after the frame's rendering, and copies
/// both where the CPU can read them.
fn frame_ends(
    stamps: Res<FrameStamps>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let Some(index) = stamps.current else {
        return;
    };
    let slot = &stamps.slots[index];
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("gpu_load_end"),
    });
    let Some(pipeline) = &stamps.pipeline else {
        return;
    };
    stamp(&mut encoder, pipeline, &slot.set, 1);
    encoder.resolve_query_set(&slot.set, 0..2, &slot.resolved, 0);
    encoder.copy_buffer_to_buffer(&slot.resolved, 0, &slot.read, 0, STAMP_BYTES);
    pending.push([encoder.finish()]);
    slot.state.store(RECORDED, Ordering::Release);
}

/// Writes timestamp `index` at the end of a compute pass of one thread.
fn stamp(
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    set: &wgpu::QuerySet,
    index: u32,
) {
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("gpu_load"),
        timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
            query_set: set,
            beginning_of_pass_write_index: None,
            end_of_pass_write_index: Some(index),
        }),
    });
    pass.set_pipeline(pipeline);
    pass.dispatch_workgroups(1, 1, 1);
}

/// Once the frame is submitted, reads back the slot it wrote. On Apple's
/// GPUs a pass's sample is sometimes not written, and the slot then reads
/// the timestamp it held before. A sample can also land late, after its
/// frame's copy, and then shows up in the slot's next reading. A reading is
/// kept only when both timestamps changed since the slot's last reading and
/// the second is after the first. It must also be shorter than
/// `MAX_FRAMES_SPANNED` render frames, since a longer one spans frames.
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
                    let ms = (end - begin) as f64 * period_ns / 1.0e6;
                    load.with(|measured| {
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
