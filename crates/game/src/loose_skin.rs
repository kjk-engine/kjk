//! Robe skirts and capes pushed out of their wearer's legs on the GPU, for
//! https://github.com/kjk-engine/roadmap/issues/5. The robe solver
//! (`loose_parts`) moves only a garment's bones. The skin between two bones
//! is blended between them, so a leg that swings between two bones passes
//! through the cloth. After the skin, the vertex shader `loose_skin.wgsl`
//! pushes each vertex of the garment out of the six leg capsules of its
//! wearer: the thighs, the shins and the feet. It keeps nothing between
//! frames, so it can never get stuck.
//!
//! - A mesh that the solver moves gets the vertex attribute `PUSH` when it
//!   is spawned (`loose_parts::push`). Its x is how much of the way out the
//!   vertex is pushed, all of it for every vertex of such a mesh. Its y is how
//!   deep the vertex rests inside a leg capsule in the rest pose, so that
//!   nothing is pushed at rest. A mesh with the attribute takes this
//!   module's vertex shader in the main pass and in the prepass
//!   (`pushed`), and no other mesh does. The shadow tiles push it the same
//!   way (`shadow_tiles`), and leave it out of a model's shadow meshes.
//! - The capsules are in the storage list `LEGS`, one entry for each model
//!   of the scene, found by the model in the mesh's tag
//!   (`scene::owner_tag`). Each entry holds the six capsules in the world
//!   this frame and the frame before, for the motion vectors.
//!   `loose_parts` writes the entries of the bodies it solves (`LegWrites`)
//!   after Bevy has placed every node, and this module sends them to the
//!   GPU.

use bevy::asset::{RenderAssetUsages, load_internal_asset, uuid_handle};
use bevy::mesh::{MeshVertexAttribute, MeshVertexBufferLayoutRef};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::render_asset::RenderAssets;
use bevy::render::render_resource::{
    RenderPipelineDescriptor, SpecializedMeshPipelineError, VertexFormat,
};
use bevy::render::renderer::RenderQueue;
use bevy::render::storage::{GpuShaderBuffer, ShaderBuffer};
use bevy::render::{ExtractSchedule, MainWorld, Render, RenderApp, RenderSystems};
use bevy::shader::ShaderDefVal;

pub const LEGS: Handle<ShaderBuffer> = uuid_handle!("3b8d5e17-6c42-4f9a-b0e3-71d2a94c6f08");

pub(crate) const SHADER: Handle<Shader> = uuid_handle!("c7a13f62-2e9b-4d58-a64f-0b5e8d1c3a97");

const LEGS_SHADER: Handle<Shader> = uuid_handle!("5e0f94a8-b1d3-4c27-9f86-2a7c3e81d4b6");

pub const PUSH: MeshVertexAttribute =
    MeshVertexAttribute::new("LoosePush", 988_540_941, VertexFormat::Float32x4);

/// Bevy's own locations end at 7, and the danglymeshes' slot is at 8.
pub const PUSH_LOCATION: u32 = 9;

/// Its location in the shadow tiles' pass (`shadow_tiles.wgsl`).
pub(crate) const SHADOW_PUSH_LOCATION: u32 = 8;

/// `loose_legs.wgsl`'s `LEG_CAPSULES`: the thighs, the shins and the feet.
pub(crate) const LEG_CAPSULES: usize = 6;

/// `loose_legs.wgsl`'s `BODY_CAPSULES`: the legs first, then the pelvis,
/// the belly, the chest, the head and the arms.
pub(crate) const CAPSULES: usize = 14;

/// `loose_legs.wgsl`'s `CHAINS`: the most chains of a garment that fold and
/// flutter. A chain past the last counts as the last.
pub(crate) const CHAINS: usize = 16;

/// One entry of `LEGS`, as `vec4`s: the capsules this frame, then the frame
/// before, then the chains' ripples this frame and the frame before. Each
/// capsule is two `vec4`s, its ends with its radius in the first's w. Each
/// ripple is three (`Ripple::packed`).
type Entry = [[f32; 4]; 4 * CAPSULES + 6 * CHAINS];

pub(crate) const ENTRY_BYTES: usize = std::mem::size_of::<Entry>();

// `loose_legs.wgsl`'s `Legs` holds `BODY_CAPSULES` capsules of 32 bytes and
// `CHAINS` ripples of 48 bytes, for each of the two frames.
const _: () = assert!(ENTRY_BYTES == 2 * (CAPSULES * 32 + CHAINS * 48));

/// How many models of a scene can have an entry. A model past the last
/// one is not pushed.
pub(crate) const ENTRIES: usize = 4096;

pub struct LooseSkinPlugin;

impl Plugin for LooseSkinPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LegWrites>();
        if let Some(render) = app.get_sub_app_mut(RenderApp) {
            render
                .init_resource::<LegWrites>()
                .add_systems(ExtractSchedule, extract_writes)
                .add_systems(Render, write_legs.in_set(RenderSystems::PrepareResources));
        } else {
            app.add_systems(Last, forget_writes);
        }
    }
}

/// The shader and the list, which every Odyssey material binds.
pub fn init_legs(app: &mut App) {
    load_internal_asset!(
        app,
        LEGS_SHADER,
        "loose_legs.wgsl",
        crate::shader_checks::wgsl
    );
    load_internal_asset!(app, SHADER, "loose_skin.wgsl", crate::shader_checks::wgsl);
    let list = ShaderBuffer::with_size(ENTRIES * ENTRY_BYTES, RenderAssetUsages::RENDER_WORLD);
    let _ = app
        .world_mut()
        .resource_mut::<Assets<ShaderBuffer>>()
        .insert(&LEGS, list);
}

/// Whether the bodies the robe solver moves are drawn in flat colours, a
/// debugging aid: `KJK_LOOSE_PAINT=1`. The robe or cape is magenta, each
/// boot green, and everything else black, with no light, so that a
/// recording can count the boot's pixels that show through the robe
/// (`loose_parts::paint`).
pub(crate) fn painted() -> bool {
    static PAINTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PAINTED.get_or_init(|| std::env::var("KJK_LOOSE_PAINT").as_deref() == Ok("1"))
}

/// Whether robes and capes catch a soft rim of light at a grazing angle,
/// `KJK_LOOSE_SHEEN=1` (`loose_skin.wgsl`'s `SHEEN`).
fn sheen() -> bool {
    static SHEEN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SHEEN.get_or_init(|| std::env::var("KJK_LOOSE_SHEEN").as_deref() == Ok("1"))
}

/// How far out of a leg capsule's radius a pushed vertex is put (metres).
/// The capsules are the size of the limbs, and the cloth lies on them.
pub(crate) const CLOTH_OFF_LEG: f32 = 0.01;

/// A leg capsule in the world: its ends, its radius in its node's space,
/// and its node's scale, which grows both the radius and the depth that a
/// vertex rests inside it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LegCapsule {
    pub(crate) ends: [Vec3; 2],
    pub(crate) radius: f32,
    pub(crate) scale: f32,
}

impl LegCapsule {
    /// A capsule of radius 0, which pushes nothing.
    pub(crate) const NONE: LegCapsule = LegCapsule {
        ends: [Vec3::ZERO; 2],
        radius: 0.0,
        scale: 0.0,
    };

    /// As `loose_legs.wgsl` reads it: the first end with the radius,
    /// `CLOTH_OFF_LEG` added and scaled, then the second end with the
    /// scale.
    fn packed(self) -> [[f32; 4]; 2] {
        let [a, b] = self.ends;
        [
            [a.x, a.y, a.z, (self.radius + CLOTH_OFF_LEG) * self.scale],
            [b.x, b.y, b.z, self.scale],
        ]
    }
}

/// A vertex's two chains and how far across from the first to the second
/// it lies, from 0 to 1, packed into one number as `loose_legs.wgsl`'s
/// `cloth_at` reads it.
pub(crate) fn pack_chains(first: usize, second: usize, across: f32) -> f32 {
    let last = CHAINS - 1;
    (first.min(last)
        + 16 * second.min(last)
        + 256 * (across.clamp(0.0, 1.0) * 255.0).round() as usize) as f32
}

/// The chains and the share across of `pack_chains`.
pub(crate) fn unpack_chains(packed: f32) -> (usize, usize, f32) {
    let ids = packed.round() as usize;
    (ids & 15, (ids >> 4) & 15, (ids >> 8) as f32 / 255.0)
}

/// The `PUSH` attribute of a vertex in the middle of an edge, from its two
/// ends (`loose_subdivide`). It rests inside every capsule that either end
/// rests inside, and as deep as the deeper. It hangs halfway down between
/// them. Its two chains are the two that weigh most over both ends.
pub(crate) fn push_between(a: [f32; 4], b: [f32; 4]) -> [f32; 4] {
    let mask = |x: f32| (x.round() as u32).saturating_sub(1);
    let inside = if a[0] <= 0.0 || b[0] <= 0.0 {
        0.0
    } else {
        1.0 + (mask(a[0]) | mask(b[0])) as f32
    };
    let mut weights = [0.0f32; CHAINS];
    for push in [a, b] {
        let (first, second, across) = unpack_chains(push[3]);
        weights[first] += (1.0 - across) / 2.0;
        weights[second] += across / 2.0;
    }
    let mut order: Vec<usize> = (0..CHAINS).collect();
    order.sort_by(|&x, &y| weights[y].total_cmp(&weights[x]));
    let (first, second) = (order[0], order[1]);
    let both = weights[first] + weights[second];
    let chains = match weights[second] > 0.0 && both > 0.0 {
        true => pack_chains(first, second, weights[second] / both),
        false => pack_chains(first, first, 0.0),
    };
    [inside, a[1].max(b[1]), (a[2] + b[2]) / 2.0, chains]
}

/// What the vertex shader needs of one chain of a garment to fold and
/// flutter its cloth (`loose_legs.wgsl`'s `rippled`).
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Ripple {
    /// The chain's way down from its root to its tip, in the world.
    pub(crate) way: Vec3,
    /// How far the hem flutters at most (metres).
    pub(crate) height: f32,
    /// Where the flutter is in its cycle (radians).
    pub(crate) phase: f32,
    /// How much the chain's neighbours have closed in on it, from 0 (at
    /// their rest distance or farther) to 1 (touching). The cloth between
    /// two chains folds out by it.
    pub(crate) squeeze: f32,
    /// The middle of the chain's points, in the world.
    pub(crate) middle: Vec3,
}

impl Ripple {
    fn packed(self) -> [[f32; 4]; 3] {
        let (w, m) = (self.way, self.middle);
        [
            [w.x, w.y, w.z, self.height],
            [self.phase, self.squeeze, 0.0, 0.0],
            [m.x, m.y, m.z, 0.0],
        ]
    }
}

/// The entries to send this frame, each with its model, and each model's
/// capsules and ripples sent last, which are the next entry's frame
/// before.
#[derive(Resource, Default)]
pub(crate) struct LegWrites {
    entries: Vec<(usize, Entry)>,
    last: HashMap<usize, ([LegCapsule; CAPSULES], [Ripple; CHAINS])>,
}

impl LegWrites {
    /// Sends model `model`'s capsules and ripples this frame, with those it
    /// sent the frame before. A model past `ENTRIES` is left out.
    pub(crate) fn set(
        &mut self,
        model: usize,
        now: [LegCapsule; CAPSULES],
        ripples: [Ripple; CHAINS],
    ) {
        if model >= ENTRIES {
            return;
        }
        let (was, ripples_was) = (self.last)
            .insert(model, (now, ripples))
            .unwrap_or((now, ripples));
        let capsules = now.iter().chain(&was).flat_map(|capsule| capsule.packed());
        let ripples = ripples
            .iter()
            .chain(&ripples_was)
            .flat_map(|ripple| ripple.packed());
        let mut entry: Entry = [[0.0; 4]; 4 * CAPSULES + 6 * CHAINS];
        for (packed, value) in entry.iter_mut().zip(capsules.chain(ripples)) {
            *packed = value;
        }
        self.entries.push((model, entry));
    }

    /// Empties model `model`'s entry, for a model that no longer wears a
    /// garment the solver moves. Its index may come back for another model.
    pub(crate) fn free(&mut self, model: usize) {
        if model < ENTRIES && self.last.remove(&model).is_some() {
            let mut entry: Entry = [[0.0; 4]; 4 * CAPSULES + 6 * CHAINS];
            let none = LegCapsule::NONE.packed();
            for pair in entry[..4 * CAPSULES].chunks_mut(2) {
                pair.copy_from_slice(&none);
            }
            self.entries.push((model, entry));
        }
    }
}

fn extract_writes(mut main: ResMut<MainWorld>, mut writes: ResMut<LegWrites>) {
    if let Some(mut written) = main.get_resource_mut::<LegWrites>() {
        writes.entries.append(&mut written.entries);
    }
}

fn forget_writes(mut writes: ResMut<LegWrites>) {
    writes.entries.clear();
}

fn write_legs(
    lists: Res<RenderAssets<GpuShaderBuffer>>,
    queue: Res<RenderQueue>,
    mut writes: ResMut<LegWrites>,
) {
    let Some(list) = lists.get(LEGS.id()) else {
        writes.entries.clear();
        return;
    };
    for (model, entry) in writes.entries.drain(..) {
        let bytes: Vec<u8> = entry
            .iter()
            .flatten()
            .flat_map(|value| value.to_ne_bytes())
            .collect();
        queue.write_buffer(&list.buffer, (model * ENTRY_BYTES) as u64, &bytes);
    }
}

/// Gives a mesh with the `PUSH` attribute this module's vertex shader. It
/// returns whether the mesh has the attribute.
pub(crate) fn pushed(
    descriptor: &mut RenderPipelineDescriptor,
    layout: &MeshVertexBufferLayoutRef,
) -> Result<bool, SpecializedMeshPipelineError> {
    if !layout.0.contains(PUSH) {
        return Ok(false);
    }
    let push = layout
        .0
        .get_layout(&[PUSH.at_shader_location(PUSH_LOCATION)])?;
    if let Some(buffer) = descriptor.vertex.buffers.first_mut() {
        buffer.attributes.extend(push.attributes);
    }
    descriptor
        .vertex
        .shader_defs
        .push(ShaderDefVal::UInt("LOOSE_PUSH".into(), PUSH_LOCATION));
    if sheen() {
        descriptor.vertex.shader_defs.push("LOOSE_SHEEN".into());
    }
    descriptor.vertex.shader = SHADER;
    Ok(true)
}
