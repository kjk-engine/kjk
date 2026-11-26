//! The sharpening that offsets the softness of a temporal resolve (the
//! options' Sharpening, `display.sharpening`). It runs on the scene camera
//! under every temporal choice of anti-aliasing: Bevy's TAA, kjk's own
//! upscaler (`upscale`), DLAA and DLSS. It does not run under multisampling,
//! which has no temporal blur to offset. `temporal::apply` puts `Sharpen` on
//! the scene camera and takes it off again.
//!
//! The pass (`sharpen.wgsl`) is contrast adaptive. A pixel is sharpened
//! against its four neighbours by less where they already differ a lot, so a
//! hard edge does not get a halo. Bevy's `ContrastAdaptiveSharpening` is not
//! used, for three reasons. Bevy's pass runs among the post-processing
//! passes with no order against kjk's grade and the screens' composite
//! (`video_effect`). It expects values from 0 to 1, where Enhanced
//! lighting's frame goes above 1. It may also run after bloom and sharpen
//! the glow.
//!
//! This pass runs after the temporal resolve and after the encoding pass of
//! `linear_frame`, since it expects encoded values. It runs before bloom,
//! the roll-off, the HUD and the screens, which all draw after it at the
//! window's size. So neither the interface nor the glow is sharpened. It
//! reads its range on values brought into 0 to 1.
//!
//! `KJK_SHARPEN` (0 to 1) overrides the setting for a run, for tuning.

use std::collections::HashMap;
use std::sync::LazyLock;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer_sized};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BufferInitDescriptor,
    BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, FragmentState, Operations,
    PipelineCache, RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
    ShaderStages, TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::display::Display;
use crate::temporal::Antialias;

const SHADER: Handle<Shader> = uuid_handle!("7b19c3e5-2f6d-4a80-b4e7-0d5a9c1f8e26");

/// The strength of the options' Medium, and the default (0 to 1). It was
/// chosen by comparing captures of kjk's upscaler at a half of the window's
/// side.
pub const MEDIUM: f32 = 0.35;

/// `KJK_SHARPEN` for the run, read once: 0 to 1, 0 turns the pass off.
static OVERRIDE: LazyLock<Option<f32>> = LazyLock::new(|| {
    std::env::var("KJK_SHARPEN")
        .ok()
        .and_then(|value| value.parse::<f32>().ok())
        .map(|value| value.clamp(0.0, 1.0))
});

/// The strength the scene camera sharpens with: `KJK_SHARPEN`'s, else the
/// setting's.
pub fn strength(display: &Display) -> f32 {
    OVERRIDE.unwrap_or(display.sharpening)
}

/// On the scene camera under a temporal choice: how strongly its resolved
/// frame is sharpened, from 0 (not at all) to 1.
#[derive(Component, ExtractComponent, Clone, Copy, Debug, PartialEq)]
pub struct Sharpen {
    pub strength: f32,
}

/// The sharpening the scene camera takes under `used`: none under
/// multisampling, else `display`'s strength.
pub fn wanted(used: Antialias, display: &Display) -> Option<Sharpen> {
    used.temporal().then(|| Sharpen {
        strength: strength(display),
    })
}

pub struct SharpenPlugin;

impl Plugin for SharpenPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "sharpen.wgsl", crate::shader_checks::wgsl);
        app.add_plugins(ExtractComponentPlugin::<Sharpen>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .add_systems(RenderStartup, init)
            .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources))
            .add_systems(
                Core3d,
                sharpen
                    .after(Core3dSystems::EarlyPostProcess)
                    .after(crate::linear_frame::Encoded)
                    .before(Core3dSystems::PostProcess),
            );
    }
}

/// The pass's bindings and its pipeline for each frame format.
#[derive(Resource)]
struct Pipeline {
    layout: BindGroupLayoutDescriptor,
    fullscreen: FullscreenShader,
    ids: HashMap<TextureFormat, CachedRenderPipelineId>,
}

/// A view's sharpening pipeline.
#[derive(Component)]
struct PipelineId(CachedRenderPipelineId);

fn init(mut commands: Commands, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "sharpen",
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
        fullscreen: fullscreen.clone(),
        ids: HashMap::new(),
    });
}

/// The pipeline for each sharpened view's frame format.
fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<Pipeline>,
    views: Query<(Entity, &ExtractedView), With<Sharpen>>,
) {
    let pipeline = &mut *pipeline;
    for (entity, view) in &views {
        let format = view.target_format;
        let id = *pipeline.ids.entry(format).or_insert_with(|| {
            cache.queue_render_pipeline(RenderPipelineDescriptor {
                label: Some("sharpen".into()),
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

/// The sharpening pass over the resolved frame. It does nothing at a
/// strength of 0.
fn sharpen(
    view: ViewQuery<(&ViewTarget, &Sharpen, &PipelineId)>,
    pipeline: Option<Res<Pipeline>>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, sharpen, id) = view.into_inner();
    let (Some(pipeline), Some(render)) = (pipeline, cache.get_render_pipeline(id.0)) else {
        return;
    };
    if sharpen.strength <= 0.0 {
        return;
    }
    let post = target.post_process_write();
    let strength = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("sharpen"),
        contents: &[sharpen.strength, 0.0, 0.0, 0.0]
            .iter()
            .flat_map(|value: &f32| value.to_le_bytes())
            .collect::<Vec<u8>>(),
        usage: BufferUsages::UNIFORM,
    });
    let bindings = device.create_bind_group(
        "sharpen",
        &cache.get_bind_group_layout(&pipeline.layout),
        &BindGroupEntries::sequential((post.source, strength.as_entire_binding())),
    );
    // The pass's time on the GPU, where a run measures it
    // (`console::gpu_times`).
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "sharpen");
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("sharpen"),
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

/// The setting reads as the strength, and only the temporal choices are
/// sharpened.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;
    let read = |value: Option<&str>| {
        let mut settings = crate::settings::Settings::defaults();
        if let Some(value) = value {
            settings.apply(ui::screens::options::SHARPENING, value);
        }
        settings.display().sharpening
    };
    // Unset, a step of the options, past each end, and not a number.
    let cases = [
        (None, MEDIUM),
        (Some("0.6"), 0.6),
        (Some("2"), 1.0),
        (Some("-1"), 0.0),
        (Some("sharp"), MEDIUM),
    ];
    let wrong_reads: Vec<_> = cases
        .iter()
        .map(|&(value, wanted)| (value, read(value), wanted))
        .filter(|(_, got, wanted)| got != wanted)
        .collect();
    let display = Display {
        sharpening: 0.6,
        ..Display::default()
    };
    let wrong_choices: Vec<_> = crate::temporal::CHOICES
        .iter()
        .filter(|(choice, _)| wanted(*choice, &display).is_some() != choice.temporal())
        .map(|(_, name)| *name)
        .collect();
    vec![
        (
            "sharpening_setting_reads",
            check(
                wrong_reads.is_empty(),
                format!("wrong reads {wrong_reads:?}"),
            ),
        ),
        (
            "sharpening_after_temporal_choices_only",
            check(
                wrong_choices.is_empty(),
                format!("wrong choices {wrong_choices:?}"),
            ),
        ),
    ]
}
