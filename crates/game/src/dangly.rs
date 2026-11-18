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
//! A camera resolving frames over time takes the sway's motion from each
//! slot's previous place, the sway being the mesh's own and no transform's.

use asset_odyssey::model::DanglyIr;
use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef, VertexAttributeValues};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    RenderPipelineDescriptor, SpecializedMeshPipelineError, VertexFormat,
};
use bevy::render::renderer::RenderQueue;
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::{ExtractSchedule, MainWorld, Render, RenderApp, RenderSystems};
use console::{AnimationTime, DumpDue, DumpFacts};
use std::sync::atomic::{AtomicU64, Ordering};

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

pub const SWAY: Handle<ShaderBuffer> = uuid_handle!("6a2f9e41-3c87-4b5d-a1e0-9d74c2b8f356");

const SWAY_SHADER: Handle<Shader> = uuid_handle!("d41c7b93-5e28-4f0a-8c6d-2b9e7a1f3c58");

pub const SLOT: MeshVertexAttribute =
    MeshVertexAttribute::new("DanglySlot", 988_540_940, VertexFormat::Uint32);

/// Bevy's own locations end at 7.
pub const SLOT_LOCATION: u32 = 8;

pub const SLOT_BYTES: usize = 24;

const SLOTS: u32 = 1 << 17;

static UPLOADED: AtomicU64 = AtomicU64::new(0);

pub struct DanglyPlugin;

impl Plugin for DanglyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            sway.after(bevy::transform::TransformSystems::Propagate),
        )
        .add_systems(Last, (note_seen, free_slots));
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .init_resource::<SwayWrites>()
                .add_systems(ExtractSchedule, extract_writes)
                .add_systems(Render, write_sway.in_set(RenderSystems::PrepareResources));
        } else {
            app.add_systems(Last, forget_writes);
        }
    }
}

pub fn init_sway(app: &mut App) {
    load_internal_asset!(app, SWAY_SHADER, "sway.wgsl", crate::shader_checks::wgsl);
    let list =
        ShaderBuffer::with_size(SLOTS as usize * SLOT_BYTES, RenderAssetUsages::RENDER_WORLD);
    let _ = app
        .world_mut()
        .resource_mut::<Assets<ShaderBuffer>>()
        .insert(&SWAY, list);
    app.init_resource::<SwaySlots>();
}

#[derive(Resource, Default)]
pub struct SwaySlots {
    end: u32,
    free: Vec<(u32, u32)>,
    held: HashMap<AssetId<Mesh>, (u32, u32)>,
    held_changed: bool,
    runs: Vec<(u32, u32)>,
    bytes: Vec<u8>,
    refused: u32,
}

impl SwaySlots {
    fn take(&mut self, count: u32) -> Option<u32> {
        if let Some(at) = self.free.iter().position(|&(_, n)| n >= count) {
            let (first, n) = self.free[at];
            if n == count {
                self.free.remove(at);
            } else {
                self.free[at] = (first + count, n - count);
            }
            return Some(first);
        }
        let first = self.end;
        if first + count > SLOTS {
            if self.refused == 0 {
                warn!(
                    target: "dangly",
                    "the sway list is full: a danglymesh of {count} vertices stays at rest"
                );
            }
            self.refused += 1;
            return None;
        }
        self.end += count;
        Some(first)
    }

    fn give_back(&mut self, first: u32, count: u32) {
        let at = self.free.partition_point(|&(f, _)| f < first);
        self.free.insert(at, (first, count));
        if let Some(&(next, n)) = self.free.get(at + 1)
            && first + count == next
        {
            self.free[at].1 += n;
            self.free.remove(at + 1);
        }
        if at > 0 && self.free[at - 1].0 + self.free[at - 1].1 == first {
            self.free[at - 1].1 += self.free[at].1;
            self.free.remove(at);
        }
        if let Some(&(f, n)) = self.free.last()
            && f + n == self.end
        {
            self.end = f;
            self.free.pop();
        }
    }

    fn write(&mut self, first: u32, places: impl Iterator<Item = (Vec3, Vec3)>) {
        let from = self.bytes.len();
        for (now, was) in places {
            for value in now.to_array().into_iter().chain(was.to_array()) {
                self.bytes.extend_from_slice(&value.to_ne_bytes());
            }
        }
        let count = ((self.bytes.len() - from) / SLOT_BYTES) as u32;
        match self.runs.last_mut() {
            Some((last, n)) if *last + *n == first => *n += count,
            _ => self.runs.push((first, count)),
        }
    }

    pub fn hold(&mut self, mesh: AssetId<Mesh>, slotted: &mut Slotted) {
        self.held
            .insert(mesh, (slotted.first, slotted.shown.len() as u32));
        self.held_changed = true;
        slotted.mesh = mesh;
    }
}

pub struct Slotted {
    mesh: AssetId<Mesh>,
    first: u32,
    shown: Vec<Vec3>,
}

pub fn slot(mesh: &mut Mesh, slots: &mut SwaySlots) -> Option<Slotted> {
    let Some(VertexAttributeValues::Float32x3(positions)) =
        mesh.attribute(Mesh::ATTRIBUTE_POSITION)
    else {
        return None;
    };
    let shown: Vec<Vec3> = positions.iter().copied().map(Vec3::from_array).collect();
    let count = u32::try_from(shown.len()).ok()?;
    let first = slots.take(count)?;
    slots.write(first, shown.iter().map(|&at| (at, at)));
    mesh.insert_attribute(SLOT, (first..first + count).collect::<Vec<u32>>());
    Some(Slotted {
        mesh: AssetId::default(),
        first,
        shown,
    })
}

pub fn slot_attribute(
    descriptor: &mut RenderPipelineDescriptor,
    layout: &MeshVertexBufferLayoutRef,
) -> Result<bool, SpecializedMeshPipelineError> {
    if descriptor.vertex.shader_defs.contains(&"SKINNED".into()) || !layout.0.contains(SLOT) {
        return Ok(false);
    }
    let slot = layout
        .0
        .get_layout(&[SLOT.at_shader_location(SLOT_LOCATION)])?;
    if let Some(buffer) = descriptor.vertex.buffers.first_mut() {
        buffer.attributes.extend(slot.attributes);
    }
    Ok(true)
}

fn free_slots(mut events: MessageReader<AssetEvent<Mesh>>, mut slots: ResMut<SwaySlots>) {
    for event in events.read() {
        if let AssetEvent::Removed { id } = event
            && let Some((first, count)) = slots.held.remove(id)
        {
            slots.give_back(first, count);
            slots.held_changed = true;
        }
    }
}

#[derive(Resource)]
pub(crate) struct SwayWrites {
    copy: Vec<u8>,
    changed: Option<(usize, usize)>,
    held: HashMap<AssetId<Mesh>, (u32, u32)>,
    sent: bool,
}

impl Default for SwayWrites {
    fn default() -> SwayWrites {
        SwayWrites {
            copy: vec![0; SLOTS as usize * SLOT_BYTES],
            changed: None,
            held: HashMap::default(),
            sent: false,
        }
    }
}

impl SwayWrites {
    pub(crate) fn run_of(&self, mesh: AssetId<Mesh>) -> Option<(u32, u32)> {
        self.held.get(&mesh).copied()
    }

    pub(crate) fn sent(&self) -> bool {
        self.sent
    }
}

fn extract_writes(mut main: ResMut<MainWorld>, mut writes: ResMut<SwayWrites>) {
    let Some(mut slots) = main.get_resource_mut::<SwaySlots>() else {
        return;
    };
    let slots = &mut *slots;
    if std::mem::take(&mut slots.held_changed) {
        writes.held.clone_from(&slots.held);
    }
    let mut from = 0;
    for &(first, count) in &slots.runs {
        let to = from + count as usize * SLOT_BYTES;
        let at = first as usize * SLOT_BYTES;
        let end = at + (to - from);
        writes.copy[at..end].copy_from_slice(&slots.bytes[from..to]);
        writes.changed = Some(match writes.changed {
            Some((low, high)) => (low.min(at), high.max(end)),
            None => (at, end),
        });
        from = to;
    }
    slots.runs.clear();
    slots.bytes.clear();
}

pub(crate) fn forget_writes(mut slots: ResMut<SwaySlots>) {
    slots.runs.clear();
    slots.bytes.clear();
}

fn write_sway(
    lists: Res<RenderAssets<GpuShaderBuffer>>,
    queue: Res<RenderQueue>,
    mut writes: ResMut<SwayWrites>,
) {
    writes.sent = false;
    let Some((low, high)) = writes.changed else {
        return;
    };
    let Some(list) = lists.get(SWAY.id()) else {
        return;
    };
    queue.write_buffer(&list.buffer, low as u64, &writes.copy[low..high]);
    UPLOADED.store((high - low) as u64, Ordering::Relaxed);
    writes.changed = None;
    writes.sent = true;
}

/// A drawn danglymesh's spring and where its vertices are now, on its mesh's
/// entity.
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
    moving: bool,
    mesh: AssetId<Mesh>,
    first: u32,
    shown: Vec<Vec3>,
}

impl Dangly {
    pub fn new(ir: &DanglyIr, Slotted { mesh, first, shown }: Slotted) -> Dangly {
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
            mesh,
            first,
            shown,
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

/// A constraint below 0 lets a vertex stray further than the displacement.
pub fn reach(ir: &DanglyIr) -> f32 {
    let free = ir
        .constraints
        .iter()
        .filter(|&&constraint| constraint != 0.0)
        .map(|constraint| (1.0 - constraint / CONSTRAINT_FULL).abs())
        .fold(1.0, f32::max);
    (ir.displacement * free).abs()
}

/// Whether a camera saw each danglymesh this frame, for `sway` next frame
/// (its own frame's is not known until after it has run).
fn note_seen(mut danglies: Query<(&mut Dangly, &ViewVisibility)>) {
    for (mut dangly, seen) in &mut danglies {
        dangly.seen = seen.get();
    }
}

/// Each danglymesh one frame on, its slots written when a vertex moved.
/// A danglymesh whose entity now shows another mesh is left alone.
fn sway(
    time: Res<Time>,
    mut slots: ResMut<SwaySlots>,
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
    let mut moved = 0;
    if dt <= 0.0 {
        // A frozen clock. What moved last frame is still now.
        for (mut dangly, _, mesh) in &mut danglies {
            if mesh.id() == dangly.mesh && std::mem::take(&mut dangly.moving) {
                slots.write(dangly.first, dangly.shown.iter().map(|&at| (at, at)));
            }
        }
    } else {
        wind.tick(dt);
        let dt = dt.clamp(DT_MIN, DT_MAX);
        let to_game = game_to_bevy().inverse();
        for (mut dangly, global, mesh) in &mut danglies {
            if mesh.id() != dangly.mesh {
                continue;
            }
            let (scale, turn, place) = global.to_scale_rotation_translation();
            // One the camera did not see last frame holds still (its place
            // kept, so it does not lurch when it is seen again). A windy
            // area's leaves would otherwise write every mesh every frame
            // [verify: whether the executable animates parts it does not
            // draw].
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
            moved += usize::from(stepped);
            let Dangly {
                first, at, shown, ..
            } = &mut *dangly;
            if stepped {
                slots.write(*first, at.iter().copied().zip(shown.iter().copied()));
                shown.clone_from(at);
            } else {
                slots.write(*first, shown.iter().map(|&at| (at, at)));
            }
        }
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
                "runs": slots.runs.len(),
                "written_bytes": slots.bytes.len(),
                "uploaded_bytes": UPLOADED.load(Ordering::Relaxed),
                "refused": slots.refused,
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
        let ir = DanglyIr {
            displacement: 1.0,
            tightness: 10.0,
            period: 2.0,
            constraints: vec![0.0, 1.0],
            rest: vec![[0.0, 0.0, 0.0], [0.0, -1.0, 0.0]],
        };
        let shown = ir.rest.iter().copied().map(Vec3::from_array).collect();
        Dangly::new(
            &ir,
            Slotted {
                mesh: AssetId::default(),
                first: 0,
                shown,
            },
        )
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
