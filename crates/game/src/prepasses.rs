//! The scene camera always keeps Bevy's `DepthPrepass`, even with a coverage
//! of none. Taking it away would compile every main pass pipeline anew in the
//! middle of play. Bevy queues a prepass mesh again only when it becomes
//! visible or its pipeline is specialized anew, so a coverage change
//! specializes the view's pipelines anew (`respecialize_on_change`).
//!
//! An earlier version also had a coverage of only the rooms' meshes.
//! `KJK_DEPTH_PREPASS=rooms` now gives the full depth prepass and logs a
//! warning.

use std::time::{Duration, Instant};

use bevy::core_pipeline::prepass::{
    AlphaMask3dPrepass, DepthPrepass, MotionVectorPrepass, NormalPrepass, Opaque3dPrepass,
};
use bevy::pbr::PendingPrepassMeshMaterialQueues;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::camera::DirtySpecializations;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_phase::ViewBinnedRenderPhases;
use bevy::render::view::{ExtractedView, RenderVisibleEntities, RetainedViewEntity};
use bevy::render::{Render, RenderApp, RenderSystems};
use console::DumpFacts;

use crate::gpu_load::{GpuLoad, Measured};
use crate::warmup::Compiled;

const NEED_SHARE: f64 = 0.8;
const DROP_SHARE: f64 = 0.7;
const FIRST_GROWTH: f64 = 1.3;
const GROWTH_RANGE: (f64, f64) = (1.0, 4.0);
const WINDOW: Duration = Duration::from_millis(250);
const MIN_READINGS: usize = 3;
const MAX_WINDOW: Duration = Duration::from_secs(1);
const HOLD: Duration = Duration::from_secs(2);

#[derive(
    Component, ExtractComponent, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord,
)]
pub enum DepthCoverage {
    #[default]
    None,
    Full,
}

impl DepthCoverage {
    const ALL: [DepthCoverage; 2] = [DepthCoverage::None, DepthCoverage::Full];

    pub fn name(self) -> &'static str {
        match self {
            DepthCoverage::None => "none",
            DepthCoverage::Full => "full",
        }
    }

    fn index(self) -> usize {
        match self {
            DepthCoverage::None => 0,
            DepthCoverage::Full => 1,
        }
    }

    fn named(name: &str) -> Option<DepthCoverage> {
        DepthCoverage::ALL
            .into_iter()
            .find(|coverage| coverage.name() == name)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Need {
    pub depth: DepthCoverage,
    pub normals: bool,
    pub motion: bool,
}

impl Need {
    pub const TEMPORAL: Need = Need {
        depth: DepthCoverage::Full,
        normals: false,
        motion: true,
    };
    pub const AMBIENT_OCCLUSION: Need = Need {
        depth: DepthCoverage::Full,
        normals: true,
        motion: false,
    };

    fn union(self, other: Need) -> Need {
        Need {
            depth: self.depth.max(other.depth),
            normals: self.normals || other.normals,
            motion: self.motion || other.motion,
        }
    }
}

#[derive(Resource, Default)]
pub struct PrepassNeeds {
    need: Need,
    by: Vec<&'static str>,
}

impl PrepassNeeds {
    pub fn declare(&mut self, by: &'static str, need: Need) {
        self.need = self.need.union(need);
        if !self.by.contains(&by) {
            self.by.push(by);
        }
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclarePrepassNeeds;

#[derive(Component, Default)]
pub struct DerivedPrepasses;

#[derive(Resource, Default)]
pub struct Prepasses {
    pub depth: DepthCoverage,
    pub normals: bool,
    pub motion: bool,
    why: String,
    frames: [u64; 2],
    switches: u64,
    load_decides: bool,
}

#[derive(Resource)]
struct Forced(Option<DepthCoverage>);

fn forced() -> Option<DepthCoverage> {
    let name = std::env::var("KJK_DEPTH_PREPASS").ok()?;
    let name = name.trim();
    if name == "rooms" {
        warn!(target: "display", "KJK_DEPTH_PREPASS=rooms is no longer a coverage. The full depth prepass is drawn instead.");
        return Some(DepthCoverage::Full);
    }
    let coverage = DepthCoverage::named(name);
    if coverage.is_none() {
        warn!(target: "display", "KJK_DEPTH_PREPASS={name} is not none or full, so it is ignored");
    }
    coverage
}

#[derive(Resource)]
struct LoadCheck {
    full: bool,
    /// The check's answer decided the coverage when it last took in a
    /// measurement. A window that spans a change is thrown away.
    decides: bool,
    /// The device times the GPU, and the last window held a reading.
    measured: bool,
    last: Option<(f64, f64)>,
    /// The timed windows since the run began, and their GPU and render ms
    /// added up. A bench takes their means between two dumps.
    windows: (u64, f64, f64),
    window_started: Instant,
    gpu_ms: Vec<f64>,
    render_ms: f64,
    render_frames: u32,
    hold_until: Instant,
    growth: f64,
    dropped_from: Option<Dropped>,
}

/// The first window after a drop still holds frames drawn with the prepass,
/// since the GPU's readings arrive one to three frames late.
#[derive(Clone, Copy)]
struct Dropped {
    gpu: f64,
    share: f64,
    settled: bool,
}

impl Default for LoadCheck {
    fn default() -> LoadCheck {
        let now = Instant::now();
        LoadCheck {
            full: true,
            decides: false,
            measured: false,
            last: None,
            windows: (0, 0.0, 0.0),
            window_started: now,
            gpu_ms: Vec::new(),
            render_ms: 0.0,
            render_frames: 0,
            hold_until: now,
            growth: FIRST_GROWTH,
            dropped_from: None,
        }
    }
}

impl LoadCheck {
    fn reason(&self) -> String {
        match (self.measured, self.last) {
            (true, Some((gpu, render))) => format!(
                "GPU {gpu:.2} ms of a {render:.2} ms render frame ({:.0}%), growth without the prepass {:.2}",
                100.0 * gpu / render.max(1e-6),
                self.growth
            ),
            _ => "no GPU timing".into(),
        }
    }

    fn restart_window(&mut self, now: Instant) {
        self.window_started = now;
        self.gpu_ms.clear();
        self.render_ms = 0.0;
        self.render_frames = 0;
    }

    fn update(&mut self, measured: Measured, now: Instant, decides: bool, quiet: bool) {
        if !quiet || decides != self.decides {
            self.decides = decides;
            self.restart_window(now);
            self.dropped_from = None;
            return;
        }
        self.gpu_ms.extend(&measured.gpu_ms);
        self.render_ms += measured.render_ms;
        self.render_frames += measured.render_frames;
        let age = now.duration_since(self.window_started);
        if age < WINDOW || (self.gpu_ms.len() < MIN_READINGS && age < MAX_WINDOW) {
            return;
        }
        let window = (!self.gpu_ms.is_empty() && self.render_frames > 0).then(|| {
            self.gpu_ms.sort_by(f64::total_cmp);
            (
                self.gpu_ms[self.gpu_ms.len() / 2],
                self.render_ms / f64::from(self.render_frames),
            )
        });
        self.restart_window(now);
        let Some((gpu, render)) = window.filter(|_| measured.timed) else {
            self.measured = false;
            if decides {
                self.ask(now);
            }
            return;
        };
        self.measured = true;
        self.last = Some((gpu, render));
        self.windows.0 += 1;
        self.windows.1 += gpu;
        self.windows.2 += render;
        if !decides {
            return;
        }
        let share = gpu / render.max(1e-6);
        if self.full {
            if now >= self.hold_until && share * self.growth < DROP_SHARE {
                self.full = false;
                self.dropped_from = Some(Dropped {
                    gpu,
                    share,
                    settled: false,
                });
            }
            return;
        }
        let grown = |before: &Dropped| gpu / before.gpu.max(1e-6);
        if share >= NEED_SHARE {
            // The drop bounced. The growth then becomes at least what
            // would have predicted this share, so the same load cannot
            // drop the prepass again.
            if let Some(before) = self.dropped_from.take() {
                let at_least = NEED_SHARE / before.share.max(1e-6);
                self.growth = grown(&before).max(at_least).max(self.growth);
            }
            self.growth = self.growth.clamp(GROWTH_RANGE.0, GROWTH_RANGE.1);
            self.ask(now);
            return;
        }
        match self.dropped_from {
            Some(before) if before.settled => {
                self.growth = grown(&before).clamp(GROWTH_RANGE.0, GROWTH_RANGE.1);
                self.dropped_from = None;
            }
            Some(ref mut before) => before.settled = true,
            None => {}
        }
    }

    fn ask(&mut self, now: Instant) {
        if !self.full {
            self.full = true;
            self.hold_until = now + HOLD;
        }
    }
}

pub struct PrepassesPlugin;

impl Plugin for PrepassesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            crate::gpu_load::GpuLoadPlugin,
            ExtractComponentPlugin::<DepthCoverage>::default(),
        ))
        .insert_resource(Forced(forced()))
        .init_resource::<PrepassNeeds>()
        .init_resource::<Prepasses>()
        .init_resource::<LoadCheck>()
        .add_systems(
            Last,
            (
                check_gpu_load.in_set(DeclarePrepassNeeds),
                (derive, facts)
                    .chain()
                    .after(DeclarePrepassNeeds)
                    .after(crate::temporal::Applied),
            ),
        );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.add_systems(
            Render,
            (
                respecialize_on_change.in_set(RenderSystems::PrepareAssets),
                keep_coverage
                    .in_set(RenderSystems::QueueMeshes)
                    .after(bevy::pbr::queue_prepass_material_meshes),
            ),
        );
        #[cfg(debug_assertions)]
        render_app.add_systems(Render, needs_met.in_set(RenderSystems::PrepareViews));
    }
}

fn check_gpu_load(
    load: Option<Res<GpuLoad>>,
    compiled: Option<Res<Compiled>>,
    prepasses: Res<Prepasses>,
    mut check: ResMut<LoadCheck>,
) {
    let Some(load) = load else { return };
    let quiet = compiled.is_none_or(|compiled| compiled.quiet());
    check.update(load.take(), Instant::now(), prepasses.load_decides, quiet);
}

#[allow(clippy::type_complexity)]
fn derive(
    mut needs: ResMut<PrepassNeeds>,
    forced: Res<Forced>,
    check: Res<LoadCheck>,
    mut prepasses: ResMut<Prepasses>,
    cameras: Query<
        (
            Entity,
            Has<DepthPrepass>,
            Has<NormalPrepass>,
            Has<MotionVectorPrepass>,
            Option<&DepthCoverage>,
        ),
        With<DerivedPrepasses>,
    >,
    mut commands: Commands,
) {
    let PrepassNeeds { need, by } = std::mem::take(&mut *needs);
    if cameras.is_empty() {
        let frames = prepasses.frames;
        let switches = prepasses.switches;
        *prepasses = Prepasses {
            frames,
            switches,
            ..default()
        };
        return;
    }
    let load_depth = match forced.0 {
        Some(coverage) => coverage,
        None if check.full => DepthCoverage::Full,
        None => DepthCoverage::None,
    };
    let mut depth = need.depth.max(load_depth);
    if need.normals || need.motion {
        depth = DepthCoverage::Full;
    }
    for (camera, has_depth, has_normals, has_motion, coverage) in &cameras {
        let mut camera = commands.entity(camera);
        if !has_depth {
            camera.try_insert(DepthPrepass);
        }
        if need.normals && !has_normals {
            camera.try_insert(NormalPrepass);
        }
        if !need.normals && has_normals {
            camera.try_remove::<NormalPrepass>();
        }
        if need.motion && !has_motion {
            camera.try_insert(MotionVectorPrepass);
        }
        if !need.motion && has_motion {
            camera.try_remove::<MotionVectorPrepass>();
        }
        if coverage != Some(&depth) {
            camera.try_insert(depth);
        }
    }
    let changed = (prepasses.depth, prepasses.normals, prepasses.motion)
        != (depth, need.normals, need.motion);
    if changed {
        let why = if need.depth >= load_depth && !by.is_empty() {
            by.join(", ")
        } else if forced.0.is_some() {
            "KJK_DEPTH_PREPASS".to_string()
        } else {
            check.reason()
        };
        info!(
            target: "display",
            depth = depth.name(),
            normals = need.normals,
            motion = need.motion,
            "prepasses: {why}"
        );
        prepasses.switches += 1;
        prepasses.why = why;
    }
    prepasses.depth = depth;
    prepasses.normals = need.normals;
    prepasses.motion = need.motion;
    prepasses.frames[depth.index()] += 1;
    prepasses.load_decides = forced.0.is_none() && depth == load_depth;
}

fn facts(prepasses: Res<Prepasses>, check: Res<LoadCheck>, mut facts: ResMut<DumpFacts>) {
    if !facts.wanted() {
        return;
    }
    let [none, full] = prepasses.frames;
    facts.0.insert(
        "depth_prepass".into(),
        serde_json::json!({
            "coverage": prepasses.depth.name(),
            "normals": prepasses.normals,
            "motion": prepasses.motion,
            "why": prepasses.why,
            "frames": { "none": none, "full": full },
            "switches": prepasses.switches,
            "gpu_timed": check.measured,
            "gpu_ms": check.last.map(|(gpu, _)| gpu),
            "render_ms": check.last.map(|(_, render)| render),
            "growth": check.growth,
            "windows": check.windows.0,
            "gpu_ms_sum": check.windows.1,
            "render_ms_sum": check.windows.2,
        }),
    );
}

fn respecialize_on_change(
    views: Query<(&ExtractedView, &DepthCoverage)>,
    mut dirty: ResMut<DirtySpecializations>,
    mut seen: Local<HashMap<RetainedViewEntity, DepthCoverage>>,
) {
    for (view, coverage) in &views {
        let key = view.retained_view_entity;
        if seen
            .insert(key, *coverage)
            .is_some_and(|was| was != *coverage)
        {
            dirty.views.insert(key);
        }
    }
    if seen.len() > views.iter().count() {
        let live: Vec<RetainedViewEntity> = views
            .iter()
            .map(|(view, _)| view.retained_view_entity)
            .collect();
        seen.retain(|key, _| live.contains(key));
    }
}

fn keep_coverage(
    views: Query<(&ExtractedView, &RenderVisibleEntities, &DepthCoverage)>,
    dirty: Res<DirtySpecializations>,
    pending: Res<PendingPrepassMeshMaterialQueues>,
    mut opaque: ResMut<ViewBinnedRenderPhases<Opaque3dPrepass>>,
    mut mask: ResMut<ViewBinnedRenderPhases<AlphaMask3dPrepass>>,
) {
    for (view, visible, coverage) in &views {
        if *coverage == DepthCoverage::Full {
            continue;
        }
        let key = view.retained_view_entity;
        let (Some(visible), Some(view_pending)) = (visible.get::<Mesh3d>(), pending.get(&key))
        else {
            continue;
        };
        let mut opaque_phase = opaque.get_mut(&key);
        let mut mask_phase = mask.get_mut(&key);
        for (_, main_entity) in dirty.iter_to_queue(key, visible, &view_pending.prev_frame) {
            if let Some(phase) = opaque_phase.as_mut() {
                phase.remove(*main_entity);
            }
            if let Some(phase) = mask_phase.as_mut() {
                phase.remove(*main_entity);
            }
        }
    }
}

#[cfg(debug_assertions)]
#[allow(clippy::type_complexity)]
fn needs_met(
    views: Query<(
        Entity,
        Has<bevy::anti_alias::taa::TemporalAntiAliasing>,
        Has<crate::upscale::Upscale>,
        Has<bevy::pbr::ScreenSpaceAmbientOcclusion>,
        Has<DepthPrepass>,
        Has<NormalPrepass>,
        Has<MotionVectorPrepass>,
        Option<&DepthCoverage>,
    )>,
    #[cfg(dlss)] dlss: Query<(), With<bevy::anti_alias::dlss::Dlss>>,
) {
    for (view, taa, upscale, occlusion, depth, normals, motion, coverage) in &views {
        #[cfg(dlss)]
        let temporal = taa || upscale || dlss.contains(view);
        #[cfg(not(dlss))]
        let temporal = taa || upscale;
        let full = depth && coverage == Some(&DepthCoverage::Full);
        debug_assert!(
            !temporal || (full && motion),
            "view {view}: TAA, the upscaler or DLSS without a full depth prepass and the motion prepass"
        );
        debug_assert!(
            !occlusion || (full && normals),
            "view {view}: ambient occlusion without a full depth prepass and the normal prepass"
        );
    }
}
