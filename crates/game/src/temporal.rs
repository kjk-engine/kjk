//! Anti-aliasing as the options choose it (`display.msaa`, `Antialias`):
//! multisampling (1, 2 or 4 samples), Bevy's temporal anti-aliasing, or
//! NVIDIA's DLSS (the `dlss` feature: Vulkan on Windows and Linux, an RTX
//! card), DLAA at the window's size or upscaled from a smaller frame.
//!
//! A temporal choice jitters the scene camera's projection each frame and
//! resolves the frames together through the motion vectors of the depth
//! and motion prepass (`MotionVectorPrepass`): every surface that writes
//! depth writes its motion there too (`material`, `motion_proxy`), and what
//! does not (glows, particles, blended sheets) moves with what is behind it.
//!
//! Every camera drawing onto the world's target (the scene's, the overlays
//! that draw the HUD, the screens, the fade, a movie) takes the same
//! samples and texture, as they share the world's main texture only while
//! they agree; the jitter, the prepass and the resolve are the scene
//! camera's alone, so the overlays draw crisp at the window's size after
//! the resolve (and after DLSS's upscale).
//!
//! The history the resolve keeps is dropped (`reset`) on a cut: the camera
//! jumping (a dialogue's shots, an area entered, a teleport, `CUT_METRES`
//! or `CUT_DEGREES` in one frame, a change of field of view) or coming back
//! from not drawing (a movie, a loading screen), and whenever a view says so
//! (`TemporalCut`).
//!
//! `KJK_AA` overrides the setting for a run (`taa`, `dlss_quality`, …);
//! `KJK_MOTION_DEBUG=1` draws the motion vectors over the scene instead
//! (`motion_debug.wgsl`: grey still, red right, green down, 16 pixels a
//! frame at the extremes; magenta where the camera has no prepass).

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::RenderTarget;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::camera::{MipBias, TemporalJitter};
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::camera::Hdr;

use crate::display::Display;
use crate::lifecycle_view::OverlayCamera;

const DEBUG_SHADER: Handle<Shader> = uuid_handle!("3c1d7e92-5a40-4f6b-9e21-8d0b6f4a2c75");

/// A camera moving farther than this in one frame cut (metres).
pub const CUT_METRES: f32 = 2.0;
/// Or turning farther (degrees).
pub const CUT_DEGREES: f32 = 30.0;
/// Or its field of view changing by more (degrees): a zoom eased over
/// frames is motion, a shot's new lens is a cut.
pub const CUT_FOV_DEGREES: f32 = 2.0;

/// DLSS's quality modes: the frame it draws, as a share of the window's
/// side, before it upscales (NVIDIA's ratios).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DlssQuality {
    /// The window's size: anti-aliasing alone.
    Dlaa,
    /// Two thirds.
    Quality,
    /// 58 %.
    Balanced,
    /// A half.
    Performance,
    /// A third.
    UltraPerformance,
}

/// How the scene is anti-aliased.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Antialias {
    /// Multisampling at 1 (none), 2 or 4 samples.
    Msaa(u32),
    Taa,
    Dlss(DlssQuality),
}

/// Each choice and its setting's value, in the options' order.
pub const CHOICES: [(Antialias, &str); 10] = [
    (Antialias::Msaa(1), "1"),
    (Antialias::Msaa(2), "2"),
    (Antialias::Msaa(4), "4"),
    (Antialias::Taa, "taa"),
    (Antialias::Dlss(DlssQuality::Dlaa), "dlaa"),
    (Antialias::Dlss(DlssQuality::Quality), "dlss_quality"),
    (Antialias::Dlss(DlssQuality::Balanced), "dlss_balanced"),
    (Antialias::Dlss(DlssQuality::Performance), "dlss_performance"),
    (
        Antialias::Dlss(DlssQuality::UltraPerformance),
        "dlss_ultra_performance",
    ),
    (Antialias::Msaa(4), ""),
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

    /// The samples a pixel of the world's target takes.
    fn msaa(self) -> Msaa {
        match self {
            Antialias::Msaa(1) | Antialias::Taa | Antialias::Dlss(_) => Msaa::Off,
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
        .filter(|(choice, value)| {
            !value.is_empty() && (dlss || !matches!(choice, Antialias::Dlss(_)))
        })
        .map(|(_, value)| value.to_string())
        .collect()
}

/// A view saying the scene camera cut (a shot it framed anew, an area
/// entered), so the resolve forgets the frames before: set it for the frame.
#[derive(Resource, Default)]
pub struct TemporalCut(pub bool);

/// What the scene camera was last frame: where, facing, its field of view,
/// whether it drew (`cut`).
#[derive(Component, Clone, Copy)]
struct LastPose {
    at: Vec3,
    facing: Quat,
    fov: f32,
    drew: bool,
}

/// What the machine offers and the run uses (`dump`'s `antialias`).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct Temporal {
    /// The choice the run asked for.
    pub asked: Antialias,
    /// What it uses: DLSS asked of a card or build without it is TAA.
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

pub struct TemporalPlugin;

impl Plugin for TemporalPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, DEBUG_SHADER, "motion_debug.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<MotionDebugMaterial>::default())
            .init_resource::<Temporal>()
            .init_resource::<TemporalCut>()
            .add_systems(Last, (choose, apply, cut).chain())
            .add_systems(
                PostUpdate,
                motion_debug.before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// Whether this card and build have DLSS's upscaling.
#[cfg(feature = "dlss")]
fn dlss_supported(world: &World) -> bool {
    world.contains_resource::<bevy::anti_alias::dlss::DlssSuperResolutionSupported>()
}

#[cfg(not(feature = "dlss"))]
fn dlss_supported(_world: &World) -> bool {
    false
}

/// The project id NVIDIA's runtime asks of every application (any fixed
/// UUID; this one is kjk's). Inserted before Bevy's plugins (`runtime`).
#[cfg(feature = "dlss")]
pub fn dlss_project_id() -> bevy::anti_alias::dlss::DlssProjectId {
    bevy::anti_alias::dlss::DlssProjectId(bevy::asset::uuid::Uuid::from_u128(
        0x6b1f_3c2e_9d84_4a17_b5e0_2f7c_8a91_d463,
    ))
}

/// What the run uses, from what it asks and the machine has.
fn choose(world: &mut World) {
    // A run without the options (a viewer) has the defaults.
    let asked = chosen(&world.get_resource::<Display>().cloned().unwrap_or_else(Display::default));
    let dlss = dlss_supported(world);
    let used = match asked {
        Antialias::Dlss(_) if !dlss => Antialias::Taa,
        other => other,
    };
    let now = Temporal { asked, used, dlss };
    let mut temporal = world.resource_mut::<Temporal>();
    if *temporal != now {
        if asked != used {
            warn!(target: "display", ?asked, ?used, "DLSS is not available here: TAA instead");
        }
        info!(target: "display", ?used, "anti-aliasing");
        *temporal = now;
    }
}

/// Whether a camera draws onto the world's target: the window's, or the
/// headless frame's image.
fn on_world_target(target: Option<&RenderTarget>, capture: Option<&console::CaptureTarget>) -> bool {
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
    )>,
    mut commands: Commands,
) {
    let used = temporal.used;
    let hdr = matches!(used, Antialias::Dlss(_));
    for (entity, mut msaa, target, is_3d, overlay, has_hdr, has_taa, dlss_on) in &mut cameras {
        let has_dlss = dlss_on.is_some();
        if !on_world_target(target, capture.as_deref()) {
            continue;
        }
        if *msaa != used.msaa() {
            *msaa = used.msaa();
        }
        let mut camera = commands.entity(entity);
        if hdr != has_hdr {
            if hdr {
                camera.insert(Hdr);
            } else {
                camera.remove::<Hdr>();
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
        if let Antialias::Dlss(quality) = used {
            // Only when it changes: a new `Dlss` makes NVIDIA's feature anew.
            if dlss_on != Some(&DlssOn(quality)) {
                insert_dlss(&mut camera, quality);
            }
        } else if has_dlss {
            remove_dlss(&mut camera);
        }
        if !used.temporal() && (has_taa || has_dlss) {
            camera.remove::<(TemporalJitter, MipBias, DepthPrepass, MotionVectorPrepass)>();
        }
        if used.temporal() {
            camera.insert_if_new(LastPose {
                at: Vec3::ZERO,
                facing: Quat::IDENTITY,
                fov: 0.0,
                drew: false,
            });
        }
    }
}

/// A scene camera DLSS draws for, at its quality.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct DlssOn(DlssQuality);

#[cfg(feature = "dlss")]
fn insert_dlss(camera: &mut EntityCommands, quality: DlssQuality) {
    use bevy::anti_alias::dlss::{Dlss, DlssPerfQualityMode};
    let perf_quality_mode = match quality {
        DlssQuality::Dlaa => DlssPerfQualityMode::Dlaa,
        DlssQuality::Quality => DlssPerfQualityMode::Quality,
        DlssQuality::Balanced => DlssPerfQualityMode::Balanced,
        DlssQuality::Performance => DlssPerfQualityMode::Performance,
        DlssQuality::UltraPerformance => DlssPerfQualityMode::UltraPerformance,
    };
    camera.insert((
        Dlss {
            perf_quality_mode,
            ..default()
        },
        DlssOn(quality),
    ));
}

#[cfg(not(feature = "dlss"))]
fn insert_dlss(camera: &mut EntityCommands, quality: DlssQuality) {
    // Never reached: `choose` uses TAA where the build has no DLSS.
    camera.insert(DlssOn(quality));
}

#[cfg(feature = "dlss")]
fn remove_dlss(camera: &mut EntityCommands) {
    camera.remove::<(bevy::anti_alias::dlss::Dlss, DlssOn)>();
    camera.remove::<bevy::render::camera::MainPassResolutionOverride>();
}

#[cfg(not(feature = "dlss"))]
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
            Has<DlssOn>,
        ),
        Without<OverlayCamera>,
    >,
    #[cfg(feature = "dlss")] mut dlss: Query<&mut bevy::anti_alias::dlss::Dlss>,
) {
    let told = std::mem::take(&mut asked.0);
    for (camera, transform, projection, mut last, taa, has_dlss) in &mut cameras {
        let at = transform.translation();
        let facing = transform.rotation();
        let fov = match projection {
            Projection::Perspective(perspective) => perspective.fov.to_degrees(),
            _ => 0.0,
        };
        let jumped = at.distance(last.at) > CUT_METRES
            || facing.angle_between(last.facing).to_degrees() > CUT_DEGREES
            || (fov - last.fov).abs() > CUT_FOV_DEGREES;
        let reset = told || jumped || !last.drew;
        *last = LastPose {
            at,
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
        #[cfg(feature = "dlss")]
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
    let on = *on.get_or_insert_with(|| std::env::var_os("KJK_MOTION_DEBUG").is_some_and(|v| v == "1"));
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
