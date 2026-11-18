//! Bounding boxes that never change, for the meshes whose vertices move
//! every frame. These are the skinned meshes of characters and creatures,
//! and the danglymeshes.
//!
//! Bevy culls a mesh by its box (`Aabb`, in the mesh entity's own space).
//! By default it works the box of a skinned mesh out again from the bones
//! each frame. It also works a mesh's box out again from the vertices each
//! frame the mesh's data changes, as a danglymesh's does whenever it sways.
//! Each box made here is spawned with the mesh and marked `NoAutoAabb`, so
//! neither is needed.
//!
//! A skinned model's box holds every pose that leaves its root bone and its
//! pelvis near their rest places. Some clips carry them far away. A
//! cutscene can walk a body several metres, and a few attacks lunge further
//! than a body is tall. A ragdoll can slide or roll away from where its
//! model stands. While either bone is that far from its rest place, the
//! model's skinned meshes' boxes follow the bones again
//! (`DynamicSkinnedMeshBounds`). The fixed boxes come back once both bones
//! are near their rest places again. A model with neither bone has no way
//! to tell, so its skinned meshes' boxes always follow the bones.

use asset_odyssey::model::{MeshIr, ModelIr};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{DynamicSkinnedMeshBounds, VisibilitySystems};
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::room_queries::corners;
use crate::scene::{span, to_mat4};

/// The name of a model's root bone. A KOTOR body's clips move it.
const ROOT_BONE: &str = "rootdummy";

/// A part of the name of a model's pelvis node. The Jedi Academy poses and
/// the ragdolls move it.
const PELVIS: &str = "pelvis";

/// The share of a model's size its root bone or pelvis may stray from its
/// rest place while its fixed boxes still hold. The boxes are grown by the
/// model's size and this share of it.
const STRAY_SHARE: f32 = 0.5;

/// Once the root bone and the pelvis are both back within this share of the
/// distance they may stray, the model gets its fixed boxes back. The gap
/// keeps a bone that hovers near the limit from switching every frame.
const BACK_SHARE: f32 = 0.8;

pub struct FixedBoundsPlugin;

impl Plugin for FixedBoundsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            follow_strays
                .after(TransformSystems::Propagate)
                .before(VisibilitySystems::CalculateBounds),
        );
    }
}

/// The box a skinned mesh was spawned with. It is put back when its
/// model's root bone and pelvis come back near their rest places.
#[derive(Component, Clone, Copy)]
pub struct FixedBox(pub Aabb);

/// Sits on a skinned model's root. It holds the nodes that decide whether
/// the model's fixed boxes hold, each with its rest place in the model's
/// space, and how far they may stray from it.
#[derive(Component)]
pub struct RootBones {
    bones: Vec<(Entity, Vec3)>,
    may_stray: f32,
    following: bool,
}

/// The size of a model whose meshes at rest span `rest` (model space). It
/// is the length of that box's diagonal. No vertex is further than this from
/// another, so a pose that turns the body about a point inside the box
/// keeps every vertex within this distance of the box.
fn size((low, high): (Vec3, Vec3)) -> f32 {
    (high - low).length()
}

/// The box of a skinned mesh whose node rests at `node_world` (model
/// space), in the space of that node. `rest` is the model's rest box in
/// model space, around all its meshes (`scene::local_bounds`).
///
/// A pose turns the body about its root bone, which can carry a vertex up
/// to the model's size away from the rest box. A body lying down is an
/// example. The root bone itself may also stray up to `STRAY_SHARE` of the
/// model's size before the boxes follow the bones instead. The box is the
/// rest box grown on every side by both, taken into the node's space.
pub fn skinned(rest: (Vec3, Vec3), node_world: Mat4) -> Aabb {
    let (low, high) = rest;
    let reach = Vec3::splat(size(rest) * (1.0 + STRAY_SHARE));
    let to_node = node_world.inverse();
    let (node_low, node_high) = span(
        corners(low - reach, high + reach)
            .into_iter()
            .map(|at| to_node.transform_point3(at)),
    )
    .unwrap_or(rest);
    Aabb::from_min_max(node_low, node_high)
}

/// The indices of the nodes of model `ir` that decide whether its fixed
/// boxes hold: its root bone and its pelvis, those it has.
pub fn root_bone_nodes(ir: &ModelIr) -> Vec<usize> {
    let named = |wanted: &dyn Fn(&str) -> bool| {
        ir.nodes
            .iter()
            .position(|node| wanted(&node.name.to_ascii_lowercase()))
    };
    [
        named(&|name| name == ROOT_BONE),
        named(&|name| name.contains(PELVIS)),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The root bones of model `ir` whose node entities are `nodes`, for a
/// model whose meshes at rest span `rest`. `None` when the model has
/// neither a `rootdummy` nor a pelvis.
pub fn root_bones(ir: &ModelIr, nodes: &[Entity], rest: (Vec3, Vec3)) -> Option<RootBones> {
    let bones: Vec<(Entity, Vec3)> = root_bone_nodes(ir)
        .into_iter()
        .filter_map(|index| {
            let at = to_mat4(&ir.nodes[index].world).transform_point3(Vec3::ZERO);
            Some((*nodes.get(index)?, at))
        })
        .collect();
    (!bones.is_empty()).then(|| RootBones {
        bones,
        may_stray: size(rest) * STRAY_SHARE,
        following: false,
    })
}

/// The box of a danglymesh `part`, in the space of its node. It holds the
/// mesh's vertices and their rest places. It is grown on every side by the
/// mesh's displacement, the farthest a vertex may stray from its rest place
/// along each axis.
pub fn dangly(part: &MeshIr) -> Option<Aabb> {
    let dangly = part.dangly.as_ref()?;
    let (low, high) = span(
        part.positions
            .iter()
            .chain(&dangly.rest)
            .map(|&at| Vec3::from_array(at)),
    )?;
    let reach = Vec3::splat(dangly.displacement.abs());
    Some(Aabb::from_min_max(low - reach, high + reach))
}

/// A model whose root bone or pelvis strays too far has its skinned
/// meshes' boxes follow the bones. One whose bones come back has its fixed
/// boxes back. A mesh that a cut took off its fixed box has no `FixedBox`,
/// so it is left alone.
///
/// `NoAutoAabb` stays on while the boxes follow the bones. Bevy's
/// `update_skinned_mesh_bounds` ignores it, and without it Bevy would also
/// work out a swaying skinned danglymesh's box from its vertices each frame.
fn follow_strays(
    mut models: Query<(Entity, &GlobalTransform, &mut RootBones)>,
    transforms: Query<&GlobalTransform>,
    children: Query<&Children>,
    boxes: Query<&FixedBox>,
    mut commands: Commands,
) {
    for (root, model_transform, mut bones) in &mut models {
        let to_model = model_transform.affine().inverse();
        let strayed = bones
            .bones
            .iter()
            .filter_map(|&(bone, rest)| {
                let at = to_model.transform_point3(transforms.get(bone).ok()?.translation());
                Some(at.distance(rest))
            })
            .fold(0.0, f32::max);
        let follow = if bones.following {
            strayed > bones.may_stray * BACK_SHARE
        } else {
            strayed > bones.may_stray
        };
        if follow == bones.following {
            continue;
        }
        bones.following = follow;
        for mesh in children.iter_descendants(root) {
            let Ok(&FixedBox(bounds)) = boxes.get(mesh) else {
                continue;
            };
            if follow {
                commands.entity(mesh).try_insert(DynamicSkinnedMeshBounds);
            } else {
                commands
                    .entity(mesh)
                    .try_remove::<DynamicSkinnedMeshBounds>()
                    .try_insert(bounds);
            }
        }
    }
}

/// Gives the skinned mesh `mesh` boxes that follow its bones from now on, in
/// place of its fixed box. Its mesh must hold its bones' boxes
/// (`Mesh::generate_skinned_mesh_bounds`).
pub fn follow_bones_for_good(mesh: &mut EntityCommands) {
    mesh.remove::<FixedBox>().insert(DynamicSkinnedMeshBounds);
}
