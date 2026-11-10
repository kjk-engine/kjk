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
        app.add_plugins(MaterialPlugin::<UiMaterial>::default())
            .add_systems(Last, rebind);
    }
}

/// Marks a material changed when the image it samples changes, so that the
/// material is prepared again with the image's new texture.
///
/// Bevy prepares a material's bindings once, with the GPU texture its image
/// has at that moment. When the image changes later, Bevy makes a new GPU
/// texture for it, but the material keeps sampling the old one. A 3D view's
/// image (`gui_scene`) is resized whenever the window is, and its camera then
/// draws into the new texture. Without this system the screens went on
/// showing the view's last frame from before the resize, so the main menu's
/// figure and smoke stood still.
fn rebind(mut images: MessageReader<AssetEvent<Image>>, mut materials: ResMut<Assets<UiMaterial>>) {
    let changed: Vec<AssetId<Image>> = images
        .read()
        .filter_map(|event| match event {
            AssetEvent::Modified { id } => Some(*id),
            _ => None,
        })
        .collect();
    if changed.is_empty() {
        return;
    }
    let stale: Vec<AssetId<UiMaterial>> = materials
        .iter()
        .filter(|(_, material)| {
            (material.texture.as_ref()).is_some_and(|texture| changed.contains(&texture.id()))
        })
        .map(|(id, _)| id)
        .collect();
    for id in stale {
        if let Some(material) = materials.get_mut(id) {
            material.into_inner();
        }
    }
}

/// The probes of the screens' material (`kjk sim probes`, group "ui
/// material").
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    vec![("a_resized_view_is_sampled_anew", resized_view_sampled())]
}

/// A 3D view's image resized, as the window's resize does: the material
/// sampling it is changed, so it is prepared again with the new texture,
/// and a material sampling another image is left alone. Without `rebind`
/// neither changes (the cause of the frozen main menu, shown so the probe
/// can still see it).
#[cfg(feature = "probes")]
fn resized_view_sampled() -> Result<String, String> {
    use bevy::ecs::message::Messages;
    use bevy::render::render_resource::Extent3d;

    // Whether the view's material and the other material changed.
    let changed = |rebinds: bool| -> Result<(bool, bool), String> {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Image>()
            .init_asset::<UiMaterial>();
        if rebinds {
            app.add_systems(Last, rebind);
        }
        let mut images = app.world_mut().resource_mut::<Assets<Image>>();
        let (view, other) = (images.add(Image::default()), images.add(Image::default()));
        let mut materials = app.world_mut().resource_mut::<Assets<UiMaterial>>();
        let mut sampling = |texture: Handle<Image>| {
            materials.add(UiMaterial {
                params: UiParams::default(),
                texture: Some(texture),
                additive: false,
            })
        };
        let (of_view, of_other) = (sampling(view.clone()), sampling(other));
        app.update();
        app.update();
        let mut cursor = app
            .world()
            .resource::<Messages<AssetEvent<UiMaterial>>>()
            .get_cursor_current();
        app.world_mut()
            .resource_mut::<Assets<Image>>()
            .get_mut(&view)
            .ok_or("the view's image is gone")?
            .resize(Extent3d {
                width: 1920,
                height: 1080,
                depth_or_array_layers: 1,
            });
        app.update();
        app.update();
        let messages = app.world().resource::<Messages<AssetEvent<UiMaterial>>>();
        let modified: Vec<AssetId<UiMaterial>> = cursor
            .read(messages)
            .filter_map(|event| match event {
                AssetEvent::Modified { id } => Some(*id),
                _ => None,
            })
            .collect();
        Ok((
            modified.contains(&of_view.id()),
            modified.contains(&of_other.id()),
        ))
    };
    match (changed(true)?, changed(false)?) {
        ((true, false), (false, false)) => Ok(
            "the material sampling the resized view is prepared again; the other material is \
             left alone; without the rebind neither would change"
                .into(),
        ),
        (with, without) => Err(format!(
            "(view's material changed, other material changed): with the rebind {with:?}, \
             without it {without:?}"
        )),
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
