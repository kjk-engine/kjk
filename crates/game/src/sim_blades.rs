//! The blades the sim cuts with, at the drawn moment: what a gripping hand
//! reaches for (`in_hands`, `cast::Reaching`), and with `--sim-blades`,
//! drawn beside the blades the view draws, the gap between them measured.
//!
//! - **The sim's blade** at the drawn moment is worked out again in the view
//!   as the sim works it out (`saber::blades::place`): the skeleton's pose
//!   that the character's last snapshot before the moment carries
//!   (`Fighter::pose`), posed with the server's look (`Posed::pose`,
//!   `saber::look`) and placed at its origin pushed ahead along its velocity,
//!   turned to its legs' yaw (the origin, velocity and view blended to the
//!   moment). At a tick's time that is the sim's own blade: the trails the
//!   snapshots carry (where each swinging blade was that tick) check it,
//!   snapshot by snapshot (`check`), but for a character the tick's own
//!   hits moved after its blades were placed.
//! - **JA's blade as the view poses it**: JA's skeleton in the pose the
//!   view's Ghoul2 player gives the body (`cast::Figure`, before the
//!   retarget onto the KOTOR body), JA's hilt in its hand, at the drawn
//!   origin. Between it and the sim's is what the playback still does its
//!   own way (the cgame's look, a third change mid-blend); between it and
//!   the drawn one, the retarget, the grip and the reach.
//! - **Drawn**: the sim's blade green, JA's as the view poses it cyan, and a
//!   yellow line from the drawn blade's tip to the sim's; every living
//!   body's box as shots, blades and uses meet it (`sim::body`: JA's box,
//!   or where a beast's or droid's model is drawn), magenta, and beside a
//!   model's the JA box it stands in for, grey.
//! - **Measured**, per lit blade in hand and frame, for each pair: how far
//!   apart the hilt ends and the tips are (units) and the angle between the
//!   blades; gathered by what the torso plays into the dumps
//!   (`sim_blades`), and written each frame to `blade-gap.csv` in the run's
//!   folder. A blade still growing or shrinking is drawn but not measured.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use collision::UNIT;
use console::{DumpDue, DumpFacts, RunFolder};
use sim::combat::saber::{Kit, blades};
use sim::skeleton::{Blade, BladeId, Hand};
use sim::{Instances, SimId, Snapshot};

use crate::cast;
use crate::lifecycle_view::SceneCamera;
use crate::scene::{DrawnBlades, game_to_bevy};

/// A sim blade at the drawn moment, JA's as the view poses it, and the
/// drawn blade they stand for.
#[derive(Debug, Clone, Copy)]
pub struct Shown {
    pub id: SimId,
    /// Its place among the character's blades (`blades::slots`).
    pub slot: usize,
    /// Its index in `LiveBlades`.
    pub spec: usize,
    /// Game space, units.
    pub sim: Blade,
    pub ja: Option<Blade>,
    pub torso: &'static str,
    pub saber_move: &'static str,
    /// The torso's skeleton is blending into its animation.
    pub blending: bool,
    /// Its body was posed this frame (`Cast::nearest`); one held in its last
    /// pose is drawn but not measured.
    pub posed: bool,
}

/// The pairs measured: drawn and sim, JA as the view poses it and sim,
/// drawn and JA.
const PAIRS: [&str; 3] = ["drawn_sim", "ja_sim", "drawn_ja"];

/// How far apart two blades were, over some frames.
#[derive(Debug, Clone, Copy, Default)]
struct Gap {
    frames: u32,
    hilt: Spread,
    tip: Spread,
    angle: Spread,
}

#[derive(Debug, Clone, Copy, Default)]
struct Spread {
    sum: f64,
    max: f32,
}

impl Spread {
    fn add(&mut self, value: f32) {
        self.sum += f64::from(value);
        self.max = self.max.max(value);
    }

    fn json(&self, frames: u32) -> serde_json::Value {
        let round = |v: f64| (v * 100.0).round() / 100.0;
        serde_json::json!({
            "mean": round(self.sum / f64::from(frames.max(1))),
            "max": round(f64::from(self.max)),
        })
    }
}

impl Gap {
    fn add(&mut self, [hilt, tip, angle]: [f32; 3]) {
        self.frames += 1;
        self.hilt.add(hilt);
        self.tip.add(tip);
        self.angle.add(angle);
    }

    fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "frames": self.frames,
            "hilt_units": self.hilt.json(self.frames),
            "tip_units": self.tip.json(self.frames),
            "angle_degrees": self.angle.json(self.frames),
        })
    }
}

/// The gaps of each pair (`PAIRS`), and the frames blending.
#[derive(Debug, Clone, Copy, Default)]
struct Gaps {
    pairs: [Gap; 3],
    blending: u32,
}

impl Gaps {
    fn json(&self) -> serde_json::Value {
        let mut out: serde_json::Map<_, _> = PAIRS
            .iter()
            .zip(&self.pairs)
            .map(|(name, gap)| (name.to_string(), gap.json()))
            .collect();
        out.insert("blending".into(), self.blending.into());
        serde_json::Value::Object(out)
    }
}

/// The overlay's state: this frame's sim blades, the gaps so far, the
/// trails checked, and where the frames are written.
#[derive(Resource, Default)]
pub struct SimBlades {
    /// The drawn moment (sim ms) and its blades.
    pub ms: f64,
    pub shown: Vec<Shown>,
    all: Gaps,
    by_torso: BTreeMap<&'static str, Gaps>,
    /// Frames of blades not measured: their bodies held in their last pose.
    held: u32,
    /// Trails compared with the blade worked out again, those apart, and
    /// how far apart at worst (units, degrees).
    checked: u32,
    apart: u32,
    worst: [f32; 2],
    csv: Option<std::io::BufWriter<std::fs::File>>,
    mesh: Option<Handle<Mesh>>,
}

/// The snapshots around `ms`: the last at or before it (the first, before
/// all), the one after it, and how far between them `ms` is.
fn around(history: &[Arc<Snapshot>], ms: f64) -> Option<(&Snapshot, &Snapshot, f32)> {
    let last = history.len().checked_sub(1)?;
    let after = history.partition_point(|s| (s.time_ms as f64) <= ms);
    let a = &history[after.saturating_sub(1).min(last)];
    let b = history.get(after).unwrap_or(a);
    let span = b.time_ms.saturating_sub(a.time_ms) as f64;
    let t = if span > 0.0 {
        ((ms - a.time_ms as f64) / span).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    Some((a, b, t))
}

/// A character's sim blades at a moment, by their place among its blades
/// (`blades::slots`) with which blade each is, and what its torso plays.
struct Moment {
    blades: [Option<(Blade, BladeId)>; 2],
    blending: bool,
    torso: &'static str,
    saber_move: &'static str,
}

/// The sim's blades in hand of character `id` at `ms` in `history` (the
/// own character's from its predictions, `own`), each with its hand: what
/// a gripping hand reaches for (`cast::Cast::draw`).
pub fn in_hands(
    level: &sim::Level,
    (history, ms): (&[Arc<Snapshot>], f64),
    id: SimId,
    own: Option<&cast::Own<'_>>,
) -> Vec<(Blade, Hand)> {
    moment(level, (history, ms), id, own).map_or_else(Vec::new, |(moment, _)| {
        moment
            .blades
            .into_iter()
            .flatten()
            .map(|(blade, which)| (blade, which.hand))
            .collect()
    })
}

/// The sim's blades of `id` at `ms`, or the own character's from its
/// predictions, and the moment they are at.
fn moment(
    level: &sim::Level,
    (history, ms): (&[Arc<Snapshot>], f64),
    id: SimId,
    own: Option<&cast::Own<'_>>,
) -> Option<(Moment, f64)> {
    match own {
        Some(own) if own.id == id => {
            Some((at(level, own.history, id, own.ms, own.correction)?, own.ms))
        }
        _ => Some((at(level, history, id, ms, [0.0; 3])?, ms)),
    }
}

/// The sim's blades of character `id` at `ms` in `history`, moved by
/// `correction` (units; see the module documentation).
fn at(
    level: &sim::Level,
    history: &[Arc<Snapshot>],
    id: SimId,
    ms: f64,
    correction: [f32; 3],
) -> Option<Moment> {
    let data = level.anims().skeleton()?;
    let (a, b, t) = around(history, ms)?;
    let (ea, eb) = (cast::entity(a, id)?, cast::entity(b, id)?);
    let (fighter, pa, pb) = (ea.fighter?, ea.player?, eb.player?);
    let mut ps = pa;
    ps.origin =
        std::array::from_fn(|i| pa.origin[i] + (pb.origin[i] - pa.origin[i]) * t + correction[i]);
    ps.velocity = std::array::from_fn(|i| pa.velocity[i] + (pb.velocity[i] - pa.velocity[i]) * t);
    ps.view =
        std::array::from_fn(|i| pa.view[i] + blades::normalize_180(pb.view[i] - pa.view[i]) * t);
    let now = ms.max(0.0) as u64;
    let (pose, legs_yaw) = fighter
        .pose
        .pose(data, &ps, (&fighter.saber, fighter.alive()), now)?;
    let placement = blades::placement(&ps, sim::TickRate::default().msec(a.tick), legs_yaw);
    let kit = Kit::of(&cast::sheet(ea, &Instances::in_snapshot(level, a)));
    Some(Moment {
        blades: blades::slots(&kit, &fighter.saber, fighter.hand).map(|slot| {
            let slot = slot?;
            Some((blades::place(data, &pose, now, &placement, &slot)?, slot.id))
        }),
        blending: fighter
            .pose
            .torso
            .and_then(|torso| torso.playback.blend)
            .is_some_and(|blend| (now as i64) < blend.start_ms + i64::from(blend.length_ms)),
        torso: pa.torso.anim.name(),
        saber_move: fighter.saber.current_move.name(),
    })
}

/// JA's blade `which` as the view poses character `id` at `ms`, standing
/// at `origin` (units; see the module documentation).
fn ja_blade(
    cast: &cast::Cast,
    id: SimId,
    ms: f64,
    origin: [f32; 3],
    which: BladeId,
) -> Option<Blade> {
    let rig = &cast.rig;
    let bolt = rig.hands[usize::from(which.hand == Hand::Left)].as_ref()?;
    let saber = rig.sabers.of(which.saber == blades::STAFF);
    let (start, along) = *saber.ends.get(which.blade)?;
    let model = Mat4::from_translation(Vec3::from_array(origin) * UNIT)
        * Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2)
        * cast.figure(id)?.bolt(ms, bolt);
    let muzzle = model.transform_point3(Vec3::from_array(start)) / UNIT;
    let direction = model
        .transform_vector3(Vec3::from_array(along))
        .normalize_or_zero();
    let length = *saber.lengths.get(which.blade)?;
    Some(Blade {
        muzzle: muzzle.to_array(),
        direction: direction.to_array(),
        tip: (muzzle + direction * length).to_array(),
    })
}

/// Every lit blade in hand `cast` drew at `ms`, as the sim has it and as
/// JA's skeleton has it: the own character's from its predictions (`own`);
/// those in `posing` were posed this frame.
pub fn in_hand(
    cast: &cast::Cast,
    level: &sim::Level,
    (history, ms): (&[Arc<Snapshot>], f64),
    own: Option<&cast::Own<'_>>,
    posing: &[SimId],
) -> Vec<Shown> {
    let mut shown = Vec::new();
    for (id, drawn) in cast.blades_in_hand() {
        let (Some((moment, at_ms)), Some(stands)) = (
            moment(level, (history, ms), id, own),
            cast::blended_for(history, id, ms, own),
        ) else {
            continue;
        };
        for &(slot, spec) in drawn {
            let Some((sim, which)) = moment.blades.get(slot).copied().flatten() else {
                continue;
            };
            shown.push(Shown {
                id,
                slot,
                spec,
                sim,
                ja: ja_blade(cast, id, at_ms, stands.origin, which),
                torso: moment.torso,
                saber_move: moment.saber_move,
                blending: moment.blending,
                posed: posing.contains(&id),
            });
        }
    }
    shown
}

/// How far apart two blades' hilt ends and tips are, and the angle between
/// them (degrees).
fn gap((ah, at): (Vec3, Vec3), (bh, bt): (Vec3, Vec3)) -> [f32; 3] {
    let angle = (at - ah)
        .normalize_or_zero()
        .angle_between((bt - bh).normalize_or_zero())
        .to_degrees();
    [ah.distance(bh), at.distance(bt), angle]
}

fn ends(blade: &Blade) -> (Vec3, Vec3) {
    (Vec3::from_array(blade.muzzle), Vec3::from_array(blade.tip))
}

impl SimBlades {
    /// Starts writing the frames to `blade-gap.csv` in `folder`.
    pub fn open(&mut self, folder: &std::path::Path) {
        let file = std::fs::File::create(folder.join("blade-gap.csv"));
        self.csv = file.ok().map(std::io::BufWriter::new).and_then(|mut csv| {
            let gaps: Vec<String> = PAIRS
                .iter()
                .flat_map(|pair| ["hilt", "tip", "angle"].map(|what| format!("{pair}_{what}")))
                .collect();
            let places: Vec<String> = ["sim", "drawn", "ja"]
                .iter()
                .flat_map(|blade| {
                    ["hilt", "tip"]
                        .into_iter()
                        .flat_map(move |end| ["x", "y", "z"].map(|a| format!("{blade}_{end}_{a}")))
                })
                .collect();
            writeln!(
                csv,
                "ms,id,slot,torso,move,blending,{},{}",
                gaps.join(","),
                places.join(",")
            )
            .ok()?;
            Some(csv)
        });
    }

    /// Compares each swinging blade's trail in `snapshot` with the blade
    /// worked out again at its time.
    pub fn check(&mut self, level: &sim::Level, snapshot: &Arc<Snapshot>) {
        let history = std::slice::from_ref(snapshot);
        for entity in &snapshot.entities {
            let Some(fighter) = entity.fighter else {
                continue;
            };
            let Some(again) = at(level, history, entity.id, snapshot.time_ms as f64, [0.0; 3])
            else {
                continue;
            };
            for (trail, again) in fighter.trails.iter().zip(again.blades) {
                let (Some(trail), Some((blade, _))) = (trail, again) else {
                    continue;
                };
                if trail.at_ms != snapshot.time_ms {
                    continue;
                }
                let muzzle =
                    Vec3::from_array(trail.muzzle).distance(Vec3::from_array(blade.muzzle));
                let direction = Vec3::from_array(trail.direction)
                    .angle_between(Vec3::from_array(blade.direction))
                    .to_degrees();
                self.checked += 1;
                self.apart += u32::from(muzzle > 0.01 || direction > 0.01);
                self.worst = [self.worst[0].max(muzzle), self.worst[1].max(direction)];
            }
        }
    }

    /// Measures one blade against the drawn one (`drawn`, game space,
    /// metres), unless it is growing or shrinking.
    fn measure(&mut self, shown: &Shown, drawn: [Vec3; 2]) {
        if !shown.posed {
            self.held += 1;
            return;
        }
        let drawn = (drawn[0] / UNIT, drawn[1] / UNIT);
        let sim = ends(&shown.sim);
        if drawn.0.distance(drawn.1) < sim.0.distance(sim.1) * 0.99 {
            return;
        }
        let ja = shown.ja.as_ref().map(ends);
        let gaps = [
            Some(gap(drawn, sim)),
            ja.map(|ja| gap(ja, sim)),
            ja.map(|ja| gap(drawn, ja)),
        ];
        for totals in [&mut self.all, self.by_torso.entry(shown.torso).or_default()] {
            for (total, gap) in totals.pairs.iter_mut().zip(gaps) {
                if let Some(gap) = gap {
                    total.add(gap);
                }
            }
            totals.blending += u32::from(shown.blending);
        }
        if let Some(csv) = &mut self.csv {
            let cells = |values: Option<[f32; 3]>| {
                values.map_or_else(
                    || ",,".to_string(),
                    |v| format!("{:.2},{:.2},{:.2}", v[0], v[1], v[2]),
                )
            };
            let places: Vec<String> = [Some(sim), Some(drawn), ja]
                .iter()
                .flat_map(|ends| {
                    let (h, t) = ends.unzip();
                    [h, t].map(|end| cells(end.map(|v| v.to_array())))
                })
                .collect();
            let _ = writeln!(
                csv,
                "{:.1},{},{},{},{},{},{},{}",
                self.ms,
                shown.id.0,
                shown.slot,
                shown.torso,
                shown.saber_move,
                u8::from(shown.blending),
                gaps.map(cells).join(","),
                places.join(",")
            );
        }
    }
}

/// The overlay's lines and the frame's gaps, after the blades are drawn.
#[allow(clippy::too_many_arguments)]
pub fn show(
    mut sim: ResMut<SimBlades>,
    drawn: Res<DrawnBlades>,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
    cameras: Query<&GlobalTransform, SceneCamera>,
    (mut meshes, mut materials): (ResMut<Assets<Mesh>>, ResMut<Assets<StandardMaterial>>),
    mut commands: Commands,
    folder: Option<Res<RunFolder>>,
    mut opened: Local<bool>,
    (presented, swap): (
        Option<Res<crate::sim_host::PresentedSnapshot>>,
        Option<Res<crate::sim_host::SceneSwap>>,
    ),
) {
    let sim = &mut *sim;
    if !*opened {
        *opened = true;
        if let Some(folder) = folder {
            sim.open(&folder.0);
        }
    }
    let Some(eye) = cameras.iter().next().map(GlobalTransform::translation) else {
        return;
    };
    let to_bevy = |units: [f32; 3]| game_to_bevy() * (Vec3::from_array(units) * UNIT);
    let mut quads = Quads::default();
    let mut now = Vec::new();
    for shown in std::mem::take(&mut sim.shown) {
        let sim_tip = to_bevy(shown.sim.tip);
        quads.line(eye, to_bevy(shown.sim.muzzle), sim_tip, 0.02, GREEN);
        if let Some(ja) = &shown.ja {
            quads.line(eye, to_bevy(ja.muzzle), to_bevy(ja.tip), 0.014, CYAN);
        }
        let Some(ends) = drawn.0.get(shown.spec).copied().flatten() else {
            continue;
        };
        quads.line(eye, game_to_bevy() * ends[1], sim_tip, 0.01, YELLOW);
        sim.measure(&shown, ends);
        now.push(serde_json::json!({
            "id": shown.id.0,
            "slot": shown.slot,
            "torso": shown.torso,
            "move": shown.saber_move,
            "blending": shown.blending,
        }));
    }
    if let (Some(presented), Some(swap)) = (presented, swap) {
        bodies(&mut quads, eye, presented.0.current(), &swap.level);
    }
    let mesh = quads.mesh();
    match &sim.mesh {
        Some(handle) => {
            if let Some(mut old) = meshes.get_mut(handle) {
                *old = mesh;
            }
        }
        None => {
            let handle = meshes.add(mesh);
            let material = materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                ..default()
            });
            commands.spawn((
                Mesh3d(handle.clone()),
                MeshMaterial3d(material),
                Transform::IDENTITY,
                bevy::camera::visibility::NoFrustumCulling,
            ));
            sim.mesh = Some(handle);
        }
    }
    if dump_due.0
        && let Some(csv) = &mut sim.csv
    {
        let _ = csv.flush();
    }
    facts.0.insert(
        "sim_blades".into(),
        serde_json::json!({
            "now": now,
            "all": sim.all.json(),
            "by_torso": sim
                .by_torso
                .iter()
                .map(|(torso, gaps)| (torso.to_string(), gaps.json()))
                .collect::<serde_json::Map<_, _>>(),
            "held_frames": sim.held,
            "trails": {
                "checked": sim.checked,
                "apart": sim.apart,
                "worst_units": sim.worst[0],
                "worst_degrees": sim.worst[1],
            },
        }),
    );
}

/// Every living body of `snapshot` as the sim meets it, its box's edges.
fn bodies(quads: &mut Quads, eye: Vec3, snapshot: &Snapshot, level: &sim::Level) {
    let instances = Instances::in_snapshot(level, snapshot);
    for entity in &snapshot.entities {
        let (Some(player), Some(fighter)) = (entity.player, entity.fighter) else {
            continue;
        };
        if !fighter.alive() {
            continue;
        }
        let bulk = sim::bulk_of(instances.of_entity(entity));
        edges(quads, eye, &sim::body(entity.id, &player, bulk), MAGENTA);
        if bulk.is_some() {
            edges(quads, eye, &sim::body(entity.id, &player, None), GREY);
        }
    }
}

/// `body`'s box's twelve edges: its corners one axis apart.
fn edges(quads: &mut Quads, eye: Vec3, body: &collision::Body, colour: [f32; 4]) {
    let corner = |i: usize| {
        let pick = |axis: usize| {
            if i >> axis & 1 == 0 {
                body.min[axis]
            } else {
                body.max[axis]
            }
        };
        game_to_bevy() * Vec3::new(pick(0), pick(1), pick(2))
    };
    for i in 0..8 {
        for axis in 0..3 {
            if i >> axis & 1 == 0 {
                quads.line(eye, corner(i), corner(i | 1 << axis), 0.02, colour);
            }
        }
    }
}

const MAGENTA: [f32; 4] = [1.0, 0.2, 0.9, 1.0];
const GREY: [f32; 4] = [0.6, 0.6, 0.6, 1.0];
const GREEN: [f32; 4] = [0.1, 1.0, 0.2, 1.0];
const CYAN: [f32; 4] = [0.1, 0.9, 1.0, 1.0];
const YELLOW: [f32; 4] = [1.0, 0.9, 0.1, 1.0];

/// Camera-facing lines as quads with vertex colours.
#[derive(Default)]
struct Quads {
    positions: Vec<[f32; 3]>,
    colours: Vec<[f32; 4]>,
}

impl Quads {
    /// A line from `a` to `b` (Bevy space), `width` metres wide, facing
    /// `eye`.
    fn line(&mut self, eye: Vec3, a: Vec3, b: Vec3, width: f32, colour: [f32; 4]) {
        let side = (a - eye).cross(b - eye).normalize_or_zero() * width / 2.0;
        for corner in [a + side, a - side, b - side, b + side] {
            self.positions.push(corner.to_array());
            self.colours.push(colour);
        }
    }

    /// The mesh, both faces of each quad; never empty (one invisible quad),
    /// so it keeps its buffers.
    fn mesh(mut self) -> Mesh {
        if self.positions.is_empty() {
            self.positions = vec![[0.0; 3]; 4];
            self.colours = vec![[0.0; 4]; 4];
        }
        let quads = self.positions.len() as u32 / 4;
        let indices: Vec<u32> = (0..quads)
            .flat_map(|q| [0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2].map(|i| q * 4 + i))
            .collect();
        let normals = vec![[0.0, 0.0, 1.0]; self.positions.len()];
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colours);
        mesh.insert_indices(Indices::U32(indices));
        mesh
    }
}
