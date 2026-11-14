//! A distortion's material (`heat_haze.wgsl`): what the executable draws
//! for its distortion-class models (`model_fx::Emitter::distorts`; the
//! swoop's `lmg_distort`, the Force powers' ripples). It draws their nodes
//! into the stencil alone (colour off; particles where their alpha passes
//! its test, `GREATER` 0), copies the frame, and draws the copy back over
//! the stencil displaced by its procedural `distortiontex` (32 × 32,
//! `ringtexdistort`) stretched over the screen: each quad here shows the
//! scene behind it where its art's alpha passes, displaced so
//! (`ring_distort.wgsl`, which `glow_shell`'s distortions share).
//!
//! The copy is of the whole scene, as the executable's is: the surfaces
//! that show it (these, `glow_shell`'s distortion, JA's refraction) are
//! drawn after the transparent pass (`draw_after_transparent`), not in
//! Bevy's transmissive pass, whose copy holds the opaque scene alone. From
//! that one, everything drawn blended (Tatooine's sky and spires, rooms
//! whose textures carry alpha) was missing, and a Force Wave showed the
//! area's fog colour, the clear colour, in their place.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::camera::{MainPassResolutionOverride, Viewport};
use bevy::core_pipeline::core_3d::{main_opaque_pass_3d, main_transparent_pass_3d};
use bevy::core_pipeline::{Core3d, Core3dSystems};
use bevy::image::ToExtents;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{
    MaterialPipeline, MaterialPipelineKey, Transmissive3d, ViewTransmissionTexture,
    main_transmissive_pass_3d,
};
use bevy::prelude::*;
use bevy::render::RenderApp;
use bevy::render::camera::ExtractedCamera;
use bevy::render::render_phase::{SortedRenderPhase, ViewSortedRenderPhases};
use bevy::render::render_resource::{
    AsBindGroup, BlendState, RenderPassDescriptor, RenderPipelineDescriptor,
    SpecializedMeshPipelineError, StoreOp,
};
use bevy::render::renderer::{RenderContext, ViewQuery};
use bevy::render::view::{ExtractedView, ViewDepthTexture, ViewTarget};
use bevy::shader::ShaderRef;

const SHADER: Handle<Shader> = uuid_handle!("3c1d7e52-9a40-4b6f-8e21-5f0a6b9d2c47");
/// `kjk::screen_copy`: the frame copy read by where a point stands on the
/// drawn frame, for every surface that shows it.
const SCREEN_COPY: Handle<Shader> = uuid_handle!("8e5b2f19-4c7a-4d03-b6e1-27a9c0f3d58e");
/// `kjk::ring_distort`: the executable's ring displacement of that copy.
const RING_DISTORT: Handle<Shader> = uuid_handle!("5d7a2e90-3b18-4c6f-a4d2-9e0b7f1c6a35");

pub struct HeatHazePlugin;

impl Plugin for HeatHazePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SCREEN_COPY, "screen_copy.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, RING_DISTORT, "ring_distort.wgsl", Shader::from_wgsl);
        load_internal_asset!(app, SHADER, "heat_haze.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<HeatHazeMaterial>::default());
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app.init_resource::<Held>().add_systems(
            Core3d,
            (
                hold_back
                    .after(main_opaque_pass_3d)
                    .before(main_transmissive_pass_3d),
                give_back
                    .after(main_transparent_pass_3d)
                    .after(crate::effects_quality::ParticlesDrawn),
                draw_after_transparent.after(give_back),
            )
                .in_set(Core3dSystems::MainPass),
        );
    }
}

/// The transmissive items of the view being drawn, held back from Bevy's
/// transmissive pass until the transparent pass is done.
#[derive(Resource, Default)]
struct Held(SortedRenderPhase<Transmissive3d>);

/// Takes the view's transmissive items out of Bevy's pass, still copying
/// the opaque scene as it would have: the speed blur, drawn first of the
/// transparent surfaces, smears that copy.
fn hold_back(
    view: ViewQuery<(
        &ExtractedCamera,
        &ExtractedView,
        &ViewTarget,
        Option<&ViewTransmissionTexture>,
    )>,
    mut phases: ResMut<ViewSortedRenderPhases<Transmissive3d>>,
    mut held: ResMut<Held>,
    mut ctx: RenderContext,
) {
    let (camera, extracted, target, transmission) = view.into_inner();
    let Some(phase) = phases.get_mut(&extracted.retained_view_entity) else {
        return;
    };
    held.0 = std::mem::take(phase);
    if !held.0.items.is_empty() {
        copy_frame(&mut ctx, camera, target, transmission);
    }
}

/// Puts the held items back for `draw_after_transparent`.
fn give_back(
    view: ViewQuery<&ExtractedView>,
    mut phases: ResMut<ViewSortedRenderPhases<Transmissive3d>>,
    mut held: ResMut<Held>,
) {
    if let Some(phase) = phases.get_mut(&view.into_inner().retained_view_entity) {
        *phase = std::mem::take(&mut held.0);
    }
}

/// What `draw_after_transparent` reads of a view.
type DrawnView = (
    &'static ExtractedCamera,
    &'static ExtractedView,
    &'static ViewTarget,
    Option<&'static ViewTransmissionTexture>,
    &'static ViewDepthTexture,
    Option<&'static MainPassResolutionOverride>,
);

/// Copies the whole frame, then draws the transmissive items over it,
/// far to near, as Bevy's transmissive pass draws them in one step.
fn draw_after_transparent(
    world: &World,
    view: ViewQuery<DrawnView>,
    phases: Res<ViewSortedRenderPhases<Transmissive3d>>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (camera, extracted, target, transmission, depth, resolution) = view.into_inner();
    let Some(phase) = phases.get(&extracted.retained_view_entity) else {
        return;
    };
    if phase.items.is_empty() {
        return;
    }
    copy_frame(&mut ctx, camera, target, transmission);
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("distortions_after_transparent"),
        color_attachments: &[Some(target.get_color_attachment())],
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(viewport) =
        Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
    {
        pass.set_camera_viewport(&viewport);
    }
    if let Err(err) = phase.render(&mut pass, world, view_entity) {
        error!("drawing the distortions: {err:?}");
    }
}

/// The frame as it stands into the view's transmission texture: all of the
/// main texture (Bevy makes the copy its size and format), so whatever
/// rectangle the main pass draws into (the viewport, or under DLSS its
/// render size, `screen_copy.wgsl`) is in it at the same texels.
fn copy_frame(
    ctx: &mut RenderContext,
    camera: &ExtractedCamera,
    target: &ViewTarget,
    transmission: Option<&ViewTransmissionTexture>,
) {
    let (Some(size), Some(transmission)) = (camera.physical_target_size, transmission) else {
        return;
    };
    ctx.command_encoder().copy_texture_to_texture(
        target.main_texture().as_image_copy(),
        transmission.texture.as_image_copy(),
        size.to_extents(),
    );
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct HeatHazeMaterial {
    /// The particles' art: its alpha is the stencil's test.
    #[texture(0)]
    #[sampler(1)]
    art: Option<Handle<Image>>,
}

impl HeatHazeMaterial {
    pub fn new(art: Option<Handle<Image>>) -> HeatHazeMaterial {
        HeatHazeMaterial { art }
    }
}

impl Material for HeatHazeMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    /// With the alpha mode left opaque, this puts it in the transmissive
    /// phase (`draw_after_transparent`); `specialize` sets its blend.
    fn reads_view_transmission_texture(&self) -> bool {
        true
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _pipeline: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if let Some(fragment) = descriptor.fragment.as_mut() {
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState::REPLACE);
            }
        }
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}
