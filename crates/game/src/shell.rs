//! KOTOR's shell pass (see `shell.wgsl`; the ProgFX shells, read from the
//! K1 executable for behaviour: a character's skinned meshes drawn again,
//! each vertex pushed out along its normal, in a texture of their own,
//! blended as the texture says): here the duel mode's red warning shell
//! over a character whose swing cannot be blocked, added to what is behind
//! (`duel_view`).

use std::collections::BTreeMap;

use bevy::asset::{load_internal_asset, uuid_handle};
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use sim::SimId;

use crate::cast::BodyRoots;
use crate::material::OdysseyMaterial;

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

/// Where the characters' meshes are, to draw shells over.
pub(crate) struct Bodies<'a, 'w, 's> {
    pub roots: &'a BodyRoots,
    pub children: &'a Query<'w, 's, &'static Children>,
    pub meshes: &'a ShellMeshes<'w, 's>,
}

/// The meshes a shell may cover: an Odyssey mesh, skinned or not.
pub(crate) type ShellMeshes<'w, 's> = Query<
    'w,
    's,
    (&'static Mesh3d, Option<&'static SkinnedMesh>),
    With<MeshMaterial3d<OdysseyMaterial>>,
>;

/// Shells in `material` over the characters `wanted` (by character, in
/// `by`): new ones for those newly wanted, none left over the others or
/// over a character no longer drawn.
pub(crate) fn follow(
    by: &mut BTreeMap<SimId, Vec<Entity>>,
    wanted: &[SimId],
    material: &Handle<ShellMaterial>,
    bodies: &Bodies,
    commands: &mut Commands,
) {
    let ended: Vec<SimId> = by
        .keys()
        .filter(|id| !wanted.contains(id) || !bodies.roots.0.contains_key(id))
        .copied()
        .collect();
    for id in ended {
        for entity in by.remove(&id).into_iter().flatten() {
            if let Ok(mut shell) = commands.get_entity(entity) {
                shell.despawn();
            }
        }
    }
    for &id in wanted {
        let Some(&root) = bodies.roots.0.get(&id) else {
            continue;
        };
        if by.contains_key(&id) {
            continue;
        }
        let mut drawn = Vec::new();
        for entity in bodies.children.iter_descendants(root) {
            let Ok((mesh, skinned)) = bodies.meshes.get(entity) else {
                continue;
            };
            let mut shell = commands.spawn((
                Mesh3d(mesh.0.clone()),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                ChildOf(entity),
            ));
            if let Some(skinned) = skinned {
                shell.insert(skinned.clone());
            }
            drawn.push(shell.id());
        }
        by.insert(id, drawn);
    }
}
