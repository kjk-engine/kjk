//! `--sim` in the viewers (`kjk view`, `kjk rig`). It runs the robe and cape
//! solver on the robe skirts and capes of the body shown, as the game does
//! (`loose_parts`), so that a person can watch it on one body
//! (https://github.com/kjk-engine/roadmap/issues/5).
//!
//! The viewers have no sim. The solver therefore steps by the time the clip
//! plays at, and the body's movement through the world is the movement of
//! the node above the pelvis, which a clip moves along the ground. The
//! floor is at the model's feet.

use std::time::Instant;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use console::{AnimationTime, DumpFacts};
use loose::{Frame, Solver};

use crate::loose_parts::Model;
use crate::scene::{SceneContent, SceneModel, animation_seconds};

/// The solver on one body.
#[derive(Component)]
pub(crate) struct Simulated {
    solver: Solver,
    /// Each moved bone's rest turn. It is written back before the clip
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
        let placed = &scene.0.models[model.index];
        let ir = &placed.ir;
        let clips: Vec<_> = scene
            .0
            .loose
            .iter()
            .flat_map(|loose| &loose.chain)
            .collect();
        let root = ir
            .node_index(asset_odyssey::hooks::PELVIS)
            .and_then(|pelvis| ir.nodes[pelvis].parent)
            .unwrap_or(0);
        let Some(model) = Model::new(ir, &clips, root) else {
            warn!("{}: --sim found no robe skirt or cape", ir.name);
            continue;
        };
        let solver = model.solver();
        info!(
            "{}: --sim moves {} points on {}",
            ir.name,
            solver.len(),
            model
                .bones()
                .iter()
                .map(|&(node, _)| ir.nodes[node].name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        commands.entity(entity).insert(Simulated {
            solver,
            rest: model.bones().to_vec(),
            last: None,
            stats: Stats::default(),
            locals: Vec::with_capacity(ir.nodes.len()),
        });
    }
}

/// Every moved bone back at its rest turn before the clip poses the body.
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
