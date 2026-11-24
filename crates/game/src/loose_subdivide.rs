//! A finer copy of a robe or cape mesh, made once when the mesh is built.
//! KOTOR's robes have about 8 cm between vertices, so the skin bends between
//! the solved bones in large flat pieces, and the folds and the flutter of
//! the vertex shader (`loose_legs.wgsl`) cannot show on them. Each triangle
//! is cut into four through the middles of its edges, which puts about
//! 4 cm between vertices.
//!
//! A new vertex takes the mean of its edge's two ends in every attribute
//! but the push out of the body (`loose_skin::push_between`).
//! Its four bones are the four that weigh most over both ends, with their
//! weights made whole again. A normal is made unit length again. Two
//! triangles that share an edge share the new vertex on it. Two sides of a
//! seam in the texture are separate vertices, so each side gets its own.

use std::collections::HashMap;

use bevy::mesh::{Indices, Mesh, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

/// `mesh` with each triangle cut into four, or `None` when it is not a list
/// of triangles with indices, or holds an attribute of a kind this does not
/// know.
pub(crate) fn subdivided(mesh: &Mesh) -> Option<Mesh> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let triangles: Vec<[usize; 3]> = match mesh.indices()? {
        Indices::U16(list) => list
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| t.map(usize::from))
            .collect(),
        Indices::U32(list) => list
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| t.map(|v| v as usize))
            .collect(),
    };
    let count = mesh.count_vertices();
    let mut middles: HashMap<(usize, usize), usize> = HashMap::new();
    let mut edges: Vec<(usize, usize)> = Vec::new();
    let mut middle = |a: usize, b: usize| -> usize {
        let key = (a.min(b), a.max(b));
        *middles.entry(key).or_insert_with(|| {
            edges.push(key);
            count + edges.len() - 1
        })
    };
    let mut indices: Vec<u32> = Vec::with_capacity(triangles.len() * 12);
    for &[a, b, c] in &triangles {
        let (ab, bc, ca) = (middle(a, b), middle(b, c), middle(c, a));
        for t in [[a, ab, ca], [ab, b, bc], [ca, bc, c], [ab, bc, ca]] {
            indices.extend(t.map(|v| v as u32));
        }
    }
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, mesh.asset_usage);
    let joints = mesh.attribute(Mesh::ATTRIBUTE_JOINT_INDEX);
    let weights = mesh.attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT);
    for (attribute, values) in mesh.attributes() {
        if attribute.id == Mesh::ATTRIBUTE_JOINT_INDEX.id
            || attribute.id == Mesh::ATTRIBUTE_JOINT_WEIGHT.id
        {
            continue;
        }
        let unit = attribute.id == Mesh::ATTRIBUTE_NORMAL.id;
        let push = attribute.id == crate::loose_skin::PUSH.id;
        let values = match values {
            VertexAttributeValues::Float32(v) => {
                VertexAttributeValues::Float32(extended(v, &edges, |a, b| (a + b) / 2.0))
            }
            VertexAttributeValues::Float32x2(v) => {
                VertexAttributeValues::Float32x2(extended(v, &edges, |a, b| {
                    ((Vec2::from(a) + Vec2::from(b)) / 2.0).into()
                }))
            }
            VertexAttributeValues::Float32x3(v) => {
                VertexAttributeValues::Float32x3(extended(v, &edges, |a, b| {
                    let mean = (Vec3::from(a) + Vec3::from(b)) / 2.0;
                    match unit {
                        true => mean.normalize_or(Vec3::from(a)).into(),
                        false => mean.into(),
                    }
                }))
            }
            VertexAttributeValues::Float32x4(v) => {
                VertexAttributeValues::Float32x4(extended(v, &edges, |a, b| match push {
                    true => crate::loose_skin::push_between(a, b),
                    false => ((Vec4::from(a) + Vec4::from(b)) / 2.0).into(),
                }))
            }
            _ => return None,
        };
        out.insert_attribute(attribute.clone(), values);
    }
    if let (
        Some(VertexAttributeValues::Uint16x4(joints)),
        Some(VertexAttributeValues::Float32x4(weights)),
    ) = (joints, weights)
    {
        let mut new_joints = joints.clone();
        let mut new_weights = weights.clone();
        for &(a, b) in &edges {
            let (j, w) = merged((joints[a], weights[a]), (joints[b], weights[b]));
            new_joints.push(j);
            new_weights.push(w);
        }
        out.insert_attribute(
            Mesh::ATTRIBUTE_JOINT_INDEX,
            VertexAttributeValues::Uint16x4(new_joints),
        );
        out.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, new_weights);
    }
    out.insert_indices(Indices::U32(indices));
    Some(out)
}

/// `values` with one more value for each edge, made from its two ends.
fn extended<T: Copy>(values: &[T], edges: &[(usize, usize)], mean: impl Fn(T, T) -> T) -> Vec<T> {
    let mut out = values.to_vec();
    out.extend(edges.iter().map(|&(a, b)| mean(values[a], values[b])));
    out
}

/// The four bones that weigh most over two vertices taken half each, with
/// their weights summed to one.
fn merged(a: ([u16; 4], [f32; 4]), b: ([u16; 4], [f32; 4])) -> ([u16; 4], [f32; 4]) {
    let mut sums: Vec<(u16, f32)> = Vec::with_capacity(8);
    for (joints, weights) in [a, b] {
        for (&joint, &weight) in joints.iter().zip(&weights) {
            if weight <= 0.0 {
                continue;
            }
            match sums.iter_mut().find(|(j, _)| *j == joint) {
                Some((_, sum)) => *sum += weight / 2.0,
                None => sums.push((joint, weight / 2.0)),
            }
        }
    }
    sums.sort_by(|x, y| y.1.total_cmp(&x.1));
    sums.truncate(4);
    let total: f32 = sums.iter().map(|(_, w)| w).sum();
    let mut joints = [0u16; 4];
    let mut weights = [0.0f32; 4];
    for (k, (joint, weight)) in sums.into_iter().enumerate() {
        joints[k] = joint;
        weights[k] = if total > 0.0 { weight / total } else { 0.0 };
    }
    (joints, weights)
}
