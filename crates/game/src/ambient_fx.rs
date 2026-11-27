//! Jedi Academy's ambient effects that a level editor project places in an
//! area (`level_edit::effects`): smoke, steam, sparks, fire, dust. Each is
//! played again and again where it stands, every `every` seconds, as Jedi
//! Academy's `fx_runner` entity plays its effect.
//!
//! - **In play** each effect asks `combat_fx` to play it (`Bursts`), which
//!   draws its particles, plays its sounds and runs the effects it runs.
//!   An effect farther than `REACH` from the camera waits, as one that Jedi
//!   Academy's client does not see is not drawn. `combat_fx` reads the
//!   effect files when play starts (`names`).
//! - **In the level editor** the effects are laid out once for a loop of
//!   `PREVIEW_MS` (`preview`), played at the same pace, and the scene draws
//!   the loop again and again (`scene_fx`). Each play is also laid out one
//!   loop earlier, so the particles alive when the loop starts again are
//!   still there. The preview plays no sound.

use std::collections::BTreeMap;

use asset_ja::fx::{self, Rng, Spawned};
use bevy::prelude::*;
use level_edit::effects::AmbientEffect;

use crate::combat_fx::{Burst, Bursts, FightDrawn};
use crate::scene::SceneContent;
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::viewer::{Effect, Effects, LoadedScene};
use crate::viewer_ja::Files;

const UNIT: f32 = asset_ja::UNIT;

/// The length of the editor's loop of the effects (milliseconds).
pub const PREVIEW_MS: f32 = 4000.0;
/// How far from the camera an effect plays (metres).
pub const REACH: f32 = 60.0;
/// How many effects one effect's runners may play in turn, in the editor's
/// preview (`combat_fx` counts its own).
const RUNNER_DEPTH: usize = 3;
/// A client frame of the editor's loop (milliseconds).
const FRAME_MS: f32 = 1000.0 / 60.0;
/// An effect behind by more than this (milliseconds), as after a load or a
/// pause, plays once now and goes on from there.
const MOST_BEHIND_MS: f32 = 2000.0;

/// The name each effect file is played by, each once, as text that lives
/// as long as the program: `combat_fx` keys its effects so.
static NAMES: std::sync::Mutex<BTreeMap<String, &'static str>> =
    std::sync::Mutex::new(BTreeMap::new());

/// `name` as text that lives as long as the program (`NAMES`).
fn kept(name: &str) -> &'static str {
    let mut names = NAMES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(&kept) = names.get(name) {
        return kept;
    }
    let kept: &'static str = Box::leak(name.to_string().into_boxed_str());
    names.insert(name.to_string(), kept);
    kept
}

/// The effects the active level editor projects place anywhere, of either
/// game, by the names `combat_fx` plays them by: it reads them when play
/// starts.
pub fn names() -> Vec<&'static str> {
    ["k1", "tsl"]
        .into_iter()
        .flat_map(level_edit::active::effect_names)
        .map(|name| kept(&name))
        .collect()
}

/// The axis an effect plays along: forward, then its two sides (game
/// space).
fn axis_of(effect: &AmbientEffect) -> [Vec3; 3] {
    fx::axis(Vec3::from_array(effect.forward()))
}

/// The game's generator for the `play`th play of the effect at `at`.
fn seed(at: [f32; 3], play: i64) -> Rng {
    let place = at[0].to_bits() ^ at[1].to_bits().rotate_left(11) ^ at[2].to_bits().rotate_left(22);
    crate::view_rng::ja_seed(place, (play as u32).wrapping_mul(0x85eb_ca6b))
}

/// Lays out `effects` in `scene` for the level editor's view: each one
/// played every `every` seconds over a loop of `PREVIEW_MS`, and once more
/// a loop earlier, so the loop goes round without a seam. The effect files
/// and the effects their runners play are read from Jedi Academy
/// (`files`), with their shaders' textures. What does not read goes to
/// `notes`.
pub fn preview(
    files: &Files,
    effects: &[AmbientEffect],
    scene: &mut LoadedScene,
    notes: &mut Vec<String>,
) {
    if effects.is_empty() {
        return;
    }
    let mut read: BTreeMap<String, fx::Effect> = BTreeMap::new();
    let mut load = |name: &str, notes: &mut Vec<String>| -> fx::Effect {
        read.entry(name.to_string())
            .or_insert_with(|| {
                crate::viewer_ja::effect_file(files, &format!("effects/{name}.efx"), notes)
            })
            .clone()
    };
    let mut items: Vec<Effect> = Vec::new();
    for effect in effects {
        let played = load(&effect.played_name(), notes);
        let every = (effect.every * 1000.0).max(FRAME_MS);
        let plays = (PREVIEW_MS / every).ceil() as i64;
        let at = Vec3::from_array(effect.at) / UNIT;
        let axis = axis_of(effect);
        for play in 0..plays {
            let start = play as f32 * every;
            if start >= PREVIEW_MS {
                break;
            }
            for shift in [0.0, -PREVIEW_MS] {
                let mut rng = seed(effect.at, play);
                let mut queue: std::collections::VecDeque<(Spawned, usize)> =
                    fx::play(&played, at, axis, start + shift, &mut rng, &|_, to| to)
                        .into_iter()
                        .map(|spawned| (spawned, 0))
                        .collect();
                while let Some((spawned, depth)) = queue.pop_front() {
                    match spawned.kind {
                        fx::Kind::Runner if depth < RUNNER_DEPTH => {
                            for name in spawned.shader.split(';').filter(|n| !n.is_empty()) {
                                let run = load(name, notes);
                                let mut rng = seed(effect.at, play.wrapping_add(depth as i64 + 1));
                                let axis = fx::axis(spawned.normal);
                                queue.extend(
                                    fx::play(
                                        &run,
                                        spawned.origin,
                                        axis,
                                        spawned.start_ms,
                                        &mut rng,
                                        &|_, to| to,
                                    )
                                    .into_iter()
                                    .map(|spawned| (spawned, depth + 1)),
                                );
                            }
                        }
                        fx::Kind::Runner | fx::Kind::Sound | fx::Kind::Decal => {}
                        _ => items.push(Effect::Fx(spawned)),
                    }
                }
            }
        }
    }
    if items.is_empty() {
        return;
    }
    let mut shaders: Vec<String> = (items.iter())
        .filter_map(|item| match item {
            Effect::Fx(spawned) => Some(spawned.shader.to_ascii_lowercase()),
            _ => None,
        })
        .collect();
    shaders.sort();
    shaders.dedup();
    let (textures, missing) = crate::viewer_ja::textures(files, &shaders);
    if !missing.is_empty() {
        notes.push(format!(
            "ambient effects: no texture for {}",
            missing.join(", ")
        ));
    }
    for (name, texture) in textures {
        scene.textures.entry(name).or_insert(texture);
    }
    let blends = crate::viewer_ja::effect_blends(files, shaders, notes);
    let laid = &mut scene.effects;
    if laid.items.is_empty() {
        *laid = Effects {
            period_ms: PREVIEW_MS,
            frame_ms: FRAME_MS,
            frame_origin_ms: 0.0,
            ..Effects::default()
        };
    }
    laid.items.extend(items);
    laid.blends.extend(blends);
}

pub struct AmbientFxPlugin;

impl Plugin for AmbientFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Paced>().add_systems(
            PostUpdate,
            play.run_if(resource_exists::<PresentedSnapshot>)
                .before(FightDrawn),
        );
    }
}

/// When each effect of the scene plays next (the view's milliseconds), and
/// the scene it is of.
#[derive(Resource, Default)]
struct Paced {
    generation: Option<u32>,
    next: Vec<Option<f32>>,
    /// The name each effect is played by (`kept`), looked up once when
    /// the scene is taken. Each play then needs no name made and no look
    /// in `NAMES`.
    names: Vec<&'static str>,
}

/// Asks `combat_fx` to play each of the scene's ambient effects
/// (`LoadedScene::ambient_effects`) whose time has come, while the camera is
/// within `REACH` of it.
fn play(
    presented: Res<PresentedSnapshot>,
    swap: Option<Res<SceneSwap>>,
    scene: Option<Res<SceneContent>>,
    cameras: Query<&GlobalTransform, crate::lifecycle_view::SceneCamera>,
    mut paced: ResMut<Paced>,
    mut bursts: ResMut<Bursts>,
) {
    let (Some(scene), Some(last)) = (scene, presented.1.last()) else {
        return;
    };
    let effects = &scene.0.ambient_effects;
    let generation = swap.as_ref().map(|swap| swap.generation);
    if paced.generation != generation || paced.next.len() != effects.len() {
        *paced = Paced {
            generation,
            next: vec![None; effects.len()],
            names: (effects.iter())
                .map(|effect| kept(&effect.played_name()))
                .collect(),
        };
    }
    if effects.is_empty() {
        return;
    }
    let now = last.time_ms as f32;
    let eye = cameras
        .iter()
        .next()
        .map(|camera| crate::scene::game_to_bevy().inverse() * camera.translation());
    let Paced { next, names, .. } = &mut *paced;
    for ((effect, next), &name) in effects.iter().zip(next.iter_mut()).zip(names.iter()) {
        let near = eye.is_none_or(|eye| eye.distance(Vec3::from_array(effect.at)) <= REACH);
        let every = (effect.every * 1000.0).max(FRAME_MS);
        let due = next.get_or_insert(now);
        if !near || now - *due > MOST_BEHIND_MS {
            *due = now;
            if !near {
                continue;
            }
        }
        while *due <= now {
            bursts.0.push(Burst {
                effect: name,
                at: Vec3::from_array(effect.at) / UNIT,
                normal: Vec3::from_array(effect.forward()),
                ms: *due,
                times: 1,
                axis: Some(axis_of(effect)),
            });
            *due += every;
        }
    }
}
