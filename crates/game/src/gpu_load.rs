//! How busy the GPU is: its time for each frame against the render
//! thread's frame time. The depth prepass's check (`prepasses`) reads it.
//!
//! The GPU's time is measured with two timestamps a frame. Each is written
//! by a compute pass of one thread that does nothing, in a command buffer
//! of its own. The first pass is queued before every other command buffer
//! of the frame, and the second after the frame's rendering, so every pass
//! of every camera and every shadow view runs between the two. Both
//! timestamps are taken at the end of their pass. The two values are
//! copied to a buffer that the CPU reads back one to three frames later
//! (`SLOTS`). That is all it costs: two passes of one thread, a 16-byte copy
//! and one read back a frame. Bevy's own render diagnostics
//! (`KJK_GPU_TIMES=1`, `console::gpu_times`) time each pass and count
//! fragments, and they stay off by default.
//!
//! Apple's GPUs take timestamps only at a pass's boundaries. A timestamp
//! written between passes, or by a pass with no work, reads 0 there, so
//! Bevy's per-pass timings read 0 on a Mac. Some of their samples are not
//! written at all, so a reading that repeats a slot's last one is dropped
//! (`read_back`). About a quarter of a Mac's frames are kept, which is
//! plenty for an average over half a second.
//!
//! The render thread's frame time is the time from the start of one
//! frame's rendering to the start of the next. It includes the time the
//! render thread waits, for the main thread's next frame or for the GPU.
//! So when the GPU's time comes close to it, the GPU is what holds the
//! frame rate back.
//!
//! A device without timestamp queries (`TIMESTAMP_QUERY`) measures nothing,
//! and `Measured::timed` says so.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::prelude::*;
use bevy::render::render_resource::{
    Buffer, BufferDescriptor, BufferUsages, CommandEncoderDescriptor, MapMode, WgpuFeatures,
};
use bevy::render::renderer::{
    PendingCommandBuffers, RenderDevice, RenderGraph, RenderGraphSystems, RenderQueue,
};
use bevy::render::{Render, RenderApp, RenderSystems};

/// How many frames of timestamps can be in flight at once.
const SLOTS: usize = 3;

/// The shader of the passes that write the timestamps: one thread that does
/// nothing.
const SHADER: &str = "@compute @workgroup_size(1) fn main() {}";

const FREE: u8 = 0;
const RECORDED: u8 = 1;
const READING: u8 = 2;

/// The frames measured since the main world last took them, shared by the
/// render thread, which fills it, and the main thread, which takes it.
#[derive(Resource, Clone, Default)]
pub(crate) struct GpuLoad(Arc<Mutex<Measured>>);

/// Sums over the frames measured since the last `take`.
#[derive(Default, Clone, Copy, Debug)]
pub(crate) struct Measured {
    /// The device can time the GPU's frames.
    pub timed: bool,
    pub gpu_ms: f64,
    pub gpu_frames: u32,
    pub render_ms: f64,
    pub render_frames: u32,
}

impl GpuLoad {
    /// The frames measured since the last call, and a fresh start.
    pub(crate) fn take(&self) -> Measured {
        let Ok(mut measured) = self.0.lock() else {
            return Measured::default();
        };
        let taken = *measured;
        *measured = Measured {
            timed: taken.timed,
            ..Measured::default()
        };
        taken
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
            .init_resource::<Timer>()
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

/// The render world's side: the slots, the slot this frame writes, and when
/// the last frame's rendering started.
#[derive(Resource, Default)]
struct Timer {
    /// The device has timestamp queries, checked once.
    timed: Option<bool>,
    slots: Vec<Slot>,
    current: Option<usize>,
    started: Option<Instant>,
    pipeline: Option<wgpu::ComputePipeline>,
}

impl Timer {
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
                size: 16,
                usage: BufferUsages::QUERY_RESOLVE | BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            }),
            read: device.create_buffer(&BufferDescriptor {
                label: Some("gpu_load_read"),
                size: 16,
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
    mut timer: ResMut<Timer>,
    load: Res<GpuLoad>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let now = Instant::now();
    if let Some(started) = timer.started.replace(now) {
        let ms = now.duration_since(started).as_secs_f64() * 1000.0;
        load.with(|measured| {
            measured.render_ms += ms;
            measured.render_frames += 1;
        });
    }
    let timed = *timer
        .timed
        .get_or_insert_with(|| device.features().contains(WgpuFeatures::TIMESTAMP_QUERY));
    load.with(|measured| measured.timed = timed);
    timer.current = None;
    if !timed {
        return;
    }
    let Some(index) = timer.free_slot(&device) else {
        return;
    };
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("gpu_load_begin"),
    });
    let pipeline = timer.pipeline(&device);
    stamp(&mut encoder, &pipeline, &timer.slots[index].set, 0);
    pending.push([encoder.finish()]);
    timer.current = Some(index);
}

/// Writes the second timestamp after the frame's rendering, and copies
/// both where the CPU can read them.
fn frame_ends(
    timer: Res<Timer>,
    device: Res<RenderDevice>,
    mut pending: ResMut<PendingCommandBuffers>,
) {
    let Some(index) = timer.current else {
        return;
    };
    let slot = &timer.slots[index];
    let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("gpu_load_end"),
    });
    let Some(pipeline) = &timer.pipeline else {
        return;
    };
    stamp(&mut encoder, pipeline, &slot.set, 1);
    let mut copy = device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("gpu_load_copy"),
    });
    copy.resolve_query_set(&slot.set, 0..2, &slot.resolved, 0);
    copy.copy_buffer_to_buffer(&slot.resolved, 0, &slot.read, 0, 16);
    pending.push([encoder.finish(), copy.finish()]);
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
/// the timestamp it held before. A reading is kept only when both
/// timestamps changed since the slot's last reading and the second is
/// after the first.
fn read_back(timer: Res<Timer>, load: Res<GpuLoad>, queue: Res<RenderQueue>) {
    let period_ns = f64::from(queue.get_timestamp_period());
    for slot in &timer.slots {
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
                        measured.gpu_ms += ms;
                        measured.gpu_frames += 1;
                    });
                }
            }
            state.store(FREE, Ordering::Release);
        });
    }
}
