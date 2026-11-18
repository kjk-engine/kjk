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

use crate::gpu_load::{GpuLoad, Lost, Measured};
use crate::warmup::Compiled;

const NEED_SHARE: f64 = 0.8;
const DROP_SHARE: f64 = 0.7;
const FIRST_GROWTH: f64 = 1.3;
const GROWTH_RANGE: (f64, f64) = (1.0, 4.0);
const WINDOW: Duration = Duration::from_millis(250);
const MIN_READINGS: usize = 3;
const MAX_WINDOW: Duration = Duration::from_secs(1);
const HOLD: Duration = Duration::from_secs(2);
/// A frame's GPU time can be longer than its render frame only while
/// frames queue up behind the GPU, and not by this many render frames.
const MAX_FRAMES_SPANNED: f64 = 2.0;

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
    /// `why` is the GPU load check's reason. A dump then gives the check's
    /// reason at the time of the dump.
    why_check: bool,
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
    /// The device has timestamp queries.
    timer: bool,
    /// The last window's median GPU ms and mean render ms. It is `None`
    /// when that window kept no reading, and before the scene's first.
    last: Option<(f64, f64)>,
    /// The windows closed since the scene camera appeared.
    scene_windows: u64,
    run: RunTotals,
    window_started: Instant,
    gpu_ms: Vec<f64>,
    render_ms: f64,
    render_frames: u32,
    hold_until: Instant,
    growth: f64,
    dropped_from: Option<Dropped>,
}

/// What the check measured since the run began. A bench takes the
/// difference between two dumps.
#[derive(Default, Clone, Copy)]
struct RunTotals {
    /// The windows that kept a reading, and their GPU and render ms added
    /// up.
    timed: (u64, f64, f64),
    untimed: u64,
    lost: Lost,
}

/// The first window after a drop still holds frames drawn with the prepass,
/// since the GPU's readings arrive a few frames late.
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
            timer: true,
            last: None,
            scene_windows: 0,
            run: RunTotals::default(),
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
        match self.last {
            Some((gpu, render)) => format!(
                "GPU {gpu:.2} ms of a {render:.2} ms render frame ({:.0}%), growth without the prepass {:.2}",
                100.0 * gpu / render.max(1e-6),
                self.growth
            ),
            None if !self.timer => "this GPU cannot be timed".into(),
            None if self.scene_windows == 0 => "no GPU reading yet".into(),
            None => "no GPU reading in the last window".into(),
        }
    }

    fn restart_window(&mut self, now: Instant) {
        self.window_started = now;
        self.gpu_ms.clear();
        self.render_ms = 0.0;
        self.render_frames = 0;
    }

    /// A window's median GPU ms and mean render ms. A reading longer than
    /// `MAX_FRAMES_SPANNED` mean render frames is dropped. A window that
    /// keeps fewer than `MIN_READINGS` has no reading, since one far-off
    /// reading would then decide.
    fn close_window(&mut self) -> Option<(f64, f64)> {
        let render =
            (self.render_frames > 0).then(|| self.render_ms / f64::from(self.render_frames))?;
        let readings = self.gpu_ms.len();
        self.gpu_ms.retain(|ms| *ms <= MAX_FRAMES_SPANNED * render);
        self.run.lost.too_long += (readings - self.gpu_ms.len()) as u32;
        if self.gpu_ms.len() < MIN_READINGS {
            return None;
        }
        self.gpu_ms.sort_by(f64::total_cmp);
        Some((self.gpu_ms[self.gpu_ms.len() / 2], render))
    }

    /// `scene` says whether a scene camera is up and no area is loading.
    /// Otherwise the check starts over, so a loading screen's frames and
    /// what it learned in another area never decide for the next area.
    fn update(
        &mut self,
        measured: Measured,
        now: Instant,
        decides: bool,
        quiet: bool,
        scene: bool,
    ) {
        self.run.lost.add(measured.lost);
        if measured.render_frames > 0 {
            self.timer = measured.timed;
        }
        if !scene {
            *self = LoadCheck {
                run: self.run,
                timer: self.timer,
                ..LoadCheck::default()
            };
            return;
        }
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
        let window = self.close_window().filter(|_| self.timer);
        self.restart_window(now);
        self.scene_windows += 1;
        self.last = window;
        let Some((gpu, render)) = window else {
            self.run.untimed += 1;
            if decides {
                self.ask(now);
            }
            return;
        };
        self.run.timed.0 += 1;
        self.run.timed.1 += gpu;
        self.run.timed.2 += render;
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
    cameras: Query<(), With<DerivedPrepasses>>,
    loading: Option<Res<crate::sim_host::Loading>>,
    mut check: ResMut<LoadCheck>,
) {
    let Some(load) = load else { return };
    let quiet = compiled.is_none_or(|compiled| compiled.quiet());
    let scene = !cameras.is_empty() && !loading.is_some_and(|loading| loading.active);
    check.update(
        load.take(),
        Instant::now(),
        prepasses.load_decides,
        quiet,
        scene,
    );
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
    let by_features = need.depth >= load_depth && !by.is_empty();
    let by_check = !by_features && forced.0.is_none();
    if changed || by_check != prepasses.why_check {
        let why = if by_features {
            by.join(", ")
        } else if forced.0.is_some() {
            "KJK_DEPTH_PREPASS".to_string()
        } else {
            check.reason()
        };
        if changed {
            info!(
                target: "display",
                depth = depth.name(),
                normals = need.normals,
                motion = need.motion,
                "prepasses: {why}"
            );
            prepasses.switches += 1;
        }
        prepasses.why = why;
        prepasses.why_check = by_check;
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
    let why = match prepasses.why_check && prepasses.load_decides {
        true => check.reason(),
        false => prepasses.why.clone(),
    };
    let (timed, gpu_sum, render_sum) = check.run.timed;
    let lost = check.run.lost;
    facts.0.insert(
        "depth_prepass".into(),
        serde_json::json!({
            "coverage": prepasses.depth.name(),
            "normals": prepasses.normals,
            "motion": prepasses.motion,
            "why": why,
            "frames": { "none": none, "full": full },
            "switches": prepasses.switches,
            "gpu_timed": check.last.is_some(),
            "gpu_ms": check.last.map(|(gpu, _)| gpu),
            "render_ms": check.last.map(|(_, render)| render),
            "growth": check.growth,
            "windows": timed,
            "gpu_ms_sum": gpu_sum,
            "render_ms_sum": render_sum,
            "untimed_windows": check.run.untimed,
            "lost": {
                "unstamped": lost.unstamped,
                "stale": lost.stale,
                "impossible": lost.impossible,
                "too_long": lost.too_long,
            },
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

/// The GPU load check's probes (`kjk sim probes --only "gpu load check"`).
#[cfg(feature = "probes")]
pub fn load_check_probes() -> sim::probe::Checks {
    use crate::gpu_load::{Reading, reading};
    use sim::probe::check;
    let start = Instant::now();
    let at = |ms: u64| start + Duration::from_millis(ms);
    // `update`'s three flags are, in order, that the check decides, that
    // nothing compiles, and that a scene camera is up with no area loading.
    // A check whose answer decides, in a scene, with its first window open.
    let opened = || {
        let mut probe = LoadCheck::default();
        probe.update(Measured::default(), at(0), true, true, true);
        probe
    };
    let period_ns = 1.0;
    let ms = |ms: u64| ms * 1_000_000;
    vec![
        (
            "a frame's GPU time is never longer than the wall time it had",
            {
                let kept = reading([ms(100), ms(110)], 0, period_ns, 30.0);
                let spanning = reading([ms(100), ms(172)], 0, period_ns, 40.0);
                let overlap = reading([ms(100), ms(110)], ms(104), period_ns, 30.0);
                let stale = reading([ms(100), ms(110)], ms(110), period_ns, 30.0);
                check(
                    kept == Reading::Kept(10.0)
                        && spanning == Reading::Impossible
                        && overlap == Reading::Kept(6.0)
                        && stale == Reading::Stale,
                    format!("{kept:?}, {spanning:?}, {overlap:?}, {stale:?}"),
                )
            },
        ),
        (
            "a window drops readings longer than two of its render frames",
            {
                let mut probe = opened();
                let readings = [30.0, 31.0, 32.0, 72.0, 80.0, 90.0];
                probe.update(Measured::probe(&readings, 33.0), at(300), true, true, true);
                let share = probe.last.map(|(gpu, render)| gpu / render);
                check(
                    probe.last == Some((31.0, 33.0)) && probe.run.lost.too_long == 3,
                    format!(
                        "last {:?}, share {share:?}, too long {}",
                        probe.last, probe.run.lost.too_long
                    ),
                )
            },
        ),
        (
            "a window with fewer than three readings says so in its reason and its facts",
            {
                let mut probe = opened();
                probe.update(Measured::probe(&[10.0; 5], 20.0), at(300), true, true, true);
                let timed = probe.reason();
                probe.update(
                    Measured::probe(&[10.0, 10.0], 20.0),
                    at(1400),
                    true,
                    true,
                    true,
                );
                let untimed = probe.reason();
                check(
                    timed.starts_with("GPU 10.00 ms of a 20.00 ms")
                        && probe.last.is_none()
                        && probe.run.timed.0 == 1
                        && probe.run.untimed == 1
                        && untimed == "no GPU reading in the last window"
                        && probe.full,
                    format!(
                        "{timed:?}, then {untimed:?}, last {:?}, full {}",
                        probe.last, probe.full
                    ),
                )
            },
        ),
        (
            "the check starts over without a scene camera and keeps the run's totals",
            {
                let mut probe = opened();
                probe.update(Measured::probe(&[2.0; 5], 20.0), at(300), true, true, true);
                let dropped = !probe.full;
                probe.growth = 4.0;
                probe.update(
                    Measured::probe(&[2.0; 5], 20.0),
                    at(400),
                    false,
                    true,
                    false,
                );
                check(
                    dropped
                        && probe.full
                        && probe.growth == FIRST_GROWTH
                        && probe.last.is_none()
                        && probe.reason() == "no GPU reading yet"
                        && probe.run.timed.0 == 1,
                    format!(
                        "dropped {dropped}, then full {}, growth {}, reason {:?}, timed windows {}",
                        probe.full,
                        probe.growth,
                        probe.reason(),
                        probe.run.timed.0
                    ),
                )
            },
        ),
    ]
}
