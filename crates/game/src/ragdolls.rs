//! Deaths as active ragdolls (`ragdoll`). The moment a character's
//! snapshots say it died, its body becomes solids joined at its bones, in
//! the pose drawn that frame and moving as its character moved, struck by
//! the blow that killed it. Its death animation keeps playing as the
//! target (a humanoid's JA death, `combat::death` picked it for the hit; a
//! beast's or droid's own KOTOR dying clip): at first the body is held to
//! it and its muscles pull its joints after it, so it staggers and falls
//! as the animation says; then the hold lets go, the muscles tire, and by
//! the time it is down the body is limp and lies as the floor, the stairs
//! and the walls let it. A droid is metal: stiff joints whose servos lock
//! where they stop. Bodies at rest sleep and cost nothing.
//!
//! View-side only: the sim's corpse (its origin, its hits) does not move
//! with it.

use std::collections::BTreeMap;
use std::sync::Arc;

use asset_odyssey::model::ModelIr;
use bevy::prelude::*;
use collision::{Contents, UNIT};
use ragdoll::{Frame, Material, Muscles, Template};
use sim::combat::fighter::Outcome;
use sim::combat::{Delivery, Fighter};
use sim::{SimId, Snapshot};

use crate::cast::{self, Blended, BodyRig, Rest};
use crate::severing;

/// How far around a falling body the level's faces are taken in (metres):
/// across, below and above its pelvis; and how far it may go before more
/// are.
const GATHER: [f32; 3] = [5.0, 6.0, 3.0];
const REGATHER: f32 = 2.5;
/// A death animation of unknown length is taken to last this long (ms).
const DEATH_MS: f64 = 1500.0;
/// The muscles let go over the death animation (shares of its length):
/// the hold, from `HOLD` at once to nothing by `HOLD_GONE`; the tone, full
/// until `TONE_TIRES`, `TONE_DOWN` by its end, nothing `LIMP_MS` after.
const HOLD: f32 = 0.85;
const HOLD_GONE: f32 = 0.6;
const TONE_TIRES: f32 = 0.45;
const TONE_DOWN: f32 = 0.3;
const LIMP_MS: f64 = 700.0;
/// How much of the killing blow (`blow`) is upward.
const BLOW_LIFT: f32 = 0.15;

/// How strongly a body still follows its death animation `since_ms` into
/// it (`length_ms` long).
fn muscles(since_ms: f64, length_ms: f64) -> Muscles {
    let t = (since_ms / length_ms.max(1.0)) as f32;
    let ease = |from: f32, to: f32| {
        let x = ((t - from) / (to - from)).clamp(0.0, 1.0);
        x * x * (3.0 - 2.0 * x)
    };
    let tone = if t < 1.0 {
        1.0 - (1.0 - TONE_DOWN) * ease(TONE_TIRES, 1.0)
    } else {
        TONE_DOWN * (1.0 - ((since_ms - length_ms) / LIMP_MS) as f32).max(0.0)
    };
    Muscles {
        hold: HOLD * (1.0 - ease(0.0, HOLD_GONE)),
        tone,
    }
}

/// How hard a weapon's killing blow throws a body (N·s).
fn blow(weapon: Option<Delivery>) -> f32 {
    match weapon {
        Some(Delivery::Saber(_)) => 32.0,
        Some(Delivery::Melee) => 26.0,
        Some(Delivery::Blaster(_)) => 24.0,
        Some(Delivery::Unarmed | Delivery::Baton) => 16.0,
        None => 0.0,
    }
}

/// How far back from its hit a killing stroke's pass by a joint is looked
/// for (ms), how near the joint a blade must have passed to have cut there
/// (metres), and the most a severed limb is put on its way for being seen
/// late (s).
const PASS_MS: f64 = 250.0;
const PASS_NEAR: f32 = 0.5;
const AHEAD: f64 = 0.05;
/// How much history the view keeps (ms): what a moment is blended from, and
/// a killing stroke's pass before its hit. A hit older than this cuts
/// nothing (a body first seen dead long after, by a player joining).
pub const KEPT_MS: f64 = PASS_MS + 100.0;

/// A blade's pass by a joint: when it was nearest (sim ms), how the blade
/// there moved (m/s) and which way it lay (hilt to tip).
#[derive(Debug, Clone, Copy)]
struct Pass {
    ms: f64,
    velocity: Vec3,
    blade: Vec3,
}

/// Where `striker`'s blade passed nearest `joint` (game space, metres) in
/// the snapshots of the `PASS_MS` up to its hit at `ms`, if near enough to
/// have cut it there.
fn pass(
    history: &[Arc<Snapshot>],
    level: &sim::Level,
    striker: SimId,
    joint: Vec3,
    ms: f64,
) -> Option<Pass> {
    // The striker's blade at each moment (metres), nearest the joint.
    let blades: Vec<(f64, Vec3, Vec3)> = history
        .iter()
        .filter(|s| (ms - PASS_MS..=ms).contains(&(s.time_ms as f64)))
        .filter_map(|s| {
            let objects = sim::Instances::in_snapshot(level, s);
            let striker = cast::entity(s, striker)?;
            let segments = crate::blade_fx::blade_segments(striker, (&objects, s.time_ms), level);
            segments
                .into_iter()
                .map(|(_, hilt, tip)| (s.time_ms as f64, hilt * UNIT, tip * UNIT))
                .min_by(|a, b| off(a, joint).1.total_cmp(&off(b, joint).1))
        })
        .collect();
    let nearest = (0..blades.len()).min_by(|&a, &b| {
        off(&blades[a], joint)
            .1
            .total_cmp(&off(&blades[b], joint).1)
    })?;
    let (along, distance) = off(&blades[nearest], joint);
    let (before, after) = (
        nearest.saturating_sub(1),
        (nearest + 1).min(blades.len() - 1),
    );
    if distance > PASS_NEAR || before == after {
        return None;
    }
    // The blade's point at that place along it, a moment either side.
    let at = |(_, hilt, tip): (f64, Vec3, Vec3)| hilt.lerp(tip, along);
    let seconds = ((blades[after].0 - blades[before].0) / 1000.0) as f32;
    let (_, hilt, tip) = blades[nearest];
    Some(Pass {
        ms: blades[nearest].0,
        velocity: (at(blades[after]) - at(blades[before])) / seconds,
        blade: (tip - hilt).normalize_or_zero(),
    })
}

/// How far along a blade (0 hilt, 1 tip) its point nearest `p` is, and how
/// far that point is from `p`.
fn off(&(_, hilt, tip): &(f64, Vec3, Vec3), p: Vec3) -> (f32, f32) {
    let line = tip - hilt;
    let along = ((p - hilt).dot(line) / line.length_squared().max(f32::EPSILON)).clamp(0.0, 1.0);
    (along, hilt.lerp(tip, along).distance(p))
}

/// One body falling (or fallen).
struct Fall {
    id: ragdoll::Id,
    doll: Arc<Doll>,
    died_ms: f64,
    length_ms: f64,
    /// The body's root where it fell (game space): its nodes are placed
    /// from there on.
    root: Frame,
    /// Where the level's faces were last taken in around it.
    gathered: Vec3,
    /// At rest, and drawn so: nothing to do until something wakes it.
    still: bool,
}

/// A body model's ragdoll: its template, and its skeleton's rest pose (each
/// node's parent and place on it), which the ragdoll's poses are drawn on.
pub struct Doll {
    pub template: Arc<Template>,
    pub rest: Arc<Rest>,
}

/// How a dying body's nodes stand apart in its death animation: at rest
/// length but for the one node a retarget moved (it and its place), or as
/// its clip drew them.
#[derive(Clone, Copy)]
pub enum Lengths {
    Rest(Option<(usize, Vec3)>),
    Drawn,
}

/// A dying character this frame, as its view has it: its body's doll, the
/// root and pose its death animation was drawn in (game space, its nodes'
/// model space), and the moment.
pub struct Dying {
    pub id: SimId,
    pub doll: Arc<Doll>,
    pub root: Frame,
    pub pose: Vec<Frame>,
    pub at: Blended,
    /// When it died, and how long its death animation is if known (ms).
    pub died_ms: f64,
    pub length_ms: Option<f64>,
}

/// Every ragdoll of a world.
#[derive(Default)]
pub struct Ragdolls {
    world: ragdoll::World,
    /// Each body model's doll, by model and how it is made (`None`: its
    /// skeleton has no ragdoll).
    dolls: BTreeMap<(String, &'static str), Option<Arc<Doll>>>,
    falls: BTreeMap<SimId, Fall>,
    /// A floor put under a level that has no faces (the course's boxes).
    floor: bool,
    stepped_ms: Option<f64>,
    /// The last step's cost (ms).
    pub step_ms: f64,
    /// Parts cut off since the scene last took them (`severing`): whose,
    /// and the cut.
    pub severed: Vec<(SimId, severing::Sever)>,
}

/// Each node of a body in model space, parents first: `local` gives a
/// node's place on its parent from its index, its parent's place (model
/// space) and its rest place on it.
pub fn model_space(rest: &Rest, mut local: impl FnMut(usize, Frame, Frame) -> Frame) -> Vec<Frame> {
    let mut pose: Vec<Frame> = Vec::with_capacity(rest.len());
    for (index, &(parent, translation, rotation)) in rest.iter().enumerate() {
        let above = parent
            .and_then(|p| pose.get(p).copied())
            .unwrap_or(Frame::IDENTITY);
        let local = local(index, above, Frame::new(translation, rotation));
        pose.push(above.then(local));
    }
    pose
}

/// Where each part of `template` is when the body's root is at `root` and
/// its nodes at `pose` (model space).
fn parts_at(template: &Template, root: Frame, pose: &[Frame]) -> Vec<Frame> {
    template
        .parts
        .iter()
        .map(|part| root.then(pose[part.node]))
        .collect()
}

/// A body model's skeleton in its rest pose, and its mesh, each vertex
/// with the node that moves it most (a skin's) or carries it (a rigid
/// mesh's).
fn skeleton(ir: &ModelIr, rest: &Rest) -> (Vec<ragdoll::Node>, Vec<(Vec3, usize)>) {
    let rest = model_space(rest, |_, _, rest| rest);
    let nodes = ir
        .nodes
        .iter()
        .zip(&rest)
        .map(|(node, &rest)| ragdoll::Node {
            name: node.name.clone(),
            parent: node.parent,
            rest,
        })
        .collect();
    let mut mesh = Vec::new();
    for part in &ir.meshes {
        let Some(&place) = rest.get(part.node) else {
            continue;
        };
        let at = |position: &[f32; 3]| place.point(Vec3::from_array(*position));
        match &part.skin {
            Some(bones) => {
                for ((position, joints), weights) in part
                    .positions
                    .iter()
                    .zip(&bones.vertex_joints)
                    .zip(&bones.vertex_weights)
                {
                    let most = (0..4).max_by(|&a, &b| weights[a].total_cmp(&weights[b]));
                    if let Some(&node) =
                        most.and_then(|slot| bones.joints.get(usize::from(joints[slot])))
                    {
                        mesh.push((at(position), node));
                    }
                }
            }
            None => mesh.extend(part.positions.iter().map(|p| (at(p), part.node))),
        }
    }
    (nodes, mesh)
}

/// Each node's place in its model as the death animation drew it: turned
/// as its transform is now, apart as `lengths` says.
pub fn drawn_pose(
    rest: &Rest,
    nodes: &[Entity],
    transforms: &Query<&mut Transform>,
    lengths: Lengths,
) -> Vec<Frame> {
    model_space(rest, |index, _, rest| {
        let drawn = nodes.get(index).and_then(|&node| transforms.get(node).ok());
        let rotation = drawn.map_or(rest.rotation, |t| t.rotation);
        let translation = match (lengths, drawn) {
            (Lengths::Rest(Some((node, place))), _) if node == index => place,
            (Lengths::Drawn, Some(t)) => t.translation,
            _ => rest.translation,
        };
        Frame::new(translation, rotation)
    })
}

/// The ragdoll `fit` makes of body model `ir` (rest pose `rest`), or why
/// it has none.
fn fitted(
    ir: &ModelIr,
    rest: &Rest,
    fit: impl FnOnce(&ragdoll::Skeleton) -> Result<Template, String>,
) -> Result<Template, String> {
    let (nodes, mesh) = skeleton(ir, rest);
    // The pose is worked out parents first.
    if nodes
        .iter()
        .enumerate()
        .any(|(i, n)| n.parent.is_some_and(|p| p >= i))
    {
        return Err("a node comes before its parent".into());
    }
    fit(&ragdoll::Skeleton {
        nodes: &nodes,
        skin: &mesh,
    })
}

/// Whether body model `ir` falls as a ragdoll when it dies, or why not: a
/// humanoid by KOTOR's humanoid plan, any other body fitted to its mesh
/// (of metal for a droid), as the views fit them (`kjk clips` counts them).
pub fn doll_fits(ir: &ModelIr, humanoid: bool, droid: bool) -> Result<(), String> {
    let rest = cast::rest_of(ir);
    let material = if droid {
        Material::Metal
    } else {
        Material::Flesh
    };
    fitted(ir, &rest, |skeleton| match humanoid {
        true => Template::fit(&ragdoll::humanoid::PLAN, skeleton),
        false => Template::auto(skeleton, material),
    })
    .map(|_| ())
}

impl Ragdolls {
    /// Whether `id` has fallen and lies still, drawn so: its body needs
    /// neither posing nor placing.
    pub fn still(&self, id: SimId) -> bool {
        self.falls.get(&id).is_some_and(|fall| fall.still)
    }

    /// Lets `id`'s fall go, if it has one: its character is up again, its
    /// body posed as the living are.
    pub fn rise(&mut self, id: SimId) {
        if let Some(fall) = self.falls.remove(&id) {
            self.world.remove(fall.id);
        }
    }

    /// How many bodies have fallen, and how many of their parts move.
    pub fn counts(&self) -> (usize, usize) {
        (self.falls.len(), self.world.counts().1)
    }

    /// Each fall, for dumps: whose, when, where its pelvis is (game
    /// space, metres), whether it has come to rest.
    pub fn facts(&self) -> serde_json::Value {
        let rows = self.falls.iter().map(|(id, fall)| {
            let pelvis = self
                .world
                .places(fall.id)
                .and_then(|places| Some(places.first()?.translation.to_array()));
            let template = &fall.doll.template;
            let cut: Vec<&str> = (1..template.parts.len())
                .filter(|&part| {
                    self.world.cut_off(fall.id, part)
                        && template.parts[part]
                            .parent
                            .is_none_or(|parent| !self.world.cut_off(fall.id, parent))
                })
                .map(|part| template.parts[part].name)
                .collect();
            serde_json::json!({
                "id": id.0,
                "died_ms": fall.died_ms,
                "pelvis_m": pelvis,
                "resting": self.world.resting(fall.id),
                "cut": cut,
            })
        });
        serde_json::Value::Array(rows.collect())
    }

    /// A humanoid body's doll, by KOTOR's humanoid plan, fitted once per
    /// model.
    pub fn humanoid(&mut self, rig: &BodyRig) -> Option<Arc<Doll>> {
        self.doll(rig.model(), &rig.rest, "humanoid", |skeleton| {
            Template::fit(&ragdoll::humanoid::PLAN, skeleton)
        })
    }

    /// A creature's doll fitted to its skeleton alone (beasts; droids of
    /// metal), once per model.
    pub fn creature(&mut self, ir: &ModelIr, material: Material) -> Option<Arc<Doll>> {
        let rest = cast::rest_of(ir);
        let kind = match material {
            Material::Flesh => "flesh",
            Material::Metal => "metal",
        };
        self.doll(ir, &rest, kind, |skeleton| {
            Template::auto(skeleton, material)
        })
    }

    fn doll(
        &mut self,
        ir: &ModelIr,
        rest: &Rest,
        kind: &'static str,
        fit: impl FnOnce(&ragdoll::Skeleton) -> Result<Template, String>,
    ) -> Option<Arc<Doll>> {
        self.dolls
            .entry((ir.name.to_ascii_lowercase(), kind))
            .or_insert_with(|| match fitted(ir, rest, fit) {
                Ok(template) => Some(Arc::new(Doll {
                    template: Arc::new(template),
                    rest: Arc::from(rest),
                })),
                Err(why) => {
                    warn!("{}: no ragdoll ({why})", ir.name);
                    None
                }
            })
            .clone()
    }

    /// The level's faces around `at` (game space) taken in, solid as the
    /// doors in `snapshot` are when first taken in; under a level with none,
    /// a floor at `soles` (the height the body stood at).
    fn gather(
        &mut self,
        (level, snapshot): (&sim::Level, Option<&Snapshot>),
        at: Vec3,
        soles: f32,
    ) {
        let Some(built) = level.collision().level() else {
            if !self.floor {
                self.floor = true;
                let (w, z) = (500.0, soles - 0.02);
                let corners = [[-w, -w], [w, -w], [w, w], [-w, w]].map(|[x, y]| Vec3::new(x, y, z));
                self.world.add_faces([
                    (0, [corners[0], corners[1], corners[2]]),
                    (0, [corners[0], corners[2], corners[3]]),
                ]);
            }
            return;
        };
        let doors = snapshot.and_then(|s| level.states_in(s));
        let reach = Vec3::new(GATHER[0], GATHER[0], 0.0);
        let min = at - reach - Vec3::Z * GATHER[1];
        let max = at + reach + Vec3::Z * GATHER[2];
        let faces = built.faces_in(
            (min.to_array(), max.to_array()),
            Contents::SOLID,
            doors.as_deref(),
        );
        self.world.add_faces(
            faces
                .into_iter()
                .map(|(brush, corners)| (brush, corners.map(Vec3::from_array))),
        );
    }

    /// The dying this frame: each new one becomes a ragdoll, each falling
    /// one follows its death animation; then the world runs on to `ms`.
    pub fn update(
        &mut self,
        dying: &[Dying],
        (level, history): (&sim::Level, &[Arc<Snapshot>]),
        ms: f64,
    ) {
        let started = std::time::Instant::now();
        let newest = history.last().map(|newest| &**newest);
        for d in dying {
            let since = ms - d.died_ms;
            let fall = match self.falls.get_mut(&d.id) {
                Some(fall) => fall,
                None => {
                    let template = d.doll.template.clone();
                    let places = parts_at(&template, d.root, &d.pose);
                    let pelvis = places[0].translation;
                    self.gather((level, newest), pelvis, d.root.translation.z);
                    let velocity = Vec3::from_array(d.at.state.velocity) * UNIT;
                    let id = self.world.spawn(template.clone(), &places, velocity);
                    if let Some(fighter) = &d.at.fighter {
                        let seen = (level, history, ms);
                        self.strike(id, (&template, &places), fighter, (d, seen));
                    }
                    self.falls.entry(d.id).or_insert(Fall {
                        id,
                        doll: d.doll.clone(),
                        died_ms: d.died_ms,
                        length_ms: d.length_ms.unwrap_or(DEATH_MS),
                        root: d.root,
                        gathered: pelvis,
                        still: false,
                    })
                }
            };
            let template = &fall.doll.template;
            let targets = parts_at(template, d.root, &d.pose);
            let muscles = muscles(since, fall.length_ms);
            self.world.drive(fall.id, &targets, muscles, ms / 1000.0);
        }
        // Bodies that went far take in the faces where they are now.
        let moved: Vec<Vec3> = self
            .falls
            .values_mut()
            .filter_map(|fall| {
                let pelvis = self.world.places(fall.id)?.first()?.translation;
                (pelvis.distance(fall.gathered) > REGATHER).then(|| {
                    fall.gathered = pelvis;
                    pelvis
                })
            })
            .collect();
        for pelvis in moved {
            self.gather((level, newest), pelvis, pelvis.z);
        }
        let seconds = self
            .stepped_ms
            .map_or(0.0, |was| (ms - was).max(0.0) / 1000.0);
        self.stepped_ms = Some(ms);
        self.world.step(seconds as f32);
        self.step_ms = started.elapsed().as_secs_f64() * 1000.0;
    }

    /// The blow that killed: the newest hit on `fighter`, at its location's
    /// part, from where its striker stood (or along the body's way); a
    /// blade's recent one may cut a limb off (`severing`), thrown the way
    /// the blade went through it and as far on as it would be at `ms`.
    fn strike(
        &mut self,
        id: ragdoll::Id,
        (template, places): (&Template, &[Frame]),
        fighter: &Fighter,
        (d, (level, history, ms)): (&Dying, (&sim::Level, &[Arc<Snapshot>], f64)),
    ) {
        let newest = history.last().map(|newest| &**newest);
        let Some(hit) = fighter.newest().find(|e| {
            e.damage > 0
                && matches!(
                    e.result,
                    Outcome::Hit | Outcome::Critical | Outcome::Automatic
                )
        }) else {
            return;
        };
        let strength = blow(hit.weapon);
        // A body fitted without a plan takes it at its root.
        let name = crate::cast::spot(hit.location).struck;
        let part = template
            .parts
            .iter()
            .position(|p| p.name == name)
            .unwrap_or(0);
        let victim = Vec3::from_array(d.at.origin) * UNIT;
        let from = hit
            .by
            .and_then(|by| cast::entity(newest?, by))
            .map(|striker| victim - Vec3::from_array(striker.position));
        let flat = |way: Vec3| way.with_z(0.0).try_normalize();
        let way = from
            .and_then(flat)
            .or_else(|| flat(Vec3::from_array(d.at.state.velocity)));
        let Some(way) = way else { return };
        let point = places[part].point(template.parts[part].solid.centre());
        let impulse = (way + Vec3::Z * BLOW_LIFT).normalize() * strength;
        self.world.strike(id, part, impulse, point);
        // When the hit landed: the first snapshot from its tick on, if the
        // history kept reaches back to it.
        let Some(hit_ms) = history
            .first()
            .filter(|first| first.tick <= hit.tick)
            .and_then(|_| history.iter().find(|s| s.tick >= hit.tick))
            .map(|s| s.time_ms as f64)
        else {
            return;
        };
        let striker = hit.by.and_then(|by| cast::entity(newest?, by));
        let sheet = striker.zip(newest).map(|(striker, newest)| {
            cast::sheet(striker, &sim::Instances::in_snapshot(level, newest))
        });
        let Some(edge) = severing::edge(hit, sheet.as_ref()) else {
            return;
        };
        let Some(limb) = severing::limb(template, hit, d.id) else {
            return;
        };
        let pass = hit
            .by
            .and_then(|by| pass(history, level, by, places[limb].translation, hit_ms));
        let blade = pass.map(|p| (p.velocity, p.blade));
        let (kick, spin) = severing::throw(template, places, (limb, way, blade), (d.id, hit.tick));
        let at = places[limb].point(template.parts[limb].solid.centre());
        let ahead = pass.map_or(0.0, |p| ((ms - p.ms) / 1000.0).clamp(0.0, AHEAD) as f32);
        if self.world.sever(id, limb, (kick, at, spin), ahead) {
            self.severed.push((
                d.id,
                severing::Sever {
                    doll: d.doll.clone(),
                    part: limb,
                    edge,
                    at: places[limb].translation,
                    way: kick.normalize_or_zero(),
                    ms,
                },
            ));
        }
    }

    /// Places the fallen bodies (at `ms`) as their ragdolls lie: each
    /// root where it fell, each part's node turned as its body is (the
    /// pelvis also moved), the nodes between and below as they were drawn.
    pub fn draw<'a>(
        &mut self,
        bodies: impl Iterator<Item = (SimId, Entity, &'a [Entity])>,
        ms: f64,
        transforms: &mut Query<&mut Transform>,
    ) {
        for (id, root, nodes) in bodies {
            let Some(fall) = self.falls.get_mut(&id) else {
                continue;
            };
            let Doll { template, rest } = &*fall.doll;
            let resting = self.world.resting(fall.id)
                && muscles(ms - fall.died_ms, fall.length_ms) == Muscles::LIMP;
            if fall.still && resting {
                continue;
            }
            let Some(places) = self.world.places(fall.id) else {
                continue;
            };
            if let Ok(mut t) = transforms.get_mut(root) {
                *t = Transform::from_translation(fall.root.translation)
                    .with_rotation(fall.root.rotation);
            }
            let to_model = fall.root.inverse();
            let mut parts = vec![None; rest.len()];
            for (index, (part, place)) in template.parts.iter().zip(&places).enumerate() {
                // A part whose node does not hang straight from its ragdoll
                // parent's (KOTOR's thighs are not under its pelvis; a
                // droid's legs hang from a groin it may not have) is placed,
                // not only turned: what lies between would carry it away from
                // its body.
                let straight = part
                    .parent
                    .is_some_and(|parent| rest[part.node].0 == Some(template.parts[parent].node));
                let placed = !straight || self.world.cut_off(fall.id, index);
                parts[part.node] = Some((placed, to_model.then(*place)));
            }
            model_space(rest, |index, above, rest| {
                let transform = nodes.get(index).and_then(|&e| transforms.get_mut(e).ok());
                match (parts[index], transform) {
                    (Some((placed, model)), Some(mut t)) => {
                        t.rotation = (above.rotation.inverse() * model.rotation).normalize();
                        t.translation = if placed {
                            above.inverse().point(model.translation)
                        } else {
                            rest.translation
                        };
                        Frame::new(t.translation, t.rotation)
                    }
                    (_, Some(t)) => Frame::new(t.translation, t.rotation),
                    (_, None) => rest,
                }
            });
            fall.still = resting;
        }
    }
}
