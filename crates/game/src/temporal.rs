//! Anti-aliasing as the options choose it (`display.msaa`, `Antialias`):
//! multisampling (1, 2 or 4 samples), Bevy's temporal anti-aliasing, or an
//! upscaler (`Upscaler`). The upscaler is NVIDIA's DLSS (the `dlss`
//! feature: Vulkan on Windows and Linux, an RTX card), or kjk's own
//! temporal upscaler (`upscale`), which works on every card. DLSS draws at
//! the window's size (DLAA) or at a smaller size. kjk's own upscaler draws
//! at a smaller size only. Both offer the same four smaller sizes (`Scale`).
//!
//! A temporal choice jitters the scene camera's projection each frame and
//! resolves the frames together through the motion vectors of the depth
//! and motion prepass (`MotionVectorPrepass`): every surface that writes
//! depth writes its motion there too (`material`, `motion_proxy`), and what
//! does not (glows, particles, blended sheets) moves with what is behind it.
//!
//! The scene camera draws the depth prepass on every choice, multisampling
//! too. Its main pass then shades only the surfaces it shows. The motion
//! prepass is drawn for a temporal choice alone.
//!
//! Every camera drawing onto the world's target (the scene's, the overlays
//! that draw the HUD, the screens, the fade, a movie) takes the same
//! samples and texture, as they share the world's main texture only while
//! they agree; the jitter, the prepass and the resolve are the scene
//! camera's alone, so the overlays draw crisp at the window's size after
//! the resolve (and after DLSS's upscale).
//!
//! The texture is a float one (`Hdr`) under DLSS, which asks for it, and
//! under the options' Enhanced lighting (`rolloff`), under every choice of
//! anti-aliasing. Otherwise it is an 8-bit one.
//!
//! The history the resolve keeps is dropped (`reset`) on a cut: the camera
//! jumping (a dialogue's shots, an area entered, a teleport: `CUT_METRES`
//! more than its speed allows, or `CUT_DEGREES`, in one frame, or a change
//! of field of view) or coming back
//! from not drawing (a movie, a loading screen), and whenever a view says so
//! (`TemporalCut`).
//!
//! `KJK_AA` overrides the setting for a run (`taa`, `taa_performance`,
//! `dlss_quality`, …);
//! `KJK_MOTION_DEBUG=1` draws the motion vectors over the scene instead
//! (`motion_debug.wgsl`: grey still, red right, green down, 16 pixels a
//! frame at the extremes; magenta where the camera has no prepass).

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::CameraMainTextureUsages;
use bevy::camera::Hdr;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::core_pipeline::prepass::{
    BackgroundMotionVectorsBindGroup, BackgroundMotionVectorsPipelineId, DepthPrepass,
    MotionVectorPrepass,
};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::camera::{MipBias, TemporalJitter};
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError, TextureUsages,
};
use bevy::shader::ShaderRef;

use crate::display::Display;
use crate::lifecycle_view::OverlayCamera;
use crate::upscale::Upscale;

const DEBUG_SHADER: Handle<Shader> = uuid_handle!("3c1d7e92-5a40-4f6b-9e21-8d0b6f4a2c75");

/// A camera moving farther than this in one frame cut (metres), over
/// `CUT_STEPS` times what it moved the frame before. A swoop at speed moves
/// farther than this every frame, and that is motion, not a cut.
pub const CUT_METRES: f32 = 2.0;
const CUT_STEPS: f32 = 2.0;
/// Or turning farther (degrees).
pub const CUT_DEGREES: f32 = 30.0;
/// Or its field of view changing by more (degrees): a zoom eased over
/// frames is motion, a shot's new lens is a cut.
pub const CUT_FOV_DEGREES: f32 = 2.0;

/// The frame an upscaler draws, as a share of the window's side, before it
/// upscales. These are NVIDIA's ratios for DLSS's quality modes, and kjk's
/// own upscaler offers the same sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// The window's size: anti-aliasing alone (DLAA).
    Full,
    /// Two thirds.
    Quality,
    /// 58 %.
    Balanced,
    /// A half.
    Performance,
    /// A third.
    UltraPerformance,
}

impl Scale {
    /// The side of the frame it draws, as a share of the window's.
    pub fn share(self) -> f32 {
        match self {
            Scale::Full => 1.0,
            Scale::Quality => 2.0 / 3.0,
            Scale::Balanced => 0.58,
            Scale::Performance => 0.5,
            Scale::UltraPerformance => 1.0 / 3.0,
        }
    }

    /// The mip bias NVIDIA asks of a frame drawn at that share, so its
    /// textures are as sharp as the frame shown: log2 of the share, less 1.
    /// The 1 is the bias Bevy's TAA gives a frame at the window's size.
    fn mip_bias(self) -> MipBias {
        MipBias(self.share().log2() - 1.0)
    }
}

/// What draws a smaller frame and shows it at the window's size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Upscaler {
    /// NVIDIA's DLSS, on an RTX card with a build that has it.
    Dlss,
    /// kjk's own temporal resolve (`upscale`), on every card.
    Taa,
}

/// How the scene is anti-aliased.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Antialias {
    /// Multisampling at 1 (none), 2 or 4 samples.
    Msaa(u32),
    /// Bevy's temporal anti-aliasing at the window's size.
    Taa,
    /// An upscaler drawing at a share of the window's side.
    Upscale(Upscaler, Scale),
}

/// Each choice and its setting's value, in the options' order. kjk's own
/// upscaler is not offered at the window's size, since that is TAA.
pub const CHOICES: [(Antialias, &str); 13] = [
    (Antialias::Msaa(1), "1"),
    (Antialias::Msaa(2), "2"),
    (Antialias::Msaa(4), "4"),
    (Antialias::Taa, "taa"),
    (
        Antialias::Upscale(Upscaler::Taa, Scale::Quality),
        "taa_quality",
    ),
    (
        Antialias::Upscale(Upscaler::Taa, Scale::Balanced),
        "taa_balanced",
    ),
    (
        Antialias::Upscale(Upscaler::Taa, Scale::Performance),
        "taa_performance",
    ),
    (
        Antialias::Upscale(Upscaler::Taa, Scale::UltraPerformance),
        "taa_ultra_performance",
    ),
    (Antialias::Upscale(Upscaler::Dlss, Scale::Full), "dlaa"),
    (
        Antialias::Upscale(Upscaler::Dlss, Scale::Quality),
        "dlss_quality",
    ),
    (
        Antialias::Upscale(Upscaler::Dlss, Scale::Balanced),
        "dlss_balanced",
    ),
    (
        Antialias::Upscale(Upscaler::Dlss, Scale::Performance),
        "dlss_performance",
    ),
    (
        Antialias::Upscale(Upscaler::Dlss, Scale::UltraPerformance),
        "dlss_ultra_performance",
    ),
];

impl Antialias {
    /// The choice a setting names: 4 samples when it names none.
    pub fn named(name: &str) -> Antialias {
        CHOICES
            .iter()
            .find(|(_, value)| *value == name.trim())
            .map_or(Antialias::Msaa(4), |(choice, _)| *choice)
    }

    pub fn name(self) -> &'static str {
        CHOICES
            .iter()
            .find(|(choice, _)| *choice == self)
            .map_or("4", |(_, value)| value)
    }

    /// Whether it resolves frames together (jitter, motion vectors).
    pub fn temporal(self) -> bool {
        !matches!(self, Antialias::Msaa(_))
    }

    /// The samples a pixel of a screen's 3D view takes (`gui_scene`: the
    /// main menu, the character sheet). A view draws into an image of its
    /// own, with no TAA or DLSS, so under those it takes 4.
    pub fn view_msaa(self) -> Msaa {
        if self.temporal() {
            Msaa::Sample4
        } else {
            self.msaa()
        }
    }

    /// The samples a pixel of the world's target takes.
    fn msaa(self) -> Msaa {
        match self {
            Antialias::Msaa(1) | Antialias::Taa | Antialias::Upscale(..) => Msaa::Off,
            Antialias::Msaa(2) => Msaa::Sample2,
            Antialias::Msaa(_) => Msaa::Sample4,
        }
    }
}

/// The choice this run makes: `KJK_AA`'s, else the setting's.
pub fn chosen(display: &Display) -> Antialias {
    std::env::var("KJK_AA").map_or(display.antialias, |name| Antialias::named(&name))
}

/// The choices this machine offers: DLSS's where its card and build have it.
pub fn offered(dlss: bool) -> Vec<String> {
    CHOICES
        .iter()
        .filter(|(choice, _)| dlss || !matches!(choice, Antialias::Upscale(Upscaler::Dlss, _)))
        .map(|(_, value)| value.to_string())
        .collect()
}

/// A view saying the scene camera cut (a shot it framed anew, an area
/// entered), so the resolve forgets the frames before: set it for the frame.
#[derive(Resource, Default)]
pub struct TemporalCut(pub bool);

/// What the scene camera was last frame: where, how far it had moved in
/// that frame (metres), facing, its field of view, whether it drew (`cut`).
#[derive(Component, Clone, Copy)]
struct LastPose {
    at: Vec3,
    step: f32,
    facing: Quat,
    fov: f32,
    drew: bool,
}

/// What the machine offers and the run uses (`dump`'s `antialias`).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Temporal {
    /// The choice the run asked for.
    pub asked: Antialias,
    /// What it uses. DLAA asked of a card or build without DLSS is TAA.
    /// A smaller DLSS frame is kjk's own upscaler at the same size.
    pub used: Antialias,
    /// The card and the build have DLSS.
    pub dlss: bool,
}

impl Default for Temporal {
    fn default() -> Temporal {
        Temporal {
            asked: Antialias::Msaa(4),
            used: Antialias::Msaa(4),
            dlss: false,
        }
    }
}

/// The cameras taking the choice's samples and texture (`apply`).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct Applied;

pub struct TemporalPlugin;

impl Plugin for TemporalPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, DEBUG_SHADER, "motion_debug.wgsl", Shader::from_wgsl);
        app.add_plugins((
            MaterialPlugin::<MotionDebugMaterial>::default(),
            crate::upscale::UpscalePlugin,
        ))
        .init_resource::<Temporal>()
        .init_resource::<TemporalCut>()
        .add_systems(Last, (choose, apply.in_set(Applied), cut).chain())
        .add_systems(
            PostUpdate,
            motion_debug.before(bevy::transform::TransformSystems::Propagate),
        );
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(
                bevy::render::Render,
                forget_background_motion.in_set(bevy::render::RenderSystems::Prepare),
            );
        }
        // After Bevy's DLSS has set the frame to draw, which it does before
        // the view targets are made.
        #[cfg(dlss)]
        if let Some(render_app) = app.get_sub_app_mut(bevy::render::RenderApp) {
            render_app.add_systems(
                bevy::render::Render,
                drawn_size
                    .in_set(bevy::render::RenderSystems::PrepareViews)
                    .after(bevy::render::view::prepare_view_targets),
            );
        }
    }
}

/// Takes Bevy's background motion pass off a camera that no longer draws
/// motion. Bevy gives a camera with a motion prepass a pipeline and a bind
/// group for that pass, and never takes them back. The scene camera keeps
/// its depth prepass when it leaves TAA or DLSS for another choice. Its
/// prepass then still ran the kept pipeline, which writes motion, into a
/// pass with no motion texture. The GPU refused that, and the game quit.
#[allow(clippy::type_complexity)]
fn forget_background_motion(
    views: Query<
        Entity,
        (
            Without<MotionVectorPrepass>,
            Or<(
                With<BackgroundMotionVectorsPipelineId>,
                With<BackgroundMotionVectorsBindGroup>,
            )>,
        ),
    >,
    mut commands: Commands,
) {
    for view in &views {
        commands.entity(view).try_remove::<(
            BackgroundMotionVectorsPipelineId,
            BackgroundMotionVectorsBindGroup,
        )>();
    }
}

/// Sets the frame a DLSS camera draws to its mode's share of the window
/// (`Scale::share`), which is the frame NVIDIA recommends for the
/// mode. Bevy's DLSS draws the smallest frame NVIDIA's upscaler accepts
/// for the mode instead. That is half the window's side for Quality,
/// Balanced and Performance alike, so the three modes drew the same frame.
/// Bevy keeps the size on the camera's render side, and reads it from there
/// for the jitter, the passes and the upscale, so it is set there.
#[cfg(dlss)]
fn drawn_size(
    mut views: Query<(
        &bevy::anti_alias::dlss::Dlss,
        &bevy::render::view::ExtractedView,
        &mut bevy::camera::MainPassResolutionOverride,
    )>,
) {
    use bevy::anti_alias::dlss::DlssPerfQualityMode as Mode;
    for (dlss, view, mut size) in &mut views {
        let scale = match dlss.perf_quality_mode {
            Mode::Dlaa => Scale::Full,
            Mode::Quality => Scale::Quality,
            Mode::Balanced => Scale::Balanced,
            Mode::Performance => Scale::Performance,
            Mode::UltraPerformance => Scale::UltraPerformance,
            // kjk never asks for it: Bevy's size stays.
            Mode::Auto => continue,
        };
        let drawn = crate::upscale::drawn(view.viewport.zw(), scale.share());
        if size.0 != drawn {
            size.0 = drawn;
        }
    }
}

/// Whether this card and build have DLSS's upscaling.
#[cfg(dlss)]
fn dlss_supported(world: &World) -> bool {
    world.contains_resource::<bevy::anti_alias::dlss::DlssSuperResolutionSupported>()
}

#[cfg(not(dlss))]
fn dlss_supported(_world: &World) -> bool {
    false
}

/// The project id NVIDIA's runtime asks of every application (any fixed
/// UUID; this one is kjk's). Inserted before Bevy's plugins (`runtime`).
#[cfg(dlss)]
pub fn dlss_project_id() -> bevy::anti_alias::dlss::DlssProjectId {
    bevy::anti_alias::dlss::DlssProjectId(bevy::asset::uuid::Uuid::from_u128(
        0x6b1f_3c2e_9d84_4a17_b5e0_2f7c_8a91_d463,
    ))
}

/// What the run uses, from what it asks and the machine has.
fn choose(world: &mut World) {
    // A run without the options (a viewer) has the defaults.
    let asked = chosen(
        &world
            .get_resource::<Display>()
            .cloned()
            .unwrap_or_else(Display::default),
    );
    let dlss = dlss_supported(world);
    let used = match asked {
        Antialias::Upscale(Upscaler::Dlss, Scale::Full) if !dlss => Antialias::Taa,
        Antialias::Upscale(Upscaler::Dlss, scale) if !dlss => {
            Antialias::Upscale(Upscaler::Taa, scale)
        }
        other => other,
    };
    let now = Temporal { asked, used, dlss };
    let mut temporal = world.resource_mut::<Temporal>();
    if *temporal != now {
        if asked != used {
            warn!(target: "display", ?asked, ?used, "DLSS is not available here, so TAA draws instead");
        }
        info!(target: "display", ?used, "anti-aliasing");
        *temporal = now;
    }
}

/// Whether a camera draws onto the world's target: the window's, or the
/// headless frame's image.
fn on_world_target(
    target: Option<&RenderTarget>,
    capture: Option<&console::CaptureTarget>,
) -> bool {
    match target {
        None | Some(RenderTarget::Window(_)) => true,
        Some(RenderTarget::Image(image)) => matches!(
            capture,
            Some(console::CaptureTarget::Image(handle)) if handle.id() == image.handle.id()
        ),
        Some(_) => false,
    }
}

/// Every camera on the world's target takes the samples and texture of the
/// choice; the scene camera its jitter, prepass and resolve.
#[allow(clippy::type_complexity)]
fn apply(
    temporal: Res<Temporal>,
    lighting: Option<Res<crate::rolloff::LightingMode>>,
    capture: Option<Res<console::CaptureTarget>>,
    mut cameras: Query<(
        Entity,
        &mut Msaa,
        Option<&RenderTarget>,
        Has<Camera3d>,
        Has<OverlayCamera>,
        Has<Hdr>,
        Has<TemporalAntiAliasing>,
        Option<&DlssOn>,
        &mut CameraMainTextureUsages,
        Has<TemporalJitter>,
        Has<DepthPrepass>,
        Option<&Upscale>,
    )>,
    mut commands: Commands,
) {
    let used = temporal.used;
    let dlss = matches!(used, Antialias::Upscale(Upscaler::Dlss, _));
    // A run without the options (a viewer) has Classic lighting.
    let hdr = dlss || lighting.is_some_and(|lighting| lighting.enhanced());
    // Leaving DLSS takes two frames. Bevy clears its DLSS state from a
    // camera's render side (NVIDIA's feature and the smaller rectangle the
    // scene is drawn in) only while the camera is still HDR. If HDR went in
    // the same frame as DLSS, that state would stay. A 4x frame would then
    // be drawn in the smaller rectangle, and a TAA frame would be black. So
    // this frame only DLSS goes, and the next frame the cameras take the
    // new choice.
    if !dlss {
        let mut leaving = false;
        for (entity, .., dlss_on, _, _, _, _) in &cameras {
            if dlss_on.is_some() {
                remove_dlss(&mut commands.entity(entity));
                leaving = true;
            }
        }
        if leaving {
            return;
        }
    }
    // Bevy's DLSS adds the storage usage to the scene camera's main
    // texture, since NVIDIA's upscaler writes its frame there. Bevy shares
    // a main texture only between cameras that ask for the same usages.
    // Without the storage usage, the HUD's camera gets an empty texture of
    // its own, and its frame replaces the world with black. So every
    // camera on the world's target asks for the same usages as the scene
    // camera while DLSS is on.
    let usages = if dlss {
        CameraMainTextureUsages::default().with(TextureUsages::STORAGE_BINDING)
    } else {
        CameraMainTextureUsages::default()
    };
    for (
        entity,
        mut msaa,
        target,
        is_3d,
        overlay,
        has_hdr,
        has_taa,
        dlss_on,
        mut main_usages,
        has_jitter,
        has_depth,
        upscale,
    ) in &mut cameras
    {
        if !on_world_target(target, capture.as_deref()) {
            continue;
        }
        if *msaa != used.msaa() {
            *msaa = used.msaa();
        }
        if main_usages.0 != usages.0 {
            main_usages.0 = usages.0;
        }
        let mut camera = commands.entity(entity);
        // A camera may be gone by the time this is applied. The menu's
        // camera is, in the frame a session's scene camera takes its place
        // (`scene::replace_camera`).
        if hdr != has_hdr {
            if hdr {
                camera.try_insert(Hdr);
            } else {
                camera.try_remove::<Hdr>();
            }
        }
        if !is_3d || overlay {
            continue;
        }
        let taa = used == Antialias::Taa;
        if taa && !has_taa {
            // DLSS's own bias, if it was DLSS, goes with it.
            camera.insert((TemporalAntiAliasing::default(), MipBias::default()));
        }
        if !taa && has_taa {
            camera.remove::<TemporalAntiAliasing>();
        }
        if let Antialias::Upscale(Upscaler::Dlss, scale) = used {
            // Only when it changes: a new `Dlss` makes NVIDIA's feature anew.
            if dlss_on != Some(&DlssOn(scale)) {
                insert_dlss(&mut camera, scale);
                // Bevy's DLSS sets its bias only in the frame it makes
                // NVIDIA's feature. Every frame after, the camera's own is
                // extracted over it. So the camera carries the bias itself.
                camera.insert(scale.mip_bias());
            }
        }
        // kjk's own upscaler: its size, jitter, motion prepass and bias,
        // all anew when the size changes.
        let own = match used {
            Antialias::Upscale(Upscaler::Taa, scale) => Some(scale),
            _ => None,
        };
        match (own, upscale) {
            (Some(scale), Some(upscale)) if upscale.scale == scale.share() => {}
            (Some(scale), _) => {
                camera.insert((
                    Upscale {
                        scale: scale.share(),
                        reset: true,
                    },
                    TemporalJitter::default(),
                    scale.mip_bias(),
                    MotionVectorPrepass,
                ));
            }
            (None, Some(_)) => {
                camera.remove::<Upscale>();
            }
            (None, None) => {}
        }
        // The scene camera draws depth first on every choice, so its main
        // pass shades only the surfaces it shows. Without it a pixel is
        // shaded once for each surface drawn there, hidden ones too, and
        // the characters' shadows are traced for each (`shadows`).
        if !has_depth {
            camera.insert(DepthPrepass);
        }
        // The jitter is there only after TAA or DLSS, with their motion
        // prepass.
        if !used.temporal() && has_jitter {
            camera.remove::<(TemporalJitter, MipBias, MotionVectorPrepass)>();
        }
        if used.temporal() {
            camera.insert_if_new(LastPose {
                at: Vec3::ZERO,
                step: 0.0,
                facing: Quat::IDENTITY,
                fov: 0.0,
                drew: false,
            });
        }
    }
}

/// A scene camera DLSS draws for, at its size.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct DlssOn(Scale);

#[cfg(dlss)]
fn insert_dlss(camera: &mut EntityCommands, scale: Scale) {
    use bevy::anti_alias::dlss::{Dlss, DlssPerfQualityMode};
    let perf_quality_mode = match scale {
        Scale::Full => DlssPerfQualityMode::Dlaa,
        Scale::Quality => DlssPerfQualityMode::Quality,
        Scale::Balanced => DlssPerfQualityMode::Balanced,
        Scale::Performance => DlssPerfQualityMode::Performance,
        Scale::UltraPerformance => DlssPerfQualityMode::UltraPerformance,
    };
    camera.insert((
        Dlss {
            perf_quality_mode,
            ..default()
        },
        DlssOn(scale),
    ));
}

#[cfg(not(dlss))]
fn insert_dlss(camera: &mut EntityCommands, scale: Scale) {
    // Never reached: `choose` uses TAA where the build has no DLSS.
    camera.insert(DlssOn(scale));
}

#[cfg(dlss)]
fn remove_dlss(camera: &mut EntityCommands) {
    camera.remove::<(bevy::anti_alias::dlss::Dlss, DlssOn)>();
    camera.remove::<bevy::camera::MainPassResolutionOverride>();
}

#[cfg(not(dlss))]
fn remove_dlss(camera: &mut EntityCommands) {
    camera.remove::<DlssOn>();
}

/// Drops the resolve's history on a cut (`TemporalCut`, a jump, a lens
/// changed, the camera drawing again), and keeps this frame's pose.
#[allow(clippy::type_complexity)]
fn cut(
    mut asked: ResMut<TemporalCut>,
    mut cameras: Query<
        (
            &Camera,
            &GlobalTransform,
            &Projection,
            &mut LastPose,
            Option<&mut TemporalAntiAliasing>,
            Option<&mut Upscale>,
            Has<DlssOn>,
        ),
        Without<OverlayCamera>,
    >,
    #[cfg(dlss)] mut dlss: Query<&mut bevy::anti_alias::dlss::Dlss>,
) {
    let told = std::mem::take(&mut asked.0);
    for (camera, transform, projection, mut last, taa, upscale, has_dlss) in &mut cameras {
        let at = transform.translation();
        let facing = transform.rotation();
        let fov = match projection {
            Projection::Perspective(perspective) => perspective.fov.to_degrees(),
            _ => 0.0,
        };
        let step = at.distance(last.at);
        let jumped = step > CUT_METRES + CUT_STEPS * last.step
            || facing.angle_between(last.facing).to_degrees() > CUT_DEGREES
            || (fov - last.fov).abs() > CUT_FOV_DEGREES;
        let reset = told || jumped || !last.drew;
        *last = LastPose {
            at,
            // A cut's own jump is no speed to measure the next frame by.
            step: if reset { 0.0 } else { step },
            facing,
            fov,
            drew: camera.is_active,
        };
        if !reset {
            continue;
        }
        if let Some(mut taa) = taa {
            taa.reset = true;
        }
        if let Some(mut upscale) = upscale {
            upscale.reset = true;
        }
        #[cfg(dlss)]
        if has_dlss {
            for mut dlss in &mut dlss {
                dlss.reset = true;
            }
        }
        let _ = has_dlss;
    }
}

/// The motion vectors drawn over the scene (`KJK_MOTION_DEBUG`).
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
pub struct MotionDebugMaterial {}

/// Drawn after every transparent surface (the sorted phase draws the least
/// first).
const LAST: f32 = 1.0e9;

impl Material for MotionDebugMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(DEBUG_SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(DEBUG_SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }

    fn depth_bias(&self) -> f32 {
        LAST
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = None;
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

#[derive(Component)]
struct MotionDebugQuad;

/// The debug quad under the scene camera, while `KJK_MOTION_DEBUG` is set.
fn motion_debug(
    cameras: Query<Entity, crate::lifecycle_view::SceneCamera>,
    quads: Query<(Entity, &ChildOf), With<MotionDebugQuad>>,
    mut materials: ResMut<Assets<MotionDebugMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
    mut on: Local<Option<bool>>,
) {
    let on =
        *on.get_or_insert_with(|| std::env::var_os("KJK_MOTION_DEBUG").is_some_and(|v| v == "1"));
    if !on {
        return;
    }
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    let mut found = false;
    for (entity, parent) in &quads {
        if parent.parent() == camera {
            found = true;
        } else {
            commands.entity(entity).despawn();
        }
    }
    if !found {
        commands.entity(camera).with_child((
            Mesh3d(meshes.add(Rectangle::new(2.0, 2.0))),
            MeshMaterial3d(materials.add(MotionDebugMaterial {})),
            MotionDebugQuad,
            Transform::IDENTITY,
            Visibility::Inherited,
            NoFrustumCulling,
        ));
    }
}
