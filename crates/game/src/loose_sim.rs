//! `--sim` in the viewers (`kjk view`, `kjk rig`). It runs the robe and cape
//! solver (`loose`) on the loose bones of the body shown, so that a person
//! can watch it before the game uses it
//! (https://github.com/kjk-engine/roadmap/issues/5).
//!
//! This module picks the loose bones by a temporary rule on their names,
//! for the viewers alone, until the loose bone finder exists. The values
//! of the points are plain defaults, until each kind of garment has its
//! own. The game never uses this module.

use std::collections::BTreeSet;
use std::time::Instant;

use asset_odyssey::model::ModelIr;
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use console::{AnimationTime, DumpFacts};
use loose::{Body, Frame, Link, Place, Point, Rig, Settings, Skeleton, Solver};
use ragdoll::Template;

use crate::scene::{SceneContent, SceneModel, animation_seconds, to_mat4};

/// Temporary, for the viewers only. It says whether a node is a loose bone
/// by its name. It takes the KOTOR 2 robe chains (`frobe1_g`, `mrobe2_g`,
/// `lrobe3_g`, `rrobe1_g`), the cape chains of Revan and Malak (`cape01_g`,
/// `cape02_g01`), Nihilus's cape (`lcape01`, `mcape02`, `rcape04`) and the
/// cloth of KOTOR 1 Malak (`cloth_1_g` to `cloth_27_g`). The loose bone
/// finder will replace it.
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
    [
        "frobe", "mrobe", "lrobe", "rrobe", "cape", "lcape", "mcape", "rcape",
    ]
    .iter()
    .any(|prefix| name.strip_prefix(prefix).is_some_and(digits_then))
}

/// The plain values every point gets until each kind of garment has its
/// own.
mod plain {
    /// A point's maximum distance from its target, as a share of its path
    /// from its root.
    pub const REACH: f32 = 0.8;
    pub const BEND: f32 = 1e-3;
    pub const DRIVE: f32 = 2e-2;
    pub const DAMPING: f32 = 2.5;
    pub const INERTIA: f32 = 0.5;
    /// How thick the garment is around its bones (metres).
    pub const THICKNESS: f32 = 0.03;
    /// Points of two chains at the same depth that are closer than this at
    /// rest (metres) are linked.
    pub const LINK_WITHIN: f32 = 0.4;
    pub const LINK_COMPLIANCE: f32 = 1e-3;
    /// A tip's length from its bone's joint, at least and at most (metres).
    pub const TIP: [f32; 2] = [0.05, 0.6];
    /// The weight a vertex must have on a bone to measure the tip's length.
    pub const TIP_WEIGHT: f32 = 0.5;
    /// The weight a vertex must have on two bones for the point of one to
    /// reach the other.
    pub const REACH_WEIGHT: f32 = 0.2;
}

/// The solver on one body.
#[derive(Component)]
pub(crate) struct Simulated {
    solver: Solver,
    /// Each loose bone's rest turn. It is written back before the clip
    /// runs, so that a bone the clip does not key has its rest turn as its
    /// target.
    rest: Vec<(usize, Quat)>,
    last: Option<f32>,
    stats: Stats,
    locals: Vec<Frame>,
}

#[derive(Debug, Default, Clone, Copy)]
struct Stats {
    frames: u64,
    substeps: u64,
    resets: u64,
    seconds: f64,
}

fn frame_of(translation: [f32; 3], rotation: [f32; 4]) -> Frame {
    let [x, y, z, w] = rotation;
    Frame::new(Vec3::from_array(translation), Quat::from_xyzw(x, y, z, w))
}

/// The skeleton of `ir` in its rest pose, rooted at the node above the
/// pelvis, which a clip moves along the ground.
fn skeleton(ir: &ModelIr) -> Skeleton {
    let root = ir
        .node_index(asset_odyssey::hooks::PELVIS)
        .and_then(|pelvis| ir.nodes[pelvis].parent)
        .unwrap_or(0);
    Skeleton {
        parents: ir.nodes.iter().map(|node| node.parent).collect(),
        rest: ir
            .nodes
            .iter()
            .map(|node| frame_of(node.translation, node.rotation))
            .collect(),
        root,
    }
}

/// A point of the plain values.
fn point(place: Place, path: f32, reach: Vec<usize>) -> Point {
    Point {
        place,
        freedom: 1.0,
        max_distance: plain::REACH * path,
        stretch: 0.0,
        bend: plain::BEND,
        drive: Some(plain::DRIVE),
        damping: plain::DAMPING,
        inertia: plain::INERTIA,
        thickness: plain::THICKNESS,
        reach,
    }
}

/// The rig of `ir`'s loose bones: a point at each loose bone's joint, and a
/// tip after each last bone of a chain.
fn rig(ir: &ModelIr) -> Rig {
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
    /// What the rig needs to know of each point while it is built.
    struct Built {
        node: usize,
        depth: usize,
        path: f32,
        at: Vec3,
    }
    let mut built: Vec<Built> = Vec::new();
    let mut rig = Rig::default();
    for &node in &loose {
        let parent = ir.nodes[node]
            .parent
            .and_then(|parent| built.iter().position(|b| b.node == parent));
        let length = Vec3::from_array(ir.nodes[node].translation).length();
        let (depth, path) =
            parent.map_or((0, 0.0), |p| (built[p].depth + 1, built[p].path + length));
        rig.points
            .push(point(Place::Joint(node), path, reach(ir, node)));
        built.push(Built {
            node,
            depth,
            path,
            at: at(node),
        });
    }
    for (index, &node) in loose.iter().enumerate() {
        if loose
            .iter()
            .any(|&other| ir.nodes[other].parent == Some(node))
        {
            continue;
        }
        let from = ir.nodes[node].parent.map_or(Vec3::ZERO, &at);
        let way = (at(node) - from).try_normalize().unwrap_or(Vec3::NEG_Z);
        let length = tip_length(ir, node);
        let turn = world[node].to_scale_rotation_translation().1;
        let path = built[index].path + length;
        rig.points.push(point(
            Place::Tip {
                point: index,
                offset: turn.inverse() * way * length,
            },
            path,
            reach(ir, node),
        ));
        built.push(Built {
            node,
            depth: built[index].depth + 1,
            path,
            at: at(node) + way * length,
        });
    }
    let root_of = |mut index: usize| {
        while let Some(parent) = ir.nodes[built[index].node]
            .parent
            .and_then(|parent| built.iter().position(|b| b.node == parent))
        {
            index = parent;
        }
        index
    };
    for a in 0..built.len() {
        for b in a + 1..built.len() {
            if root_of(a) != root_of(b)
                && built[a].depth == built[b].depth
                && built[a].depth > 0
                && built[a].at.distance(built[b].at) < plain::LINK_WITHIN
            {
                rig.links.push(Link {
                    a,
                    b,
                    compliance: plain::LINK_COMPLIANCE,
                });
            }
        }
    }
    rig
}

/// How far the tip of leaf bone `node` lies from its joint. It is the
/// farthest vertex that the bone moves with at least half its weight,
/// measured in the bone's own space.
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

/// The nodes whose bones move the same vertices as bone `node`.
fn reach(ir: &ModelIr, node: usize) -> Vec<usize> {
    let mut reached = BTreeSet::new();
    for mesh in &ir.meshes {
        let Some(skin) = &mesh.skin else { continue };
        let Some(slot) = skin.joints.iter().position(|&joint| joint == node) else {
            continue;
        };
        for (joints, weights) in skin.vertex_joints.iter().zip(&skin.vertex_weights) {
            let heavy = |k: usize| weights[k] >= plain::REACH_WEIGHT;
            if !(0..4).any(|k| usize::from(joints[k]) == slot && heavy(k)) {
                continue;
            }
            reached.extend(
                (0..4)
                    .filter(|&k| heavy(k))
                    .filter_map(|k| skin.joints.get(usize::from(joints[k])).copied()),
            );
        }
    }
    reached.into_iter().collect()
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
        let ir = &scene.0.models[model.index].ir;
        let rig = rig(ir);
        if rig.points.is_empty() {
            warn!("{}: --sim found no robe, cape or cloth bones", ir.name);
            continue;
        }
        let body = crate::ragdolls::fitted(ir, &crate::cast::rest_of(ir), |skeleton| {
            Template::fit(&ragdoll::humanoid::PLAN, skeleton)
        })
        .inspect_err(|why| warn!("{}: --sim without the body's capsules: {why}", ir.name))
        .ok()
        .map(|template| Body::new(&template));
        let solver = match Solver::new(&rig, &skeleton(ir), body, Settings::default()) {
            Ok(solver) => solver,
            Err(why) => {
                warn!("{}: --sim: {why}", ir.name);
                continue;
            }
        };
        let bones: Vec<usize> = rig
            .points
            .iter()
            .filter_map(|point| match point.place {
                Place::Joint(node) => Some(node),
                Place::Tip { .. } => None,
            })
            .collect();
        info!(
            "{}: --sim moves {} points and {} links on {}",
            ir.name,
            rig.points.len(),
            rig.links.len(),
            bones
                .iter()
                .map(|&node| ir.nodes[node].name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        commands.entity(entity).insert(Simulated {
            solver,
            rest: bones
                .iter()
                .map(|&node| {
                    (
                        node,
                        frame_of(ir.nodes[node].translation, ir.nodes[node].rotation).rotation,
                    )
                })
                .collect(),
            last: None,
            stats: Stats::default(),
            locals: Vec::with_capacity(ir.nodes.len()),
        });
    }
}

/// Every loose bone back at its rest turn before the clip poses the body.
/// A bone the clip does not key would otherwise keep the solver's last
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
fn solve(
    (time, held, slow): (
        Res<Time>,
        Res<AnimationTime>,
        Option<Res<crate::loose_view::Slow>>,
    ),
    mut bodies: Query<(&mut Simulated, &SceneModel, &Transform)>,
    mut transforms: Query<&mut Transform, Without<Simulated>>,
    mut facts: ResMut<DumpFacts>,
) {
    let seconds = animation_seconds(&time, &held) / slow.map_or(1.0, |slow| slow.0.max(0.01));
    for (mut simulated, model, placed) in &mut bodies {
        let started = Instant::now();
        let simulated = &mut *simulated;
        simulated.locals.clear();
        simulated.locals.extend(model.nodes.iter().map(|&entity| {
            transforms
                .get(entity)
                .map_or(Frame::IDENTITY, |t| Frame::new(t.translation, t.rotation))
        }));
        let elapsed = match simulated.last {
            Some(last) if seconds >= last => seconds - last,
            _ => {
                simulated.solver.reset();
                0.0
            }
        };
        simulated.last = Some(seconds);
        let model_frame = Frame::new(placed.translation, placed.rotation);
        let step = simulated.solver.step(
            elapsed,
            model_frame,
            &simulated.locals,
            Some(placed.translation.z),
        );
        for &(node, turn) in step.turns {
            if let Some(&entity) = model.nodes.get(node)
                && let Ok(mut transform) = transforms.get_mut(entity)
            {
                transform.rotation = turn;
            }
        }
        let stats = &mut simulated.stats;
        stats.frames += 1;
        stats.substeps += u64::from(step.substeps);
        stats.resets += u64::from(step.reset);
        stats.seconds += started.elapsed().as_secs_f64();
        facts.0.insert(
            "loose_sim".into(),
            serde_json::json!({
                "points": simulated.solver.len(),
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
