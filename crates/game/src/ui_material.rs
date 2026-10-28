//! The game's screens' material (`ui.wgsl`): crisp at any window size.
//! A texture is read Catmull-Rom where enlarged and from its mipmaps where
//! shrunk; a shape (`ui::raster::Pixels::shape`: a font, one-colour line
//! art) keeps its edge one screen pixel wide. Drawn into the screens' own
//! image (`ui_composite`): blended, colour over and alpha as coverage
//! (the image premultiplied); added, light that covers nothing.

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor,
    ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;

const SHADER: Handle<Shader> = uuid_handle!("3c7d1f52-9a84-4e6b-b0f3-5d28e6a14c97");

pub struct UiMaterialPlugin;

impl Plugin for UiMaterialPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(app, SHADER, "ui.wgsl", Shader::from_wgsl);
        app.add_plugins(MaterialPlugin::<UiMaterial>::default());
    }
}

#[derive(Clone, Copy, Debug, Default, ShaderType)]
pub struct UiParams {
    /// The texture's size in texels.
    pub size: Vec2,
    /// 1 for a shape.
    pub shape: u32,
    /// 1 for a 3D view (`gui_scene`, its encoded values kept as any
    /// texture's). It is drawn at its own size (`gui_scene` renders it the
    /// control's pixels), so it is read texel for texel, never enlarged (it
    /// has no `size`: Catmull-Rom on a 1 × 1 grid bent the whole view, the
    /// figure wide in the middle and ringed).
    pub view: u32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
#[bind_group_data(UiKey)]
pub struct UiMaterial {
    #[uniform(0)]
    pub params: UiParams,
    /// None: plain white.
    #[texture(1)]
    #[sampler(2)]
    pub texture: Option<Handle<Image>>,
    /// Added rather than blended (the texture's TXI says `additive`).
    pub additive: bool,
}

/// What a material's pipeline depends on: whether it adds.
#[repr(C)]
#[derive(Copy, Clone, Hash, Eq, PartialEq)]
pub struct UiKey {
    additive: bool,
}

impl From<&UiMaterial> for UiKey {
    fn from(material: &UiMaterial) -> UiKey {
        UiKey {
            additive: material.additive,
        }
    }
}

/// Light added (by its alpha), the coverage kept.
const ADD: BlendState = BlendState {
    color: BlendComponent {
        src_factor: BlendFactor::SrcAlpha,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
    alpha: BlendComponent {
        src_factor: BlendFactor::Zero,
        dst_factor: BlendFactor::One,
        operation: BlendOperation::Add,
    },
};

impl Material for UiMaterial {
    fn fragment_shader() -> ShaderRef {
        ShaderRef::Handle(SHADER)
    }

    fn alpha_mode(&self) -> AlphaMode {
        if self.additive {
            AlphaMode::Add
        } else {
            AlphaMode::Blend
        }
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
        key: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        if key.bind_group_data.additive
            && let Some(target) = descriptor
                .fragment
                .as_mut()
                .and_then(|fragment| fragment.targets.first_mut())
                .and_then(Option::as_mut)
        {
            target.blend = Some(ADD);
        }
        Ok(())
    }
}
