//! Robe skirts and capes moved by the robe and cape solver (`loose`) in
//! play, for https://github.com/kjk-engine/roadmap/issues/5. The solver
//! runs in the view only and never reaches the sim.
//!
//! - The loose bone finder (`asset_ja::loose`) reads each body model once.
//!   `solver_rig` turns what it finds into the solver's input. Only the
//!   kinds of garment in `SIMULATED` are moved. Every other loose bone keeps
//!   its clips' keys.
//! - While the solver runs, a clip bound to a body drops its keys on the
//!   moved bones (`dropped`), unless the clip is a flourish. The cast also
//!   puts those bones back at rest every frame instead of holding them
//!   (`cast::held_nodes`). The target pose of a moved bone is therefore its
//!   rest pose under its animated parent, or the flourish's keys.
//! - The cast lists the bodies it posed this frame (`Worn`). One system,
//!   after every system that writes bone transforms and before Bevy
//!   propagates them, steps each listed body's solver by game time and
//!   writes the new turns of its loose bones. A body the cast did not pose
//!   is left as it is. A fallen body is posed while its ragdoll falls, so
//!   its robe falls with it toward its rest pose. Once it lies still, the
//!   cast stops posing it and its robe keeps its last shape.
//! - A body's points go back to their target pose when it was not posed the
//!   frame before, when its model changes, when it moves more than 5 m in
//!   one frame (the solver's own test), and when a cutscene cuts to its
//!   next staged line.
//!
//! `KJK_LOOSE=off` turns the solver off for the run. The clips then keep
//! their keys on every loose bone, as before the solver.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use asset_ja::loose::{self as finder, Kind};
use asset_odyssey::anim::AnimationIr;
use asset_odyssey::model::ModelIr;
use bevy::prelude::*;
use bevy::tasks::ComputeTaskPool;
use console::{DumpDue, DumpFacts};
use loose::{Body, Frame, Link, Place, Point, Rig, Settings, Skeleton, Solver};
use ragdoll::Template;

/// The kinds of garment the solver moves. Hoods, sleeves, flaps and armour
/// keep playing their clips' keys until they are tuned for the solver.
const SIMULATED: [Kind; 2] = [Kind::Cape, Kind::RobeSkirt];

/// With fewer bodies to solve, waking the threads costs about as much as it
/// saves (`cast::PARALLEL_FROM`).
const PARALLEL_FROM: usize = 8;

/// Whether `KJK_LOOSE=off` is unset.
pub(crate) fn wanted() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| std::env::var("KJK_LOOSE").as_deref() != Ok("off"))
}

/// Whether a solver moves the loose bones in this run. The game sets it
/// from `wanted`, and the viewers from `--sim`, before any clip is bound.
static SOLVING: AtomicBool = AtomicBool::new(false);

pub(crate) fn solve_in_this_run(on: bool) {
    SOLVING.store(on, Ordering::Relaxed);
}

fn running() -> bool {
    SOLVING.load(Ordering::Relaxed)
}

/// What the finder found on one model, with no clips.
struct Found {
    rig: finder::Rig,
    /// The bones the solver moves.
    moved: Arc<[usize]>,
}

/// The finder's result for `ir`, found once per model name.
fn found(ir: &ModelIr) -> Arc<Found> {
    static FOUND: Mutex<BTreeMap<String, Arc<Found>>> = Mutex::new(BTreeMap::new());
    let mut found = FOUND.lock().unwrap_or_else(|e| e.into_inner());
    found
        .entry(ir.name.to_ascii_lowercase())
        .or_insert_with(|| {
            let rig = finder::rig(ir, &[]);
            let moved = simulated(&rig).flat_map(bones_of).collect();
            Arc::new(Found { rig, moved })
        })
        .clone()
}

/// The trees of `rig` that the solver moves.
fn simulated(rig: &finder::Rig) -> impl Iterator<Item = &finder::Tree> {
    rig.trees
        .iter()
        .filter(|tree| SIMULATED.contains(&tree.kind))
}

fn bones_of(tree: &finder::Tree) -> impl Iterator<Item = usize> + '_ {
    tree.points
        .iter()
        .filter(|point| !point.tip)
        .map(|point| point.node)
}

/// The nodes of `ir` that the solver moves, while it runs.
pub(crate) fn moved(ir: &ModelIr) -> Arc<[usize]> {
    if !running() {
        return Arc::new([]);
    }
    found(ir).moved.clone()
}

/// The nodes of `ir` whose keys in `clip` are dropped when it is bound: the
/// bones the solver moves, unless `clip` is a flourish.
pub(crate) fn dropped(ir: &ModelIr, clip: &AnimationIr) -> Arc<[usize]> {
    if !running() {
        return Arc::new([]);
    }
    let found = found(ir);
    if found.moved.is_empty() || found.rig.flourish(ir, clip) {
        return Arc::new([]);
    }
    found.moved.clone()
}

/// The solver's input for the trees of `found` that it moves
/// (`SIMULATED`). The finder gives each point's values in the solver's own
/// terms. Its links name a tree and a point, which become the point's
/// number in the solver's one list.
fn solver_rig(found: &finder::Rig) -> Rig {
    let mut first = vec![None; found.trees.len()];
    let mut count = 0;
    for (index, tree) in found.trees.iter().enumerate() {
        if SIMULATED.contains(&tree.kind) {
            first[index] = Some(count);
            count += tree.points.len();
        }
    }
    let flat = |(tree, point): (usize, usize)| first[tree].map(|first| first + point);
    let mut rig = Rig::default();
    for (index, tree) in found.trees.iter().enumerate() {
        if first[index].is_none() {
            continue;
        }
        for point in &tree.points {
            let place = match (point.tip, point.parent.and_then(|p| flat((index, p)))) {
                (true, Some(joint)) => Place::Tip {
                    point: joint,
                    offset: Vec3::from_array(point.offset),
                },
                _ => Place::Joint(point.node),
            };
            let values = point.physics;
            rig.points.push(Point {
                place,
                freedom: values.freedom,
                max_distance: values.max_distance,
                stretch: values.stretch,
                bend: values.bend,
                drive: values.drive,
                damping: values.damping,
                inertia: values.inertia,
                thickness: values.radius,
                reach: point.reach.clone(),
            });
        }
    }
    rig.links = found
        .links
        .iter()
        .filter_map(|link| {
            Some(Link {
                a: flat(link.a)?,
                b: flat(link.b)?,
                compliance: link.compliance,
            })
        })
        .collect();
    rig
}

/// The solver set up for one body model, at rest. Each body that wears the
/// model starts from a copy.
pub(crate) struct Model {
    solver: Solver,
    /// The bones the solver turns, and their rest turns.
    bones: Vec<(usize, Quat)>,
}

impl Model {
    /// The solver for `ir`'s robe skirts and capes, or `None` when it has
    /// none. `clips` are the clips that play on the model. They set how far
    /// each point may stray (`asset_ja::loose::physics`). `root` is the node
    /// whose movement through the world the points feel only in part.
    pub(crate) fn new(ir: &ModelIr, clips: &[&AnimationIr], root: usize) -> Option<Model> {
        let found = finder::rig(ir, clips);
        let rig = solver_rig(&found);
        if rig.points.is_empty() {
            return None;
        }
        let rest = crate::cast::rest_of(ir);
        let skeleton = Skeleton {
            parents: rest.iter().map(|(parent, ..)| *parent).collect(),
            rest: rest
                .iter()
                .map(|&(_, translation, rotation)| Frame::new(translation, rotation))
                .collect(),
            root,
        };
        let body = crate::ragdolls::fitted(ir, &rest, |skeleton| {
            Template::fit(&ragdoll::humanoid::PLAN, skeleton)
        })
        .inspect_err(|why| warn!("{}: robes without the body's capsules: {why}", ir.name))
        .ok()
        .map(|template| Body::new(&template));
        let solver = Solver::new(&rig, &skeleton, body, Settings::default())
            .inspect_err(|why| warn!("{}: no robe solver: {why}", ir.name))
            .ok()?;
        let bones = simulated(&found)
            .flat_map(bones_of)
            .map(|node| (node, rest[node].2))
            .collect();
        Some(Model { solver, bones })
    }

    pub(crate) fn solver(&self) -> Solver {
        self.solver.clone()
    }

    /// The bones the solver turns, each with its rest turn.
    pub(crate) fn bones(&self) -> &[(usize, Quat)] {
        &self.bones
    }
}

/// The solver of the body model `ir` in play, made once per model name, or
/// `None` while the solver is off or when the model has no robe skirt or
/// cape. The points feel the movement of the model's root node through
/// the world, which is where the cast places the character.
pub(crate) fn worn(ir: &ModelIr, clips: &[&AnimationIr]) -> Option<Arc<Model>> {
    static MODELS: Mutex<BTreeMap<String, Option<Arc<Model>>>> = Mutex::new(BTreeMap::new());
    if !running() || found(ir).moved.is_empty() {
        return None;
    }
    let mut models = MODELS.lock().unwrap_or_else(|e| e.into_inner());
    models
        .entry(ir.name.to_ascii_lowercase())
        .or_insert_with(|| Model::new(ir, clips, 0).map(Arc::new))
        .clone()
}

/// A body that wears a model the solver moves: the model's solver and the
/// body's node entities, by node.
#[derive(Clone)]
pub(crate) struct Wearer {
    pub(crate) model: Arc<Model>,
    pub(crate) nodes: Arc<[Entity]>,
}

/// A body the cast posed this frame (`Cast::draw`).
pub(crate) struct Worn {
    /// The model's root entity, which the cast places in the world.
    pub(crate) root: Entity,
    pub(crate) wearer: Wearer,
    /// The game time the body was posed at (ms).
    pub(crate) ms: f64,
    /// The height of the floor last found under the body (metres).
    pub(crate) floor: Option<f32>,
}

/// One body's solver between frames.
struct State {
    wearer: Wearer,
    solver: Solver,
    /// The frame and the game time (ms) it was last solved at.
    last: Option<(u64, f64)>,
    locals: Vec<Frame>,
    turns: Vec<(Entity, Quat)>,
    substeps: u32,
    reset: bool,
}

impl State {
    fn new(wearer: &Wearer) -> State {
        State {
            solver: wearer.model.solver(),
            locals: Vec::with_capacity(wearer.nodes.len()),
            turns: Vec::new(),
            wearer: wearer.clone(),
            last: None,
            substeps: 0,
            reset: false,
        }
    }

    /// One step to game time `ms` of frame `frame`, from the posed skeleton
    /// in `transforms`. The new turns go to `turns`.
    fn step(&mut self, worn: &Worn, (frame, cut): (u64, bool), transforms: &Query<&mut Transform>) {
        let Ok(placed) = transforms.get(worn.root) else {
            return;
        };
        self.locals.clear();
        self.locals.extend(self.wearer.nodes.iter().map(|&entity| {
            transforms
                .get(entity)
                .map_or(Frame::IDENTITY, |t| Frame::new(t.translation, t.rotation))
        }));
        let seconds = match self.last {
            Some((last, ms)) if last + 1 == frame && !cut => ((worn.ms - ms) / 1000.0) as f32,
            _ => {
                self.solver.reset();
                0.0
            }
        };
        self.last = Some((frame, worn.ms));
        let model = Frame::new(placed.translation, placed.rotation);
        let step = self.solver.step(seconds, model, &self.locals, worn.floor);
        self.substeps = step.substeps;
        self.reset = step.reset;
        self.turns.clear();
        let nodes = &self.wearer.nodes;
        self.turns.extend(
            step.turns
                .iter()
                .filter_map(|&(node, turn)| Some((*nodes.get(node)?, turn))),
        );
    }
}

/// The bodies the cast posed this frame, with what the solver did over
/// the run, for the dumps.
#[derive(Resource, Default)]
pub(crate) struct Solving {
    /// Filled by the cast each frame, emptied by `solve`.
    pub(crate) worn: Vec<Worn>,
    /// Whether a cutscene cut to another staged line this frame.
    pub(crate) cut: bool,
    states: HashMap<Entity, State>,
    frame: u64,
    totals: Totals,
}

#[derive(Default)]
struct Totals {
    ms: f64,
    frames: u64,
    bodies: u64,
    points: u64,
    substeps: u64,
    resets: u64,
}

/// Steps the solver of every body the cast posed this frame, and turns its
/// loose bones. Bodies are solved in parallel on the compute pool from
/// `PARALLEL_FROM` of them.
fn solve(
    mut solving: ResMut<Solving>,
    mut transforms: Query<&mut Transform>,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
) {
    let started = Instant::now();
    let solving = &mut *solving;
    solving.frame += 1;
    let (frame, cut) = (solving.frame, std::mem::take(&mut solving.cut));
    let worn = std::mem::take(&mut solving.worn);
    let states = &mut solving.states;
    states.retain(|&root, _| transforms.contains(root));
    for one in &worn {
        let state = states
            .entry(one.root)
            .or_insert_with(|| State::new(&one.wearer));
        if !Arc::ptr_eq(&state.wearer.model, &one.wearer.model)
            || !Arc::ptr_eq(&state.wearer.nodes, &one.wearer.nodes)
        {
            *state = State::new(&one.wearer);
        }
    }
    let index: HashMap<Entity, &Worn> = worn.iter().map(|one| (one.root, one)).collect();
    let mut jobs: Vec<(&Worn, &mut State)> = states
        .iter_mut()
        .filter_map(|(root, state)| Some((*index.get(root)?, state)))
        .collect();
    let world = &transforms;
    match ComputeTaskPool::try_get().filter(|_| jobs.len() >= PARALLEL_FROM) {
        Some(pool) => {
            let size = jobs.len().div_ceil(pool.thread_num() * 4).max(1);
            pool.scope(|scope| {
                for chunk in jobs.chunks_mut(size) {
                    scope.spawn(async move {
                        for (one, state) in chunk {
                            state.step(one, (frame, cut), world);
                        }
                    });
                }
            });
        }
        None => {
            for (one, state) in &mut jobs {
                state.step(one, (frame, cut), world);
            }
        }
    }
    let totals = &mut solving.totals;
    for (_, state) in &jobs {
        totals.bodies += 1;
        totals.points += state.solver.len() as u64;
        totals.substeps += u64::from(state.substeps);
        totals.resets += u64::from(state.reset);
    }
    for (_, state) in jobs {
        for &(entity, turn) in &state.turns {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.rotation = turn;
            }
        }
    }
    totals.ms += started.elapsed().as_secs_f64() * 1000.0;
    totals.frames += 1;
    if dump_due.0 {
        facts.0.insert(
            "loose".into(),
            serde_json::json!({
                "on": running(),
                "ms_sum": totals.ms,
                "frames": totals.frames,
                "bodies_sum": totals.bodies,
                "points_sum": totals.points,
                "substeps_sum": totals.substeps,
                "resets_sum": totals.resets,
            }),
        );
    }
}

/// Robes and capes in play.
pub(crate) struct LoosePartsPlugin;

impl Plugin for LoosePartsPlugin {
    fn build(&self, app: &mut App) {
        solve_in_this_run(wanted());
        app.init_resource::<Solving>().add_systems(
            PostUpdate,
            solve
                .after(crate::play_view::Drawn)
                .after(crate::own_fx::Drawn)
                .after(crate::dialogue_view::lips)
                .after(crate::remote_view::draw)
                .before(bevy::transform::TransformSystems::Propagate),
        );
    }
}
