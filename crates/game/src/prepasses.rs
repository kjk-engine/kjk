//! The scene camera's prepasses, derived every frame from what the frame's
//! features need. A prepass draws the scene before the main pass. There are
//! three, as in Bevy:
//!
//! - The depth prepass (`DepthPrepass`) writes depth. Its coverage
//!   (`DepthCoverage`) says which meshes it draws. It draws no mesh
//!   (`None`), only the rooms' meshes (`Rooms`), or every mesh (`Full`).
//! - The normal prepass (`NormalPrepass`) also writes each pixel's normal.
//! - The motion prepass (`MotionVectorPrepass`) also writes how far each
//!   pixel moved since the frame before.
//!
//! No setting chooses them. Each feature that reads a prepass declares what
//! it needs in `PrepassNeeds` each frame. Then `derive` gives the scene
//! camera the union of the needs. A full depth prepass wins over a rooms
//! one, and a rooms one wins over none.
//!
//! - TAA, kjk's upscaler, DLAA and DLSS (`temporal`) need a full depth
//!   prepass and the motion prepass (`Need::TEMPORAL`).
//! - Ambient occlusion (`ambient_occlusion`) needs a full depth prepass and
//!   the normal prepass (`Need::AMBIENT_OCCLUSION`).
//! - The motion debug view (`KJK_MOTION_DEBUG=1`) needs what TAA needs
//!   while it is shown.
//! - The GPU load check (`LoadCheck`) asks for a rooms depth prepass while
//!   the GPU is close to holding the frame rate back. The main pass then
//!   shades only the surfaces that the rooms do not hide. Without a depth
//!   prepass, the main pass shades every surface drawn at a pixel, hidden
//!   ones too. It also traces the characters' capsule shadows for each of
//!   them (`shadows`).
//!
//! Grass's motion twins (`grass`) and the motion proxies (`motion_proxy`)
//! draw into the motion prepass only while the camera has one. They follow
//! it and never cause it.
//!
//! A prepass that writes normals or motion draws every mesh. So a rooms or
//! empty coverage is used only when neither is needed.
//!
//! The scene camera always keeps Bevy's `DepthPrepass`, even when the
//! coverage is none. The main pass's pipelines depend on whether the view
//! has a depth prepass. Taking it away or giving it back would compile
//! every main pass pipeline anew in the middle of play, and on macOS Bevy
//! compiles them on the render thread. So a coverage of none is an empty
//! depth prepass. Its pass still clears the depth and copies it, which
//! costs the GPU little, and it costs the CPU no draw.
//!
//! Under a rooms coverage, the meshes that are not a room's are taken out
//! of the prepass's phases each frame, after Bevy queues them
//! (`keep_coverage`). Under a coverage of none, every mesh is taken out.
//! Bevy keeps its phases from frame to frame. It queues a mesh again only
//! when the mesh becomes visible or its pipeline is specialized anew. So
//! when a view's coverage changes, its pipelines are all specialized anew
//! (`respecialize_on_change`), and every visible mesh is queued again in
//! that same frame. The pipelines are found in Bevy's cache, so nothing is
//! compiled.
//!
//! `KJK_DEPTH_PREPASS=none`, `rooms` or `full` takes the GPU load check's
//! place for a run, for tests and benchmarks. The features' needs still
//! win over it. No menu offers it. `docs/PREPASSES.md` gives the
//! measurements behind all this.

use std::time::{Duration, Instant};

use bevy::core_pipeline::prepass::{
    AlphaMask3dPrepass, DepthPrepass, MotionVectorPrepass, NormalPrepass, Opaque3dPrepass,
};
use bevy::pbr::{PendingPrepassMeshMaterialQueues, RenderMeshInstances};
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

/// The GPU load check asks for the rooms' depth prepass when the GPU's
/// share of the render thread's frame time reaches this.
const NEED_SHARE: f64 = 0.8;
/// The check lets the prepass go only when the share it predicts without
/// the prepass stays below this. The predicted share is the share measured
/// with the prepass times the learned growth (`LoadCheck::growth`).
const DROP_SHARE: f64 = 0.7;
/// The growth of the GPU's time the check assumes for a first drop. It
/// learns the real growth from each drop.
const FIRST_GROWTH: f64 = 1.3;
/// The learned growth is kept between these. A smaller value is noise,
/// since the prepass only ever saves the GPU work.
const GROWTH_RANGE: (f64, f64) = (1.0, 4.0);
/// The shortest time a window of measurements covers.
const WINDOW: Duration = Duration::from_millis(250);
/// The GPU readings a window needs before the check decides on it. The
/// check takes their median, so one reading that is far off does not move
/// it. A device that drops three readings in four still gathers about 4 in
/// a window of `WINDOW` at 60 frames a second.
const MIN_READINGS: usize = 3;
/// The longest time a window waits for `MIN_READINGS`. A window that still
/// has no reading then counts as unmeasured, and the check asks for the
/// rooms' prepass.
const MAX_WINDOW: Duration = Duration::from_secs(1);
/// After the check asks for the prepass, it does not let it go again for
/// at least this long. Asking for the prepass is never held back.
const HOLD: Duration = Duration::from_secs(2);

/// Which meshes the depth prepass draws.
#[derive(
    Component, ExtractComponent, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord,
)]
pub enum DepthCoverage {
    /// No mesh. The depth prepass is empty.
    #[default]
    None,
    /// Only the rooms' meshes.
    Rooms,
    /// Every mesh.
    Full,
}

impl DepthCoverage {
    const ALL: [DepthCoverage; 3] = [
        DepthCoverage::None,
        DepthCoverage::Rooms,
        DepthCoverage::Full,
    ];

    pub fn name(self) -> &'static str {
        match self {
            DepthCoverage::None => "none",
            DepthCoverage::Rooms => "rooms",
            DepthCoverage::Full => "full",
        }
    }

    /// Its place in `DepthCoverage::ALL` and in `Prepasses::frames`.
    fn index(self) -> usize {
        match self {
            DepthCoverage::None => 0,
            DepthCoverage::Rooms => 1,
            DepthCoverage::Full => 2,
        }
    }

    fn named(name: &str) -> Option<DepthCoverage> {
        DepthCoverage::ALL
            .into_iter()
            .find(|coverage| coverage.name() == name.trim())
    }
}

/// What one feature needs of the scene camera's prepasses.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Need {
    pub depth: DepthCoverage,
    pub normals: bool,
    pub motion: bool,
}

impl Need {
    /// TAA, kjk's upscaler, DLAA and DLSS resolve frames through every
    /// mesh's depth and motion.
    pub const TEMPORAL: Need = Need {
        depth: DepthCoverage::Full,
        normals: false,
        motion: true,
    };
    /// Ambient occlusion is worked out from every mesh's depth and normal.
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

/// The needs the features declared this frame, and who declared them.
/// `derive` reads them and starts the next frame empty.
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

/// The systems that declare needs. `derive` runs after them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclarePrepassNeeds;

/// A camera whose prepasses `derive` sets. It is the scene camera on the
/// world's target (`temporal::apply`).
#[derive(Component, Default)]
pub struct DerivedPrepasses;

/// What `derive` gave the scene camera last, and why it last changed. It
/// also counts the frames at each coverage and the changes since the run
/// began.
#[derive(Resource, Default)]
pub struct Prepasses {
    pub depth: DepthCoverage,
    pub normals: bool,
    pub motion: bool,
    why: String,
    /// Frames at none, rooms and full (`DepthCoverage::index`).
    frames: [u64; 3],
    switches: u64,
    /// The GPU load check's answer decided the coverage last frame. The
    /// features asked for no depth prepass, and `KJK_DEPTH_PREPASS` is not
    /// set.
    load_decides: bool,
}

/// `KJK_DEPTH_PREPASS`, read once.
#[derive(Resource)]
struct Forced(Option<DepthCoverage>);

fn forced() -> Option<DepthCoverage> {
    let name = std::env::var("KJK_DEPTH_PREPASS").ok()?;
    let coverage = DepthCoverage::named(&name);
    if coverage.is_none() {
        warn!(target: "display", "KJK_DEPTH_PREPASS={name} is not none, rooms or full, so it is ignored");
    }
    coverage
}

/// The GPU load check. It says whether the GPU needs the rooms' depth
/// prepass.
#[derive(Resource)]
struct LoadCheck {
    rooms: bool,
    /// The device times the GPU, and the last window held a reading.
    measured: bool,
    /// The last window's median GPU ms and mean render ms, per frame.
    last: Option<(f64, f64)>,
    window_started: Instant,
    gpu_ms: Vec<f64>,
    render_ms: f64,
    render_frames: u32,
    /// The check does not let the prepass go before this.
    hold_until: Instant,
    /// How many times longer the GPU's time grew when the prepass went.
    growth: f64,
    /// The window just before the last drop, while the check still waits
    /// to learn the growth from the windows after it.
    dropped_from: Option<Dropped>,
}

/// The last window before a drop, and whether the window after the drop
/// has passed. That first window still holds frames drawn with the
/// prepass, since the GPU's readings arrive one to three frames late. So
/// the growth is learned from the second window.
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
            rooms: true,
            measured: false,
            last: None,
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
    /// Why the check asks for what it asks, for the log.
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

    /// Takes in what the GPU and the render thread measured, and decides
    /// once a window is full. It measures only while its answer decides the
    /// coverage (`decides`) and nothing is compiling (`quiet`).
    fn update(&mut self, measured: Measured, now: Instant, decides: bool, quiet: bool) {
        if !decides || !quiet {
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
            self.ask(now);
            return;
        };
        self.measured = true;
        self.last = Some((gpu, render));
        let share = gpu / render.max(1e-6);
        if self.rooms {
            if now >= self.hold_until && share * self.growth < DROP_SHARE {
                self.rooms = false;
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

    /// Asks for the rooms' prepass at once, and holds it.
    fn ask(&mut self, now: Instant) {
        if !self.rooms {
            self.rooms = true;
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

/// Gives the scene camera the union of the needs declared this frame.
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
        None if check.rooms => DepthCoverage::Rooms,
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

/// For a dump, the `depth_prepass` fact. It gives the coverage now and why,
/// and whether the normal and motion prepasses are drawn. It counts the
/// frames at each coverage and the changes since the run began. It also
/// gives the GPU load check's last window and its learned growth.
fn facts(prepasses: Res<Prepasses>, check: Res<LoadCheck>, mut facts: ResMut<DumpFacts>) {
    if !facts.wanted() {
        return;
    }
    let [none, rooms, full] = prepasses.frames;
    facts.0.insert(
        "depth_prepass".into(),
        serde_json::json!({
            "coverage": prepasses.depth.name(),
            "normals": prepasses.normals,
            "motion": prepasses.motion,
            "why": prepasses.why,
            "frames": { "none": none, "rooms": rooms, "full": full },
            "switches": prepasses.switches,
            "gpu_timed": check.measured,
            "gpu_ms": check.last.map(|(gpu, _)| gpu),
            "render_ms": check.last.map(|(_, render)| render),
            "growth": check.growth,
        }),
    );
}

/// Specializes every pipeline of a view anew when its coverage changes. Bevy
/// then queues all of the view's visible meshes again in that frame. The
/// meshes a narrower coverage took out come back at once, and those a new
/// narrower coverage leaves out are taken out at once.
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

/// A room's mesh carries its room's index plus 1 as its tag, below
/// `shadows::OWNER_TAG` (`scene::room_tag`).
fn is_room(tag: u32) -> bool {
    tag != 0 && tag & crate::shadows::OWNER_TAG == 0
}

/// Takes out of a view's prepass every mesh Bevy queued this frame that
/// its coverage leaves out. A rooms coverage keeps only the rooms' meshes,
/// and a coverage of none keeps no mesh. It meets exactly the meshes Bevy
/// queued, in the order Bevy met them.
fn keep_coverage(
    views: Query<(&ExtractedView, &RenderVisibleEntities, &DepthCoverage)>,
    dirty: Res<DirtySpecializations>,
    pending: Res<PendingPrepassMeshMaterialQueues>,
    meshes: Res<RenderMeshInstances>,
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
            let kept = *coverage == DepthCoverage::Rooms
                && meshes
                    .render_mesh_queue_data(*main_entity)
                    .is_some_and(|mesh| is_room(mesh.tag()));
            if kept {
                continue;
            }
            if let Some(phase) = opaque_phase.as_mut() {
                phase.remove(*main_entity);
            }
            if let Some(phase) = mask_phase.as_mut() {
                phase.remove(*main_entity);
            }
        }
    }
}

/// Checks that each view with TAA, kjk's upscaler, DLSS or ambient
/// occlusion has the prepasses and the full coverage its feature needs.
/// Bevy's TAA, DLSS and ambient occlusion components require their
/// prepasses, so for those only the full coverage can be missing. kjk's
/// upscaler requires nothing, so all of its needs are checked.
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
