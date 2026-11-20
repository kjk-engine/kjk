//! Generated skirt chains, for the robe and cape physics of
//! https://github.com/kjk-engine/roadmap/issues/5.
//!
//! A KOTOR 2 robe hangs from 3 or 4 chains of robe bones. The skin between
//! two chains is only blended between them, so a leg that swings between
//! two chains passes through the robe. A KOTOR 1 Jedi robe has no robe
//! bones at all. Its skirt is the lower part of the legs mesh, skinned to
//! the thighs and the shins like trousers.
//!
//! `generate` gives such a skirt its own ring of chains, made from the
//! model's data when it loads. It adds the chains' bones to the model's
//! nodes, after every node the model already has, and moves the skirt's
//! vertices onto them. The finder (`super::rig`) then finds the new bones
//! as robe skirt bones, like any other.
//!
//! # Which vertices are the skirt
//!
//! - **A model with robe skirt bones** (KOTOR 2's robes). A skirt vertex is
//!   a vertex weighted to a bone of a robe skirt tree. The share of its
//!   weight on those bones moves to the new chains. Its weight on the
//!   pelvis or the belly stays.
//! - **A model without them** (KOTOR 1's Jedi robes and Revan's). A skirt
//!   vertex is a vertex below the hips that is weighted at least
//!   `LEG_WEIGHT` to the thighs and the shins, and that lies far from the
//!   legs. Its distance is measured to the straight lines from the hip to
//!   the knee and from the knee to the ankle. A vertex within `LEG_NEAR` of
//!   a leg stays on the leg. A vertex beyond `LEG_FAR` moves to the chains
//!   entirely. Between the two, its weight moves in part. Trousers hug the
//!   legs, so they give too few skirt vertices to count (`SKIRT_MIN`). A
//!   bulky body or baggy trousers can give many, but only a skirt hangs
//!   across the middle, between the legs and well below the hips, carried
//!   by both legs (`BETWEEN_MIN`).
//!
//! # Where the chains hang
//!
//! The chains hang at `CHAINS` even angles around the pelvis, seen from
//! above. A chain is kept when enough skirt vertices lie near its angle
//! (`SECTOR_MIN`). A robe that is open at the front gets no chain there.
//! Each chain runs from the highest skirt vertex near its angle down to
//! the lowest, in `BONES` bones of equal height. Each joint sits at the
//! skirt's mean distance from the pelvis at its height and angle, so the
//! chain lies on the cloth.
//!
//! # How a vertex follows the chains
//!
//! A skirt vertex follows the two kept chains on either side of its angle,
//! each by how near its angle is, when at most one chain is missing
//! between them. Across a wider gap it follows the nearer chain. Along a
//! chain it follows the bone at its
//! height, blended with the next bone near each joint. A vertex keeps at
//! most 4 bones, the ones with the most weight.
//!
//! The new bones rest at their parent's turn. Each new bone's inverse bind
//! is made from the parent's, so that every vertex rests where it rested
//! before (`Skirt::drift` measures it).

use std::collections::BTreeSet;
use std::f32::consts::{PI, TAU};

use asset_odyssey::model::{Matrix, ModelIr, NodeIr};
use glam::{Mat4, Vec3};

use super::{Kind, NOT_BODIES};
use crate::rig;

/// How many chains could hang around a skirt, at even angles. Two
/// neighbouring chains of a skirt 0.25 m from the pelvis hang about 16 cm
/// apart at the hem.
pub const CHAINS: usize = 10;
/// The bones of each chain, from the waist to the hem.
pub const BONES: usize = 3;
/// A chain is kept when the skirt vertices within one chain's angle of it
/// weigh at least this much. Each vertex weighs its share moved to the
/// chains times how near its angle is.
const SECTOR_MIN: f32 = 6.0;
/// A chain runs from the height above which this share of its skirt
/// vertices' weight lies, down to the height below which the same share
/// lies. A few vertices at the edge of a chain's angle reach far lower,
/// where the skirt is longer beside it.
const EDGE: f32 = 0.03;
/// A vertex weighted at least this much to the thighs and the shins can be
/// a skirt vertex of a model without robe bones.
const LEG_WEIGHT: f32 = 0.5;
/// A vertex nearer than this to a leg's line stays on the leg (metres).
const LEG_NEAR: f32 = 0.08;
/// A vertex farther than this from both legs' lines moves to the chains
/// entirely (metres). 15 cm finds a KOTOR 1 Jedi robe's skirt with about 3%
/// false vertices on trousers, all of them at the hips.
const LEG_FAR: f32 = 0.15;
/// A model without robe bones has a skirt when at least this many vertices
/// move to the chains by half or more.
const SKIRT_MIN: usize = 30;
/// A skirt hangs between the legs at the front or the back, well below the
/// hips, and both legs carry it there. Trousers and bulky bodies have only
/// the inside of each leg there, which its own leg carries. A model
/// without robe bones has a skirt only when at least `BETWEEN_MIN` skirt
/// vertices that both legs carry, each by `BOTH_LEGS` or more, lie within
/// `BETWEEN_WIDTH` of the body's middle plane and `BETWEEN_DROP` or more
/// below the hip joints (metres).
const BETWEEN_MIN: usize = 3;
const BETWEEN_WIDTH: f32 = 0.06;
const BETWEEN_DROP: f32 = 0.25;
const BOTH_LEGS: f32 = 0.2;
const LEGS: [[&str; 3]; 2] = [
    ["lthigh_g", "lshin_g", "lfoot_g"],
    ["rthigh_g", "rshin_g", "rfoot_g"],
];

/// Where the skirt vertices were found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The vertices of a model's robe skirt bones.
    RobeBones,
    /// The vertices of the legs mesh that lie far from the legs.
    Legs,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::RobeBones => "robe bones",
            Source::Legs => "legs",
        }
    }
}

/// One generated chain.
#[derive(Debug, Clone)]
pub struct Chain {
    /// Its angle around the pelvis seen from above (degrees). 0 is the
    /// model's +Y, and 90 its +X.
    pub angle: f32,
    /// Its bones from the waist down, as node indices of the model.
    pub nodes: Vec<usize>,
}

/// What `generate` did to a model.
#[derive(Debug, Clone)]
pub struct Skirt {
    pub source: Source,
    pub chains: Vec<Chain>,
    /// The vertices moved onto the chains, in part or entirely.
    pub vertices: usize,
    /// The largest distance that a moved vertex's rest place moved
    /// (metres). It is near 0 when the skin's inverse binds agree with the
    /// rest pose.
    pub drift: f32,
}

impl Skirt {
    /// One line: where the skirt came from, its chains' angles, the moved
    /// vertices and the drift.
    pub fn describe(&self) -> String {
        let angles: Vec<String> = self
            .chains
            .iter()
            .map(|chain| format!("{:.0}", chain.angle))
            .collect();
        format!(
            "skirt from {}: {} chains of {} bones at {} degrees, {} vertices, drift {:.1} mm",
            self.source.name(),
            self.chains.len(),
            BONES,
            angles.join(" "),
            self.vertices,
            self.drift * 1000.0
        )
    }
}

fn mat4(m: &Matrix) -> Mat4 {
    Mat4::from_cols_array_2d(m).transpose()
}

fn matrix(m: Mat4) -> Matrix {
    m.transpose().to_cols_array_2d()
}

/// A skin vertex of one mesh, as the skirt sees it.
struct SkirtVertex {
    mesh: usize,
    vertex: usize,
    /// Its rest place in the model's space.
    at: Vec3,
    /// The share of its weight that moves to the chains.
    share: f32,
    /// Whether both legs carry it, each by `BOTH_LEGS` or more.
    both_legs: bool,
}

/// The height below which `share` of the weight of `near` lies.
fn height_at(near: &[(&SkirtVertex, f32)], share: f32) -> f32 {
    let mut heights: Vec<(f32, f32)> = near.iter().map(|&(v, weight)| (v.at.z, weight)).collect();
    heights.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f32 = heights.iter().map(|&(_, weight)| weight).sum();
    let mut below = 0.0;
    for &(z, weight) in &heights {
        below += weight;
        if below >= share * total {
            return z;
        }
    }
    heights.last().map_or(0.0, |&(z, _)| z)
}

fn distance_to_segment(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The signed turn from angle `b` to angle `a`, between -PI and PI.
fn turn(a: f32, b: f32) -> f32 {
    (a - b + PI).rem_euclid(TAU) - PI
}

fn smoothstep(from: f32, to: f32, x: f32) -> f32 {
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Where one kept chain hangs at rest, in the model's space.
struct Hang {
    /// Its angle around the pelvis (radians).
    angle: f32,
    joints: Vec<Vec3>,
    /// The heights of its first joint and of the hem below it.
    top: f32,
    hem: f32,
}

/// The chains' bones that a skirt vertex at angle `theta` and height `z`
/// follows, each with its weight. The weights add up to 1. The vertex
/// follows the kept chains on either side of its angle, each by how near
/// its angle is, when at most one chain is missing between them. A KOTOR 1
/// Jedi robe has no chain at the hips, where the skirt lies near the legs,
/// and its cloth there must not tear between front and back. Across a
/// wider gap, such as the open front of a KOTOR 2 robe, the vertex follows
/// the nearer chain. Along a chain, it follows the bone at its height, blended
/// with the next bone near each joint.
fn chain_weights(hangs: &[Hang], chains: &[Chain], theta: f32, z: f32) -> Vec<(usize, f32)> {
    let step = TAU / CHAINS as f32;
    let ahead = |a: f32| turn(a, theta).rem_euclid(TAU);
    let behind = |a: f32| turn(theta, a).rem_euclid(TAU);
    let after = (0..hangs.len())
        .min_by(|&a, &b| ahead(hangs[a].angle).total_cmp(&ahead(hangs[b].angle)))
        .expect("two chains or more");
    let before = (0..hangs.len())
        .min_by(|&a, &b| behind(hangs[a].angle).total_cmp(&behind(hangs[b].angle)))
        .expect("two chains or more");
    let (to_after, to_before) = (ahead(hangs[after].angle), behind(hangs[before].angle));
    let gap = to_after + to_before;
    let sides = if after != before && gap <= step * 2.01 {
        vec![(before, to_after / gap), (after, to_before / gap)]
    } else if to_after < to_before {
        vec![(after, 1.0)]
    } else {
        vec![(before, 1.0)]
    };
    let mut bones = Vec::with_capacity(4);
    for (chain, side) in sides {
        let hang = &hangs[chain];
        let nodes = &chains[chain].nodes;
        let along = ((hang.top - z) / (hang.top - hang.hem)).clamp(0.0, 1.0);
        let c = along * BONES as f32 - 0.5;
        if c <= 0.0 {
            bones.push((nodes[0], side));
        } else if c >= (BONES - 1) as f32 {
            bones.push((nodes[BONES - 1], side));
        } else {
            let i = c.floor() as usize;
            let f = c - i as f32;
            bones.push((nodes[i], side * (1.0 - f)));
            bones.push((nodes[i + 1], side * f));
        }
    }
    bones
}

/// Gives the skirt of `model` its own ring of chains, as the module says,
/// and returns what it did. It returns `None` and leaves the model as it
/// was when the model is not a body, or when it has no skirt.
pub fn generate(model: &mut ModelIr) -> Option<Skirt> {
    let name = model.name.to_ascii_lowercase();
    if NOT_BODIES.contains(&name.as_str()) || !rig::is_humanoid(model) {
        return None;
    }
    let pelvis = model.node_index(asset_odyssey::hooks::PELVIS)?;
    let worlds: Vec<Mat4> = model.nodes.iter().map(|node| mat4(&node.world)).collect();
    let place = |node: usize| worlds[node].w_axis.truncate();
    let found = super::rig(model, &[]);
    let robe: Vec<&super::Tree> = found
        .trees
        .iter()
        .filter(|tree| tree.kind == Kind::RobeSkirt)
        .collect();
    let robe_bones: BTreeSet<usize> = robe
        .iter()
        .flat_map(|tree| tree.points.iter().filter(|point| !point.tip))
        .map(|point| point.node)
        .collect();
    let source = if robe_bones.is_empty() {
        Source::Legs
    } else {
        Source::RobeBones
    };
    let mut legs = Vec::new();
    for side in LEGS {
        let nodes: Option<Vec<usize>> = side.iter().map(|n| model.node_index(n)).collect();
        legs.push(nodes?);
    }
    let leg_bones: BTreeSet<usize> = legs.iter().flat_map(|side| side[..2].to_vec()).collect();
    let side_of = |node: usize| legs.iter().position(|side| side[..2].contains(&node));
    let hips = legs
        .iter()
        .map(|side| place(side[0]).z)
        .fold(f32::MIN, f32::max);
    let segments: Vec<(Vec3, Vec3)> = legs
        .iter()
        .flat_map(|side| {
            [
                (place(side[0]), place(side[1])),
                (place(side[1]), place(side[2])),
            ]
        })
        .collect();

    let mut skirt: Vec<SkirtVertex> = Vec::new();
    for (index, mesh) in model.meshes.iter().enumerate() {
        let Some(skin) = &mesh.skin else { continue };
        let binds: Vec<Mat4> = (skin.joints.iter().zip(&skin.inverse_binds))
            .map(|(&node, bind)| worlds.get(node).copied().unwrap_or(Mat4::IDENTITY) * mat4(bind))
            .collect();
        for (vertex, p) in mesh.positions.iter().enumerate() {
            let p = Vec3::from_array(*p);
            let (slots, weights) = (skin.vertex_joints[vertex], skin.vertex_weights[vertex]);
            let mut at = Vec3::ZERO;
            let mut on_robe = 0.0;
            let mut on_legs = 0.0;
            let mut sides = [0.0f32; 2];
            for (&slot, &weight) in slots.iter().zip(&weights) {
                let slot = usize::from(slot);
                let (Some(&node), Some(bind)) = (skin.joints.get(slot), binds.get(slot)) else {
                    continue;
                };
                at += bind.transform_point3(p) * weight;
                if robe_bones.contains(&node) {
                    on_robe += weight;
                }
                if leg_bones.contains(&node) {
                    on_legs += weight;
                }
                if let Some(side) = side_of(node) {
                    sides[side] += weight;
                }
            }
            let share = match source {
                Source::RobeBones => on_robe,
                Source::Legs if on_legs >= LEG_WEIGHT && at.z < hips => {
                    let away = (segments.iter())
                        .map(|&(a, b)| distance_to_segment(at, a, b))
                        .fold(f32::MAX, f32::min);
                    smoothstep(LEG_NEAR, LEG_FAR, away)
                }
                Source::Legs => 0.0,
            };
            if share > 0.0 {
                skirt.push(SkirtVertex {
                    mesh: index,
                    vertex,
                    at,
                    share,
                    both_legs: sides.iter().all(|&side| side >= BOTH_LEGS),
                });
            }
        }
    }
    let between = |v: &&SkirtVertex| {
        v.both_legs
            && (v.at.x - place(pelvis).x).abs() < BETWEEN_WIDTH
            && v.at.z < hips - BETWEEN_DROP
    };
    if source == Source::Legs
        && (skirt.iter().filter(|v| v.share >= 0.5).count() < SKIRT_MIN
            || skirt.iter().filter(between).count() < BETWEEN_MIN)
    {
        return None;
    }
    if skirt.is_empty() {
        return None;
    }

    let centre = place(pelvis);
    let angle_of = |at: Vec3| (at.x - centre.x).atan2(at.y - centre.y);
    let radius_of = |at: Vec3| (at - centre).truncate().length();
    let step = TAU / CHAINS as f32;
    let mut hangs: Vec<Hang> = Vec::new();
    for k in 0..CHAINS {
        let angle = k as f32 * step;
        let near: Vec<(&SkirtVertex, f32)> = skirt
            .iter()
            .map(|v| {
                (
                    v,
                    (1.0 - turn(angle_of(v.at), angle).abs() / step) * v.share,
                )
            })
            .filter(|&(_, near)| near > 0.0)
            .collect();
        if near.iter().map(|&(_, near)| near).sum::<f32>() < SECTOR_MIN {
            continue;
        }
        let top = height_at(&near, 1.0 - EDGE);
        let hem = height_at(&near, EDGE);
        if top - hem < 1e-3 {
            continue;
        }
        let height = (top - hem) / BONES as f32;
        let joints = (0..BONES)
            .map(|j| {
                let z = top - j as f32 * height;
                let mut band = height / 2.0;
                let radius = loop {
                    let (sum, weight) = near
                        .iter()
                        .filter(|(v, _)| (v.at.z - z).abs() <= band)
                        .fold((0.0, 0.0), |(sum, weight), &(v, near)| {
                            (sum + radius_of(v.at) * near, weight + near)
                        });
                    if weight > 1e-3 {
                        break sum / weight;
                    }
                    band *= 2.0;
                };
                Vec3::new(
                    centre.x + radius * angle.sin(),
                    centre.y + radius * angle.cos(),
                    z,
                )
            })
            .collect();
        hangs.push(Hang {
            angle,
            joints,
            top,
            hem,
        });
    }
    if hangs.len() < 2 {
        return None;
    }

    let parent = robe.first().map_or(pelvis, |tree| tree.attach);
    let mut chains = Vec::with_capacity(hangs.len());
    for (k, hang) in hangs.iter().enumerate() {
        let mut above = parent;
        let mut nodes = Vec::with_capacity(BONES);
        for (j, &joint) in hang.joints.iter().enumerate() {
            let above_world = mat4(&model.nodes[above].world);
            let local = above_world.inverse().transform_point3(joint);
            let animated = model.nodes[above].animated;
            model.nodes.push(NodeIr {
                name: format!("skirt{k}_{j}"),
                parent: Some(above),
                translation: local.to_array(),
                rotation: [0.0, 0.0, 0.0, 1.0],
                world: matrix(above_world * Mat4::from_translation(local)),
                animated,
            });
            above = model.nodes.len() - 1;
            nodes.push(above);
        }
        chains.push(Chain {
            angle: hang.angle.to_degrees(),
            nodes,
        });
    }
    let worlds: Vec<Mat4> = model.nodes.iter().map(|node| mat4(&node.world)).collect();

    // Each skin's place of its vertices in the rest pose, through the
    // chains' parent: the parent's inverse bind when the skin has one, or
    // the one of the joint that carries the most skirt weight.
    let mut shared: Vec<Option<Mat4>> = vec![None; model.meshes.len()];
    for v in &skirt {
        if shared[v.mesh].is_some() {
            continue;
        }
        let Some(skin) = &model.meshes[v.mesh].skin else {
            continue;
        };
        let mut carried = vec![0.0f32; skin.joints.len()];
        for other in skirt.iter().filter(|other| other.mesh == v.mesh) {
            let (slots, weights) = (
                skin.vertex_joints[other.vertex],
                skin.vertex_weights[other.vertex],
            );
            for (&slot, &weight) in slots.iter().zip(&weights) {
                if let Some(c) = carried.get_mut(usize::from(slot)) {
                    *c += weight;
                }
            }
        }
        let reference = skin
            .joints
            .iter()
            .position(|&node| node == parent)
            .or_else(|| {
                (0..skin.joints.len())
                    .filter(|&slot| !robe_bones.contains(&skin.joints[slot]))
                    .max_by(|&a, &b| carried[a].total_cmp(&carried[b]))
            });
        shared[v.mesh] =
            reference.map(|slot| worlds[skin.joints[slot]] * mat4(&skin.inverse_binds[slot]));
    }

    let mut drift: f32 = 0.0;
    let mut vertices = 0;
    for v in &skirt {
        let Some(shared) = shared[v.mesh] else {
            continue;
        };
        let bones = chain_weights(&hangs, &chains, angle_of(v.at), v.at.z);
        let mesh = &mut model.meshes[v.mesh];
        let mesh_world = worlds[mesh.node];
        let Some(skin) = mesh.skin.as_mut() else {
            continue;
        };
        let (slots, weights) = (skin.vertex_joints[v.vertex], skin.vertex_weights[v.vertex]);
        let mut weighted: Vec<(usize, f32)> = Vec::with_capacity(8);
        for (&slot, &weight) in slots.iter().zip(&weights) {
            let Some(&node) = skin.joints.get(usize::from(slot)) else {
                continue;
            };
            if weight <= 0.0 || robe_bones.contains(&node) {
                continue;
            }
            let keep = match source {
                Source::RobeBones => weight,
                Source::Legs => weight * (1.0 - v.share),
            };
            weighted.push((node, keep));
        }
        weighted.extend(bones.iter().map(|&(node, w)| (node, w * v.share)));
        let mut merged: Vec<(usize, f32)> = Vec::with_capacity(8);
        for (node, weight) in weighted {
            match merged.iter_mut().find(|(n, _)| *n == node) {
                Some((_, w)) => *w += weight,
                None => merged.push((node, weight)),
            }
        }
        merged.sort_by(|a, b| b.1.total_cmp(&a.1));
        merged.truncate(4);
        let total: f32 = merged.iter().map(|(_, w)| w).sum();
        if total <= 0.0 {
            continue;
        }
        let mut new_slots = [0u16; 4];
        let mut new_weights = [0f32; 4];
        let mut fits = true;
        for (k, &(node, weight)) in merged.iter().enumerate() {
            let slot = match skin.joints.iter().position(|&n| n == node) {
                Some(slot) => slot,
                None => {
                    skin.joints.push(node);
                    skin.inverse_binds
                        .push(matrix(worlds[node].inverse() * shared));
                    skin.rest_inverse_binds
                        .push(matrix(worlds[node].inverse() * mesh_world));
                    skin.joints.len() - 1
                }
            };
            let Ok(slot) = u16::try_from(slot) else {
                fits = false;
                break;
            };
            new_slots[k] = slot;
            new_weights[k] = weight / total;
        }
        if !fits {
            continue;
        }
        let p = Vec3::from_array(mesh.positions[v.vertex]);
        let now: Vec3 = (0..4)
            .map(|k| {
                let slot = usize::from(new_slots[k]);
                (worlds[skin.joints[slot]] * mat4(&skin.inverse_binds[slot])).transform_point3(p)
                    * new_weights[k]
            })
            .sum();
        drift = drift.max(now.distance(v.at));
        skin.vertex_joints[v.vertex] = new_slots;
        skin.vertex_weights[v.vertex] = new_weights;
        vertices += 1;
    }
    Some(Skirt {
        source,
        chains,
        vertices,
        drift,
    })
}
