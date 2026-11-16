//! The options' Lighting (`display.lighting`) and the roll-off pass that
//! goes with it.
//!
//! Under Classic the world's frame is an 8-bit texture. Its values are
//! gamma-encoded and go from 0 to 1, as in the games' framebuffers
//! (`scene::encoded_view`). A value above 1 is cut off at 1.
//!
//! Under Enhanced the frame is a float texture (`Hdr`, `temporal`). Its
//! values are still gamma-encoded, so every blend, fade and overlay looks
//! as it does under Classic. The values may now go above 1. A bolt or a
//! blade can then be brighter than a white wall.
//!
//! Three things use the values above 1. The surface shader adds a fight's
//! lights as linear light (`odyssey.wgsl`, told by `LightList::enhanced`).
//! Lightsaber blades, blaster bolts and muzzle flashes multiply their own
//! colour (`OdysseyMaterial::glowing`). Bevy's bloom then spreads whatever
//! is above 1 into a glow around it. Its threshold is 1, so a white wall
//! does not bloom.
//!
//! A screen cannot show a value above 1. The roll-off is a pass over the
//! scene camera's frame that brings the frame back into 0 to 1. It leaves
//! every channel below 0.9 exactly as it is. Above 0.9 it bends the channel
//! toward 1, so a brighter value still shows brighter. A stored white of
//! 1.0 comes out at 0.963. The pass runs after anti-aliasing and bloom, and
//! before the video effect, the fade, the HUD and the screens. Those all
//! still receive values from 0 to 1.

use std::collections::HashMap;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::Hdr;
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{sampler, texture_2d};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, FilterMode, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, Sampler,
    SamplerBindingType, SamplerDescriptor, ShaderStages, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::storage::ShaderBuffer;
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};
use bevy_post_process::bloom::{Bloom, BloomCompositeMode, BloomPlugin, BloomPrefilter};

use crate::lifecycle_view::SceneCamera;
use crate::material::AreaLights;

const SHADER: Handle<Shader> = uuid_handle!("b47d0e92-5a1c-4f38-8e6b-91c3d2a7f054");

/// The options' Lighting (`settings`, `display.lighting`).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LightingMode {
    /// The frame's values go from 0 to 1, as the games' did.
    #[default]
    Classic,
    /// The frame's values may go above 1, and the roll-off brings them
    /// back into what a screen shows.
    Enhanced,
}

impl LightingMode {
    /// The lighting named by a value from the settings file.
    pub fn named(value: &str) -> LightingMode {
        match value {
            "enhanced" => LightingMode::Enhanced,
            _ => LightingMode::Classic,
        }
    }

    /// Whether the frame's values may go above 1.
    pub fn enhanced(self) -> bool {
        self == LightingMode::Enhanced
    }
}

/// On a scene camera whose frame the roll-off brings back into 0 to 1.
#[derive(Component, ExtractComponent, Clone, Copy)]
pub struct RollOff;

/// The glow around what is brighter than white. Only the part of a pixel
/// above 1 is spread, and it is added to the frame.
fn bloom() -> Bloom {
    Bloom {
        intensity: BLOOM_STRENGTH,
        composite_mode: BloomCompositeMode::Additive,
        prefilter: BloomPrefilter {
            threshold: 1.0,
            threshold_softness: 0.0,
        },
        ..Bloom::OLD_SCHOOL
    }
}

/// How strong the glow around a bright pixel is.
const BLOOM_STRENGTH: f32 = 0.25;

pub struct RollOffPlugin;

impl Plugin for RollOffPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "rolloff.wgsl", Shader::from_wgsl);
        app.init_resource::<LightingMode>()
            .add_plugins((ExtractComponentPlugin::<RollOff>::default(), BloomPlugin))
            .add_systems(Last, (mark.after(crate::temporal::Applied), tell_shader));
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::Prepare))
            .add_systems(
                Core3d,
                pass.in_set(Core3dSystems::PostProcess)
                    .in_set(RollOffPass)
                    .after(bevy_post_process::bloom::bloom)
                    .before(crate::video_effect::VideoEffectPass),
            );
    }
}

/// The pass bringing the scene's frame back into 0 to 1. The video effect
/// and the screens' composite run after it, since both expect values from
/// 0 to 1.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct RollOffPass;

/// Every scene camera with a float frame carries bloom and the roll-off
/// under Enhanced, and none does under Classic. DLSS gives a camera a float
/// frame under Classic too, and that frame then stays below 1.
fn mark(
    lighting: Res<LightingMode>,
    cameras: Query<(Entity, Has<Hdr>, Has<RollOff>), SceneCamera>,
    mut commands: Commands,
) {
    for (camera, hdr, marked) in &cameras {
        let wanted = lighting.enhanced() && hdr;
        // A camera may be gone by the time this is applied.
        if wanted && !marked {
            commands.entity(camera).try_insert((RollOff, bloom()));
        }
        if !wanted && marked {
            commands.entity(camera).try_remove::<(RollOff, Bloom)>();
        }
    }
}

/// Tells the surface shader which lighting the options chose, through the
/// materials' shared light list.
fn tell_shader(
    lighting: Res<LightingMode>,
    area: Option<ResMut<AreaLights>>,
    mut buffers: ResMut<Assets<ShaderBuffer>>,
) {
    let Some(mut area) = area else { return };
    let enhanced = u32::from(lighting.enhanced());
    if area.0.enhanced != enhanced {
        area.0.enhanced = enhanced;
        area.upload(&mut buffers);
    }
}

/// The pass's bindings (the frame and a sampler) and its pipeline for each
/// target format.
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
        "roll_off",
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
        sampler,
        fullscreen: fullscreen.clone(),
        ids: HashMap::new(),
    });
}

fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<Pipeline>,
    views: Query<(Entity, &ExtractedView), With<RollOff>>,
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

fn pass(
    view: ViewQuery<(&ExtractedCamera, &ViewTarget, &PipelineId), With<RollOff>>,
    pipeline: Res<Pipeline>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (camera, target, id) = view.into_inner();
    // An 8-bit frame has no value above 1, so there is nothing to bring
    // back.
    if !camera.hdr {
        return;
    }
    let Some(render) = cache.get_render_pipeline(id.0) else {
        return;
    };
    let post = target.post_process_write();
    let bindings = device.create_bind_group(
        "roll_off",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((post.source, &pipeline.sampler)),
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
