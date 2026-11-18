//! The run's vitals in every `dump`: the process's memory, the frames'
//! times since the last dump, the CPU each frame's main schedules and
//! render schedule took and the views it drew, and the assets alive, so a
//! long session's growth and stalls show in its dumps (`kjk bench` reads
//! them).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use bevy::core_pipeline::core_3d::{AlphaMask3d, Opaque3d, Transparent3d};
use bevy::core_pipeline::prepass::{AlphaMask3dPrepass, Opaque3dPrepass};
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::render::render_phase::{BinnedPhaseItem, ViewBinnedRenderPhases, ViewSortedRenderPhases};
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
    draws: [usize; 4],
    /// The render world's entities, as the last frame left them.
    entities: usize,
    /// How many of each kind of GPU object wgpu holds (buffers, textures,
    /// bind groups, …), counted every `GPU_OBJECTS_EVERY` render frames,
    /// and the render frames since.
    gpu_objects: Option<serde_json::Value>,
    since_gpu_objects: u32,
    times: VecDeque<f32>,
    graph_times: VecDeque<f32>,
    view_counts: VecDeque<f32>,
    opaque_counts: VecDeque<f32>,
    mask_counts: VecDeque<f32>,
    prepass_counts: VecDeque<f32>,
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

/// The draw calls of a binned phase, over all views. Each bin of batchable
/// meshes is one draw, with its instances batched. Each unbatched mesh is
/// one draw. Each multi-drawable batch set, which exists under indirect
/// drawing, is one multi-draw call. Before the prepass counters came, the
/// opaque count left the multi-drawable batch sets out.
fn binned_draws<P: BinnedPhaseItem>(phases: Option<Res<ViewBinnedRenderPhases<P>>>) -> usize {
    phases.map_or(0, |phases| {
        phases
            .values()
            .map(|phase| {
                phase.batchable_meshes.len()
                    + phase.multidrawable_meshes.len()
                    + phase
                        .unbatchable_meshes
                        .values()
                        .map(|unbatched| unbatched.entities.len())
                        .sum::<usize>()
            })
            .sum()
    })
}

/// The views drawn, and the draw calls of the main passes (opaque, alpha
/// mask), of the depth prepass (its opaque and alpha mask phases), and of
/// the see-through items, each drawn on its own.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_views(
    render: Res<RenderTimes>,
    views: Query<(), With<ExtractedView>>,
    opaque: Option<Res<ViewBinnedRenderPhases<Opaque3d>>>,
    mask: Option<Res<ViewBinnedRenderPhases<AlphaMask3d>>>,
    opaque_prepass: Option<Res<ViewBinnedRenderPhases<Opaque3dPrepass>>>,
    mask_prepass: Option<Res<ViewBinnedRenderPhases<AlphaMask3dPrepass>>>,
    transparent: Option<Res<ViewSortedRenderPhases<Transparent3d>>>,
    entities: Query<()>,
) {
    let transparent = transparent.map_or(0, |phases| {
        phases.values().map(|phase| phase.items.len()).sum()
    });
    let draws = [
        binned_draws(opaque),
        binned_draws(mask),
        binned_draws(opaque_prepass) + binned_draws(mask_prepass),
        transparent,
    ];
    if let Ok(mut samples) = render.0.lock() {
        samples.views = views.iter().count();
        samples.draws = draws;
        samples.entities = entities.iter().count();
    }
}

/// Render frames between two counts of wgpu's objects (`gpu_objects`).
const GPU_OBJECTS_EVERY: u32 = 30;

/// wgpu's objects that the program still holds, counted every
/// `GPU_OBJECTS_EVERY` render frames. A count that grows with every load or area change is a GPU object kept
/// alive by something that should have let it go (issue 445).
pub(crate) fn count_gpu_objects(
    render: Res<RenderTimes>,
    instance: Option<Res<bevy::render::renderer::RenderInstance>>,
) {
    let Ok(mut samples) = render.0.lock() else {
        return;
    };
    samples.since_gpu_objects += 1;
    if samples.gpu_objects.is_some() && samples.since_gpu_objects < GPU_OBJECTS_EVERY {
        return;
    }
    samples.since_gpu_objects = 0;
    let Some(report) = instance.and_then(|instance| instance.0.generate_report()) else {
        return;
    };
    let hub = &report.hub;
    let kinds = [
        ("buffers", &hub.buffers),
        ("textures", &hub.textures),
        ("texture_views", &hub.texture_views),
        ("samplers", &hub.samplers),
        ("bind_groups", &hub.bind_groups),
        ("bind_group_layouts", &hub.bind_group_layouts),
        ("pipeline_layouts", &hub.pipeline_layouts),
        ("render_pipelines", &hub.render_pipelines),
        ("compute_pipelines", &hub.compute_pipelines),
        ("shader_modules", &hub.shader_modules),
        ("query_sets", &hub.query_sets),
        ("command_encoders", &hub.command_encoders),
        ("command_buffers", &hub.command_buffers),
        ("render_bundles", &hub.render_bundles),
    ];
    let counts: serde_json::Map<String, serde_json::Value> = kinds
        .into_iter()
        .map(|(name, registry)| (name.to_string(), json!(registry.num_kept_from_user)))
        .collect();
    samples.gpu_objects = Some(serde_json::Value::Object(counts));
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
        let [opaque, mask, prepass, transparent] = samples.draws;
        keep(&mut samples.opaque_counts, opaque as f32);
        keep(&mut samples.mask_counts, mask as f32);
        keep(&mut samples.prepass_counts, prepass as f32);
        keep(&mut samples.transparent_counts, transparent as f32);
    }
}

/// When a dump is due, after the frame's main schedules: how many entities
/// have each component, how many archetypes and resources the world has,
/// and how many systems each schedule runs. A count that grows with every
/// load or area change shows something a session keeps that it should let
/// go (issue 445). It runs after `Last`, so that `Update` and the other
/// schedules `Main` runs are back in `Schedules` to be counted.
pub(crate) fn census(world: &mut World) {
    if !world.resource::<DumpDue>().0 {
        return;
    }
    let mut components = std::collections::BTreeMap::<String, u64>::new();
    for archetype in world.archetypes().iter() {
        let len = u64::from(archetype.len());
        if len == 0 {
            continue;
        }
        for &id in archetype.components() {
            let name = world
                .components()
                .get_name(id)
                .map_or_else(|| format!("{id:?}"), |name| name.to_string());
            *components.entry(name).or_default() += len;
        }
    }
    let archetypes = world.archetypes().len();
    let resources = world.iter_resources().count();
    let schedules: std::collections::BTreeMap<String, usize> = world
        .resource::<Schedules>()
        .iter()
        .map(|(label, schedule)| (format!("{label:?}"), schedule.systems_len()))
        .collect();
    let systems: usize = schedules.values().sum();
    let render = world.get_resource::<RenderTimes>().and_then(|render| {
        let samples = render.0.lock().ok()?;
        Some((samples.entities, samples.gpu_objects.clone()))
    });
    let render_entities = render.as_ref().map(|(entities, _)| *entities);
    let gpu_objects = render.and_then(|(_, objects)| objects);
    // The GPU memory wgpu's allocator has handed out and reserved.
    let gpu_memory = world
        .get_resource::<bevy::render::renderer::RenderDevice>()
        .and_then(|device| device.wgpu_device().generate_allocator_report())
        .map(|report| {
            json!({
                "allocations": report.allocations.len(),
                "blocks": report.blocks.len(),
                "allocated_mb": report.total_allocated_bytes as f64 / MB,
                "reserved_mb": report.total_reserved_bytes as f64 / MB,
            })
        });
    world.resource_mut::<DumpFacts>().0.insert(
        "census".into(),
        json!({
            "archetypes": archetypes,
            "resources": resources,
            "systems": systems,
            "schedules": schedules,
            "render_entities": render_entities,
            "gpu_objects": gpu_objects,
            "gpu_memory": gpu_memory,
            "components": components,
        }),
    );
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
    gpu: Option<ResMut<crate::gpu_times::GpuTimes>>,
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
    let [
        render_ms,
        graph_ms,
        views,
        opaque,
        mask,
        prepass,
        transparent,
    ] = match render.0.lock() {
        Ok(mut samples) => [
            spread(samples.times.drain(..).collect()),
            spread(samples.graph_times.drain(..).collect()),
            spread(samples.view_counts.drain(..).collect()),
            spread(samples.opaque_counts.drain(..).collect()),
            spread(samples.mask_counts.drain(..).collect()),
            spread(samples.prepass_counts.drain(..).collect()),
            spread(samples.transparent_counts.drain(..).collect()),
        ],
        Err(_) => std::array::from_fn(|_| json!(null)),
    };
    let main_ms = spread(main.times.drain(..).collect());
    let gpu = gpu.map_or(json!(null), |mut gpu| gpu.drain(spread));
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
            "mask_draws": mask,
            "prepass_draws": prepass,
            "transparent_draws": transparent,
            "gpu": gpu,
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

/// This process's memory now, where the platform says. On Windows the
/// footprint is the private bytes the system has committed for the process.
#[cfg(windows)]
pub fn memory() -> Option<Memory> {
    /// `PROCESS_MEMORY_COUNTERS_EX` (psapi.h).
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
        private_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            cb: u32,
        ) -> i32;
    }
    let mut counters = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Counters::default()
    };
    // SAFETY: the current process's pseudo handle needs no closing, and
    // `counters` is the structure its `cb` names, alive through the call.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    (ok != 0).then_some(Memory {
        footprint: counters.private_usage as u64,
        resident: counters.working_set_size as u64,
    })
}

/// This process's memory now, where the platform says.
#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
pub fn memory() -> Option<Memory> {
    None
}
