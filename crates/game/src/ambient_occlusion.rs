//! The options' Ambient Occlusion (`display.ambient_occlusion`), step 8 of
//! the lighting plan.
//!
//! Ambient occlusion darkens the room's light where something stands close
//! to a surface: under a chin, in an armpit, where a crate meets a wall or a
//! floor. The room's light is the area's flat ambient, the light probes'
//! light (`light_probes`) and the lightmap. A lamp's light, a fight's light
//! and a surface's own glow are never darkened (`odyssey.wgsl`,
//! `room_light_kept`). A lightmapped surface takes the occlusion at
//! `LIGHTMAP_SHARE` of its strength, which is all of it.
//!
//! It works only under Enhanced lighting, and only under TAA, DLSS or kjk's
//! own upscaler. Under Classic lighting, or under multisampling, the option
//! draws nothing. A card that cannot write 5 storage textures in one shader
//! has none either, as Bevy's own pass has none there.
//!
//! The occlusion is worked out from the scene camera's depth and normal
//! prepass, before the main pass, by Bevy 0.19.1's own method: its
//! `ScreenSpaceAmbientOcclusion` pass. Its three compute passes are run here
//! from a copy of its shaders (`ambient_occlusion.wgsl`), for one reason.
//! Bevy's passes measure a place on the frame by the window's size. Under an
//! upscaler the camera draws a smaller frame, in the top left corner of the
//! window-sized depth and normal textures. Bevy's passes then work out each
//! pixel's place in the room as if the frame filled the window, so the
//! occlusion comes out wrong, and they also run over the whole window. The
//! copy measures places on the drawn frame and runs over it only. The copy
//! also changes one count. Bevy's method hid a sliver of the horizon on
//! every flat surface, so a floor in the open kept only 0.83 of the room's
//! light. The copy keeps about 0.93 there (`ambient_occlusion.wgsl`,
//! `SLIVER`).
//!
//! Everything else is Bevy's. The camera carries Bevy's
//! `ScreenSpaceAmbientOcclusion`, which brings the normal prepass, the
//! textures and their bindings, and binds the result with the view, where
//! `odyssey.wgsl` reads it. Bevy's own dispatch is held back each frame
//! (`hold_back`), and this module's dispatch (`occlude`) runs in its place,
//! between the prepass and the main pass, with Bevy's bind groups.
//!
//! Its cost, read from the shaders at Bevy's High quality. The depth pass
//! reads each drawn pixel's depth once and writes 5 levels of it. The
//! occlusion pass reads 36 depths a pixel (3 directions, 3 steps each way,
//! 2 points a step) from those levels, one normal and 4 depths for the
//! edges, and writes 2 values. The blur reads 9 occlusion values and 3 edge
//! values a pixel and writes 1. The normal prepass adds a normal written
//! for each fragment of the depth prepass (4 bytes a pixel). Under an
//! upscaler all of it runs over the smaller frame.
//!
//! Switches for comparing pictures (`docs/RUN.md`): `KJK_AO=on` or `off`
//! overrides the setting for a run, `KJK_AO_DEBUG=1` shows the occlusion
//! alone and `KJK_AO_DEBUG=2` the prepass's normals, `KJK_AO_LIGHTMAP` sets the lightmapped surfaces' share and
//! `KJK_AO_CONTACT` the share kept of step 2's contact darkening.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::MainPassResolutionOverride;
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::pbr::{
    ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel, SsaoBindGroups,
    SsaoPipelineId,
};
use bevy::prelude::*;
use bevy::render::camera::{ExtractedCamera, TemporalJitter};
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::globals::GlobalsUniform;
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_depth_2d, texture_storage_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedComputePipelineId,
    ComputePassDescriptor, ComputePipelineDescriptor, PipelineCache, SamplerBindingType,
    ShaderStages, StorageTextureAccess, TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderAdapter, RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ViewUniform, ViewUniformOffset};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy::shader::ShaderDefVal;

use crate::lifecycle_view::SceneCamera;
use crate::rolloff::LightingMode;
use crate::temporal::Temporal;

const SHADER: Handle<Shader> = uuid_handle!("5d0e8b37-a1c4-4f92-8e6b-3c7a9f2d1b48");

/// The share of the occlusion a lightmapped surface takes (0 to 1). The
/// lightmaps hold about four texels a triangle, so they darken corners
/// little, and they hold nothing of the characters and placeables. The
/// owner chose 1 from captures at 0, 0.5 and 1 on 2026-11-14.
pub const LIGHTMAP_SHARE: f32 = 1.0;

/// The share of step 2's contact darkening (`odyssey.wgsl`, `contact_shade`)
/// kept while ambient occlusion is on (0 to 1). 1 keeps all of it.
pub const CONTACT_SHARE: f32 = 1.0;

/// How thick Bevy's pass takes everything in view to be (metres). A point
/// farther than this behind a surface in view is not hidden by it. Bevy's
/// own default.
const THICKNESS: f32 = 0.25;

/// The quality the pass runs at: Bevy's High, 3 directions a pixel with 3
/// steps each way.
const QUALITY: ScreenSpaceAmbientOcclusionQualityLevel =
    ScreenSpaceAmbientOcclusionQualityLevel::High;

/// The options' Ambient Occlusion (`settings`, `display.ambient_occlusion`).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AmbientOcclusion {
    /// No ambient occlusion. The default until the owner has looked.
    #[default]
    Off,
    /// Ambient occlusion under Enhanced lighting and TAA, DLSS or the
    /// upscaler.
    On,
}

impl AmbientOcclusion {
    /// The value named by a value from the settings file.
    pub fn named(value: &str) -> AmbientOcclusion {
        match value {
            "on" => AmbientOcclusion::On,
            _ => AmbientOcclusion::Off,
        }
    }

    /// Its value in the settings file.
    pub fn name(self) -> &'static str {
        match self {
            AmbientOcclusion::Off => "off",
            AmbientOcclusion::On => "on",
        }
    }
}

/// The setting this run uses: `KJK_AO`'s (`on` or `off`), else the
/// options'.
fn chosen(setting: AmbientOcclusion) -> AmbientOcclusion {
    std::env::var("KJK_AO").map_or(setting, |value| AmbientOcclusion::named(value.trim()))
}

/// A share from 0 to 1 read from an environment variable, else `default`.
fn share(name: &str, default: f32) -> f32 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<f32>().ok())
        .map_or(default, |value| value.clamp(0.0, 1.0))
}

/// The shader defs an occluded surface's main pass takes (`material`):
/// `OCCLUDED`, the two shares in thousandths, and `AO_DEBUG` under
/// `KJK_AO_DEBUG=1` or `AO_DEBUG_NORMALS` under `KJK_AO_DEBUG=2`. Read from the environment once.
pub fn shader_defs() -> &'static [ShaderDefVal] {
    static DEFS: OnceLock<Vec<ShaderDefVal>> = OnceLock::new();
    DEFS.get_or_init(|| {
        let permille = |value: f32| (value * 1000.0).round() as u32;
        let mut defs = vec![
            ShaderDefVal::from("OCCLUDED"),
            ShaderDefVal::UInt(
                "AO_LIGHTMAP_PERMILLE".into(),
                permille(share("KJK_AO_LIGHTMAP", LIGHTMAP_SHARE)),
            ),
            ShaderDefVal::UInt(
                "AO_CONTACT_PERMILLE".into(),
                permille(share("KJK_AO_CONTACT", CONTACT_SHARE)),
            ),
        ];
        match debug() {
            1 => defs.push(ShaderDefVal::from("AO_DEBUG")),
            2 => defs.push(ShaderDefVal::from("AO_DEBUG_NORMALS")),
            _ => {}
        }
        defs
    })
}

/// What `KJK_AO_DEBUG` shows: 1 the occlusion alone, 2 the normals the
/// occlusion is worked out from. 0 when it is not set.
fn debug() -> u32 {
    std::env::var("KJK_AO_DEBUG")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0)
}

/// Whether the card can run the pass: Bevy's own pass needs 5 storage
/// textures in one shader, and Bevy leaves it out on a card without them.
#[derive(Resource, Clone, Copy, Default)]
struct Supported(bool);

/// What the render world tells the dumps: the frames the pass ran in since
/// the run began, and the size of the frame it last ran over.
#[derive(Resource, Clone, Default)]
struct Ran(Arc<RanCounts>);

#[derive(Default)]
struct RanCounts {
    frames: AtomicU64,
    /// Width in the high 32 bits, height in the low.
    size: AtomicU64,
}

pub struct AmbientOcclusionPlugin;

impl Plugin for AmbientOcclusionPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "ambient_occlusion.wgsl", Shader::from_wgsl);
        let ran = Ran::default();
        app.init_resource::<AmbientOcclusion>()
            .init_resource::<Supported>()
            .insert_resource(ran.clone())
            .add_systems(Last, (mark, facts).chain().after(crate::temporal::Applied));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(ran)
            .add_systems(RenderStartup, init)
            .add_systems(
                Render,
                (
                    prepare.in_set(RenderSystems::PrepareResources),
                    hold_back
                        .in_set(RenderSystems::Render)
                        .before(bevy::render::renderer::render_system),
                ),
            )
            .add_systems(
                Core3d,
                occlude
                    .after(Core3dSystems::Prepass)
                    .before(Core3dSystems::MainPass),
            );
    }

    fn finish(&self, app: &mut App) {
        // As Bevy's `ScreenSpaceAmbientOcclusionPlugin` decides.
        let supported = app.get_sub_app(RenderApp).is_some_and(|render_app| {
            render_app
                .world()
                .get_resource::<RenderDevice>()
                .is_some_and(|device| device.limits().max_storage_textures_per_shader_stage >= 5)
        });
        app.insert_resource(Supported(supported));
    }
}

/// Why the pass does not run, or `None` when it does.
fn why_not(
    setting: AmbientOcclusion,
    lighting: LightingMode,
    temporal: &Temporal,
    supported: bool,
) -> Option<&'static str> {
    if chosen(setting) == AmbientOcclusion::Off {
        Some("off")
    } else if !lighting.enhanced() {
        Some("classic")
    } else if !temporal.used.temporal() {
        Some("msaa")
    } else if !supported {
        Some("unsupported")
    } else {
        None
    }
}

/// The scene camera carries Bevy's `ScreenSpaceAmbientOcclusion`, and with
/// it the normal prepass, while the pass is wanted, and neither otherwise.
/// Only a camera drawing the world (one with the depth prepass `temporal`
/// gives it) and without multisampling takes it. The camera is changed
/// only when this changes.
#[allow(clippy::type_complexity)]
fn mark(
    setting: Res<AmbientOcclusion>,
    lighting: Option<Res<LightingMode>>,
    temporal: Res<Temporal>,
    supported: Res<Supported>,
    cameras: Query<
        (
            Entity,
            &Msaa,
            Has<DepthPrepass>,
            Has<ScreenSpaceAmbientOcclusion>,
        ),
        SceneCamera,
    >,
    mut commands: Commands,
) {
    let lighting = lighting.map_or_else(LightingMode::default, |lighting| *lighting);
    let wanted = why_not(*setting, lighting, &temporal, supported.0).is_none();
    for (camera, msaa, depth, has) in &cameras {
        let on = wanted && depth && *msaa == Msaa::Off;
        // A camera may be gone by the time this is applied.
        if on && !has {
            commands.entity(camera).try_insert((
                ScreenSpaceAmbientOcclusion {
                    quality_level: QUALITY,
                    constant_object_thickness: THICKNESS,
                },
                NormalPrepass,
            ));
        }
        if !on && has {
            commands
                .entity(camera)
                .try_remove::<(ScreenSpaceAmbientOcclusion, NormalPrepass)>();
        }
    }
}

/// For a dump, `ambient_occlusion`: the setting, whether the scene camera
/// has the pass and why not, the frames the pass ran in since the run
/// began, the size it last ran over, its samples a pixel, and the shares.
fn facts(
    setting: Res<AmbientOcclusion>,
    lighting: Option<Res<LightingMode>>,
    temporal: Res<Temporal>,
    supported: Res<Supported>,
    ran: Res<Ran>,
    cameras: Query<Has<ScreenSpaceAmbientOcclusion>, SceneCamera>,
    mut facts: ResMut<console::DumpFacts>,
) {
    if !facts.wanted() {
        return;
    }
    let lighting = lighting.map_or_else(LightingMode::default, |lighting| *lighting);
    let size = ran.0.size.load(Ordering::Relaxed);
    let (slices, steps) = samples(QUALITY);
    facts.0.insert(
        "ambient_occlusion".into(),
        serde_json::json!({
            "setting": chosen(*setting).name(),
            "on": cameras.iter().any(|has| has),
            "why_not": why_not(*setting, lighting, &temporal, supported.0),
            "frames_run": ran.0.frames.load(Ordering::Relaxed),
            "size": [size >> 32, size & 0xffff_ffff],
            "samples": slices * steps * 2,
            "lightmap_share": share("KJK_AO_LIGHTMAP", LIGHTMAP_SHARE),
            "contact_share": share("KJK_AO_CONTACT", CONTACT_SHARE),
            "debug": debug(),
        }),
    );
}

/// Bevy's directions a pixel and steps each way at a quality, as its pass
/// counts them.
fn samples(quality: ScreenSpaceAmbientOcclusionQualityLevel) -> (u32, u32) {
    match quality {
        ScreenSpaceAmbientOcclusionQualityLevel::Low => (1, 2),
        ScreenSpaceAmbientOcclusionQualityLevel::Medium => (2, 2),
        ScreenSpaceAmbientOcclusionQualityLevel::High => (3, 3),
        ScreenSpaceAmbientOcclusionQualityLevel::Ultra => (9, 3),
        ScreenSpaceAmbientOcclusionQualityLevel::Custom {
            slice_count,
            samples_per_slice_side,
        } => (slice_count, samples_per_slice_side),
    }
}

/// The three passes' layouts and pipelines. The layouts are Bevy's own, so
/// Bevy's bind groups fit them.
#[derive(Resource)]
struct Pipelines {
    common: BindGroupLayoutDescriptor,
    occlusion_layout: BindGroupLayoutDescriptor,
    depth: CachedComputePipelineId,
    denoise: CachedComputePipelineId,
    /// The occlusion pass by quality and whether the view jitters.
    occlusion: HashMap<(u32, u32, bool), CachedComputePipelineId>,
    /// The textures are 16-bit floats (else 32-bit), as Bevy chose.
    half_float: bool,
}

/// A view's occlusion pipeline.
#[derive(Component)]
struct OcclusionPipeline(CachedComputePipelineId);

fn init(mut commands: Commands, adapter: Res<RenderAdapter>, cache: Res<PipelineCache>) {
    // As Bevy's `SsaoPipelines` chooses its depth format.
    let half_float = adapter
        .get_texture_format_features(TextureFormat::R16Float)
        .allowed_usages
        .contains(TextureUsages::STORAGE_BINDING);
    let format = if half_float {
        TextureFormat::R16Float
    } else {
        TextureFormat::R32Float
    };
    // Bevy's layouts, entry for entry and with Bevy's names.
    let common = BindGroupLayoutDescriptor::new(
        "ssao_common_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                sampler(SamplerBindingType::NonFiltering),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<ViewUniform>(true),
            ),
        ),
    );
    let depth_layout = BindGroupLayoutDescriptor::new(
        "ssao_preprocess_depth_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_depth_2d(),
                texture_storage_2d(format, StorageTextureAccess::WriteOnly),
                texture_storage_2d(format, StorageTextureAccess::WriteOnly),
                texture_storage_2d(format, StorageTextureAccess::WriteOnly),
                texture_storage_2d(format, StorageTextureAccess::WriteOnly),
                texture_storage_2d(format, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let occlusion_layout = BindGroupLayoutDescriptor::new(
        "ssao_ssao_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Uint),
                texture_storage_2d(format, StorageTextureAccess::WriteOnly),
                texture_storage_2d(TextureFormat::R32Uint, StorageTextureAccess::WriteOnly),
                uniform_buffer::<GlobalsUniform>(false),
                uniform_buffer::<f32>(false),
            ),
        ),
    );
    let denoise_layout = BindGroupLayoutDescriptor::new(
        "ssao_spatial_denoise_bind_group_layout",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::COMPUTE,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Uint),
                texture_storage_2d(format, StorageTextureAccess::WriteOnly),
            ),
        ),
    );
    let defs = |pass: &str| {
        let mut defs = vec![ShaderDefVal::from(pass)];
        if half_float {
            defs.push("USE_R16FLOAT".into());
        }
        defs
    };
    let depth = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("ambient_occlusion_depth".into()),
        layout: vec![depth_layout, common.clone()],
        shader: SHADER,
        shader_defs: defs("DEPTH_PASS"),
        entry_point: Some("preprocess_depth".into()),
        ..default()
    });
    let denoise = cache.queue_compute_pipeline(ComputePipelineDescriptor {
        label: Some("ambient_occlusion_denoise".into()),
        layout: vec![denoise_layout, common.clone()],
        shader: SHADER,
        shader_defs: defs("DENOISE_PASS"),
        entry_point: Some("spatial_denoise".into()),
        ..default()
    });
    commands.insert_resource(Pipelines {
        common,
        occlusion_layout,
        depth,
        denoise,
        occlusion: HashMap::new(),
        half_float,
    });
}

/// Each view's occlusion pipeline, for its quality and its jitter. A view
/// that jitters turns the pass's noise each frame, so TAA or the upscaler
/// averages it over frames.
fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    pipelines: Option<ResMut<Pipelines>>,
    views: Query<(Entity, &ScreenSpaceAmbientOcclusion, Has<TemporalJitter>)>,
) {
    let Some(mut pipelines) = pipelines else {
        return;
    };
    let pipelines = &mut *pipelines;
    for (entity, settings, jitter) in &views {
        let (slices, steps) = samples(settings.quality_level);
        let id = *pipelines
            .occlusion
            .entry((slices, steps, jitter))
            .or_insert_with(|| {
                let mut defs = vec![
                    ShaderDefVal::from("OCCLUSION_PASS"),
                    ShaderDefVal::Int("SLICE_COUNT".into(), slices as i32),
                    ShaderDefVal::Int("SAMPLES_PER_SLICE_SIDE".into(), steps as i32),
                ];
                if jitter {
                    defs.push("TEMPORAL_JITTER".into());
                }
                if pipelines.half_float {
                    defs.push("USE_R16FLOAT".into());
                }
                cache.queue_compute_pipeline(ComputePipelineDescriptor {
                    label: Some("ambient_occlusion".into()),
                    layout: vec![pipelines.occlusion_layout.clone(), pipelines.common.clone()],
                    shader: SHADER,
                    shader_defs: defs,
                    entry_point: Some("ssao".into()),
                    ..default()
                })
            });
        commands.entity(entity).insert(OcclusionPipeline(id));
    }
}

/// Holds Bevy's own dispatch back. Bevy's pass runs only on a view with its
/// pipeline, which its `Prepare` systems give every view each frame. So
/// that is taken off again before the frame is drawn. Its textures and bind
/// groups stay, and `occlude` uses them.
fn hold_back(views: Query<Entity, With<SsaoPipelineId>>, mut commands: Commands) {
    for view in &views {
        commands.entity(view).remove::<SsaoPipelineId>();
    }
}

/// The three passes over the drawn frame, with Bevy's bind groups: the
/// depth's levels, the occlusion, and the blur that writes the texture the
/// main pass reads.
fn occlude(
    view: ViewQuery<(
        &ExtractedCamera,
        &SsaoBindGroups,
        &ViewUniformOffset,
        &OcclusionPipeline,
        Option<&MainPassResolutionOverride>,
    )>,
    pipelines: Option<Res<Pipelines>>,
    cache: Res<PipelineCache>,
    ran: Res<Ran>,
    mut ctx: RenderContext,
) {
    let (camera, bind_groups, offset, occlusion, drawn) = view.into_inner();
    let Some(pipelines) = pipelines else {
        return;
    };
    let (Some(window), Some(depth), Some(occlusion), Some(denoise)) = (
        camera.physical_viewport_size,
        cache.get_compute_pipeline(pipelines.depth),
        cache.get_compute_pipeline(occlusion.0),
        cache.get_compute_pipeline(pipelines.denoise),
    ) else {
        return;
    };
    // Under an upscaler the frame drawn is smaller than the window.
    let size = drawn.map_or(window, |drawn| drawn.0);
    // The passes' time on the GPU, where a run measures it
    // (`console::gpu_times`).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "ambient_occlusion");
    let encoder = ctx.command_encoder();
    encoder.push_debug_group("ambient_occlusion");
    for (label, pipeline, group, per_workgroup) in [
        (
            "ambient_occlusion_depth",
            depth,
            &bind_groups.preprocess_depth_bind_group,
            16,
        ),
        (
            "ambient_occlusion",
            occlusion,
            &bind_groups.ssao_bind_group,
            8,
        ),
        (
            "ambient_occlusion_denoise",
            denoise,
            &bind_groups.spatial_denoise_bind_group,
            8,
        ),
    ] {
        let mut pass = encoder.begin_compute_pass(&ComputePassDescriptor {
            label: Some(label),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.set_bind_group(1, &bind_groups.common_bind_group, &[offset.offset]);
        pass.dispatch_workgroups(
            size.x.div_ceil(per_workgroup),
            size.y.div_ceil(per_workgroup),
            1,
        );
    }
    encoder.pop_debug_group();
    time_span.end(ctx.command_encoder());
    ran.0.frames.fetch_add(1, Ordering::Relaxed);
    ran.0.size.store(
        (u64::from(size.x) << 32) | u64::from(size.y),
        Ordering::Relaxed,
    );
}
