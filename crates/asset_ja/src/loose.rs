//! The loose bones of a KOTOR body. A loose bone exists only to move a
//! garment, such as a robe skirt, a cape, a hood, a sleeve or a flap. The
//! body's own skeleton is not loose. The loose bones are the nodes that the
//! humanoid map (`rig::mapped_kotor`) leaves out and that carry a garment.
//! They are what the robe and cape physics of
//! https://github.com/kjk-engine/roadmap/issues/5 moves.
//!
//! `rig` finds them in one model and turns them into points for the
//! solver. Each loose bone gives a point at its joint, and each last bone
//! gives a tip point. The tip of a chain continues the chain's last
//! segment. The tip of a bone that hangs alone points at the vertices it
//! carries. `rig` also links the points of neighbouring chains, and it
//! gives every point its physics values (`physics`).
//!
//! A node is a loose bone when all of these hold.
//!
//! 1. The model passes the humanoid test (`rig::is_humanoid`).
//! 2. The node is not mapped, and it carries garment vertices. It is a
//!    joint of a skin with at least one vertex of weight 0.25 or more, or
//!    it is a rigid mesh (not a danglymesh) in a chain of such meshes. The
//!    cutscene capes of Malak and Bastila in KOTOR 1 are such chains. A
//!    rigid piece alone, or one hung on a skinned bone (Talia's
//!    `frontflappiece`), rides on its parent.
//! 3. No mapped node lies below it, and a mapped node lies above it. The
//!    first mapped node above is its anchor.
//!
//! The loose bones then form trees, each hanging from its anchor. Whether
//! a tree is kept depends on the anchor.
//!
//! - **The trunk** (`pelvis_g`, `torso_g`, `torsoupr_g`). The tree needs at
//!   least two bones. Every garment that hangs from the trunk in both games
//!   is a chain or a tree of 2 to 27 bones. The only bone that hangs alone
//!   from the trunk is `breastbone`, which moves the chest's own skin. It is
//!   in 10 KOTOR 1 bodies, and in 29 KOTOR 2 bodies and 2 menu scenes.
//! - **An arm or a leg** (a collar, a bicep, a forearm, a thigh or a shin).
//!   Every tree is kept. These are the sleeves, Revan's robe sides,
//!   Bastila's thigh flaps and the Mandalorian's shoulder plates.
//! - **The neck or the head.** Only the hoods in `HOODS` are kept. The skin
//!   and the skeleton do not tell a hood from the face, the Rodian's
//!   antennae or the Trandoshan's eyelids. All of them hang from the head
//!   and move skin that the head moves too.
//! - **A hand or a foot.** No tree is kept. The fingers and the toes hang
//!   there.
//!
//! The models in `NOT_BODIES` have no loose bones at all.
//!
//! # What a caller can rely on
//!
//! - Every node index is an index into the `ModelIr` given to `rig`. A
//!   caller maps it to the same loaded model's nodes.
//! - The output depends only on the model and the clips. Trees follow the
//!   node order of their roots, points follow a depth-first walk from the
//!   root, and each point comes after its parent.
//! - Positions and lengths are those of the model's rest pose, in metres.
//!   The clips only change the physics values.
//! - `Rig::flat` numbers the points of all trees in one list, in which each
//!   parent still comes before its children.
//! - `Rig::bones` lists the nodes that the solver turns. An animation must
//!   put them back to their target every frame before the solver reads
//!   them.

use std::collections::{BTreeMap, BTreeSet};

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::model::{Matrix, ModelIr};
use glam::{Mat3, Quat, Vec3};

use crate::rig;

pub mod physics;
pub mod skirt;

pub use physics::Physics;

/// The roots of the hoods that hang from the head, reviewed by hand. Revan
/// wears `hood_g`, `hoodl_g` and `hoodr_g` in both games. KOTOR 2's Kreia
/// bodies and the Handmaiden's robes wear `lhood_g`, `mhood_g` (with
/// `mfhood_g` below it) and `rhood_g`.
pub const HOODS: [&str; 6] = [
    "hood_g", "hoodl_g", "hoodr_g", "lhood_g", "mhood_g", "rhood_g",
];

/// Models that pass the humanoid test but are not bodies that wear
/// anything. `pmbtest` is a test body left in both games. Its `lforearm`
/// is a second forearm bone, not a sleeve.
pub const NOT_BODIES: [&str; 1] = ["pmbtest"];

/// A skin weight at or above this makes a node carry the vertex.
const CARRIES: f32 = 0.25;
/// A skin weight at or above this makes a vertex count for a tip's length.
const DOMINANT: f32 = 0.5;
/// The tip's length is the distance to the farthest vertex of weight
/// `DOMINANT` or more, or the farthest reach along the tip's direction of a
/// vertex of weight `CARRIES` or more, whichever is longer. It is held
/// between these (metres).
const TIP_MIN: f32 = 0.05;
const TIP_MAX: f32 = 0.6;
/// The clip whose first keys are the hang pose of the loose bones.
const HANG_CLIP: &str = "pause1";
/// A tree on the upper chest is a cape when its longest chain is at least
/// this long (metres). Bastila's back flaps reach 0.3 m, and the capes 1 m
/// or more.
const CAPE_LENGTH: f32 = 0.5;
/// Two strands are joined by the skin when this many vertices each carry a
/// weight of `JOINED_WEIGHT` or more on a bone of each strand.
const JOINED_VERTICES: usize = 10;
const JOINED_WEIGHT: f32 = 0.2;
/// Two strands are near when their mean distance is at most this many
/// times the median distance from a strand to its nearest strand, and
/// their mean angles about the pelvis differ by less than `NEAR_ANGLE`.
const NEAR_SCALE: f32 = 1.5;
const NEAR_ANGLE: f32 = 120.0;
/// Where along two strands their distance is measured (shares of length).
const SAMPLES: [f32; 5] = [0.2, 0.4, 0.6, 0.8, 1.0];
/// A robe skirt strand that no other rule links is linked to the strand
/// next to it on each side around the pelvis, when that strand is at most
/// this far away (degrees). The front piece of a KOTOR 2 robe shares no
/// skin with the back chains, and they hang 113 to 129 degrees from it. A
/// front piece and a back piece with nothing between them hang about 180
/// degrees apart, across the legs, and stay unlinked.
const AROUND_ANGLE: f32 = 150.0;

/// A clip is a flourish when it turns a hood bone at least this far from its
/// rest pose (degrees). The clips that pull a hood up or down turn it 139
/// to 156 degrees, and no other clip of either game turns a hood more than
/// 33 degrees.
const FLOURISH_TURN: f32 = 90.0;

/// The kind of garment a tree moves. Each kind has its own physics values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// Hangs from the upper chest or the shoulders, with a chain of at
    /// least `CAPE_LENGTH`.
    Cape,
    /// One of two or more trees that hang from the pelvis or the belly
    /// around the legs.
    RobeSkirt,
    Hood,
    /// A single tree on the pelvis or the belly, a short piece on the upper
    /// chest, or a piece on a thigh or a shin.
    Flap,
    /// Hangs from a forearm.
    Sleeve,
    /// Hangs from a collar or a bicep: the Mandalorian's shoulder plates.
    Other,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Cape => "cape",
            Kind::RobeSkirt => "robe skirt",
            Kind::Hood => "hood",
            Kind::Flap => "flap",
            Kind::Sleeve => "sleeve",
            Kind::Other => "other",
        }
    }
}

/// A model's loose trees and the links between them, in its rest pose.
#[derive(Debug, Clone, Default)]
pub struct Rig {
    pub trees: Vec<Tree>,
    pub links: Vec<Link>,
}

/// One tree of loose bones, hanging from its anchor.
#[derive(Debug, Clone)]
pub struct Tree {
    pub kind: Kind,
    /// The first mapped node above the tree. It decides the kind.
    pub anchor: usize,
    /// The node the root bone hangs on: its parent node. The root point's
    /// offset is in this node's space. It is the anchor, or a node between
    /// the anchor and the root that carries nothing.
    pub attach: usize,
    /// The tree's points, each after its parent. The first is the root's
    /// joint.
    pub points: Vec<Point>,
}

/// A point of a tree: a loose bone's joint, or the tip after a last bone.
#[derive(Debug, Clone)]
pub struct Point {
    /// The bone whose joint this is, or the last bone for a tip.
    pub node: usize,
    pub tip: bool,
    /// The point this one hangs from, in `Tree::points`. A root has none.
    /// It hangs on `Tree::attach`.
    pub parent: Option<usize>,
    /// Where the point rests from its parent, in the space of the parent
    /// point's bone. For a joint, this is its bone's rest translation in its
    /// parent node's space, so a root's offset is in `Tree::attach`'s
    /// space. For a tip, it is in the last bone's own space.
    pub offset: [f32; 3],
    /// Where the point rests in the model's space.
    pub rest: [f32; 3],
    /// The bone that turns to aim at this point. It is the parent point's
    /// bone, or the last bone for a tip, so it follows from `parent` and
    /// `node`. A root has none.
    pub aims: Option<usize>,
    /// The rest length of the chain from the root to this point (metres).
    pub along: f32,
    /// The nodes whose bones move the same vertices as this point's bone,
    /// each with a weight of `JOINED_WEIGHT` or more, the bone included. A
    /// KOTOR 2 robe bone reaches the pelvis and the belly. The bone of a
    /// rigid mesh reaches only itself.
    pub reach: Vec<usize>,
    pub physics: Physics,
}

impl Point {
    pub fn pinned(&self) -> bool {
        self.parent.is_none()
    }
}

/// The reason two points are linked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cause {
    /// Two children of one bone. The link keeps the branch's spread, so
    /// that one turn of the bone fits all its children.
    Branch,
    /// Two strands of one tree.
    Root,
    /// At least `JOINED_VERTICES` vertices are weighted to both strands.
    Skin,
    /// The strands hang close together on the same side of the body.
    Near,
    /// A robe skirt strand that no other rule links, and the strand next to
    /// it on one side around the pelvis, at most `AROUND_ANGLE` away.
    Around,
}

impl Cause {
    pub fn name(self) -> &'static str {
        match self {
            Cause::Branch => "branch",
            Cause::Root => "root",
            Cause::Skin => "skin",
            Cause::Near => "near",
            Cause::Around => "around",
        }
    }
}

/// A distance constraint between two points of neighbouring strands. Its
/// rest length is the distance between the two points' rest positions.
#[derive(Debug, Clone)]
pub struct Link {
    /// The tree and the point of each end. `Rig::flat` turns them into one
    /// number.
    pub a: (usize, usize),
    pub b: (usize, usize),
    pub cause: Cause,
    /// The XPBD compliance of the link (metres per newton).
    pub compliance: f32,
}

/// Where the first mapped node above a tree sits on the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor {
    /// The pelvis or the belly.
    LowTrunk,
    /// The upper chest.
    Chest,
    Head,
    Arm,
    Forearm,
    Leg,
    Extremity,
}

fn anchor_of(name: &str) -> Anchor {
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "pelvis_g" | "torso_g" => Anchor::LowTrunk,
        "torsoupr_g" => Anchor::Chest,
        "neck_g" | "head_g" => Anchor::Head,
        "rforearm_g" | "lforearm_g" => Anchor::Forearm,
        "rcollar_g" | "lcollar_g" | "rbicep_g" | "lbicep_g" => Anchor::Arm,
        "rthigh_g" | "lthigh_g" | "rshin_g" | "lshin_g" => Anchor::Leg,
        _ => Anchor::Extremity,
    }
}

impl Anchor {
    fn trunk(self) -> bool {
        matches!(self, Anchor::LowTrunk | Anchor::Chest)
    }
}

fn place(m: &Matrix) -> Vec3 {
    Vec3::new(m[0][3], m[1][3], m[2][3])
}

fn turn(m: &Matrix) -> Mat3 {
    Mat3::from_cols(
        Vec3::new(m[0][0], m[1][0], m[2][0]),
        Vec3::new(m[0][1], m[1][1], m[2][1]),
        Vec3::new(m[0][2], m[1][2], m[2][2]),
    )
}

fn apply(m: &Matrix, p: Vec3) -> Vec3 {
    turn(m) * p + place(m)
}

/// What the meshes of a model put on each node.
struct Carried {
    /// Skin vertices of weight `CARRIES` or more.
    skin: Vec<usize>,
    /// Vertices of rigid meshes (not danglymeshes) on the node.
    rigid: Vec<usize>,
    /// Skin weights summed, plus one per rigid vertex: how much mesh the
    /// node moves.
    load: Vec<f32>,
    /// The distance to the farthest vertex of weight `DOMINANT` or more,
    /// in the node's own space (metres).
    reach: Vec<f32>,
    /// The sum and the count of the vertices of weight `DOMINANT` or more,
    /// in the node's own space.
    dominant_sum: Vec<(Vec3, usize)>,
    /// The vertices of weight `CARRIES` or more, in the node's own space.
    held: Vec<Vec<Vec3>>,
    /// For each skin vertex weighted `JOINED_WEIGHT` or more on two or more
    /// nodes, those nodes.
    joined: Vec<Vec<usize>>,
}

fn carried(model: &ModelIr) -> Carried {
    let n = model.nodes.len();
    let mut c = Carried {
        skin: vec![0; n],
        rigid: vec![0; n],
        load: vec![0.0; n],
        reach: vec![0.0; n],
        dominant_sum: vec![(Vec3::ZERO, 0); n],
        held: vec![Vec::new(); n],
        joined: Vec::new(),
    };
    for mesh in &model.meshes {
        let Some(skin) = &mesh.skin else {
            if mesh.dangly.is_none() && mesh.node < n {
                let node = mesh.node;
                c.rigid[node] += mesh.positions.len();
                c.load[node] += mesh.positions.len() as f32;
                for p in &mesh.positions {
                    let p = Vec3::from_array(*p);
                    c.reach[node] = c.reach[node].max(p.length());
                    c.held[node].push(p);
                    c.dominant_sum[node].0 += p;
                    c.dominant_sum[node].1 += 1;
                }
            }
            continue;
        };
        for (vertex, p) in mesh.positions.iter().enumerate() {
            let p = Vec3::from_array(*p);
            let (slots, weights) = (skin.vertex_joints[vertex], skin.vertex_weights[vertex]);
            let mut on = Vec::with_capacity(4);
            for (slot, &weight) in slots.iter().zip(&weights) {
                let slot = usize::from(*slot);
                let (Some(&node), Some(bind)) =
                    (skin.joints.get(slot), skin.inverse_binds.get(slot))
                else {
                    continue;
                };
                if weight <= 0.0 || node >= n {
                    continue;
                }
                c.load[node] += weight;
                let local = apply(bind, p);
                if weight >= CARRIES {
                    c.skin[node] += 1;
                    c.held[node].push(local);
                }
                if weight >= DOMINANT {
                    c.reach[node] = c.reach[node].max(local.length());
                    c.dominant_sum[node].0 += local;
                    c.dominant_sum[node].1 += 1;
                }
                if weight >= JOINED_WEIGHT && !on.contains(&node) {
                    on.push(node);
                }
            }
            if on.len() >= 2 {
                c.joined.push(on);
            }
        }
    }
    c
}

/// The trees of loose bones of `model`, before they become points: each
/// tree's anchor and its bones, parents first.
fn trees(model: &ModelIr, carried: &Carried) -> Vec<(usize, Vec<usize>)> {
    let nodes = &model.nodes;
    let mapped: Vec<bool> = nodes.iter().map(|n| rig::mapped_kotor(&n.name)).collect();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            children[parent].push(i);
        }
    }
    let mapped_below = |node: usize| {
        let mut stack = children[node].clone();
        while let Some(next) = stack.pop() {
            if mapped[next] {
                return true;
            }
            stack.extend(&children[next]);
        }
        false
    };
    let anchor = |node: usize| {
        let mut up = nodes[node].parent;
        while let Some(at) = up {
            if mapped[at] {
                return Some(at);
            }
            up = nodes[at].parent;
        }
        None
    };
    let mut loose: BTreeSet<usize> = (0..nodes.len())
        .filter(|&i| !mapped[i])
        .filter(|&i| carried.skin[i] > 0 || carried.rigid[i] > 0)
        .filter(|&i| !mapped_below(i) && anchor(i).is_some())
        .collect();
    let rigid_alone: Vec<usize> = loose
        .iter()
        .copied()
        .filter(|&i| carried.skin[i] == 0)
        .filter(|&i| {
            let rigid = |j: &usize| loose.contains(j) && carried.skin[*j] == 0;
            let parent = nodes[i].parent.is_some_and(|p| rigid(&p));
            let child = children[i].iter().any(rigid);
            !parent && !child
        })
        .collect();
    for i in rigid_alone {
        loose.remove(&i);
    }
    let mut found = Vec::new();
    for &root in &loose {
        if nodes[root].parent.is_some_and(|p| loose.contains(&p)) {
            continue;
        }
        let mut bones = vec![root];
        let mut at = 0;
        while at < bones.len() {
            let here = bones[at];
            bones.extend(children[here].iter().filter(|c| loose.contains(c)));
            at += 1;
        }
        let Some(hung) = anchor(root) else { continue };
        let keep = match anchor_of(&nodes[hung].name) {
            Anchor::LowTrunk | Anchor::Chest => bones.len() >= 2,
            Anchor::Head => HOODS
                .iter()
                .any(|hood| hood.eq_ignore_ascii_case(&nodes[root].name)),
            Anchor::Arm | Anchor::Forearm | Anchor::Leg => true,
            Anchor::Extremity => false,
        };
        if keep {
            found.push((hung, bones));
        }
    }
    found
}

/// How far (radians) the clips typically turn each node of `model` from
/// its hang pose: the median, over the clips that key the node, of the
/// largest turn in each clip. A few clips (sitting, lying, cutscenes) fold
/// a robe far more than a walk or a fight would, so the median is taken,
/// not the largest. The hang pose is the node's first key in the
/// first clip named `pause1`, or its rest rotation when no `pause1` keys
/// it. The supermodels' rest poses do not always match the body's (a
/// sleeve can rest half a turn away), so the angle is not taken from the
/// rest.
fn keyed_turns(model: &ModelIr, clips: &[&AnimationIr]) -> Vec<f32> {
    let mut hang: Vec<Quat> = model
        .nodes
        .iter()
        .map(|node| Quat::from_array(node.rotation).normalize())
        .collect();
    if let Some(pause) = clips
        .iter()
        .find(|clip| clip.name.eq_ignore_ascii_case(HANG_CLIP))
    {
        for track in &pause.tracks {
            if let (Some(node), Some((_, key))) =
                (model.node_index(&track.node), track.orientations.first())
            {
                let key = Quat::from_array(*key);
                if key.length_squared() > 1e-6 {
                    hang[node] = key.normalize();
                }
            }
        }
    }
    let mut swings: Vec<Vec<f32>> = vec![Vec::new(); model.nodes.len()];
    for clip in clips {
        for track in &clip.tracks {
            let Some(node) = model.node_index(&track.node) else {
                continue;
            };
            let swing = track
                .orientations
                .iter()
                .map(|(_, key)| Quat::from_array(*key))
                .filter(|key| key.length_squared() > 1e-6)
                .map(|key| hang[node].angle_between(key.normalize()))
                .filter(|angle| angle.is_finite())
                .fold(0.0_f32, f32::max);
            swings[node].push(swing);
        }
    }
    swings
        .into_iter()
        .map(|mut swing| {
            swing.sort_by(f32::total_cmp);
            swing.get(swing.len() / 2).copied().unwrap_or(0.0)
        })
        .collect()
}

/// A strand: the points of one tree from its root to one tip.
struct Strand {
    tree: usize,
    points: Vec<usize>,
}

impl Strand {
    fn tip<'a>(&self, trees: &'a [Tree]) -> &'a Point {
        &trees[self.tree].points[*self.points.last().expect("a strand has points")]
    }

    /// How far along the strand `point` rests, as a share of its length.
    fn share(&self, trees: &[Tree], point: usize) -> f32 {
        trees[self.tree].points[point].along / self.tip(trees).along.max(1e-6)
    }
}

/// The loose trees of `model`, as points with their physics values, and the
/// links between neighbouring strands. `clips` are the clips that play on
/// the model, its own and those of every model of its supermodel chain.
/// They tell how far the old hand-keyed animation turned each loose bone.
/// With fewer clips, only the maximum distances can come out smaller.
pub fn rig(model: &ModelIr, clips: &[&AnimationIr]) -> Rig {
    let name = model.name.to_ascii_lowercase();
    if NOT_BODIES.contains(&name.as_str()) || !rig::is_humanoid(model) {
        return Rig::default();
    }
    let carried = carried(model);
    let found = trees(model, &carried);
    if found.is_empty() {
        return Rig::default();
    }
    let reach = |bone: usize| -> Vec<usize> {
        let mut reach: BTreeSet<usize> = carried
            .joined
            .iter()
            .filter(|on| on.contains(&bone))
            .flatten()
            .copied()
            .collect();
        reach.insert(bone);
        reach.into_iter().collect()
    };
    let nodes = &model.nodes;
    let worlds: Vec<Vec3> = nodes.iter().map(|n| place(&n.world)).collect();
    let pelvis = model
        .node_index(asset_odyssey::hooks::PELVIS)
        .map_or(Vec3::ZERO, |p| worlds[p]);
    let keyed = keyed_turns(model, clips);

    let mut out = Rig::default();
    let mut strands: Vec<Strand> = Vec::new();
    let mut inputs: Vec<Vec<physics::Input>> = Vec::new();
    let low_trunk_trees = found
        .iter()
        .filter(|(anchor, _)| anchor_of(&nodes[*anchor].name) == Anchor::LowTrunk)
        .count();
    for (anchor, bones) in &found {
        let mut points: Vec<Point> = Vec::new();
        let mut index_of: BTreeMap<usize, usize> = BTreeMap::new();
        let mut loads: Vec<f32> = Vec::new();
        let mut path_of: Vec<Vec<usize>> = Vec::new();
        let mut stack = vec![bones[0]];
        while let Some(bone) = stack.pop() {
            let node = &nodes[bone];
            let parent = node.parent.and_then(|p| index_of.get(&p).copied());
            let here = worlds[bone];
            let along = parent.map_or(0.0, |p| {
                points[p].along + here.distance(Vec3::from(points[p].rest))
            });
            let kids: Vec<usize> = bones
                .iter()
                .copied()
                .filter(|&c| nodes[c].parent == Some(bone))
                .collect();
            let mut path = parent.map_or_else(Vec::new, |p| path_of[p].clone());
            path.push(bone);
            index_of.insert(bone, points.len());
            points.push(Point {
                node: bone,
                tip: false,
                parent,
                offset: node.translation,
                rest: here.to_array(),
                aims: parent.map(|p| points[p].node),
                along,
                reach: reach(bone),
                physics: Physics::default(),
            });
            loads.push(carried.load[bone] / 2.0);
            if let Some(p) = parent {
                let parent_kids = bones
                    .iter()
                    .filter(|&&c| nodes[c].parent == Some(points[p].node))
                    .count()
                    .max(1) as f32;
                loads[index_of[&bone]] += carried.load[points[p].node] / 2.0 / parent_kids;
            }
            path_of.push(path.clone());
            if kids.is_empty() {
                let world = &node.world;
                let held = &carried.held[bone];
                let (sum, count) = if carried.dominant_sum[bone].1 > 0 {
                    carried.dominant_sum[bone]
                } else {
                    (held.iter().sum(), held.len())
                };
                let toward = if count > 0 {
                    (sum / count as f32).try_normalize()
                } else {
                    None
                };
                let from = node.parent.map_or(here, |p| worlds[p]);
                let onward = (turn(world).transpose() * (here - from)).try_normalize();
                let direction = if parent.is_some() {
                    onward.or(toward)
                } else {
                    toward.or(onward)
                }
                .unwrap_or(Vec3::NEG_Z);
                let onward_reach = held.iter().map(|p| p.dot(direction)).fold(0.0, f32::max);
                let length = carried.reach[bone]
                    .max(onward_reach)
                    .clamp(TIP_MIN, TIP_MAX);
                let local = direction * length;
                let rest = apply(world, local);
                let head = index_of[&bone];
                points.push(Point {
                    node: bone,
                    tip: true,
                    parent: Some(head),
                    offset: local.to_array(),
                    rest: rest.to_array(),
                    aims: Some(bone),
                    along: along + length,
                    reach: reach(bone),
                    physics: Physics::default(),
                });
                loads.push(carried.load[bone] / 2.0);
                path_of.push(path);
            }
            stack.extend(kids.iter().rev());
        }
        let longest = points.iter().map(|p| p.along).fold(0.0, f32::max);
        let kind = kind_of(anchor_of(&nodes[*anchor].name), longest, low_trunk_trees);
        let tree = out.trees.len();
        let free: Vec<f32> = loads
            .iter()
            .zip(&points)
            .filter(|(_, p)| !p.pinned())
            .map(|(l, _)| *l)
            .collect();
        let mean_load = free.iter().sum::<f32>() / free.len().max(1) as f32;
        let mut tree_inputs = Vec::with_capacity(points.len());
        for (i, point) in points.iter().enumerate() {
            let at = Vec3::from(point.rest);
            let keyed_reach: f32 = path_of[i]
                .iter()
                .filter(|&&bone| point.tip || bone != point.node)
                .map(|&bone| 2.0 * at.distance(worlds[bone]) * (keyed[bone] / 2.0).sin())
                .sum();
            tree_inputs.push(physics::Input {
                pinned: point.pinned(),
                along: point.along,
                longest,
                load: if mean_load > 0.0 {
                    loads[i] / mean_load
                } else {
                    1.0
                },
                keyed_reach,
            });
        }
        for (i, point) in points.iter().enumerate() {
            if point.tip {
                let mut strand = Vec::new();
                let mut at = Some(i);
                while let Some(p) = at {
                    strand.push(p);
                    at = points[p].parent;
                }
                strand.reverse();
                strands.push(Strand {
                    tree,
                    points: strand,
                });
            }
        }
        inputs.push(tree_inputs);
        out.trees.push(Tree {
            kind,
            anchor: *anchor,
            attach: nodes[bones[0]].parent.unwrap_or(*anchor),
            points,
        });
    }
    for (tree, inputs) in out.trees.iter_mut().zip(&inputs) {
        for (point, input) in tree.points.iter_mut().zip(inputs) {
            point.physics = physics::values(tree.kind, input);
        }
    }
    let anchors: Vec<Anchor> = out
        .trees
        .iter()
        .map(|tree| anchor_of(&nodes[tree.anchor].name))
        .collect();
    out.links = links(&out.trees, &strands, &carried, pelvis, &anchors);
    out
}

fn kind_of(anchor: Anchor, longest: f32, low_trunk_trees: usize) -> Kind {
    match anchor {
        Anchor::Head => Kind::Hood,
        Anchor::Forearm => Kind::Sleeve,
        Anchor::Arm | Anchor::Extremity => Kind::Other,
        Anchor::Leg => Kind::Flap,
        Anchor::Chest if longest >= CAPE_LENGTH => Kind::Cape,
        Anchor::Chest => Kind::Flap,
        Anchor::LowTrunk if low_trunk_trees >= 2 => Kind::RobeSkirt,
        Anchor::LowTrunk => Kind::Flap,
    }
}

/// The point of a strand at `share` of its length.
fn along_strand(trees: &[Tree], strand: &Strand, share: f32) -> Vec3 {
    let points = &trees[strand.tree].points;
    let want = share * strand.tip(trees).along;
    for pair in strand.points.windows(2) {
        let (a, b) = (&points[pair[0]], &points[pair[1]]);
        if b.along >= want {
            let span = (b.along - a.along).max(1e-6);
            let t = ((want - a.along) / span).clamp(0.0, 1.0);
            return Vec3::from(a.rest).lerp(Vec3::from(b.rest), t);
        }
    }
    Vec3::from(strand.tip(trees).rest)
}

fn links(
    trees: &[Tree],
    strands: &[Strand],
    carried: &Carried,
    pelvis: Vec3,
    anchors: &[Anchor],
) -> Vec<Link> {
    let mut out = branch_links(trees);
    out.extend(strand_links(trees, strands, carried, pelvis, anchors));
    out
}

/// A link between each pair of children of a bone with several children.
fn branch_links(trees: &[Tree]) -> Vec<Link> {
    let mut out = Vec::new();
    for (t, tree) in trees.iter().enumerate() {
        for (p, point) in tree.points.iter().enumerate() {
            if point.tip {
                continue;
            }
            let kids: Vec<usize> = (0..tree.points.len())
                .filter(|&c| tree.points[c].parent == Some(p) && !tree.points[c].tip)
                .collect();
            for (i, &a) in kids.iter().enumerate() {
                for &b in &kids[i + 1..] {
                    out.push(Link {
                        a: (t, a),
                        b: (t, b),
                        cause: Cause::Branch,
                        compliance: physics::link_compliance(tree.kind, Cause::Branch),
                    });
                }
            }
        }
    }
    out
}

/// Links between the points of neighbouring strands. Two strands can be
/// neighbours when their trees are of one kind, and both hang from the
/// trunk or both from the same anchor. The median distance of the "near"
/// test is taken over the strands of one kind. A robe skirt strand that
/// none of the other rules links is then linked to its neighbours around
/// the pelvis (`AROUND_ANGLE`).
fn strand_links(
    trees: &[Tree],
    strands: &[Strand],
    carried: &Carried,
    pelvis: Vec3,
    anchors: &[Anchor],
) -> Vec<Link> {
    let mean_distance = |a: &Strand, b: &Strand| {
        SAMPLES
            .iter()
            .map(|&s| along_strand(trees, a, s).distance(along_strand(trees, b, s)))
            .sum::<f32>()
            / SAMPLES.len() as f32
    };
    let angle = |s: &Strand| {
        let (sum_sin, sum_cos) = SAMPLES.iter().fold((0.0, 0.0), |(ss, cc), &share| {
            let at = along_strand(trees, s, share) - pelvis;
            let a = at.x.atan2(at.y);
            (ss + a.sin(), cc + a.cos())
        });
        f32::atan2(sum_sin, sum_cos).to_degrees()
    };
    let bones = |s: &Strand| -> BTreeSet<usize> {
        s.points
            .iter()
            .map(|&p| &trees[s.tree].points[p])
            .filter(|p| !p.tip)
            .map(|p| p.node)
            .collect()
    };
    let linkable = |a: &Strand, b: &Strand| {
        trees[a.tree].kind == trees[b.tree].kind
            && (trees[a.tree].anchor == trees[b.tree].anchor
                || (anchors[a.tree].trunk() && anchors[b.tree].trunk()))
    };
    let mut nearest: BTreeMap<Kind, Vec<f32>> = BTreeMap::new();
    for (i, a) in strands.iter().enumerate() {
        let best = strands
            .iter()
            .enumerate()
            .filter(|&(j, b)| j != i && linkable(a, b))
            .map(|(_, b)| mean_distance(a, b))
            .fold(f32::MAX, f32::min);
        if best < f32::MAX {
            nearest.entry(trees[a.tree].kind).or_default().push(best);
        }
    }
    let median: BTreeMap<Kind, f32> = nearest
        .into_iter()
        .map(|(kind, mut distances)| {
            distances.sort_by(f32::total_cmp);
            (kind, distances[distances.len() / 2])
        })
        .collect();
    let mut pairs: Vec<(usize, usize, Cause)> = Vec::new();
    for (i, a) in strands.iter().enumerate() {
        for (j, b) in strands.iter().enumerate().skip(i + 1) {
            if !linkable(a, b) {
                continue;
            }
            let (own_a, own_b) = (bones(a), bones(b));
            let cause = if a.tree == b.tree {
                Some(Cause::Root)
            } else {
                let joined = carried
                    .joined
                    .iter()
                    .filter(|on| on.iter().any(|n| own_a.contains(n)))
                    .filter(|on| on.iter().any(|n| own_b.contains(n)))
                    .count();
                let gap = (angle(a) - angle(b)).rem_euclid(360.0);
                let gap = gap.min(360.0 - gap);
                if joined >= JOINED_VERTICES {
                    Some(Cause::Skin)
                } else if mean_distance(a, b) <= NEAR_SCALE * median[&trees[a.tree].kind]
                    && gap < NEAR_ANGLE
                {
                    Some(Cause::Near)
                } else {
                    None
                }
            };
            if let Some(cause) = cause {
                pairs.push((i, j, cause));
            }
        }
    }
    let lonely: Vec<usize> = (0..strands.len())
        .filter(|&i| trees[strands[i].tree].kind == Kind::RobeSkirt)
        .filter(|&i| !pairs.iter().any(|&(a, b, _)| a == i || b == i))
        .collect();
    for i in lonely {
        let a = &strands[i];
        let turn = |j: usize| (angle(&strands[j]) - angle(a) + 180.0).rem_euclid(360.0) - 180.0;
        let others: Vec<usize> = (0..strands.len())
            .filter(|&j| strands[j].tree != a.tree && linkable(a, &strands[j]))
            .collect();
        let left = others.iter().copied().filter(|&j| turn(j) > 0.0);
        let right = others.iter().copied().filter(|&j| turn(j) <= 0.0);
        let left = left.min_by(|&x, &y| turn(x).total_cmp(&turn(y)));
        let right = right.max_by(|&x, &y| turn(x).total_cmp(&turn(y)));
        for j in left.into_iter().chain(right) {
            let pair = (i.min(j), i.max(j), Cause::Around);
            if turn(j).abs() <= AROUND_ANGLE && !pairs.contains(&pair) {
                pairs.push(pair);
            }
        }
    }
    let mut out: Vec<Link> = Vec::new();
    let mut seen: BTreeSet<((usize, usize), (usize, usize))> = BTreeSet::new();
    for (i, j, cause) in pairs {
        let (a, b) = (&strands[i], &strands[j]);
        let kind = trees[a.tree].kind;
        let compliance = physics::link_compliance(kind, cause);
        let only = |s: &Strand, other: &Strand| -> Vec<usize> {
            s.points
                .iter()
                .copied()
                .filter(|p| s.tree != other.tree || !other.points.contains(p))
                .filter(|&p| !trees[s.tree].points[p].pinned())
                .collect()
        };
        let (pa, pb) = (only(a, b), only(b, a));
        if pa.is_empty() || pb.is_empty() {
            continue;
        }
        // Each point of one strand is paired with the point of the other
        // that sits at the nearest share of its length, both ways.
        let partner = |from: &Strand, p: usize, to: &Strand, among: &[usize]| {
            let want = from.share(trees, p);
            *among
                .iter()
                .min_by(|&&x, &&y| {
                    (to.share(trees, x) - want)
                        .abs()
                        .total_cmp(&(to.share(trees, y) - want).abs())
                })
                .expect("not empty")
        };
        let points = pa
            .iter()
            .map(|&p| ((a.tree, p), (b.tree, partner(a, p, b, &pb))))
            .chain(
                pb.iter()
                    .map(|&q| ((a.tree, partner(b, q, a, &pa)), (b.tree, q))),
            );
        for (x, y) in points {
            let key = (x.min(y), x.max(y));
            if x == y || !seen.insert(key) {
                continue;
            }
            out.push(Link {
                a: key.0,
                b: key.1,
                cause,
                compliance,
            });
        }
    }
    out
}

impl Rig {
    /// The number of a point when the points of all trees are put in one
    /// list, tree after tree.
    pub fn flat(&self, (tree, point): (usize, usize)) -> usize {
        self.trees[..tree]
            .iter()
            .map(|tree| tree.points.len())
            .sum::<usize>()
            + point
    }

    /// The loose bones: the node of every joint point, in tree order.
    pub fn bones(&self) -> Vec<usize> {
        self.trees
            .iter()
            .flat_map(|tree| tree.points.iter().filter(|point| !point.tip))
            .map(|point| point.node)
            .collect()
    }

    /// Whether `clip` is a flourish on `model`, whose rig this is. A
    /// flourish is a clip whose keys turn a hood bone `FLOURISH_TURN` or more
    /// from its rest pose. These are the clips that pull a hood up or down,
    /// such as Kreia's and the Handmaiden's `onhood` and `offhood`. The keys
    /// of every other clip on a loose bone only imitate physics.
    pub fn flourish(&self, model: &ModelIr, clip: &AnimationIr) -> bool {
        let hoods: BTreeSet<usize> = self
            .trees
            .iter()
            .filter(|tree| tree.kind == Kind::Hood)
            .flat_map(|tree| tree.points.iter().filter(|point| !point.tip))
            .map(|point| point.node)
            .collect();
        if hoods.is_empty() {
            return false;
        }
        let most = FLOURISH_TURN.to_radians();
        clip.tracks.iter().any(|track| {
            let Some(node) = model.node_index(&track.node).filter(|n| hoods.contains(n)) else {
                return false;
            };
            let rest = Quat::from_array(model.nodes[node].rotation).normalize();
            track
                .orientations
                .iter()
                .map(|(_, key)| Quat::from_array(*key))
                .filter(|key| key.length_squared() > 1e-6)
                .any(|key| rest.angle_between(key.normalize()) >= most)
        })
    }

    /// The rig as text, one line per tree and one for the links, with the
    /// node names of `model`. A tree reads as its kind, its anchor and its
    /// bones from the root, a branch in brackets, and the tip length of each
    /// last bone in centimetres.
    pub fn describe(&self, model: &ModelIr) -> String {
        let name = |node: usize| model.nodes[node].name.to_ascii_lowercase();
        let mut text = String::new();
        for tree in &self.trees {
            text += &format!(
                "  {} from {}: {}\n",
                tree.kind.name(),
                name(tree.anchor),
                chain(tree, 0, &name)
            );
        }
        let mut groups: BTreeMap<(Cause, String), usize> = BTreeMap::new();
        for link in &self.links {
            let label = |(t, p): (usize, usize)| name(self.trees[t].points[p].node);
            let root = |t: usize| name(self.trees[t].points[0].node);
            let key = match link.cause {
                Cause::Branch => format!("{}|{}", label(link.a), label(link.b)),
                Cause::Root => root(link.a.0),
                Cause::Skin | Cause::Near | Cause::Around => {
                    format!("{}~{}", root(link.a.0), root(link.b.0))
                }
            };
            *groups.entry((link.cause, key)).or_default() += 1;
        }
        if !groups.is_empty() {
            let links: Vec<String> = groups
                .iter()
                .map(|((cause, key), count)| format!("{} {key} x{count}", cause.name()))
                .collect();
            text += &format!("  links: {}\n", links.join(", "));
        }
        text
    }
}

/// The bones from `point` down, as `describe` writes them.
fn chain(tree: &Tree, point: usize, name: &impl Fn(usize) -> String) -> String {
    let here = &tree.points[point];
    let kids: Vec<usize> = (0..tree.points.len())
        .filter(|&c| tree.points[c].parent == Some(point))
        .collect();
    if let [only] = kids[..]
        && tree.points[only].tip
    {
        let length = Vec3::from(tree.points[only].offset).length();
        return format!("{} ({:.0} cm)", name(here.node), length * 100.0);
    }
    let below: Vec<String> = kids.iter().map(|&c| chain(tree, c, name)).collect();
    match below.len() {
        0 => name(here.node),
        1 => format!("{} > {}", name(here.node), below[0]),
        _ => format!("{} > [{}]", name(here.node), below.join(" | ")),
    }
}
