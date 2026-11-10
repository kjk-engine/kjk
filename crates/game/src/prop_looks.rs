//! What breaking and hurting a prop looks like, drawn from the prop's own
//! mesh and texture (`ragdoll::shatter`): a breakable prop flies apart into
//! pieces of itself, its base left standing. Looks-only: nothing here is
//! synced or saved; every machine cuts the same pieces from the model's
//! name and finds the rest itself.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use asset_odyssey::model::{MeshIr, ModelIr};
use asset_odyssey::texture::{TextureFacts, TextureIr};
use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use ragdoll::shatter::{self, Piece, Plain};
use ragdoll::{DebrisId, DebrisShape};
use sim::destruction::{Class, Material, Size};
use sim::look;

use crate::body_marks;
use crate::glow_shell::GlowMaterial;
use crate::made::Made;
use crate::material::{self, OdysseyMaterial, Surface};
use crate::prop_breaks::{PropChanges, PropsSeen};
use crate::scene::{
    AnimatedMaterials, DrawnBlades, MeshPart, SceneContent, SceneModel, base_mesh, game_to_bevy,
};
use crate::script_fx::{HookVisual, HookVisuals, Place};
use crate::shot_fx::BoltEnds;
use crate::sim_host::{PresentedSnapshot, SceneSwap};

/// How many pieces a prop is cut into, by size (small, medium, large).
pub(crate) const PIECES: [usize; 3] = [6, 10, 16];
/// The share of a prop's height its base stands to.
const BASE_SHARE: f32 = 0.25;
/// How fast pieces fly (m/s): from a blow, from a blast; how much they
/// rise for each metre out; how fast they spin at most (rad/s).
const FLING: (f32, f32) = (2.0, 4.0);
const BLAST: (f32, f32) = (5.0, 8.0);
const RISE: f32 = 0.8;
const SPIN: f32 = 8.0;
/// A piece bigger than this (m³, a fist) stays for the visit.
const KEPT_M3: f32 = 0.001;
/// At most this many pieces fly or lie at once.
const MAX_PIECES: usize = 256;
/// A small piece fades after this long (s, seeded between), shrinking
/// away over `SHRINK_S`.
const FADE_S: (f64, f64) = (10.0, 20.0);
const SHRINK_S: f64 = 0.5;
/// How far a small and a medium prop jolt from a blow (degrees), and how
/// soon they settle (ms).
const SHAKE_DEG: [f32; 2] = [6.0, 3.0];
const SHAKE_MS: f32 = 150.0;
/// A prop keeps its newest this many marks, each this wide (half-width,
/// m, seeded between).
const MARKS: [usize; 2] = [8, 24];
const MARK_M: (f32, f32) = (0.08, 0.14);
/// How much wider a mark's soot is than the mark.
const SOOT_SCALE: f32 = 3.0;
/// A blow throws this many shards (seeded between), this fast (m/s), gone
/// within 2 s (ms until they shrink away).
const SHARDS: (usize, usize) = (3, 6);
const SHARD_SPEED: (f32, f32) = (2.0, 3.5);
const SHARD_FADE_MS: f64 = 1500.0;
/// More dents at each damage stage, how wide and deep (m), by size.
const DENTS: [usize; 3] = [2, 2, 4];
const DENT_RADIUS_M: [f32; 3] = [0.15, 0.15, 0.3];
const DENT_DEPTH_M: [f32; 3] = [0.04, 0.04, 0.08];
/// A prop's material at each damage stage (0, 1, 2 and on) is this dark.
const DARKENED: [f32; 3] = [1.0, 0.9, 0.8];
/// A mesh glowing more than this in any channel is a screen; a dead screen
/// is this dark on top; a scarred one flickers every so often (ms, seeded
/// between).
const SCREEN_GLOW: f32 = 0.1;
const SCREEN_DEAD: f32 = 0.2;
const FLICKER_MS: (f32, f32) = (80.0, 300.0);
/// A blade whose hilt is this near a prop's feet (m) may have struck it; a
/// blow from this near is a fist's or a kick's.
const BLADE_REACH_M: f32 = 3.0;
const BLUNT_REACH_M: f32 = 3.0;
/// How long the blades' places are kept to find where a swing met a prop
/// (ms): the sim's hit may come a frame or two after the blade passed.
const TRAIL_MS: f64 = 150.0;
/// Segments laid between two frames of a blade, sweeping it.
const SWEEP_STEPS: usize = 8;

/// What a destruction detail level (`display::Debris`) draws: whether
/// pieces fly at all and the small ones too, how many pieces at most,
/// whether hits throw shards, every how many of the glass's shards burst
/// (0: none) and how many at most, whether the glass glints, how many
/// lasting emitters burn at most and whether they light what is near.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Detail {
    pub flies: bool,
    pub small_pieces: bool,
    pub most_pieces: usize,
    pub shards: bool,
    pub glass_every: usize,
    pub most_glass: usize,
    pub glass_burst: bool,
    pub emitters: usize,
    pub lights: bool,
}

/// What `display`'s destruction detail draws (all of it with no display:
/// a headless check).
pub(crate) fn detail(display: Option<&crate::display::Display>) -> Detail {
    use crate::display::Debris;
    match display.map_or(Debris::Full, |display| display.debris) {
        Debris::Full => Detail {
            flies: true,
            small_pieces: true,
            most_pieces: MAX_PIECES,
            shards: true,
            glass_every: 1,
            most_glass: 256,
            glass_burst: true,
            emitters: 24,
            lights: true,
        },
        Debris::Low => Detail {
            flies: true,
            small_pieces: false,
            most_pieces: 96,
            shards: false,
            glass_every: 3,
            most_glass: 64,
            glass_burst: true,
            emitters: 8,
            lights: false,
        },
        Debris::Off => Detail {
            flies: false,
            small_pieces: false,
            most_pieces: MAX_PIECES,
            shards: false,
            glass_every: 0,
            most_glass: 0,
            glass_burst: false,
            emitters: 0,
            lights: false,
        },
    }
}

/// A number from 0 to 1 (1 never) from `parts` and `salt`, the same on
/// every machine: FNV-1a 64 of their bits, then splitmix64's finish.
pub(crate) fn noise(parts: &[f32], salt: u64) -> f32 {
    let hash = parts
        .iter()
        .flat_map(|part| part.to_bits().to_le_bytes())
        .chain(salt.to_le_bytes())
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
    let mut z = hash.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
}

/// A prop's pieces whose middle is in the lowest quarter of its `height`
/// (model frame, its feet at 0), or that touch the `floor` (their lowest
/// point within `TOUCH_M` of it: a table's legs): the base that stays where
/// it stood; at least the lowest piece of a set that has any.
pub(crate) fn base(pieces: &[Piece], height: f32, floor: f32) -> Vec<usize> {
    let low = |piece: &Piece| (piece.hull.iter().map(|p| p[2])).fold(piece.middle[2], f32::min);
    let standing: Vec<usize> = (0..pieces.len())
        .filter(|&i| {
            pieces[i].middle[2] < height * BASE_SHARE || low(&pieces[i]) - floor <= TOUCH_M
        })
        .collect();
    if !standing.is_empty() {
        return standing;
    }
    (0..pieces.len())
        .min_by(|&a, &b| low(&pieces[a]).total_cmp(&low(&pieces[b])))
        .into_iter()
        .collect()
}

/// How near the floor a piece's lowest point is to stand on it (metres).
const TOUCH_M: f32 = 0.05;

/// How a piece at `middle` flies from a blow from `from` (game space,
/// metres): away from it, up and out; harder for a blast.
pub(crate) fn fling(middle: [f32; 3], from: [f32; 3], blast: bool) -> [f32; 3] {
    let (dx, dy) = (middle[0] - from[0], middle[1] - from[1]);
    let flat = (dx * dx + dy * dy).sqrt();
    let (ox, oy) = if flat > 1e-4 {
        (dx / flat, dy / flat)
    } else {
        (1.0, 0.0)
    };
    let length = (1.0 + RISE * RISE).sqrt();
    let (lo, hi) = if blast { BLAST } else { FLING };
    let speed = lo + (hi - lo) * noise(&[middle, from].concat(), u64::from(blast));
    [ox, oy, RISE].map(|c| c / length * speed)
}

/// How a piece at `middle` spins as it flies (rad/s).
fn spin(middle: [f32; 3]) -> [f32; 3] {
    [1, 2, 3].map(|salt| (noise(&middle, salt) * 2.0 - 1.0) * SPIN)
}

/// Whether a piece of `volume` (m³) stays for the visit (bigger than a
/// fist) rather than fading.
pub(crate) fn kept(volume: f32) -> bool {
    volume > KEPT_M3
}

/// How long a piece lasts: it fades after a while, stays for the visit, or
/// is a broken prop's base (it stands as long as the prop is broken).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Lasts {
    Fades,
    Visit,
    Base,
}

/// Of the live pieces (`(born ms, how long it lasts)`), the ones to fade to
/// bring them down to `cap`: the oldest fading first, then the oldest kept;
/// never a base (a broken prop must not vanish whole).
pub(crate) fn evict(live: &[(u64, Lasts)], cap: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..live.len())
        .filter(|&i| live[i].1 != Lasts::Base)
        .collect();
    order.sort_by_key(|&i| (live[i].1, live[i].0));
    order.truncate(live.len().saturating_sub(cap));
    order
}

/// Whether a prop's screens die in a burst of sparks as it is drawn at
/// `stage` from `drawn`; `arrived`: first seen so (already wrecked when
/// the area loaded).
pub(crate) fn dies(drawn: u8, stage: u8, arrived: bool) -> bool {
    !arrived && drawn < 2 && stage >= 2
}

/// The blade segments a blow may have come from: each frame's in `trail`
/// (oldest first, each blade by its index), and between two frames in a
/// row, the blade swept from one to the next.
pub(crate) fn swept(trail: &[Vec<Option<[Vec3; 2]>>]) -> Vec<[Vec3; 2]> {
    let mut segments: Vec<[Vec3; 2]> = trail.iter().flatten().flatten().copied().collect();
    for pair in trail.windows(2) {
        for (was, now) in pair[0].iter().zip(&pair[1]) {
            let (Some([h0, t0]), Some([h1, t1])) = (was, now) else {
                continue;
            };
            segments.extend((1..SWEEP_STEPS).map(|step| {
                let k = step as f32 / SWEEP_STEPS as f32;
                [h0.lerp(*h1, k), t0.lerp(*t1, k)]
            }));
        }
    }
    segments
}

/// Whether the segment `a`–`b` passes through the box `min`–`max`.
fn enters_box(segment: [Vec3; 2], min: Vec3, max: Vec3) -> bool {
    meets_box(segment, min, max).is_some()
}

/// Where the segment `a`–`b` first meets the box `min`–`max`, if it does:
/// the point and the outward normal of the face it comes in by (zero for a
/// segment that starts inside).
fn meets_box([a, b]: [Vec3; 2], min: Vec3, max: Vec3) -> Option<(Vec3, Vec3)> {
    let way = b - a;
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut normal = Vec3::ZERO;
    for axis in 0..3 {
        if way[axis].abs() < 1e-6 {
            if a[axis] < min[axis] || a[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let (t0, t1) = (
            (min[axis] - a[axis]) / way[axis],
            (max[axis] - a[axis]) / way[axis],
        );
        if t0.min(t1) > lo {
            lo = t0.min(t1);
            normal = Vec3::ZERO;
            normal[axis] = -way[axis].signum();
        }
        hi = hi.min(t0.max(t1));
    }
    (lo <= hi).then_some((a + way * lo, normal))
}

/// What a prop's look and lost hit points (`(look, damage)`) going from
/// `was` (`None`: first seen) to `now` show of its wreck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Wreck {
    /// Standing, or nothing changed.
    Standing,
    /// Already broken when first seen: its base only, no flight, no burst.
    BaseOnly,
    /// Broken now: its base stays, the rest flies.
    Flies,
}

pub(crate) fn wreck(was: Option<(u8, u16)>, now: (u8, u16)) -> Wreck {
    let dead = now.0 == look::DEAD;
    match was {
        None if dead => Wreck::BaseOnly,
        Some(was) if dead && was.0 != look::DEAD => Wreck::Flies,
        _ => Wreck::Standing,
    }
}

/// A model cut into pieces: the pieces and each one's collider (made with
/// the cut, off the main thread, and shared by every copy of the model that
/// breaks), which of the model's meshes each of their parts is (by its
/// index among those cut), and how tall the model stands (m).
pub(crate) struct Fracture {
    pub(crate) pieces: Vec<Piece>,
    shapes: Vec<Option<DebrisShape>>,
    /// Each piece's parts as meshes, made the first time any copy of the
    /// model breaks and drawn by every one after.
    drawn: std::sync::OnceLock<Vec<PieceMeshes>>,
    pub(crate) meshes: Vec<usize>,
    pub(crate) height: f32,
}

/// Each breakable prop's model cut into pieces, by model name, once ready:
/// kept from area to area (the cut is the model's).
#[derive(Resource, Default)]
pub(crate) struct Fractures {
    ready: BTreeMap<String, Arc<Fracture>>,
    cutting: BTreeMap<String, Task<Fracture>>,
    /// Each destructible prop's model's meshes that can be cut, and which
    /// mesh each is (`cuttable`).
    plains: BTreeMap<String, Arc<Cuttable>>,
}

/// A model's meshes that can be cut (model frame) and which mesh each is,
/// and its glass (`prop_glass`): each glass mesh's number and itself.
type Cuttable = (Vec<Plain>, Vec<usize>, Vec<(usize, Plain)>);

/// The meshes of `ir` that can be cut, in the model's frame (at rest: each
/// node's rest transform, a skinned mesh posed by its bones, a dangly one
/// as it hangs still), and which mesh each is: every drawn mesh with
/// triangles that shows at rest, a lightmapped one only where the model has
/// nothing else (a piece has no lightmap uvs); capped unless its surface is
/// alpha-tested, blended or two-sided. Its glass is apart: it breaks as
/// glass (`prop_glass`), not into the prop's pieces.
fn cuttable(ir: &ModelIr, textures: &BTreeMap<String, TextureIr>) -> Cuttable {
    let drawn =
        |mesh: &&MeshIr| mesh.texture.is_some() && !mesh.indices.is_empty() && mesh.alpha > 0.0;
    let lightmapped =
        |mesh: &MeshIr| mesh.lightmap.is_some() && mesh.uv2.len() == mesh.positions.len();
    let plain_only = ir
        .meshes
        .iter()
        .filter(drawn)
        .any(|mesh| !lightmapped(mesh));
    let rest = ir.rest();
    let mut cut: Cuttable = (Vec::new(), Vec::new(), Vec::new());
    for (number, mesh) in (ir.meshes.iter().enumerate())
        .filter(|(_, mesh)| drawn(mesh) && !(plain_only && lightmapped(mesh)))
    {
        let normals = ir
            .placed(mesh, &rest, &mesh.normals, 0.0)
            .into_iter()
            .map(|n| {
                let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                n.map(|c| c / length)
            });
        let facts: Vec<_> = mesh
            .texture
            .iter()
            .filter_map(|name| textures.get(name).map(|texture| &texture.facts))
            .collect();
        let surface = material::surface(
            (mesh.transparency_hint, false),
            mesh.alpha,
            (facts.first().copied(), &facts),
        );
        let open = facts.iter().any(|facts| facts.two_sided || facts.decal);
        let plain = Plain {
            positions: ir.placed(mesh, &rest, &mesh.positions, 1.0),
            normals: normals.collect(),
            uvs: mesh.uv1.clone(),
            indices: mesh.indices.clone(),
            capped: matches!(surface, Surface::Opaque | Surface::OpaqueEnvMasked) && !open,
        };
        let node = ir
            .nodes
            .get(mesh.node)
            .map_or("", |node| node.name.as_str());
        if crate::prop_glass::is_glass(mesh, node, surface, &facts) {
            cut.2.push((number, plain));
        } else {
            cut.0.push(plain);
            cut.1.push(number);
        }
    }
    cut
}

impl Fractures {
    /// Starts cutting model `ir` into `count` pieces, off the main thread,
    /// unless it is cut or being cut.
    pub(crate) fn cut(
        &mut self,
        ir: &ModelIr,
        textures: &BTreeMap<String, TextureIr>,
        count: usize,
    ) {
        let name = ir.name.to_ascii_lowercase();
        if self.ready.contains_key(&name) || self.cutting.contains_key(&name) {
            return;
        }
        let cut = self.plains(ir, textures);
        let numbers = cut.1.clone();
        let seed = shatter::seed_of(&name);
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let pieces = shatter::shatter(&cut.0, count, seed);
            let shapes = (pieces.iter())
                .map(|piece| DebrisShape::hull(&piece.hull))
                .collect();
            let height = pieces
                .iter()
                .flat_map(|piece| &piece.hull)
                .map(|p| p[2])
                .fold(0.0f32, f32::max);
            Fracture {
                pieces,
                shapes,
                drawn: std::sync::OnceLock::new(),
                meshes: numbers,
                height,
            }
        });
        self.cutting.insert(name, task);
    }

    /// Takes in the cuts that are done.
    pub(crate) fn collect(&mut self) {
        let done: Vec<String> = (self.cutting.iter())
            .filter(|(_, task)| task.is_finished())
            .map(|(name, _)| name.clone())
            .collect();
        for name in done {
            if let Some(fracture) = self
                .cutting
                .remove(&name)
                .and_then(|task| block_on(poll_once(task)))
            {
                self.ready.insert(name, Arc::new(fracture));
            }
        }
    }

    /// Model `ir`'s meshes that can be cut, made the first time.
    pub(crate) fn plains(
        &mut self,
        ir: &ModelIr,
        textures: &BTreeMap<String, TextureIr>,
    ) -> Arc<Cuttable> {
        (self.plains.entry(ir.name.to_ascii_lowercase()))
            .or_insert_with(|| Arc::new(cuttable(ir, textures)))
            .clone()
    }

    pub(crate) fn get(&self, ir: &ModelIr) -> Option<&Arc<Fracture>> {
        self.ready.get(&ir.name.to_ascii_lowercase())
    }
}

/// A piece of a broken prop as drawn: its body while it flies (`None`: a
/// base piece, or one come to rest), when it came, how long it lasts, when
/// it starts to fade (ms of `Time`).
#[derive(Component)]
pub(crate) struct Flying {
    debris: Option<DebrisId>,
    born_ms: f64,
    lasts: Lasts,
    fade_ms: Option<f64>,
}

/// A prop broken whose pieces are not out yet (its model or cut not ready),
/// and whether they fly.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Owed {
    flies: bool,
    from: [f32; 3],
    blast: bool,
    size: Size,
}

impl Owed {
    /// How many pieces its model is cut into.
    pub(crate) fn pieces(&self) -> usize {
        PIECES[self.size as usize]
    }
}

/// The pieces' world (the level's faces around each break) and the props
/// broken whose pieces are owed, for the scene swap they belong to.
#[derive(Resource, Default)]
pub(crate) struct PropLooks {
    generation: u32,
    world: ragdoll::World,
    floor: bool,
    owed: BTreeMap<u32, Owed>,
    done: BTreeSet<u32>,
    /// This frame's blows, by prop.
    impacts: BTreeMap<u32, Impact>,
    /// Each prop's marks (each one a stage entity or two), newest last.
    /// Each with where it struck (model frame).
    marks: BTreeMap<u32, VecDeque<(Vec3, Vec<Entity>)>>,
    /// Props jolting from a blow.
    jolts: BTreeMap<u32, Jolt>,
    /// The blades as drawn these last frames (ms of `Time`), oldest first.
    trail: VecDeque<(f64, Vec<Option<[Vec3; 2]>>)>,
    /// Each damaged prop's stage as the sim has it (and whether it was
    /// first seen so), and as drawn.
    wanted: BTreeMap<u32, (u8, sim::destruction::Destructible, [f32; 3], bool)>,
    staged: BTreeMap<u32, Staged>,
}

/// A damaged prop as drawn: its stage, and each of its meshes' own
/// material.
struct Staged {
    stage: u8,
    meshes: Vec<StagedMesh>,
}

/// A damaged prop's mesh with its own copy of its material: the copy's
/// tint and glow as the model has them, whether it is a screen, and when a
/// scarred screen next flickers (ms of `Time`) and whether it is lit.
struct StagedMesh {
    number: usize,
    copy: Handle<OdysseyMaterial>,
    tint: Vec4,
    self_illum: Vec4,
    screen: bool,
    next_ms: f64,
    lit: bool,
}

/// What struck a prop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Blow {
    Bolt,
    Blade,
    Blunt,
    Blast,
}

/// Where a blow landed on a prop (game space, metres), the way the prop's
/// face there looks, and what struck.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Impact {
    at: Vec3,
    normal: Vec3,
    blow: Blow,
}

/// A prop's root jolting from a blow: where it rests, the axis it tips
/// about (its parent's space), since when (ms of `Time`), and its kind.
struct Jolt {
    root: Entity,
    rest: Transform,
    axis: Vec3,
    since_ms: f64,
    size: Size,
    class: Class,
}

/// What the pieces cost since the last dump (`destruction` in its facts):
/// the frames, the pieces' world's steps (ms: total, worst), and the most
/// pieces drawn and bodies moving in a frame.
#[derive(Resource, Default)]
struct Cost {
    frames: u32,
    step_ms: (f64, f64),
    pieces: usize,
    moving: usize,
}

impl Cost {
    fn add(&mut self, step_ms: f64, pieces: usize, moving: usize) {
        self.frames += 1;
        self.step_ms = (self.step_ms.0 + step_ms, self.step_ms.1.max(step_ms));
        self.pieces = self.pieces.max(pieces);
        self.moving = self.moving.max(moving);
    }

    /// The facts, and the window started over.
    fn facts(&mut self) -> serde_json::Value {
        let cost = std::mem::take(self);
        serde_json::json!({
            "frames": cost.frames,
            "step_ms_mean": cost.step_ms.0 / f64::from(cost.frames.max(1)),
            "step_ms_max": cost.step_ms.1,
            "pieces_max": cost.pieces,
            "moving_max": cost.moving,
        })
    }
}

pub struct PropLooksPlugin;

impl Plugin for PropLooksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fractures>()
            .init_resource::<PropLooks>()
            .init_resource::<Cost>()
            .add_systems(
                PostUpdate,
                (start_over, hits, stages, wrecks, fly)
                    .chain()
                    .after(PropsSeen)
                    .after(crate::scene::BladesDrawn)
                    .after(crate::shot_fx::ShotsSeen)
                    .run_if(resource_exists::<PresentedSnapshot>),
            );
    }
}

/// The model of prop `index` in the scene and its model root, once drawn.
pub(crate) fn model_of(
    index: u32,
    (scene, made): (&SceneContent, Option<&Made>),
    scene_models: &Query<(Entity, &SceneModel)>,
) -> Option<(usize, Entity)> {
    let model = (scene.0.instance_models.get(&index).copied())
        .or_else(|| made.and_then(|made| made.prop_model(index)))?;
    let root = (scene_models.iter())
        .find(|(_, scene_model)| scene_model.index == model)
        .map(|(root, _)| root)?;
    Some((model, root))
}

/// Where a blow on a prop at `position` came from: the nearest creature
/// within 10 m, or under the prop (its pieces rise straight up).
pub(crate) fn blow_from(position: [f32; 3], creatures: &[Vec3]) -> [f32; 3] {
    let at = Vec3::from_array(position);
    (creatures.iter())
        .filter(|c| c.distance(at) > 0.1 && c.distance(at) <= 10.0)
        .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)))
        .map_or([position[0], position[1], position[2] - 1.0], |c| {
            c.to_array()
        })
}

/// Each drawn mesh's place in its model, and its material.
pub(crate) type MeshParts<'w, 's> =
    Query<'w, 's, (&'static MeshPart, &'static MeshMaterial3d<OdysseyMaterial>)>;

/// Breakable props: their models cut once first seen; broken ones hidden
/// and their pieces put in their place (or owed until the cut is ready).
#[allow(clippy::too_many_arguments)]
pub(crate) fn wrecks(
    mut commands: Commands,
    (presented, swap, scene, made): (
        Res<PresentedSnapshot>,
        Res<SceneSwap>,
        Res<SceneContent>,
        Option<Res<Made>>,
    ),
    changes: Res<PropChanges>,
    (mut fractures, mut looks): (ResMut<Fractures>, ResMut<PropLooks>),
    scene_models: Query<(Entity, &SceneModel)>,
    (globals, children, parts): (Query<&GlobalTransform>, Query<&Children>, MeshParts),
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
    damage: Option<Res<crate::prop_damage::PropDamage>>,
    display: Option<Res<crate::display::Display>>,
) {
    let detail = detail(display.as_deref());
    fractures.collect();
    let made = made.as_deref();
    for change in &changes.changes {
        if change.d.class != Class::Break || change.own_dead {
            continue;
        }
        if change.was.is_none()
            && let Some((model, _)) = model_of(change.index, (&scene, made), &scene_models)
        {
            let count = PIECES[change.d.size as usize];
            fractures.cut(&scene.0.models[model].ir, &scene.0.textures, count);
        }
        let flies = match wreck(change.was, change.now) {
            Wreck::Standing => continue,
            Wreck::BaseOnly => false,
            Wreck::Flies => detail.flies,
        };
        if !looks.done.contains(&change.index) {
            let (from, blast) = looks
                .blow_on(change.index)
                .unwrap_or((blow_from(change.position, &changes.creatures), false));
            let size = change.d.size;
            looks.owed.insert(
                change.index,
                Owed {
                    flies,
                    from,
                    blast,
                    size,
                },
            );
        }
    }
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    let newest = presented.1.last().map(|snapshot| &**snapshot);
    let owed: Vec<(u32, Owed)> = looks.owed.iter().map(|(&i, &o)| (i, o)).collect();
    for (index, owe) in owed {
        let Some((model, root)) = model_of(index, (&scene, made), &scene_models) else {
            continue;
        };
        let ir = &scene.0.models[model].ir;
        let Some(fracture) = fractures.get(ir).cloned() else {
            if !fractures
                .cutting
                .contains_key(&ir.name.to_ascii_lowercase())
            {
                fractures.cut(ir, &scene.0.textures, owe.pieces());
            }
            continue;
        };
        looks.owed.remove(&index);
        looks.done.insert(index);
        commands.entity(root).try_insert(Visibility::Hidden);
        let Ok(global) = globals.get(root) else {
            continue;
        };
        let game = in_game(global);
        let materials: BTreeMap<usize, Handle<OdysseyMaterial>> = (children.iter_descendants(root))
            .filter_map(|entity| parts.get(entity).ok())
            .map(|(part, material)| (part.0, material.0.clone()))
            .collect();
        let standing = base(&fracture.pieces, fracture.height, 0.0);
        info!(
            "prop #{index} ({}) at {:.1?}: {} pieces, {} standing, {}",
            ir.name,
            game.translation,
            fracture.pieces.len(),
            standing.len(),
            if owe.flies {
                "the rest fly"
            } else {
                "already broken"
            }
        );
        // Knocked off before (`prop_damage`): gone already.
        let none = BTreeSet::new();
        let gone = (damage.as_deref().and_then(|damage| damage.gone(index))).unwrap_or(&none);
        let out: Vec<(usize, bool)> = crate::prop_damage::remaining(fracture.pieces.len(), gone)
            .into_iter()
            .map(|number| (number, standing.contains(&number)))
            .filter(|&(_, stays)| owe.flies || stays)
            .collect();
        lay_pieces(
            &mut commands,
            &mut meshes,
            looks.split(),
            (&swap.level, newest),
            (&fracture, &out),
            (game, &ir.name, &materials),
            (owe.from, owe.blast, detail),
            now_ms,
        );
    }
}

/// Pieces `out` of `fracture` (each by its number, and whether it stays: a
/// base) of model `name` at `game` in its meshes' `materials`: those that
/// stay stand, the rest are thrown as debris by a blow from `from` (a
/// `blast`) into the pieces' world, gathered round them from `level`; the
/// small ones fade, or are not thrown at all where `detail` leaves them out.
#[allow(clippy::too_many_arguments)]
pub(crate) fn lay_pieces(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    LooksWorld { world, floor }: LooksWorld,
    level: (&sim::Level, Option<&sim::Snapshot>),
    (fracture, out): (&Fracture, &[(usize, bool)]),
    (game, name, materials): (Transform, &str, &BTreeMap<usize, Handle<OdysseyMaterial>>),
    (from, blast, detail): ([f32; 3], bool, Detail),
    now_ms: f64,
) {
    if out.iter().any(|&(_, stays)| !stays) {
        crate::ragdolls::gather(world, floor, level, game.translation, game.translation.z);
    }
    for &(number, stays) in out {
        let Some(piece) = fracture.pieces.get(number) else {
            continue;
        };
        let middle = game
            .transform_point(Vec3::from_array(piece.middle))
            .to_array();
        let shape = fracture.shapes.get(number).and_then(Option::as_ref);
        let debris = shape.filter(|_| !stays).map(|shape| {
            world.debris(
                shape,
                game.translation.to_array(),
                game.rotation.to_array(),
                fling(middle, from, blast),
                spin(middle),
            )
        });
        let small = !kept(piece.volume);
        if small && !stays && !detail.small_pieces {
            continue;
        }
        let fade_ms = (small && !stays).then(|| {
            let (lo, hi) = FADE_S;
            now_ms + (lo + (hi - lo) * f64::from(noise(&middle, 9))) * 1000.0
        });
        spawn_piece(
            commands,
            (
                &fracture.meshes_of(meshes, number),
                &fracture.meshes,
                materials,
            ),
            (game, format!("{name} piece {number}")),
            Flying {
                debris,
                born_ms: now_ms,
                lasts: match (stays, small) {
                    (true, _) => Lasts::Base,
                    (false, true) => Lasts::Fades,
                    (false, false) => Lasts::Visit,
                },
                fade_ms,
            },
        );
    }
}

/// The things that strike props as drawn this frame: blaster bolts' last
/// runs, and lit blades (hilt, tip), game space.
type Strikers<'w> = (Option<Res<'w, BoltEnds>>, Option<Res<'w, DrawnBlades>>);

/// What a mark is made with: the meshes, the marks' looks and materials.
type MarkMakers<'w> = (
    ResMut<'w, Assets<Mesh>>,
    Option<ResMut<'w, body_marks::Looks>>,
    ResMut<'w, Assets<GlowMaterial>>,
);

/// The model's place in game space (the scene's root turns game space to
/// Bevy's), from its root's.
pub(crate) fn in_game(global: &GlobalTransform) -> Transform {
    Transform::from_matrix(Mat4::from_quat(game_to_bevy().inverse()) * global.to_matrix())
}

/// The triangles of `plains` (model frame) placed by `game`.
fn placed_triangles<'a>(
    plains: impl IntoIterator<Item = &'a Plain>,
    game: &Transform,
) -> Vec<[[f32; 3]; 3]> {
    plains
        .into_iter()
        .flat_map(|plain| {
            plain.indices.as_chunks::<3>().0.iter().map(|t| {
                t.map(|v| {
                    game.transform_point(Vec3::from_array(plain.positions[v as usize]))
                        .to_array()
                })
            })
        })
        .collect()
}

/// How far short of a prop's box a bolt's run may end and still have
/// ended on it (metres): where the sim's trace stops.
const BOX_EDGE_M: f32 = 0.02;

/// A prop's box as blows meet it: least and greatest corners in its
/// model's frame, placed in game space by `game`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Boxed {
    pub game: Transform,
    pub min: Vec3,
    pub max: Vec3,
}

impl Boxed {
    /// Its model's box at rest (`ModelIr::rest_bounds`, as the sim meets
    /// it), or one as big as its `size` where the model draws nothing.
    pub(crate) fn of(ir: &ModelIr, size: Size, game: Transform) -> Boxed {
        let (min, max) = match ir.rest_bounds() {
            Some([x0, y0, z0, x1, y1, z1]) => (Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1)),
            None => {
                let half = rules::destruction::bodies::HALF_M[size as usize];
                let height = rules::destruction::bodies::HEIGHT_M[size as usize];
                (Vec3::new(-half, -half, 0.0), Vec3::new(half, half, height))
            }
        };
        Boxed { game, min, max }
    }
}

/// Where a blow on a prop landed: a bolt's run or a blade crossing its own
/// triangles (`triangles`, game space) first; else a bolt's run ending on
/// its box (`boxed`), the face it struck; else the point on the box facing
/// `from`, where the blow came from: a blade's when one passed through the
/// box, a fist or kick when that is within reach, a blast or power
/// otherwise.
pub(crate) fn impact(
    triangles: &[[[f32; 3]; 3]],
    (bolts, blades): (&[[Vec3; 2]], &[[Vec3; 2]]),
    boxed: &Boxed,
    from: Vec3,
) -> Impact {
    let crossing = |runs: &[[Vec3; 2]], blow| {
        runs.iter().find_map(|[a, b]| {
            let (at, normal) = shatter::first_crossing(a.to_array(), b.to_array(), triangles)?;
            Some(Impact {
                at: Vec3::from_array(at),
                normal: Vec3::from_array(normal),
                blow,
            })
        })
    };
    let feet = boxed.game.translation;
    let near: Vec<[Vec3; 2]> = (blades.iter())
        .filter(|[hilt, _]| hilt.distance(feet) <= BLADE_REACH_M)
        .copied()
        .collect();
    if let Some(found) = crossing(bolts, Blow::Bolt).or_else(|| crossing(&near, Blow::Blade)) {
        return found;
    }
    let to_own = boxed.game.compute_affine().inverse();
    let own = |[a, b]: [Vec3; 2]| [to_own.transform_point3(a), to_own.transform_point3(b)];
    let to_game = |at: Vec3, normal: Vec3| {
        (
            boxed.game.transform_point(at),
            (boxed.game.rotation * normal).normalize_or_zero(),
        )
    };
    // A bolt's run that ended on the box (it stops where the sim's met it,
    // a hair inside or short): the face it came in by.
    let edge = Vec3::splat(BOX_EDGE_M);
    let ended = bolts.iter().find_map(|&run| {
        let (at, normal) = meets_box(own(run), boxed.min - edge, boxed.max + edge)?;
        (normal != Vec3::ZERO).then_some((at, normal))
    });
    if let Some((at, normal)) = ended {
        let (at, normal) = to_game(at + normal * BOX_EDGE_M * -1.0, normal);
        return Impact {
            at,
            normal,
            blow: Blow::Bolt,
        };
    }
    let (middle, halves) = ((boxed.min + boxed.max) / 2.0, (boxed.max - boxed.min) / 2.0);
    let blade = (near.iter()).any(|&segment| enters_box(own(segment), boxed.min, boxed.max));
    let way = (to_own.transform_point3(from) - middle)
        .try_normalize()
        .unwrap_or(Vec3::Z);
    // The face the way leaves the box by: its axis has the least room.
    let (axis, reach) = (0..3)
        .filter(|&i| way[i].abs() > 1e-4)
        .map(|i| (i, halves[i] / way[i].abs()))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((2, halves.z));
    let mut normal = Vec3::ZERO;
    normal[axis] = way[axis].signum();
    let blow = if blade {
        Blow::Blade
    } else if from.distance(feet) <= BLUNT_REACH_M {
        Blow::Blunt
    } else {
        Blow::Blast
    };
    let (at, normal) = to_game(middle + way * reach, normal);
    Impact { at, normal, blow }
}

/// Materials that throw shards of themselves when struck (the rest spark
/// or puff dust).
fn throws_shards(material: Material) -> bool {
    matches!(
        material,
        Material::Wood | Material::Ceramic | Material::Stone | Material::Plastic | Material::Debris
    )
}

/// Blows on props: where each landed (kept for a break's pieces), and for a
/// prop left standing, its impact there, shards of itself, a mark that
/// stays and a jolt.
/// A scene swapped in: every piece of the old one gone and the looks
/// started over, first in the frame, so the props first seen with it (their
/// changes come once) are taken in by every system after.
fn start_over(
    mut commands: Commands,
    swap: Res<SceneSwap>,
    mut looks: ResMut<PropLooks>,
    flying: Query<Entity, With<Flying>>,
) {
    if looks.generation == swap.generation {
        return;
    }
    let left = flying.iter().count();
    for entity in &flying {
        commands.entity(entity).try_despawn();
    }
    info!("scene swap {}: {left} prop pieces cleared", swap.generation);
    *looks = PropLooks {
        generation: swap.generation,
        ..PropLooks::default()
    };
}

#[allow(clippy::too_many_arguments)]
fn hits(
    mut commands: Commands,
    (presented, swap, scene, made): (
        Res<PresentedSnapshot>,
        Res<SceneSwap>,
        Res<SceneContent>,
        Option<Res<Made>>,
    ),
    changes: Res<PropChanges>,
    (mut fractures, mut looks): (ResMut<Fractures>, ResMut<PropLooks>),
    scene_models: Query<(Entity, &SceneModel)>,
    (globals, children, parts, transforms): (
        Query<&GlobalTransform>,
        Query<&Children>,
        MeshParts,
        Query<&Transform>,
    ),
    (bolts, blades): Strikers,
    (mut meshes, mut mark_looks, mut glows): MarkMakers,
    (visuals, damage): (
        Option<ResMut<HookVisuals>>,
        Option<Res<crate::prop_damage::PropDamage>>,
    ),
    time: Res<Time>,
    display: Option<Res<crate::display::Display>>,
) {
    let detail = detail(display.as_deref());
    if looks.generation != swap.generation {
        return;
    }
    looks.impacts.clear();
    let made = made.as_deref();
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    let newest = presented.1.last().map(|snapshot| &**snapshot);
    let bolts = bolts.as_deref().map_or(&[][..], |bolts| &bolts.0);
    looks.trail.push_back((
        now_ms,
        blades.as_deref().map_or_else(Vec::new, |b| b.0.clone()),
    ));
    while looks
        .trail
        .front()
        .is_some_and(|(at, _)| now_ms - at > TRAIL_MS)
    {
        looks.trail.pop_front();
    }
    let frames: Vec<Vec<Option<[Vec3; 2]>>> = looks
        .trail
        .iter()
        .map(|(_, blades)| blades.clone())
        .collect();
    let blades = swept(&frames);
    let mut sparked = Vec::new();
    for change in &changes.changes {
        let Some(was) = change.was else {
            continue;
        };
        if change.now.1 <= was.1 {
            continue;
        }
        let Some((model, root)) = model_of(change.index, (&scene, made), &scene_models) else {
            continue;
        };
        let Ok(global) = globals.get(root) else {
            continue;
        };
        let ir = &scene.0.models[model].ir;
        let game = in_game(global);
        let cut = fractures.plains(ir, &scene.0.textures);
        let feet = Vec3::from_array(change.position);
        let from = Vec3::from_array(blow_from(change.position, &changes.creatures));
        let found = impact(
            // Its glass too: a blade meets the glass before what is behind.
            &placed_triangles(
                cut.0.iter().chain(cut.2.iter().map(|(_, glass)| glass)),
                &game,
            ),
            (bolts, &blades),
            &Boxed::of(ir, change.d.size, game),
            from,
        );
        debug!(
            "prop #{} ({}) struck by {:?} at {:.2?} facing {:.2?}",
            change.index, ir.name, found.blow, found.at, found.normal
        );
        looks.impacts.insert(change.index, found);
        if wreck(change.was, change.now) != Wreck::Standing {
            continue;
        }
        sparked.push(HookVisual {
            row: crate::prop_breaks::hit_row(change.d.material),
            place: Place::At(found.at),
        });
        let materials: BTreeMap<usize, Handle<OdysseyMaterial>> = (children.iter_descendants(root))
            .filter_map(|entity| parts.get(entity).ok())
            .map(|(part, material)| (part.0, material.0.clone()))
            .collect();
        if detail.shards && throws_shards(change.d.material) {
            let LooksWorld { world, floor } = looks.split();
            crate::ragdolls::gather(world, floor, (&swap.level, newest), feet, feet.z);
            let seed = u64::from(change.index) << 32 | u64::from(change.now.1);
            let count = SHARDS.0
                + (noise(&found.at.to_array(), seed) * (SHARDS.1 - SHARDS.0 + 1) as f32) as usize;
            let near = game.compute_affine().inverse().transform_point3(found.at);
            for (number, shard) in shatter::shards(&cut.0, near.to_array(), count, seed)
                .iter()
                .enumerate()
            {
                let middle = game.transform_point(Vec3::from_array(shard.middle));
                let speed = SHARD_SPEED.0
                    + (SHARD_SPEED.1 - SHARD_SPEED.0) * noise(&middle.to_array(), seed);
                let velocity = (found.normal + Vec3::Z * 0.5).normalize_or(Vec3::Z) * speed;
                let debris = DebrisShape::hull(&shard.hull).map(|shape| {
                    world.debris(
                        &shape,
                        game.translation.to_array(),
                        game.rotation.to_array(),
                        velocity.to_array(),
                        spin(middle.to_array()),
                    )
                });
                spawn_piece(
                    &mut commands,
                    (&made_meshes(&mut meshes, shard), &cut.1, &materials),
                    (game, format!("{} shard {number}", ir.name)),
                    Flying {
                        debris,
                        born_ms: now_ms,
                        lasts: Lasts::Fades,
                        fade_ms: Some(now_ms + SHARD_FADE_MS),
                    },
                );
            }
        }
        // No mark over a gap a chunk left (`prop_damage`).
        let struck = game.compute_affine().inverse().transform_point3(found.at);
        let missing =
            (damage.as_deref()).map_or_else(BTreeSet::new, |damage| damage.missing(change.index));
        let over_gap = (fractures.get(ir)).is_some_and(|fracture| {
            crate::prop_damage::over_gap(struck.to_array(), &fracture.pieces, &missing)
        });
        if let Some(mark_looks) = mark_looks.as_deref_mut().filter(|_| !over_gap) {
            let laid = lay_mark(
                &mut commands,
                (&mut meshes, mark_looks, &mut glows),
                (ir, &children, &parts, &globals),
                (root, found),
                noise(&found.at.to_array(), u64::from(change.now.1)),
            );
            if !laid.is_empty() {
                let marks = looks.marks.entry(change.index).or_default();
                if let Some((_, gone)) = keep_newest(marks, (struck, laid), change.d.class) {
                    for gone in gone {
                        commands.entity(gone).try_despawn();
                    }
                }
            }
        }
        if shake(change.d.size, change.d.class, 0.0) > 0.0
            && let Ok(rest) = transforms.get(root)
        {
            let away = Vec3::new(-found.normal.x, -found.normal.y, 0.0);
            if let Some(axis) = Vec3::Z.cross(away).try_normalize() {
                let rest = looks
                    .jolts
                    .get(&change.index)
                    .map_or(*rest, |jolt| jolt.rest);
                looks.jolts.insert(
                    change.index,
                    Jolt {
                        root,
                        rest,
                        axis,
                        since_ms: now_ms,
                        size: change.d.size,
                        class: change.d.class,
                    },
                );
            }
        }
    }
    if let Some(mut visuals) = visuals {
        visuals.0.extend(sparked);
    }
}

/// Lays a mark where `found` struck the model `ir` drawn under `root`, on
/// each of its static meshes' own triangles there: a scorch (`body_marks`'
/// burn) for every blow, glowing too for a blade. `turn` (0 to 1) turns it.
/// The marks' entities (none if nothing was covered).
fn lay_mark(
    commands: &mut Commands,
    (meshes, looks, glows): (
        &mut Assets<Mesh>,
        &mut body_marks::Looks,
        &mut Assets<GlowMaterial>,
    ),
    (ir, children, parts, globals): (
        &ModelIr,
        &Query<&Children>,
        &MeshParts,
        &Query<&GlobalTransform>,
    ),
    (root, found): (Entity, Impact),
    turn: f32,
) -> Vec<Entity> {
    let ray = -found.normal;
    let (size, soot) = mark_sizes(turn);
    let spin = turn * std::f32::consts::TAU;
    // The soot under it first, wide and faint, then its own stages.
    let soot = looks.soot(glows).map(|made| ((made, false), soot));
    let stages: Vec<_> = (soot.into_iter())
        .chain(
            (looks.stages(glows).into_iter())
                .filter(|(_, glows)| !glows || found.blow == Blow::Blade)
                .map(|stage| (stage, size)),
        )
        .collect();
    let mut laid = Vec::new();
    for entity in children.iter_descendants(root) {
        let (Ok((part, _)), Ok(global)) = (parts.get(entity), globals.get(entity)) else {
            continue;
        };
        let Some(mesh) = ir
            .meshes
            .get(part.0)
            .filter(|m| m.skin.is_none() && m.dangly.is_none())
        else {
            continue;
        };
        let game = in_game(global);
        let at: Vec<Vec3> = (mesh.positions.iter())
            .map(|&p| game.transform_point(Vec3::from_array(p)))
            .collect();
        let mut source = base_mesh(
            &mesh.positions,
            &mesh.normals,
            &mesh.uv1,
            RenderAssetUsages::default(),
        );
        source.insert_indices(Indices::U32(mesh.indices.clone()));
        let mut covered: BTreeMap<u32, Handle<Mesh>> = BTreeMap::new();
        for ((stage, glowing), across) in &stages {
            let axes = body_marks::mark_axes(ray, spin, (*across, *across));
            let key = across.to_bits();
            let covered = match covered.get(&key) {
                Some(made) => made.clone(),
                None => {
                    let Some(made) = body_marks::gore(&source, &at, (found.at, ray), axes) else {
                        continue;
                    };
                    let made = meshes.add(made);
                    covered.insert(key, made.clone());
                    made
                }
            };
            let mut spawned = commands.spawn((
                Mesh3d(covered.clone()),
                MeshMaterial3d(stage.clone()),
                Transform::IDENTITY,
                NotShadowCaster,
            ));
            if *glowing {
                spawned.insert(body_marks::Glowing);
            }
            let spawned = spawned.id();
            commands.entity(entity).add_child(spawned);
            laid.push(spawned);
        }
    }
    laid
}

/// A piece's parts as meshes: which of its model's cut meshes each is, and
/// the mesh.
pub(crate) type PieceMeshes = Vec<(usize, Handle<Mesh>)>;

/// `piece`'s parts as meshes, new.
fn made_meshes(meshes: &mut Assets<Mesh>, piece: &Piece) -> PieceMeshes {
    (piece.meshes.iter())
        .map(|(source, plain)| {
            let mut mesh = base_mesh(
                &plain.positions,
                &plain.normals,
                &plain.uvs,
                RenderAssetUsages::RENDER_WORLD,
            );
            mesh.insert_indices(Indices::U32(plain.indices.clone()));
            (*source, meshes.add(mesh))
        })
        .collect()
}

impl Fracture {
    /// Piece `number`'s meshes: every piece's made the first time.
    pub(crate) fn meshes_of(&self, meshes: &mut Assets<Mesh>, number: usize) -> PieceMeshes {
        let drawn = self.drawn.get_or_init(|| {
            (self.pieces.iter())
                .map(|piece| made_meshes(meshes, piece))
                .collect()
        });
        drawn.get(number).cloned().unwrap_or_else(Vec::new)
    }
}

/// A piece (or shard) of a model drawn as `parts`, placed by `game`, in its
/// source meshes' own materials (`numbers`: which mesh each cut one is), as
/// `flying`.
fn spawn_piece(
    commands: &mut Commands,
    (parts, numbers, materials): (
        &PieceMeshes,
        &[usize],
        &BTreeMap<usize, Handle<OdysseyMaterial>>,
    ),
    (game, name): (Transform, String),
    flying: Flying,
) {
    let entity = commands
        .spawn((
            Transform::from_rotation(game_to_bevy()) * game,
            Visibility::default(),
            flying,
            Name::new(name),
        ))
        .id();
    piece_meshes(commands, (parts, numbers, materials), entity);
}

/// A piece's `parts` as children of `entity`, each in its source mesh's
/// material (`numbers`: which mesh each cut one is), casting no shadow.
pub(crate) fn piece_meshes(
    commands: &mut Commands,
    (parts, numbers, materials): (
        &PieceMeshes,
        &[usize],
        &BTreeMap<usize, Handle<OdysseyMaterial>>,
    ),
    entity: Entity,
) {
    for (source, mesh) in parts {
        let Some(material) = numbers.get(*source).and_then(|m| materials.get(m)) else {
            continue;
        };
        let child = commands
            .spawn((
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                NotShadowCaster,
            ))
            .id();
        commands.entity(entity).add_child(child);
    }
}

/// What a damage stage is drawn with: the meshes, the materials and those
/// that scroll or flip.
type StageMakers<'w> = (
    ResMut<'w, Assets<OdysseyMaterial>>,
    Option<ResMut<'w, AnimatedMaterials>>,
);

/// Whether a clip of `ir`'s own moves node `node`.
pub(crate) fn keyed(ir: &ModelIr, node: usize) -> bool {
    let Some(name) = ir.nodes.get(node).map(|node| &node.name) else {
        return false;
    };
    (ir.animations.iter().flat_map(|clip| &clip.tracks)).any(|track| {
        track.node.eq_ignore_ascii_case(name)
            && (track.positions.len() > 1 || track.orientations.len() > 1)
    })
}

/// Damaged props drawn at their stage (`sim::destruction::stage`): their
/// own copy of each material darkened; a damage-class one's screens
/// flickering, then dead with a burst of sparks (its dents and chunks are
/// `prop_damage`'s). Collision never changes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn stages(
    mut commands: Commands,
    (swap, scene, made): (Res<SceneSwap>, Res<SceneContent>, Option<Res<Made>>),
    changes: Res<PropChanges>,
    mut looks: ResMut<PropLooks>,
    scene_models: Query<(Entity, &SceneModel)>,
    (children, parts): (Query<&Children>, MeshParts),
    (mut materials, mut animated): StageMakers,
    visuals: Option<ResMut<HookVisuals>>,
    time: Res<Time>,
) {
    if looks.generation != swap.generation {
        return;
    }
    for change in &changes.changes {
        if change.was.is_none() {
            debug!(
                "prop #{} ({:?}) stands at {:.1?}",
                change.index, change.d, change.position
            );
        }
        let stage = sim::destruction::stage(change.d, change.now.1);
        if stage > 0 && change.now.0 != look::DEAD {
            looks.wanted.insert(
                change.index,
                (stage, change.d, change.position, change.was.is_none()),
            );
        }
    }
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    let made = made.as_deref();
    let mut sparked = Vec::new();
    let due: Vec<(u32, u8, sim::destruction::Destructible, [f32; 3], bool)> = (looks.wanted.iter())
        .filter(|(index, (stage, ..))| {
            looks
                .staged
                .get(index)
                .is_none_or(|drawn| drawn.stage < *stage)
        })
        .map(|(&index, &(stage, d, feet, arrived))| (index, stage, d, feet, arrived))
        .collect();
    for (index, stage, d, feet, arrived) in due {
        let Some((model, root)) = model_of(index, (&scene, made), &scene_models) else {
            continue;
        };
        let ir = &scene.0.models[model].ir;
        let drawn: Vec<(Entity, usize, Handle<OdysseyMaterial>)> = (children
            .iter_descendants(root))
        .filter_map(|entity| {
            let (part, material) = parts.get(entity).ok()?;
            Some((entity, part.0, material.0.clone()))
        })
        .collect();
        // Each mesh's own material, copied the first time.
        let staged = looks.staged.entry(index).or_insert_with(|| {
            let mut own = Vec::new();
            for (entity, number, material) in &drawn {
                let Some(copy) = materials.get(material).cloned() else {
                    continue;
                };
                let (tint, self_illum) = (copy.params.tint, copy.params.self_illum);
                let copy = materials.add(copy);
                if let Some(animated) = animated.as_deref_mut() {
                    animated.copied(material, &copy);
                }
                commands
                    .entity(*entity)
                    .try_insert(MeshMaterial3d(copy.clone()));
                let mesh = &ir.meshes[*number];
                let facts = mesh
                    .texture
                    .as_ref()
                    .and_then(|name| scene.0.textures.get(name))
                    .map(|t| &t.facts);
                own.push(StagedMesh {
                    number: *number,
                    copy,
                    tint,
                    self_illum,
                    screen: d.class == Class::Damage && screen(mesh, facts),
                    next_ms: now_ms,
                    lit: true,
                });
            }
            Staged {
                stage: 0,
                meshes: own,
            }
        });

        debug!(
            "prop #{index} ({}) at stage {stage}: {} meshes, {} screens",
            ir.name,
            staged.meshes.len(),
            staged.meshes.iter().filter(|mesh| mesh.screen).count()
        );
        let dies = dies(staged.stage, stage, arrived);
        staged.stage = stage;
        for mesh in &staged.meshes {
            let Some(mut material) = materials.get_mut(&mesh.copy) else {
                continue;
            };
            let dark = darkened(stage)
                * if mesh.screen && stage >= 2 {
                    SCREEN_DEAD
                } else {
                    1.0
                };
            material.params.tint = (mesh.tint.truncate() * dark).extend(mesh.tint.w);
            if mesh.screen && stage >= 2 {
                material.params.self_illum = Vec4::ZERO;
                if let Some(animated) = animated.as_deref_mut() {
                    animated.stop(mesh.copy.id());
                }
            }
        }
        if dies && staged.meshes.iter().any(|mesh| mesh.screen) {
            let middle = Vec3::from_array(feet)
                + Vec3::Z * rules::destruction::bodies::HEIGHT_M[d.size as usize] / 2.0;
            sparked.push(HookVisual {
                row: crate::prop_breaks::hit_row(Material::Electronic),
                place: Place::At(middle),
            });
        }
    }
    // Scarred screens flicker.
    for (&index, staged) in &mut looks.staged {
        if staged.stage != 1 {
            continue;
        }
        for mesh in staged
            .meshes
            .iter_mut()
            .filter(|mesh| mesh.screen && now_ms >= mesh.next_ms)
        {
            mesh.lit = !mesh.lit;
            let roll = noise(&[mesh.next_ms as f32], u64::from(index));
            mesh.next_ms = now_ms + f64::from(FLICKER_MS.0 + (FLICKER_MS.1 - FLICKER_MS.0) * roll);
            if let Some(mut material) = materials.get_mut(&mesh.copy) {
                material.params.self_illum = if mesh.lit {
                    mesh.self_illum
                } else {
                    Vec4::ZERO
                };
            }
        }
    }
    if let Some(mut visuals) = visuals {
        visuals.0.extend(sparked);
    }
}

/// The world and its floor flag, borrowed apart from the rest.
pub(crate) struct LooksWorld<'a> {
    world: &'a mut ragdoll::World,
    floor: &'a mut bool,
}

impl PropLooks {
    /// Whether it is the scene swap `generation`'s (`wrecks` starts it over
    /// on a new one).
    pub(crate) fn current(&self, generation: u32) -> bool {
        self.generation == generation
    }

    /// Takes away prop `index`'s marks struck where `gone` says (model
    /// frame).
    pub(crate) fn drop_marks(
        &mut self,
        index: u32,
        gone: impl Fn(Vec3) -> bool,
        commands: &mut Commands,
    ) {
        if let Some(marks) = self.marks.get_mut(&index) {
            marks.retain(|(at, entities)| {
                let keep = !gone(*at);
                if !keep {
                    entities
                        .iter()
                        .for_each(|&entity| commands.entity(entity).try_despawn());
                }
                keep
            });
        }
    }

    /// Prop `index`'s meshes' own (staged) materials, by mesh number, once
    /// it is drawn damaged.
    pub(crate) fn copies(&self, index: u32) -> BTreeMap<usize, Handle<OdysseyMaterial>> {
        (self.staged.get(&index).into_iter())
            .flat_map(|staged| &staged.meshes)
            .map(|mesh| (mesh.number, mesh.copy.clone()))
            .collect()
    }

    /// Where this frame's blow on prop `index` came from (a little off the
    /// face it struck, game space) and whether it was a blast.
    pub(crate) fn blow_on(&self, index: u32) -> Option<([f32; 3], bool)> {
        let impact = self.impacts.get(&index)?;
        Some((
            (impact.at + impact.normal).to_array(),
            impact.blow == Blow::Blast,
        ))
    }

    pub(crate) fn split(&mut self) -> LooksWorld<'_> {
        LooksWorld {
            world: &mut self.world,
            floor: &mut self.floor,
        }
    }
}

/// The pieces fly on: their world steps, each follows its body until it
/// rests (then lies where it came to, out of the world), the small ones
/// fade, and the oldest go when there are too many.
fn fly(
    mut commands: Commands,
    mut looks: ResMut<PropLooks>,
    mut pieces: Query<(Entity, &mut Flying, &mut Transform)>,
    time: Res<Time>,
    (mut cost, mut facts): (ResMut<Cost>, ResMut<console::DumpFacts>),
    display: Option<Res<crate::display::Display>>,
) {
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    let stepped = std::time::Instant::now();
    looks.world.step(time.delta_secs());
    let step_ms = stepped.elapsed().as_secs_f64() * 1000.0;
    cost.add(step_ms, pieces.iter().len(), looks.world.counts().1);
    if facts.wanted() {
        facts.0.insert("destruction".into(), cost.facts());
    }
    looks.jolts.retain(|_, jolt| {
        let degrees = shake(jolt.size, jolt.class, (now_ms - jolt.since_ms) as f32);
        let turned = Quat::from_axis_angle(jolt.axis, degrees.to_radians()) * jolt.rest.rotation;
        commands.entity(jolt.root).try_insert(Transform {
            rotation: turned,
            ..jolt.rest
        });
        degrees > 0.0
    });
    let mut live: Vec<(Entity, (u64, Lasts))> = Vec::new();
    for (entity, mut piece, mut transform) in &mut pieces {
        if let Some(id) = piece.debris {
            if let Some((at, turn)) = looks.world.debris_pose(id) {
                let game = Transform::from_translation(Vec3::from_array(at))
                    .with_rotation(Quat::from_array(turn));
                *transform = Transform::from_rotation(game_to_bevy()) * game;
            }
            if looks.world.resting_debris(id) {
                looks.world.remove_debris(id);
                piece.debris = None;
            }
        }
        if let Some(fade) = piece.fade_ms
            && now_ms >= fade
        {
            let left = 1.0 - (now_ms - fade) / (SHRINK_S * 1000.0);
            if left <= 0.0 {
                if let Some(id) = piece.debris.take() {
                    looks.world.remove_debris(id);
                }
                commands.entity(entity).try_despawn();
                continue;
            }
            transform.scale = Vec3::splat(left as f32);
        }
        if piece.fade_ms.is_none() || piece.fade_ms.is_some_and(|fade| fade > now_ms) {
            live.push((entity, (piece.born_ms as u64, piece.lasts)));
        }
    }
    let ages: Vec<(u64, Lasts)> = live.iter().map(|(_, age)| *age).collect();
    for evicted in evict(&ages, detail(display.as_deref()).most_pieces) {
        if let Ok((_, mut piece, _)) = pieces.get_mut(live[evicted].0) {
            piece.fade_ms = Some(now_ms);
        }
    }
}

/// How far a small or medium prop jolts away from a blow (degrees), `since_ms`
/// after it: 6 or 3 at once, nothing by 150 ms; large and damage-class props
/// never move.
pub(crate) fn shake(size: Size, class: Class, since_ms: f32) -> f32 {
    let most = match (class, size) {
        (Class::Damage, _) | (_, Size::Large) => return 0.0,
        (_, Size::Small) => SHAKE_DEG[0],
        (_, Size::Medium) => SHAKE_DEG[1],
    };
    most * (1.0 - since_ms / SHAKE_MS).clamp(0.0, 1.0)
}

/// Lays `mark` among the `marks` of a prop of `class`, the newest last:
/// past `kept_marks` the oldest goes (returned, to be taken away).
pub(crate) fn keep_newest<T>(marks: &mut VecDeque<T>, mark: T, class: Class) -> Option<T> {
    marks.push_back(mark);
    (marks.len() > kept_marks(class))
        .then(|| marks.pop_front())
        .flatten()
}

/// How many marks a prop of `class` keeps.
pub(crate) fn kept_marks(class: Class) -> usize {
    match class {
        Class::Break => MARKS[0],
        Class::Damage => MARKS[1],
    }
}

/// A mark's size (metres across) turned `turn` (0 to 1), and its soot's.
pub(crate) fn mark_sizes(turn: f32) -> (f32, f32) {
    let size = MARK_M.0 + (MARK_M.1 - MARK_M.0) * turn;
    (size, size * SOOT_SCALE)
}

/// The dents a damage-class prop of `size` has at `stage`, seeded by
/// `seed` (its model's and its own): `DENTS` more at each stage, at seeded
/// points on the surface of `meshes` (model frame): which mesh, where, how
/// wide and how deep (m). A later stage keeps the earlier ones.
pub(crate) fn dents(
    seed: u64,
    stage: u8,
    size: Size,
    meshes: &[Plain],
) -> Vec<(usize, [f32; 3], f32, f32)> {
    let triangles: Vec<(usize, [u32; 3])> = (meshes.iter().enumerate())
        .flat_map(|(m, plain)| {
            plain
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .map(move |t| (m, *t))
        })
        .collect();
    if triangles.is_empty() {
        return Vec::new();
    }
    let size = size as usize;
    let mut found = Vec::new();
    for stage in 1..=stage {
        for dent in 0..DENTS[size] {
            let roll = |k: u8| noise(&[f32::from(stage), dent as f32, f32::from(k)], seed);
            let pick = ((roll(0) * triangles.len() as f32) as usize).min(triangles.len() - 1);
            let (m, t) = triangles[pick];
            let corner = |i: usize| Vec3::from_array(meshes[m].positions[t[i] as usize]);
            let (mut u, mut v) = (roll(1), roll(2));
            if u + v > 1.0 {
                (u, v) = (1.0 - u, 1.0 - v);
            }
            let at = corner(0) + (corner(1) - corner(0)) * u + (corner(2) - corner(0)) * v;
            found.push((m, at.to_array(), DENT_RADIUS_M[size], DENT_DEPTH_M[size]));
        }
    }
    found
}

/// Whether a mesh is a screen: it glows, or its texture scrolls or flips.
pub(crate) fn screen(mesh: &MeshIr, facts: Option<&TextureFacts>) -> bool {
    mesh.self_illum.iter().any(|&c| c > SCREEN_GLOW)
        || mesh.uv_scroll.is_some()
        || facts.is_some_and(|facts| facts.cycle.is_some())
}

/// Whether a mesh may be dented: not skinned, dangly or keyed by a clip
/// (`keyed`), whose motion a rebuilt mesh would freeze or tear.
pub(crate) fn dentable(mesh: &MeshIr, keyed: bool) -> bool {
    !mesh.skinned && mesh.skin.is_none() && mesh.dangly.is_none() && !keyed
}

/// How dark a prop's material is at a damage stage.
pub(crate) fn darkened(stage: u8) -> f32 {
    DARKENED[usize::from(stage).min(DARKENED.len() - 1)]
}

#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use sim::look;
    use sim::probe::check;
    let piece = |z: f32| Piece {
        middle: [0.0, 0.0, z],
        ..Piece::default()
    };
    let set = [0.1, 0.2, 0.3, 0.6, 0.9].map(piece);
    let chosen = base(&set, 1.0, 0.0);
    let from = [0.0, 0.0, 0.5];
    let single = fling([1.0, 0.0, 0.5], from, false);
    let blast = fling([1.0, 0.0, 0.5], from, true);
    let speed = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let live = [
        (100, Lasts::Fades),
        (200, Lasts::Visit),
        (300, Lasts::Fades),
        (400, Lasts::Visit),
        (500, Lasts::Fades),
    ];
    let evicted = evict(&live, 3);
    let bases = [
        (100, Lasts::Base),
        (200, Lasts::Base),
        (300, Lasts::Visit),
        (400, Lasts::Fades),
    ];
    let past_bases = evict(&bases, 1);
    let bursts = [
        dies(0, 2, true),
        dies(0, 2, false),
        dies(1, 3, false),
        dies(2, 3, false),
    ];
    let owed = |size| Owed {
        flies: true,
        from: [0.0; 3],
        blast: false,
        size,
    };
    let counts = [Size::Small, Size::Medium, Size::Large].map(|size| owed(size).pieces());
    // A panel standing on x = 0 (y -0.5 to 0.5, z 0 to 1), a medium prop.
    let panel = [
        [[0.0, -0.5, 0.0], [0.0, 0.5, 0.0], [0.0, 0.5, 1.0]],
        [[0.0, -0.5, 0.0], [0.0, 0.5, 1.0], [0.0, -0.5, 1.0]],
    ];
    let blade = |y: f32| Some([Vec3::new(-1.0, y, 0.5), Vec3::new(0.8, y, 0.5)]);
    let medium = Boxed {
        game: Transform::IDENTITY,
        min: Vec3::new(-0.5, -0.5, 0.0),
        max: Vec3::new(0.5, 0.5, 1.0),
    };
    // A blade swung through it between two frames, beside it in both.
    // Each blow on that medium prop at the origin, from 1.5 m before it.
    let strike = |triangles: &[[[f32; 3]; 3]], blades: &[[Vec3; 2]]| {
        impact(triangles, (&[], blades), &medium, Vec3::new(-1.5, 0.0, 0.5))
    };
    let through = strike(&panel, &swept(&[vec![blade(-0.9)], vec![blade(0.9)]]));
    // A blade inside the prop's box that meets none of its triangles, and
    // the same blow with no blade about.
    let inside = [[Vec3::new(0.1, 0.1, 0.3), Vec3::new(0.3, 0.2, 0.9)]];
    let (boxed, fist) = (strike(&[], &inside), strike(&[], &[]));
    // A console's box (1 m by 0.4 m) turned 90°: its back face is x = -0.2
    // in game space. A bolt from 2 m off ends on it, missing its triangles.
    let console = Boxed {
        game: Transform::from_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
        min: Vec3::new(-0.5, -0.2, 0.0),
        max: Vec3::new(0.5, 0.2, 1.6),
    };
    let shot = [Vec3::new(-2.0, 0.1, 0.8), Vec3::new(-0.19, 0.1, 0.8)];
    let bolted = impact(&[], (&[shot], &[]), &console, shot[0]);
    let first = wreck(None, (look::DEAD, 20));
    let now = wreck(Some((look::DEFAULT, 3)), (look::DEAD, 20));
    let standing = wreck(None, (look::DEFAULT, 0));
    let shakes = [
        shake(Size::Small, Class::Break, 0.0),
        shake(Size::Small, Class::Break, 150.0),
        shake(Size::Medium, Class::Break, 0.0),
        shake(Size::Large, Class::Break, 0.0),
        shake(Size::Small, Class::Damage, 0.0),
    ];
    let mut marks = VecDeque::new();
    let dropped: Vec<u32> = (1..=9)
        .filter_map(|mark| keep_newest(&mut marks, mark, Class::Break))
        .collect();
    let mesh = |self_illum: [f32; 3]| MeshIr {
        node: 0,
        positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 3],
        uv1: vec![[0.0; 2]; 3],
        uv2: Vec::new(),
        indices: vec![0, 1, 2],
        texture: Some("screen".into()),
        lightmap: None,
        transparency_hint: 0,
        skinned: false,
        self_illum,
        alpha: 1.0,
        ambient: [1.0; 3],
        diffuse: [1.0; 3],
        uv_scroll: None,
        skin: None,
        dangly: None,
    };
    let lit = screen(&mesh([0.5; 3]), None);
    let dark = screen(&mesh([0.0; 3]), None);
    let scrolls = screen(
        &MeshIr {
            uv_scroll: Some([1.0, 0.0]),
            ..mesh([0.0; 3])
        },
        None,
    );
    let flips = screen(
        &mesh([0.0; 3]),
        Some(&TextureFacts {
            cycle: Some((4, 8.0)),
            ..TextureFacts::default()
        }),
    );
    let skinned = MeshIr {
        skin: Some(asset_odyssey::model::SkinIr {
            joints: Vec::new(),
            inverse_binds: Vec::new(),
            rest_inverse_binds: Vec::new(),
            vertex_joints: Vec::new(),
            vertex_weights: Vec::new(),
        }),
        ..mesh([0.0; 3])
    };
    let dangly = MeshIr {
        dangly: Some(asset_odyssey::model::DanglyIr {
            displacement: 0.0,
            tightness: 0.0,
            period: 0.0,
            constraints: Vec::new(),
            rest: Vec::new(),
        }),
        ..mesh([0.0; 3])
    };
    let dentables = [
        dentable(&mesh([0.0; 3]), false),
        dentable(&skinned, false),
        dentable(&dangly, false),
        dentable(&mesh([0.0; 3]), true),
    ];
    let surface = [Plain {
        positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
        normals: vec![[0.0, 0.0, 1.0]; 4],
        uvs: vec![[0.0; 2]; 4],
        indices: vec![0, 1, 2, 2, 1, 3],
        capped: true,
    }];
    let (one, two) = (
        dents(7, 1, Size::Medium, &surface),
        dents(7, 2, Size::Medium, &surface),
    );
    let large = dents(7, 1, Size::Large, &surface);
    let again = dents(7, 2, Size::Medium, &surface);
    let darks = [0, 1, 2, 3].map(darkened);
    vec![
        (
            "prop_pieces_evict_keeps_bases",
            check(
                past_bases == [3, 2],
                format!(
                    "two bases, one kept, one fading, cap 1: fades {past_bases:?} \
                     (want [3, 2]: never a base)"
                ),
            ),
        ),
        (
            "prop_stage_no_burst_on_arrival",
            check(
                bursts == [false, true, true, false],
                format!(
                    "wrecked when first seen, wrecked now, wrecked from damaged, wrecked \
                     further: {bursts:?} (want [false, true, true, false])"
                ),
            ),
        ),
        (
            "prop_pieces_count_by_size",
            check(
                counts == PIECES,
                format!("owed small, medium, large cut into {counts:?} (want {PIECES:?})"),
            ),
        ),
        (
            "prop_hit_blade_swept",
            check(
                through.blow == Blow::Blade && through.at.x.abs() < 1e-4,
                format!(
                    "a blade swept through a panel between frames: {:?} at {:.2?} \
                     (want Blade on the panel, x 0)",
                    through.blow, through.at
                ),
            ),
        ),
        (
            "prop_hit_blade_in_box",
            check(
                boxed.blow == Blow::Blade && fist.blow == Blow::Blunt,
                format!(
                    "a blade inside the box, no triangle met: {:?} (want Blade); no blade \
                     about: {:?} (want Blunt)",
                    boxed.blow, fist.blow
                ),
            ),
        ),
        (
            "prop_hit_bolt_face",
            check(
                bolted.blow == Blow::Bolt
                    && bolted.normal.distance(Vec3::NEG_X) < 1e-3
                    && bolted.at.distance(Vec3::new(-0.2, 0.1, 0.8)) < 1e-3,
                format!(
                    "a bolt ending on a turned console's box: {:?} at {:.2?} facing {:.2?} \
                     (want Bolt at [-0.20, 0.10, 0.80] facing -x)",
                    bolted.blow, bolted.at, bolted.normal
                ),
            ),
        ),
        (
            "prop_stage_dents_same",
            check(
                one.len() == 2
                    && two.len() == 4
                    && two.starts_with(&one)
                    && two == again
                    && large.len() == 4,
                format!(
                    "medium stage 1 {} dents (want 2), stage 2 {} holding stage 1's {} (want 4, true), \
                     the same again {}, large stage 1 {} (want 4)",
                    one.len(),
                    two.len(),
                    two.starts_with(&one),
                    two == again,
                    large.len()
                ),
            ),
        ),
        (
            "prop_stage_screen",
            check(
                lit && !dark && scrolls && flips,
                format!(
                    "glowing {lit}, dark {dark}, scrolling {scrolls}, flipping {flips} (want true, false, true, true)"
                ),
            ),
        ),
        (
            "prop_stage_skips_moving",
            check(
                dentables == [true, false, false, false],
                format!("still, skinned, dangly, keyed: {dentables:?} (want only the still one)"),
            ),
        ),
        (
            "prop_stage_darkened_subtle",
            check(
                darks == [1.0, 0.9, 0.8, 0.8],
                format!("stages 0-3 {darks:?} (want [1, 0.9, 0.8, 0.8])"),
            ),
        ),
        (
            "prop_hit_shake",
            check(
                shakes == [6.0, 0.0, 3.0, 0.0, 0.0],
                format!(
                    "small at 0 and 150 ms, medium, large, damage class: {shakes:?} \
                     (want [6, 0, 3, 0, 0])"
                ),
            ),
        ),
        (
            "prop_hit_marks_capped",
            check(
                dropped == [1] && marks.len() == 8 && marks.front() == Some(&2),
                format!(
                    "nine marks dropped {dropped:?} (want [1]), {} kept from {:?}",
                    marks.len(),
                    marks.front()
                ),
            ),
        ),
        (
            "prop_pieces_base",
            check(
                chosen == [0, 1],
                format!("base of a 1 m set {chosen:?} (want [0, 1]: middles under 0.25)"),
            ),
        ),
        (
            "prop_pieces_fling",
            check(
                single[0] > 0.0
                    && single[2] > 0.0
                    && (2.0..=4.0).contains(&speed(single))
                    && (5.0..=8.0).contains(&speed(blast))
                    && kept(0.002)
                    && !kept(0.0005),
                format!(
                    "a blow {single:?} ({:.1} m/s, want 2–4 away and up), a blast {:.1} m/s \
                     (5–8); a 2 l piece kept {}, a half-litre one {}",
                    speed(single),
                    speed(blast),
                    kept(0.002),
                    kept(0.0005)
                ),
            ),
        ),
        (
            "prop_pieces_evict",
            check(
                evicted == [0, 2],
                format!("cap 3 of 5 (two kept) fades {evicted:?} (want [0, 2], the oldest unkept)"),
            ),
        ),
        (
            "prop_pieces_already_broken",
            check(
                first == Wreck::BaseOnly && now == Wreck::Flies && standing == Wreck::Standing,
                format!(
                    "first seen broken {first:?} (want BaseOnly), broken now {now:?} (Flies), \
                     first seen standing {standing:?} (Standing)"
                ),
            ),
        ),
    ]
    .into_iter()
    .chain(fault_probes())
    .chain(detail_probes())
    .collect()
}

/// A piece standing from `low` to `high` (model frame, metres), its middle
/// between.
#[cfg(feature = "probes")]
fn upright(low: f32, high: f32) -> Piece {
    Piece {
        middle: [0.0, 0.0, (low + high) / 2.0],
        hull: vec![[0.0, 0.0, low], [0.0, 0.0, high]],
        ..Piece::default()
    }
}

/// The playtest log's faults: legged furniture's legs stand, a floating set
/// keeps its lowest piece, and a plant is cut.
/// The destruction detail: the setting reads back, each level draws no
/// more than the one above it, and off throws, bursts and burns nothing.
#[cfg(feature = "probes")]
fn detail_probes() -> sim::probe::Checks {
    use crate::display::Debris;
    use sim::probe::check;
    let mut settings = crate::settings::Settings::defaults();
    let mut read = vec![settings.display().debris];
    for value in ["low", "off", "full"] {
        settings.apply("display.debris", value);
        read.push(settings.display().debris);
    }
    let at = |debris: Debris| {
        detail(Some(&crate::display::Display {
            debris,
            ..crate::display::Display::default()
        }))
    };
    let levels = [Debris::Full, Debris::Low, Debris::Off].map(at);
    let no_more = levels.windows(2).all(|pair| {
        let (above, below) = (pair[0], pair[1]);
        (below.flies <= above.flies)
            && (below.small_pieces <= above.small_pieces)
            && (below.shards <= above.shards)
            && (below.glass_burst <= above.glass_burst)
            && (below.lights <= above.lights)
            && below.emitters <= above.emitters
            && below.most_glass <= above.most_glass
            && (below.glass_every == 0 || below.glass_every >= above.glass_every)
    });
    let off = levels[2];
    let quiet = !off.flies
        && !off.shards
        && !off.glass_burst
        && off.glass_every == 0
        && off.emitters == 0
        && !off.lights;
    vec![
        (
            "prop_detail_setting",
            check(
                read == [Debris::Full, Debris::Low, Debris::Off, Debris::Full],
                format!("default, low, off, full read {read:?}"),
            ),
        ),
        (
            "prop_detail_levels",
            check(
                no_more && quiet && detail(None) == levels[0],
                format!(
                    "each level no more than the one above: {no_more}; off quiet: {quiet}; none is full: {}",
                    detail(None) == levels[0]
                ),
            ),
        ),
    ]
}

#[cfg(feature = "probes")]
fn fault_probes() -> sim::probe::Checks {
    use sim::probe::check;
    // A table: four legs from the floor to 0.7 m, a top from 0.7 to 0.8.
    let table = [
        upright(0.0, 0.7),
        upright(0.0, 0.7),
        upright(0.0, 0.7),
        upright(0.0, 0.7),
        upright(0.7, 0.8),
    ];
    let legs = base(&table, 0.8, 0.0);
    let mut damage_marks = VecDeque::new();
    let damage_dropped = (0..30)
        .filter_map(|mark| keep_newest(&mut damage_marks, mark, Class::Damage))
        .count();
    let kept = (kept_marks(Class::Damage), kept_marks(Class::Break));
    let sizes = [0.0, 0.5, 1.0].map(mark_sizes);
    // A lamp hung above the floor: nothing touches it, nothing low.
    let hung = [upright(1.0, 1.4), upright(0.6, 1.0), upright(1.4, 2.0)];
    let floating = base(&hung, 2.0, 0.0);
    let plant = crate::k1_resolver().map(|resolver| {
        let parsed = crate::viewer::parsed(&resolver, "plc_firixa01");
        let ir = parsed.ok().flatten().map(|m| ModelIr::from_model(&m));
        ir.map(|ir| {
            let (plains, ..) = cuttable(&ir, &BTreeMap::new());
            let why: Vec<String> = (ir.meshes.iter())
                .map(|mesh| {
                    format!(
                        "texture {} skin {} dangly {} lightmapped {} triangles {}",
                        mesh.texture.is_some(),
                        mesh.skin.is_some(),
                        mesh.dangly.is_some(),
                        mesh.lightmap.is_some() && mesh.uv2.len() == mesh.positions.len(),
                        mesh.indices.len() / 3
                    )
                })
                .collect();
            (plains.len(), why)
        })
    });
    vec![
        (
            "marks_kept_by_class",
            check(
                kept == (24, 8) && damage_marks.len() == 24 && damage_dropped == 6,
                format!(
                    "damage, break keep {kept:?} (want (24, 8)); 30 marks on a damage prop kept {}, \
                     dropped {damage_dropped} (want 24, 6)",
                    damage_marks.len()
                ),
            ),
        ),
        (
            "soot_under_mark",
            check(
                sizes
                    .iter()
                    .all(|(mark, soot)| (soot - 3.0 * mark).abs() < 1e-5),
                format!("marks and their soot (m across): {sizes:?} (want soot 3× each)"),
            ),
        ),
        (
            "prop_pieces_base_legs",
            check(
                legs == [0, 1, 2, 3],
                format!("a table's base {legs:?} (want its four legs [0, 1, 2, 3])"),
            ),
        ),
        (
            "prop_pieces_base_never_empty",
            check(
                floating == [1],
                format!("a hung set's base {floating:?} (want its lowest piece [1])"),
            ),
        ),
        (
            "prop_cut_plant",
            match plant {
                None => Ok("no K1 install: skipped".into()),
                Some(None) => Err("plc_firixa01 did not read".into()),
                Some(Some((plains, why))) => check(
                    plains > 0,
                    format!(
                        "plc_firixa01 cuts into {plains} surfaces (want some); meshes: {why:?}"
                    ),
                ),
            },
        ),
    ]
}
