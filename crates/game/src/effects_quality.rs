//! The options' Effects Quality (`display.effects`).
//!
//! At High, KOTOR's emitter particles are drawn in the scene's transparent
//! pass, as they always were.
//!
//! At Low they are drawn into a texture half the frame's width and half its
//! height, a quarter of its pixels, and that texture is then laid over the
//! frame (`effects_quality.wgsl`). Smoke and mist are large quads drawn over
//! one another many times: the main menu's cover each pixel of the view
//! about 75 times. On a weak GPU the time goes into blending those pixels,
//! not into the number of particles, so drawing a quarter of the pixels
//! saves most of it while every particle is still drawn.
//!
//! What counts as a particle here: the quads of a level's emitters, drawn
//! through the particle list (`model_fx`), and the emitters of the screens'
//! 3D views (`emitter_view`), which sort after every other see-through
//! surface (`material::EMITTERS_LAST`). Jedi Academy's effects (`scene_fx`)
//! stay at full size.
//!
//! The order of drawing. See-through surfaces (windows, lightsaber blades,
//! glows) are drawn far to near, and each emitter's particles are one item
//! among them, sorted by the emitter's place. A run is a stretch of that
//! order with particles only. Each run is drawn into the half-size texture
//! and laid over the frame at its own point of the order, and the other
//! see-through surfaces up to the next run are then drawn over it at full
//! size. So the order is the same as at High, as long as the frame has at
//! most `RUNS` runs. A frame with more runs puts every particle after the
//! last run's start into that run, and the see-through surfaces among them
//! are drawn over all of them. Such a surface then shows over the smoke in
//! front of it. A blade adds light, so it reads as light shining through
//! the smoke. The screens' emitters are drawn after everything else, so
//! they are always a single run.
//!
//! The texture's colour holds the particles' light, already multiplied by
//! how much each covers. Its alpha holds how much of the scene still shows
//! through them. Each particle's own blend writes both: an added particle
//! leaves the alpha as it is, and an alpha-blended one multiplies it by what
//! it lets through. Laying the texture over the frame then multiplies the
//! frame by the alpha and adds the colour, which is both blends at once.

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
use bevy::render::extract_component::{ExtractComponent, ExtractComponentPlugin};
use bevy::render::render_phase::{SortedRenderPhase, ViewSortedRenderPhases};
use bevy::render::render_resource::binding_types::{
    texture_2d, texture_depth_2d, texture_depth_2d_multisampled,
};
use bevy::render::render_resource::{
    BindGroupEntries, BindGroupLayoutDescriptor, BindGroupLayoutEntries, BlendComponent,
    BlendFactor, BlendOperation, BlendState, CachedRenderPipelineId, ColorTargetState, ColorWrites,
    CompareFunction, DepthStencilState, Extent3d, FragmentState, LoadOp, MultisampleState,
    Operations, PipelineCache, RenderPassColorAttachment, RenderPassDepthStencilAttachment,
    RenderPassDescriptor, RenderPipelineDescriptor, ShaderStages, StoreOp, TextureDescriptor,
    TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
};
use bevy::render::renderer::{RenderContext, RenderDevice, ViewQuery};
use bevy::render::sync_world::MainEntity;
use bevy::render::texture::{CachedTexture, TextureCache};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::shader::ShaderDefVal;

use crate::material::EMITTERS_LAST;

const SHADER: Handle<Shader> = uuid_handle!("6f2c9a41-d83e-4b17-a5c0-3e8d1f7b9264");

/// The options' Effects Quality (`settings`, `display.effects`).
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EffectsQuality {
    /// Particles are drawn into a texture of half the frame's width and
    /// height.
    Low,
    /// Particles are drawn at the frame's own size.
    #[default]
    High,
}

impl EffectsQuality {
    /// The quality named by a value from the settings file.
    pub fn named(value: &str) -> EffectsQuality {
        match value {
            "low" => EffectsQuality::Low,
            _ => EffectsQuality::High,
        }
    }

    /// Its value in the settings file.
    pub fn name(self) -> &'static str {
        match self {
            EffectsQuality::Low => "low",
            EffectsQuality::High => "high",
        }
    }

    /// How many of the frame's pixels a particle's quad is drawn over: a
    /// quarter at Low.
    pub fn pixel_part(self) -> f64 {
        match self {
            EffectsQuality::Low => 0.25,
            EffectsQuality::High => 1.0,
        }
    }
}

/// On a 3D camera whose particles are drawn at half size.
#[derive(Component, ExtractComponent, Clone, Copy)]
pub struct HalfSizeParticles;

pub struct EffectsQualityPlugin;

impl Plugin for EffectsQualityPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "effects_quality.wgsl", Shader::from_wgsl);
        app.init_resource::<EffectsQuality>()
            .add_plugins(ExtractComponentPlugin::<HalfSizeParticles>::default())
            // The facts read the particle lists after every spray wrote its
            // list for the frame, and before the render world takes them.
            .add_systems(Last, (mark, level_particle_facts));
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
                    draw_half_size.after(main_transparent_pass_3d),
                    give_back.after(draw_half_size),
                )
                    .in_set(Core3dSystems::MainPass)
                    .in_set(ParticlesDrawn),
            );
    }
}

/// The systems that draw the particles at half size and lay them over the
/// frame. The distortions drawn after the transparent pass (`heat_haze`)
/// come after them, as they came after the particles before.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ParticlesDrawn;

/// Every 3D camera takes the marker at Low and loses it at High. A marked
/// camera's depth can also be read by a shader, which the half-size pass
/// needs. A camera is changed only in the frame the setting changes or the
/// camera is added.
fn mark(
    quality: Res<EffectsQuality>,
    mut cameras: Query<(Entity, &mut Camera3d, Has<HalfSizeParticles>)>,
    mut commands: Commands,
) {
    let changed = quality.is_changed();
    let low = *quality == EffectsQuality::Low;
    for (entity, mut camera, marked) in &mut cameras {
        if !changed && !camera.is_added() {
            continue;
        }
        let usages = TextureUsages::from(camera.depth_texture_usages);
        if low && !marked {
            camera.depth_texture_usages = (usages | TextureUsages::TEXTURE_BINDING).into();
            commands.entity(entity).try_insert(HalfSizeParticles);
        }
        if !low && marked {
            camera.depth_texture_usages = (usages - TextureUsages::TEXTURE_BINDING).into();
            commands.entity(entity).try_remove::<HalfSizeParticles>();
        }
    }
}

/// For a dump, how much drawing the level's particles take, measured the
/// same way on any GPU, as `gui_scene` measures the screens' views:
/// `level_particles`, the quads of the level's emitters drawn through the
/// particle list, the pixels of the scene camera's view they cover (each
/// as often as it is drawn over), the view's pixels (at the smaller size
/// kjk's upscaler draws it, when the upscaler is on), the samples each
/// pixel takes, the Effects Quality, and the pixels the particles are drawn
/// over at it (a quarter of those they cover at Low).
fn level_particle_facts(
    quality: Res<EffectsQuality>,
    cameras: Query<
        (
            &Camera,
            &GlobalTransform,
            &Msaa,
            Option<&crate::upscale::Upscale>,
        ),
        crate::lifecycle_view::SceneCamera,
    >,
    sprays: Query<(
        &GlobalTransform,
        &MeshMaterial3d<crate::material::OdysseyMaterial>,
    )>,
    materials: Res<Assets<crate::material::OdysseyMaterial>>,
    buffers: Res<Assets<bevy::render::storage::ShaderBuffer>>,
    mut facts: ResMut<console::DumpFacts>,
) {
    if !facts.wanted() {
        return;
    }
    let mut views = Vec::new();
    for (camera, eye, msaa, upscale) in &cameras {
        if !camera.is_active {
            continue;
        }
        let Some(window) = camera.physical_viewport_size() else {
            continue;
        };
        // kjk's upscaler draws the scene, and its particles, smaller.
        let size = upscale.map_or(window, |upscale| {
            crate::upscale::drawn(window, upscale.scale)
        });
        let clip_from_world = camera.clip_from_view() * eye.to_matrix().inverse();
        let (mut quads, mut pixels) = (0usize, 0.0f64);
        for (place, material) in &sprays {
            let Some(list) = materials
                .get(&material.0)
                .filter(|made| made.particles.id() != crate::material::NO_PARTICLES.id())
                .and_then(|made| buffers.get(&made.particles))
                .and_then(|buffer| buffer.data.as_deref())
            else {
                continue;
            };
            let to_world = place.to_matrix();
            let vertices = list.as_chunks::<{ crate::material::PARTICLE_BYTES }>().0;
            for quad in vertices.as_chunks::<4>().0 {
                // A vertex starts with its place, three floats.
                let corners = quad.map(|vertex| {
                    let [x, y, z] =
                        std::array::from_fn(|k| f32::from_ne_bytes(vertex.as_chunks::<4>().0[k]));
                    to_world.transform_point3(Vec3::new(x, y, z)).to_array()
                });
                let covered =
                    crate::emitter_view::covered(&corners, clip_from_world, size.as_vec2());
                // The list's room past its quads holds nothing.
                if covered > 0.0 {
                    quads += 1;
                    pixels += covered;
                }
            }
        }
        views.push(serde_json::json!({
            "quads": quads,
            "pixels": pixels.round(),
            "view_pixels": size.x * size.y,
            "samples": msaa.samples(),
            "effects": quality.name(),
            "shaded": (pixels * quality.pixel_part()).round(),
        }));
    }
    facts
        .0
        .insert("level_particles".into(), serde_json::Value::Array(views));
}

/// The format of the half-size texture: float, so added light above 1 is
/// kept until the texture is laid over the frame.
const HALF_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// What is known of a pipeline met in the transparent phase.
#[derive(Clone, Copy)]
struct Known {
    /// It draws the particle list (`material::PARTICLES_SHADER`).
    particle_list: bool,
    /// Its copy for the half-size texture, once one was asked for. `None`
    /// inside when its blend cannot be drawn there (only additive and
    /// alpha blends can).
    half: Option<Option<CachedRenderPipelineId>>,
}

/// The pipelines met so far, and the passes' own.
#[derive(Resource, Default)]
struct Variants {
    known: HashMap<CachedRenderPipelineId, Known>,
    /// Each half-size copy's original, to put the items back as they were.
    original: HashMap<CachedRenderPipelineId, CachedRenderPipelineId>,
    /// The depth downsample and the composite, by MSAA samples and the
    /// frame's format.
    passes: HashMap<(u32, TextureFormat), Passes>,
}

#[derive(Clone)]
struct Passes {
    downsample_layout: BindGroupLayoutDescriptor,
    composite_layout: BindGroupLayoutDescriptor,
    downsample: CachedRenderPipelineId,
    composite: CachedRenderPipelineId,
}

/// At most this many runs of particles are drawn apart (`Run`). The
/// particles after the last run's start all go into the last run.
const RUNS: usize = 4;

/// A run of particles, one after another in the far-to-near order, and the
/// other see-through surfaces between it and the next run.
#[derive(Default)]
struct Run {
    /// The particles, with their half-size pipelines.
    particles: SortedRenderPhase<Transparent3d>,
    /// The see-through surfaces drawn over them, at full size.
    after: SortedRenderPhase<Transparent3d>,
}

/// What `hold_back` took out of the view's transparent phase this frame.
#[derive(Resource, Default)]
struct Held {
    /// The runs, of which the first `used` hold anything this frame.
    runs: Vec<Run>,
    used: usize,
    /// Every item's key in the phase's order, to put them back in it.
    order: Vec<(Entity, MainEntity)>,
    /// Which items are particles, in the phase's order.
    kinds: Vec<bool>,
    /// The textures and passes of this frame, when anything was held.
    frame: Option<HalfFrame>,
}

struct HalfFrame {
    colour: CachedTexture,
    depth: CachedTexture,
    passes: Passes,
}

/// The alpha part of a particle's blend in the half-size texture: what
/// keeps the alpha as how much of the scene shows through.
fn through(colour: BlendComponent) -> Option<BlendComponent> {
    let keep = match colour.dst_factor {
        BlendFactor::One => BlendFactor::One,
        BlendFactor::OneMinusSrcAlpha => BlendFactor::OneMinusSrcAlpha,
        _ => return None,
    };
    Some(BlendComponent {
        src_factor: BlendFactor::Zero,
        dst_factor: keep,
        operation: BlendOperation::Add,
    })
}

/// A particle pipeline's copy for the half-size texture: one sample a
/// pixel, the texture's format, and its blend writing what shows through.
fn half_size_copy(original: &RenderPipelineDescriptor) -> Option<RenderPipelineDescriptor> {
    let mut copy = original.clone();
    copy.label = Some("half_size_particles".into());
    copy.multisample = MultisampleState::default();
    let fragment = copy.fragment.as_mut()?;
    for target in fragment.targets.iter_mut().flatten() {
        let blend = target.blend?;
        target.blend = Some(BlendState {
            color: blend.color,
            alpha: through(blend.color)?,
        });
        target.format = HALF_FORMAT;
    }
    if let Some(depth) = copy.depth_stencil.as_mut() {
        depth.format = CORE_3D_DEPTH_FORMAT;
    }
    Some(copy)
}

impl Variants {
    fn known(&mut self, cache: &PipelineCache, id: CachedRenderPipelineId) -> &mut Known {
        self.known.entry(id).or_insert_with(|| Known {
            particle_list: cache.get_render_pipeline_descriptor(id).vertex.shader.id()
                == crate::material::PARTICLES_SHADER.id(),
            half: None,
        })
    }

    /// The half-size copy of `id`, asked for the first time it is needed.
    fn half(
        &mut self,
        cache: &PipelineCache,
        id: CachedRenderPipelineId,
    ) -> Option<CachedRenderPipelineId> {
        if let Some(half) = self.known(cache, id).half {
            return half;
        }
        let half = half_size_copy(cache.get_render_pipeline_descriptor(id))
            .map(|copy| cache.queue_render_pipeline(copy));
        self.known(cache, id).half = Some(half);
        if let Some(half) = half {
            self.original.insert(half, id);
        }
        half
    }

    fn passes(
        &mut self,
        cache: &PipelineCache,
        fullscreen: &FullscreenShader,
        samples: u32,
        format: TextureFormat,
    ) -> Passes {
        self.passes
            .entry((samples, format))
            .or_insert_with(|| make_passes(cache, fullscreen, samples, format))
            .clone()
    }
}

fn make_passes(
    cache: &PipelineCache,
    fullscreen: &FullscreenShader,
    samples: u32,
    format: TextureFormat,
) -> Passes {
    let multisampled = samples > 1;
    let scene_depth = if multisampled {
        texture_depth_2d_multisampled()
    } else {
        texture_depth_2d()
    };
    let downsample_layout = BindGroupLayoutDescriptor::new(
        "half_size_depth",
        &BindGroupLayoutEntries::single(ShaderStages::FRAGMENT, scene_depth),
    );
    let composite_layout = BindGroupLayoutDescriptor::new(
        "half_size_composite",
        &BindGroupLayoutEntries::sequential(
            ShaderStages::FRAGMENT,
            (
                scene_depth,
                texture_2d(TextureSampleType::Float { filterable: false }),
                texture_depth_2d(),
            ),
        ),
    );
    let mut defs: Vec<ShaderDefVal> = Vec::new();
    if multisampled {
        defs.push("MULTISAMPLED".into());
    }
    let downsample = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("half_size_depth".into()),
        layout: vec![downsample_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: SHADER,
            shader_defs: defs.clone(),
            entry_point: Some("downsample".into()),
            targets: vec![],
        }),
        depth_stencil: Some(DepthStencilState {
            format: CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::Always),
            stencil: default(),
            bias: default(),
        }),
        ..default()
    });
    defs.push("COMPOSITE".into());
    let composite = cache.queue_render_pipeline(RenderPipelineDescriptor {
        label: Some("half_size_composite".into()),
        layout: vec![composite_layout.clone()],
        vertex: fullscreen.to_vertex_state(),
        fragment: Some(FragmentState {
            shader: SHADER,
            shader_defs: defs,
            entry_point: Some("composite".into()),
            // The frame times what shows through, plus the particles' light.
            targets: vec![Some(ColorTargetState {
                format,
                blend: Some(BlendState {
                    color: BlendComponent {
                        src_factor: BlendFactor::One,
                        dst_factor: BlendFactor::SrcAlpha,
                        operation: BlendOperation::Add,
                    },
                    alpha: BlendComponent {
                        src_factor: BlendFactor::Zero,
                        dst_factor: BlendFactor::One,
                        operation: BlendOperation::Add,
                    },
                }),
                write_mask: ColorWrites::ALL,
            })],
        }),
        multisample: MultisampleState {
            count: samples,
            ..default()
        },
        ..default()
    });
    Passes {
        downsample_layout,
        composite_layout,
        downsample,
        composite,
    }
}

/// What `hold_back` reads of a view.
type HeldView = (
    &'static ExtractedView,
    &'static ExtractedCamera,
    &'static ViewTarget,
    &'static Msaa,
    Option<&'static MainPassResolutionOverride>,
);

/// The size of the frame's corner the scene is drawn into: the camera's
/// viewport, made smaller by an upscaler (`game::upscale`) or DLSS, out to
/// its far corner. Without either it is the whole target.
fn drawn_extent(
    camera: &ExtractedCamera,
    resolution: Option<&MainPassResolutionOverride>,
) -> Option<UVec2> {
    match Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution) {
        Some(viewport) => Some(viewport.physical_position + viewport.physical_size),
        None => camera.physical_target_size,
    }
}

/// Takes the particles, and the see-through surfaces after the first one,
/// out of the view's transparent phase, so Bevy's pass draws only those
/// before it. Holds nothing, and the frame is drawn as at High, while a
/// half-size pipeline is still being compiled.
#[allow(clippy::too_many_arguments)]
fn hold_back(
    view: ViewQuery<HeldView, With<HalfSizeParticles>>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    mut held: ResMut<Held>,
    mut variants: ResMut<Variants>,
    cache: Res<PipelineCache>,
    fullscreen: Res<FullscreenShader>,
    mut textures: ResMut<TextureCache>,
    device: Res<RenderDevice>,
) {
    let (extracted, camera, target, msaa, resolution) = view.into_inner();
    held.frame = None;
    let Some(phase) = phases.get_mut(&extracted.retained_view_entity) else {
        return;
    };
    // Under an upscaler the scene fills only a corner of the target. The
    // half-size textures cover that corner, so the composite's edge clamp
    // stops at the drawn frame's last texel.
    let Some(size) = drawn_extent(camera, resolution) else {
        return;
    };
    let held = &mut *held;
    held.kinds.clear();
    let mut first = None;
    let mut ready = true;
    // An item drawn as part of the one before it (an empty batch range)
    // goes where that one goes.
    let mut last = false;
    for item in phase.items.values() {
        let particle = if item.batch_range.is_empty() {
            last
        } else {
            let emitted = item.distance >= EMITTERS_LAST / 2.0;
            let particle = (emitted || variants.known(&cache, item.pipeline).particle_list)
                && match variants.half(&cache, item.pipeline) {
                    Some(half) => {
                        ready &= cache.get_render_pipeline(half).is_some();
                        true
                    }
                    None => false,
                };
            last = particle;
            particle
        };
        if particle && first.is_none() {
            first = Some(held.kinds.len());
        }
        held.kinds.push(particle);
    }
    let Some(first) = first else {
        return;
    };
    let passes = variants.passes(
        &cache,
        &fullscreen,
        msaa.samples(),
        target.main_texture_format(),
    );
    if !ready
        || cache.get_render_pipeline(passes.downsample).is_none()
        || cache.get_render_pipeline(passes.composite).is_none()
    {
        return;
    }
    let half = Extent3d {
        width: size.x.div_ceil(2),
        height: size.y.div_ceil(2),
        depth_or_array_layers: 1,
    };
    let texture = |label: &'static str, format: TextureFormat| TextureDescriptor {
        label: Some(label),
        size: half,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    let colour = textures.get(&device, texture("half_size_particles", HALF_FORMAT));
    let depth = textures.get(&device, texture("half_size_depth", CORE_3D_DEPTH_FORMAT));
    held.order.clear();
    held.runs.resize_with(RUNS, Run::default);
    held.used = 0;
    let mut previous = false;
    for (at, (key, mut item)) in std::mem::take(&mut phase.items).into_iter().enumerate() {
        held.order.push(key);
        let particle = held.kinds[at];
        if at < first {
            phase.items.insert(key, item);
            continue;
        }
        if particle && !previous && held.used < RUNS {
            held.used += 1;
        }
        previous = particle;
        let run = &mut held.runs[held.used - 1];
        if particle {
            if let Some(Some(half)) = variants.known.get(&item.pipeline).and_then(|k| k.half) {
                item.pipeline = half;
            }
            run.particles.items.insert(key, item);
        } else {
            run.after.items.insert(key, item);
        }
    }
    held.frame = Some(HalfFrame {
        colour,
        depth,
        passes,
    });
}

/// What `draw_half_size` reads of a view.
type DrawnView = (
    &'static ExtractedCamera,
    &'static ViewTarget,
    &'static ViewDepthTexture,
    Option<&'static MainPassResolutionOverride>,
);

/// The depth made half size, then for each run its particles drawn into
/// the half-size texture, the texture laid over the frame, and the
/// see-through surfaces between this run and the next drawn over it.
fn draw_half_size(
    world: &World,
    view: ViewQuery<DrawnView, With<HalfSizeParticles>>,
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
    let (Some(downsample), Some(composite)) = (
        cache.get_render_pipeline(frame.passes.downsample),
        cache.get_render_pipeline(frame.passes.composite),
    ) else {
        return;
    };
    let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution);
    let half_depth = || RenderPassDepthStencilAttachment {
        view: &frame.depth.default_view,
        depth_ops: Some(Operations {
            load: LoadOp::Load,
            store: StoreOp::Store,
        }),
        stencil_ops: None,
    };

    let bindings = device.create_bind_group(
        "half_size_depth",
        &cache.get_bind_group_layout(&frame.passes.downsample_layout),
        &BindGroupEntries::single(depth.view()),
    );
    {
        let mut pass = ctx
            .command_encoder()
            .begin_render_pass(&RenderPassDescriptor {
                label: Some("half_size_depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    depth_ops: Some(Operations {
                        load: LoadOp::Clear(0.0),
                        store: StoreOp::Store,
                    }),
                    ..half_depth()
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        pass.set_pipeline(downsample);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
    }

    let bindings = device.create_bind_group(
        "half_size_composite",
        &cache.get_bind_group_layout(&frame.passes.composite_layout),
        &BindGroupEntries::sequential((
            depth.view(),
            &frame.colour.default_view,
            &frame.depth.default_view,
        )),
    );
    let half_viewport = viewport.as_ref().map(|viewport| Viewport {
        physical_position: viewport.physical_position / 2,
        physical_size: UVec2::new(
            viewport.physical_size.x.div_ceil(2),
            viewport.physical_size.y.div_ceil(2),
        ),
        depth: viewport.depth.clone(),
    });
    for run in &held.runs[..held.used] {
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("half_size_particles"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: &frame.colour.default_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Clear(LinearRgba::BLACK.into()),
                    store: StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(half_depth()),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = &half_viewport {
            pass.set_camera_viewport(viewport);
        }
        if let Err(err) = run.particles.render(&mut pass, world, view_entity) {
            error!("drawing the half-size particles: {err:?}");
        }
        drop(pass);

        // The composite reads the frame's depth, so it cannot have it bound
        // to draw into as well: the surfaces after it get a pass of their
        // own.
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("half_size_composite"),
            color_attachments: &[Some(target.get_color_attachment())],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = &viewport {
            pass.set_camera_viewport(viewport);
        }
        pass.set_render_pipeline(composite);
        pass.set_bind_group(0, &bindings, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);

        if run.after.items.is_empty() {
            continue;
        }
        let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("after_half_size_particles"),
            color_attachments: &[Some(target.get_color_attachment())],
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = &viewport {
            pass.set_camera_viewport(viewport);
        }
        if let Err(err) = run.after.render(&mut pass, world, view_entity) {
            error!("drawing the see-through surfaces after the particles: {err:?}");
        }
    }
}

/// Puts every held item back into the view's phase, in its order and with
/// its own pipeline: the phase keeps its items from frame to frame.
fn give_back(
    view: ViewQuery<&ExtractedView, With<HalfSizeParticles>>,
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
        let mut item = before.swap_remove(&key);
        for run in &mut held.runs[..held.used] {
            if item.is_some() {
                break;
            }
            item = run
                .particles
                .items
                .swap_remove(&key)
                .map(|mut item| {
                    if let Some(original) = variants.original.get(&item.pipeline) {
                        item.pipeline = *original;
                    }
                    item
                })
                .or_else(|| run.after.items.swap_remove(&key));
        }
        if let Some(item) = item {
            phase.items.insert(key, item);
        }
    }
    held.used = 0;
}
