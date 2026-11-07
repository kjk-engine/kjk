//! The run's vitals in every `dump`: the process's memory, the frames'
//! times since the last dump, the CPU each frame's main schedules and
//! render schedule took and the views it drew, and the assets alive, so a
//! long session's growth and stalls show in its dumps (`kjk bench` reads
//! them).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::core_pipeline::core_3d::{Opaque3d, Transparent3d};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::render::render_phase::{ViewBinnedRenderPhases, ViewSortedRenderPhases};
use bevy::render::view::ExtractedView;
use serde_json::json;

use crate::{DumpDue, DumpFacts};

/// Frame times kept: every frame since the last dump, up to the last 8 s
/// at 500 Hz.
const FRAMES_KEPT: usize = 4000;
/// A frame this long or longer is a hitch.
const HITCH_MS: f32 = 33.4;

/// The frames' times (ms) since the last dump.
#[derive(Resource, Default)]
pub(crate) struct FrameTimes(VecDeque<f32>);

pub(crate) fn time_frames(time: Res<Time<Real>>, mut frames: ResMut<FrameTimes>) {
    keep(&mut frames.0, time.delta_secs() * 1000.0);
}

fn keep(values: &mut VecDeque<f32>, value: f32) {
    if values.len() == FRAMES_KEPT {
        values.pop_front();
    }
    values.push_back(value);
}

/// Runs before `First`: a frame's main schedules start.
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub(crate) struct FrameStart;

/// Runs after `Last`: a frame's main schedules are done.
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub(crate) struct FrameEnd;

/// The CPU (ms) each frame's main schedules took, since the last dump,
/// and when this frame's started.
#[derive(Resource, Default)]
pub(crate) struct MainTimes {
    started: Option<Instant>,
    times: VecDeque<f32>,
}

pub(crate) fn main_started(mut main: ResMut<MainTimes>) {
    main.started = Some(Instant::now());
}

pub(crate) fn main_ended(mut main: ResMut<MainTimes>) {
    if let Some(started) = main.started.take() {
        keep(&mut main.times, started.elapsed().as_secs_f32() * 1000.0);
    }
}

/// The render schedule's CPU (ms) and views per frame, filled on the
/// render thread, read on the main one.
#[derive(Resource, Clone, Default)]
pub(crate) struct RenderTimes(Arc<Mutex<RenderSamples>>);

#[derive(Default)]
pub(crate) struct RenderSamples {
    started: Option<Instant>,
    graph_started: Option<Instant>,
    graph: f32,
    views: usize,
    draws: (usize, usize),
    times: VecDeque<f32>,
    graph_times: VecDeque<f32>,
    view_counts: VecDeque<f32>,
    opaque_counts: VecDeque<f32>,
    transparent_counts: VecDeque<f32>,
}

/// Before Bevy's `render_system`: the graph's encoding, the submit and
/// the present start.
pub(crate) fn graph_started(render: Res<RenderTimes>) {
    if let Ok(mut samples) = render.0.lock() {
        samples.graph_started = Some(Instant::now());
    }
}

pub(crate) fn graph_ended(render: Res<RenderTimes>) {
    if let Ok(mut samples) = render.0.lock()
        && let Some(started) = samples.graph_started.take()
    {
        samples.graph = started.elapsed().as_secs_f32() * 1000.0;
    }
}

pub(crate) fn render_started(render: Res<RenderTimes>) {
    if let Ok(mut samples) = render.0.lock() {
        samples.started = Some(Instant::now());
    }
}

/// The views drawn, and the main passes' draws: opaque bins (each one
/// draw, its instances batched) and unbatched opaque meshes, and the
/// see-through items, each drawn on its own.
pub(crate) fn render_views(
    render: Res<RenderTimes>,
    views: Query<(), With<ExtractedView>>,
    opaque: Option<Res<ViewBinnedRenderPhases<Opaque3d>>>,
    transparent: Option<Res<ViewSortedRenderPhases<Transparent3d>>>,
) {
    let opaque = opaque.map_or(0, |phases| {
        phases
            .values()
            .map(|phase| {
                phase.batchable_meshes.len()
                    + phase
                        .unbatchable_meshes
                        .values()
                        .map(|unbatched| unbatched.entities.len())
                        .sum::<usize>()
            })
            .sum()
    });
    let transparent = transparent.map_or(0, |phases| {
        phases.values().map(|phase| phase.items.len()).sum()
    });
    if let Ok(mut samples) = render.0.lock() {
        samples.views = views.iter().count();
        samples.draws = (opaque, transparent);
    }
}

pub(crate) fn render_ended(render: Res<RenderTimes>) {
    if let Ok(mut samples) = render.0.lock()
        && let Some(started) = samples.started.take()
    {
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        let views = samples.views as f32;
        let graph = std::mem::take(&mut samples.graph);
        keep(&mut samples.graph_times, graph);
        keep(&mut samples.times, ms);
        keep(&mut samples.view_counts, views);
        let (opaque, transparent) = samples.draws;
        keep(&mut samples.opaque_counts, opaque as f32);
        keep(&mut samples.transparent_counts, transparent as f32);
    }
}

/// Mean, p50, p95, p99 and max of `values`.
fn spread(mut values: Vec<f32>) -> serde_json::Value {
    if values.is_empty() {
        return json!(null);
    }
    values.sort_by(f32::total_cmp);
    let at = |q: f32| values[((values.len() as f32 - 1.0) * q).round() as usize];
    let mean = values.iter().sum::<f32>() / values.len() as f32;
    json!({
        "mean": mean, "p50": at(0.5), "p95": at(0.95), "p99": at(0.99),
        "max": values[values.len() - 1], "n": values.len(),
    })
}

/// When a dump is due: the vitals into its facts, and the frames' times
/// start again.
#[allow(clippy::too_many_arguments)]
pub(crate) fn publish(
    due: Res<DumpDue>,
    mut facts: ResMut<DumpFacts>,
    mut frames: ResMut<FrameTimes>,
    mut main: ResMut<MainTimes>,
    render: Res<RenderTimes>,
    images: Option<Res<Assets<Image>>>,
    meshes: Option<Res<Assets<Mesh>>>,
    cameras: Query<(
        &Camera,
        Option<&bevy::camera::visibility::RenderLayers>,
        Has<Camera3d>,
    )>,
) {
    if !due.0 {
        return;
    }
    let mut cameras: Vec<serde_json::Value> = cameras
        .iter()
        .map(|(camera, layers, three_d)| {
            json!({
                "order": camera.order,
                "active": camera.is_active,
                "3d": three_d,
                "layers": layers.map(|layers| layers.iter().collect::<Vec<_>>()),
            })
        })
        .collect();
    cameras.sort_by_key(|camera| camera["order"].as_i64());
    let (render_ms, graph_ms, views, opaque, transparent) = match render.0.lock() {
        Ok(mut samples) => (
            spread(samples.times.drain(..).collect()),
            spread(samples.graph_times.drain(..).collect()),
            spread(samples.view_counts.drain(..).collect()),
            spread(samples.opaque_counts.drain(..).collect()),
            spread(samples.transparent_counts.drain(..).collect()),
        ),
        Err(_) => (
            json!(null),
            json!(null),
            json!(null),
            json!(null),
            json!(null),
        ),
    };
    let main_ms = spread(main.times.drain(..).collect());
    let mut times: Vec<f32> = frames.0.drain(..).collect();
    let mean = (!times.is_empty()).then(|| times.iter().sum::<f32>() / times.len() as f32);
    times.sort_by(f32::total_cmp);
    let at = |q: f32| {
        times
            .get(((times.len() as f32 - 1.0) * q).round() as usize)
            .copied()
    };
    let image_bytes: usize = images.as_ref().map_or(0, |images| {
        images
            .iter()
            .filter_map(|(_, image)| image.data.as_ref().map(Vec::len))
            .sum()
    });
    let memory = memory();
    facts.0.insert(
        "vitals".into(),
        json!({
            "footprint_mb": memory.map(|m| m.footprint as f64 / MB),
            "resident_mb": memory.map(|m| m.resident as f64 / MB),
            "frames": times.len(),
            "frame_ms": {
                "mean": mean,
                "p50": at(0.5), "p95": at(0.95), "p99": at(0.99), "max": times.last(),
                "hitches": times.iter().filter(|&&ms| ms >= HITCH_MS).count(),
            },
            "images": images.as_ref().map_or(0, |images| images.len()),
            "image_cpu_mb": image_bytes as f64 / MB,
            "meshes": meshes.as_ref().map_or(0, |meshes| meshes.len()),
            "main_ms": main_ms,
            "render_ms": render_ms,
            "graph_ms": graph_ms,
            "views": views,
            "opaque_draws": opaque,
            "transparent_draws": transparent,
            "cameras": cameras,
        }),
    );
}

const MB: f64 = 1024.0 * 1024.0;

/// The process's memory, in bytes.
#[derive(Clone, Copy)]
pub struct Memory {
    /// What the system charges the process (macOS: the physical
    /// footprint, GPU allocations included; elsewhere the resident set).
    pub footprint: u64,
    pub resident: u64,
}

/// This process's memory now, where the platform says.
#[cfg(target_os = "macos")]
pub fn memory() -> Option<Memory> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v2>::zeroed();
    // SAFETY: `info` is a rusage_info_v2, the flavour asked for, and lives
    // through the call.
    let ok = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as i32,
            libc::RUSAGE_INFO_V2,
            info.as_mut_ptr().cast(),
        )
    } == 0;
    // SAFETY: the call filled it.
    let info = ok.then(|| unsafe { info.assume_init() })?;
    Some(Memory {
        footprint: info.ri_phys_footprint,
        resident: info.ri_resident_size,
    })
}

/// This process's memory now, where the platform says.
#[cfg(target_os = "linux")]
pub fn memory() -> Option<Memory> {
    // In kilobytes, whatever the page size.
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?;
    let resident = line.split_whitespace().next()?.parse::<u64>().ok()? * 1024;
    Some(Memory {
        footprint: resident,
        resident,
    })
}

/// This process's memory now, where the platform says.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn memory() -> Option<Memory> {
    None
}
