//! Bevy works the box of a skinned mesh out again from its bones each frame,
//! and a changed mesh's box from its vertices. The boxes made here are fixed
//! (`NoAutoAabb`). A cutscene, a lunge or a ragdoll can carry the root bone
//! or the pelvis far from its rest place, and while either is that far the
//! model's skinned meshes' boxes follow the bones (`DynamicSkinnedMeshBounds`).

use asset_odyssey::model::{MeshIr, ModelIr};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{DynamicSkinnedMeshBounds, VisibilitySystems};
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::room_queries::corners;
use crate::scene::{span, to_mat4};

const ROOT_BONE: &str = "rootdummy";

const PELVIS: &str = "pelvis";

const STRAY_SHARE: f32 = 0.5;

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

#[derive(Component, Clone, Copy)]
pub struct FixedBox(pub Aabb);

#[derive(Component)]
pub struct RootBones {
    bones: Vec<(Entity, Vec3)>,
    may_stray: f32,
    following: bool,
}

fn size((low, high): (Vec3, Vec3)) -> f32 {
    (high - low).length()
}

/// A pose turns the body about its root bone, so a vertex can end up the
/// model's size away from the rest box, and the root bone may stray besides.
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

pub fn dangly(part: &MeshIr) -> Option<Aabb> {
    let dangly = part.dangly.as_ref()?;
    let (low, high) = span(
        part.positions
            .iter()
            .chain(&dangly.rest)
            .map(|&at| Vec3::from_array(at)),
    )?;
    let reach = Vec3::splat(crate::dangly::reach(dangly));
    Some(Aabb::from_min_max(low - reach, high + reach))
}

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

pub fn follow_bones_for_good(mesh: &mut EntityCommands) {
    mesh.remove::<FixedBox>().insert(DynamicSkinnedMeshBounds);
}
