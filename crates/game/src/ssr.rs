//! Screen-Space Reflections pass (`ssr.wgsl`) for single-sample linear HDR cameras.
//!
//! Runs immediately after `main_opaque_pass_3d` and before transparent / legacy
//! blend / transmissive passes (`heat_haze`, `legacy_blend`, `main_transparent_pass_3d`).
//! Smooth / metallic / env-mapped opaque surfaces encode `(r_bin << 4 | s_bin) + 2.0`
//! into `ViewTarget.a` inside `odyssey.wgsl`; this pass raymarches only those pixels
//! against `depth_prepass_texture`, reads HDR radiance and `normal_prepass_texture`,
//! blends Fresnel reflections into `ViewTarget`, and restores `alpha = 1.0`.
//!
//! Controlled via `KJK_SSR=off` (or `KJK_PHASE5=off`) and `KJK_SSR_DEBUG=1`.

use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::{Hdr, MainPassResolutionOverride, Viewport};
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_depth_2d, uniform_buffer,
};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode, FragmentState,
    Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    ShaderType, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::lifecycle_view::SceneCamera;
use crate::prepasses::{DeclarePrepassNeeds, DerivedPrepasses, DepthCoverage, Need, PrepassNeeds};
use crate::rolloff::LightingMode;

const SHADER: Handle<Shader> = uuid_handle!("3f8a1e62-7c4d-4b90-8a53-1d2e7c9f4b01");

/// System set grouping `ssr::pass` between `main_opaque_pass_3d` and transparents.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SsrPass;

/// Marks a single-sample linear HDR scene camera eligible for Screen-Space Reflections.
#[derive(Component, ExtractComponent, Clone, Copy)]
struct SsrEnabled;

#[derive(Clone, Copy, ShaderType)]
struct SsrParams {
    bounds: UVec4,
    frame_debug: UVec4,
}

fn debug_mode() -> u32 {
    static DEBUG: OnceLock<u32> = OnceLock::new();
    *DEBUG.get_or_init(|| {
        std::env::var("KJK_SSR_DEBUG")
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    })
}

pub struct SsrPlugin;

impl Plugin for SsrPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "ssr.wgsl", crate::shader_checks::wgsl);
        app.add_plugins(ExtractComponentPlugin::<SsrEnabled>::default())
            .add_systems(
                Last,
                mark.in_set(DeclarePrepassNeeds)
                    .after(crate::temporal::Applied),
            );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                pass.in_set(Core3dSystems::MainPass)
                    .in_set(SsrPass)
                    .after(main_opaque_pass_3d)
                    .before(main_transparent_pass_3d),
            );
    }
}

#[allow(clippy::type_complexity)]
fn mark(
    lighting: Option<Res<LightingMode>>,
    mut needs: ResMut<PrepassNeeds>,
    cameras: Query<(Entity, Has<Hdr>, &Msaa, Has<SsrEnabled>), (SceneCamera, With<DerivedPrepasses>)>,
    mut commands: Commands,
) {
    let enhanced = lighting.is_some_and(|mode| mode.enhanced());
    let enabled = enhanced && crate::material::ssr_enabled();
    for (camera, hdr, msaa, has_ssr) in &cameras {
        let on = enabled && hdr && *msaa == Msaa::Off;
        if on {
            needs.declare(
                "ssr",
                Need {
                    depth: DepthCoverage::Full,
                    normals: true,
                    motion: false,
                },
            );
        }
        if on && !has_ssr {
            commands.entity(camera).try_insert(SsrEnabled);
        }
        if !on && has_ssr {
            commands.entity(camera).try_remove::<SsrEnabled>();
        }
    }
}

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen: FullscreenShader,
    ids: HashMap<TextureFormat, CachedRenderPipelineId>,
}

#[derive(Component)]
struct PipelineId(CachedRenderPipelineId);

fn init(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "ssr",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_depth_2d(),
                texture_2d(TextureSampleType::Float { filterable: false }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<SsrParams>(false),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("ssr_linear"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..SamplerDescriptor::default()
    });
    commands.insert_resource(Pipeline {
        layout,
        sampler,
        fullscreen: fullscreen.clone(),
        ids: HashMap::new(),
    });
}

fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<Pipeline>,
    views: Query<(Entity, &ExtractedView), With<SsrEnabled>>,
) {
    let pipeline = &mut *pipeline;
    for (entity, view) in &views {
        let format = view.target_format;
        let id = *pipeline.ids.entry(format).or_insert_with(|| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("ssr".into()),
                layout: vec![pipeline.layout.clone()],
                vertex: pipeline.fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: SHADER,
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
}

type SsrView = (
    &'static ExtractedCamera,
    &'static ViewTarget,
    &'static ViewUniformOffset,
    &'static ViewPrepassTextures,
    Option<&'static MainPassResolutionOverride>,
    &'static PipelineId,
);

fn pass(
    view: ViewQuery<SsrView, With<SsrEnabled>>,
    pipeline: Option<Res<Pipeline>>,
    view_uniforms: Res<ViewUniforms>,
    frame: Res<FrameCount>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (camera, target, view_offset, prepass, resolution, id) = view.into_inner();
    let (Some(pipeline), Some(render), Some(view_binding), Some(depth), Some(normal)) = (
        pipeline,
        cache.get_render_pipeline(id.0),
        view_uniforms.uniforms.binding(),
        &prepass.depth,
        &prepass.normal,
    ) else {
        return;
    };
    let tex_size = target.main_texture().size();
    let bounds = match Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution) {
        Some(vp) => UVec4::new(
            vp.physical_position.x,
            vp.physical_position.y,
            vp.physical_position.x + vp.physical_size.x,
            vp.physical_position.y + vp.physical_size.y,
        ),
        None => UVec4::new(0, 0, tex_size.width, tex_size.height),
    };
    let params = SsrParams {
        bounds,
        frame_debug: UVec4::new(frame.0, debug_mode(), 0, 0),
    };
    let mut param_bytes = [0u8; 32];
    for (idx, word) in bounds
        .to_array()
        .into_iter()
        .chain(params.frame_debug.to_array())
        .enumerate()
    {
        param_bytes[idx * 4..(idx + 1) * 4].copy_from_slice(&word.to_le_bytes());
    }
    let params_buf = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("ssr_params"),
        contents: &param_bytes,
        usage: BufferUsages::UNIFORM,
    });
    let post = target.post_process_write();
    let bindings = device.create_bind_group(
        "ssr",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            post.source,
            &depth.texture.default_view,
            &normal.texture.default_view,
            &pipeline.sampler,
            view_binding,
            params_buf.as_entire_binding(),
        )),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "ssr");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("ssr"),
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
        pass.set_bind_group(0, &bindings, &[view_offset.offset]);
        pass.draw(0..3, 0..1);
    }
    time_span.end(ctx.command_encoder());
}

/// Pack `(roughness, refl)` into an exact `f16` opaque alpha tag (`>= 3.0`),
/// matching `odyssey.wgsl` and `ssr.wgsl`. Returns `1.0` when surface should not
/// receive SSR.
pub(crate) fn pack_alpha(roughness: f32, refl: f32) -> f32 {
    if roughness < 0.52 && refl > 0.04 {
        let r_bin = ((roughness / 0.52) * 15.0).round().clamp(0.0, 15.0) as u32;
        let s_bin = (refl * 15.0).round().clamp(1.0, 15.0) as u32;
        2.0 + ((r_bin * 16 + s_bin) as f32)
    } else {
        1.0
    }
}

/// Unpack `(roughness, refl)` from `ViewTarget.a` as `ssr.wgsl` does.
pub(crate) fn unpack_alpha(alpha: f32) -> Option<(f32, f32)> {
    if alpha < 2.5 {
        return None;
    }
    let packed = (alpha - 2.0).round().max(0.0) as u32;
    let r_bin = (packed >> 4).min(15);
    let s_bin = (packed & 15).min(15);
    Some(((r_bin as f32 / 15.0) * 0.52, s_bin as f32 / 15.0))
}
