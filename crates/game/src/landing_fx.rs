//! Landings and rolls, as JA sounds them (`docs/SOUND.md`, "Landings"),
//! from the presented snapshots. The sim says what a step did (move_body's
//! `events`); how hard it came down is the vertical speed the snapshot
//! before it held (JA units a second), squared over 10,000 (the sim's
//! `landing` force):
//!
//! - A roll starting: `roll1` in the body slot, going with the roller. One
//!   out of a landing (a crouched body lands at twice the force, the roll
//!   takes two thirds of it) over 7 also sounds the fall below.
//! - A hard fall (the sim's `FALL`, over 7): `land1`, or past 44
//!   `fallsplat`. A jump coming down at or above where it left counts as 8
//!   at most; one coming down lower counts 0.3 less for each unit of its
//!   Force jump level's height it did not fall below its start, never
//!   under 8.
//! - A softer landing (the sim's `LAND`): a footstep (`ambience`).
//! - Any landing at over 300 units a second on dirt, sand, snow, mud or
//!   gravel (the floor's ground, from its texture: `collision::Ground`):
//!   JA's landing effect for it (`materials/<ground>_large`) where it
//!   touched, its dust and its own sound (`sand_land`, `snow_land`, …).

use std::collections::BTreeMap;

use audio::Channel;
use bevy::prelude::*;
use collision::{Contents, Extents, Ground};
use sim::combat::saber::Kit;
use sim::motion::{MotionState, events};
use sim::{EntityState, Instances, Level, SimId, Snapshot};

use crate::cast;
use crate::combat_fx::{Burst, Bursts, FightDrawn};
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::sound::{Cue, Cues};

const UNIT: f32 = asset_ja::UNIT;
const ROLL: &str = "sound/player/roll1.wav";
const LAND: &str = "sound/player/land1.wav";
const SPLAT: &str = "sound/player/fallsplat.wav";
/// A landing's force sounded as a fall, and as a splat.
const FALL_OVER: f32 = 7.0;
const SPLAT_OVER: f32 = 44.0;
/// The least force a jump's landing is sounded with.
const JUMP_LEAST: f32 = 8.0;
/// Each unit of a Force jump's height not fallen below its start takes
/// this much off the force.
const JUMP_EASE: f32 = 0.3;
/// A crouched landing counts twice; the roll out of it takes two thirds.
const CROUCHED: f32 = 2.0;
const ROLLED: f32 = 1.0 / 3.0;
/// The falling speed (units a second) a landing raises its ground's dust at.
const DUST_SPEED: f32 = 300.0;
/// How far below its box's origin a landing's ground is looked for (units:
/// the 24 to the soles and a margin).
const GROUND: f32 = 40.0;

/// The grounds JA raises dust from, and their landing effects.
const DUSTS: [(Ground, &str); 5] = [
    (Ground::Dirt, "materials/dirt_large"),
    (Ground::Sand, "materials/sand_large"),
    (Ground::Snow, "materials/snow_large"),
    (Ground::Mud, "materials/mud_large"),
    (Ground::Gravel, "materials/gravel_large"),
];
/// The effects this asks `combat_fx` for.
pub const EFFECTS: [&str; 5] = [
    DUSTS[0].1, DUSTS[1].1, DUSTS[2].1, DUSTS[3].1, DUSTS[4].1,
];

/// The sounds this plays itself (the effects' own are `combat_fx`'s).
pub fn sounds() -> impl Iterator<Item = String> {
    [ROLL, LAND, SPLAT].into_iter().map(String::from)
}

pub struct LandingFxPlugin;

/// Each character's fall as last seen: its vertical speed and the height
/// its jump left from (0: none).
#[derive(Resource, Default)]
struct Falls {
    seen: BTreeMap<SimId, (f32, f32)>,
    generation: u32,
}

impl Plugin for LandingFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Falls>().add_systems(
            PostUpdate,
            sound
                // Only in a session (the menu waits without one).
                .run_if(resource_exists::<PresentedSnapshot>)
                .before(FightDrawn),
        );
    }
}

fn sound(
    presented: Res<PresentedSnapshot>,
    swap: Res<SceneSwap>,
    mut falls: ResMut<Falls>,
    (mut cues, mut bursts): (ResMut<Cues>, ResMut<Bursts>),
) {
    let falls = &mut *falls;
    if swap.generation != falls.generation {
        *falls = Falls {
            generation: swap.generation,
            ..Falls::default()
        };
    }
    for snapshot in &presented.1 {
        for entity in &snapshot.entities {
            let Some(player) = &entity.player else {
                continue;
            };
            let was = falls
                .seen
                .insert(entity.id, (player.velocity[2], player.takeoff_z));
            if let Some(was) = was {
                landed(entity, player, was, (snapshot, &swap.level), (&mut cues.0, &mut bursts.0));
            }
        }
    }
}

/// What `entity` sounds landing this step, having fallen at `speed` from a
/// jump that left at `takeoff`.
fn landed(
    entity: &EntityState,
    player: &MotionState,
    (speed, takeoff): (f32, f32),
    (snapshot, level): (&Snapshot, &Level),
    (cues, bursts): (&mut Vec<Cue>, &mut Vec<Burst>),
) {
    let ev = player.events;
    if ev & (events::ROLL | events::FALL | events::LAND) == 0 {
        return;
    }
    let ms = snapshot.time_ms as f32;
    let at = Vec3::from_array(entity.position) / UNIT;
    if ev & events::ROLL != 0 {
        cues.push(Cue {
            entity: Some(entity.id),
            channel: Channel::Body,
            follows: true,
            ..Cue::at(ROLL.to_string(), at, ms)
        });
    }
    // A roll on the run starts on the ground.
    if speed >= 0.0 {
        return;
    }
    if ev & (events::ROLL | events::FALL) != 0 {
        let mut force = speed * speed * 0.0001;
        if ev & events::ROLL != 0 {
            force *= CROUCHED * ROLLED;
        } else if player.ducked() {
            force *= CROUCHED;
        }
        if force > FALL_OVER {
            let force = if takeoff != 0.0 {
                let objects = Instances::in_snapshot(level, snapshot);
                let kit = Kit::of(&cast::sheet(entity, &objects));
                let level = kit.force_jump.max(u8::from(kit.max_fp > 0)).min(3);
                jump_eased(force, player.origin[2], takeoff, level)
            } else {
                force
            };
            let name = if force > SPLAT_OVER { SPLAT } else { LAND };
            cues.push(Cue {
                entity: Some(entity.id),
                ..Cue::at(name.to_string(), at, ms)
            });
        }
    }
    if -speed >= DUST_SPEED
        && let Some((effect, ground, normal)) = ground(level, snapshot, at)
    {
        bursts.push(Burst {
            effect,
            at: ground,
            normal,
            ms,
            times: 1,
            axis: None,
        });
    }
}

/// A jump's landing force: `force` coming down at `z` from a jump that
/// left at `takeoff` (units), its Force jump at `level`.
fn jump_eased(force: f32, z: f32, takeoff: f32, level: u8) -> f32 {
    if force <= JUMP_LEAST {
        return force;
    }
    if z.trunc() >= takeoff.trunc() {
        return JUMP_LEAST;
    }
    let below = takeoff.trunc() - z.trunc();
    let unfallen = (rules::force::jump::HEIGHTS[usize::from(level)] - below).max(0.0);
    (force.trunc() - unfallen * JUMP_EASE).max(JUMP_LEAST)
}

/// The landing effect for the ground under `at` (units), where it was met
/// and its facing; none off dirt, sand, snow, mud and gravel.
fn ground(level: &Level, snapshot: &Snapshot, at: Vec3) -> Option<(&'static str, Vec3, Vec3)> {
    let states = level.states_in(snapshot);
    let met = level.with(states.as_deref(), |world| {
        world.trace(
            (at * UNIT).to_array(),
            ((at - Vec3::Z * GROUND) * UNIT).to_array(),
            &Extents::POINT,
            Contents::SOLID,
        )
    });
    if met.fraction >= 1.0 || met.start_solid {
        return None;
    }
    let &(_, effect) = DUSTS
        .iter()
        .find(|(ground, _)| *ground == met.surface.ground)?;
    let normal = Vec3::from_array(met.plane.normal)
        .try_normalize()
        .unwrap_or(Vec3::Z);
    Some((effect, Vec3::from_array(met.end) / UNIT, normal))
}

