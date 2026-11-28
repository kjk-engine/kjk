//! Volumetric Fog, Sun Light Shafts & Local Fitted/Saber Light Cones (`volumetrics.wgsl`).
//!
//! Runs once per frame in linear HDR after `Core3dSystems::MainPass` and before
//! temporal resolve / upscaling (`Core3dSystems::EarlyPostProcess`).
//!
//! Controlled via `KJK_VOLUMETRICS=off` (or `KJK_PHASE5=off`),
//! `KJK_VOLUMETRICS_STRENGTH=<f32>` (default `1.0`), and `KJK_VOLUMETRICS_DEBUG=1`.

use std::collections::HashMap;
use std::sync::OnceLock;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::{Hdr, MainPassResolutionOverride, Viewport};
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::diagnostic::FrameCount;
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::extract_resource::{ExtractResource, ExtractResourcePlugin};
use bevy::render::render_resource::binding_types::{texture_2d, texture_depth_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, Operations,
    PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
    ShaderStages, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::fitted_lights::GridLight;
use crate::lifecycle_view::SceneCamera;
use crate::prepasses::{DeclarePrepassNeeds, DerivedPrepasses, DepthCoverage, Need, PrepassNeeds};
use crate::rolloff::LightingMode;
use crate::scene::{BladeLights, SceneContent, SceneLights};

const SHADER: Handle<Shader> = uuid_handle!("8c3f1d90-4a2b-4e78-9d12-6f0e3a5b8c74");
const MAX_LIGHTS: usize = 16;
const MAX_DIST: f32 = 36.0;
const LOCAL_DENSITY: f32 = 0.042;
const SUN_DENSITY: f32 = 0.055;
const UNIFORM_BYTES: usize = 880;

/// System set grouping `volumetrics::pass` between `MainPass` and `EarlyPostProcess`.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct VolumetricsPass;

/// Marks a single-sample linear HDR scene camera eligible for volumetric scattering.
#[derive(Component, ExtractComponent, Clone, Copy)]
struct VolumetricsEnabled;

/// Fitted lights of the active scene kept on CPU for volumetric selection (`scene.rs`).
#[derive(Resource, Default)]
pub(crate) struct FittedVolumeLights(pub(crate) Vec<GridLight>);

#[derive(Clone, Copy, Default)]
struct VolLight {
    pos_radius: Vec4,
    color_strength: Vec4,
    aim_cone: Vec4,
}

/// Extracted frame parameters consumed by `volumetrics.wgsl`.
#[derive(Resource, ExtractResource, Clone)]
struct VolumetricState {
    sun_dir: Vec4,
    sun_color: Vec4,
    sun2_dir: Vec4,
    sun2_color: Vec4,
    light_count: u32,
    lights: [VolLight; MAX_LIGHTS],
}

impl Default for VolumetricState {
    fn default() -> Self {
        Self {
            sun_dir: Vec4::ZERO,
            sun_color: Vec4::ZERO,
            sun2_dir: Vec4::ZERO,
            sun2_color: Vec4::ZERO,
            light_count: 0,
            lights: [VolLight::default(); MAX_LIGHTS],
        }
    }
}

fn strength_scale() -> f32 {
    static STRENGTH: OnceLock<f32> = OnceLock::new();
    *STRENGTH.get_or_init(|| {
        std::env::var("KJK_VOLUMETRICS_STRENGTH")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or(1.0)
    })
}

fn debug_mode() -> u32 {
    static DEBUG: OnceLock<u32> = OnceLock::new();
    *DEBUG.get_or_init(|| {
        std::env::var("KJK_VOLUMETRICS_DEBUG")
            .ok()
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0)
    })
}

pub struct VolumetricsPlugin;

impl Plugin for VolumetricsPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "volumetrics.wgsl", crate::shader_checks::wgsl);
        app.init_resource::<FittedVolumeLights>()
            .init_resource::<VolumetricState>()
            .add_plugins((
                ExtractComponentPlugin::<VolumetricsEnabled>::default(),
                ExtractResourcePlugin::<VolumetricState>::default(),
            ))
            .add_systems(
                Last,
                (
                    mark.in_set(DeclarePrepassNeeds)
                        .after(crate::temporal::Applied),
                    gather_volumetric_lights.after(crate::temporal::Applied),
                ),
            );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                pass.in_set(VolumetricsPass)
                    .after(Core3dSystems::MainPass)
                    .before(Core3dSystems::EarlyPostProcess),
            );
    }
}

#[allow(clippy::type_complexity)]
fn mark(
    lighting: Option<Res<LightingMode>>,
    mut needs: ResMut<PrepassNeeds>,
    cameras: Query<
        (Entity, Has<Hdr>, &Msaa, Has<VolumetricsEnabled>),
        (SceneCamera, With<DerivedPrepasses>),
    >,
    mut commands: Commands,
) {
    let enhanced = lighting.is_some_and(|mode| mode.enhanced());
    let enabled = enhanced && crate::material::volumetrics_enabled() && strength_scale() > 0.0;
    for (camera, hdr, msaa, has_vol) in &cameras {
        let on = enabled && hdr && *msaa == Msaa::Off;
        if on {
            needs.declare(
                "volumetrics",
                Need {
                    depth: DepthCoverage::Full,
                    normals: false,
                    motion: false,
                },
            );
        }
        if on && !has_vol {
            commands.entity(camera).try_insert(VolumetricsEnabled);
        }
        if !on && has_vol {
            commands.entity(camera).try_remove::<VolumetricsEnabled>();
        }
    }
}

fn gather_volumetric_lights(
    lighting: Option<Res<LightingMode>>,
    scene: Option<Res<SceneContent>>,
    fitted: Res<FittedVolumeLights>,
    scene_lights: Option<Res<SceneLights>>,
    blades: Option<Res<BladeLights>>,
    cameras: Query<(&GlobalTransform, Has<VolumetricsEnabled>), SceneCamera>,
    mut state: ResMut<VolumetricState>,
) {
    let active_cam = cameras.iter().find(|(_, has)| *has);
    if !lighting.is_some_and(|mode| mode.enhanced())
        || !crate::material::volumetrics_enabled()
        || active_cam.is_none()
    {
        if state.light_count != 0 || state.sun_dir.w != 0.0 || state.sun2_dir.w != 0.0 {
            *state = VolumetricState::default();
        }
        return;
    }
    let cam_pos = active_cam.map_or(Vec3::ZERO, |(xf, _)| xf.translation());

    let (sun_dir, sun_color, sun2_dir, sun2_color) = if let Some(loaded) = scene.as_ref() {
        let s1 = loaded.0.probes.sun.map_or((Vec4::ZERO, Vec4::ZERO), |sun| {
            (sun.way.normalize_or_zero().extend(1.0), sun.colour.extend(0.0))
        });
        let s2 = loaded.0.probes.sun2.map_or((Vec4::ZERO, Vec4::ZERO), |(way, col)| {
            (way.normalize_or_zero().extend(1.0), col.extend(0.0))
        });
        (s1.0, s1.1, s2.0, s2.1)
    } else {
        (Vec4::ZERO, Vec4::ZERO, Vec4::ZERO, Vec4::ZERO)
    };

    let mut candidates: Vec<(f32, VolLight)> = Vec::new();
    let max_dist_sq = (MAX_DIST + 8.0) * (MAX_DIST + 8.0);
    let push_candidate = |list: &mut Vec<(f32, VolLight)>, pos: Vec3, radius: f32, rgb: Vec3, aim: Vec4| {
        let luma = rgb.x * 0.299 + rgb.y * 0.587 + rgb.z * 0.114;
        if luma <= 0.04 || radius <= 0.2 {
            return;
        }
        let dist_sq = pos.distance_squared(cam_pos);
        if dist_sq > max_dist_sq {
            return;
        }
        let score = (luma * radius.clamp(1.0, 14.0)) / (1.0 + dist_sq * 0.06);
        list.push((
            score,
            VolLight {
                pos_radius: pos.extend(radius),
                color_strength: rgb.extend(0.0),
                aim_cone: aim,
            },
        ));
    };

    for light in &fitted.0 {
        let pos = light.position_radius.truncate();
        let radius = light.position_radius.w;
        let lin_col = light.colour;
        push_candidate(&mut candidates, pos, radius, lin_col, light.aim);
    }

    if let Some(blades) = blades.as_ref() {
        for light in &blades.0 {
            let pos = light.position_radius.truncate();
            let radius = light.position_radius.w;
            let mult = light.color_multiplier.w.max(0.0);
            let lin_col = Vec3::new(
                light.color_multiplier.x.max(0.0).powf(2.2),
                light.color_multiplier.y.max(0.0).powf(2.2),
                light.color_multiplier.z.max(0.0).powf(2.2),
            ) * mult;
            push_candidate(&mut candidates, pos, radius, lin_col, Vec4::ZERO);
        }
    }

    if let Some(sl) = scene_lights.as_ref()
        && fitted.0.is_empty()
        && let Some(active) = &sl.active
    {
        for &idx in active {
            if let Some(light) = sl.all.get(idx)
                && light.kind.x == 0
            {
                let pos = light.position_radius.truncate();
                let radius = light.position_radius.w.min(14.0);
                let mult = light.color_multiplier.w.clamp(0.0, 2.5);
                let lin_col = Vec3::new(
                    light.color_multiplier.x.max(0.0).powf(2.2),
                    light.color_multiplier.y.max(0.0).powf(2.2),
                    light.color_multiplier.z.max(0.0).powf(2.2),
                ) * (mult * 0.55);
                push_candidate(&mut candidates, pos, radius, lin_col, Vec4::ZERO);
            }
        }
    }

    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut packed = [VolLight::default(); MAX_LIGHTS];
    let count = candidates.len().min(MAX_LIGHTS);
    for (dst, (_, src)) in packed.iter_mut().zip(candidates.into_iter().take(MAX_LIGHTS)) {
        *dst = src;
    }

    *state = VolumetricState {
        sun_dir,
        sun_color,
        sun2_dir,
        sun2_color,
        light_count: count as u32,
        lights: packed,
    };
}

#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    fullscreen: FullscreenShader,
    ids: HashMap<TextureFormat, CachedRenderPipelineId>,
}

#[derive(Component)]
struct PipelineId(CachedRenderPipelineId);

fn init(mut commands: Commands, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "volumetrics",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_depth_2d(),
                uniform_buffer::<ViewUniform>(true),
                uniform_buffer::<[Vec4; 55]>(false),
            ),
        ),
    );
    commands.insert_resource(Pipeline {
        layout,
        fullscreen: fullscreen.clone(),
        ids: HashMap::new(),
    });
}

fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<Pipeline>,
    views: Query<(Entity, &ExtractedView), With<VolumetricsEnabled>>,
) {
    let pipeline = &mut *pipeline;
    for (entity, view) in &views {
        let format = view.target_format;
        let id = *pipeline.ids.entry(format).or_insert_with(|| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("volumetrics".into()),
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

fn write_words(bytes: &mut [u8], at: &mut usize, words: [u32; 4]) {
    for word in words {
        bytes[*at..*at + 4].copy_from_slice(&word.to_le_bytes());
        *at += 4;
    }
}

fn pack_uniform(state: &VolumetricState, bounds: UVec4, frame_count: u32) -> [u8; UNIFORM_BYTES] {
    let mut bytes = [0u8; UNIFORM_BYTES];
    let mut at = 0usize;
    write_words(&mut bytes, &mut at, bounds.to_array());
    write_words(&mut bytes, &mut at, state.sun_dir.to_array().map(f32::to_bits));
    write_words(&mut bytes, &mut at, state.sun_color.to_array().map(f32::to_bits));
    write_words(&mut bytes, &mut at, state.sun2_dir.to_array().map(f32::to_bits));
    write_words(&mut bytes, &mut at, state.sun2_color.to_array().map(f32::to_bits));
    write_words(
        &mut bytes,
        &mut at,
        [strength_scale(), LOCAL_DENSITY, SUN_DENSITY, MAX_DIST].map(f32::to_bits),
    );
    write_words(
        &mut bytes,
        &mut at,
        [frame_count, state.light_count, debug_mode(), 0],
    );
    for light in &state.lights {
        write_words(&mut bytes, &mut at, light.pos_radius.to_array().map(f32::to_bits));
        write_words(&mut bytes, &mut at, light.color_strength.to_array().map(f32::to_bits));
        write_words(&mut bytes, &mut at, light.aim_cone.to_array().map(f32::to_bits));
    }
    bytes
}

type VolumetricsView = (
    &'static ExtractedCamera,
    &'static ViewTarget,
    &'static ViewUniformOffset,
    &'static ViewPrepassTextures,
    Option<&'static MainPassResolutionOverride>,
    &'static PipelineId,
);

#[allow(clippy::too_many_arguments)]
fn pass(
    view: ViewQuery<VolumetricsView, With<VolumetricsEnabled>>,
    pipeline: Option<Res<Pipeline>>,
    state: Res<VolumetricState>,
    view_uniforms: Res<ViewUniforms>,
    frame: Res<FrameCount>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let has_suns = state.sun_dir.w > 0.5 || state.sun2_dir.w > 0.5;
    if !has_suns && state.light_count == 0 {
        return;
    }
    let (camera, target, view_offset, prepass, resolution, id) = view.into_inner();
    let (Some(pipeline), Some(render), Some(view_binding), Some(depth)) = (
        pipeline,
        cache.get_render_pipeline(id.0),
        view_uniforms.uniforms.binding(),
        &prepass.depth,
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
    let uniform_bytes = pack_uniform(&state, bounds, frame.0);
    let uniform_buf = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("volumetrics_uniform"),
        contents: &uniform_bytes,
        usage: BufferUsages::UNIFORM,
    });
    let post = target.post_process_write();
    let bindings = device.create_bind_group(
        "volumetrics",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            post.source,
            &depth.texture.default_view,
            view_binding,
            uniform_buf.as_entire_binding(),
        )),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "volumetrics");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("volumetrics"),
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

/// Evaluates the Henyey-Greenstein scattering phase function for cosine angle `cos_theta`
/// and asymmetry factor `g` in `(-1, 1)`.
pub(crate) fn hg_phase(cos_theta: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denom = (1.0 + g2 - 2.0 * g * cos_theta).max(1e-4);
    (1.0 - g2) / (4.0 * std::f32::consts::PI * denom.powf(1.5))
}
