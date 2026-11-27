//! Two-texture legacy blending for Single-Sample Linear HDR (`LinearLegacyBlend`).
//!
//! Classic KOTOR / Jedi Academy transparent surfaces (`Blend`, `BlendVertex`,
//! `Additive`, `AddOne`, `Lighten`, `Modulate`, `Hologram`, glows, and shells)
//! expect blending arithmetic to happen in gamma space and often rely on values
//! clamping or multiplying twice (`Modulate`: `2 * src * dst`). Blending them
//! directly into a linear HDR target produces grey-purple shifts on sand, haze,
//! and sky layers (such as Dune Sea `tat_m18aa`).
//!
//! When `LinearLegacyBlend` is on a scene camera (`LightingMode::enhanced()`
//! and `Msaa::Off`), legacy non-depth-writing transparents are held out of
//! `Transparent3d` before `main_transparent_pass_3d`, rendered into two
//! `Rgba16Float` targets:
//! - `@location(0) accum_colour` (cleared to `0`)
//! - `@location(1) transmittance` (cleared to `1`)
//!
//! Then one fullscreen pass composites them onto the linear HDR `ViewTarget`
//! while preserving linear background radiance above `1.0`:
//! `decode_22(trans * encode_22(clamp(bg, 0, 1)) + accum) + max(bg - 1, 0) * trans`.
//! Physical HDR glow draws (`HDR_GLOW`) and post-`first_legacy` items are drawn
//! back into the linear `ViewTarget` right after the composite.

use std::collections::HashMap;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::{MainPassResolutionOverride, Viewport};
use bevy::core_pipeline::core_3d::{
    CORE_3D_DEPTH_FORMAT, Transparent3d, main_opaque_pass_3d, main_transparent_pass_3d,
};
use bevy::core_pipeline::{Core3d, Core3dSystems, FullscreenShader};
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::camera::ExtractedCamera;
use bevy::render::diagnostic::RecordDiagnostics;
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_phase::{SortedRenderPhase, ViewSortedRenderPhases};
use bevy::render::render_resource::binding_types::{texture_2d, uniform_buffer};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
    BlendFactor, BlendOperation, BlendState, BufferInitDescriptor, BufferUsages,
    CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, FragmentState, LoadOp,
    MultisampleState, Operations, PipelineCache, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipelineDescriptor, ShaderStages, StoreOp, TextureDescriptor, TextureDimension,
    TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::shader::ShaderDefVal;

const SHADER: Handle<Shader> = uuid_handle!("9d4e71a2-6c38-4b09-a2f5-8e1c4d7b3a91");
const TARGET_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Marks a single-sample linear HDR scene camera that routes legacy
/// non-depth-writing transparents through the two-texture composite pass.
#[derive(Component, ExtractComponent, Clone, Copy)]
pub(crate) struct LinearLegacyBlend;

/// System set grouping `legacy_blend` pass execution before `heat_haze` gives back.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct LegacyBlendDrawn;

pub(crate) struct LegacyBlendPlugin;

impl Plugin for LegacyBlendPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "legacy_blend.wgsl", crate::shader_checks::wgsl);
        app.add_plugins(ExtractComponentPlugin::<LinearLegacyBlend>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<Held>()
            .init_resource::<Variants>()
            .add_systems(
                Core3d,
                (
                    hold_back
                        .after(main_opaque_pass_3d)
                        .before(main_transparent_pass_3d),
                    draw_legacy_blend.after(main_transparent_pass_3d),
                    give_back.after(draw_legacy_blend),
                )
                    .in_set(Core3dSystems::MainPass)
                    .in_set(LegacyBlendDrawn),
            );
    }
}

/// Maps a single-target legacy blend state into `(accum, transmittance, is_modulate)`
/// blend states for two `Rgba16Float` targets.
///
/// Returns `None` for blend modes outside the supported legacy transparent set.
fn mrt_blend(original: BlendState) -> Option<(BlendState, BlendState, bool)> {
    if original.color.operation != BlendOperation::Add {
        return None;
    }
    if original.color.src_factor == BlendFactor::Dst
        && original.color.dst_factor == BlendFactor::Src
    {
        let multiply = BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::Src,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        };
        return Some((multiply, multiply, true));
    }
    let valid_src = matches!(
        original.color.src_factor,
        BlendFactor::One | BlendFactor::SrcAlpha | BlendFactor::Zero
    );
    let valid_dst = matches!(
        original.color.dst_factor,
        BlendFactor::One
            | BlendFactor::OneMinusSrcAlpha
            | BlendFactor::SrcAlpha
            | BlendFactor::Zero
    );
    if !valid_src || !valid_dst {
        return None;
    }
    let keep_alpha = BlendComponent {
        src_factor: BlendFactor::Zero,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    };
    let accum = BlendState {
        color: original.color,
        alpha: keep_alpha,
    };
    let trans = BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::Zero,
            dst_factor: original.color.dst_factor,
            operation: BlendOperation::Add,
        },
        alpha: keep_alpha,
    };
    Some((accum, trans, false))
}

/// Builds a two-target (`accum_colour`, `transmittance`) variant of `original`
/// when `original` represents a legacy non-depth-writing transparent draw.
fn mrt_copy(original: &RenderPipelineDescriptor) -> Option<RenderPipelineDescriptor> {
    let fragment = original.fragment.as_ref()?;
    let sid = fragment.shader.id();
    let known_shader = sid == crate::material::SHADER.id()
        || sid == crate::glow_shell::SHADER.id()
        || sid == crate::shell::SHADER.id();
    if !known_shader {
        return None;
    }
    if fragment
        .shader_defs
        .iter()
        .any(|d| matches!(d, ShaderDefVal::Bool(name, true) if name == "HDR_GLOW"))
    {
        return None;
    }
    if original
        .depth_stencil
        .as_ref()
        .is_some_and(|d| d.depth_write_enabled == Some(true))
    {
        return None;
    }
    let target = fragment.targets.first().and_then(|t| t.as_ref())?;
    let (accum_blend, trans_blend, is_modulate) = mrt_blend(target.blend?)?;

    let mut copy = original.clone();
    copy.label = Some("legacy_blend_mrt".into());
    copy.multisample = MultisampleState::default();
    let copy_fragment = copy.fragment.as_mut()?;
    copy_fragment.shader_defs.push("LEGACY_BLEND".into());
    if is_modulate {
        copy_fragment.shader_defs.push("LEGACY_MODULATE".into());
    }
    copy_fragment.targets = vec![
        Some(ColorTargetState {
            format: TARGET_FORMAT,
            blend: Some(accum_blend),
            write_mask: ColorWrites::ALL,
        }),
        Some(ColorTargetState {
            format: TARGET_FORMAT,
            blend: Some(trans_blend),
            write_mask: ColorWrites::ALL,
        }),
    ];
    if let Some(depth) = copy.depth_stencil.as_mut() {
        depth.format = CORE_3D_DEPTH_FORMAT;
    }
    Some(copy)
}

#[derive(Clone)]
struct CompositePass {
    layout: BindGroupLayoutDescriptor,
    pipeline: CachedRenderPipelineId,
}

#[derive(Resource, Default)]
struct Variants {
    /// Original pipeline -> `Some(mrt_id)` for legacy transparents, `None` for
    /// HDR glows / depth-writing / non-legacy pipelines.
    mrt: HashMap<CachedRenderPipelineId, Option<CachedRenderPipelineId>>,
    /// MRT pipeline -> original pipeline (to restore `Transparent3d`).
    original: HashMap<CachedRenderPipelineId, CachedRenderPipelineId>,
    /// Composite fullscreen pipeline per `ViewTarget` format.
    composites: HashMap<TextureFormat, CompositePass>,
}

impl Variants {
    fn mrt(
        &mut self,
        cache: &PipelineCache,
        id: CachedRenderPipelineId,
    ) -> Option<CachedRenderPipelineId> {
        if let Some(cached) = self.mrt.get(&id) {
            return *cached;
        }
        let desc = cache.get_render_pipeline_descriptor(id);
        let built = mrt_copy(desc).map(|copy| cache.queue_render_pipeline(copy));
        self.mrt.insert(id, built);
        if let Some(mrt_id) = built {
            self.original.insert(mrt_id, id);
        }
        built
    }

    fn composite(
        &mut self,
        cache: &PipelineCache,
        fullscreen: &FullscreenShader,
        format: TextureFormat,
    ) -> CompositePass {
        self.composites
            .entry(format)
            .or_insert_with(|| make_composite(cache, fullscreen, format))
            .clone()
    }
}

fn make_composite(
    cache: &PipelineCache,
    fullscreen: &FullscreenShader,
    format: TextureFormat,
) -> CompositePass {
    let layout = BindGroupLayoutDescriptor::new(
        "legacy_blend_composite",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_2d(TextureSampleType::Float { filterable: false }),
                uniform_buffer::<UVec4>(false),
            ),
        ),
    );
    let pipeline = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("legacy_blend_composite".into()),
        layout: vec![layout.clone()],
        vertex: fullscreen.to_vertex_state(),
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
    });
    CompositePass { layout, pipeline }
}

struct LegacyFrame {
    accum: CachedTexture,
    trans: CachedTexture,
    bounds: [u32; 4],
    composite: CompositePass,
}

#[derive(Resource, Default)]
struct Held {
    legacy: SortedRenderPhase<Transparent3d>,
    after: SortedRenderPhase<Transparent3d>,
    order: Vec<(Entity, MainEntity)>,
    is_legacy: Vec<bool>,
    frame: Option<LegacyFrame>,
}

type HeldView = (
    &'static ExtractedView,
    &'static ExtractedCamera,
    &'static ViewTarget,
    Option<&'static MainPassResolutionOverride>,
);

fn drawn_extent(
    camera: &ExtractedCamera,
    resolution: Option<&MainPassResolutionOverride>,
) -> Option<(UVec2, [u32; 4])> {
    match Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution) {
        Some(vp) => {
            let far = vp.physical_position + vp.physical_size;
            Some((
                far,
                [vp.physical_position.x, vp.physical_position.y, far.x, far.y],
            ))
        }
        None => {
            let size = camera.physical_target_size?;
            Some((size, [0, 0, size.x, size.y]))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn hold_back(
    view: ViewQuery<HeldView, With<LinearLegacyBlend>>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut held: ResMut<Held>,
    mut variants: ResMut<Variants>,
    cache: Res<PipelineCache>,
    fullscreen: Res<FullscreenShader>,
    mut textures: ResMut<TextureCache>,
    device: Res<RenderDevice>,
) {
    let (extracted, camera, target, resolution) = view.into_inner();
    held.frame = None;
    let Some(phase) = phases.get_mut(&extracted.retained_view_entity) else {
        return;
    };
    let Some((extent, bounds)) = drawn_extent(camera, resolution) else {
        return;
    };
    if extent.x == 0 || extent.y == 0 {
        return;
    }

    let held = &mut *held;
    held.is_legacy.clear();
    let mut first = None;
    let mut ready = true;
    let mut last = false;

    for item in phase.items.values() {
        let is_legacy = if item.batch_range.is_empty() {
            last
        } else {
            let mapped = variants.mrt(&cache, item.pipeline).is_some_and(|mrt| {
                ready &= cache.get_render_pipeline(mrt).is_some();
                true
            });
            last = mapped;
            mapped
        };
        if is_legacy && first.is_none() {
            first = Some(held.is_legacy.len());
        }
        held.is_legacy.push(is_legacy);
    }

    let Some(first) = first else {
        return;
    };
    let composite = variants.composite(&cache, &fullscreen, target.main_texture_format());
    if !ready || cache.get_render_pipeline(composite.pipeline).is_none() {
        return;
    }

    let size = Extent3d {
        width: extent.x,
        height: extent.y,
        depth_or_array_layers: 1,
    };
    let make_tex = |label: &'static str| TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TARGET_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    let accum = textures.get(&device, make_tex("legacy_blend_accum"));
    let trans = textures.get(&device, make_tex("legacy_blend_transmittance"));

    held.order.clear();
    held.legacy.items.clear();
    held.after.items.clear();

    for (at, (key, mut item)) in std::mem::take(&mut phase.items).into_iter().enumerate() {
        held.order.push(key);
        if at < first {
            phase.items.insert(key, item);
            continue;
        }
        if held.is_legacy[at] {
            if let Some(Some(mrt_id)) = variants.mrt.get(&item.pipeline) {
                item.pipeline = *mrt_id;
            }
            held.legacy.items.insert(key, item);
        } else {
            held.after.items.insert(key, item);
        }
    }

    held.frame = Some(LegacyFrame {
        accum,
        trans,
        bounds,
        composite,
    });
}

type DrawnView = (
    &'static ExtractedCamera,
    &'static ViewTarget,
    &'static ViewDepthTexture,
    Option<&'static MainPassResolutionOverride>,
);

fn draw_legacy_blend(
    world: &World,
    view: ViewQuery<DrawnView, With<LinearLegacyBlend>>,
    held: Res<Held>,
    cache: Res<PipelineCache>,
    device: Res<RenderDevice>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (camera, target, depth, resolution) = view.into_inner();
    let Some(frame) = &held.frame else {
        return;
    };
    let Some(composite) = cache.get_render_pipeline(frame.composite.pipeline) else {
        return;
    };
    let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution);
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let time_span = diagnostics.time_span(ctx.command_encoder(), "legacy_blend");

    {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("legacy_blend_mrt"),
            color_attachments: &[
                Some(RenderPassColorAttachment {
                    view: &frame.accum.default_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(LinearRgba::BLACK.into()),
                        store: StoreOp::Store,
                    },
                }),
                Some(RenderPassColorAttachment {
                    view: &frame.trans.default_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(LinearRgba::WHITE.into()),
                        store: StoreOp::Store,
                    },
                }),
            ],
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = &viewport {
            pass.set_camera_viewport(viewport);
        }
        if let Err(err) = held.legacy.render(&mut pass, world, view_entity) {
            error!("drawing legacy transparents: {err:?}");
        }
    }

    let post = target.post_process_write();
    let bounds = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("legacy_blend_bounds"),
        contents: &frame
            .bounds
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<u8>>(),
        usage: BufferUsages::UNIFORM,
    });
    let bindings = device.create_bind_group(
        "legacy_blend_composite",
        &cache.get_bind_group_layout(&frame.composite.layout),
        &BindGroupEntries::sequential((
            post.source,
            &frame.accum.default_view,
            &frame.trans.default_view,
            bounds.as_entire_binding(),
        )),
    );
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("legacy_blend_composite"),
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
        pass.set_pipeline(composite);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }

    if !held.after.items.is_empty() {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("legacy_blend_hdr_after"),
            color_attachments: &[Some(target.get_color_attachment())],
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = &viewport {
            pass.set_camera_viewport(viewport);
        }
        if let Err(err) = held.after.render(&mut pass, world, view_entity) {
            error!("drawing HDR glows after legacy composite: {err:?}");
        }
    }
    time_span.end(ctx.command_encoder());
}

fn give_back(
    view: ViewQuery<&ExtractedView, With<LinearLegacyBlend>>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut held: ResMut<Held>,
    variants: Res<Variants>,
) {
    if held.frame.take().is_none() {
        return;
    }
    let Some(phase) = phases.get_mut(&view.into_inner().retained_view_entity) else {
        return;
    };
    let held = &mut *held;
    let mut before = std::mem::take(&mut phase.items);
    for key in held.order.drain(..) {
        let item = before
            .swap_remove(&key)
            .or_else(|| {
                held.legacy.items.swap_remove(&key).map(|mut item| {
                    if let Some(original) = variants.original.get(&item.pipeline) {
                        item.pipeline = *original;
                    }
                    item
                })
            })
            .or_else(|| held.after.items.swap_remove(&key));
        if let Some(item) = item {
            phase.items.insert(key, item);
        }
    }
}

#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::probe::check;

    let blend_ok = mrt_blend(BlendState::ALPHA_BLENDING).is_some_and(|(accum, trans, modulate)| {
        !modulate
            && accum.color.src_factor == BlendFactor::SrcAlpha
            && accum.color.dst_factor == BlendFactor::OneMinusSrcAlpha
            && trans.color.src_factor == BlendFactor::Zero
            && trans.color.dst_factor == BlendFactor::OneMinusSrcAlpha
    });

    let additive = BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::SrcAlpha,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent::OVER,
    };
    let additive_ok = mrt_blend(additive).is_some_and(|(accum, trans, modulate)| {
        !modulate
            && accum.color.src_factor == BlendFactor::SrcAlpha
            && accum.color.dst_factor == BlendFactor::One
            && trans.color.src_factor == BlendFactor::Zero
            && trans.color.dst_factor == BlendFactor::One
    });

    let add_one = BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent::OVER,
    };
    let add_one_ok = mrt_blend(add_one).is_some_and(|(accum, trans, modulate)| {
        !modulate
            && accum.color.src_factor == BlendFactor::One
            && trans.color.dst_factor == BlendFactor::One
    });

    let hologram = crate::glow_shell::hologram_blend();
    let hologram_ok = mrt_blend(hologram).is_some_and(|(accum, trans, modulate)| {
        !modulate
            && accum.color.src_factor == BlendFactor::One
            && accum.color.dst_factor == BlendFactor::SrcAlpha
            && trans.color.src_factor == BlendFactor::Zero
            && trans.color.dst_factor == BlendFactor::SrcAlpha
    });

    let mod_state = BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::Dst,
            dst_factor: BlendFactor::Src,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent::OVER,
    };
    let modulate_ok = mrt_blend(mod_state).is_some_and(|(accum, trans, modulate)| {
        modulate
            && accum.color.src_factor == BlendFactor::Zero
            && accum.color.dst_factor == BlendFactor::Src
            && trans.color.src_factor == BlendFactor::Zero
            && trans.color.dst_factor == BlendFactor::Src
    });

    let ok = blend_ok && additive_ok && add_one_ok && hologram_ok && modulate_ok;
    vec![(
        "mrt_blend_maps_all_legacy_modes",
        check(
            ok,
            format!(
                "blend={blend_ok} additive={additive_ok} add_one={add_one_ok} hologram={hologram_ok} modulate={modulate_ok}"
            ),
        ),
    )]
}
