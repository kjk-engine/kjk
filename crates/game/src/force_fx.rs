//! The force powers' effects on a rig timeline's player, laid out once for
//! the whole clip as JA's client would show them each client frame
//! (`viewer::Effects`):
//!
//! - lightning: the power's effect file played from the left hand along
//!   the torso's aim every client frame, its random numbers seeded by the
//!   frame;
//! - grip: two puffs at the left hand every client frame, added twice as
//!   JA's client adds them, the first tinted by a slow red pulse;
//! - push and pull: the refraction model's distortion, following the hand
//!   for its first 200 ms;
//! - while a thrown saber flies, a pulsing yellow glow on the right hand.
//!
//! The counts go to the notes.

use std::collections::BTreeMap;

use asset_ja::angles::Angles;
use asset_ja::fx::{self, Rng, Spawned};
use bevy::math::Vec3;

use crate::rig_timeline::{Power, PowerKind};
use crate::viewer::{Effect, EffectBlend, Effects};
use crate::viewer_ja::Files;

const FRAME_MS: f32 = 1000.0 / 60.0;
const UNIT: f32 = asset_ja::UNIT;

/// What the effects follow, in the timeline's milliseconds and game-space
/// metres.
pub struct Player<'a> {
    pub left_hand: &'a dyn Fn(f32) -> Vec3,
    pub right_hand: &'a dyn Fn(f32) -> Vec3,
    /// The timeline's angle samples (ms, pose).
    pub angles: &'a [(f32, asset_ja::angles::Pose)],
    /// When thrown sabers fly: (from, to) ms.
    pub flights: &'a [(f32, f32)],
}

/// The effects of `powers` on `player`, the timeline starting at
/// `start_ms` of the clip, which repeats every `period_ms`.
pub fn effects(
    files: &Files,
    powers: &[Power],
    player: &Player,
    (start_ms, period_ms): (f32, f32),
    notes: &mut Vec<String>,
) -> Effects {
    let mut out = Effects {
        period_ms,
        frame_ms: FRAME_MS,
        frame_origin_ms: start_ms,
        ..Effects::default()
    };
    let line = crate::viewer_ja::effect_file(files, "effects/force/lightning.efx", notes);
    let wide = crate::viewer_ja::effect_file(files, "effects/force/lightningwide.efx", notes);
    // Where a line from a to b stops: the floor (z 0) is the only solid.
    let trace = |a: Vec3, b: Vec3| {
        if b.z < 0.0 && a.z > 0.0 {
            a + (b - a) * (a.z / (a.z - b.z))
        } else {
            b
        }
    };
    let units = |p: Vec3| p / UNIT;
    let frames = |from: f32, to: f32| {
        let first = (from / FRAME_MS).ceil() as i64;
        (first..)
            .map(|k| k as f32 * FRAME_MS)
            .take_while(move |&t| t < to)
    };
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    for power in powers {
        match power.kind {
            PowerKind::Lightning => {
                let effect = if power.level > 2 { &wide } else { &line };
                for t in frames(power.start, power.end) {
                    let axis = axis_of(torso_aim(player.angles, t));
                    // The game's generator; seeded by the frame, so a shot
                    // repeats.
                    let mut rng = Rng((t / FRAME_MS).round() as i32);
                    let spawned = fx::play(
                        effect,
                        units((player.left_hand)(t)),
                        axis,
                        start_ms + t,
                        &mut rng,
                        &trace,
                    );
                    *counts.entry("lightning").or_insert(0) += spawned.len();
                    out.items.extend(spawned.into_iter().map(Effect::Fx));
                }
            }
            PowerKind::Grip => {
                use crate::force_view::{PUFF_MS, grip_pair, grip_red};
                for t in frames(power.start, power.end) {
                    let origin = units((player.left_hand)(t)).to_array();
                    let red = grip_red(t);
                    // JA's client adds the grip's effect twice a frame.
                    for _ in 0..2 {
                        for (speed, rotation, colour, shader) in grip_pair(red) {
                            out.items.push(Effect::Puff {
                                start_ms: start_ms + t,
                                end_ms: start_ms + t + PUFF_MS,
                                origin,
                                speed,
                                radius: 2.0,
                                rotation,
                                colour,
                                shader: shader.to_string(),
                            });
                            *counts.entry("grip puffs").or_insert(0) += 1;
                        }
                    }
                }
            }
            PowerKind::Push | PowerKind::Pull => {
                let path = frames(power.start, (power.start + 200.0).min(power.end))
                    .map(|t| (start_ms + t, units((player.left_hand)(t)).to_array()))
                    .collect();
                out.items.push(Effect::Refraction {
                    start_ms: start_ms + power.start,
                    end_ms: start_ms + power.end,
                    pull: power.kind == PowerKind::Pull,
                    path,
                });
                *counts.entry("distortions").or_insert(0) += 1;
            }
        }
    }
    // While a thrown saber flies the right hand glows, its alpha a slow
    // pulse; each client frame's glow lives a millisecond.
    for &(from, to) in player.flights {
        for t in frames(from, to) {
            let wave = (t * 0.003).sin() * 0.08 + 0.1;
            out.items.push(Effect::Fx(fx::sprite_particle(
                units((player.right_hand)(t)),
                (8.0, 8.0),
                (wave, wave),
                start_ms + t,
                1.0,
                "gfx/effects/yellow_glow",
                fx::Flags::USE_ALPHA,
            )));
            *counts.entry("hand glows").or_insert(0) += 1;
        }
    }
    if !counts.is_empty() {
        let tally: Vec<String> = counts.iter().map(|(key, n)| format!("{n} {key}")).collect();
        notes.push(format!("force effects: {}", tally.join(", ")));
    }
    out
}

/// The torso's aim (pitch, yaw) at `ms`: the last sample by then.
fn torso_aim(angles: &[(f32, asset_ja::angles::Pose)], ms: f32) -> Angles {
    let at = angles.partition_point(|(t, _)| *t <= ms);
    angles
        .get(at.saturating_sub(1))
        .map_or([0.0; 3], |(_, pose)| [pose.torso[0], pose.torso[1], 0.0])
}

/// Quake's axis for (pitch, yaw, 0) (`sim::math::angle_vectors`), turned
/// into game space (yaw 0 is KOTOR's +Y, Quake's +Y the player's left):
/// forward, left, up.
fn axis_of(angles: Angles) -> [Vec3; 3] {
    let (forward, right, up) = sim::math::angle_vectors([angles[0], angles[1], 0.0]);
    let game = |q: [f32; 3]| Vec3::new(-q[1], q[0], q[2]);
    [game(forward), -game(right), game(up)]
}

/// How the effect shaders' first stages blend, from the JA shader files.
pub fn blends(
    files: &Files,
    effects: &Effects,
    notes: &mut Vec<String>,
) -> BTreeMap<String, EffectBlend> {
    let mut names: Vec<String> = effects
        .items
        .iter()
        .filter_map(|item| match item {
            Effect::Fx(Spawned { shader, .. }) | Effect::Puff { shader, .. } => {
                Some(shader.to_ascii_lowercase())
            }
            Effect::Refraction { .. } => None,
        })
        .collect();
    names.sort();
    names.dedup();
    crate::viewer_ja::effect_blends(files, names, notes)
}
