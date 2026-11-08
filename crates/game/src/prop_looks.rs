//! What breaking and hurting a prop looks like, drawn from the prop's own
//! mesh and texture (`ragdoll::shatter`): a breakable prop flies apart into
//! pieces of itself, its base left standing. Looks-only: nothing here is
//! synced or saved; every machine cuts the same pieces from the model's
//! name and finds the rest itself.

use std::collections::{BTreeMap, BTreeSet};
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
use sim::destruction::Class;
use sim::look;

use crate::made::Made;
use crate::material::{self, OdysseyMaterial, Surface};
use crate::prop_breaks::{PropChanges, PropsSeen};
use crate::scene::{MeshPart, SceneContent, SceneModel, base_mesh, game_to_bevy};
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
}

/// The meshes of `ir` that can be cut, in the model's frame (each node's
/// rest transform applied), and which mesh each is: drawn, not skinned,
/// dangly or lightmapped (a piece has no lightmap uvs); capped unless its
/// surface is alpha-tested, blended or two-sided.
fn plains(ir: &ModelIr, textures: &BTreeMap<String, TextureIr>) -> (Vec<Plain>, Vec<usize>) {
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
        let (meshes, numbers) = plains(ir, textures);
        let seed = shatter::seed_of(&name);
        let task = AsyncComputeTaskPool::get().spawn(async move {
            let pieces = shatter::shatter(&meshes, count, seed);
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
}

pub struct PropLooksPlugin;

impl Plugin for PropLooksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Fractures>()
            .init_resource::<PropLooks>()
            .add_systems(
                PostUpdate,
                (wrecks, fly)
                    .chain()
                    .after(PropsSeen)
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
            let from = blow_from(change.position, &changes.creatures);
            looks.owed.insert(
                change.index,
                Owed {
                    flies,
                    from,
                    blast: false,
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
        // The model's place in game space (the scene's root turns game
        // space to Bevy's).
        let game =
            Transform::from_matrix(Mat4::from_quat(game_to_bevy().inverse()) * global.to_matrix());
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
            let entity = commands
                .spawn((
                    Transform::from_rotation(game_to_bevy()) * game,
                    Visibility::default(),
                    Flying {
                        debris,
                        born_ms: now_ms,
                        kept: !small || stays,
                        fade_ms,
                    },
                    Name::new(format!("{} piece {number}", ir.name)),
                ))
                .id();
            for (source, plain) in &piece.meshes {
                let Some(material) = fracture.meshes.get(*source).and_then(|m| materials.get(m))
                else {
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
    vec![
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
