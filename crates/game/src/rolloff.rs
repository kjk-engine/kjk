//! The options' Lighting (`display.lighting`), AgX filmic tonemapping,
//! linear HDR bloom, and camera linear-main routing.
//!
//! Under Classic (`LightingMode::Classic`) the world's frame is an 8-bit
//! texture (`Rgba8Unorm`) holding gamma-encoded values from `0` to `1`.
//!
//! Under Enhanced (`LightingMode::Enhanced` / `LightingMode::Baked`) on
//! single-sample cameras (`Msaa::Off`, including TAA, kjk Upscaler, and DLSS),
//! opaque geometry and `HDR_GLOW` emitters render in true linear HDR directly to
//! `ViewTarget` (`Rgba16Float`), while legacy non-depth-writing transparents
//! render through `legacy_blend` (`(accum_colour, transmittance)` -> linear
//! composite). When multi-sample MSAA (`2x`/`4x`) is active, the main pass
//! stays gamma-encoded (`LINEAR_MAIN_BIT` unset) and `DecodeBeforeBloom`
//! converts to linear light right before bloom and tonemapping.
//!
//! Bevy's bloom runs in linear HDR, followed by `RollOffPass` (AgX default,
//! selectable via `KJK_TONEMAP=agx|agx_punchy|aces|rolloff`) encoding into
//! sRGB `[0, 1]`, followed by `sharpen`, `video_effect`, and `ui_composite`.

use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::Hdr;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode,
    FragmentState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::storage::ShaderBuffer;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy_post_process::bloom::{Bloom, BloomCompositeMode, BloomPlugin, BloomPrefilter};

use crate::legacy_blend::LinearLegacyBlend;
use crate::lifecycle_view::SceneCamera;
use crate::material::{AreaLights, ENHANCED_BIT, LINEAR_MAIN_BIT};
use crate::scene::AreaFogColor;

const SHADER: Handle<Shader> = uuid_handle!("b47d0e92-5a1c-4f38-8e6b-91c3d2a7f054");

/// The options' Lighting (`settings`, `display.lighting`).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightingMode {
    /// The frame's values go from 0 to 1, as the games' did.
    #[default]
    Classic,
    /// True linear HDR main pass, linear bloom, and filmic tonemapping.
    Enhanced,
    /// Enhanced, with kjk's baked lightmaps from the lighting download
    /// (`lighting_bakes`). The bake itself is taken when kjk starts
    /// (`lighting_bakes::chosen`), so a change to or from it takes effect at
    /// the next start (`settings::Settings::lighting`).
    Baked,
}

impl LightingMode {
    /// The lighting named by a value from the settings file.
    pub fn named(value: &str) -> LightingMode {
        match value {
            "enhanced" => LightingMode::Enhanced,
            crate::lighting_bakes::SETTING_VALUE => LightingMode::Baked,
            _ => LightingMode::Classic,
        }
    }

    /// Whether Enhanced linear HDR / tonemapping is active.
    pub fn enhanced(self) -> bool {
        matches!(self, LightingMode::Enhanced | LightingMode::Baked)
    }
}

/// On a scene camera whose HDR frame is tonemapped into `[0, 1]` sRGB.
#[derive(Component, ExtractComponent, Clone, Copy)]
pub struct RollOff;

/// On an Enhanced scene camera using multi-sample MSAA (`2x`/`4x`), whose
/// resolved main pass stays gamma-encoded and converts to linear before bloom.
#[derive(Component, ExtractComponent, Clone, Copy)]
pub(crate) struct DecodeBeforeBloom;

/// Default bloom intensity in linear HDR space (`KJK_BLOOM` overrides).
const BLOOM_STRENGTH: f32 = 0.12;

fn bloom_strength() -> f32 {
    static STRENGTH: OnceLock<f32> = OnceLock::new();
    *STRENGTH.get_or_init(|| {
        std::env::var("KJK_BLOOM")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(BLOOM_STRENGTH)
    })
}

pub(crate) fn tonemap_mode_from(value: Option<&str>) -> u32 {
    match value.map(str::trim) {
        Some("agx_punchy" | "punchy") => 1,
        Some("aces") => 2,
        Some("rolloff") => 3,
        _ => 0,
    }
}

fn tonemap_mode() -> u32 {
    static MODE: OnceLock<u32> = OnceLock::new();
    *MODE.get_or_init(|| {
        let env = std::env::var("KJK_TONEMAP").ok();
        tonemap_mode_from(env.as_deref())
    })
}

/// The glow around what is brighter than white in linear HDR.
fn bloom() -> Bloom {
    Bloom {
        intensity: bloom_strength(),
        composite_mode: BloomCompositeMode::Additive,
        prefilter: BloomPrefilter {
            threshold: 1.0,
            threshold_softness: 0.2,
        },
        ..Bloom::OLD_SCHOOL
    }
}

pub struct RollOffPlugin;

impl Plugin for RollOffPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "rolloff.wgsl", crate::shader_checks::wgsl);
        app.init_resource::<LightingMode>()
            .add_plugins((
                ExtractComponentPlugin::<RollOff>::default(),
                ExtractComponentPlugin::<DecodeBeforeBloom>::default(),
                BloomPlugin,
            ))
            .add_systems(
                Last,
                (mark, tell_shader, sync_clear_color).after(crate::temporal::Applied),
            );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                (
                    decode_pass
                        .in_set(Core3dSystems::PostProcess)
                        .in_set(DecodeLinearPass)
                        .before(bevy_post_process::bloom::bloom),
                    pass.in_set(Core3dSystems::PostProcess)
                        .in_set(RollOffPass)
                        .after(bevy_post_process::bloom::bloom)
                        .before(crate::video_effect::VideoEffectPass),
                ),
            );
    }
}

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct DecodeLinearPass;

#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RollOffPass;

type MarkedCamera = (
    Entity,
    Has<Hdr>,
    &'static Msaa,
    Has<RollOff>,
    Has<LinearLegacyBlend>,
    Has<DecodeBeforeBloom>,
);

fn mark(
    lighting: Res<LightingMode>,
    cameras: Query<MarkedCamera, SceneCamera>,
    mut commands: Commands,
) {
    for (camera, hdr, msaa, rolled, legacy, decoded) in &cameras {
        let wanted = lighting.enhanced() && hdr;
        let wanted_legacy = wanted && *msaa == Msaa::Off;
        let wanted_decode = wanted && *msaa != Msaa::Off;

        // A camera may be gone by the time this is applied.
        if wanted && !rolled {
            commands.entity(camera).try_insert((RollOff, bloom()));
        }
        if !wanted && rolled {
            commands.entity(camera).try_remove::<(RollOff, Bloom)>();
        }
        if wanted_legacy && !legacy {
            commands.entity(camera).try_insert(LinearLegacyBlend);
        }
        if !wanted_legacy && legacy {
            commands.entity(camera).try_remove::<LinearLegacyBlend>();
        }
        if wanted_decode && !decoded {
            commands.entity(camera).try_insert(DecodeBeforeBloom);
        }
        if !wanted_decode && decoded {
            commands.entity(camera).try_remove::<DecodeBeforeBloom>();
        }
    }
}

/// Whether the main 3D pass draws in true linear HDR on `SceneCamera`.
///
/// True when Enhanced (or Enhanced Lightmaps) is active and the scene camera
/// uses single-sample (`Msaa::Off`, including TAA / kjk Upscaler / DLSS). When
/// multi-sample MSAA (2x/4x) is active, the main pass stays gamma-encoded to
/// avoid multi-sample transmissive conflicts and converts to linear before bloom.
pub(crate) fn linear_main_active(
    lighting: Option<&LightingMode>,
    cameras: &Query<&Msaa, SceneCamera>,
) -> bool {
    lighting.is_some_and(|l| l.enhanced())
        && cameras.iter().next().is_some_and(|msaa| *msaa == Msaa::Off)
}

fn tell_shader(
    lighting: Res<LightingMode>,
    cameras: Query<&Msaa, SceneCamera>,
    area: Option<ResMut<AreaLights>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let Some(mut area) = area else { return };
    let enhanced = if lighting.enhanced() {
        let linear = if linear_main_active(Some(&lighting), &cameras) {
            LINEAR_MAIN_BIT
        } else {
            0
        };
        ENHANCED_BIT | linear
    } else {
        0
    };
    if area.0.enhanced != enhanced {
        area.0.enhanced = enhanced;
        area.upload(&mut buffers);
    }
}

/// Keep `ClearColor` aligned with the active main-pass encoding (`ViewTarget`
/// converts `ClearColor` via `Srgba` because `scene::encoded_view` sets
/// `CompositingSpace::Srgb`). Under `linear_main_active`, passing `fog^2.2`
/// clears the `Rgba16Float` target to exact linear `decode_22(fog)`.
fn sync_clear_color(
    lighting: Res<LightingMode>,
    cameras: Query<&Msaa, SceneCamera>,
    fog: Option<Res<AreaFogColor>>,
    mut clear: ResMut<ClearColor>,
) {
    let Some(fog) = fog else { return };
    let [r, g, b] = fog.0;
    let expected = if linear_main_active(Some(&lighting), &cameras) {
        Color::srgb(r.powf(2.2), g.powf(2.2), b.powf(2.2))
    } else {
        Color::srgb(r, g, b)
    };
    if clear.0 != expected {
        clear.0 = expected;
    }
}

/// The pass's bindings and cached pipelines for each target format.
#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    decode_layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen: FullscreenShader,
    ids: HashMap<TextureFormat, CachedRenderPipelineId>,
    decode_ids: HashMap<TextureFormat, CachedRenderPipelineId>,
}

#[derive(Component)]
struct PipelineId(CachedRenderPipelineId);

#[derive(Component)]
struct DecodePipelineId(CachedRenderPipelineId);

fn init(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "roll_off",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<UVec4>(false),
            ),
        ),
    );
    let decode_layout = BindGroupLayoutDescriptor::new(
        "decode_linear",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
            ),
        ),
    );
    // The frame is the target's size: each pixel read as it is.
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        ..SamplerDescriptor::default()
    });
    commands.insert_resource(Pipeline {
        layout,
        decode_layout,
        sampler,
        fullscreen: fullscreen.clone(),
        ids: HashMap::new(),
        decode_ids: HashMap::new(),
    });
}

fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<Pipeline>,
    views: Query<(Entity, &ExtractedView), With<RollOff>>,
    decode_views: Query<(Entity, &ExtractedView), With<DecodeBeforeBloom>>,
) {
    let pipeline = &mut *pipeline;
    for (entity, view) in &views {
        let format = view.target_format;
        let id = *pipeline.ids.entry(format).or_insert_with(|| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("roll_off".into()),
                layout: vec![pipeline.layout.clone()],
                vertex: pipeline.fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: SHADER,
                    entry_point: Some("fragment".into()),
                    targets: vec![Some(ColorTargetState {
                        format,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        });
        commands.entity(entity).insert(PipelineId(id));
    }
    for (entity, view) in &decode_views {
        let format = view.target_format;
        let id = *pipeline.decode_ids.entry(format).or_insert_with(|| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("decode_linear".into()),
                layout: vec![pipeline.decode_layout.clone()],
                vertex: pipeline.fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: SHADER,
                    entry_point: Some("decode_linear".into()),
                    targets: vec![Some(ColorTargetState {
                        format,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })],
                    ..default()
                }),
                ..default()
            })
        });
        commands.entity(entity).insert(DecodePipelineId(id));
    }
}

fn decode_pass(
    view: ViewQuery<(&ExtractedCamera, &ViewTarget, &DecodePipelineId), With<DecodeBeforeBloom>>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (camera, target, id) = view.into_inner();
    if !camera.hdr {
        return;
    }
    let Some(render) = cache.get_render_pipeline(id.0) else {
        return;
    };
    let post = target.post_process_write();
    let bindings = device.create_bind_group(
        "decode_linear",
        &cache.get_bind_group_layout(&pipeline.decode_layout),
        &BindGroupEntries::sequential((post.source, &pipeline.sampler)),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "decode_linear");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("decode_linear"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: post.destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations::default(),
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(render);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }
    time_span.end(ctx.command_encoder());
}

fn pass(
    view: ViewQuery<(&ExtractedCamera, &ViewTarget, &PipelineId), With<RollOff>>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (camera, target, id) = view.into_inner();
    // An 8-bit frame has no value above 1, so there is nothing to tonemap.
    if !camera.hdr {
        return;
    }
    let Some(render) = cache.get_render_pipeline(id.0) else {
        return;
    };
    let post = target.post_process_write();
    let params = [tonemap_mode(), 0, 0, 0];
    let uniform = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("roll_off_mode"),
        contents: &params
            .iter()
            .flat_map(|value: &u32| value.to_le_bytes())
            .collect::<Vec<u8>>(),
        usage: BufferUsages::UNIFORM,
    });
    let bindings = device.create_bind_group(
        "roll_off",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((post.source, &pipeline.sampler, uniform.as_entire_binding())),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "roll_off");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("roll_off"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: post.destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations::default(),
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(render);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }
    time_span.end(ctx.command_encoder());
}

#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;

    const MIN_EV: f32 = -12.47393;
    const MAX_EV: f32 = 4.026069;

    const AGX_INSET_COLS: [[f32; 3]; 3] = [
        [0.842_479_05, 0.042_328_242, 0.042_375_654],
        [0.078_433_6, 0.878_468_63, 0.078_433_6],
        [0.079_223_745, 0.079_166_13, 0.879_143],
    ];

    const AGX_OUTSET_COLS: [[f32; 3]; 3] = [
        [1.196_879, -0.052_896_854, -0.052_971_635],
        [-0.098_020_88, 1.151_903_2, -0.098_043_45],
        [-0.099_029_74, -0.098_961_174, 1.151_073_7],
    ];

    fn mul_mat3_cols(cols: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
        [
            cols[0][0] * v[0] + cols[1][0] * v[1] + cols[2][0] * v[2],
            cols[0][1] * v[0] + cols[1][1] * v[1] + cols[2][1] * v[2],
            cols[0][2] * v[0] + cols[1][2] * v[1] + cols[2][2] * v[2],
        ]
    }

    fn encode_srgb(c: f32) -> f32 {
        let v = c.max(0.0);
        if v <= 0.0031308 {
            12.92 * v
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        }
    }

    fn agx_contrast(x: f32) -> f32 {
        let x2 = x * x;
        let x4 = x2 * x2;
        15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x
            + 0.4298 * x2
            + 0.1191 * x
            - 0.00232
    }

    fn agx_ref(rgb: [f32; 3], punchy: bool) -> [f32; 3] {
        let mut v = mul_mat3_cols(AGX_INSET_COLS, rgb);
        for c in &mut v {
            let ev = ((*c).max(1e-10).log2() - MIN_EV) / (MAX_EV - MIN_EV);
            *c = agx_contrast(ev.clamp(0.0, 1.0));
        }
        if punchy {
            let luma = 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
            for c in &mut v {
                let sat = c.max(0.0).powf(1.35);
                *c = luma + (sat - luma) * 1.4;
            }
        }
        let out = mul_mat3_cols(AGX_OUTSET_COLS, v);
        out.map(|c| encode_srgb(c.clamp(0.0, 1.0).powf(2.2)))
    }

    fn aces_ref(rgb: [f32; 3]) -> [f32; 3] {
        rgb.map(|x| {
            let x = x.max(0.0);
            let mapped = ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0);
            encode_srgb(mapped)
        })
    }

    let modes_ok = tonemap_mode_from(None) == 0
        && tonemap_mode_from(Some("agx")) == 0
        && tonemap_mode_from(Some(" agx_punchy ")) == 1
        && tonemap_mode_from(Some("punchy")) == 1
        && tonemap_mode_from(Some("aces")) == 2
        && tonemap_mode_from(Some("rolloff")) == 3
        && tonemap_mode_from(Some("unknown")) == 0;

    let roundtrip_ok = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        .into_iter()
        .all(|basis| {
            let back = mul_mat3_cols(AGX_OUTSET_COLS, mul_mat3_cols(AGX_INSET_COLS, basis));
            (0..3).all(|i| (back[i] - basis[i]).abs() < 1e-3)
        });

    let levels = [0.0_f32, 0.18, 1.0, 4.0, 16.0];
    let agx_ok = [false, true].into_iter().all(|punchy| {
        let mut prev = -1.0_f32;
        levels.into_iter().all(|l| {
            let mapped = agx_ref([l, l, l], punchy);
            let valid = mapped.into_iter().all(|c| (0.0..=1.0).contains(&c)) && mapped[0] > prev;
            prev = mapped[0];
            valid
        })
    });
    let mut prev = -1.0_f32;
    let aces_ok = levels.into_iter().all(|l| {
        let mapped = aces_ref([l, l, l]);
        let valid = mapped.into_iter().all(|c| (0.0..=1.0).contains(&c)) && mapped[0] > prev;
        prev = mapped[0];
        valid
    });

    vec![
        (
            "tonemap_mode_parses_env_names",
            check(modes_ok, format!("modes_ok={modes_ok}")),
        ),
        (
            "agx_inset_outset_roundtrips_unit_vectors",
            check(roundtrip_ok, format!("roundtrip_ok={roundtrip_ok}")),
        ),
        (
            "tonemappers_are_bounded_and_monotonic_on_luminance_ramp",
            check(
                agx_ok && aces_ok,
                format!("agx_ok={agx_ok} aces_ok={aces_ok}"),
            ),
        ),
    ]
}
