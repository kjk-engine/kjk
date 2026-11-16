//! A linear frame through the temporal pass (`docs/LIGHTING-PLAN.md`,
//! step 9). It runs only when `KJK_LINEAR=temporal` asks for it, only under
//! Enhanced lighting, and only while a temporal pass runs (Bevy's TAA, kjk's
//! upscaler or DLSS). With the switch unset, under Classic lighting and
//! under MSAA, nothing here runs.
//!
//! The frame holds gamma-encoded values (decision 6, `rolloff.rs`). A
//! temporal pass is made for linear light, and Bevy tells DLSS that the
//! frame is linear. So one pass decodes the frame to linear light just
//! before the temporal pass, and another encodes it again just after. The
//! lighting, the blends, bloom, the roll-off and everything drawn after them
//! receive the same encoded values as before.
//!
//! A dump's `linear_frame` fact says which mode the switch asked for,
//! whether it is in use, and how many frames each pass ran in.

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::{Hdr, MainPassResolutionOverride, Viewport};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_resource::binding_types::texture_2d;
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, CachedRenderPipelineId,
    ColorTargetState, ColorWrites, FragmentState, Operations, PipelineCache,
    RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor, ShaderStages,
    TextureFormat, TextureSampleType,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::view::{ExtractedView, ViewTarget};
use bevy::render::{Render, RenderApp, RenderStartup, RenderSystems};

use crate::lifecycle_view::SceneCamera;
use crate::rolloff::LightingMode;
use crate::temporal::Temporal;

const SHADER: Handle<Shader> = uuid_handle!("4c1e8a7b-2f93-4d06-b5a8-0e7d3c9f6a21");

/// Whether `KJK_LINEAR=temporal` is set, read once.
fn asked() -> bool {
    static ASKED: OnceLock<bool> = OnceLock::new();
    *ASKED.get_or_init(|| std::env::var("KJK_LINEAR").as_deref() == Ok("temporal"))
}

/// The frames the decoding pass and the encoding pass ran in since the run
/// began.
static DECODED: AtomicU64 = AtomicU64::new(0);
static ENCODED: AtomicU64 = AtomicU64::new(0);

/// On a scene camera whose frame is decoded before the temporal pass and
/// encoded after it.
#[derive(Component, ExtractComponent, Clone, Copy)]
pub struct LinearFrame;

pub struct LinearFramePlugin;

impl Plugin for LinearFramePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "linear_frame.wgsl", Shader::from_wgsl);
        app.add_plugins(ExtractComponentPlugin::<LinearFrame>::default())
            .add_systems(Last, (mark.after(crate::temporal::Applied), facts).chain());
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
                        .after(Core3dSystems::MainPass)
                        .before(Core3dSystems::EarlyPostProcess),
                    encode_pass
                        .after(Core3dSystems::EarlyPostProcess)
                        .before(Core3dSystems::PostProcess),
                ),
            );
    }
}

/// Whether the passes are wanted: the switch is set, the lighting is
/// Enhanced, and a temporal pass runs (TAA, kjk's upscaler or DLSS). Under
/// MSAA there is no temporal pass, and the two passes would only undo each
/// other.
fn wanted(lighting: Option<&LightingMode>, temporal: &Temporal) -> bool {
    asked() && lighting.is_some_and(|lighting| lighting.enhanced()) && temporal.used.temporal()
}

/// Every scene camera with a float frame takes the passes while they are
/// wanted, as `rolloff::mark` gives it the roll-off.
fn mark(
    lighting: Option<Res<LightingMode>>,
    temporal: Res<Temporal>,
    cameras: Query<(Entity, Has<Hdr>, Has<LinearFrame>), SceneCamera>,
    mut commands: Commands,
) {
    let wanted = wanted(lighting.as_deref(), &temporal);
    for (camera, hdr, marked) in &cameras {
        let wanted = wanted && hdr;
        // A camera may be gone by the time this is applied.
        if wanted && !marked {
            commands.entity(camera).try_insert(LinearFrame);
        }
        if !wanted && marked {
            commands.entity(camera).try_remove::<LinearFrame>();
        }
    }
}

/// For a dump, `linear_frame`: the mode the switch asked for, whether it is
/// in use, how many scene cameras take the passes, and the frames each pass
/// ran in since the run began.
fn facts(
    lighting: Option<Res<LightingMode>>,
    temporal: Res<Temporal>,
    cameras: Query<Has<LinearFrame>, SceneCamera>,
    mut facts: ResMut<console::DumpFacts>,
) {
    if !facts.wanted() {
        return;
    }
    facts.0.insert(
        "linear_frame".into(),
        serde_json::json!({
            "mode": if asked() { "temporal" } else { "off" },
            "active": wanted(lighting.as_deref(), &temporal),
            "cameras": cameras.iter().filter(|has| *has).count(),
            "decode_frames": DECODED.load(Ordering::Relaxed),
            "encode_frames": ENCODED.load(Ordering::Relaxed),
        }),
    );
}

/// The passes' binding (the frame) and their pipelines for each target
/// format.
#[derive(Resource)]
struct Pipelines {
    layout: BindGroupLayoutDescriptor,
    fullscreen: FullscreenShader,
    ids: HashMap<(TextureFormat, bool), CachedRenderPipelineId>,
}

/// A view's two pipelines: decoding and encoding.
#[derive(Component)]
struct PipelineIds {
    decode: CachedRenderPipelineId,
    encode: CachedRenderPipelineId,
}

fn init(mut commands: Commands, fullscreen: Res<FullscreenShader>) {
    let layout = BindGroupLayoutDescriptor::new(
        "linear_frame",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (texture_2d(TextureSampleType::Float { filterable: true }),),
        ),
    );
    commands.insert_resource(Pipelines {
        layout,
        fullscreen: fullscreen.clone(),
        ids: HashMap::new(),
    });
}

fn prepare(
    mut commands: Commands,
    cache: Res<PipelineCache>,
    mut pipelines: ResMut<Pipelines>,
    views: Query<(Entity, &ExtractedView), With<LinearFrame>>,
) {
    let pipelines = &mut *pipelines;
    for (entity, view) in &views {
        let format = view.target_format;
        let mut id = |decode: bool| {
            *pipelines.ids.entry((format, decode)).or_insert_with(|| {
                cache.queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some(
                        if decode {
                            "linear_decode"
                        } else {
                            "linear_encode"
                        }
                        .into(),
                    ),
                    layout: vec![pipelines.layout.clone()],
                    vertex: pipelines.fullscreen.to_vertex_state(),
                    fragment: Some(FragmentState {
                        shader: SHADER,
                        entry_point: Some(
                            if decode {
                                "decode_frame"
                            } else {
                                "encode_frame"
                            }
                            .into(),
                        ),
                        targets: vec![Some(ColorTargetState {
                            format,
                            blend: None,
                            write_mask: ColorWrites::ALL,
                        })],
                        ..default()
                    }),
                    ..default()
                })
            })
        };
        let ids = PipelineIds {
            decode: id(true),
            encode: id(false),
        };
        commands.entity(entity).insert(ids);
    }
}

/// One pass over the frame, from `post_process_write`'s source to its
/// destination. With `drawn`, only that part of the frame is worked out, and
/// the rest of the destination is cleared to black.
#[allow(clippy::too_many_arguments)]
fn run(
    target: &ViewTarget,
    drawn: Option<Viewport>,
    id: CachedRenderPipelineId,
    label: &'static str,
    pipelines: &Pipelines,
    cache: &PipelineCache,
    device: &RenderDevice,
    ctx: &mut RenderContext,
) -> bool {
    let Some(render) = cache.get_render_pipeline(id) else {
        return false;
    };
    let post = target.post_process_write();
    let bindings = device.create_bind_group(
        label,
        &cache.get_bind_group_layout(&pipelines.layout),
        &BindGroupEntries::sequential((post.source,)),
    );
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), label);
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some(label),
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
        if let Some(drawn) = drawn {
            pass.set_scissor_rect(
                drawn.physical_position.x,
                drawn.physical_position.y,
                drawn.physical_size.x,
                drawn.physical_size.y,
            );
        }
        pass.draw(0..3, 0..1);
    }
    time_span.end(ctx.command_encoder());
    true
}

/// The frame is decoded to linear light after the main pass, before the
/// temporal pass. Under kjk's upscaler and DLSS the scene is drawn into the
/// top left part of the frame (`MainPassResolutionOverride`), and those
/// passes read that part alone. So only that part is decoded.
fn decode_pass(
    view: ViewQuery<
        (
            &ViewTarget,
            &PipelineIds,
            &ExtractedCamera,
            Option<&MainPassResolutionOverride>,
        ),
        With<LinearFrame>,
    >,
    pipelines: Res<Pipelines>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, ids, camera, resolution) = view.into_inner();
    // The two passes run in the same frames. A frame decoded and not
    // encoded, or encoded and not decoded, would show wrong.
    if cache.get_render_pipeline(ids.encode).is_none() {
        return;
    }
    // None when the main pass draws the whole frame. A scissor rectangle
    // must lie inside the frame.
    let drawn = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
        .zip(camera.physical_target_size)
        .map(|(mut drawn, size)| {
            drawn.clamp_to_size(size);
            drawn
        });
    if run(
        target,
        drawn,
        ids.decode,
        "linear_decode",
        &pipelines,
        &cache,
        &device,
        &mut ctx,
    ) {
        DECODED.fetch_add(1, Ordering::Relaxed);
    }
}

/// The frame is encoded again after the temporal pass, before bloom and the
/// roll-off.
fn encode_pass(
    view: ViewQuery<(&ViewTarget, &PipelineIds), With<LinearFrame>>,
    pipelines: Res<Pipelines>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let (target, ids) = view.into_inner();
    if cache.get_render_pipeline(ids.decode).is_none() {
        return;
    }
    // The temporal pass writes the whole frame, so the whole frame is
    // encoded.
    if run(
        target,
        None,
        ids.encode,
        "linear_encode",
        &pipelines,
        &cache,
        &device,
        &mut ctx,
    ) {
        ENCODED.fetch_add(1, Ordering::Relaxed);
    }
}
