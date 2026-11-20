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

/// Gives the body model `ir` its generated skirt chains
/// (`asset_ja::loose::skirt`) while the solver runs. The model cache of
/// play and the viewers (`viewer::cached`) and the model viewer call it
/// once, as they read a model, before anything looks at the model's nodes.
/// The menus and the effects read their models without it, and the solver
/// does not move them. With the solver off, a KOTOR 2 robe keeps its robe bones
/// and their keys, and a KOTOR 1 robe stays on its legs.
pub(crate) fn add_skirt_chains(ir: &mut ModelIr) {
    if !running() {
        return;
    }
    if let Some(skirt) = finder::skirt::generate(ir) {
        debug!("{}: {}", ir.name, skirt.describe());
    }
}

/// Turns the solver on or off for a viewer's run, from its loose part
/// options, before it reads its model (`add_skirt_chains`). A viewer without them
/// leaves the run as it was.
pub(crate) fn solve_in_viewer(loose: Option<&Loose>) {
    if let Some(loose) = loose {
        solve_in_this_run(loose.sim);
    }
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
    /// The leg capsules of the body: each one's node, its ends in the
    /// node's frame and its radius (`loose_skin`).
    legs: Vec<(usize, [Vec3; 2], f32)>,
    /// For each mesh of the model, the `loose_skin::PUSH` attribute of a
    /// mesh that the solver moves, or `None`.
    pushes: Vec<Option<Vec<[f32; 2]>>>,
}

impl Model {
    /// The solver for `ir`'s robe skirts and capes, or `None` when it has
    /// none. `clips` are the clips that play on the model. They set how far
    /// each point may stray (`asset_ja::loose::physics`). The points feel
    /// the pelvis's movement through the world only in part, and the rest
    /// of the body's in full.
    pub(crate) fn new(ir: &ModelIr, clips: &[&AnimationIr]) -> Option<Model> {
        let found = finder::rig(ir, clips);
        let rig = solver_rig(&found, &SIMULATED);
        if rig.points.is_empty() {
            return None;
        }
        let Some(pelvis) = ir.node_index(asset_odyssey::hooks::PELVIS) else {
            warn!("{}: no robe solver without a pelvis", ir.name);
            return None;
        };
        let rest = crate::cast::rest_of(ir);
        let skeleton = Skeleton {
            parents: rest.iter().map(|(parent, ..)| *parent).collect(),
            rest: rest
                .iter()
                .map(|&(_, translation, rotation)| Frame::new(translation, rotation))
                .collect(),
            anchor: pelvis,
        };
        let body = crate::ragdolls::fitted(ir, &rest, |skeleton| {
            Template::fit(&ragdoll::humanoid::PLAN, skeleton)
        })
        .inspect_err(|why| warn!("{}: robes without the body's capsules: {why}", ir.name))
        .ok()
        .map(|template| Body::new(&template));
        let legs: Vec<(usize, [Vec3; 2], f32)> = body
            .iter()
            .flat_map(|body| body.legs())
            .take(crate::loose_skin::CAPSULES)
            .collect();
        let settings = Settings {
            trace: traced().is_some(),
            ..Settings::default()
        };
        let solver = Solver::new(&rig, &skeleton, body, settings)
            .inspect_err(|why| warn!("{}: no robe solver: {why}", ir.name))
            .ok()?;
        if settings.trace {
            info!("loose trace {}:\n{}", ir.name, solver.describe());
        }
        let moved: Vec<usize> = simulated(&found).flat_map(bones_of).collect();
        let bones = moved.iter().map(|&node| (node, rest[node].2)).collect();
        let pushes = pushes(ir, &moved, &legs);
        Some(Model {
            solver,
            bones,
            legs,
            pushes,
        })
    }

    pub(crate) fn bones(&self) -> &[(usize, Quat)] {
        &self.bones
    }
}

/// How much nearer to a capsule's axis than at rest a vertex that rests
/// inside the capsule may come before it is pushed (metres).
const INSIDE_MARGIN: f32 = 0.005;

/// The `loose_skin::PUSH` attribute of each mesh of `ir` that the bones
/// `moved` move, from the leg capsules `legs` in the rest pose. A vertex's
/// share is the sum of its weights on the moved bones. Its depth is how far
/// it rests inside the capsule it is deepest in, with `loose_skin::CLOTH_OFF_LEG`
/// added to the radius, plus `INSIDE_MARGIN`.
fn pushes(
    ir: &ModelIr,
    moved: &[usize],
    legs: &[(usize, [Vec3; 2], f32)],
) -> Vec<Option<Vec<[f32; 2]>>> {
    let world = |node: usize| crate::scene::to_mat4(&ir.nodes[node].world);
    let rest: Vec<([Vec3; 2], f32)> = legs
        .iter()
        .map(|&(node, ends, radius)| {
            let frame = world(node);
            (ends.map(|end| frame.transform_point3(end)), radius)
        })
        .collect();
    ir.meshes
        .iter()
        .map(|mesh| {
            let skin = mesh.skin.as_ref()?;
            let binds: Vec<Mat4> = (skin.joints.iter().zip(&skin.inverse_binds))
                .map(|(&node, bind)| world(node) * crate::scene::to_mat4(bind))
                .collect();
            let mut any = false;
            let push = (mesh.positions.iter().enumerate())
                .map(|(vertex, p)| {
                    let p = Vec3::from_array(*p);
                    let (slots, weights) = (
                        skin.vertex_joints.get(vertex),
                        skin.vertex_weights.get(vertex),
                    );
                    let (Some(slots), Some(weights)) = (slots, weights) else {
                        return [0.0, 0.0];
                    };
                    let mut share = 0.0;
                    let mut at = Vec3::ZERO;
                    for (&slot, &weight) in slots.iter().zip(weights) {
                        let slot = usize::from(slot);
                        let (Some(&node), Some(bind)) = (skin.joints.get(slot), binds.get(slot))
                        else {
                            continue;
                        };
                        at += bind.transform_point3(p) * weight;
                        if moved.contains(&node) {
                            share += weight;
                        }
                    }
                    if share <= 0.0 {
                        return [0.0, 0.0];
                    }
                    any = true;
                    let depth = rest
                        .iter()
                        .map(|&([a, b], radius)| {
                            let from_axis = at.distance(ragdoll::closest_on_segment(at, a, b));
                            radius + crate::loose_skin::CLOTH_OFF_LEG - from_axis + INSIDE_MARGIN
                        })
                        .fold(0.0, f32::max);
                    [share.min(1.0), depth]
                })
                .collect();
            any.then_some(push)
        })
        .collect()
}

/// The `loose_skin::PUSH` attribute of mesh `mesh` of `ir`, when the solver
/// moves it (`Model::pushes`). It is `None` while the solver or the push
/// is off, for a model it does not move and for a mesh with no vertex on a
/// moved bone.
pub(crate) fn push(ir: &ModelIr, mesh: usize) -> Option<Vec<[f32; 2]>> {
    if !running() || !pushing() {
        return None;
    }
    let models = MODELS.lock().unwrap_or_else(|e| e.into_inner());
    let model = models.get(&ir.name.to_ascii_lowercase())?.as_ref()?;
    model.pushes.get(mesh)?.clone()
}

/// Whether robes and capes are pushed out of the legs on the GPU in this
/// run. `KJK_LOOSE_PUSH=off` turns it off, to weigh its cost in a bench.
fn pushing() -> bool {
    static PUSHING: OnceLock<bool> = OnceLock::new();
    *PUSHING.get_or_init(|| std::env::var("KJK_LOOSE_PUSH").as_deref() != Ok("off"))
}

/// The robe and cape values changed for the run, a tuning aid:
/// `KJK_LOOSE_TUNE` (`asset_ja::loose::physics::tune`).
fn tune() {
    let Ok(spec) = std::env::var("KJK_LOOSE_TUNE") else {
        return;
    };
    let not_understood = finder::physics::tune(&spec);
    info!("robes and capes tuned: {spec}");
    if !not_understood.is_empty() {
        warn!("KJK_LOOSE_TUNE does not understand {not_understood:?}");
    }
}

/// Each body model's solver, by model name (`worn`).
static MODELS: Mutex<BTreeMap<String, Option<Arc<Model>>>> = Mutex::new(BTreeMap::new());

/// The robe solver's trace, a debugging aid, from `KJK_LOOSE_TRACE` (the
/// file), `KJK_LOOSE_TRACE_FROM` and `KJK_LOOSE_TRACE_TO` (the span of game
/// time, in seconds). Each step of each body then writes one JSON line,
/// and each of its substeps within the span writes another
/// (`loose::Settings::trace`). The log lists each traced model's points
/// and what they meet.
struct Trace {
    file: Mutex<std::fs::File>,
    from: f64,
    to: f64,
}

fn traced() -> Option<&'static Trace> {
    static TRACED: OnceLock<Option<Trace>> = OnceLock::new();
    TRACED
        .get_or_init(|| {
            let path = std::env::var("KJK_LOOSE_TRACE").ok()?;
            let file = (std::fs::OpenOptions::new().create(true).append(true))
                .open(&path)
                .inspect_err(|why| warn!("the robe trace {path} does not open: {why}"))
                .ok()?;
            let seconds = |name: &str| std::env::var(name).ok()?.parse::<f64>().ok();
            Some(Trace {
                file: Mutex::new(file),
                from: seconds("KJK_LOOSE_TRACE_FROM").unwrap_or(0.0),
                to: seconds("KJK_LOOSE_TRACE_TO").unwrap_or(f64::MAX),
            })
        })
        .as_ref()
}

/// The points of one body solved this frame and its root entity, for the
/// overlay (`loose_view::debug_overlay`). With several bodies, which one it
/// is may change from frame to frame.
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
    solve_in_viewer(loose);
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
    /// The body's model in the scene's models. Its meshes carry it in their
    /// tag, and its leg capsules have that entry of `loose_skin::LEGS`.
    pub(crate) model: usize,
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
    /// The body's model in the scene's models (`Worn::model`).
    model: usize,
}

impl State {
    fn new(wearer: &Wearer, model: usize) -> State {
        State {
            wearing: Wearing::new(&wearer.model),
            wearer: wearer.clone(),
            last: None,
            model,
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
        if let Some(traced) = traced() {
            self.write_trace(traced, worn, (frame, seconds), model);
        }
    }

    /// The frame's line of the trace, and the lines of its substeps when
    /// the game time is within the trace's span. The frame's line gives the
    /// body's root entity, the game time, the model's place and facing, and
    /// the pelvis's place and turn in the world.
    fn write_trace(
        &self,
        trace: &Trace,
        worn: &Worn,
        (frame, seconds): (u64, Option<f32>),
        model: Frame,
    ) {
        use std::io::Write;
        let mut file = trace.file.lock().unwrap_or_else(|e| e.into_inner());
        let pelvis = self.wearing.solver.view().anchor;
        let (axis, angle) = pelvis.rotation.to_axis_angle();
        let facing = model.rotation * Vec3::Y;
        let v = |p: Vec3| format!("[{:.4},{:.4},{:.4}]", p.x, p.y, p.z);
        let _ = writeln!(
            file,
            "{{\"body\":{},\"frame\":{frame},\"ms\":{:.2},\"dt\":{},\"substeps\":{},\"reset\":{},\"floor\":{},\"model\":{},\"yaw\":{:.2},\"root\":{},\"root_axis\":[{:.3},{:.3},{:.3}],\"root_angle\":{:.2}}}",
            worn.root.index(),
            worn.ms,
            seconds.map_or("null".into(), |s| format!("{s:.5}")),
            self.wearing.substeps,
            self.wearing.reset,
            worn.floor.map_or("null".into(), |f| format!("{f:.4}")),
            v(model.translation),
            facing.y.atan2(facing.x).to_degrees(),
            v(pelvis.translation),
            axis.x,
            axis.y,
            axis.z,
            angle.to_degrees()
        );
        if (trace.from..=trace.to).contains(&(worn.ms / 1000.0)) {
            for line in self.wearing.solver.trace() {
                let _ = writeln!(file, "{line}");
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
    /// The models whose entry of `loose_skin::LEGS` must be emptied.
    freed: Vec<usize>,
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
    let freed = &mut solving.freed;
    states.retain(|&root, state| {
        let kept = transforms.contains(root);
        if !kept {
            freed.push(state.model);
        }
        kept
    });
    for one in &worn {
        let state = states
            .entry(one.root)
            .or_insert_with(|| State::new(&one.wearer, one.model));
        if !Arc::ptr_eq(&state.wearer.model, &one.wearer.model)
            || !Arc::ptr_eq(&state.wearer.nodes, &one.wearer.nodes)
            || state.model != one.model
        {
            solving.freed.push(state.model);
            *state = State::new(&one.wearer, one.model);
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
    if let Some((one, state)) = jobs.first().filter(|_| crate::loose_view::overlaid()) {
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

/// Sends the leg capsules of every solved body in the world to its model's
/// entry of `loose_skin::LEGS`, and empties the entries of the models
/// freed. It runs after Bevy has placed every node, so the capsules are
/// where the skin puts the legs.
fn place_legs(
    mut solving: ResMut<Solving>,
    globals: Query<&GlobalTransform>,
    mut writes: ResMut<crate::loose_skin::LegWrites>,
) {
    use crate::loose_skin::{CAPSULES, LegCapsule};
    let solving = &mut *solving;
    for model in solving.freed.drain(..) {
        writes.free(model);
    }
    for state in solving.states.values() {
        let mut capsules = [LegCapsule::NONE; CAPSULES];
        for (capsule, &(node, ends, radius)) in capsules.iter_mut().zip(&state.wearer.model.legs) {
            let Some(global) = (state.wearer.nodes.get(node)).and_then(|&n| globals.get(n).ok())
            else {
                continue;
            };
            *capsule = LegCapsule {
                ends: ends.map(|end| global.transform_point(end)),
                radius,
                scale: global.affine().matrix3.x_axis.length(),
            };
        }
        writes.set(state.model, capsules);
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
        tune();
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
            )
            .add_systems(
                PostUpdate,
                place_legs.after(bevy::transform::TransformSystems::Propagate),
            );
    }
}
