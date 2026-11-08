//! Danglymeshes (hair, flaps, capes, hoods, leaves) swaying as the games'
//! executables sway them (TSL's `PartDanglyMesh::Animate`; K1 has the same
//! class): each vertex is a damped spring pulled back to its rest place,
//! moved each frame by its mesh's travel and turn since the last one, so it
//! trails behind a walk and swings through a turn.
//!
//! Per frame, with the frame's time held to 12.5–35 ms: a mesh that jumped
//! more than 5 m (a module change, a teleport) snaps back to rest. Otherwise
//! each vertex whose constraint is not 0 is carried back by the travel (in
//! the last frame's node space) and turned with its velocity by the turn,
//! so it keeps its place in the world; its velocity gains the pull towards
//! rest (constraint × tightness × ½) less its damping (period × 1.5), and it
//! moves by it. It may stray at most `displacement × (1 − constraint/255)`
//! from rest along each axis, a box rather than a ball. A vertex with
//! constraint 0 is never moved (and one with 255 cannot stray): the data
//! pins roots that way. The data's displacements are small (2.5–3.5 cm for
//! most hair, flaps and hoods; Juhani's cloth 12.5 cm), and the executable
//! uses them unscaled, so the sway is subtle, as in the games.
//!
//! The frame's time is the game's: a held clock (`anim`, `record`) steps the
//! springs by what it moved and not at all while frozen [verify: whether
//! the executable steps them while the game is paused].
//!
//! The area's wind (`wind`) blows on each free vertex too: its push over
//! the frame at the node's place, in node space, × 20 × the vertex's own
//! reach, added before the box holds it. That is what moves the leaves of
//! the areas' trees.
//!
//! Where the vertices were the frame before goes in the mesh too
//! (`material::PREVIOUS_POSITION`): a camera resolving frames over time
//! (`temporal`) takes their motion from it, the sway being the mesh's own
//! and no transform's.

use asset_odyssey::model::DanglyIr;
use bevy::prelude::*;
use console::{AnimationTime, DumpDue, DumpFacts};

use crate::scene::game_to_bevy;
use crate::wind::Wind;

/// The frame's time is held within these (seconds).
const DT_MIN: f32 = 0.0125;
const DT_MAX: f32 = 0.035;
/// A jump further than this (metres) snaps every vertex back to rest.
const JUMP_M: f32 = 5.0;
/// The spring's pull per unit of constraint × tightness.
const PULL: f32 = 0.5;
/// The damping per unit of period.
const DAMPING: f32 = 1.5;
/// A constraint's full scale: at it a vertex cannot stray.
const CONSTRAINT_FULL: f32 = 255.0;
/// A vertex straying further than this along an axis is put back at rest.
const LOST: f32 = 1e10;
/// The wind's push on a free vertex per metre it blows over the frame, in
/// units of the mesh's displacement.
const WIND_GAIN: f32 = 20.0;

pub struct DanglyPlugin;

impl Plugin for DanglyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            // Where the node stands this frame, and before the meshes'
            // changes are sent on.
            sway.after(bevy::transform::TransformSystems::Propagate)
                .before(bevy::asset::AssetEventSystems),
        )
        .add_systems(Last, note_seen);
    }
}

/// A drawn danglymesh's spring and where its vertices are now, on its mesh's
/// entity (its own mesh asset, which `sway` rewrites).
#[derive(Component)]
pub struct Dangly {
    displacement: f32,
    tightness: f32,
    period: f32,
    constraints: Vec<f32>,
    rest: Vec<Vec3>,
    at: Vec<Vec3>,
    velocity: Vec<Vec3>,
    /// The node's world place and turn last frame; `None` before the first.
    last: Option<(Vec3, Quat)>,
    /// Whether a camera saw the mesh last frame (`note_seen`).
    seen: bool,
    /// Its mesh's previous positions are not its positions: written in a
    /// frame that moved it, and left until a frame that does not.
    moving: bool,
}

impl Dangly {
    pub fn new(ir: &DanglyIr) -> Dangly {
        let rest: Vec<Vec3> = ir.rest.iter().copied().map(Vec3::from_array).collect();
        Dangly {
            displacement: ir.displacement,
            tightness: ir.tightness,
            period: ir.period,
            constraints: ir.constraints.clone(),
            at: rest.clone(),
            velocity: vec![Vec3::ZERO; rest.len()],
            rest,
            last: None,
            seen: true,
            moving: false,
        }
    }

    /// One frame of `dt` seconds (held within `DT_MIN`..`DT_MAX`) with the
    /// node at `place`, `turn` (world), its scale `scale`, and the wind
    /// blowing `wind` (world, metres over the frame): whether any vertex
    /// moved.
    fn step(&mut self, (place, turn, scale): (Vec3, Quat, Vec3), dt: f32, wind: Vec3) -> bool {
        let (last_place, last_turn) = self.last.unwrap_or((place, turn));
        self.last = Some((place, turn));
        if place.distance(last_place) > JUMP_M {
            let moved = self.at != self.rest;
            self.at.clone_from(&self.rest);
            return moved;
        }
        // The travel in the last frame's node space, backwards (what the
        // vertices lag by), and the turn from that frame's space to this.
        let travel = (last_turn.inverse() * (last_place - place)) / scale;
        let turned = turn.inverse() * last_turn;
        let wind = (turn.inverse() * wind) / scale * WIND_GAIN;
        let pull = self.tightness * PULL * dt;
        let damping = self.period * DAMPING * dt;
        let mut moved = false;
        for (i, &constraint) in self.constraints.iter().enumerate() {
            if constraint == 0.0 {
                continue;
            }
            let rest = self.rest[i];
            let vertex = turned * (self.at[i] + travel);
            let velocity = turned * self.velocity[i];
            let velocity = velocity + (rest - vertex) * constraint * pull - velocity * damping;
            self.velocity[i] = velocity;
            let free = 1.0 - constraint / CONSTRAINT_FULL;
            let offset = vertex + velocity * dt + wind * (free * self.displacement) - rest;
            let at = if offset.cmpgt(Vec3::splat(LOST)).any()
                || offset.cmplt(Vec3::splat(-LOST)).any()
            {
                rest
            } else {
                let reach = self.displacement * free;
                rest + offset.map(|axis| {
                    if axis > reach {
                        reach
                    } else if axis < -reach {
                        -reach
                    } else {
                        axis
                    }
                })
            };
            moved |= at != self.at[i];
            self.at[i] = at;
        }
        moved
    }
}

/// A danglymesh's mesh: where its vertices were is where they are, and then,
/// if they moved (`now`), they are there.
fn settle(meshes: &mut Assets<Mesh>, mesh: &Handle<Mesh>, now: Option<Vec<[f32; 3]>>) {
    let Some(mut mesh) = meshes.get_mut(mesh) else {
        return;
    };
    if let Some(was) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned() {
        mesh.insert_attribute(crate::material::PREVIOUS_POSITION, was);
    }
    if let Some(now) = now {
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, now);
    }
}

/// A danglymesh's mesh as it is spawned: its previous positions its rest.
pub fn with_previous(mut mesh: Mesh) -> Mesh {
    if let Some(rest) = mesh.attribute(Mesh::ATTRIBUTE_POSITION).cloned() {
        mesh.insert_attribute(crate::material::PREVIOUS_POSITION, rest);
    }
    mesh
}

/// Whether a camera saw each danglymesh this frame, for `sway` next frame
/// (its own frame's is not known until after it has run).
fn note_seen(mut danglies: Query<(&mut Dangly, &ViewVisibility)>) {
    for (mut dangly, seen) in &mut danglies {
        dangly.seen = seen.get();
    }
}

/// Each danglymesh one frame on, its mesh rewritten when a vertex moved.
/// And in the dump: how many there are, how many moved this frame, and the
/// farthest any vertex is from rest (node space).
fn sway(
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut danglies: Query<(&mut Dangly, &GlobalTransform, &Mesh3d)>,
    (mut facts, dump_due): (ResMut<DumpFacts>, Res<DumpDue>),
    (held, mut last_held): (Res<AnimationTime>, Local<Option<f32>>),
    mut wind: ResMut<Wind>,
) {
    // A held clock (`anim`, `record`) steps by what it moved, and a frozen
    // one not at all, so a recorded frame is the same on every run.
    let dt = match (held.0, std::mem::replace(&mut *last_held, held.0)) {
        (Some(now), Some(last)) => now - last,
        (Some(_), None) => 0.0,
        (None, _) => time.delta_secs(),
    };
    if dt <= 0.0 {
        // A frozen clock: what moved last frame is still now.
        for (mut dangly, _, mesh) in &mut danglies {
            if std::mem::take(&mut dangly.moving) {
                settle(&mut meshes, &mesh.0, None);
            }
        }
        return;
    }
    wind.tick(dt);
    let dt = dt.clamp(DT_MIN, DT_MAX);
    let to_game = game_to_bevy().inverse();
    let mut moved = 0;
    for (mut dangly, global, mesh) in &mut danglies {
        let (scale, turn, place) = global.to_scale_rotation_translation();
        // One the camera did not see last frame holds still (its place kept,
        // so it does not lurch when it is seen again): a windy area's
        // leaves otherwise rewrite every mesh every frame [verify: whether
        // the executable animates parts it does not draw].
        if !dangly.seen {
            dangly.last = Some((place, turn));
            continue;
        }
        let blowing = game_to_bevy() * wind.at(to_game * place, dt);
        let stepped = dangly.step((place, turn, scale), dt, blowing);
        if !stepped && !dangly.moving {
            continue;
        }
        dangly.moving = stepped;
        let now = stepped.then(|| dangly.at.iter().map(|at| at.to_array()).collect());
        moved += usize::from(stepped);
        settle(&mut meshes, &mesh.0, now);
    }
    if dump_due.0 {
        let farthest = danglies
            .iter()
            .flat_map(|(dangly, ..)| {
                dangly
                    .at
                    .iter()
                    .zip(&dangly.rest)
                    .map(|(at, rest)| at.distance(*rest))
            })
            .fold(0.0, f32::max);
        facts.0.insert(
            "dangly".into(),
            serde_json::json!({
                "meshes": danglies.iter().len(),
                "moved": moved,
                "farthest_m": farthest,
            }),
        );
    }
}

/// The spring without an app: a still mesh stays at rest, a walk leaves a
/// tip behind with its root pinned, a turn swings the tip within its box,
/// and a jump snaps it back.
#[cfg(feature = "probes")]
pub fn probes() -> crate::probe_kit::Checks {
    use crate::probe_kit::check;
    // A root pinned (constraint 0), a tip that may stray almost 1 m along
    // each axis (displacement 1, constraint 1).
    let tail = || {
        Dangly::new(&DanglyIr {
            displacement: 1.0,
            tightness: 10.0,
            period: 2.0,
            constraints: vec![0.0, 1.0],
            rest: vec![[0.0, 0.0, 0.0], [0.0, -1.0, 0.0]],
        })
    };
    let at = |place: Vec3, turn: Quat| (place, turn, Vec3::ONE);
    let still = at(Vec3::ZERO, Quat::IDENTITY);
    let dt = 0.02;

    let mut dangly = tail();
    let moved = (0..10).any(|_| dangly.step(still, dt, Vec3::ZERO));
    let rests = check(
        !moved && dangly.at == dangly.rest,
        format!("{:?}", dangly.at),
    );

    let mut dangly = tail();
    dangly.step(still, dt, Vec3::ZERO);
    for n in 1..=5 {
        dangly.step(at(Vec3::X * 0.1 * n as f32, Quat::IDENTITY), dt, Vec3::ZERO);
    }
    let trails = check(
        dangly.at[0] == dangly.rest[0] && dangly.at[1].x < -0.1,
        format!("root {:?}, tip {:?}", dangly.at[0], dangly.at[1]),
    );

    let mut dangly = tail();
    dangly.step(still, dt, Vec3::ZERO);
    dangly.step(
        at(
            Vec3::ZERO,
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
        ),
        dt,
        Vec3::ZERO,
    );
    let offset = dangly.at[1] - dangly.rest[1];
    let reach = 1.0 - 1.0 / CONSTRAINT_FULL;
    let swings = check(
        offset.x.abs() > 0.5 && offset.abs().max_element() <= reach + 1e-6,
        format!("tip off rest by {offset:?}, reach {reach}"),
    );

    let mut dangly = tail();
    dangly.step(still, dt, Vec3::ZERO);
    dangly.step(at(Vec3::X * 0.5, Quat::IDENTITY), dt, Vec3::ZERO);
    let strayed = dangly.at != dangly.rest;
    dangly.step(at(Vec3::X * 10.0, Quat::IDENTITY), dt, Vec3::ZERO);
    let snaps = check(
        strayed && dangly.at == dangly.rest,
        format!("strayed {strayed}, then {:?}", dangly.at),
    );

    // A standing mesh in a wind along +X leans its tip downwind, its root
    // pinned, the tip within its box.
    let mut dangly = tail();
    for _ in 0..20 {
        dangly.step(still, dt, Vec3::X * 0.02);
    }
    let lean = dangly.at[1] - dangly.rest[1];
    let leans = check(
        dangly.at[0] == dangly.rest[0] && lean.x > 0.05 && lean.x <= reach + 1e-6,
        format!("tip off rest by {lean:?}"),
    );

    let mut checks = crate::wind::probes();
    checks.extend([
        ("dangly_still_rests", rests),
        ("dangly_wind_leans", leans),
        ("dangly_walk_trails", trails),
        ("dangly_turn_swings_in_box", swings),
        ("dangly_jump_snaps", snaps),
    ]);
    checks
}
