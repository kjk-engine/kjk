//! What breaking and hurting a prop looks like, drawn from the prop's own
//! mesh and texture (`ragdoll::shatter`): a breakable prop flies apart into
//! pieces of itself, its base left standing. Looks-only: nothing here is
//! synced or saved; every machine cuts the same pieces from the model's
//! name and finds the rest itself.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use asset_odyssey::model::ModelIr;
use asset_odyssey::texture::TextureIr;
use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::Indices;
use bevy::prelude::*;
use bevy::tasks::{AsyncComputeTaskPool, Task, block_on, poll_once};
use ragdoll::DebrisId;
use ragdoll::shatter::{self, Piece, Plain};
use sim::destruction::{Class, Material, Size};
use sim::look;

use crate::body_marks;
use crate::glow_shell::GlowMaterial;
use crate::made::Made;
use crate::material::{self, OdysseyMaterial, Surface};
use crate::prop_breaks::{PropChanges, PropsSeen};
use crate::scene::{DrawnBlades, MeshPart, SceneContent, SceneModel, base_mesh, game_to_bevy};
use crate::script_fx::{HookVisual, HookVisuals, Place};
use crate::shot_fx::BoltEnds;
use crate::sim_host::{PresentedSnapshot, SceneSwap};

/// How many pieces a prop is cut into, by size (small, medium, large).
const PIECES: [usize; 3] = [6, 10, 16];
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
const MARKS: usize = 8;
const MARK_M: (f32, f32) = (0.08, 0.14);
/// A blow throws this many shards (seeded between), this fast (m/s), gone
/// within 2 s (ms until they shrink away).
const SHARDS: (usize, usize) = (3, 6);
const SHARD_SPEED: (f32, f32) = (2.0, 3.5);
const SHARD_FADE_MS: f64 = 1500.0;
/// A blade whose hilt is this near a prop's feet (m) may have struck it; a
/// blow from this near is a fist's or a kick's.
const BLADE_REACH_M: f32 = 3.0;
const BLUNT_REACH_M: f32 = 3.0;

/// A number from 0 to 1 (1 never) from `parts` and `salt`, the same on
/// every machine: FNV-1a 64 of their bits, then splitmix64's finish.
fn noise(parts: &[f32], salt: u64) -> f32 {
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
/// (model frame, its feet at 0): the base that stays where it stood.
pub(crate) fn base(pieces: &[Piece], height: f32) -> Vec<usize> {
    (0..pieces.len())
        .filter(|&i| pieces[i].middle[2] < height * BASE_SHARE)
        .collect()
}

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

/// Of the live pieces (`(born ms, kept)`), the ones to fade to bring them
/// down to `cap`: the oldest unkept first, then the oldest kept.
pub(crate) fn evict(live: &[(u64, bool)], cap: usize) -> Vec<usize> {
    let mut order: Vec<usize> = (0..live.len()).collect();
    order.sort_by_key(|&i| (live[i].1, live[i].0));
    order.truncate(live.len().saturating_sub(cap));
    order
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

/// A model cut into pieces: the pieces, which of the model's meshes each
/// of their parts is (by its index among those cut), and how tall the
/// model stands (m).
pub(crate) struct Fracture {
    pieces: Vec<Piece>,
    meshes: Vec<usize>,
    height: f32,
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

/// A model's meshes that can be cut (model frame), and which mesh each is.
type Cuttable = (Vec<Plain>, Vec<usize>);

/// The meshes of `ir` that can be cut, in the model's frame (each node's
/// rest transform applied), and which mesh each is: drawn, not skinned,
/// dangly or lightmapped (a piece has no lightmap uvs); capped unless its
/// surface is alpha-tested, blended or two-sided.
fn cuttable(ir: &ModelIr, textures: &BTreeMap<String, TextureIr>) -> (Vec<Plain>, Vec<usize>) {
    ir.meshes
        .iter()
        .enumerate()
        .filter(|(_, mesh)| {
            mesh.texture.is_some()
                && mesh.skin.is_none()
                && mesh.dangly.is_none()
                && !(mesh.lightmap.is_some() && mesh.uv2.len() == mesh.positions.len())
                && !mesh.indices.is_empty()
        })
        .map(|(number, mesh)| {
            let world = ir
                .nodes
                .get(mesh.node)
                .map_or(asset_odyssey::model::IDENTITY, |n| n.world);
            let place = |p: [f32; 3], w: f32| -> [f32; 3] {
                std::array::from_fn(|r| {
                    world[r][0] * p[0] + world[r][1] * p[1] + world[r][2] * p[2] + world[r][3] * w
                })
            };
            let turn = |n: [f32; 3]| {
                let n = place(n, 0.0);
                let length = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
                n.map(|c| c / length)
            };
            let facts: Vec<_> = mesh
                .texture
                .iter()
                .filter_map(|name| textures.get(name).map(|texture| &texture.facts))
                .collect();
            let surface = material::surface((mesh.transparency_hint, false), mesh.alpha, &facts);
            let open = facts.iter().any(|facts| facts.two_sided || facts.decal);
            let plain = Plain {
                positions: mesh.positions.iter().map(|&p| place(p, 1.0)).collect(),
                normals: mesh.normals.iter().map(|&n| turn(n)).collect(),
                uvs: mesh.uv1.clone(),
                indices: mesh.indices.clone(),
                capped: matches!(surface, Surface::Opaque | Surface::OpaqueEnvMasked) && !open,
            };
            (plain, number)
        })
        .unzip()
}

impl Fractures {
    /// Starts cutting model `ir` into `count` pieces, off the main thread,
    /// unless it is cut or being cut.
    fn cut(&mut self, ir: &ModelIr, textures: &BTreeMap<String, TextureIr>, count: usize) {
        let name = ir.name.to_ascii_lowercase();
        if self.ready.contains_key(&name) || self.cutting.contains_key(&name) {
            return;
        }
        let cut = self.plains(ir, textures);
        let numbers = cut.1.clone();
        let seed = shatter::seed_of(&name);
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let pieces = shatter::shatter(&cut.0, count, seed);
            let height = pieces
                .iter()
                .flat_map(|piece| &piece.hull)
                .map(|p| p[2])
                .fold(0.0f32, f32::max);
            Fracture {
                pieces,
                meshes: numbers,
                height,
            }
        });
        self.cutting.insert(name, task);
    }

    /// Takes in the cuts that are done.
    fn collect(&mut self) {
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
    fn plains(&mut self, ir: &ModelIr, textures: &BTreeMap<String, TextureIr>) -> Arc<Cuttable> {
        (self.plains.entry(ir.name.to_ascii_lowercase()))
            .or_insert_with(|| Arc::new(cuttable(ir, textures)))
            .clone()
    }

    fn get(&self, ir: &ModelIr) -> Option<&Arc<Fracture>> {
        self.ready.get(&ir.name.to_ascii_lowercase())
    }
}

/// A piece of a broken prop as drawn: its body while it flies (`None`: a
/// base piece, or one come to rest), when it came, whether it stays, when
/// it starts to fade (ms of `Time`).
#[derive(Component)]
pub(crate) struct Flying {
    debris: Option<DebrisId>,
    born_ms: f64,
    kept: bool,
    fade_ms: Option<f64>,
}

/// A prop broken whose pieces are not out yet (its model or cut not ready),
/// and whether they fly.
#[derive(Debug, Clone, Copy)]
struct Owed {
    flies: bool,
    from: [f32; 3],
    blast: bool,
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
    marks: BTreeMap<u32, VecDeque<Vec<Entity>>>,
    /// Props jolting from a blow.
    jolts: BTreeMap<u32, Jolt>,
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

pub struct PropLooksPlugin;

impl Plugin for PropLooksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fractures>()
            .init_resource::<PropLooks>()
            .add_systems(
                PostUpdate,
                (hits, wrecks, fly)
                    .chain()
                    .after(PropsSeen)
                    .after(crate::shot_fx::ShotsSeen)
                    .run_if(resource_exists::<PresentedSnapshot>),
            );
    }
}

/// The model of prop `index` in the scene and its model root, once drawn.
fn model_of(
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
fn blow_from(position: [f32; 3], creatures: &[Vec3]) -> [f32; 3] {
    let at = Vec3::from_array(position);
    (creatures.iter())
        .filter(|c| c.distance(at) > 0.1 && c.distance(at) <= 10.0)
        .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)))
        .map_or([position[0], position[1], position[2] - 1.0], |c| {
            c.to_array()
        })
}

/// Each drawn mesh's place in its model, and its material.
type MeshParts<'w, 's> =
    Query<'w, 's, (&'static MeshPart, &'static MeshMaterial3d<OdysseyMaterial>)>;

/// Breakable props: their models cut once first seen; broken ones hidden
/// and their pieces put in their place (or owed until the cut is ready).
#[allow(clippy::too_many_arguments)]
fn wrecks(
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
    flying: Query<Entity, With<Flying>>,
    mut meshes: ResMut<Assets<Mesh>>,
    time: Res<Time>,
) {
    if looks.generation != swap.generation {
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
            Wreck::Flies => true,
        };
        if !looks.done.contains(&change.index) {
            let (from, blast) = match looks.impacts.get(&change.index) {
                Some(impact) => (
                    (impact.at + impact.normal).to_array(),
                    impact.blow == Blow::Blast,
                ),
                None => (blow_from(change.position, &changes.creatures), false),
            };
            looks.owed.insert(change.index, Owed { flies, from, blast });
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
                fractures.cut(ir, &scene.0.textures, PIECES[1]);
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
        let standing = base(&fracture.pieces, fracture.height);
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
        let LooksWorld { world, floor } = looks.split();
        if owe.flies {
            crate::ragdolls::gather(
                world,
                floor,
                (&swap.level, newest),
                game.translation,
                game.translation.z,
            );
        }
        for (number, piece) in fracture.pieces.iter().enumerate() {
            let stays = standing.contains(&number);
            if !owe.flies && !stays {
                continue;
            }
            let middle = game
                .transform_point(Vec3::from_array(piece.middle))
                .to_array();
            let debris = (owe.flies && !stays)
                .then(|| {
                    world.debris(
                        &piece.hull,
                        game.translation.to_array(),
                        game.rotation.to_array(),
                        fling(middle, owe.from, owe.blast),
                        spin(middle),
                    )
                })
                .flatten();
            let small = !kept(piece.volume);
            let fade_ms = (small && !stays).then(|| {
                let (lo, hi) = FADE_S;
                now_ms + (lo + (hi - lo) * f64::from(noise(&middle, 9))) * 1000.0
            });
            spawn_piece(
                &mut commands,
                &mut meshes,
                (piece, &fracture.meshes, &materials),
                (game, format!("{} piece {number}", ir.name)),
                Flying {
                    debris,
                    born_ms: now_ms,
                    kept: !small || stays,
                    fade_ms,
                },
            );
        }
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
fn in_game(global: &GlobalTransform) -> Transform {
    Transform::from_matrix(Mat4::from_quat(game_to_bevy().inverse()) * global.to_matrix())
}

/// The triangles of `plains` (model frame) placed by `game`.
fn placed_triangles(plains: &[Plain], game: &Transform) -> Vec<[[f32; 3]; 3]> {
    plains
        .iter()
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

/// Where a blow on a prop landed: a bolt's run or a blade crossing its own
/// triangles (`triangles`, game space) first; else the point on its body
/// box (standing at `feet`, of `size`) facing `from`, where the blow came
/// from, a fist or kick when that is within reach, a blast or power
/// otherwise.
pub(crate) fn impact(
    triangles: &[[[f32; 3]; 3]],
    (bolts, blades): (&[[Vec3; 2]], &[[Vec3; 2]]),
    (feet, size): (Vec3, Size),
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
    let near: Vec<[Vec3; 2]> = (blades.iter())
        .filter(|[hilt, _]| hilt.distance(feet) <= BLADE_REACH_M)
        .copied()
        .collect();
    if let Some(found) = crossing(bolts, Blow::Bolt).or_else(|| crossing(&near, Blow::Blade)) {
        return found;
    }
    let half = rules::destruction::bodies::HALF_M[size as usize];
    let height = rules::destruction::bodies::HEIGHT_M[size as usize];
    let middle = feet + Vec3::Z * (height / 2.0);
    let halves = Vec3::new(half, half, height / 2.0);
    let way = (from - middle).try_normalize().unwrap_or(Vec3::Z);
    // The face the way leaves the box by: its axis has the least room.
    let (axis, reach) = (0..3)
        .filter(|&i| way[i].abs() > 1e-4)
        .map(|i| (i, halves[i] / way[i].abs()))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap_or((2, halves.z));
    let mut normal = Vec3::ZERO;
    normal[axis] = way[axis].signum();
    let blow = if from.distance(feet) <= BLUNT_REACH_M {
        Blow::Blunt
    } else {
        Blow::Blast
    };
    Impact {
        at: middle + way * reach,
        normal,
        blow,
    }
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
    visuals: Option<ResMut<HookVisuals>>,
    time: Res<Time>,
) {
    if looks.generation != swap.generation {
        return;
    }
    looks.impacts.clear();
    let made = made.as_deref();
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    let newest = presented.1.last().map(|snapshot| &**snapshot);
    let bolts = bolts.as_deref().map_or(&[][..], |bolts| &bolts.0);
    let blades: Vec<[Vec3; 2]> = blades
        .as_deref()
        .map_or_else(Vec::new, |b| b.0.iter().flatten().copied().collect());
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
            &placed_triangles(&cut.0, &game),
            (bolts, &blades),
            (feet, change.d.size),
            from,
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
        if throws_shards(change.d.material) {
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
                let debris = world.debris(
                    &shard.hull,
                    game.translation.to_array(),
                    game.rotation.to_array(),
                    velocity.to_array(),
                    spin(middle.to_array()),
                );
                spawn_piece(
                    &mut commands,
                    &mut meshes,
                    (shard, &cut.1, &materials),
                    (game, format!("{} shard {number}", ir.name)),
                    Flying {
                        debris,
                        born_ms: now_ms,
                        kept: false,
                        fade_ms: Some(now_ms + SHARD_FADE_MS),
                    },
                );
            }
        }
        if let Some(mark_looks) = mark_looks.as_deref_mut() {
            let laid = lay_mark(
                &mut commands,
                (&mut meshes, mark_looks, &mut glows),
                (ir, &children, &parts, &globals),
                (root, found),
                noise(&found.at.to_array(), u64::from(change.now.1)),
            );
            if !laid.is_empty() {
                let marks = looks.marks.entry(change.index).or_default();
                for gone in keep_newest(marks, laid).into_iter().flatten() {
                    commands.entity(gone).try_despawn();
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
    let size = MARK_M.0 + (MARK_M.1 - MARK_M.0) * turn;
    let axes = body_marks::mark_axes(ray, turn * std::f32::consts::TAU, (size, size));
    let stages: Vec<_> = (looks.stages(glows).into_iter())
        .filter(|(_, glows)| !glows || found.blow == Blow::Blade)
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
        let Some(covered) = body_marks::gore(&source, &at, (found.at, ray), axes) else {
            continue;
        };
        let covered = meshes.add(covered);
        for (stage, glowing) in &stages {
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

/// A piece (or shard) of a model, placed by `game`, in its source meshes'
/// own materials (`numbers`: which mesh each cut one is), as `flying`.
fn spawn_piece(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    (piece, numbers, materials): (&Piece, &[usize], &BTreeMap<usize, Handle<OdysseyMaterial>>),
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
    for (source, plain) in &piece.meshes {
        let Some(material) = numbers.get(*source).and_then(|m| materials.get(m)) else {
            continue;
        };
        let mut mesh = base_mesh(
            &plain.positions,
            &plain.normals,
            &plain.uvs,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_indices(Indices::U32(plain.indices.clone()));
        let child = commands
            .spawn((
                Mesh3d(meshes.add(mesh)),
                MeshMaterial3d(material.clone()),
                Transform::IDENTITY,
                NotShadowCaster,
            ))
            .id();
        commands.entity(entity).add_child(child);
    }
}

/// The world and its floor flag, borrowed apart from the rest.
struct LooksWorld<'a> {
    world: &'a mut ragdoll::World,
    floor: &'a mut bool,
}

impl PropLooks {
    fn split(&mut self) -> LooksWorld<'_> {
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
) {
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    looks.world.step(time.delta_secs());
    looks.jolts.retain(|_, jolt| {
        let degrees = shake(jolt.size, jolt.class, (now_ms - jolt.since_ms) as f32);
        let turned = Quat::from_axis_angle(jolt.axis, degrees.to_radians()) * jolt.rest.rotation;
        commands.entity(jolt.root).try_insert(Transform {
            rotation: turned,
            ..jolt.rest
        });
        degrees > 0.0
    });
    let mut live: Vec<(Entity, (u64, bool))> = Vec::new();
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
            live.push((entity, (piece.born_ms as u64, piece.kept)));
        }
    }
    let ages: Vec<(u64, bool)> = live.iter().map(|(_, age)| *age).collect();
    for evicted in evict(&ages, MAX_PIECES) {
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

/// Lays `mark` among a prop's `marks`, the newest last: past `MARKS` the
/// oldest goes (returned, to be taken away).
pub(crate) fn keep_newest<T>(marks: &mut VecDeque<T>, mark: T) -> Option<T> {
    marks.push_back(mark);
    (marks.len() > MARKS).then(|| marks.pop_front()).flatten()
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
    let chosen = base(&set, 1.0);
    let from = [0.0, 0.0, 0.5];
    let single = fling([1.0, 0.0, 0.5], from, false);
    let blast = fling([1.0, 0.0, 0.5], from, true);
    let speed = |v: [f32; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    let live = [
        (100, false),
        (200, true),
        (300, false),
        (400, true),
        (500, false),
    ];
    let evicted = evict(&live, 3);
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
        .filter_map(|mark| keep_newest(&mut marks, mark))
        .collect();
    vec![
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
}
