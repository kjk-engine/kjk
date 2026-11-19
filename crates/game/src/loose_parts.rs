//! Robe skirts and capes moved by the robe and cape solver (`loose`) in
//! play, for https://github.com/kjk-engine/roadmap/issues/5. The solver
//! runs in the view only and never reaches the sim.
//!
//! - The loose bone finder (`asset_ja::loose`) reads each body model once,
//!   when it is loaded, with the clips of its supermodel chain (`worn`).
//!   `solver_rig` turns what it finds into the solver's input. Only the
//!   kinds of garment in `SIMULATED` are moved. Every other loose bone keeps
//!   its clips' keys.
//! - While the solver runs, a clip bound to a body drops its keys on the
//!   moved bones (`dropped`), unless the clip is a flourish. The cast also
//!   puts those bones back at rest every frame instead of holding them
//!   (`cast::held_nodes`). The target pose of a moved bone is therefore its
//!   rest pose under its animated parent, or the flourish's keys. The
//!   solver then turns it part of the way toward straight down
//!   (`loose::Point::hang`).
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
//! their keys on every loose bone.
//!
//! What the finder finds, and each model's solver, are kept once per model
//! name for the whole process. One process plays one game, so a name names
//! one model.

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
use vfs::resolver::Resolver;

use crate::loose_view::Loose;
use ragdoll::Template;

/// The kinds of garment the solver moves. Hoods, sleeves, flaps and armour
/// keep playing their clips' keys until they are tuned for the solver.
const SIMULATED: [Kind; 2] = [Kind::Cape, Kind::RobeSkirt];

pub(crate) fn wanted() -> bool {
    static WANTED: OnceLock<bool> = OnceLock::new();
    *WANTED.get_or_init(|| std::env::var("KJK_LOOSE").as_deref() != Ok("off"))
}

/// Whether a solver moves the loose bones in this run. The game sets it
/// from `wanted` when its plugin is built, and the viewers from `--sim`
/// when they load their model (`viewed`). Both happen before any clip is
/// bound.
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

/// The nodes of `ir` whose keys in `clip` are dropped when it is bound.
/// They are the bones the solver moves, unless `clip` is a flourish.
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

/// The solver's input for the trees of `found` whose kind is in `kinds`.
/// The finder gives each point's values in the solver's own terms. Its
/// links name a tree and a point, which become the point's number in the
/// solver's one list. A link to a tree that is left out is dropped. The
/// finder links only strands of the same kind, so no link is lost between
/// two kept trees.
fn solver_rig(found: &finder::Rig, kinds: &[Kind]) -> Rig {
    let mut first = vec![None; found.trees.len()];
    let mut count = 0;
    for (index, tree) in found.trees.iter().enumerate() {
        if kinds.contains(&tree.kind) {
            first[index] = Some(count);
            count += tree.points.len();
        }
    }
    let flat = |(tree, point): (usize, usize)| first[tree].map(|first| first + point);
    let mut rig = Rig::default();
    for (tree, base) in found.trees.iter().zip(&first) {
        let Some(base) = *base else { continue };
        for point in &tree.points {
            let place = match point.parent.filter(|_| point.tip) {
                Some(joint) => Place::Tip {
                    point: base + joint,
                    offset: Vec3::from_array(point.offset),
                },
                None => Place::Joint(point.node),
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
                hang: values.hang,
                reach: point.reach.clone(),
            });
        }
    }
    rig.links = (found.links.iter())
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
    /// each point may stray (`asset_ja::loose::physics`). The points feel
    /// the pelvis's movement through the world only in part, and the rest
    /// of the body's in full.
    pub(crate) fn new(ir: &ModelIr, clips: &[&AnimationIr]) -> Option<Model> {
        let found = finder::rig(ir, clips);
        let mut rig = solver_rig(&found, &SIMULATED);
        let env = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<f32>().ok());
        for point in &mut rig.points {
            if let Some(m) = env("KJK_LOOSE_DRIVE_MUL") {
                point.drive = point.drive.map(|d| d * m);
            }
            if let Some(m) = env("KJK_LOOSE_DAMP_MUL") {
                point.damping *= m;
            }
            if let Some(v) = env("KJK_LOOSE_INERTIA") {
                point.inertia = v;
            }
            if let Some(m) = env("KJK_LOOSE_MAXD_MUL") {
                point.max_distance *= m;
            }
        }
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
            anchor: ir.node_index(asset_odyssey::hooks::PELVIS).unwrap_or(0),
        };
        let body = crate::ragdolls::fitted(ir, &rest, |skeleton| {
            Template::fit(&ragdoll::humanoid::PLAN, skeleton)
        })
        .inspect_err(|why| warn!("{}: robes without the body's capsules: {why}", ir.name))
        .ok()
        .map(|template| Body::new(&template));
        let solver = Solver::new(&rig, &skeleton, body, debug_settings())
            .inspect_err(|why| warn!("{}: no robe solver: {why}", ir.name))
            .ok()?;
        info!(
            "loose debug {}: settings {:?}\n{}\nnodes: {}",
            ir.name,
            debug_settings(),
            solver.describe(),
            ir.nodes
                .iter()
                .enumerate()
                .map(|(i, n)| format!("{i}={}", n.name))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let bones = simulated(&found)
            .flat_map(bones_of)
            .map(|node| (node, rest[node].2))
            .collect();
        Some(Model { solver, bones })
    }

    pub(crate) fn bones(&self) -> &[(usize, Quat)] {
        &self.bones
    }
}

/// The solver's settings, changed by the diagnosis switches.
fn debug_settings() -> Settings {
    let env = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<f32>().ok());
    let mut s = Settings::default();
    if let Some(v) = env("KJK_LOOSE_RATE") {
        s.rate = v;
    }
    if let Some(v) = env("KJK_LOOSE_MOST") {
        s.most = v as u32;
    }
    if let Some(v) = env("KJK_LOOSE_PASSES") {
        s.passes = v as u32;
    }
    if let Some(v) = env("KJK_LOOSE_TRAVEL") {
        s.anchor_travel = v;
    }
    if let Some(v) = env("KJK_LOOSE_TURN") {
        s.anchor_turn = v.to_radians();
    }
    if let Some(v) = env("KJK_LOOSE_MAXSTEP") {
        s.max_step = v;
    }
    let off = std::env::var("KJK_LOOSE_NO").unwrap_or_default();
    let has = |w: &str| off.split(',').any(|x| x == w);
    s.debug.segments = !has("segments");
    s.debug.legs = !has("legs");
    s.debug.backstop = !has("backstop");
    s.debug.drive = !has("drive");
    s.debug.collide = !has("collide");
    s.debug.trace = std::env::var("KJK_LOOSE_TRACE").is_ok();
    s
}

/// The traced body's solver view and its root entity, for the overlay.
#[derive(Resource, Default)]
pub(crate) struct DebugView {
    pub(crate) view: Option<loose::View>,
    pub(crate) root: Option<Entity>,
}

/// The solver of the body model `ir` in play, or `None` while the solver
/// is off or when the model has no robe skirt or cape. It is made once per
/// model name, from the clips of the model's supermodel chain (`cache` and
/// `resolver` read it), so every body that wears the model gets the same
/// values.
pub(crate) fn worn(
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    ir: &ModelIr,
) -> Result<Option<Arc<Model>>, String> {
    static MODELS: Mutex<BTreeMap<String, Option<Arc<Model>>>> = Mutex::new(BTreeMap::new());
    if !running() || found(ir).moved.is_empty() {
        return Ok(None);
    }
    let name = ir.name.to_ascii_lowercase();
    if let Some(model) = MODELS.lock().unwrap_or_else(|e| e.into_inner()).get(&name) {
        return Ok(model.clone());
    }
    let chain = crate::viewer::chain_clips(cache, resolver, ir)?;
    let clips: Vec<&AnimationIr> = chain.iter().collect();
    let model = Model::new(ir, &clips).map(Arc::new);
    MODELS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(name, model.clone());
    Ok(model)
}

/// The options of a viewer that shows `ir`, with the clips of its
/// supermodel chain when `--sim` asks for the solver. A viewer with any of
/// the loose part options turns the solver on or off for the run here. It
/// must call this before it binds any clip, because binding drops the keys
/// of the bones the solver moves.
pub(crate) fn viewed(
    loose: Option<&Loose>,
    cache: &mut BTreeMap<String, Option<ModelIr>>,
    resolver: &Resolver,
    ir: &ModelIr,
) -> Result<Option<Loose>, String> {
    let sim = loose.is_some_and(|loose| loose.sim);
    if loose.is_some() {
        solve_in_this_run(sim);
    }
    Ok(match loose {
        Some(loose) if sim => Some(Loose {
            chain: crate::viewer::chain_clips(cache, resolver, ir)?,
            ..loose.clone()
        }),
        loose => loose.cloned(),
    })
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

/// One body's solver between frames, in play or in a viewer, and the turns
/// of its loose bones from its last step.
pub(crate) struct Wearing {
    solver: Solver,
    locals: Vec<Frame>,
    turns: Vec<(Entity, Quat)>,
    substeps: u32,
    reset: bool,
}

impl Wearing {
    pub(crate) fn new(model: &Model) -> Wearing {
        Wearing {
            solver: model.solver.clone(),
            locals: Vec::new(),
            turns: Vec::new(),
            substeps: 0,
            reset: false,
        }
    }

    /// One step of `seconds` of game time. With `None`, the points go back
    /// to their target pose instead. `model` is the model's place in the
    /// world and `floor` the floor's height. `nodes` are the body's node
    /// entities, and `local` reads one's animated transform.
    pub(crate) fn step(
        &mut self,
        seconds: Option<f32>,
        (model, floor): (Frame, Option<f32>),
        nodes: &[Entity],
        local: impl Fn(Entity) -> Option<Frame>,
    ) {
        self.locals.clear();
        (self.locals).extend(
            nodes
                .iter()
                .map(|&node| local(node).unwrap_or(Frame::IDENTITY)),
        );
        if seconds.is_none() {
            self.solver.reset();
        }
        let step = (self.solver).step(seconds.unwrap_or(0.0), model, &self.locals, floor);
        self.substeps = step.substeps;
        self.reset = step.reset;
        self.turns.clear();
        self.turns.extend(
            (step.turns.iter()).filter_map(|&(node, turn)| Some((*nodes.get(node)?, turn))),
        );
    }

    /// Each loose bone's entity and its new local turn.
    pub(crate) fn turns(&self) -> &[(Entity, Quat)] {
        &self.turns
    }

    pub(crate) fn points(&self) -> usize {
        self.solver.len()
    }

    pub(crate) fn substeps(&self) -> u32 {
        self.substeps
    }

    pub(crate) fn reset(&self) -> bool {
        self.reset
    }
}

/// A body's solver in play, with the frame and the game time (ms) it was
/// last solved at.
struct State {
    wearer: Wearer,
    wearing: Wearing,
    last: Option<(u64, f64)>,
}

impl State {
    fn new(wearer: &Wearer) -> State {
        State {
            wearing: Wearing::new(&wearer.model),
            wearer: wearer.clone(),
            last: None,
        }
    }

    /// A step to the game time of `worn` in frame `frame`. The points go
    /// back to their target pose after a frame the body was not solved in,
    /// and on a cut.
    fn step(&mut self, worn: &Worn, (frame, cut): (u64, bool), transforms: &Query<&mut Transform>) {
        let Ok(placed) = transforms.get(worn.root) else {
            return;
        };
        let seconds = self
            .last
            .filter(|&(last, _)| last + 1 == frame && !cut)
            .map(|(_, ms)| ((worn.ms - ms) / 1000.0) as f32);
        self.last = Some((frame, worn.ms));
        let model = Frame::new(placed.translation, placed.rotation);
        self.wearing
            .step(seconds, (model, worn.floor), &self.wearer.nodes, |node| {
                let t = transforms.get(node).ok()?;
                Some(Frame::new(t.translation, t.rotation))
            });
        if let Ok(path) = std::env::var("KJK_LOOSE_TRACE") {
            use std::io::Write;
            let from = std::env::var("KJK_LOOSE_TRACE_FROM")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0);
            let to = std::env::var("KJK_LOOSE_TRACE_TO")
                .ok()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(1e9);
            let inside = worn.ms / 1000.0 >= from && worn.ms / 1000.0 <= to;
            if let Ok(mut file) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
            {
                let r = self.wearing.solver.view().anchor;
                let (axis, angle) = r.rotation.to_axis_angle();
                let _ = writeln!(
                    file,
                    "{{\"frame\":{frame},\"ms\":{:.2},\"dt\":{},\"substeps\":{},\"reset\":{},\"floor\":{:?},\"model\":[{:.4},{:.4},{:.4}],\"yaw\":{:.2},\"root\":[{:.4},{:.4},{:.4}],\"root_axis\":[{:.3},{:.3},{:.3}],\"root_angle\":{:.2}}}",
                    worn.ms,
                    seconds.map_or("null".into(), |s| format!("{s:.5}")),
                    self.wearing.substeps,
                    self.wearing.reset,
                    worn.floor,
                    model.translation.x,
                    model.translation.y,
                    model.translation.z,
                    {
                        let f = model.rotation * Vec3::Y;
                        f.y.atan2(f.x).to_degrees()
                    },
                    r.translation.x,
                    r.translation.y,
                    r.translation.z,
                    axis.x,
                    axis.y,
                    axis.z,
                    angle.to_degrees()
                );
                if inside {
                    for line in &self.wearing.solver.trace {
                        let _ = writeln!(file, "{line}");
                    }
                }
            }
        }
    }
}

/// The bodies the cast posed this frame, with what the solver did over
/// the run, for the dumps.
#[derive(Resource, Default)]
pub(crate) struct Solving {
    worn: Vec<Worn>,
    cut: bool,
    states: HashMap<Entity, State>,
    frame: u64,
    totals: Totals,
}

impl Solving {
    /// The bodies the cast posed this frame, and whether a cutscene cut to
    /// another staged line. `solve` steps them later in the frame.
    pub(crate) fn posed(&mut self, worn: Vec<Worn>, cut: bool) {
        self.worn = worn;
        self.cut = cut;
    }
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
/// `cast::PARALLEL_FROM` of them.
fn solve(
    mut debug_view: ResMut<DebugView>,
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
    match ComputeTaskPool::try_get().filter(|_| jobs.len() >= crate::cast::PARALLEL_FROM) {
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
        totals.points += state.wearing.points() as u64;
        totals.substeps += u64::from(state.wearing.substeps());
        totals.resets += u64::from(state.wearing.reset());
    }
    if let Some((one, state)) = jobs.first() {
        debug_view.view = Some(state.wearing.solver.view());
        debug_view.root = Some(one.root);
    }
    for (_, state) in jobs {
        for &(entity, turn) in state.wearing.turns() {
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

/// Every system that writes the transforms of a body's bones. The solver
/// reads the final pose after all of them.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PosesBones;

/// Robes and capes in play.
pub(crate) struct LoosePartsPlugin;

impl Plugin for LoosePartsPlugin {
    fn build(&self, app: &mut App) {
        solve_in_this_run(wanted());
        app.init_resource::<Solving>()
            .init_resource::<DebugView>()
            .add_systems(
                PostUpdate,
                crate::loose_view::debug_overlay
                    .after(bevy::transform::TransformSystems::Propagate),
            )
            .configure_sets(
                PostUpdate,
                (crate::play_view::Drawn, crate::own_fx::Drawn).in_set(PosesBones),
            )
            .add_systems(
                PostUpdate,
                solve
                    .after(PosesBones)
                    .before(bevy::transform::TransformSystems::Propagate),
            );
    }
}
