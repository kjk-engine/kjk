//! kjk's own temporal upscaler (`Upscaler::Taa`, the options' TAA with a
//! size). It works on every card. It draws the scene smaller and shows it
//! at the window's size.
//!
//! The scene camera draws its frame at a share of the window's side through
//! `MainPassResolutionOverride`, as it does under DLSS. Bevy's passes then
//! draw into the top left corner of the window-sized main texture. The
//! camera's projection is jittered by a different part of a pixel each
//! frame, along a Halton sequence. The sequence is longer for a smaller
//! frame, so that every pixel of the window is passed over.
//!
//! The resolve (`upscale.wgsl`) runs where Bevy's TAA would run. It keeps a
//! history at the window's size. Each frame it moves the history along the
//! motion vectors, clamps it to the colours of the smaller frame around the
//! pixel, and mixes in this frame's samples. A sample that fell nearer a
//! pixel's centre counts for more. The clamp is what removes trails behind
//! moving things. A cut (`temporal::TemporalCut`) drops the history.
//!
//! A sharpening pass of the upscaler's own (`upscale_sharpen.wgsl`) runs on
//! the resolved frame, at `SHARPENING`. It is contrast adaptive, as Bevy's
//! `ContrastAdaptiveSharpening` is. Bevy's pass is not used, for three
//! reasons. Bevy's pass runs among the post-processing passes with no order
//! against kjk's grade and the screens' composite (`video_effect`). It
//! expects values from 0 to 1, where Enhanced lighting's frame goes above 1.
//! It may also run after bloom and sharpen the glow. The upscaler's pass
//! runs right after the resolve, before bloom, and reads its range on values
//! brought into 0 to 1. Bloom, the roll-off, the HUD and the screens all draw
//! after the resolve and the sharpening, at the window's size.

use std::collections::HashMap;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::MainPassResolutionOverride;
use bevy::core_pipeline::prepass::ViewPrepassTextures;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::diagnostic::FrameCount;
use bevy::image::ToExtents;
use bevy::prelude::*;
use bevy::render::camera::{ExtractedCamera, TemporalJitter};
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::render_resource::binding_types::{
    sampler, texture_2d, texture_depth_2d, uniform_buffer_sized,
};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode, FragmentState,
    Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::sync_world::RenderEntity;
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{ExtractSchedule, MainWorld, Render, RenderApp, RenderStartup, RenderSystems};

const SHADER: Handle<Shader> = uuid_handle!("d2a46f1e-8b3c-4e57-9c0a-6f1b2e7d4a93");
const SHARPEN_SHADER: Handle<Shader> = uuid_handle!("7b19c3e5-2f6d-4a80-b4e7-0d5a9c1f8e26");

/// How strongly the sharpening pass sharpens the resolved frame (0 to 1).
/// Chosen by comparing captures at a half of the window's side.
pub const SHARPENING: f32 = 0.35;

/// The sharpening this run uses: `KJK_SHARPEN` (0 to 1, 0 turns the pass
/// off) for tuning, else `SHARPENING`.
pub fn sharpening() -> f32 {
    std::env::var("KJK_SHARPEN")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .map_or(SHARPENING, |value| value.clamp(0.0, 1.0))
}

/// The history keeps a float colour and, in its alpha, how many frames the
/// pixel has stood still. An 8-bit alpha could not hold that count.
const HISTORY_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// On a scene camera the upscaler draws for: the share of the window's side
/// it draws, and whether it drops its history this frame.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct Upscale {
    pub scale: f32,
    pub reset: bool,
}

pub struct UpscalePlugin;

impl Plugin for UpscalePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "upscale.wgsl", Shader::from_wgsl);
        load_internal_asset!(
            app,
            SHARPEN_SHADER,
            "upscale_sharpen.wgsl",
            Shader::from_wgsl
        );
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init)
            .add_systems(ExtractSchedule, extract)
            .add_systems(
                Render,
                (
                    drawn_size.in_set(RenderSystems::PrepareViews),
                    prepare.in_set(RenderSystems::PrepareResources),
                ),
            )
            .add_systems(Core3d, resolve.in_set(Core3dSystems::EarlyPostProcess));
    }
}

/// Copies the main world's `Upscale` to the camera's render side, and
/// clears what the upscaler left there once the camera no longer has it.
fn extract(mut commands: Commands, mut main_world: ResMut<MainWorld>, had: Query<Has<Upscale>>) {
    let mut cameras = main_world.query::<(RenderEntity, &Camera, Option<&mut Upscale>)>();
    for (entity, camera, upscale) in cameras.iter_mut(&mut main_world) {
        let Ok(mut render) = commands.get_entity(entity) else {
            continue;
        };
        match upscale {
            Some(mut upscale) if camera.is_active => {
                render.insert(*upscale);
                upscale.reset = false;
            }
            _ => {
                if had.get(entity) == Ok(true) {
                    render.remove::<(
                        Upscale,
                        MainPassResolutionOverride,
                        History,
                        PipelineId,
                        Sizes,
                    )>();
                }
            }
        }
    }
}

/// The sizes the history was made for: the window's and the drawn frame's.
/// The history is dropped when either changes.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
struct Sizes(UVec2, UVec2);

/// The frame a camera draws, its jitter for this frame, and a reset when the
/// sizes change. Runs before Bevy makes the view's uniforms, which read the
/// size and the jitter.
#[allow(clippy::type_complexity)]
fn drawn_size(
    frame: Res<FrameCount>,
    mut views: Query<(
        Entity,
        &ExtractedView,
        &mut Upscale,
        Option<&mut TemporalJitter>,
        Option<&mut MainPassResolutionOverride>,
        Option<&Sizes>,
    )>,
    mut commands: Commands,
) {
    for (entity, view, mut upscale, jitter, size, sizes) in &mut views {
        let window = view.viewport.zw();
        let drawn = drawn(window, upscale.scale);
        match size {
            Some(mut size) if size.0 != drawn => size.0 = drawn,
            Some(_) => {}
            None => {
                commands
                    .entity(entity)
                    .insert(MainPassResolutionOverride(drawn));
            }
        }
        let now = Sizes(window, drawn);
        if sizes != Some(&now) {
            upscale.reset = true;
            commands.entity(entity).insert(now);
        }
        if let Some(mut jitter) = jitter {
            jitter.offset = jitter_at(frame.0, upscale.scale);
        }
    }
}

/// The frame drawn at `scale` of the window's side.
pub fn drawn(window: UVec2, scale: f32) -> UVec2 {
    (window.as_vec2() * scale)
        .round()
        .as_uvec2()
        .max(UVec2::ONE)
}

/// How many jitter positions a frame drawn at `scale` cycles through. Bevy's
/// TAA uses 8 at the window's size. A smaller frame needs as many for each
/// of the window's pixels that one of its pixels covers.
fn phases(scale: f32) -> u32 {
    ((8.0 / (scale * scale)).ceil() as u32).clamp(8, 128)
}

/// The jitter of frame `frame`, from -0.5 to 0.5 of a drawn pixel on each
/// axis: the Halton sequence of bases 2 and 3, from its first point.
fn jitter_at(frame: u32, scale: f32) -> Vec2 {
    let index = frame % phases(scale) + 1;
    Vec2::new(halton(index, 2), halton(index, 3)) - 0.5
}

fn halton(mut index: u32, base: u32) -> f32 {
    let mut fraction = 1.0;
    let mut value = 0.0;
    while index > 0 {
        fraction /= base as f32;
        value += fraction * (index % base) as f32;
        index /= base;
    }
    value
}

/// What a sample's weight is multiplied by at `scale`, so that the weights
/// average to 1 over the jitter's positions. A sample's weight is
/// `exp(-2 d²)`, with `d` its distance from a shown pixel's centre in shown
/// pixels (`upscale.wgsl`). Over a drawn pixel, which spans `1 / scale`
/// shown pixels on each side, that averages to the square of the mean of
/// `exp(-2 x²)` along one side.
fn gain(scale: f32) -> f32 {
    let side = 1.0 / scale;
    let steps = 256;
    let mean = (0..steps)
        .map(|step| {
            let x = ((step as f32 + 0.5) / steps as f32 - 0.5) * side;
            (-2.0 * x * x).exp()
        })
        .sum::<f32>()
        / steps as f32;
    1.0 / (mean * mean)
}

/// The two history textures, read and written in turn.
#[derive(Component)]
struct History {
    read: CachedTexture,
    write: CachedTexture,
}

/// The two passes' bindings and their pipelines.
#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    fullscreen: FullscreenShader,
    ids: HashMap<(TextureFormat, bool, bool), CachedRenderPipelineId>,
    sharpen_layout: BindGroupLayoutDescriptor,
    sharpen_ids: HashMap<TextureFormat, CachedRenderPipelineId>,
    /// The sharpening's strength for the run (`sharpening`).
    sharpening: f32,
}

/// A view's resolve pipeline and sharpening pipeline.
#[derive(Component)]
struct PipelineId(CachedRenderPipelineId, CachedRenderPipelineId);

fn init(mut commands: Commands, device: Res<RenderDevice>, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "taa_upscale",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                // The drawn frame.
                texture_2d(TextureSampleType::Float { filterable: true }),
                // The history.
                texture_2d(TextureSampleType::Float { filterable: true }),
                // The motion vectors.
                texture_2d(TextureSampleType::Float { filterable: true }),
                // The depth.
                texture_depth_2d(),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer_sized(false, None),
            ),
        ),
    );
    let sampler = device.create_sampler(&SamplerDescriptor {
        label: Some("taa_upscale"),
        mag_filter: FilterMode::Linear,
        min_filter: FilterMode::Linear,
        ..SamplerDescriptor::default()
    });
    let sharpen_layout = BindGroupLayoutDescriptor::new(
        "taa_upscale_sharpen",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                uniform_buffer_sized(false, None),
            ),
        ),
    );
    commands.insert_resource(Pipeline {
        layout,
        sampler,
        fullscreen: fullscreen.clone(),
        ids: HashMap::new(),
        sharpen_layout,
        sharpen_ids: HashMap::new(),
        sharpening: sharpening(),
    });
}

/// The history textures and the pipeline for each upscaled view.
fn prepare(
    mut commands: Commands,
    mut textures: ResMut<TextureCache>,
    device: Res<RenderDevice>,
    frame: Res<FrameCount>,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<Pipeline>,
    views: Query<(Entity, &ExtractedView, &ExtractedCamera, &Upscale)>,
) {
    let pipeline = &mut *pipeline;
    for (entity, view, camera, upscale) in &views {
        let Some(size) = camera.physical_target_size else {
            continue;
        };
        let mut descriptor = TextureDescriptor {
            label: Some("taa_upscale_history_1"),
            size: size.to_extents(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: HISTORY_FORMAT,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        };
        let first = textures.get(&device, descriptor.clone());
        descriptor.label = Some("taa_upscale_history_2");
        let second = textures.get(&device, descriptor);
        let history = if frame.0.is_multiple_of(2) {
            History {
                read: second,
                write: first,
            }
        } else {
            History {
                read: first,
                write: second,
            }
        };
        let format = view.target_format;
        let key = (format, camera.hdr, upscale.reset);
        let mut id_for = |(format, tonemap, reset): (TextureFormat, bool, bool)| {
            *pipeline
                .ids
                .entry((format, tonemap, reset))
                .or_insert_with(|| {
                    let mut defs = Vec::new();
                    if tonemap {
                        defs.push("TONEMAP".into());
                    }
                    if reset {
                        defs.push("RESET".into());
                    }
                    cache.queue_render_pipeline(RenderPipelineDescriptor {
                        label: Some("taa_upscale".into()),
                        layout: vec![pipeline.layout.clone()],
                        vertex: pipeline.fullscreen.to_vertex_state(),
                        fragment: Some(FragmentState {
                            shader: SHADER,
                            shader_defs: defs,
                            targets: vec![
                                Some(ColorTargetState {
                                    format,
                                    blend: None,
                                    write_mask: ColorWrites::ALL,
                                }),
                                Some(ColorTargetState {
                                    format: HISTORY_FORMAT,
                                    blend: None,
                                    write_mask: ColorWrites::ALL,
                                }),
                            ],
                            ..default()
                        }),
                        ..default()
                    })
                })
        };
        let id = id_for(key);
        // The pipeline without the reset is needed the frame after a reset.
        id_for((format, camera.hdr, false));
        let sharpen = *pipeline.sharpen_ids.entry(format).or_insert_with(|| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("taa_upscale_sharpen".into()),
                layout: vec![pipeline.sharpen_layout.clone()],
                vertex: pipeline.fullscreen.to_vertex_state(),
                fragment: Some(FragmentState {
                    shader: SHARPEN_SHADER,
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
        commands
            .entity(entity)
            .insert((history, PipelineId(id, sharpen)));
    }
}

/// The resolve: the drawn frame into the window-sized history and frame.
#[allow(clippy::type_complexity)]
fn resolve(
    view: ViewQuery<(
        &ExtractedCamera,
        &ViewTarget,
        &History,
        &ViewPrepassTextures,
        &PipelineId,
        &MainPassResolutionOverride,
        &TemporalJitter,
        &Upscale,
    )>,
    pipeline: Option<Res<Pipeline>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (camera, target, history, prepass, id, drawn, jitter, upscale) = view.into_inner();
    let Some(pipeline) = pipeline else {
        return;
    };
    let (Some(render), Some(motion), Some(depth), Some(shown)) = (
        cache.get_render_pipeline(id.0),
        &prepass.motion_vectors,
        &prepass.depth,
        camera.physical_target_size,
    ) else {
        return;
    };
    let post = target.post_process_write();
    let params = [
        drawn.0.x as f32,
        drawn.0.y as f32,
        shown.x as f32,
        shown.y as f32,
        jitter.offset.x,
        jitter.offset.y,
        gain(upscale.scale),
        0.0,
    ];
    let params = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("taa_upscale"),
        contents: &params
            .iter()
            .flat_map(|value: &f32| value.to_le_bytes())
            .collect::<Vec<u8>>(),
        usage: BufferUsages::UNIFORM,
    });
    let bindings = device.create_bind_group(
        "taa_upscale",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            post.source,
            &history.read.default_view,
            &motion.texture.default_view,
            &depth.texture.default_view,
            &pipeline.sampler,
            params.as_entire_binding(),
        )),
    );
    // The pass's time on the GPU, where a run measures it
    // (`console::gpu_times`).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "taa_upscale");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("taa_upscale"),
                color_attachments: &[
                    Some(RenderPassColorAttachment {
                        view: post.destination,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations::default(),
                    }),
                    Some(RenderPassColorAttachment {
                        view: &history.write.default_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: Operations::default(),
                    }),
                ],
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
    if pipeline.sharpening > 0.0
        && let Some(sharpen) = cache.get_render_pipeline(id.1)
    {
        sharpen_frame(&mut ctx, target, &pipeline, &cache, &device, sharpen);
    }
}

/// The sharpening pass over the resolved frame.
fn sharpen_frame(
    ctx: &mut RenderContext,
    target: &ViewTarget,
    pipeline: &Pipeline,
    cache: &PipelineCache,
    device: &RenderDevice,
    render: &bevy::render::render_resource::RenderPipeline,
) {
    let post = target.post_process_write();
    let strength = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("taa_upscale_sharpen"),
        contents: &[pipeline.sharpening, 0.0, 0.0, 0.0]
            .iter()
            .flat_map(|value: &f32| value.to_le_bytes())
            .collect::<Vec<u8>>(),
        usage: BufferUsages::UNIFORM,
    });
    let bindings = device.create_bind_group(
        "taa_upscale_sharpen",
        &cache.get_bind_group_layout(&pipeline.sharpen_layout),
        &BindGroupEntries::sequential((post.source, strength.as_entire_binding())),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "taa_upscale_sharpen");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("taa_upscale_sharpen"),
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
