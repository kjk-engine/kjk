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
//! The options' Cloth physics (`ClothPhysics`) chooses which bodies the
//! solver moves. At Off, the solver is off for the run, as with
//! `KJK_LOOSE=off`. At Low, it moves only the players' own characters. At
//! Medium, it moves the bodies near the camera at the full rate and those
//! farther away at `REDUCED_RATE`, and leaves those far away or out of view
//! to their clips (`Tier`). At High, it moves every body at the full rate.
//! A body that starts or stops being moved blends between its clips and
//! the solver over `BLEND_SECONDS`, its push out of the legs, its folds
//! and its flutter with it (`loose_skin::LegWrites::set`).
//!
//! Low, Medium and High change in play. Off is read once, when the game
//! starts (`off_at_start`). The solver changes the models as they are
//! read: it gives them generated skirt chains (`add_skirt_chains`) and
//! drops their clips' keys on the bones it moves (`dropped`). A model
//! already read keeps those changes until the game starts again. Turned to
//! Off in play, every body blends back to its clips and its push fades,
//! but the robes keep their generated chains and their dropped keys. Turned
//! from Off in play, nothing is moved until the game starts again.
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
use loose::{Body, Frame, Link, Place, Point, Rig, Sample, Settings, Skeleton, Solver};
use vfs::resolver::Resolver;

use crate::loose_view::Loose;
use ragdoll::Template;

pub mod replay;

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

/// Whether the options' Cloth physics was Off when the game started. The
/// settings set it when they are read (`settings::SettingsPlugin`), before
/// any model is read. Nothing turns it back on for the run.
static OFF_AT_START: AtomicBool = AtomicBool::new(false);

pub(crate) fn off_at_start() {
    OFF_AT_START.store(true, Ordering::Relaxed);
}

fn running() -> bool {
    SOLVING.load(Ordering::Relaxed) && !OFF_AT_START.load(Ordering::Relaxed)
}

/// The options' Cloth physics (`display.cloth`): which bodies the solver
/// moves, and how often.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ClothPhysics {
    /// No solver and no push out of the legs. Robes and capes move as in
    /// the original games.
    Off,
    /// Only the players' own characters, at the full rate.
    Low,
    /// The bodies near the camera at the full rate, those farther away at
    /// `REDUCED_RATE`, and none far away or out of view (`Tier`).
    #[default]
    Medium,
    /// Every body at the full rate.
    High,
}

impl ClothPhysics {
    /// The value named by a value from the settings file.
    pub fn named(value: &str) -> ClothPhysics {
        match value {
            "off" => ClothPhysics::Off,
            "low" => ClothPhysics::Low,
            "high" => ClothPhysics::High,
            _ => ClothPhysics::Medium,
        }
    }

    /// Its value in the settings file.
    pub fn name(self) -> &'static str {
        match self {
            ClothPhysics::Off => "off",
            ClothPhysics::Low => "low",
            ClothPhysics::Medium => "medium",
            ClothPhysics::High => "high",
        }
    }
}

/// How a body's robe moves this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    /// Stepped at the solver's full rate, 240 substeps a second.
    Full,
    /// Stepped at `REDUCED_RATE`.
    Reduced,
    /// Not stepped. The robe follows its clips.
    Animated,
}

/// How long a body takes to blend from its clips to the solver, or back
/// (seconds of game time). A quarter of a second is 15 frames at 60 frames
/// a second. A robe that swings 60 degrees from its rest pose turns back
/// about 4 degrees a frame, which reads as the robe settling, not as a
/// jump.
const BLEND_SECONDS: f32 = 0.25;

/// At Medium, a body this close to the camera (metres) is stepped at the
/// full rate. At the game's view angle of 55 degrees, a body of 1.8 m is
/// then at least 14% of the frame's height (150 pixels of 1080), and its
/// robe about 75. The solver's limits against passing through the body
/// were tuned at the full rate, and a robe this large shows a leg through
/// it.
const FULL_WITHIN: f32 = 12.0;

/// At Medium, a body this close to the camera (metres), and farther than
/// `FULL_WITHIN`, is stepped at `REDUCED_RATE`. Its robe is then 35 to 75
/// pixels tall in a frame 1080 pixels high, so a few centimetres of cloth
/// in a leg is one or two pixels. It is within the cast's `POSE_RANGE` of
/// 30 m, past which a body is posed only every few frames and could not be
/// stepped every frame anyway. Past it the robe follows its clips.
const REDUCED_WITHIN: f32 = 25.0;

/// How much farther a body may go (metres) before it drops to the next
/// tier, so that a body that stands at the line does not switch back and
/// forth every frame.
const TIER_MARGIN: f32 = 1.0;

/// The solver's substeps per second for a body at `Tier::Reduced`, half of
/// the full 240. It was tested on the 14 records of play that weigh the
/// solver against passing through the body (`replay::replay_rate`). At 240
/// they gave 1 frame of 12,569 with 10 or more robe corners sunk in the
/// body, and 1,119 frames with 3 or more boot vertices through the robe's
/// back. At 120 they gave 0 and 1,353. At 80 they gave 5 and 1,464. At 120
/// the robe sinks no more, and a boot shows through it in 10.8% of the
/// frames instead of 8.9%. At `FULL_WITHIN` and farther, a few boot
/// vertices through the robe are a pixel or two.
const REDUCED_RATE: f32 = 120.0;

/// The solver's full substeps per second (`loose::Settings::rate`).
const FULL_RATE: f32 = 240.0;

/// At Medium, a body counts as in view while a sphere this large (metres)
/// around its middle touches the camera's view. A body just past the
/// frame's edge is then still moved, so that one that comes into view is
/// already moving.
const IN_VIEW_RADIUS: f32 = 2.5;

/// The levels that `KJK_CLOTH_AT` sets during the run, a debugging aid to
/// record a body blending between its clips and the solver. Each is a game
/// time in seconds and a level, such as `3:off,5:high`.
fn scheduled_levels() -> &'static [(f64, ClothPhysics)] {
    static LEVELS: OnceLock<Vec<(f64, ClothPhysics)>> = OnceLock::new();
    LEVELS.get_or_init(|| {
        let Ok(spec) = std::env::var("KJK_CLOTH_AT") else {
            return Vec::new();
        };
        let mut levels: Vec<(f64, ClothPhysics)> = (spec.split(','))
            .filter_map(|one| {
                let (at, level) = one.split_once(':')?;
                Some((at.trim().parse().ok()?, ClothPhysics::named(level.trim())))
            })
            .collect();
        levels.sort_by(|a, b| a.0.total_cmp(&b.0));
        levels
    })
}

/// Where the camera was last frame, in game space (metres), and what it
/// saw, for Medium's tiers.
pub(crate) struct Seen {
    pub(crate) eye: Vec3,
    pub(crate) sees: Option<bevy::camera::primitives::Frustum>,
}

/// The tier of a body at `level`, standing at `at` (game space, metres).
/// `was` is its tier last frame, and `seen` the camera.
fn tier(level: ClothPhysics, (player, at): (bool, Vec3), seen: Option<&Seen>, was: Tier) -> Tier {
    match level {
        ClothPhysics::Off => Tier::Animated,
        ClothPhysics::Low if player => Tier::Full,
        ClothPhysics::Low => Tier::Animated,
        ClothPhysics::High => Tier::Full,
        ClothPhysics::Medium => {
            let Some(seen) = seen.filter(|_| !player) else {
                return Tier::Full;
            };
            let kept = |tier: Tier| if was <= tier { TIER_MARGIN } else { 0.0 };
            let in_view = seen.sees.is_none_or(|sees| {
                let sphere = bevy::camera::primitives::Sphere {
                    center: (crate::scene::game_to_bevy() * (at + Vec3::Z)).into(),
                    radius: IN_VIEW_RADIUS + kept(Tier::Reduced),
                };
                sees.intersects_sphere(&sphere, true)
            });
            let distance = at.distance(seen.eye);
            if !in_view {
                Tier::Animated
            } else if distance <= FULL_WITHIN + kept(Tier::Full) {
                Tier::Full
            } else if distance <= REDUCED_WITHIN + kept(Tier::Reduced) {
                Tier::Reduced
            } else {
                Tier::Animated
            }
        }
    }
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
                aero: values.aero,
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
    /// The body model's name, lowercase.
    name: String,
    solver: Solver,
    /// The bones the solver turns, and their rest turns.
    bones: Vec<(usize, Quat)>,
    /// The leg capsules of the body: each one's node, its ends in the
    /// node's frame and its radius.
    legs: Vec<(usize, [Vec3; 2], f32)>,
    /// Every capsule of the body, the legs first, as the vertex shader
    /// reads them (`loose_skin`).
    capsules: Vec<(usize, [Vec3; 2], f32)>,
    /// For each mesh of the model, the `loose_skin::PUSH` attribute of a
    /// mesh that the solver moves, or `None`.
    pushes: Vec<Option<Vec<[f32; 4]>>>,
    /// Each point's chain in the solver (`Solver::chains`), held below
    /// `loose_skin::CHAINS`, and how many chains there are.
    chains: (Vec<usize>, usize),
    /// For each skinned mesh of the model, its colours under
    /// `KJK_LOOSE_PAINT` (`paint`).
    paints: Vec<Option<Vec<[f32; 4]>>>,
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
        if rig.points.is_empty() {
            return None;
        }
        let moved: Vec<usize> = simulated(&found).flat_map(bones_of).collect();
        if skin_kept_out() {
            rig.samples = samples(ir, &moved);
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
        .map(|template| {
            let mut body = Body::new(&template);
            for (part, node) in body.feet() {
                let to_node = crate::scene::to_mat4(&ir.nodes[node].world).inverse();
                let points: Vec<Vec3> = (skinned_on(ir, node).into_iter())
                    .map(|p| to_node.transform_point3(p))
                    .collect();
                body.wrap(part, &points);
            }
            body
        });
        let legs: Vec<(usize, [Vec3; 2], f32)> = body
            .iter()
            .flat_map(|body| body.legs())
            .take(crate::loose_skin::LEG_CAPSULES)
            .collect();
        let capsules: Vec<(usize, [Vec3; 2], f32)> = body
            .iter()
            .flat_map(|body| body.capsules())
            .take(crate::loose_skin::CAPSULES)
            .collect();
        let feet_of = body.clone();
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
        let bones = moved.iter().map(|&node| (node, rest[node].2)).collect();
        let chains_of_points = solver.chains();
        let chain_of: HashMap<usize, usize> = (chains_of_points.iter())
            .filter_map(|&(chain, node)| Some((node?, chain)))
            .collect();
        let pushes = pushes(ir, &moved, &capsules, &chain_of);
        let last = crate::loose_skin::CHAINS - 1;
        let chains = (
            chains_of_points
                .iter()
                .map(|&(chain, _)| chain.min(last))
                .collect::<Vec<_>>(),
            chains_of_points
                .iter()
                .map(|&(chain, _)| chain + 1)
                .max()
                .unwrap_or(0)
                .min(last + 1),
        );
        let feet: Vec<usize> = (feet_of.iter())
            .flat_map(|body| body.legs())
            .enumerate()
            .filter(|(index, _)| index % 3 == 2)
            .map(|(_, (node, ..))| node)
            .collect();
        let paints = if crate::loose_skin::painted() {
            paints(ir, &moved, &feet)
        } else {
            Vec::new()
        };
        Some(Model {
            name: ir.name.to_ascii_lowercase(),
            solver,
            bones,
            legs,
            capsules,
            chains,
            pushes,
            paints,
        })
    }

    pub(crate) fn bones(&self) -> &[(usize, Quat)] {
        &self.bones
    }
}

/// The colours of each skinned mesh of `ir` under `KJK_LOOSE_PAINT`: a
/// vertex that the bones `moved` carry by half or more is magenta, one that
/// the feet `feet` carry by half or more is green, and any other is black.
fn paints(ir: &ModelIr, moved: &[usize], feet: &[usize]) -> Vec<Option<Vec<[f32; 4]>>> {
    ir.meshes
        .iter()
        .map(|mesh| {
            let skin = mesh.skin.as_ref()?;
            let share = |vertex: usize, nodes: &[usize]| {
                let (Some(slots), Some(weights)) = (
                    skin.vertex_joints.get(vertex),
                    skin.vertex_weights.get(vertex),
                ) else {
                    return 0.0;
                };
                (slots.iter().zip(weights))
                    .filter(|&(&slot, _)| {
                        (skin.joints.get(usize::from(slot)))
                            .is_some_and(|node| nodes.contains(node))
                    })
                    .map(|(_, &weight)| weight)
                    .sum::<f32>()
            };
            Some(
                (0..mesh.positions.len())
                    .map(|vertex| {
                        if share(vertex, moved) >= 0.5 {
                            [1.0, 0.0, 1.0, 1.0]
                        } else if share(vertex, feet) >= 0.5 {
                            [0.0, 1.0, 0.0, 1.0]
                        } else {
                            [0.0, 0.0, 0.0, 1.0]
                        }
                    })
                    .collect(),
            )
        })
        .collect()
}

/// The colours of mesh `mesh` of `ir` under `KJK_LOOSE_PAINT`, when the
/// solver moves the model (`paints`).
pub(crate) fn paint(ir: &ModelIr, mesh: usize) -> Option<Vec<[f32; 4]>> {
    if !running() || !crate::loose_skin::painted() {
        return None;
    }
    let models = MODELS.lock().unwrap_or_else(|e| e.into_inner());
    let model = models.get(&ir.name.to_ascii_lowercase())?.as_ref()?;
    model.paints.get(mesh)?.clone()
}

/// Every vertex of `ir`'s skinned meshes that node `node` carries with a
/// weight of at least one half, where it rests in model space. A boot is
/// the vertices on its foot.
fn skinned_on(ir: &ModelIr, node: usize) -> Vec<Vec3> {
    let world = |node: usize| crate::scene::to_mat4(&ir.nodes[node].world);
    let mut points = Vec::new();
    for mesh in &ir.meshes {
        let Some(skin) = mesh.skin.as_ref() else {
            continue;
        };
        let binds: Vec<Mat4> = (skin.joints.iter().zip(&skin.inverse_binds))
            .map(|(&node, bind)| world(node) * crate::scene::to_mat4(bind))
            .collect();
        for (vertex, p) in mesh.positions.iter().enumerate() {
            let (Some(slots), Some(weights)) = (
                skin.vertex_joints.get(vertex),
                skin.vertex_weights.get(vertex),
            ) else {
                continue;
            };
            let mut placed = Mat4::ZERO;
            let mut on_node = 0.0;
            for (&slot, &weight) in slots.iter().zip(weights) {
                let slot = usize::from(slot);
                if let Some(bind) = binds.get(slot) {
                    placed += *bind * weight;
                }
                if skin.joints.get(slot) == Some(&node) {
                    on_node += weight;
                }
            }
            if on_node >= 0.5 {
                points.push(placed.transform_point3(Vec3::from_array(*p)));
            }
        }
    }
    points
}

/// The vertices of `ir`'s skinned meshes that the bones `moved` carry by
/// half or more, as the solver's samples of the skin (`loose::Sample`). A
/// vertex that several meshes share, or that rests within a millimetre of
/// another, is taken once.
fn samples(ir: &ModelIr, moved: &[usize]) -> Vec<Sample> {
    let world = |node: usize| crate::scene::to_mat4(&ir.nodes[node].world);
    let mut seen = std::collections::HashSet::new();
    let mut samples = Vec::new();
    for mesh in &ir.meshes {
        let Some(skin) = mesh.skin.as_ref() else {
            continue;
        };
        for (vertex, p) in mesh.positions.iter().enumerate() {
            let (Some(slots), Some(weights)) = (
                skin.vertex_joints.get(vertex),
                skin.vertex_weights.get(vertex),
            ) else {
                continue;
            };
            let p = Vec3::from_array(*p);
            let mut parts = Vec::new();
            let mut placed = Vec3::ZERO;
            for (&slot, &weight) in slots.iter().zip(weights) {
                let slot = usize::from(slot);
                let (Some(&node), Some(bind)) =
                    (skin.joints.get(slot), skin.inverse_binds.get(slot))
                else {
                    continue;
                };
                if weight <= 0.0 {
                    continue;
                }
                let local = crate::scene::to_mat4(bind).transform_point3(p);
                placed += world(node).transform_point3(local) * weight;
                parts.push((node, weight, local));
            }
            let loose: f32 = (parts.iter())
                .filter(|(node, ..)| moved.contains(node))
                .map(|(_, weight, _)| weight)
                .sum();
            if loose < 0.5 || !seen.insert((placed * 1000.0).round().as_ivec3().to_array()) {
                continue;
            }
            samples.push(Sample { parts });
        }
    }
    samples
}

/// Whether the solver keeps samples of the robe's skin out of the body.
/// `KJK_LOOSE_SKIN=off` leaves only the points and bones to collide.
fn skin_kept_out() -> bool {
    static KEPT: OnceLock<bool> = OnceLock::new();
    *KEPT.get_or_init(|| std::env::var("KJK_LOOSE_SKIN").as_deref() != Ok("off"))
}

/// How much nearer to a capsule's axis than at rest a vertex that rests
/// inside the capsule may come before it is pushed (metres).
const INSIDE_MARGIN: f32 = 0.005;

/// The `loose_skin::PUSH` attribute of each mesh of `ir` that the bones
/// `moved` move, from the body capsules `capsules` in the rest pose, the
/// legs first. Every vertex of such a mesh is pushed all the way out, also
/// those the moved bones carry little or not at all. Those are the robe's
/// skin at the hips, carried by the pelvis and the thighs, and in a flip
/// the hip bends so far that the skin blended between them folds into the
/// thigh. A vertex's x is 1 plus a mask of the capsules after the legs that
/// it rests inside, such as the chest for the top of a robe, which never
/// push it. Its y is how far it rests inside the leg capsule it is deepest
/// in, with `loose_skin::CLOTH_OFF_LEG` added to the radius, plus
/// `INSIDE_MARGIN`. Its z is how far down the garment it hangs at rest
/// (metres), from the highest vertex the moved bones carry, times the
/// share they carry of it. Its w packs the two chains (`chain_of`, by node)
/// whose moved bones carry it most, and its share of the second against
/// both, the way `loose_legs.wgsl`'s `cloth_at` reads it.
fn pushes(
    ir: &ModelIr,
    moved: &[usize],
    capsules: &[(usize, [Vec3; 2], f32)],
    chain_of: &HashMap<usize, usize>,
) -> Vec<Option<Vec<[f32; 4]>>> {
    let world = |node: usize| crate::scene::to_mat4(&ir.nodes[node].world);
    let rest: Vec<([Vec3; 2], f32)> = capsules
        .iter()
        .map(|&(node, ends, radius)| {
            let frame = world(node);
            (ends.map(|end| frame.transform_point3(end)), radius)
        })
        .collect();
    let legs = crate::loose_skin::LEG_CAPSULES.min(rest.len());
    ir.meshes
        .iter()
        .map(|mesh| {
            let skin = mesh.skin.as_ref()?;
            let binds: Vec<Mat4> = (skin.joints.iter().zip(&skin.inverse_binds))
                .map(|(&node, bind)| world(node) * crate::scene::to_mat4(bind))
                .collect();
            let mut any = false;
            let mut top = f32::MIN;
            let push: Vec<([f32; 4], f32, f32)> = (mesh.positions.iter().enumerate())
                .map(|(vertex, p)| {
                    let p = Vec3::from_array(*p);
                    let (slots, weights) = (
                        skin.vertex_joints.get(vertex),
                        skin.vertex_weights.get(vertex),
                    );
                    let (Some(slots), Some(weights)) = (slots, weights) else {
                        return ([0.0; 4], 0.0, 0.0);
                    };
                    let mut at = Vec3::ZERO;
                    let mut share = 0.0f32;
                    let mut by_chain = [0.0f32; crate::loose_skin::CHAINS];
                    for (&slot, &weight) in slots.iter().zip(weights) {
                        let slot = usize::from(slot);
                        let (Some(&node), Some(bind)) = (skin.joints.get(slot), binds.get(slot))
                        else {
                            continue;
                        };
                        at += bind.transform_point3(p) * weight;
                        if weight > 0.0 && moved.contains(&node) {
                            any = true;
                            share += weight;
                            let chain = chain_of.get(&node).copied().unwrap_or(0);
                            by_chain[chain.min(crate::loose_skin::CHAINS - 1)] += weight;
                        }
                    }
                    if share > 0.0 {
                        top = top.max(at.z);
                    }
                    let inside_by = |&([a, b], radius): &([Vec3; 2], f32)| {
                        let from_axis = at.distance(ragdoll::closest_on_segment(at, a, b));
                        radius + crate::loose_skin::CLOTH_OFF_LEG - from_axis + INSIDE_MARGIN
                    };
                    let depth = rest[..legs].iter().map(inside_by).fold(0.0, f32::max);
                    let inside = (rest[legs..].iter().enumerate())
                        .filter(|(_, capsule)| inside_by(capsule) > 0.0)
                        .fold(0u32, |mask, (bit, _)| mask | 1 << bit);
                    let mut order: Vec<usize> = (0..by_chain.len()).collect();
                    order.sort_by(|&a, &b| by_chain[b].total_cmp(&by_chain[a]));
                    let (first, second) = (order[0], order[1]);
                    let both = by_chain[first] + by_chain[second];
                    let (second, across) = match by_chain[second] > 0.0 {
                        true => (second, by_chain[second] / both),
                        false => (first, 0.0),
                    };
                    let packed = crate::loose_skin::pack_chains(first, second, across);
                    (
                        [1.0 + inside as f32, depth, 0.0, packed],
                        at.z,
                        share.min(1.0),
                    )
                })
                .collect();
            let push = (push.into_iter())
                .map(|([inside, depth, _, chain], z, share)| {
                    [inside, depth, (top - z).max(0.0) * share, chain]
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
pub(crate) fn push(ir: &ModelIr, mesh: usize) -> Option<Vec<[f32; 4]>> {
    if !running() || !pushing() {
        return None;
    }
    let models = MODELS.lock().unwrap_or_else(|e| e.into_inner());
    let model = models.get(&ir.name.to_ascii_lowercase())?.as_ref()?;
    model.pushes.get(mesh)?.clone()
}

/// How far the hem flutters at `FLUTTER_SPEED` and above (metres).
const FLUTTER_HEIGHT: f32 = 0.025;
/// The speed through still air at which a hem flutters its most (metres per
/// second).
const FLUTTER_SPEED: f32 = 8.0;
/// How many flutters run down a chain each second, and how many more for
/// each metre per second of speed through the air.
const FLUTTER_HERTZ: f32 = 1.0;
const FLUTTER_HERTZ_PER_SPEED: f32 = 0.5;
/// How quickly a chain's flutter and squeeze follow their values (per
/// second).
const RIPPLE_FOLLOW: f32 = 8.0;

/// A value of the cloth's look that a run may change through its variable:
/// its name and its default.
struct Tuned {
    name: &'static str,
    default: f32,
    cell: OnceLock<f32>,
}

/// The share of the deepest fold that the cloth between two chains keeps
/// when nothing squeezes it, `KJK_LOOSE_FOLD_REST`. A robe hangs in soft
/// pleats between the places it is held.
static FOLD_AT_REST: Tuned = Tuned {
    name: "KJK_LOOSE_FOLD_REST",
    default: 0.3,
    cell: OnceLock::new(),
};

/// The squeeze of two chains, as a share of their rest distance, at which
/// the cloth between them folds its deepest, `KJK_LOOSE_FOLD_FULL`. Links
/// between chains shorten by 7% or less nine tenths of the time in the
/// acrobatics record, and by 44% at the most.
static FOLD_FULL: Tuned = Tuned {
    name: "KJK_LOOSE_FOLD_FULL",
    default: 0.2,
    cell: OnceLock::new(),
};

fn tuned(value: &Tuned) -> f32 {
    *value.cell.get_or_init(|| {
        (std::env::var(value.name).ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(value.default)
    })
}

/// A scale for the run from the variable `name`, 1 when it is not set.
fn strength(name: &str, cell: &'static OnceLock<f32>) -> f32 {
    *cell.get_or_init(|| {
        (std::env::var(name).ok())
            .and_then(|value| value.parse().ok())
            .unwrap_or(1.0)
    })
}

fn fold_strength() -> f32 {
    static CELL: OnceLock<f32> = OnceLock::new();
    strength("KJK_LOOSE_FOLDS", &CELL)
}

fn flutter_strength() -> f32 {
    static CELL: OnceLock<f32> = OnceLock::new();
    strength("KJK_LOOSE_FLUTTER", &CELL)
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

/// The animated skeleton that each solved body's solver read, a measuring
/// aid: `KJK_LOOSE_RECORD` (the file). Each step of each body writes one
/// JSON line (`State::write_record`), and `replay` steps the solver again
/// on them without the game, the same way every time.
fn recorded() -> Option<&'static Mutex<std::fs::File>> {
    static RECORDED: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();
    RECORDED
        .get_or_init(|| {
            let path = std::env::var("KJK_LOOSE_RECORD").ok()?;
            (std::fs::File::create(&path))
                .inspect_err(|why| warn!("the robe record {path} does not open: {why}"))
                .ok()
                .map(Mutex::new)
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
    /// Whether the body is a player's own character.
    pub(crate) player: bool,
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
    /// Each chain's ripple, sent to the vertex shader (`loose_skin`).
    ripples: [crate::loose_skin::Ripple; crate::loose_skin::CHAINS],
    /// How much the solver's turns weigh against the clips', from 0 to 1.
    /// The push, the folds and the flutter are scaled by it.
    weight: f32,
    tier: Tier,
    /// The game time the body was last posed at (ms).
    posed: Option<f64>,
}

impl State {
    fn new(wearer: &Wearer, model: usize) -> State {
        State {
            wearing: Wearing::new(&wearer.model),
            wearer: wearer.clone(),
            last: None,
            model,
            ripples: Default::default(),
            weight: 0.0,
            tier: Tier::Animated,
            posed: None,
        }
    }

    /// Moves the body to `tier` for the frame posed at `ms`, and its weight
    /// toward the tier's by the game time since it was last posed. Returns
    /// whether the solver steps it this frame.
    fn enter(&mut self, tier: Tier, ms: f64) -> bool {
        let seconds = self.posed.map_or(0.0, |was| ((ms - was) / 1000.0) as f32);
        self.posed = Some(ms);
        let wanted = if tier == Tier::Animated { 0.0 } else { 1.0 };
        let change = (seconds.max(0.0) / BLEND_SECONDS).min(1.0);
        self.weight += (wanted - self.weight).clamp(-change, change);
        self.tier = tier;
        let rate = if tier == Tier::Reduced {
            REDUCED_RATE
        } else {
            FULL_RATE
        };
        if self.wearing.solver.rate() != rate {
            self.wearing.solver.set_rate(rate);
        }
        tier != Tier::Animated || self.weight > 0.0
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
        self.ripple(seconds.unwrap_or(0.0));
        if let Some(traced) = traced() {
            self.write_trace(traced, worn, (frame, seconds), model);
        }
        if let Some(file) = recorded() {
            self.write_record(file, worn, (seconds, model));
        }
    }

    /// Each chain's folds and flutter after a step of `seconds`, for the
    /// vertex shader. A chain squeezes by how far the links to its
    /// neighbouring chains have shortened, as a share of their rest length.
    /// The cloth between chains always hangs in shallow folds
    /// (`FOLD_AT_REST` of the deepest), and they deepen to the deepest when
    /// the squeeze reaches `FOLD_FULL`.
    /// Its hem flutters with the square of its speed through still air, up
    /// to `FLUTTER_SPEED`, and its flutter runs faster the faster it goes.
    /// Both follow their values over about an eighth of a second, so that
    /// the cloth does not flicker. `KJK_LOOSE_FOLDS` and `KJK_LOOSE_FLUTTER`
    /// scale them for the run, and 0 turns them off.
    fn ripple(&mut self, seconds: f32) {
        let solver = &self.wearing.solver;
        let view = solver.view();
        let airflow = solver.airflow();
        let (of_point, count) = &self.wearer.model.chains;
        let follow = (seconds * RIPPLE_FOLLOW).min(1.0);
        let mut squeezed = [(0.0f32, 0.0f32); crate::loose_skin::CHAINS];
        for (a, b, rest, now) in solver.links() {
            let (ca, cb) = (of_point[a], of_point[b]);
            if ca == cb || rest <= 0.0 {
                continue;
            }
            let squeeze = (1.0 - now / rest).clamp(0.0, 1.0);
            for chain in [ca, cb] {
                squeezed[chain].0 += squeeze;
                squeezed[chain].1 += 1.0;
            }
        }
        for chain in 0..*count {
            let mut points = (0..of_point.len()).filter(|&point| of_point[point] == chain);
            let Some(root) = points.next() else { continue };
            let world = |point: usize| view.anchor.point(view.positions[point]);
            let (mut tip, mut sum, mut middle, mut n) = (root, airflow[root], world(root), 1.0);
            for point in points {
                (tip, sum, middle, n) =
                    (point, sum + airflow[point], middle + world(point), n + 1.0);
            }
            let speed = (sum / n).min(FLUTTER_SPEED);
            let ripple = &mut self.ripples[chain];
            let height = flutter_strength() * FLUTTER_HEIGHT * (speed / FLUTTER_SPEED).powi(2);
            ripple.height += (height - ripple.height) * follow;
            let (total, links) = squeezed[chain];
            let squeeze = if links > 0.0 { total / links } else { 0.0 };
            let (rest, full) = (tuned(&FOLD_AT_REST), tuned(&FOLD_FULL));
            let fold = rest + (1.0 - rest) * (squeeze / full.max(1e-3)).min(1.0);
            ripple.squeeze += (fold_strength() * fold - ripple.squeeze) * follow;
            ripple.way = (world(tip) - world(root)).normalize_or_zero();
            ripple.middle = middle / n;
            let hertz = FLUTTER_HERTZ + FLUTTER_HERTZ_PER_SPEED * speed;
            ripple.phase = (ripple.phase + seconds * std::f32::consts::TAU * hertz)
                % (std::f32::consts::TAU * 64.0);
        }
    }

    /// The step's line of the record (`recorded`): the body's model, its
    /// root entity, the game time, the seconds stepped (`null` for a reset),
    /// the model's place, the floor, and each node's animated frame in its
    /// parent's, seven numbers each (place, then turn).
    fn write_record(
        &self,
        file: &Mutex<std::fs::File>,
        worn: &Worn,
        (seconds, model): (Option<f32>, Frame),
    ) {
        use std::io::Write;
        let frame = |f: &Frame| {
            let (t, q) = (f.translation, f.rotation);
            [t.x, t.y, t.z, q.x, q.y, q.z, q.w]
        };
        let line = serde_json::json!({
            "model": self.wearer.model.name,
            "body": worn.root.to_bits(),
            "ms": worn.ms,
            "dt": seconds,
            "root": frame(&model),
            "floor": worn.floor,
            "locals": self.wearing.locals.iter().flat_map(frame).collect::<Vec<f32>>(),
        });
        let mut file = file.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(file, "{line}");
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
    seen: Option<Seen>,
    states: HashMap<Entity, State>,
    /// The models whose entry of `loose_skin::LEGS` must be emptied.
    freed: Vec<usize>,
    frame: u64,
    totals: Totals,
}

impl Solving {
    /// The bodies the cast posed this frame, whether a cutscene cut to
    /// another staged line, and the camera. `solve` steps them later in the
    /// frame.
    pub(crate) fn posed(&mut self, worn: Vec<Worn>, cut: bool, seen: Option<Seen>) {
        self.worn = worn;
        self.cut = cut;
        self.seen = seen;
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
    reduced: u64,
    blending: u64,
    animated: u64,
}

/// Steps the solver of every body the cast posed this frame that its tier
/// moves (`tier`), and turns its loose bones by the body's weight. Bodies
/// are solved in parallel on the compute pool from `cast::PARALLEL_FROM`
/// of them. A body back on its clips is forgotten, and a body on its clips
/// gets no solver until its tier moves it. Its first step then starts
/// from its target pose.
fn solve(
    mut debug_view: ResMut<DebugView>,
    mut solving: ResMut<Solving>,
    mut transforms: Query<&mut Transform>,
    mut level: ResMut<ClothPhysics>,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
) {
    let started = Instant::now();
    let solving = &mut *solving;
    solving.frame += 1;
    let (frame, cut) = (solving.frame, std::mem::take(&mut solving.cut));
    let worn = std::mem::take(&mut solving.worn);
    let seen = solving.seen.take();
    if let Some(one) = worn.first()
        && let Some(&(_, scheduled)) = (scheduled_levels().iter())
            .rev()
            .find(|(at, _)| *at * 1000.0 <= one.ms)
        && level.set_if_neq(scheduled)
    {
        info!(
            "cloth physics {} at {:.2} s",
            scheduled.name(),
            one.ms / 1000.0
        );
    }
    let states = &mut solving.states;
    let freed = &mut solving.freed;
    states.retain(|&root, state| {
        let kept = transforms.contains(root);
        if !kept {
            freed.push(state.model);
        }
        kept
    });
    let mut index: HashMap<Entity, (&Worn, Tier)> = HashMap::with_capacity(worn.len());
    let mut animated = 0;
    for one in &worn {
        let Ok(placed) = transforms.get(one.root) else {
            continue;
        };
        let was = states
            .get(&one.root)
            .map_or(Tier::Animated, |state| state.tier);
        let tier = tier(*level, (one.player, placed.translation), seen.as_ref(), was);
        if tier == Tier::Animated && !states.contains_key(&one.root) {
            animated += 1;
            continue;
        }
        index.insert(one.root, (one, tier));
        let state = states
            .entry(one.root)
            .or_insert_with(|| State::new(&one.wearer, one.model));
        if !Arc::ptr_eq(&state.wearer.model, &one.wearer.model)
            || !Arc::ptr_eq(&state.wearer.nodes, &one.wearer.nodes)
            || state.model != one.model
        {
            freed.push(state.model);
            *state = State::new(&one.wearer, one.model);
        }
    }
    let mut jobs: Vec<(&Worn, &mut State)> = states
        .iter_mut()
        .filter_map(|(root, state)| {
            let &(one, tier) = index.get(root)?;
            if state.enter(tier, one.ms) {
                Some((one, state))
            } else {
                animated += 1;
                None
            }
        })
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
        totals.reduced += u64::from(state.tier == Tier::Reduced);
        totals.blending += u64::from(state.weight < 1.0);
    }
    totals.animated += animated;
    if let Some((one, state)) = jobs.first().filter(|_| crate::loose_view::overlaid()) {
        debug_view.view = Some(state.wearing.solver.view());
        debug_view.root = Some(one.root);
    }
    for (_, state) in jobs {
        let weight = state.weight;
        for &(entity, turn) in state.wearing.turns() {
            if let Ok(mut transform) = transforms.get_mut(entity) {
                transform.rotation = match weight < 1.0 {
                    true => transform.rotation.slerp(turn, weight),
                    false => turn,
                };
            }
        }
    }
    states.retain(|_, state| {
        let kept = state.tier != Tier::Animated || state.weight > 0.0;
        if !kept {
            freed.push(state.model);
        }
        kept
    });
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
                "reduced_sum": totals.reduced,
                "blending_sum": totals.blending,
                "animated_sum": totals.animated,
                "level": level.name(),
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
        for (capsule, &(node, ends, radius)) in
            capsules.iter_mut().zip(&state.wearer.model.capsules)
        {
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
        writes.set(state.model, capsules, state.ripples, state.weight);
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
            .init_resource::<ClothPhysics>()
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
