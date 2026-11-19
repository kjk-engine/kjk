//! The loose bones of a KOTOR body: the bones that exist only to move a
//! garment (a robe skirt, a cape, a hood, a sleeve, a flap), as opposed to
//! the body's own skeleton. They are the nodes that the humanoid map
//! (`rig::mapped_kotor`) leaves out and that carry a garment. The plan is
//! `docs/research/2026-11-19-loose-parts-plan.md` (steps L1 and L2).
//!
//! `rig` finds them in one model and turns them into points for the
//! solver: a point at each loose bone's joint, and a tip point after each
//! last bone. The tip of a chain continues the chain's last segment. The
//! tip of a bone that hangs alone points at the vertices it carries. It also links the points of neighbouring chains, and gives
//! every point its physics values (`physics`). The result is plain data in
//! the model's rest pose.
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
//! - **The trunk** (`pelvis_g`, `torso_g`, `torsoupr_g`): the tree needs at
//!   least two bones. Every garment that hangs from the trunk in both games
//!   is a chain or a tree of 2 to 27 bones. A bone that hangs alone from
//!   the trunk moves the body's own skin: it is `breastbone`, in 10 KOTOR 1
//!   and 29 KOTOR 2 bodies.
//! - **An arm or a leg** (a collar, a bicep, a forearm, a thigh or a shin):
//!   every tree is kept. These are the sleeves, Revan's robe sides,
//!   Bastila's thigh flaps and the Mandalorian's shoulder plates.
//! - **The neck or the head**: only the hoods in `HOODS`. The skin and the
//!   skeleton do not tell a hood from the face, the Rodian's antennae or
//!   the Trandoshan's eyelids, since all of them hang from the head and
//!   move skin that the head moves too.
//! - **A hand or a foot**: never. The fingers and the toes hang there.
//!
//! The models in `NOT_BODIES` have no loose bones at all.

use std::collections::{BTreeMap, BTreeSet};

use asset_odyssey::anim::AnimationIr;
use asset_odyssey::model::{Matrix, ModelIr};
use glam::{Mat3, Quat, Vec3};

use crate::rig;

pub mod physics;

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
/// Two strands are joined by the skin when this many vertices carry a
/// weight of `JOINED_WEIGHT` or more on a bone of each.
const JOINED_VERTICES: usize = 10;
const JOINED_WEIGHT: f32 = 0.2;
/// Two strands are near when their mean distance is at most this many
/// times the median distance from a strand to its nearest strand, and
/// their mean angles about the pelvis differ by less than `NEAR_ANGLE`.
const NEAR_SCALE: f32 = 1.5;
const NEAR_ANGLE: f32 = 120.0;
/// Where along two strands their distance is measured (shares of length).
const SAMPLES: [f32; 5] = [0.2, 0.4, 0.6, 0.8, 1.0];

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
    /// The mapped node the tree hangs from.
    pub anchor: usize,
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
    /// The point this one hangs from, in `Tree::points`. `None` for a root,
    /// which hangs from its bone's parent node.
    pub parent: Option<usize>,
    /// Where the point rests from its parent, in the space of the parent's
    /// bone. For a joint this is its bone's rest translation in its parent
    /// node's space. For a tip it is in the last bone's own space.
    pub offset: [f32; 3],
    /// Where the point rests in the model's space.
    pub rest: [f32; 3],
    /// The bone that turns to aim at this point: the parent point's bone,
    /// or the last bone for a tip. `None` for a root.
    pub aims: Option<usize>,
    /// The rest length of the chain from the root to this point (metres).
    pub along: f32,
    pub physics: Physics,
}

impl Point {
    pub fn pinned(&self) -> bool {
        self.parent.is_none()
    }
}

/// Why two points are linked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Why {
    /// Two children of one bone. The link keeps the branch's spread, so
    /// that one turn of the bone fits all its children.
    Branch,
    /// Two strands of one tree.
    Root,
    /// At least `JOINED_VERTICES` vertices are weighted to both strands.
    Skin,
    /// The strands hang close together on the same side of the body.
    Near,
}

impl Why {
    pub fn name(self) -> &'static str {
        match self {
            Why::Branch => "branch",
            Why::Root => "root",
            Why::Skin => "skin",
            Why::Near => "near",
        }
    }
}

/// A distance constraint between two points of neighbouring strands.
#[derive(Debug, Clone)]
pub struct Link {
    /// (tree, point) of each end.
    pub a: (usize, usize),
    pub b: (usize, usize),
    /// The rest distance (metres).
    pub length: f32,
    pub why: Why,
    /// The XPBD compliance of the link (metres per newton).
    pub compliance: f32,
}

/// Where the first mapped node above a tree sits on the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Anchor {
    Trunk,
    Head,
    Arm,
    Forearm,
    Leg,
    Extremity,
}

fn anchor_of(name: &str) -> Anchor {
    let name = name.to_ascii_lowercase();
    match name.as_str() {
        "pelvis_g" | "torso_g" | "torsoupr_g" => Anchor::Trunk,
        "neck_g" | "head_g" => Anchor::Head,
        "rforearm_g" | "lforearm_g" => Anchor::Forearm,
        "rcollar_g" | "lcollar_g" | "rbicep_g" | "lbicep_g" => Anchor::Arm,
        "rthigh_g" | "lthigh_g" | "rshin_g" | "lshin_g" => Anchor::Leg,
        _ => Anchor::Extremity,
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
    /// The farthest vertex of weight `DOMINANT` or more, in the node's own
    /// space (metres), and the sum and count of those vertices.
    reach: Vec<f32>,
    dominant: Vec<(Vec3, usize)>,
    /// The vertices of weight `CARRIES` or more, in the node's own space.
    held: Vec<Vec<Vec3>>,
    /// Skin vertices weighted `JOINED_WEIGHT` or more on two nodes.
    shared: BTreeMap<(usize, usize), usize>,
}

fn carried(model: &ModelIr) -> Carried {
    let n = model.nodes.len();
    let mut c = Carried {
        skin: vec![0; n],
        rigid: vec![0; n],
        load: vec![0.0; n],
        reach: vec![0.0; n],
        dominant: vec![(Vec3::ZERO, 0); n],
        held: vec![Vec::new(); n],
        shared: BTreeMap::new(),
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
                    c.dominant[node].0 += p;
                    c.dominant[node].1 += 1;
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
                    c.dominant[node].0 += local;
                    c.dominant[node].1 += 1;
                }
                if weight >= JOINED_WEIGHT && !on.contains(&node) {
                    on.push(node);
                }
            }
            for (i, &a) in on.iter().enumerate() {
                for &b in &on[i + 1..] {
                    *c.shared.entry((a.min(b), a.max(b))).or_default() += 1;
                }
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
            Anchor::Trunk => bones.len() >= 2,
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

/// The loose trees of `model`, as points with their physics values, and the
/// links between neighbouring strands. `clips` are the clips that play on
/// the model (its own and its supermodels'). They tell how far the old
/// hand-keyed animation turned each loose bone.
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
    let nodes = &model.nodes;
    let worlds: Vec<Vec3> = nodes.iter().map(|n| place(&n.world)).collect();
    let pelvis = model
        .node_index(asset_odyssey::hooks::PELVIS)
        .map_or(Vec3::ZERO, |p| worlds[p]);
    let keyed = keyed_turns(model, clips);

    let mut out = Rig::default();
    let mut strands: Vec<Strand> = Vec::new();
    let mut inputs: Vec<Vec<physics::Input>> = Vec::new();
    let roots_on_low_trunk = found
        .iter()
        .filter(|(hung, _)| {
            let name = nodes[*hung].name.to_ascii_lowercase();
            name == "pelvis_g" || name == "torso_g"
        })
        .count();
    for (hung, bones) in &found {
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
            let ends = kids.len().max(1) as f32;
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
                let (sum, count) = if carried.dominant[bone].1 > 0 {
                    carried.dominant[bone]
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
                    physics: Physics::default(),
                });
                loads.push(carried.load[bone] / 2.0 / ends);
                path_of.push(path);
            }
            stack.extend(kids.iter().rev());
        }
        let longest = points.iter().map(|p| p.along).fold(0.0, f32::max);
        let kind = kind_of(&nodes[*hung].name, longest, roots_on_low_trunk);
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
            anchor: *hung,
            points,
        });
    }
    for (tree, inputs) in out.trees.iter_mut().zip(&inputs) {
        for (point, input) in tree.points.iter_mut().zip(inputs) {
            point.physics = physics::values(tree.kind, input);
        }
    }
    let groups: Vec<Anchor> = out
        .trees
        .iter()
        .map(|tree| anchor_of(&nodes[tree.anchor].name))
        .collect();
    out.links = links(&out.trees, &strands, &carried, pelvis, &groups);
    out
}

fn kind_of(anchor: &str, longest: f32, low_trunk_trees: usize) -> Kind {
    let name = anchor.to_ascii_lowercase();
    match anchor_of(&name) {
        Anchor::Head => Kind::Hood,
        Anchor::Forearm => Kind::Sleeve,
        Anchor::Arm | Anchor::Extremity => Kind::Other,
        Anchor::Leg => Kind::Flap,
        Anchor::Trunk if name == "torsoupr_g" => {
            if longest >= CAPE_LENGTH {
                Kind::Cape
            } else {
                Kind::Flap
            }
        }
        Anchor::Trunk if low_trunk_trees >= 2 => Kind::RobeSkirt,
        Anchor::Trunk => Kind::Flap,
    }
}

/// The point of a strand at `share` of its length.
fn along_strand(trees: &[Tree], strand: &Strand, share: f32) -> Vec3 {
    let points = &trees[strand.tree].points;
    let length = points[*strand.points.last().expect("a strand has points")].along;
    let want = share * length;
    for pair in strand.points.windows(2) {
        let (a, b) = (&points[pair[0]], &points[pair[1]]);
        if b.along >= want {
            let span = (b.along - a.along).max(1e-6);
            let t = ((want - a.along) / span).clamp(0.0, 1.0);
            return Vec3::from(a.rest).lerp(Vec3::from(b.rest), t);
        }
    }
    Vec3::from(points[*strand.points.last().expect("a strand has points")].rest)
}

fn links(
    trees: &[Tree],
    strands: &[Strand],
    carried: &Carried,
    pelvis: Vec3,
    groups: &[Anchor],
) -> Vec<Link> {
    let mut out = branch_links(trees);
    out.extend(strand_links(trees, strands, carried, pelvis, groups));
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
                        length: Vec3::from(tree.points[a].rest)
                            .distance(Vec3::from(tree.points[b].rest)),
                        why: Why::Branch,
                        compliance: physics::link_compliance(tree.kind, Why::Branch),
                    });
                }
            }
        }
    }
    out
}

/// Links between the points of neighbouring strands. Two strands can be
/// neighbours when their trees are of one kind, and both hang from the
/// trunk or both from the same anchor.
fn strand_links(
    trees: &[Tree],
    strands: &[Strand],
    carried: &Carried,
    pelvis: Vec3,
    groups: &[Anchor],
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
                || (groups[a.tree] == Anchor::Trunk && groups[b.tree] == Anchor::Trunk))
    };
    let mut nearest = Vec::new();
    for (i, a) in strands.iter().enumerate() {
        let best = strands
            .iter()
            .enumerate()
            .filter(|&(j, b)| j != i && linkable(a, b))
            .map(|(_, b)| mean_distance(a, b))
            .fold(f32::MAX, f32::min);
        if best < f32::MAX {
            nearest.push(best);
        }
    }
    nearest.sort_by(f32::total_cmp);
    let median = nearest.get(nearest.len() / 2).copied().unwrap_or(0.0);
    let mut out: Vec<Link> = Vec::new();
    let mut seen: BTreeSet<((usize, usize), (usize, usize))> = BTreeSet::new();
    for (i, a) in strands.iter().enumerate() {
        for b in &strands[i + 1..] {
            if !linkable(a, b) {
                continue;
            }
            let (own_a, own_b) = (bones(a), bones(b));
            let why = if a.tree == b.tree {
                Some(Why::Root)
            } else {
                let joined: usize = own_a
                    .iter()
                    .flat_map(|&x| own_b.iter().map(move |&y| (x.min(y), x.max(y))))
                    .map(|key| carried.shared.get(&key).copied().unwrap_or(0))
                    .sum();
                let gap = (angle(a) - angle(b)).rem_euclid(360.0);
                let gap = gap.min(360.0 - gap);
                if joined >= JOINED_VERTICES {
                    Some(Why::Skin)
                } else if mean_distance(a, b) <= NEAR_SCALE * median && gap < NEAR_ANGLE {
                    Some(Why::Near)
                } else {
                    None
                }
            };
            let Some(why) = why else { continue };
            let kind = trees[a.tree].kind;
            let compliance = physics::link_compliance(kind, why);
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
            let share = |s: &Strand, p: usize| {
                let points = &trees[s.tree].points;
                points[p].along / points[*s.points.last().expect("a strand has points")].along
            };
            let mut pairs = Vec::new();
            for &p in &pa {
                let q = *pb
                    .iter()
                    .min_by(|&&x, &&y| {
                        (share(b, x) - share(a, p))
                            .abs()
                            .total_cmp(&(share(b, y) - share(a, p)).abs())
                    })
                    .expect("not empty");
                pairs.push(((a.tree, p), (b.tree, q)));
            }
            for &q in &pb {
                let p = *pa
                    .iter()
                    .min_by(|&&x, &&y| {
                        (share(a, x) - share(b, q))
                            .abs()
                            .total_cmp(&(share(a, y) - share(b, q)).abs())
                    })
                    .expect("not empty");
                pairs.push(((a.tree, p), (b.tree, q)));
            }
            for (x, y) in pairs {
                let key = (x.min(y), x.max(y));
                if x == y || !seen.insert(key) {
                    continue;
                }
                let length = Vec3::from(trees[x.0].points[x.1].rest)
                    .distance(Vec3::from(trees[y.0].points[y.1].rest));
                out.push(Link {
                    a: key.0,
                    b: key.1,
                    length,
                    why,
                    compliance,
                });
            }
        }
    }
    out
}

impl Rig {
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
        let mut groups: BTreeMap<(Why, String), usize> = BTreeMap::new();
        for link in &self.links {
            let label = |(t, p): (usize, usize)| name(self.trees[t].points[p].node);
            let root = |t: usize| name(self.trees[t].points[0].node);
            let key = match link.why {
                Why::Branch => format!("{}|{}", label(link.a), label(link.b)),
                Why::Root => root(link.a.0),
                Why::Skin | Why::Near => format!("{}~{}", root(link.a.0), root(link.b.0)),
            };
            *groups.entry((link.why, key)).or_default() += 1;
        }
        if !groups.is_empty() {
            let links: Vec<String> = groups
                .iter()
                .map(|((why, key), count)| format!("{} {key} x{count}", why.name()))
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
