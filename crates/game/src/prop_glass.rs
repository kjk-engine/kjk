//! Glass: the see-through parts of a destructible prop (a kolto tank's
//! tank, a canister's tube, a landspeeder's windscreen) break as glass,
//! apart from the rest of it. At the first blow that hurts the prop (its
//! damage, synced, so every seat and every save agrees) its glass is gone:
//! cut once per model into small shards of itself (`shatter::glass`), they
//! burst out from the blow with JA's glass burst and sound, fall, skitter,
//! lie flat and fade. The rest of the prop breaks or wears as its material
//! does (`prop_looks`, `prop_damage`), which never cut its glass.
//!
//! How much of it is drawn follows the destruction detail
//! (`prop_looks::Detail`): every shard, a third, or none (the glass simply
//! goes); the crash is heard at every level.
//!
//! Shards fall on their own simple arcs to the floor the prop stands on,
//! not through the pieces' physics: there are many, they are thin and they
//! last seconds. Looks-only: nothing here is synced or saved.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use asset_odyssey::model::{MeshIr, ModelIr};
use asset_odyssey::texture::{TextureFacts, TextureIr};
use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::Indices;
use bevy::prelude::*;
use ragdoll::shatter::{self, Piece};
use sim::look;

use crate::combat_fx::{Burst, Bursts};
use crate::made::Made;
use crate::material::{OdysseyMaterial, Surface};
use crate::prop_breaks::PropChanges;
use crate::prop_looks::{Fractures, MeshParts, PropLooks, blow_from, in_game, model_of, noise};
use crate::scene::{SceneContent, SceneModel, base_mesh, game_to_bevy};
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::sound::{Cue, Cues};

/// JA's glass burst (`effects/chunks/glassbreak.efx`: glinting chips) and
/// its window's breaking (`sound/effects/glassbreak1.wav`).
const BURST: &str = "chunks/glassbreak";
const SOUND: &str = "sound/effects/glassbreak1.wav";

/// The JA effects this plays (`combat_fx::Bursts`).
pub const EFFECTS: [&str; 1] = [BURST];

/// The JA sounds this plays.
pub fn sounds() -> impl Iterator<Item = String> {
    [SOUND].into_iter().map(String::from)
}

/// How many shards a model's glass breaks into: this many a square metre,
/// at least and at most these.
const SHARDS_PER_M2: f32 = 16.0;
const SHARDS: (usize, usize) = (8, 96);
/// How fast shards fly from a blow and from a blast (m/s, seeded between),
/// how fast they rise at most, how fast they spin at most (rad/s).
const SPEED: (f32, f32) = (1.5, 3.5);
const BLAST_SPEED: (f32, f32) = (3.0, 6.0);
const RISE: f32 = 1.5;
const SPIN: f32 = 14.0;
/// How shards meet the floor: the share of their fall they bounce back
/// with, the share of their slide they keep, and how slow a fall comes to
/// rest (m/s).
const BOUNCE: f32 = 0.3;
const SLIDE: f32 = 0.5;
const STILL: f32 = 0.8;
/// How long a shard lasts (s, seeded between), shrinking away over the
/// last `SHRINK_S`; at most `MOST` at once, the oldest going first.
const LASTS_S: (f64, f64) = (2.5, 5.0);
const SHRINK_S: f64 = 0.4;
const MOST: usize = 256;
/// How fast shards fall (m/s²).
const GRAVITY: f32 = 9.81;
/// Where a burst plays: at this many of the shards, spread over the glass.
const BURSTS: usize = 4;

/// Whether `mesh` (of node `node`, drawn as `surface` from textures with
/// `facts`) is glass: it shows at rest and is see-through (blended, not a
/// decal: a screen or a painted light is no glass), or its node or texture
/// says glass (a windscreen, drawn opaque with a reflection).
pub(crate) fn is_glass(
    mesh: &MeshIr,
    node: &str,
    surface: Surface,
    facts: &[&TextureFacts],
) -> bool {
    glass_by(mesh.alpha, mesh.texture.as_deref(), node, surface, facts)
}

/// `is_glass` of a mesh of rest alpha `alpha` and texture `texture`.
fn glass_by(
    alpha: f32,
    texture: Option<&str>,
    node: &str,
    surface: Surface,
    facts: &[&TextureFacts],
) -> bool {
    let named = |name: &str| name.to_ascii_lowercase().contains("glass");
    let see_through = surface == Surface::Blend && !facts.iter().any(|facts| facts.decal);
    alpha > 0.0 && (see_through || named(node) || texture.is_some_and(named))
}

/// A model's glass broken: its shards (model frame), which of the model's
/// meshes each glass mesh is, where each shard's middle is and the way its
/// face looks (model frame), and the shards' meshes about their middles,
/// made the first time any copy breaks.
struct Cut {
    shards: Vec<Piece>,
    numbers: Vec<usize>,
    faces: Vec<Vec3>,
    drawn: OnceLock<Vec<crate::prop_looks::PieceMeshes>>,
}

impl Cut {
    /// Model `ir`'s glass (`glass`: each mesh's number and itself) broken,
    /// seeded by the model's name; none for a model with no glass.
    fn of(ir: &ModelIr, glass: &[(usize, shatter::Plain)]) -> Option<Cut> {
        if glass.is_empty() {
            return None;
        }
        let plains: Vec<shatter::Plain> = glass.iter().map(|(_, plain)| plain.clone()).collect();
        let area: f32 = (plains.iter())
            .flat_map(|plain| {
                plain.indices.as_chunks::<3>().0.iter().map(|t| {
                    let [a, b, c] = t.map(|v| Vec3::from_array(plain.positions[v as usize]));
                    (b - a).cross(c - a).length() / 2.0
                })
            })
            .sum();
        let count = ((area * SHARDS_PER_M2).round() as usize).clamp(SHARDS.0, SHARDS.1);
        let seed = shatter::seed_of(&ir.name) ^ 0x6c61_7373;
        let shards = shatter::glass(&plains, count, seed);
        let faces = (shards.iter())
            .map(|shard| {
                // The first triangle's face: a shard's back faces come last.
                (shard.meshes.first())
                    .and_then(|(_, plain)| {
                        let t = plain.indices.as_chunks::<3>().0.first()?;
                        let [a, b, c] = t.map(|v| Vec3::from_array(plain.positions[v as usize]));
                        (b - a).cross(c - a).try_normalize()
                    })
                    .unwrap_or(Vec3::Z)
            })
            .collect();
        Some(Cut {
            shards,
            numbers: glass.iter().map(|(number, _)| *number).collect(),
            faces,
            drawn: OnceLock::new(),
        })
    }

    /// Shard `number`'s meshes, each about the shard's middle, with the
    /// glass mesh it is of (by its place in `numbers`).
    fn meshes_of(
        &self,
        meshes: &mut Assets<Mesh>,
        number: usize,
    ) -> crate::prop_looks::PieceMeshes {
        let drawn = self.drawn.get_or_init(|| {
            (self.shards.iter())
                .map(|shard| {
                    let middle = Vec3::from_array(shard.middle);
                    (shard.meshes.iter())
                        .map(|(source, plain)| {
                            let positions: Vec<[f32; 3]> = (plain.positions.iter())
                                .map(|p| (Vec3::from_array(*p) - middle).to_array())
                                .collect();
                            let mut mesh = base_mesh(
                                &positions,
                                &plain.normals,
                                &plain.uvs,
                                RenderAssetUsages::RENDER_WORLD,
                            );
                            mesh.insert_indices(Indices::U32(plain.indices.clone()));
                            (*source, meshes.add(mesh))
                        })
                        .collect()
                })
                .collect()
        });
        drawn.get(number).cloned().unwrap_or_else(Vec::new)
    }
}

/// Each model's glass broken (none: the model has none), kept from area to
/// area; and, for the scene swap they belong to, the props whose glass is
/// gone and those hurt whose glass is still to go (once drawn): whether it
/// broke while seen, and where the prop stands.
#[derive(Resource, Default)]
pub(crate) struct Glass {
    cuts: BTreeMap<String, Option<Arc<Cut>>>,
    generation: u32,
    gone: BTreeSet<u32>,
    owed: BTreeMap<u32, (bool, [f32; 3])>,
}

impl Glass {
    /// Model `ir`'s glass broken, cut the first time.
    fn cut(
        &mut self,
        ir: &ModelIr,
        textures: &BTreeMap<String, TextureIr>,
        fractures: &mut Fractures,
    ) -> Option<Arc<Cut>> {
        (self.cuts.entry(ir.name.to_ascii_lowercase()))
            .or_insert_with(|| Cut::of(ir, &fractures.plains(ir, textures).2).map(Arc::new))
            .clone()
    }
}

/// A shard of glass as it falls (game space): where, turned how, how fast
/// it moves and spins, the floor it falls to, the way its face looks
/// (model frame), whether it lies still, when it came and when it starts to
/// shrink away (ms of `Time`).
#[derive(Component)]
pub(crate) struct Shard {
    at: Vec3,
    turn: Quat,
    velocity: Vec3,
    spin: Vec3,
    floor: f32,
    face: Vec3,
    still: bool,
    born_ms: f64,
    fade_ms: f64,
}

pub struct PropGlassPlugin;

impl Plugin for PropGlassPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Glass>().add_systems(
            PostUpdate,
            (shatter, fall)
                .chain()
                .after(crate::prop_looks::stages)
                .before(crate::prop_looks::wrecks)
                .run_if(resource_exists::<PresentedSnapshot>),
        );
    }
}

/// Whether a prop is hurt (`(look, damage)`): any damage, or broken.
fn hurt((shown, damage): (u8, u16)) -> bool {
    damage > 0 || shown == look::DEAD
}

/// Props hurt for the first time lose their glass: its meshes hidden, and
/// when it happened while seen, its shards thrown from the blow with a
/// burst and a crash.
#[allow(clippy::too_many_arguments)]
fn shatter(
    mut commands: Commands,
    (presented, swap, scene, made): (
        Res<PresentedSnapshot>,
        Res<SceneSwap>,
        Res<SceneContent>,
        Option<Res<Made>>,
    ),
    changes: Res<PropChanges>,
    (mut fractures, looks, mut glass): (ResMut<Fractures>, Res<PropLooks>, ResMut<Glass>),
    scene_models: Query<(Entity, &SceneModel)>,
    (globals, children, parts): (Query<&GlobalTransform>, Query<&Children>, MeshParts),
    shards: Query<Entity, With<Shard>>,
    mut meshes: ResMut<Assets<Mesh>>,
    (mut cues, mut bursts): (ResMut<Cues>, ResMut<Bursts>),
    time: Res<Time>,
    display: Option<Res<crate::display::Display>>,
) {
    let detail = crate::prop_looks::detail(display.as_deref());
    if glass.generation != swap.generation {
        glass.generation = swap.generation;
        glass.gone.clear();
        glass.owed.clear();
        for shard in &shards {
            commands.entity(shard).try_despawn();
        }
    }
    if !looks.current(swap.generation) {
        return;
    }
    for change in &changes.changes {
        if change.own_dead || glass.gone.contains(&change.index) || !hurt(change.now) {
            continue;
        }
        let live = change.was.is_some_and(|was| !hurt(was));
        glass.owed.insert(change.index, (live, change.position));
    }
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    let sound_ms = presented.0.current().time_ms as f32;
    let made = made.as_deref();
    let owed: Vec<(u32, (bool, [f32; 3]))> = glass.owed.iter().map(|(&i, &o)| (i, o)).collect();
    for (index, (live, feet)) in owed {
        let Some((model, root)) = model_of(index, (&scene, made), &scene_models) else {
            continue;
        };
        glass.owed.remove(&index);
        glass.gone.insert(index);
        let ir = &scene.0.models[model].ir;
        let Some(cut) = glass.cut(ir, &scene.0.textures, &mut fractures) else {
            continue;
        };
        // The glass goes: its meshes hidden, their materials kept for the
        // shards.
        let mut materials: BTreeMap<usize, Handle<OdysseyMaterial>> = BTreeMap::new();
        for entity in children.iter_descendants(root) {
            if let Ok((part, material)) = parts.get(entity)
                && let Some(at) = cut.numbers.iter().position(|number| *number == part.0)
            {
                materials.insert(at, material.0.clone());
                commands.entity(entity).try_insert(Visibility::Hidden);
            }
        }
        if !live {
            debug!(
                "prop #{index} ({}): first seen with its glass gone",
                ir.name
            );
            continue;
        }
        let Ok(global) = globals.get(root) else {
            continue;
        };
        let game = in_game(global);
        let (from, blast) = looks
            .blow_on(index)
            .unwrap_or((blow_from(feet, &changes.creatures), false));
        let from = Vec3::from_array(from);
        let axis = game.translation;
        // As many of its shards as the detail draws (`Detail::glass_every`).
        let drawn = (cut.shards.iter().enumerate())
            .filter(|(number, _)| detail.glass_every > 0 && number % detail.glass_every == 0);
        for (number, shard) in drawn {
            let at = game.transform_point(Vec3::from_array(shard.middle));
            let roll = |salt: u64| noise(&at.to_array(), salt ^ u64::from(index));
            let away = (at - from).normalize_or(Vec3::Z);
            let outward = Vec3::new(at.x - axis.x, at.y - axis.y, 0.0).normalize_or(away);
            let (lo, hi) = if blast { BLAST_SPEED } else { SPEED };
            let speed = lo + (hi - lo) * roll(1);
            let velocity =
                (away + outward).normalize_or(Vec3::Z) * speed + Vec3::Z * RISE * roll(2);
            let spin = Vec3::new(roll(3), roll(4), roll(5)).map(|c| (c * 2.0 - 1.0) * SPIN);
            let lasts = LASTS_S.0 + (LASTS_S.1 - LASTS_S.0) * f64::from(roll(6));
            let entity = commands
                .spawn((
                    Transform::from_rotation(game_to_bevy())
                        * Transform::from_translation(at).with_rotation(game.rotation),
                    Visibility::default(),
                    Shard {
                        at,
                        turn: game.rotation,
                        velocity,
                        spin,
                        floor: feet[2],
                        face: cut.faces[number],
                        still: false,
                        born_ms: now_ms,
                        fade_ms: now_ms + lasts * 1000.0,
                    },
                    Name::new(format!("{} glass {number}", ir.name)),
                ))
                .id();
            for (source, mesh) in cut.meshes_of(&mut meshes, number) {
                let Some(material) = materials.get(&source) else {
                    continue;
                };
                let child = commands
                    .spawn((
                        Mesh3d(mesh),
                        MeshMaterial3d(material.clone()),
                        Transform::IDENTITY,
                        NotShadowCaster,
                    ))
                    .id();
                commands.entity(entity).add_child(child);
            }
        }
        // The crash and the glint, over the glass.
        let units = |at: Vec3| at / asset_ja::UNIT;
        let middles: Vec<Vec3> = (cut.shards.iter())
            .map(|shard| game.transform_point(Vec3::from_array(shard.middle)))
            .collect();
        let middle = middles.iter().sum::<Vec3>() / middles.len().max(1) as f32;
        cues.0
            .push(Cue::at(SOUND.to_string(), units(middle), sound_ms));
        let step = (middles.len() / BURSTS).max(1);
        let glints = if detail.glass_burst { BURSTS } else { 0 };
        for at in middles.iter().step_by(step).take(glints) {
            bursts.0.push(Burst {
                effect: BURST,
                at: units(*at),
                normal: (*at - from).normalize_or(Vec3::Z),
                ms: sound_ms,
                times: 1,
                axis: None,
            });
        }
        info!(
            "prop #{index} ({}): its glass shatters into {} shards",
            ir.name,
            cut.shards.len()
        );
    }
}

/// The shards fall: down to their floor, bouncing a little and sliding on,
/// then lying flat, still; each shrinks away when its time is up, the
/// oldest early when there are too many.
fn fall(
    mut commands: Commands,
    mut shards: Query<(Entity, &mut Shard, &mut Transform)>,
    time: Res<Time>,
    display: Option<Res<crate::display::Display>>,
) {
    let most = MOST.min(crate::prop_looks::detail(display.as_deref()).most_glass);
    let now_ms = time.elapsed_secs_f64() * 1000.0;
    let dt = time.delta_secs().min(0.1);
    let mut live: Vec<(f64, Entity)> = Vec::new();
    for (entity, mut shard, mut transform) in &mut shards {
        if !shard.still {
            shard.velocity.z -= GRAVITY * dt;
            let step = shard.velocity * dt;
            shard.at += step;
            shard.turn = (Quat::from_scaled_axis(shard.spin * dt) * shard.turn).normalize();
            if shard.at.z <= shard.floor && shard.velocity.z < 0.0 {
                shard.at.z = shard.floor;
                if -shard.velocity.z * BOUNCE > STILL {
                    shard.velocity.z *= -BOUNCE;
                    shard.velocity.x *= SLIDE;
                    shard.velocity.y *= SLIDE;
                    shard.spin *= SLIDE;
                } else {
                    // Lying flat on whichever side came down.
                    shard.still = true;
                    let face = shard.turn * shard.face;
                    let flat = if face.z >= 0.0 { Vec3::Z } else { Vec3::NEG_Z };
                    shard.turn = Quat::from_rotation_arc(face, flat) * shard.turn;
                }
            }
        }
        let left = 1.0 - (now_ms - shard.fade_ms) / (SHRINK_S * 1000.0);
        if left <= 0.0 {
            commands.entity(entity).try_despawn();
            continue;
        }
        *transform = Transform::from_rotation(game_to_bevy())
            * Transform::from_translation(shard.at)
                .with_rotation(shard.turn)
                .with_scale(Vec3::splat(left.min(1.0) as f32));
        if now_ms < shard.fade_ms {
            live.push((shard.born_ms, entity));
        }
    }
    if live.len() > most {
        live.sort_by(|a, b| a.0.total_cmp(&b.0));
        for &(_, entity) in &live[..live.len() - most] {
            if let Ok((_, mut shard, _)) = shards.get_mut(entity) {
                shard.fade_ms = now_ms;
            }
        }
    }
}

/// Glass found and broken as it should be: a kolto tank's tank and a
/// windscreen are glass, a screen and a hidden mesh are not; a pane breaks
/// into small shards; a shard thrown lies flat on its floor.
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    use asset_odyssey::texture::TextureFacts;
    let plain = TextureFacts::default();
    let decal = TextureFacts {
        decal: true,
        ..TextureFacts::default()
    };
    let cases: [(&str, bool, bool); 6] = [
        (
            "kolto tank's tank",
            glass_by(
                1.0,
                Some("plc_koltoglass"),
                "mesh30",
                Surface::Blend,
                &[&plain],
            ),
            true,
        ),
        (
            "windscreen",
            glass_by(
                1.0,
                Some("plc_spdrwin01"),
                "glass03",
                Surface::OpaqueEnvMasked,
                &[&plain],
            ),
            true,
        ),
        (
            "see-through tube",
            glass_by(
                1.0,
                Some("plc_kolpuz"),
                "cylinder50",
                Surface::Blend,
                &[&plain],
            ),
            true,
        ),
        (
            "screen (a decal)",
            glass_by(
                1.0,
                Some("plc_swscrn01"),
                "swpscreen01",
                Surface::Blend,
                &[&decal],
            ),
            false,
        ),
        (
            "hidden mesh",
            glass_by(0.0, Some("plc_desk1"), "mesh32", Surface::Blend, &[&plain]),
            false,
        ),
        (
            "metal",
            glass_by(
                1.0,
                Some("plc_koltank"),
                "mesh01",
                Surface::Opaque,
                &[&plain],
            ),
            false,
        ),
    ];
    let wrong: Vec<String> = (cases.iter())
        .filter(|(_, got, want)| got != want)
        .map(|(what, got, want)| format!("{what}: glass {got}, want {want}"))
        .collect();
    vec![(
        "prop_glass_found",
        sim::probe::check(
            wrong.is_empty(),
            format!("{} cases, wrong: {wrong:?}", cases.len()),
        ),
    )]
}
