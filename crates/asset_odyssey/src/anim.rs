//! Animation clips as keyframe tracks per node, read the way reone reads
//! them:
//!
//! - Position keys (controller 8) are offsets: the runtime adds
//!   `animation scale × key` to the node's rest position. xoreos adds the rest
//!   position first and scales the sum; the two agree when the scale is 1.
//! - Orientation keys (controller 20): two columns is one packed word per
//!   row (x, y, z in 11, 11 and 10 bits as `1 − v / 1023` (or 511), w the
//!   negative root of the rest, or 0 with x, y, z normalised when they
//!   already reach length 1); four columns are x, y, z, w.
//! - Bézier rows keep only their value (the first of three).
//! - Every other controller (an emitter's birth rate or colours, a light's
//!   colour) is kept as its first row, by node: what a pose clip sets
//!   (`charrec_light`'s alignment backdrops are clips of length 0).

use fmt_odyssey::mdl::{self, Controller, Node};

use crate::model::{Matrix, ModelIr};

use mdl::{CONTROLLER_ORIENTATION as ORIENTATION, CONTROLLER_POSITION as POSITION};

#[derive(Debug, Clone)]
pub struct NodeTrack {
    pub node: String,
    /// (time, offset from the rest position).
    pub positions: Vec<(f32, [f32; 3])>,
    /// (time, rotation as x, y, z, w).
    pub orientations: Vec<(f32, [f32; 4])>,
}

/// A node's controllers other than position and orientation, at their
/// first row: (controller number, values).
#[derive(Debug, Clone)]
pub struct NodeSettings {
    pub node: String,
    pub keys: Vec<(u32, Vec<f32>)>,
}

#[derive(Debug, Clone)]
pub struct AnimationIr {
    pub name: String,
    pub length: f32,
    pub transition: f32,
    /// The node the clip applies under; `None` when it is the model itself
    /// (reone keeps the root only when it differs from the model name).
    pub root: Option<String>,
    pub tracks: Vec<NodeTrack>,
    pub settings: Vec<NodeSettings>,
    pub events: Vec<(f32, String)>,
}

fn rows(controller: &Controller) -> impl Iterator<Item = (f32, &[f32])> {
    let columns = usize::from(controller.columns);
    let stride = if controller.bezier {
        columns * 3
    } else {
        columns
    };
    controller
        .times
        .iter()
        .enumerate()
        .filter_map(move |(row, &time)| {
            let at = row * stride;
            controller
                .values
                .get(at..at + columns)
                .map(|values| (time, values))
        })
}

fn track(node: &Node) -> Option<NodeTrack> {
    let mut positions = Vec::new();
    let mut orientations = Vec::new();
    for controller in &node.controllers {
        match (controller.kind, controller.columns) {
            (POSITION, 3) => {
                positions.extend(rows(controller).map(|(t, v)| (t, [v[0], v[1], v[2]])));
            }
            (ORIENTATION, 2) => {
                // One packed word per row, kept in the float's bits.
                orientations.extend(
                    controller
                        .times
                        .iter()
                        .zip(&controller.values)
                        .map(|(&t, v)| (t, mdl::unpack_quaternion(v.to_bits()))),
                );
            }
            (ORIENTATION, 4) => {
                orientations.extend(rows(controller).map(|(t, v)| (t, [v[0], v[1], v[2], v[3]])));
            }
            _ => {}
        }
    }
    (!positions.is_empty() || !orientations.is_empty()).then(|| NodeTrack {
        node: node.name.clone(),
        positions,
        orientations,
    })
}

fn settings(node: &Node) -> Option<NodeSettings> {
    let keys: Vec<(u32, Vec<f32>)> = node
        .controllers
        .iter()
        .filter(|controller| !matches!(controller.kind, POSITION | ORIENTATION))
        .filter_map(|controller| Some((controller.kind, rows(controller).next()?.1.to_vec())))
        .collect();
    (!keys.is_empty()).then(|| NodeSettings {
        node: node.name.to_ascii_lowercase(),
        keys,
    })
}

fn collect(node: &Node, tracks: &mut Vec<NodeTrack>, set: &mut Vec<NodeSettings>) {
    tracks.extend(track(node));
    set.extend(settings(node));
    for child in &node.children {
        collect(child, tracks, set);
    }
}

impl AnimationIr {
    pub fn from_animation(animation: &mdl::Animation, model_name: &str) -> AnimationIr {
        let (mut tracks, mut settings) = (Vec::new(), Vec::new());
        collect(&animation.root, &mut tracks, &mut settings);
        let root = animation.root_name.trim().to_ascii_lowercase();
        AnimationIr {
            name: animation.name.to_ascii_lowercase(),
            length: animation.length,
            transition: animation.transition,
            root: (!root.is_empty() && !root.eq_ignore_ascii_case(model_name)).then_some(root),
            tracks,
            settings,
            events: animation.events.clone(),
        }
    }
}

impl AnimationIr {
    /// The track posing each of `model`'s nodes (by the model's node
    /// index): a track names its node without case, and takes it only where
    /// the model marks the node animated and it is at or under the clip's
    /// root.
    pub fn bind(&self, model: &ModelIr) -> Vec<Option<usize>> {
        let mut bound = vec![None; model.nodes.len()];
        for (track, keys) in self.tracks.iter().enumerate() {
            let Some(node) = model.node_index(&keys.node) else {
                continue;
            };
            let under_root = self
                .root
                .as_deref()
                .is_none_or(|root| model.under(node, root));
            if model.nodes[node].animated && under_root {
                bound[node] = Some(track);
            }
        }
        bound
    }

    /// A node's place and turn relative to its parent `at` seconds in,
    /// as track `track` keys it: its rest place plus `scale` times the
    /// position key, the orientation key's turn; what the track does not
    /// key stays at rest.
    pub fn local(
        &self,
        track: Option<usize>,
        (rest_place, rest_turn): ([f32; 3], [f32; 4]),
        scale: f32,
        at: f32,
    ) -> ([f32; 3], [f32; 4]) {
        let track = track.and_then(|track| self.tracks.get(track));
        let place = track
            .and_then(|track| sample_position(&track.positions, at))
            .map_or(rest_place, |key| {
                [0, 1, 2].map(|i| rest_place[i] + scale * key[i])
            });
        let turn = track
            .and_then(|track| sample_orientation(&track.orientations, at))
            .unwrap_or(rest_turn);
        (place, turn)
    }
}

/// Node `node` of `model` in the model's space as `clip` (its `bind` of
/// the model, `bound`) poses it `at` seconds in: each node from the root
/// down at its `local` pose.
pub fn posed(
    model: &ModelIr,
    (clip, bound): (&AnimationIr, &[Option<usize>]),
    scale: f32,
    node: usize,
    at: f32,
) -> Matrix {
    let mut chain = vec![node];
    while let Some(parent) = chain.last().and_then(|&n| model.nodes.get(n)?.parent) {
        chain.push(parent);
    }
    chain
        .iter()
        .rev()
        .fold(crate::model::IDENTITY, |placed, &n| {
            let (place, turn) = clip.local(
                bound.get(n).copied().flatten(),
                (model.nodes[n].translation, model.nodes[n].rotation),
                scale,
                at,
            );
            crate::model::multiply(&placed, &crate::model::matrix(place, normalised(turn)))
        })
}

/// Every node of `model` in the model's space as `clip` poses it `at`
/// seconds in (`posed`, for all nodes at once: a node follows its parent,
/// which `ModelIr` lists before it).
pub fn pose(model: &ModelIr, clip: &AnimationIr, scale: f32, at: f32) -> Vec<Matrix> {
    let bound = clip.bind(model);
    let mut worlds: Vec<Matrix> = Vec::with_capacity(model.nodes.len());
    for (n, node) in model.nodes.iter().enumerate() {
        let (place, turn) = clip.local(bound[n], (node.translation, node.rotation), scale, at);
        let local = crate::model::matrix(place, normalised(turn));
        let parent = node.parent.map_or(crate::model::IDENTITY, |p| worlds[p]);
        worlds.push(crate::model::multiply(&parent, &local));
    }
    worlds
}

fn normalised(q: [f32; 4]) -> [f32; 4] {
    let length = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    if length > f32::EPSILON {
        q.map(|v| v / length)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// Linear sampling between keys, clamped at the ends (xoreos and reone
/// both hold the first and last key).
pub fn sample_position(keys: &[(f32, [f32; 3])], time: f32) -> Option<[f32; 3]> {
    let (first, last) = (keys.first()?, keys.last()?);
    if time <= first.0 {
        return Some(first.1);
    }
    if time >= last.0 {
        return Some(last.1);
    }
    let next = keys.iter().position(|(t, _)| *t > time)?;
    let (a, b) = (keys[next - 1], keys[next]);
    let f = (time - a.0) / (b.0 - a.0).max(f32::EPSILON);
    Some([0, 1, 2].map(|i| a.1[i] + (b.1[i] - a.1[i]) * f))
}

/// Normalised linear interpolation along the shorter arc (xoreos flips the
/// second key past 90°; reone slerps); both clamp at the ends.
pub fn sample_orientation(keys: &[(f32, [f32; 4])], time: f32) -> Option<[f32; 4]> {
    let (first, last) = (keys.first()?, keys.last()?);
    if time <= first.0 {
        return Some(first.1);
    }
    if time >= last.0 {
        return Some(last.1);
    }
    let next = keys.iter().position(|(t, _)| *t > time)?;
    let (a, b) = (keys[next - 1], keys[next]);
    let f = (time - a.0) / (b.0 - a.0).max(f32::EPSILON);
    let dot: f32 = (0..4).map(|i| a.1[i] * b.1[i]).sum();
    let sign = if dot < 0.0 { -1.0 } else { 1.0 };
    let q = [0, 1, 2, 3].map(|i| a.1[i] * (1.0 - f) + sign * b.1[i] * f);
    let length = q
        .iter()
        .map(|v| v * v)
        .sum::<f32>()
        .sqrt()
        .max(f32::EPSILON);
    Some(q.map(|v| v / length))
}
