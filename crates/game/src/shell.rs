//! KOTOR's shell pass (see `shell.wgsl`; the ProgFX shells, read from the
//! K1 executable for behaviour: a character's skinned meshes drawn again,
//! each vertex pushed out along its normal, in a texture of their own,
//! blended as the texture says): here the stealth field's, `fx_tex_stealth`,
//! added to what is behind (`stealth_view`).

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const SHADER: Handle<Shader> = uuid_handle!("8f3c2a71-5d64-4b0e-9e1f-7a2b6c4d9e05");

/// How far a shell stands off its model (metres: KOTOR's 0.02).
pub const PUSH_M: f32 = 0.02;

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct ShellParams {
    /// What the texture is multiplied by (alpha scales what is added).
    pub tint: Vec4,
    /// x: the push (metres); y: the texture's scroll.
    pub push: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct ShellMaterial {
    #[uniform(0)]
    pub params: ShellParams,
    #[texture(1)]
    #[sampler(2)]
    pub texture: Option<Handle<Image>>,
}

impl Material for ShellMaterial {
    fn vertex_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
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
        // As the Odyssey material: both sides, no depth writes over the
        // model it covers.
        descriptor.primitive.cull_mode = None;
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_write_enabled = Some(false);
        }
        Ok(())
    }
}

pub struct ShellPlugin;

impl Plugin for ShellPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "shell.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<ShellMaterial>::default());
    }
}
