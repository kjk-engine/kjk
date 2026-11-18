//! The scene camera's prepasses, derived every frame from what the frame's
//! features need. A prepass draws the scene before the main pass. There are
//! three, as in Bevy:
//!
//! - The depth prepass (`DepthPrepass`) writes depth. Its coverage
//!   (`DepthCoverage`) says which meshes it draws: none (there is no depth
//!   prepass), only the rooms' meshes (`Rooms`), or every mesh (`Full`).
//! - The normal prepass (`NormalPrepass`) also writes each pixel's normal.
//! - The motion prepass (`MotionVectorPrepass`) also writes how far each
//!   pixel moved since the frame before.
//!
//! No setting chooses them. Each feature that reads a prepass declares what
//! it needs in `PrepassNeeds` each frame, and `derive` gives the scene
//! camera the union of the needs. A full depth prepass wins over a rooms
//! one.
//!
//! - TAA, kjk's upscaler, DLAA and DLSS (`temporal`) need a full depth
//!   prepass and the motion prepass (`Need::TEMPORAL`).
//! - Ambient occlusion (`ambient_occlusion`) needs a full depth prepass and
//!   the normal prepass (`Need::AMBIENT_OCCLUSION`).
//! - The motion debug view (`KJK_MOTION_DEBUG=1`) needs what TAA needs
//!   while it is shown.
//! - The GPU load check (`LoadCheck`) needs a rooms depth prepass while the
//!   GPU is close to holding the frame rate back. The main pass then shades
//!   only the surfaces the rooms do not hide. Without a depth prepass it
//!   shades every surface drawn at a pixel, hidden ones too, and traces the
//!   characters' capsule shadows for each (`shadows`).
//!
//! Grass's motion twins (`grass`) and the motion proxies (`motion_proxy`)
//! draw into the motion prepass only while the camera has one. They follow
//! it and never cause it.
//!
//! A prepass that writes normals or motion draws every mesh, so a rooms
//! coverage is used only when neither is needed. With a rooms coverage, the
//! meshes that are not a room's are taken out of the prepass's phases each
//! frame, after Bevy queues them (`keep_rooms_only`). Bevy keeps its phases
//! from frame to frame and queues a mesh again only when it becomes visible
//! or its pipeline is specialized anew. So when a view's coverage changes,
//! its pipelines are all specialized anew (`respecialize_on_change`), and
//! every visible mesh is queued again in that same frame.
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

use crate::gpu_load::GpuLoad;

/// The GPU's share of the render thread's frame time at or above which the
/// GPU load check asks for the rooms' depth prepass.
pub const NEED_SHARE: f64 = 0.8;
/// The share at or below which it lets the prepass go. Dropping the prepass
/// raises the GPU's time, so this is well below `NEED_SHARE`: the GPU's time
/// can grow by 60% before the check asks for the prepass again.
pub const DROP_SHARE: f64 = 0.5;
/// The time the shares are averaged over before the check decides.
const WINDOW: Duration = Duration::from_millis(500);
/// The check holds its answer at least this long after a change.
const HOLD: Duration = Duration::from_secs(2);
/// A prepass asked for again this soon after it was let go bounced: the
/// GPU needed it after all. The check then keeps it longer before it may
/// let it go again, from `FIRST_BACKOFF`, doubled at each bounce, up to
/// `MAX_BACKOFF`.
const BOUNCE: Duration = Duration::from_secs(10);
const FIRST_BACKOFF: Duration = Duration::from_secs(15);
const MAX_BACKOFF: Duration = Duration::from_secs(300);

/// Which meshes the depth prepass draws.
#[derive(
    Component, ExtractComponent, Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord,
)]
pub enum DepthCoverage {
    /// No depth prepass.
    #[default]
    None,
    /// Only the rooms' meshes.
    Rooms,
    /// Every mesh.
    Full,
}

impl DepthCoverage {
    pub fn name(self) -> &'static str {
        match self {
            DepthCoverage::None => "none",
            DepthCoverage::Rooms => "rooms",
            DepthCoverage::Full => "full",
        }
    }

    fn named(name: &str) -> Option<DepthCoverage> {
        [
            DepthCoverage::None,
            DepthCoverage::Rooms,
            DepthCoverage::Full,
        ]
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

/// A camera whose prepasses `derive` sets: the scene camera on the world's
/// target (`temporal::apply`).
#[derive(Component, Default)]
pub struct DerivedPrepasses;

/// What `derive` gave the scene camera last, why it last changed, and how often each
/// coverage was used since the run began.
#[derive(Resource, Default)]
pub struct Prepasses {
    pub depth: DepthCoverage,
    pub normals: bool,
    pub motion: bool,
    why: String,
    /// Frames at none, rooms and full.
    frames: [u64; 3],
    switches: u64,
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

/// The GPU load check: whether the GPU needs the rooms' depth prepass.
#[derive(Resource)]
struct LoadCheck {
    rooms: bool,
    /// The device times the GPU, and has measured a window.
    measured: bool,
    /// The last window's GPU ms and render ms, per frame.
    last: Option<(f64, f64)>,
    window_started: Instant,
    gpu_ms: f64,
    gpu_frames: u32,
    render_ms: f64,
    render_frames: u32,
    hold_until: Instant,
    dropped_at: Option<Instant>,
    backoff: Duration,
}

impl Default for LoadCheck {
    fn default() -> LoadCheck {
        let now = Instant::now();
        LoadCheck {
            rooms: true,
            measured: false,
            last: None,
            window_started: now,
            gpu_ms: 0.0,
            gpu_frames: 0,
            render_ms: 0.0,
            render_frames: 0,
            hold_until: now,
            dropped_at: None,
            backoff: FIRST_BACKOFF,
        }
    }
}

impl LoadCheck {
    /// Why the check asks for what it asks, for the log.
    fn reason(&self) -> String {
        match (self.measured, self.last) {
            (true, Some((gpu, render))) => format!(
                "GPU {gpu:.2} ms of a {render:.2} ms render frame ({:.0}%)",
                100.0 * gpu / render.max(1e-6)
            ),
            _ => "no GPU timing yet".into(),
        }
    }

    /// Takes in what the GPU and the render thread measured, and decides
    /// once a window is full.
    fn update(&mut self, measured: crate::gpu_load::Measured, now: Instant) {
        self.gpu_ms += measured.gpu_ms;
        self.gpu_frames += measured.gpu_frames;
        self.render_ms += measured.render_ms;
        self.render_frames += measured.render_frames;
        if now.duration_since(self.window_started) < WINDOW {
            return;
        }
        let window = (self.gpu_frames > 0 && self.render_frames > 0).then(|| {
            (
                self.gpu_ms / f64::from(self.gpu_frames),
                self.render_ms / f64::from(self.render_frames),
            )
        });
        self.window_started = now;
        self.gpu_ms = 0.0;
        self.gpu_frames = 0;
        self.render_ms = 0.0;
        self.render_frames = 0;
        let Some((gpu, render)) = window.filter(|_| measured.timed) else {
            // A device that cannot time the GPU keeps the rooms' prepass.
            if !measured.timed {
                self.measured = false;
                self.rooms = true;
            }
            return;
        };
        self.measured = true;
        self.last = Some((gpu, render));
        if now < self.hold_until {
            return;
        }
        let share = gpu / render.max(1e-6);
        if !self.rooms && share >= NEED_SHARE {
            self.rooms = true;
            self.hold_until = now + HOLD;
            if self
                .dropped_at
                .is_some_and(|dropped| now.duration_since(dropped) < BOUNCE)
            {
                self.hold_until = now + self.backoff;
                self.backoff = (self.backoff * 2).min(MAX_BACKOFF);
            }
        } else if self.rooms && share <= DROP_SHARE {
            self.rooms = false;
            self.hold_until = now + HOLD;
            self.dropped_at = Some(now);
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
                keep_rooms_only
                    .in_set(RenderSystems::QueueMeshes)
                    .after(bevy::pbr::queue_prepass_material_meshes),
            ),
        );
        #[cfg(debug_assertions)]
        render_app.add_systems(Render, needs_met.in_set(RenderSystems::PrepareViews));
    }
}

fn check_gpu_load(load: Option<Res<GpuLoad>>, mut check: ResMut<LoadCheck>) {
    let Some(load) = load else { return };
    check.update(load.take(), Instant::now());
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
        let wants_depth = depth != DepthCoverage::None;
        if wants_depth && !has_depth {
            camera.try_insert(DepthPrepass);
        }
        if !wants_depth && has_depth {
            camera.try_remove::<DepthPrepass>();
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
    if cameras.is_empty() {
        return;
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
    prepasses.frames[depth as usize] += 1;
}

/// For a dump, `depth_prepass`: the coverage now and why, whether the
/// normal and motion prepasses are drawn, the frames at each coverage and
/// the changes since the run began, and the GPU load check's last window.
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
        }),
    );
}

/// Specializes every pipeline of a view anew when its coverage changes, so
/// Bevy queues all its visible meshes again: the meshes a rooms coverage
/// took out come back at once, and those a new rooms coverage leaves out
/// are taken out at once.
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

/// Takes out of a rooms coverage's prepass every mesh Bevy queued this
/// frame that is not a room's. It meets exactly the meshes Bevy queued,
/// in the order Bevy met them.
fn keep_rooms_only(
    views: Query<(&ExtractedView, &RenderVisibleEntities, &DepthCoverage)>,
    dirty: Res<DirtySpecializations>,
    pending: Res<PendingPrepassMeshMaterialQueues>,
    meshes: Res<RenderMeshInstances>,
    mut opaque: ResMut<ViewBinnedRenderPhases<Opaque3dPrepass>>,
    mut mask: ResMut<ViewBinnedRenderPhases<AlphaMask3dPrepass>>,
) {
    for (view, visible, coverage) in &views {
        if *coverage != DepthCoverage::Rooms {
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
            let room = meshes
                .render_mesh_queue_data(*main_entity)
                .is_some_and(|mesh| is_room(mesh.tag()));
            if room {
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
