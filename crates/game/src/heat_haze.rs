//! A distortion's material (`heat_haze.wgsl`): what the executable draws
//! for its distortion-class models (`model_fx::Emitter::distorts`; the
//! swoop's `lmg_distort`, the Force powers' ripples). It draws their nodes
//! into the stencil alone (colour off; particles where their alpha passes
//! its test, `GREATER` 0), copies the frame, and draws the copy back over
//! the stencil displaced by its procedural `distortiontex` (32 × 32,
//! `ringtexdistort`) stretched over the screen: each quad here shows the
//! scene behind it (Bevy's view transmission texture, the scene before
//! transparent surfaces) where its art's alpha passes, displaced so.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendState, RenderPipelineDescriptor, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const SHADER: Handle<Shader> = uuid_handle!("3c1d7e52-9a40-4b6f-8e21-5f0a6b9d2c47");

pub struct HeatHazePlugin;

impl Plugin for HeatHazePlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "heat_haze.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<HeatHazeMaterial>::default());
    }
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

    fn reads_view_transmission_texture(&self) -> bool {
        true
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
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
