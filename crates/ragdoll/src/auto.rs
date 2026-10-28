//! A ragdoll for any skeleton, without a plan: the node all of the mesh
//! hangs from is its root, and every node carrying a real piece of it a
//! body, each a capsule (or a box, for metal) laid along its own vertices'
//! spreads, joined where its node hangs on its parent's. For beasts and
//! droids, whose skeletons are each their own.

use glam::{Mat3, Quat, Vec3};

use crate::body::{
    Group, Joint, JointKind, Material, Part, Skeleton, Solid, Template, segment_distance,
    skin_by_part,
};

/// A node needs this many vertices, spread at least this far from it
/// (metres), to be a body of its own: fingers and greebles ride on their
/// parent's.
const LEAST_VERTICES: usize = 12;
const LEAST_SPREAD: f32 = 0.07;
/// At most this many bodies.
fn most_parts(material: Material) -> usize {
    match material {
        Material::Flesh => 20,
        Material::Metal => 16,
    }
}
/// The share of a body's vertices its solid may leave out at its ends: a
/// capsule's skin is soft; a droid's plates are where they are.
fn ends(material: Material) -> f32 {
    match material {
        Material::Flesh => 0.05,
        Material::Metal => 0.0,
    }
}
/// The share of a capsule's vertices it may leave out around it.
const AROUND: f32 = 0.35;
/// No part weighs less than this share of the average part.
const LIGHTEST: f32 = 0.25;
/// A capsule's radius, at least and at most (metres).
const RADIUS: [f32; 2] = [0.025, 0.6];

impl Template {
    /// A template fitted to `skeleton` alone, of `material`.
    pub fn auto(skeleton: &Skeleton, material: Material) -> Result<Template, String> {
        let nodes = skeleton.nodes;
        let mut counts = vec![0usize; nodes.len()];
        for &(_, node) in skeleton.skin {
            if let Some(count) = counts.get_mut(node) {
                *count += 1;
            }
        }
        let carrying: Vec<usize> = (0..nodes.len())
            .filter(|&node| counts[node] >= LEAST_VERTICES)
            .collect();
        // The root: the deepest node every node carrying mesh hangs from, so
        // no piece of it is left where the animation had it.
        let above = |mut node: usize| {
            let mut chain = vec![node];
            while let Some(parent) = nodes[node].parent {
                chain.push(parent);
                node = parent;
            }
            chain
        };
        let root = carrying
            .first()
            .and_then(|&first| {
                above(first)
                    .into_iter()
                    .find(|&a| carrying.iter().all(|&n| above(n).contains(&a)))
            })
            .ok_or("no node carries enough of the mesh")?;
        // The rest by how far their mesh spreads, the small left to ride.
        let mut chosen: Vec<(f32, usize)> = carrying
            .iter()
            .filter(|&&node| node != root)
            .map(|&node| {
                let spread = skeleton
                    .skin
                    .iter()
                    .filter(|&&(_, n)| n == node)
                    .map(|&(p, _)| p.distance(nodes[node].rest.translation))
                    .fold(0.0, f32::max);
                (spread, node)
            })
            .filter(|&(spread, _)| spread >= LEAST_SPREAD)
            .collect();
        chosen.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        chosen.truncate(most_parts(material) - 1);
        // Parents first: by depth, then index (the root is the shallowest).
        let depth = |node: usize| above(node).len();
        let mut picked: Vec<usize> = chosen.into_iter().map(|(_, node)| node).collect();
        picked.push(root);
        picked.sort_by_key(|&node| (depth(node), node));
        let skin = skin_by_part(skeleton, &picked);
        let fits: Vec<Option<Solid>> = skin.iter().map(|points| fit(points, material)).collect();
        let (cone, twist) = material.give();
        let mut parts: Vec<Part> = Vec::with_capacity(picked.len());
        for (index, &node) in picked.iter().enumerate() {
            let rest = nodes[node].rest;
            let fit = fits[index].unwrap_or(Solid::Capsule {
                ends: [rest.translation; 2],
                radius: RADIUS[0] * 2.0,
            });
            // The nearest chosen node above it (the root has none).
            let parent = above(node)
                .into_iter()
                .skip(1)
                .find_map(|n| picked[..index].iter().position(|&p| p == n));
            let joint = parent.map(|parent| {
                let anchor = rest.translation;
                let aim = (fit.centre() - anchor)
                    .try_normalize()
                    .unwrap_or(fit.axis());
                let kind = JointKind::Ball {
                    cone: cone.to_radians(),
                    twist: twist.to_radians(),
                };
                Joint::at(
                    (parts[parent].rest, rest),
                    anchor,
                    (aim, Quat::IDENTITY),
                    kind,
                )
            });
            parts.push(Part {
                name: "",
                node,
                parent,
                rest,
                solid: fit.moved(rest.inverse()),
                mass: fit.volume() * material.density(),
                joint,
                group: Group::Core,
            });
        }
        // A part with little or no mesh (a bare root) still weighs a
        // share: joints between very unequal bodies shake.
        let least = parts.iter().map(|part| part.mass).sum::<f32>() / parts.len() as f32 * LIGHTEST;
        for part in &mut parts {
            part.mass = part.mass.max(least);
        }
        let mass: f32 = parts.iter().map(|part| part.mass).sum();
        for part in &mut parts {
            part.mass /= mass.max(1e-6);
        }
        Ok(Template::new(parts, mass, material))
    }
}

/// The solid fitted to a part's points (model space): a box for metal
/// along their three spreads, a capsule for flesh along the longest;
/// `None` for too few points.
fn fit(points: &[Vec3], material: Material) -> Option<Solid> {
    if points.len() < LEAST_VERTICES {
        return None;
    }
    let n = points.len() as f32;
    let mean = points.iter().sum::<Vec3>() / n;
    let spread = points.iter().fold(Mat3::ZERO, |sum, &p| {
        let d = p - mean;
        sum + Mat3::from_cols(d * d.x, d * d.y, d * d.z)
    }) / n;
    // The largest spread, then the largest across it.
    let first = largest(spread);
    let second = largest(spread - outer(first) * first.dot(spread * first));
    // Each spread from end to end but `ends` of the points.
    let reach = |axis: Vec3| {
        let mut along: Vec<f32> = points.iter().map(|&p| (p - mean).dot(axis)).collect();
        along.sort_by(f32::total_cmp);
        let share = |s: f32| along[((along.len() - 1) as f32 * s) as usize];
        (share(ends(material)), share(1.0 - ends(material)))
    };
    match material {
        Material::Metal => {
            let third = first
                .cross(second)
                .try_normalize()
                .unwrap_or_else(|| first.any_orthonormal_vector());
            let second = third.cross(first);
            let axes = Mat3::from_cols(first, second, third);
            let [x, y, z] = [first, second, third].map(reach);
            let middle = Vec3::new(x.0 + x.1, y.0 + y.1, z.0 + z.1) * 0.5;
            let half =
                (Vec3::new(x.1 - x.0, y.1 - y.0, z.1 - z.0) * 0.5).max(Vec3::splat(RADIUS[0]));
            Some(Solid::Cuboid {
                centre: mean + axes * middle,
                turn: Quat::from_mat3(&axes),
                half,
            })
        }
        Material::Flesh => {
            let (low, high) = reach(first);
            let (a, b) = (mean + first * low, mean + first * high);
            let mut around: Vec<f32> = points.iter().map(|&p| segment_distance(p, a, b)).collect();
            around.sort_by(f32::total_cmp);
            let radius = around[((around.len() - 1) as f32 * (1.0 - AROUND)) as usize]
                .clamp(RADIUS[0], RADIUS[1]);
            // The caps reach `radius` past each end: drawn in by it, never
            // past the middle.
            let tuck = radius.min((high - low) * 0.5);
            Some(Solid::Capsule {
                ends: [a + first * tuck, b - first * tuck],
                radius,
            })
        }
    }
}

/// A symmetric matrix's axis of largest spread (power iteration).
fn largest(m: Mat3) -> Vec3 {
    let mut axis = Vec3::new(0.577, 0.577, 0.577);
    for _ in 0..32 {
        axis = (m * axis).try_normalize().unwrap_or(Vec3::Z);
    }
    axis
}

fn outer(v: Vec3) -> Mat3 {
    Mat3::from_cols(v * v.x, v * v.y, v * v.z)
}
