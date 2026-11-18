//! The GPU's time is taken by two timestamps a frame, each at the end of a
//! one-thread compute pass, queued before and after all of the frame's
//! rendering. Apple's GPUs are not timed. They take timestamps only at the
//! boundaries of a pass that does work, and they run successive command
//! buffers side by side, so the two passes do not enclose the frame.

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

const SLOTS: usize = 3;

const SHADER: &str = "@compute @workgroup_size(1) fn main() {}";

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
}

impl GpuLoad {
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

struct Slot {
    set: wgpu::QuerySet,
    resolved: Buffer,
    read: Buffer,
    state: Arc<AtomicU8>,
    last: Arc<[AtomicU64; 2]>,
}

#[derive(Resource, Default)]
struct FrameStamps {
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

/// On Apple's GPUs a sample is sometimes not written or lands late, so a
/// reading is kept only when both timestamps changed and are in order.
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
