//! A ragdoll for any skeleton, without a plan. Beasts and droids use it,
//! because each of them has a skeleton of its own.
//!
//! Every node that carries a real piece of the mesh becomes a body. A flesh
//! body is a capsule. A metal body is a box. Each solid is laid along the
//! spread of its own vertices. The root is the body with the most body
//! below it.
//!
//! A body hangs on the nearest body above it in the skeleton. The joint is
//! where its node hangs on its parent node. A body with no body above it
//! hangs on the body, among those already hung, that its skin shares most
//! with. Two skins share a vertex when the nodes of both bodies move it.
//! The joint is at the middle of the shared vertices. A body whose skin
//! shares no vertex with a hung body hangs on the nearest one instead. Its
//! joint is where the two skins come closest.
//!
//! A node that carries mesh but has no body at or above it rides on the
//! body its skin shares most with, or else on the nearest body.

use glam::{Mat3, Quat, Vec3};

use crate::body::{
    Group, Joint, JointKind, Material, Part, Rider, Skeleton, Solid, Template, Vertex, owners,
    segment_distance, skin_by_part,
};

/// A node needs this many vertices, spread at least this far from it
/// (metres), to be a body of its own: fingers and greebles ride on their
/// parent's.
const LEAST_VERTICES: usize = 12;
const LEAST_SPREAD: f32 = 0.07;
/// The most bodies a ragdoll can have. A krayt dragon has 23 pieces that
/// are worth a body. Seven are along its back, four are in its tail, and
/// three are in each of its four legs. The end of its tail and its tongue
/// compete with its paws for the last places.
fn most_parts(material: Material) -> usize {
    match material {
        Material::Flesh => 28,
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
/// A vertex of a hanging piece is part of its seam with the piece it hangs
/// on when it lies no farther from that piece than the closest vertex
/// does, plus this share of the hanging piece's thickness. The joint is at
/// the middle of the seam.
const SEAM: f32 = 0.5;
/// No part weighs less than this share of the average part.
const LIGHTEST: f32 = 0.25;
/// The smallest and the largest radius of a capsule (metres). The largest
/// radius used to be 0.6 m. Only the four largest beasts of KOTOR 1 have
/// parts with a radius over 0.6 m. They are the krayt dragon, the rancor,
/// the bantha and the firaxan shark. The largest radius is 1.98 m, on the
/// krayt dragon's chest.
const RADIUS: [f32; 2] = [0.025, 2.0];

impl Template {
    /// A template fitted to `skeleton` alone, of `material`.
    pub fn auto(skeleton: &Skeleton, material: Material) -> Result<Template, String> {
        let nodes = skeleton.nodes;
        let mut by_node: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
        for (index, vertex) in skeleton.skin.iter().enumerate() {
            if let Some(indices) = by_node.get_mut(vertex.node) {
                indices.push(index);
            }
        }
        for (node, child) in nodes.iter().enumerate() {
            if let Some(parent) = child.parent {
                children[parent].push(node);
            }
        }
        let above = |mut node: usize| {
            let mut chain = vec![node];
            while let Some(parent) = nodes[node].parent {
                chain.push(parent);
                node = parent;
            }
            chain
        };
        let own = |node: usize| by_node[node].iter().map(|&index| skeleton.skin[index].at);
        // The points of `node`'s piece, as `skin_by_part` would give them with
        // `node` chosen last. They keep the skin's order, because the fitted
        // solid changes with the order its points are summed in.
        let piece_of = |node: usize, chosen: &[usize]| {
            let mut indices: Vec<usize> = Vec::new();
            let mut stack = vec![node];
            while let Some(at) = stack.pop() {
                indices.extend(&by_node[at]);
                stack.extend(children[at].iter().filter(|child| !chosen.contains(child)));
            }
            indices.sort_unstable();
            indices
                .into_iter()
                .map(|index| skeleton.skin[index].at)
                .collect::<Vec<Vec3>>()
        };
        // The nodes that carry a piece of their own become bodies, the
        // largest pieces first. A node's piece is its own mesh and all the
        // mesh that hangs below it, down to the next node that is a body. A
        // paw's claws therefore count for the paw until the claws are bodies
        // of their own. A forearm is chosen before a tongue.
        let candidates: Vec<usize> = (0..nodes.len())
            .filter(|&node| {
                by_node[node].len() >= LEAST_VERTICES
                    && own(node).any(|p| p.distance(nodes[node].rest.translation) >= LEAST_SPREAD)
            })
            .collect();
        let mut chosen: Vec<usize> = Vec::new();
        // A candidate's volume changes only when a node below it is chosen.
        let mut volumes: Vec<Option<Option<f32>>> = vec![None; nodes.len()];
        while chosen.len() < most_parts(material) {
            for &node in &candidates {
                if volumes[node].is_none() && !chosen.contains(&node) {
                    volumes[node] =
                        Some(fit(&piece_of(node, &chosen), material).map(|solid| solid.volume()));
                }
            }
            let largest = candidates
                .iter()
                .filter(|node| !chosen.contains(node))
                .filter_map(|&node| Some((volumes[node]??, node)))
                .max_by(|a, b| a.0.total_cmp(&b.0).then(b.1.cmp(&a.1)));
            match largest {
                Some((_, node)) => {
                    chosen.push(node);
                    for ancestor in above(node).into_iter().skip(1) {
                        volumes[ancestor] = None;
                    }
                }
                None => break,
            }
        }
        if chosen.is_empty() {
            return Err("no node carries enough of the mesh".into());
        }
        // A node's mesh with no body at or above it rides on the body its
        // skin shares most with, or else on the nearest body.
        let mut skin = skin_by_part(skeleton, &chosen);
        let mut owner = owners(skeleton, &chosen);
        let mut riders: Vec<(usize, usize)> = Vec::new();
        let unowned: Vec<usize> = (0..nodes.len())
            .filter(|&node| !by_node[node].is_empty() && owner[node].is_none())
            .collect();
        for node in unowned {
            owner[node] = Some(chosen.len());
            let mine: Vec<Vec3> = own(node).collect();
            let sharing = (0..chosen.len())
                .filter_map(|part| Some((shared(skeleton, &owner, chosen.len(), part)?.0, part)))
                .max_by(|a, b| a.0.total_cmp(&b.0));
            let nearest = || {
                (0..chosen.len())
                    .filter_map(|part| Some((seam(&mine, &skin[part], 0.0)?.0, part)))
                    .min_by(|a, b| a.0.total_cmp(&b.0))
            };
            let Some((_, part)) = sharing.or_else(nearest) else {
                owner[node] = None;
                continue;
            };
            owner[node] = Some(part);
            skin[part].extend(mine);
            riders.push((node, part));
        }
        let position = |node: usize| chosen.iter().position(|&n| n == node).unwrap_or(0);
        let piece = |node: usize| &skin[position(node)];
        let fits: Vec<Solid> = chosen
            .iter()
            .zip(&skin)
            .map(|(&node, points)| {
                fit(points, material).unwrap_or(Solid::Capsule {
                    ends: [nodes[node].rest.translation; 2],
                    radius: RADIUS[0] * 2.0,
                })
            })
            .collect();
        let solid = |node: usize| fits[position(node)];
        // The root is the body with the largest volume of bodies at and
        // below its node.
        let below = |node: usize| {
            chosen
                .iter()
                .filter(|&&body| above(body).contains(&node))
                .map(|&body| solid(body).volume())
                .sum::<f32>()
        };
        let root = chosen
            .iter()
            .copied()
            .max_by(|&a, &b| below(a).total_cmp(&below(b)).then(b.cmp(&a)))
            .unwrap_or(chosen[0]);
        let hung_on = |node: usize| {
            above(node)
                .into_iter()
                .skip(1)
                .find(|ancestor| chosen.contains(ancestor))
        };
        // Two bodies meet at the middle of the vertices their skins share.
        // If they share none, they meet where their skins come closest.
        let meet = |node: usize, held: usize| {
            shared(skeleton, &owner, position(node), position(held)).or_else(|| {
                let within = thickness(solid(node)) * SEAM;
                seam(piece(node), piece(held), within).map(|(apart, middle)| (-apart, middle))
            })
        };
        let whole = |node: usize| {
            skeleton
                .skin
                .iter()
                .map(|vertex| share(vertex, &owner, position(node)))
                .sum::<f32>()
                .max(f32::EPSILON)
        };
        // The bodies are placed parents first. A body hangs on the nearest
        // body above it once that body is placed. The joint is where its
        // node hangs on its parent node. A node can rest farther from its
        // skin than its piece is long. The bantha's horns rest at its hips,
        // for example. Such a joint is where the two skins meet instead.
        // A body with no body above it hangs on the placed body its skin
        // shares most with, at the middle of the shared vertices. A body
        // whose skin shares no vertex with a placed body hangs on the placed
        // body it comes closest to.
        let mut placed: Vec<(usize, Option<usize>, Vec3)> = vec![(root, None, Vec3::ZERO)];
        while placed.len() < chosen.len() {
            let is_placed = |node: usize| placed.iter().position(|&(n, _, _)| n == node);
            let waiting = chosen
                .iter()
                .copied()
                .filter(|&node| is_placed(node).is_none());
            let hung = waiting.clone().find_map(|node| {
                let held = hung_on(node)?;
                let parent = is_placed(held)?;
                let rest = nodes[node].rest.translation;
                let near = piece(node)
                    .iter()
                    .map(|p| p.distance(rest))
                    .min_by(f32::total_cmp)
                    .unwrap_or(0.0);
                let anchor = match near > length(solid(node)) {
                    true => meet(node, held).map_or(rest, |(_, middle)| middle),
                    false => rest,
                };
                Some((node, parent, anchor))
            });
            let (node, parent, anchor) = match hung {
                Some(hung) => hung,
                None => waiting
                    .filter(|&node| hung_on(node).is_none())
                    .flat_map(|node| {
                        placed
                            .iter()
                            .enumerate()
                            .filter_map(move |(parent, &(p, _, _))| {
                                let (how, middle) = meet(node, p)?;
                                let how = if how > 0.0 { how / whole(node) } else { how };
                                Some((how, node, parent, middle))
                            })
                    })
                    .max_by(|a, b| a.0.total_cmp(&b.0))
                    .map(|(_, node, parent, anchor)| (node, parent, anchor))
                    .ok_or("a body hangs on nothing")?,
            };
            placed.push((node, Some(parent), anchor));
        }
        let (cone, twist) = material.give();
        let mut parts: Vec<Part> = Vec::with_capacity(placed.len());
        for &(node, parent, anchor) in &placed {
            let rest = nodes[node].rest;
            let fit = solid(node);
            let joint = parent.map(|parent| {
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
        // A part with little or no mesh still weighs at least a quarter of
        // the average part, because joints between bodies of very unequal
        // mass shake.
        let least = parts.iter().map(|part| part.mass).sum::<f32>() / parts.len() as f32 * LIGHTEST;
        for part in &mut parts {
            part.mass = part.mass.max(least);
        }
        let mass: f32 = parts.iter().map(|part| part.mass).sum();
        for part in &mut parts {
            part.mass /= mass.max(1e-6);
        }
        let riders = riders
            .iter()
            .map(|&(node, body)| {
                let part = placed
                    .iter()
                    .position(|&(n, _, _)| n == chosen[body])
                    .unwrap_or(0);
                Rider {
                    node,
                    part,
                    on_part: parts[part].rest.inverse().then(nodes[node].rest),
                }
            })
            .collect();
        let mut template = Template::new(parts, mass, material);
        template.riders = riders;
        Ok(template)
    }
}

/// The share of `vertex` that the nodes of `part` move. `owner` holds each
/// node's part.
fn share(vertex: &Vertex, owner: &[Option<usize>], part: usize) -> f32 {
    vertex
        .bones
        .iter()
        .filter(|&&(node, _)| owner.get(node).copied().flatten() == Some(part))
        .map(|&(_, weight)| weight)
        .sum()
}

/// The skin that parts `a` and `b` share. The first value is how much of
/// the skin both parts move. The second is the middle of that skin. It is
/// None when the two parts share no vertex.
fn shared(skeleton: &Skeleton, owner: &[Option<usize>], a: usize, b: usize) -> Option<(f32, Vec3)> {
    let (mut weight, mut sum) = (0.0, Vec3::ZERO);
    for vertex in skeleton.skin {
        let both = share(vertex, owner, a).min(share(vertex, owner, b));
        weight += both;
        sum += vertex.at * both;
    }
    (weight > 0.0).then(|| (weight, sum / weight))
}

/// How long a solid is, end to end.
fn length(solid: Solid) -> f32 {
    match solid {
        Solid::Capsule {
            ends: [a, b],
            radius,
        } => a.distance(b) + 2.0 * radius,
        Solid::Cuboid { half, .. } => half.max_element() * 2.0,
    }
}

/// How far a solid reaches from its segment.
fn thickness(solid: Solid) -> f32 {
    match solid {
        Solid::Capsule { radius, .. } => radius,
        Solid::Cuboid { half, .. } => half.min_element(),
    }
}

/// Where the skin of `hanging` meets the skin of `held`, in model space.
/// The first value is the closest distance between the two skins. The
/// second is the middle of the seam. The seam is the vertices of `hanging`
/// that lie no farther from `held` than that closest distance plus
/// `within`. It is None when either skin is empty.
fn seam(hanging: &[Vec3], held: &[Vec3], within: f32) -> Option<(f32, Vec3)> {
    let nearest: Vec<(f32, Vec3)> = hanging
        .iter()
        .filter_map(|&a| {
            held.iter()
                .map(|&b| (a.distance(b), (a + b) * 0.5))
                .min_by(|x, y| x.0.total_cmp(&y.0))
        })
        .collect();
    let apart = nearest.iter().map(|n| n.0).min_by(f32::total_cmp)?;
    let near: Vec<Vec3> = nearest
        .iter()
        .filter(|n| n.0 <= apart + within)
        .map(|n| n.1)
        .collect();
    Some((apart, near.iter().sum::<Vec3>() / near.len() as f32))
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
        let mut share = |s: f32| ranked(&mut along, s);
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
            let radius = ranked(&mut around, 1.0 - AROUND).clamp(RADIUS[0], RADIUS[1]);
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

/// The value a sort of `values` would place `share` of the way along it.
fn ranked(values: &mut [f32], share: f32) -> f32 {
    let at = ((values.len() - 1) as f32 * share) as usize;
    *values.select_nth_unstable_by(at, f32::total_cmp).1
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
