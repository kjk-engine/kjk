//! `--sim` in the viewers (`kjk view`, `kjk rig`): the robe and cape solver
//! (`loose`) run on the loose bones of the body shown, so that a person can
//! watch it before it is in the game (step L5 of
//! `docs/research/2026-11-19-loose-parts-plan.md`).
//!
//! The loose bones are picked here by a temporary rule on their names, for
//! the viewers alone, until the loose bone finder of step L1 exists. The
//! values of the points are plain defaults, until step L2 gives them. The
//! game never uses this module.

use std::collections::BTreeSet;
use std::time::Instant;

use asset_odyssey::model::ModelIr;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use console::{AnimationTime, DumpFacts};
use loose::body::{Body, Capsule};
use loose::{Frame, Link, Point, Pose, Rig, Settings, Solver};
use ragdoll::Template;

use crate::scene::{SceneContent, SceneModel, animation_seconds, to_mat4};

/// Temporary, for the viewers only: whether a node is a loose bone by its
/// name. It takes the KOTOR 2 robe chains (`frobe1_g`, `mrobe2_g`,
/// `lrobe3_g`, `rrobe1_g`), the cape chains of Revan and Malak (`cape01_g`,
/// `cape02_g01`), Nihilus's cape (`lcape01`, `mcape02`, `rcape04`) and the
/// cloth of KOTOR 1 Malak (`cloth_1_g` to `cloth_27_g`). Step L1 replaces
/// it with the loose bone finder.
fn temporary_loose(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    let digits_then = |rest: &str| -> bool {
        let tail = rest.trim_start_matches(|c: char| c.is_ascii_digit());
        tail.len() < rest.len()
            && tail
                .chars()
                .all(|c| c == '_' || c == 'g' || c.is_ascii_digit())
    };
    if let Some(rest) = name.strip_prefix("cloth_") {
        return digits_then(rest);
    }
    for prefix in [
        "frobe", "mrobe", "lrobe", "rrobe", "cape", "lcape", "mcape", "rcape",
    ] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return digits_then(rest);
        }
    }
    false
}

/// The plain values every point gets until step L2 gives each kind of
/// garment its own.
mod plain {
    /// A point's maximum distance from its target, as a share of its path
    /// from its root.
    pub const REACH: f32 = 0.8;
    pub const BEND: f32 = 1e-3;
    pub const DRIVE: f32 = 2e-2;
    pub const DAMPING: f32 = 2.5;
    pub const INERTIA: f32 = 0.5;
    /// The garment's thickness around its bones (metres).
    pub const THICKNESS: f32 = 0.03;
    /// Points of two chains at the same depth closer than this at rest
    /// (metres) are linked.
    pub const LINKED: f32 = 0.4;
    pub const LINK: f32 = 1e-3;
    /// A tip's length from its bone's joint, at least and at most (metres).
    pub const TIP: [f32; 2] = [0.05, 0.6];
    /// The weight a vertex must have on a bone to measure the tip's length.
    pub const TIP_WEIGHT: f32 = 0.5;
    /// The weight a vertex must have on two bones for the point's capsules
    /// to include the part of the other bone.
    pub const REACH_WEIGHT: f32 = 0.2;
    /// The most capsules a point meets.
    pub const CAPSULES: usize = 6;
}

/// The solver on one body, and what it needs to read and write the body's
/// nodes.
#[derive(Component)]
pub(crate) struct Simulated {
    solver: Solver,
    /// The node of each point's bone; a tip has none.
    nodes: Vec<Option<usize>>,
    /// The node each root point hangs on.
    attach: Vec<usize>,
    /// The node whose frame is the solver's root: the one the pelvis hangs
    /// on, which the animation moves through the world.
    root: usize,
    body: Option<Body>,
    /// Each loose bone's rest turn, written back before the animation runs,
    /// so that a bone the clip does not key has its rest turn as its target.
    rest: Vec<(usize, Quat)>,
    last: Option<f32>,
    stats: Stats,
    /// Reused each frame.
    frames: Vec<Frame>,
    capsules: Vec<Capsule>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    frames: u64,
    substeps: u64,
    resets: u64,
    seconds: f64,
}

fn rotation_of(rotation: [f32; 4]) -> Quat {
    let [x, y, z, w] = rotation;
    Quat::from_xyzw(x, y, z, w)
}

/// The rig of `ir`'s loose bones, the node of each point, and the node
/// each root hangs on.
fn rig(ir: &ModelIr) -> (Rig, Vec<Option<usize>>, Vec<usize>) {
    let joints: BTreeSet<usize> = ir
        .meshes
        .iter()
        .filter_map(|mesh| mesh.skin.as_ref())
        .flat_map(|skin| skin.joints.iter().copied())
        .collect();
    let loose: Vec<usize> = (0..ir.nodes.len())
        .filter(|&node| joints.contains(&node) && temporary_loose(&ir.nodes[node].name))
        .collect();
    let world: Vec<Mat4> = ir.nodes.iter().map(|node| to_mat4(&node.world)).collect();
    let at = |node: usize| world[node].transform_point3(Vec3::ZERO);
    let mut rig = Rig::default();
    let mut nodes = Vec::new();
    let mut attach = Vec::new();
    let mut point_of = vec![None; ir.nodes.len()];
    let mut depth = Vec::new();
    let mut path = Vec::new();
    for &node in &loose {
        let parent_node = ir.nodes[node].parent;
        let parent = parent_node.and_then(|p| point_of[p]);
        let offset = Vec3::from_array(ir.nodes[node].translation);
        let (d, length) = match parent {
            Some(p) => (depth[p] + 1, path[p] + offset.length()),
            None => (0, 0.0),
        };
        point_of[node] = Some(rig.points.len());
        rig.points.push(point(parent, offset, at(node), length));
        nodes.push(Some(node));
        attach.push(parent_node.unwrap_or(0));
        depth.push(d);
        path.push(length);
    }
    for &node in &loose {
        let has_child = loose
            .iter()
            .any(|&other| ir.nodes[other].parent == Some(node));
        if has_child {
            continue;
        }
        let Some(head) = point_of[node] else { continue };
        let from = ir.nodes[node].parent.map_or(Vec3::ZERO, &at);
        let way = (at(node) - from).try_normalize().unwrap_or(Vec3::NEG_Z);
        let length = tip_length(ir, node);
        let turn = world[node].to_scale_rotation_translation().1;
        let offset = turn.inverse() * way * length;
        let total = path[head] + length;
        rig.points
            .push(point(Some(head), offset, at(node) + way * length, total));
        nodes.push(None);
        attach.push(0);
        depth.push(depth[head] + 1);
        path.push(total);
    }
    let roots = rig.roots();
    for a in 0..rig.points.len() {
        for b in a + 1..rig.points.len() {
            if roots[a] != roots[b]
                && depth[a] == depth[b]
                && depth[a] > 0
                && rig.points[a].at.distance(rig.points[b].at) < plain::LINKED
            {
                rig.links.push(Link {
                    a,
                    b,
                    compliance: plain::LINK,
                });
            }
        }
    }
    (rig, nodes, attach)
}

fn point(parent: Option<usize>, offset: Vec3, at: Vec3, path: f32) -> Point {
    Point {
        parent,
        offset,
        at,
        pinned: parent.is_none(),
        freedom: 1.0,
        max_distance: plain::REACH * path,
        stretch: 0.0,
        bend: plain::BEND,
        drive: Some(plain::DRIVE),
        damping: plain::DAMPING,
        inertia: plain::INERTIA,
        radius: plain::THICKNESS,
    }
}

/// How far the tip of leaf bone `node` lies from its joint: the farthest
/// vertex that the bone moves with at least half its weight, measured in
/// the bone's own space.
fn tip_length(ir: &ModelIr, node: usize) -> f32 {
    let mut longest = 0.0f32;
    for mesh in &ir.meshes {
        let Some(skin) = &mesh.skin else { continue };
        let Some(slot) = skin.joints.iter().position(|&joint| joint == node) else {
            continue;
        };
        let Some(bind) = skin.inverse_binds.get(slot).map(to_mat4) else {
            continue;
        };
        for (vertex, position) in mesh.positions.iter().enumerate() {
            let weight: f32 = (0..4)
                .filter(|&k| usize::from(skin.vertex_joints[vertex][k]) == slot)
                .map(|k| skin.vertex_weights[vertex][k])
                .sum();
            if weight >= plain::TIP_WEIGHT {
                let local = bind.transform_point3(Vec3::from_array(*position));
                longest = longest.max(local.length());
            }
        }
    }
    longest.clamp(plain::TIP[0], plain::TIP[1])
}

/// For each point, the capsules its skin weights reach: the parts that own
/// the bones that move the same vertices as the point's bone.
fn reach(ir: &ModelIr, body: &Body, nodes: &[Option<usize>], rig: &Rig) -> Vec<Vec<usize>> {
    let parents: Vec<Option<usize>> = ir.nodes.iter().map(|node| node.parent).collect();
    let mut reached: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); nodes.len()];
    for (point, node) in nodes.iter().enumerate() {
        let node = node.or_else(|| rig.points[point].parent.and_then(|p| nodes[p]));
        let Some(node) = node else { continue };
        for mesh in &ir.meshes {
            let Some(skin) = &mesh.skin else { continue };
            let Some(slot) = skin.joints.iter().position(|&joint| joint == node) else {
                continue;
            };
            for (joints, weights) in skin.vertex_joints.iter().zip(&skin.vertex_weights) {
                let on_bone = (0..4)
                    .any(|k| usize::from(joints[k]) == slot && weights[k] >= plain::REACH_WEIGHT);
                if !on_bone {
                    continue;
                }
                for k in 0..4 {
                    if weights[k] < plain::REACH_WEIGHT {
                        continue;
                    }
                    let Some(&other) = skin.joints.get(usize::from(joints[k])) else {
                        continue;
                    };
                    if let Some(part) = body.part_of(&parents, other) {
                        reached[point].insert(part);
                    }
                }
            }
        }
    }
    reached
        .into_iter()
        .map(|set| set.into_iter().collect())
        .collect()
}

/// The solver set up on a scene's first model, once it is spawned.
fn start(
    mut commands: Commands,
    scene: Option<Res<SceneContent>>,
    added: Query<(Entity, &SceneModel), Added<SceneModel>>,
) {
    let Some(scene) = scene else { return };
    if !scene.0.loose.as_ref().is_some_and(|loose| loose.sim) {
        return;
    }
    for (entity, model) in &added {
        if model.index != 0 {
            continue;
        }
        let ir = &scene.0.models[0].ir;
        let (rig, nodes, attach) = rig(ir);
        if rig.points.is_empty() {
            warn!("{}: --sim found no robe, cape or cloth bones", ir.name);
            continue;
        }
        let template = crate::ragdolls::fitted(ir, &crate::cast::rest_of(ir), |skeleton| {
            Template::fit(&ragdoll::humanoid::PLAN, skeleton)
        })
        .inspect_err(|why| warn!("{}: --sim without the body's capsules: {why}", ir.name))
        .ok();
        let body = template.as_ref().map(Body::new);
        let collision = body.as_ref().map(|body| {
            let reached = reach(ir, body, &nodes, &rig);
            loose::body::contacts(&rig, body, &reached, plain::CAPSULES)
        });
        let contacts = collision
            .as_ref()
            .map_or(0, |c| c.contacts.iter().map(Vec::len).sum::<usize>());
        let solver = match Solver::new(&rig, Settings::default(), collision) {
            Ok(solver) => solver,
            Err(why) => {
                warn!("{}: --sim: {why}", ir.name);
                continue;
            }
        };
        let root = ir
            .node_index(asset_odyssey::hooks::PELVIS)
            .and_then(|pelvis| ir.nodes[pelvis].parent)
            .unwrap_or(0);
        let rest = nodes
            .iter()
            .flatten()
            .map(|&node| (node, rotation_of(ir.nodes[node].rotation)))
            .collect();
        info!(
            "{}: --sim moves {} points ({} roots, {} links, {} capsule contacts) on {}",
            ir.name,
            rig.points.len(),
            rig.points.iter().filter(|p| p.parent.is_none()).count(),
            rig.links.len(),
            contacts,
            nodes
                .iter()
                .flatten()
                .map(|&node| ir.nodes[node].name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        commands.entity(entity).insert(Simulated {
            solver,
            nodes,
            attach,
            root,
            body,
            rest,
            last: None,
            stats: Stats::default(),
            frames: Vec::with_capacity(ir.nodes.len()),
            capsules: Vec::new(),
        });
    }
}

/// Every loose bone back at its rest turn before the clip poses the body:
/// a bone the clip does not key would otherwise keep the solver's last
/// turn, and the solver would read it as its target.
fn at_rest(bodies: Query<(&Simulated, &SceneModel)>, mut transforms: Query<&mut Transform>) {
    for (simulated, model) in &bodies {
        for &(node, turn) in &simulated.rest {
            if let Some(&entity) = model.nodes.get(node)
                && let Ok(mut transform) = transforms.get_mut(entity)
            {
                transform.rotation = turn;
            }
        }
    }
}

/// One step of the solver on each body, by the time the clip plays at, and
/// the new turns of its loose bones.
#[allow(clippy::too_many_arguments)]
fn solve(
    time: Res<Time>,
    held: Res<AnimationTime>,
    slow: Option<Res<crate::loose_view::Slow>>,
    scene: Option<Res<SceneContent>>,
    mut bodies: Query<(&mut Simulated, &SceneModel, &Transform)>,
    mut transforms: Query<&mut Transform, Without<Simulated>>,
    mut facts: ResMut<DumpFacts>,
) {
    let Some(scene) = scene else { return };
    let seconds = animation_seconds(&time, &held) / slow.map_or(1.0, |slow| slow.0.max(0.01));
    for (mut simulated, model, placed) in &mut bodies {
        let started = Instant::now();
        let ir = &scene.0.models[0].ir;
        let simulated = &mut *simulated;
        simulated.frames.clear();
        for (index, node) in ir.nodes.iter().enumerate() {
            let local = model
                .nodes
                .get(index)
                .and_then(|&entity| transforms.get(entity).ok())
                .map_or(
                    Frame::new(
                        Vec3::from_array(node.translation),
                        rotation_of(node.rotation),
                    ),
                    |t| Frame::new(t.translation, t.rotation),
                );
            let frame = match node.parent {
                Some(parent) if parent < index => simulated.frames[parent].then(local),
                _ => local,
            };
            simulated.frames.push(frame);
        }
        let model_world = Frame::new(placed.translation, placed.rotation);
        let root_model = simulated.frames[simulated.root];
        let to_root = root_model.inverse();
        let attach: Vec<Frame> = simulated
            .attach
            .iter()
            .map(|&node| to_root.then(simulated.frames[node]))
            .collect();
        let turns: Vec<Quat> = simulated
            .nodes
            .iter()
            .map(|node| {
                node.and_then(|node| model.nodes.get(node))
                    .and_then(|&entity| transforms.get(entity).ok())
                    .map_or(Quat::IDENTITY, |t| t.rotation)
            })
            .collect();
        let mut capsules = std::mem::take(&mut simulated.capsules);
        if let Some(body) = &simulated.body {
            let frames = &simulated.frames;
            body.pose(|node| to_root.then(frames[node]), &mut capsules);
        }
        let pose = Pose {
            root: model_world.then(root_model),
            attach: &attach,
            turns: &turns,
            capsules: &capsules,
            floor: Some(placed.translation.z),
        };
        let elapsed = match simulated.last {
            Some(last) if seconds >= last => seconds - last,
            _ => {
                simulated.solver.reset();
                0.0
            }
        };
        simulated.last = Some(seconds);
        let report = simulated.solver.step(elapsed, &pose);
        let new = simulated.solver.turns(&pose);
        simulated.capsules = capsules;
        for (point, node) in simulated.nodes.iter().enumerate() {
            let Some(node) = node else { continue };
            if let Some(&entity) = model.nodes.get(*node)
                && let Ok(mut transform) = transforms.get_mut(entity)
            {
                transform.rotation = new[point];
            }
        }
        let stats = &mut simulated.stats;
        stats.frames += 1;
        stats.substeps += u64::from(report.substeps);
        stats.resets += u64::from(report.reset);
        stats.seconds += started.elapsed().as_secs_f64();
        facts.0.insert(
            "loose_sim".into(),
            serde_json::json!({
                "points": simulated.solver.points().len(),
                "frames": stats.frames,
                "substeps": stats.substeps,
                "resets": stats.resets,
                "ms_per_frame": stats.seconds * 1000.0 / stats.frames.max(1) as f64,
            }),
        );
    }
}

pub(crate) struct LooseSimPlugin;

impl Plugin for LooseSimPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, at_rest).add_systems(
            PostUpdate,
            (start, solve).chain().before(TransformSystems::Propagate),
        );
    }
}
