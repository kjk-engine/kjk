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
//! A skinned model's box holds every pose that leaves its root bone near
//! its rest place. Some clips carry the root bone far away. A cutscene can
//! walk a body several metres, and a few attacks lunge further than a body
//! is tall. A ragdoll can slide or roll away from where its model stands.
//! While a model's root bone is that far from its rest place, its skinned
//! meshes' boxes follow the bones again (`DynamicSkinnedMeshBounds`). The
//! fixed boxes come back once the root bone is near its rest place again.

use asset_odyssey::model::{MeshIr, ModelIr};
use bevy::camera::primitives::Aabb;
use bevy::camera::visibility::{
    DynamicSkinnedMeshBounds, NoAutoAabb, NoFrustumCulling, VisibilitySystems,
};
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::room_queries::corners;
use crate::scene::to_mat4;

/// The nodes whose distance from their rest place decides whether a
/// model's boxes hold. A KOTOR body's clips move its `rootdummy`, and the
/// Jedi Academy poses and the ragdolls move its pelvis.
const ROOT_BONE: &str = "rootdummy";
const PELVIS: &str = "pelvis";

/// The share of a model's size its root bone may stray from its rest
/// place while its fixed boxes still hold. The boxes are grown by the
/// model's size and this share of it.
const STRAY_SHARE: f32 = 0.5;

/// A root bone that comes back within this share of the distance it may
/// stray gives the model its fixed boxes back. The gap keeps a bone that
/// hovers near the limit from switching every frame.
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
/// model's root bone comes back near its rest place.
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

/// The lowest and highest of `points`, or `None` when there are none.
fn span(points: impl IntoIterator<Item = Vec3>) -> Option<(Vec3, Vec3)> {
    points.into_iter().fold(None, |span, at| match span {
        None => Some((at, at)),
        Some((low, high)) => Some((low.min(at), high.max(at))),
    })
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
        corners(low, high)
            .into_iter()
            .map(|at| to_node.transform_point3(at)),
    )
    .unwrap_or(rest);
    Aabb::from_min_max(node_low - reach, node_high + reach)
}

/// The root bones of model `ir` whose node entities are `nodes`, for a
/// model whose meshes at rest span `rest`. `None` when the model has
/// neither a `rootdummy` nor a pelvis.
pub fn root_bones(ir: &ModelIr, nodes: &[Entity], rest: (Vec3, Vec3)) -> Option<RootBones> {
    let named = |wanted: &dyn Fn(&str) -> bool| {
        ir.nodes
            .iter()
            .position(|node| wanted(&node.name.to_ascii_lowercase()))
    };
    let bones: Vec<(Entity, Vec3)> = [
        named(&|name| name == ROOT_BONE),
        named(&|name| name.contains(PELVIS)),
    ]
    .into_iter()
    .flatten()
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

/// A model whose root bones stray too far has its skinned meshes' boxes
/// follow the bones. One whose root bones come back has its fixed boxes
/// back. A mesh that is never culled (a cut one) is left alone.
fn follow_strays(
    mut models: Query<(Entity, &GlobalTransform, &mut RootBones)>,
    places: Query<&GlobalTransform>,
    children: Query<&Children>,
    boxes: Query<&FixedBox, Without<NoFrustumCulling>>,
    mut commands: Commands,
) {
    for (root, placed, mut bones) in &mut models {
        let to_model = placed.affine().inverse();
        let strayed = bones
            .bones
            .iter()
            .filter_map(|&(bone, rest)| {
                let at = to_model.transform_point3(places.get(bone).ok()?.translation());
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
                commands
                    .entity(mesh)
                    .try_remove::<NoAutoAabb>()
                    .try_insert(DynamicSkinnedMeshBounds);
            } else {
                commands
                    .entity(mesh)
                    .try_remove::<DynamicSkinnedMeshBounds>()
                    .try_insert((bounds, NoAutoAabb));
            }
        }
    }
}
