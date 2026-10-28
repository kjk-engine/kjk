//! A player's body, and a humanoid creature's, out of a fight moves as
//! KOTOR's own characters do: its
//! body's clips (`pause1`, `walk`, `run`, found through its supermodel
//! chain, so a woman's are the female ones) laid over the JA pose that
//! drives it everywhere else.
//!
//! - **When:** alive, its weapons away (`weapons::armed`), no script's
//!   animation playing, and its legs in JA's plain standing (its idle
//!   too), walking or running, forwards, sideways or backwards
//!   (`BOTH_STAND1`, `BOTH_STAND1IDLE1`, `BOTH_WALK1`, `BOTH_RUN1`,
//!   `BOTH_WALKBACK1`, `BOTH_RUNBACK1`): the KOTOR clip of the same.
//!   Anything else (a fight, a jump, a roll, a Force move) is JA's.
//! - **Facing** (the owner's call): moving, the body turns to the way it
//!   goes, as KOTOR's characters do (KOTOR has no strafing), at
//!   `TURN_DEG_S`; going backwards it faces the other way and plays its
//!   walk or run in reverse (KOTOR has no backpedal). Standing, it faces
//!   as JA's legs do (the view, as they turn to it).
//! - **Blending:** each clip's weight over the JA pose eases to 1 or 0
//!   over `EASE_S`, so going from standing to walking cross-fades the two
//!   clips and leaving the gait fades back into JA's pose; the weights are
//!   laid over the JA pose as one weighted mean (`shown`).
//! - **Timing:** standing plays at its own pace; walking and running
//!   advance by the distance covered, a cycle per stride
//!   (`appearance.2da`'s `walkdist`, `rundist`), so the feet keep to the
//!   floor at any speed.
//!
//! Render only: the sim moves the character, this draws it.

use std::sync::Arc;

use asset_odyssey::anim::AnimationIr;
use sim::motion::Anim;

use crate::scene::Animator;

/// Seconds a clip takes to come in or go out.
const EASE_S: f32 = 0.25;
/// How fast the body turns to where it goes (degrees a second).
const TURN_DEG_S: f32 = 720.0;
/// Below this (metres a second) the body is not going anywhere to face.
const MOVING: f32 = 0.3;
/// The clips, in `GaitClips::clips` order.
pub const NAMES: [&str; 3] = ["pause1", "walk", "run"];
const PAUSE: usize = 0;
const WALK: usize = 1;
const RUN: usize = 2;

/// A body's gait clips and its strides (metres a walk and a run cycle
/// cover).
#[derive(Clone, Default)]
pub struct GaitClips {
    pub clips: [Option<Arc<AnimationIr>>; 3],
    pub strides: [f32; 2],
}

/// A gait clip and whether it plays backwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub clip: usize,
    pub backwards: bool,
}

/// The JA legs' animation as a gait clip, if it is plain standing, walking
/// or running (backwards: the clip reversed).
pub fn of_legs(legs: Anim) -> Option<Step> {
    let (clip, backwards) = match legs {
        Anim::BOTH_STAND1 | Anim::BOTH_STAND1IDLE1 => (PAUSE, false),
        Anim::BOTH_WALK1 => (WALK, false),
        Anim::BOTH_RUN1 => (RUN, false),
        Anim::BOTH_WALKBACK1 => (WALK, true),
        Anim::BOTH_RUNBACK1 => (RUN, true),
        _ => return None,
    };
    Some(Step { clip, backwards })
}

/// How the body moves this frame: metres a second over the floor, the way
/// it goes and the way JA's legs face (game degrees).
#[derive(Debug, Clone, Copy)]
pub struct Motion {
    pub speed: f32,
    pub travel: f32,
    pub legs: f32,
}

/// `weight` moved `step` toward `target`, not past it.
pub fn ease(weight: f32, target: f32, step: f32) -> f32 {
    if weight < target {
        (weight + step).min(target)
    } else {
        (weight - step).max(target)
    }
}

/// How far (seconds) into a walk or run cycle `length` s long, which covers
/// `stride` m, a body moving at `speed` m/s for `seconds` goes: a cycle a
/// stride, so the feet keep to the floor.
pub fn by_stride(speed: f32, seconds: f32, stride: f32, length: f32) -> f32 {
    speed * seconds / stride.max(0.1) * length
}

/// `angle` (degrees) wrapped into −180 to 180.
fn wrap(angle: f32) -> f32 {
    (angle + 180.0).rem_euclid(360.0) - 180.0
}

/// A body's gait: its clips bound to its nodes, each one's weight and how
/// far into it it is.
pub struct Gait {
    clips: [Option<Animator>; 3],
    strides: [f32; 2],
    weights: [f32; 3],
    at: [f32; 3],
    /// Each clip played in reverse (its latest wanting).
    backwards: [bool; 3],
    /// Where the body faces (game degrees), turning to where it goes.
    facing: Option<f32>,
    last_ms: Option<f64>,
}

impl Gait {
    /// `clips` bound to the nodes of the body `placed` spawned as.
    pub fn bound(
        placed: &crate::viewer::Placed,
        clips: &GaitClips,
        nodes: &[bevy::prelude::Entity],
    ) -> Box<Gait> {
        let bound = clips.clips.each_ref().map(|clip| {
            clip.as_ref()
                .and_then(|clip| crate::scene::animator(placed, clip, nodes))
        });
        Box::new(Gait::new(bound, clips.strides))
    }

    pub fn new(clips: [Option<Animator>; 3], strides: [f32; 2]) -> Gait {
        Gait {
            clips,
            strides,
            weights: [0.0; 3],
            at: [0.0; 3],
            backwards: [false; 3],
            facing: None,
            last_ms: None,
        }
    }

    /// At `ms`, `wanted` plays (none: JA's pose), the body moving so.
    pub fn follow(&mut self, wanted: Option<Step>, motion: Motion, ms: f64) {
        let seconds = self
            .last_ms
            .map_or(0.0, |last| ((ms - last) / 1000.0) as f32);
        self.last_ms = Some(ms);
        let seconds = seconds.clamp(0.0, 0.25);
        let wanted = wanted.filter(|step| self.clips[step.clip].is_some());
        if let Some(step) = wanted {
            self.backwards[step.clip] = step.backwards;
        }
        // Moving, it faces the way it goes (the other way, backwards);
        // standing or under JA's pose, as JA's legs do.
        let going = wanted.filter(|step| step.clip != PAUSE && motion.speed > MOVING);
        let target = going.map_or(motion.legs, |step| {
            motion.travel + if step.backwards { 180.0 } else { 0.0 }
        });
        let facing = self.facing.unwrap_or(target);
        let turn = wrap(target - facing);
        let most = TURN_DEG_S * seconds;
        self.facing = Some(wrap(facing + turn.clamp(-most, most)));
        for (clip, weight) in self.weights.iter_mut().enumerate() {
            let target = if wanted.map(|step| step.clip) == Some(clip) {
                1.0
            } else {
                0.0
            };
            *weight = ease(*weight, target, seconds / EASE_S);
        }
        for (clip, at) in self.at.iter_mut().enumerate() {
            let Some(animator) = &self.clips[clip] else {
                continue;
            };
            let length = animator.length().max(1e-3);
            let advance = match clip {
                PAUSE => seconds,
                _ => by_stride(motion.speed, seconds, self.strides[clip - 1], length),
            };
            let advance = if self.backwards[clip] {
                -advance
            } else {
                advance
            };
            *at = (*at + advance).rem_euclid(length);
        }
    }

    /// Where the body faces (game degrees) while a clip shows.
    pub fn facing(&self) -> Option<f32> {
        self.facing
            .filter(|_| self.weights.iter().any(|weight| *weight > 0.0))
    }

    /// Each clip's weight over the JA pose, by name (for `dump`).
    pub fn facts(&self) -> serde_json::Value {
        NAMES
            .iter()
            .zip(self.weights)
            .filter(|(_, weight)| *weight > 0.0)
            .map(|(name, weight)| (name.to_string(), serde_json::json!(weight)))
            .collect::<serde_json::Map<_, _>>()
            .into()
    }

    /// The clips to lay over the JA pose, in order, each with how far in it
    /// is and the share it takes of what is laid so far: together the
    /// weighted mean of the JA pose and the clips.
    pub fn shown(&self) -> Vec<(&Animator, f32, f32)> {
        let mut laid = (1.0 - self.weights.iter().sum::<f32>()).max(0.0);
        let mut shown = Vec::new();
        for (clip, &weight) in self.weights.iter().enumerate() {
            let Some(animator) = self.clips[clip].as_ref().filter(|_| weight > 0.0) else {
                continue;
            };
            laid += weight;
            shown.push((animator, self.at[clip], weight / laid));
        }
        shown
    }
}
