//! The screens over the world as KOTOR blends them: in gamma space. The
//! screens draw into an image of their own whose values are the art's
//! sRGB-encoded ones (`ui_view`: textures uploaded unconverted, a target
//! that stores what it is given), so they blend among themselves as the
//! games and `ui::raster` do; then one pass over the window composites it
//! (premultiplied) over the world, which keeps encoded values too
//! (`scene::encoded_view`): `screens + (1 - alpha) · world`. Blended in
//! linear light instead, a translucent dark panel reads lighter than the
//! games'. The same pass lifts or lowers the whole frame by the
//! brightness's gamma (`display`): `value^(1 / gamma)`.

use std::collections::HashMap;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer_sized};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode, FragmentState,
    Operations, Origin3d, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    TexelCopyTextureInfo, TextureAspect, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

const SHADER: Handle<Shader> = uuid_handle!("8e0b6a41-27c3-4f5d-9a1e-6c4d2b7f3e90");

/// On the camera that composites: the screens' image, the snapshot image for
/// seamless menu transitions, and the frame's gamma (1 as drawn).
#[derive(Component, ExtractComponent, Clone, PartialEq)]
pub struct UiComposite {
    pub screens: Handle<Image>,
    pub previous: Handle<Image>,
    pub gamma: f32,
    /// When true, copies `screens` into `previous` on the GPU this frame.
    pub capture: bool,
    /// Transition progress in `0.0..1.0` (`< 0.0` when inactive).
    pub progress: f32,
    /// Wipe direction: `+1.0` left-to-right, `-1.0` right-to-left.
    pub direction: f32,
    /// Accent glow colour along the wipe edge.
    pub tint: [f32; 3],
    /// The screens are held back, and the window shows the world alone
    /// (the main menu waits for its 3D view: `gui_scene::hold_menu`).
    pub held: bool,
    /// The screens are left out for this frame, and the window shows the
    /// world alone with the brightness still applied (a photo mode
    /// screenshot: `photo_mode::bare_shot`).
    pub bare: bool,
}

impl UiComposite {
    pub fn new(screens: Handle<Image>, previous: Handle<Image>) -> UiComposite {
        UiComposite {
            screens,
            previous,
            gamma: 1.0,
            capture: false,
            progress: -1.0,
            direction: 1.0,
            tint: [0.0; 3],
            held: false,
            bare: false,
        }
    }
}

pub struct UiCompositePlugin;

impl Plugin for UiCompositePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "ui_composite.wgsl", crate::shader_checks::wgsl);
        app.add_plugins(ExtractComponentPlugin::<UiComposite>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                composite
                    .in_set(Core3dSystems::PostProcess)
                    .after(crate::video_effect::VideoEffectPass),
            );
    }
}

/// The pass's bindings (the world, the screens, a sampler) and its
/// pipeline for each target format.
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
        "ui_composite",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                texture_2d(TextureSampleType::Float { filterable: true }),
                sampler(SamplerBindingType::Filtering),
                uniform_buffer_sized(false, None),
            ),
        ),
    );
    // Both images are the window's size: each pixel read as it is.
    let sampler = device.create_sampler(&SamplerDescriptor {
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
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
    views: Query<(Entity, &ExtractedView), With<UiComposite>>,
) {
    let pipeline = &mut *pipeline;
    for (entity, view) in &views {
        let format = view.target_format;
        let id = *pipeline.ids.entry(format).or_insert_with(|| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("ui_composite".into()),
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

fn composite(
    view: ViewQuery<(&ViewTarget, &UiComposite, &PipelineId)>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    images: Res<RenderAssets<GpuImage>>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, composite, id) = view.into_inner();
    if composite.held {
        return;
    }
    let (Some(render), Some(screens), Some(previous)) = (
        cache.get_render_pipeline(id.0),
        images.get(&composite.screens),
        images.get(&composite.previous),
    ) else {
        return;
    };
    if composite.capture && screens.texture_descriptor.size == previous.texture_descriptor.size {
        ctx.command_encoder().copy_texture_to_texture(
            TexelCopyTextureInfo {
                texture: &screens.texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyTextureInfo {
                texture: &previous.texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            screens.texture_descriptor.size,
        );
    }
    let post = target.post_process_write();
    let data = [
        composite.gamma,
        composite.progress,
        composite.direction,
        if composite.bare { 1.0 } else { 0.0 },
        composite.tint[0],
        composite.tint[1],
        composite.tint[2],
        0.0,
    ];
    let settings = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("ui_composite"),
        contents: &data
            .iter()
            .flat_map(|value: &f32| value.to_le_bytes())
            .collect::<Vec<u8>>(),
        usage: BufferUsages::UNIFORM,
    });
    let bindings = device.create_bind_group(
        "ui_composite",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((
            post.source,
            &screens.texture_view,
            &previous.texture_view,
            &pipeline.sampler,
            settings.as_entire_binding(),
        )),
    );
    // The pass's time on the GPU, where a run measures it
    // (`console::gpu_times`).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "ui_composite");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("ui_composite"),
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
