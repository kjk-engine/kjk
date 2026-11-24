//! The GPU's time per render pass, measured when `KJK_GPU_TIMES=1` is set.
//! Bevy's render diagnostics put timestamp queries around its main passes.
//! They also count the passes' fragment shader invocations where the
//! adapter counts them. Bevy's shadow-map systems time nothing themselves,
//! so a span of our own goes around them. Each frame's measurements are
//! summed per pass over every camera and every light view. A dump's vitals
//! then give each pass's GPU milliseconds per frame. The timings are off
//! by default. The queries cost a little, and an older build has none, so
//! a bench comparing builds leaves them off.

use std::collections::{HashMap, VecDeque};
use std::time::Instant;

use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::diagnostic::DiagnosticsStore;
use bevy::pbr::{EARLY_SHADOW_PASS, per_view_shadow_pass, shared_shadow_pass};
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::diagnostic::{DiagnosticsRecorder, RecordDiagnostics, RenderDiagnosticsPlugin};
use bevy::render::renderer::{PendingCommandBuffers, RenderDevice};
use serde_json::json;

/// The span our shadow systems open and close.
const SHADOWS: &str = "shadows";
/// Frames kept per pass between dumps, as `vitals` keeps frame times.
const FRAMES_KEPT: usize = 4000;

/// Whether this run times its passes on the GPU.
pub fn wanted() -> bool {
    std::env::var("KJK_GPU_TIMES").is_ok_and(|value| value == "1")
}

pub(crate) struct GpuTimesPlugin;

impl Plugin for GpuTimesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(RenderDiagnosticsPlugin)
            .init_resource::<GpuTimes>()
            .add_systems(Update, collect);
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            // The two systems are exclusive. So the span opens and closes on
            // the one thread that runs the schedule, whichever executor runs
            // it. The recorder pairs a span's ends per thread.
            render.add_systems(
                Core3d,
                (
                    shadows_begin
                        .before(per_view_shadow_pass::<EARLY_SHADOW_PASS>)
                        .before(shared_shadow_pass::<EARLY_SHADOW_PASS>),
                    shadows_end
                        .after(per_view_shadow_pass::<EARLY_SHADOW_PASS>)
                        .after(shared_shadow_pass::<EARLY_SHADOW_PASS>)
                        .before(Core3dSystems::MainPass),
                ),
            );
        }
    }
}

fn shadows_begin(world: &mut World) {
    stamp(world, true);
}

fn shadows_end(world: &mut World) {
    stamp(world, false);
}

/// A timestamp written in a command buffer of its own, queued after the
/// systems before this one and before those after it.
fn stamp(world: &mut World, begin: bool) {
    let Some(recorder) = world.get_resource::<DiagnosticsRecorder>() else {
        return;
    };
    let mut encoder = world
        .resource::<RenderDevice>()
        .create_command_encoder(&default());
    if begin {
        recorder.begin_time_span(&mut encoder, SHADOWS.into());
    } else {
        recorder.end_time_span(&mut encoder);
    }
    world
        .resource_mut::<PendingCommandBuffers>()
        .push_encoder(encoder);
}

/// Each pass's GPU ms, and the main passes' fragment invocations, per
/// frame since the last dump.
#[derive(Resource, Default)]
pub(crate) struct GpuTimes {
    /// The newest measurement taken in, per diagnostic path.
    seen: HashMap<String, Instant>,
    ms: HashMap<String, VecDeque<f32>>,
    fragments: HashMap<String, VecDeque<f32>>,
    /// The top-level spans' ms added up, per frame.
    spans: VecDeque<f32>,
}

/// A frame's diagnostics arrive in one batch, all stamped with the same
/// instant. Each path's measurements at its newest instant are summed once.
fn collect(store: Option<Res<DiagnosticsStore>>, mut times: ResMut<GpuTimes>) {
    let Some(store) = store else { return };
    let mut spans = None;
    for diagnostic in store.iter() {
        let path = diagnostic.path().as_str();
        let Some(inner) = path.strip_prefix("render/") else {
            continue;
        };
        let (pass, kind) = match inner.rsplit_once('/') {
            Some((pass, "elapsed_gpu")) => (pass, Kind::Ms),
            Some((pass, "fragment_shader_invocations")) => (pass, Kind::Fragments),
            _ => continue,
        };
        let Some(newest) = diagnostic.measurements().map(|m| m.time).max() else {
            continue;
        };
        if times.seen.get(path) == Some(&newest) {
            continue;
        }
        times.seen.insert(path.to_string(), newest);
        let sum: f64 = diagnostic
            .measurements()
            .filter(|m| m.time == newest)
            .map(|m| m.value)
            .sum();
        let kept = match kind {
            Kind::Ms => &mut times.ms,
            Kind::Fragments => &mut times.fragments,
        };
        keep(kept.entry(pass.to_string()).or_default(), sum as f32);
        if matches!(kind, Kind::Ms) && !pass.contains('/') {
            *spans.get_or_insert(0.0) += sum as f32;
        }
    }
    if let Some(spans) = spans {
        keep(&mut times.spans, spans);
    }
}

enum Kind {
    Ms,
    Fragments,
}

fn keep(values: &mut VecDeque<f32>, value: f32) {
    if values.len() == FRAMES_KEPT {
        values.pop_front();
    }
    values.push_back(value);
}

impl GpuTimes {
    /// The frames since the last dump, as the vitals' `gpu`. It gives each
    /// pass's ms and fragments spread over those frames. The frames then
    /// start again.
    pub(crate) fn drain(&mut self, spread: fn(Vec<f32>) -> serde_json::Value) -> serde_json::Value {
        let ms = per_pass(&mut self.ms, spread);
        let fragments = per_pass(&mut self.fragments, spread);
        json!({
            "spans_ms": spread(self.spans.drain(..).collect()),
            "ms": ms,
            "fragments": fragments,
        })
    }
}

fn per_pass(
    kept: &mut HashMap<String, VecDeque<f32>>,
    spread: fn(Vec<f32>) -> serde_json::Value,
) -> serde_json::Map<String, serde_json::Value> {
    let mut passes: Vec<_> = kept
        .iter_mut()
        .map(|(pass, values)| (pass.clone(), spread(values.drain(..).collect())))
        .collect();
    passes.sort_by(|a, b| a.0.cmp(&b.0));
    passes.into_iter().collect()
}
