//! Bounding boxes that never change, for the meshes whose vertices move
//! every frame: a character's skinned meshes and the danglymeshes.
//!
//! Bevy culls a mesh by its box (`Aabb`, in the mesh entity's own space).
//! By default it works the box out again from the bones each frame for a
//! skinned mesh, and from the vertices each frame a mesh's data changes,
//! as a danglymesh's does whenever it sways. Each box made here is spawned
//! with the mesh, marked `NoAutoAabb`, and large enough for every place
//! its vertices can reach, so neither is needed.

use asset_odyssey::model::{MeshIr, ModelIr};
use bevy::camera::primitives::Aabb;
use bevy::prelude::*;

use crate::scene::to_mat4;

/// The box of a skinned mesh `part` of model `ir`, in the space of the
/// mesh's node.
///
/// The model's skinned meshes in their rest pose span a box in the model's
/// space. A pose can carry a vertex anywhere within the model's own size
/// of that box: a body lying down reaches no further from where it stood
/// than its height, and an arm raised or a leg kicking reaches less far.
/// The box is that rest box grown by the model's size on every side, taken
/// into the node's space.
pub fn skinned(ir: &ModelIr, part: &MeshIr) -> Aabb {
    let mut low = Vec3::splat(f32::INFINITY);
    let mut high = Vec3::splat(f32::NEG_INFINITY);
    for mesh in ir.meshes.iter().filter(|mesh| mesh.skin.is_some()) {
        let Some(node) = ir.nodes.get(mesh.node) else {
            continue;
        };
        let world = to_mat4(&node.world);
        for &position in &mesh.positions {
            let at = world.transform_point3(Vec3::from_array(position));
            low = low.min(at);
            high = high.max(at);
        }
    }
    let to_node = ir
        .nodes
        .get(part.node)
        .map_or(Mat4::IDENTITY, |node| to_mat4(&node.world).inverse());
    let (mut node_low, mut node_high) =
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for corner in 0..8 {
        let pick = |bit: usize, axis: usize| {
            if corner & bit == 0 {
                low[axis]
            } else {
                high[axis]
            }
        };
        let at = to_node.transform_point3(Vec3::new(pick(1, 0), pick(2, 1), pick(4, 2)));
        node_low = node_low.min(at);
        node_high = node_high.max(at);
    }
    let reach = Vec3::splat((high - low).max_element());
    Aabb::from_min_max(node_low - reach, node_high + reach)
}

/// The box of a danglymesh `part`, in the space of its node: its vertices
/// and their rest places, grown by how far a vertex may stray from rest
/// along each axis (`dangly`).
pub fn dangly(part: &MeshIr) -> Option<Aabb> {
    let dangly = part.dangly.as_ref()?;
    let (low, high) = part.positions.iter().chain(&dangly.rest).fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(low, high), &at| {
            (
                low.min(Vec3::from_array(at)),
                high.max(Vec3::from_array(at)),
            )
        },
    );
    let reach = Vec3::splat(dangly.displacement.abs());
    (low.cmple(high).all()).then(|| Aabb::from_min_max(low - reach, high + reach))
}
