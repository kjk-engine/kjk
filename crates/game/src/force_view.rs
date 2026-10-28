//! The force powers' effects in `kjk play`, drawn each frame from the
//! casters' channels and hands as JA's client draws them: lightning from
//! its effect files at the left hand, played back from every client frame
//! whose primitives may still live; a grip's puffs at the left hand, two
//! a client frame, each added twice, tinted by a slow red pulse (`grip`);
//! and a push's or pull's distortion, following the hand for a moment and
//! then held where it was (`scene_fx::distort`).
//!
//! Everyone is drawn from the host's snapshots at the view's time, but a
//! joined player's own character, which shows from its predictions at its
//! own time (`shots::OwnFire`): its casts' effects are there at once
//! (QUIRKS.md Q-0071), where JA's cgame shows its own player's from the
//! snapshot's state, a round trip on.

use std::collections::BTreeMap;

use asset_ja::fx::{self, Rng};
use asset_odyssey::texture::TextureIr;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;
use collision::{Contents, Extents};
use console::DumpFacts;
use sim::combat::force::{Gesture, Power, ja};
use sim::{EntityState, SimId};

use crate::cast::Hitboxes;
use crate::lifecycle_view::SceneCamera;
use crate::material::OdysseyMaterial;
use crate::scene::game_to_bevy;
use crate::scene_fx::{self, Batch, Puff, RefractionMaterial, ShaderMeshes, View};
use crate::shots::OwnFire;
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::viewer::{EffectBlend, EffectModel};
use crate::viewer_ja::Files;

const UNIT: f32 = asset_ja::UNIT;
/// JA's client frames, as the effects are timed (QUIRKS.md Q-0005).
const FRAME_MS: f32 = 1000.0 / 60.0;
/// A grip's puffs: how long each lives, how fast it drifts (units a
/// second, along the view's left), and its two shaders.
pub(crate) const PUFF_MS: f32 = 120.0;
pub(crate) const PUFF_SPEED: f32 = 55.0;
pub(crate) const GRIP_SHADERS: [&str; 2] = ["gfx/effects/forcePush", "gfx/effects/sabers/red_glow"];
/// A push's or pull's distortion: how long it lasts, and how long it
/// follows the hand.
const DISTORT_MS: f32 = 500.0;
const FOLLOW_MS: f32 = 200.0;
/// How long a lightning efx's primitives may live, at most (ms): how far
/// back its copies are played.
const LIGHTNING_BACK_MS: f32 = 250.0;

/// The effects' files, read from JA's.
pub struct Setup {
    /// Lightning at levels 1 and 2, and at 3.
    lightning: [fx::Effect; 2],
    refraction: Option<EffectModel>,
    textures: BTreeMap<String, TextureIr>,
    blends: BTreeMap<String, EffectBlend>,
}

/// Reads the powers' effects; what is missing goes to the notes.
pub fn load(files: &Files, notes: &mut Vec<String>) -> Setup {
    let lightning = [
        "effects/force/lightning.efx",
        "effects/force/lightningwide.efx",
    ]
    .map(|path| crate::viewer_ja::effect_file(files, path, notes));
    let mut shaders: Vec<String> = lightning
        .iter()
        .flat_map(|effect| &effect.templates)
        .flat_map(|template| &template.shaders)
        .map(String::as_str)
        .chain(GRIP_SHADERS)
        .map(str::to_ascii_lowercase)
        .collect();
    shaders.sort();
    shaders.dedup();
    let (textures, missing) = crate::viewer_ja::textures(files, &shaders);
    if !missing.is_empty() {
        notes.push(format!(
            "force effects: no texture for {}",
            missing.join(", ")
        ));
    }
    let blends = crate::viewer_ja::effect_blends(files, shaders, notes);
    Setup {
        lightning,
        refraction: crate::viewer_rig::refraction_model(files, notes),
        textures,
        blends,
    }
}

/// Its systems; a session comes in with `install`.
pub struct ForceViewPlugin;

/// A session's powers' effects, put into `app`: their meshes are made on
/// the next frame.
pub fn install(app: &mut World, setup: Setup) {
    app.insert_resource(Pending(Some(setup)));
}

/// The setup until the first frame builds the meshes.
#[derive(Resource)]
struct Pending(Option<Setup>);

/// The effects and the meshes they are drawn into.
#[derive(Resource)]
struct Drawn {
    lightning: [fx::Effect; 2],
    shaders: ShaderMeshes,
    refraction: Option<(EffectModel, Handle<Mesh>)>,
    /// Each distortion's cast (ms) and where it stopped following the hand.
    held: BTreeMap<SimId, (f32, Vec3)>,
    /// Client frames drawn with lightning, grip and a distortion.
    frames: [usize; 3],
}

impl Plugin for ForceViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreUpdate, spawn.run_if(resource_exists::<Pending>))
            .add_systems(
                PostUpdate,
                // After the bodies are placed (the hands), before the
                // meshes' changes are sent on.
                draw.run_if(resource_exists::<Drawn>)
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::asset::AssetEventSystems),
            );
    }
}

/// A mesh and a material per shader, and the distortion's; they outlive the
/// scenes.
fn spawn(
    mut commands: Commands,
    mut pending: ResMut<Pending>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<OdysseyMaterial>>,
    mut refractions: ResMut<Assets<RefractionMaterial>>,
) {
    let Some(setup) = pending.0.take() else {
        return;
    };
    let shaders = ShaderMeshes::spawn(
        &mut commands,
        (&setup.blends, &setup.textures),
        &mut images,
        &mut meshes,
        &mut materials,
    );
    let refraction = setup.refraction.map(|model| {
        let mesh = meshes.add(Batch::default().into_mesh());
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(refractions.add(RefractionMaterial::default())),
            Transform::IDENTITY,
            NoFrustumCulling,
        ));
        (model, mesh)
    });
    commands.insert_resource(Drawn {
        lightning: setup.lightning,
        shaders,
        refraction,
        held: BTreeMap::new(),
        frames: [0; 3],
    });
    commands.remove_resource::<Pending>();
}

/// The client frames' times from `from` to `to` (ms).
fn frames(from: f32, to: f32) -> impl Iterator<Item = f32> {
    let first = (from / FRAME_MS).ceil() as i64;
    (first..)
        .map(|k| k as f32 * FRAME_MS)
        .take_while(move |&t| t <= to)
}

/// The grip's tint at the client's clock `t` (ms): a slow pulse of red.
pub(crate) fn grip_red(t: f32) -> u8 {
    let wave = (t * 0.004).sin() * 0.08 + 0.1;
    (200.0 + wave * 255.0).min(255.0) as u8
}

/// The two puffs a grip adds each time, in order: (speed along the view's
/// left in units a second, rotation in degrees, colour, shader). The first
/// drifts left tinted `red`, the second right, white and turned over.
pub(crate) fn grip_pair(red: u8) -> [(f32, f32, [u8; 3], &'static str); 2] {
    [
        (PUFF_SPEED, 0.0, [red, 0, 0], GRIP_SHADERS[0]),
        (-PUFF_SPEED, 180.0, [255, 255, 255], GRIP_SHADERS[1]),
    ]
}

/// One client frame `t` (the caster's view time, ms) of a grip channel
/// that started at `from`, from the left hand at `hand` (units): the puffs
/// of every client frame since that may still live, all tinted as now,
/// each alive drawn twice (JA's client adds the grip's effect twice a
/// frame). A puff whose shader has no blend is left out.
fn grip(
    batches: &mut BTreeMap<String, Batch>,
    (from, t): (f32, f32),
    hand: Vec3,
    blend_of: &dyn Fn(&str) -> Option<EffectBlend>,
    view: &View,
    to_bevy: impl Fn(Vec3) -> Vec3 + Copy,
) {
    let red = grip_red(t);
    for start in frames(from.max(t - PUFF_MS), t) {
        for (speed, rotation, colour, shader) in grip_pair(red) {
            let shader = shader.to_ascii_lowercase();
            let Some(blend) = blend_of(&shader) else {
                continue;
            };
            let puff = Puff {
                start_ms: start,
                end_ms: start + PUFF_MS,
                origin: hand.to_array(),
                speed,
                radius: 2.0,
                rotation,
                colour,
            };
            if !puff.alive(t) {
                continue;
            }
            let batch = batches.entry(shader).or_default();
            for _ in 0..2 {
                puff.draw(batch, (blend, t), view, to_bevy);
            }
        }
    }
}

/// A push's or pull's cast (sim ms) under way in `force`, and whether it
/// is a pull.
fn throw(force: &sim::combat::force::ForceState) -> Option<(u64, bool)> {
    let (gesture, until) = force.hand?;
    let (hand, wait, pull) = match gesture {
        Gesture::Push => (ja::PUSH_HAND_MS, ja::PUSH_COOLDOWN_MS, false),
        Gesture::Pull => (ja::PULL_HAND_MS, ja::PULL_COOLDOWN_MS, true),
        Gesture::Choke => return None,
    };
    let cast = until.checked_sub(hand)?;
    (force.push_pull_ready == cast + wait).then_some((cast, pull))
}

#[allow(clippy::too_many_arguments)]
fn draw(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    hitboxes: Res<Hitboxes>,
    fire: Res<OwnFire>,
    drawn: Option<ResMut<Drawn>>,
    places: Query<&GlobalTransform>,
    cameras: Query<(&GlobalTransform, &Projection, &Camera), SceneCamera>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut facts: ResMut<DumpFacts>,
) {
    let Some(mut drawn) = drawn else { return };
    let drawn = &mut *drawn;
    let Ok((camera, projection, info)) = cameras.single() else {
        return;
    };
    let view = View::of(camera, projection, info);
    let (previous, current) = (presented.0.previous(), presented.0.current());
    let now = previous.time_ms as f64
        + (current.time_ms as f64 - previous.time_ms as f64) * f64::from(presented.0.alpha());
    let now = now as f32;
    // Everyone at the view's time, the own character at its own.
    let own = fire.own.zip(fire.shift);
    let mut casters: Vec<(&EntityState, f32)> = current
        .entities
        .iter()
        .filter(|entity| own.is_none_or(|(own, _)| own.id != entity.id))
        .map(|entity| (entity, now))
        .collect();
    if let Some((own, shift)) = &own {
        casters.push((own, now + *shift as f32));
    }
    let level = &swap.level;
    let doors = level.states_in(current);
    let ends = |from: Vec3, to: Vec3| {
        let hit = level.with(doors.as_deref(), |world| {
            world.trace(
                (from * UNIT).to_array(),
                (to * UNIT).to_array(),
                &Extents::POINT,
                Contents::SOLID,
            )
        });
        from + (to - from) * hit.fraction
    };
    let to_game = game_to_bevy().inverse();
    let to_bevy = |p: Vec3| game_to_bevy() * (p * UNIT);
    let eye = view.eye();
    let mut batches: BTreeMap<String, Batch> = BTreeMap::new();
    let mut refraction = (Batch::default(), Vec::new());
    let mut held = BTreeMap::new();
    for (entity, t) in casters {
        let (Some(fighter), Some(ps)) = (entity.fighter, entity.player) else {
            continue;
        };
        if !fighter.alive() {
            continue;
        }
        let force = fighter.force;
        let hand = hitboxes
            .0
            .get(&entity.id)
            .and_then(|spots| spots[sim::combat::Location::HandLeft as usize])
            .and_then(|node| places.get(node).ok())
            .map_or_else(
                || Vec3::from_array(sim::combat::mind::chest(&ps)),
                |place| to_game * place.translation() / UNIT,
            );
        let (forward, ..) = sim::math::angle_vectors(ps.view);
        let axis = fx::axis(Vec3::from_array(forward));
        match force.channel.map(|c| (c.power, c.level, c.from as f32)) {
            Some((Power::Lightning, level, from)) => {
                let effect = &drawn.lightning[usize::from(level > 2)];
                for start in frames(from.max(t - LIGHTNING_BACK_MS), t) {
                    let mut rng = Rng((entity.id.0 as i32) ^ (start / FRAME_MS) as i32);
                    for spawned in fx::play(effect, hand, axis, start, &mut rng, &ends) {
                        if !spawned.alive(t) {
                            continue;
                        }
                        let shader = spawned.shader.to_ascii_lowercase();
                        if let Some(look) = drawn.shaders.look(&shader) {
                            let batch = batches.entry(shader).or_default();
                            let mut rng = Rng(spawned.seed ^ (t / FRAME_MS) as i32);
                            scene_fx::primitive(
                                batch,
                                &spawned,
                                look,
                                (t, FRAME_MS),
                                &mut rng,
                                &eye,
                                to_bevy,
                            );
                        }
                    }
                }
                drawn.frames[0] += 1;
            }
            Some((Power::Grip, _, from)) => {
                grip(
                    &mut batches,
                    (from, t),
                    hand,
                    &|shader| drawn.shaders.blend(shader),
                    &view,
                    to_bevy,
                );
                drawn.frames[1] += 1;
            }
            _ => {}
        }
        if let (Some((cast, pull)), Some((model, _))) = (throw(&force), &drawn.refraction) {
            let cast = cast as f32;
            if t >= cast && t < cast + DISTORT_MS {
                let at = match drawn.held.get(&entity.id) {
                    Some(&(was, at)) if was == cast && t >= cast + FOLLOW_MS => at,
                    _ => hand,
                };
                held.insert(entity.id, (cast, at));
                let (batch, indices) = &mut refraction;
                scene_fx::distort(
                    model,
                    (cast, pull, &[(cast, at.to_array())]),
                    t,
                    &view,
                    batch,
                    indices,
                    to_bevy,
                );
                drawn.frames[2] += 1;
            }
        }
    }
    drawn.held = held;
    drawn.shaders.fill(batches, &mut meshes);
    if let Some((_, mesh)) = &drawn.refraction
        && let Some(mut handle) = meshes.get_mut(mesh)
    {
        let (batch, indices) = refraction;
        *handle = batch.into_triangles(indices);
    }
    facts.0.insert(
        "force_fx".into(),
        serde_json::json!({
            "lightning_frames": drawn.frames[0],
            "grip_frames": drawn.frames[1],
            "distortion_frames": drawn.frames[2],
        }),
    );
}
