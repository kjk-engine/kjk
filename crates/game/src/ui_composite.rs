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
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::binding_types::{sampler, texture_2d, uniform_buffer_sized};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FilterMode, FragmentState,
    Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
    TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::texture::GpuImage;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

const SHADER: Handle<Shader> = uuid_handle!("8e0b6a41-27c3-4f5d-9a1e-6c4d2b7f3e90");

/// On the camera that composites: the screens' image, and the frame's
/// gamma (1 as drawn).
#[derive(Component, ExtractComponent, Clone)]
pub struct UiComposite {
    pub screens: Handle<Image>,
    pub gamma: f32,
}

impl UiComposite {
    pub fn new(screens: Handle<Image>) -> UiComposite {
        UiComposite {
            screens,
            gamma: 1.0,
        }
    }
}

pub struct UiCompositePlugin;

impl Plugin for UiCompositePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "ui_composite.wgsl", Shader::from_wgsl);
        app.add_plugins(ExtractComponentPlugin::<UiComposite>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
            .add_systems(Core3d, composite.in_set(Core3dSystems::PostProcess));
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
    let (target, screens, id) = view.into_inner();
    let gamma = screens.gamma;
    let (Some(render), Some(screens)) = (
        cache.get_render_pipeline(id.0),
        images.get(&screens.screens),
    ) else {
        return;
    };
    let post = target.post_process_write();
    let settings = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("ui_composite"),
        contents: &[gamma, 0.0, 0.0, 0.0]
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
            &pipeline.sampler,
            settings.as_entire_binding(),
        )),
    );
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
