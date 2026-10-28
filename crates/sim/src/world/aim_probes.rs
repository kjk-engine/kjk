//! The probes of the crosshair line (`kjk sim probes --only aim`): a
//! player's shots go where the middle of its screen looks, whatever its
//! camera (`Command::aim`, `combat::mind::crosshair`), on the proving
//! ground, no install:
//! - the round trip: a camera's place turned into a command's aim
//!   (`mind::aim_for`) gives a crosshair line within a step of the camera's
//!   own, for JA's third-person camera (behind the line it looks along),
//!   the own eyes (below it and beside it) and a camera far off;
//! - a bolt: the player's pistol fired with its own eyes' aim, and with
//!   none (the third-person camera's), flies at where each line meets the
//!   level, and the two meet it apart (the eyes' line runs lower).

use super::SimWorld;
use crate::combat::mind;
use crate::input::{Command, Seat, TickInput, buttons};
use crate::math::{self, Vec3};
use crate::probe::{Checks, check};
use crate::{StepReason, WorldConfig};
use collision::{Contents, Extents, UNIT};

const MSEC: u32 = 8;
/// The eyes' aim the bolt is fired with: 2 units right, 9 below the
/// third-person line, as a first-person camera stood in a playtest.
const EYES: [i8; 3] = [0, -4, -18];

pub fn probes() -> Checks {
    vec![
        ("aim_round_trip", round_trip()),
        ("bolt_flies_at_the_camera_line", bolt()),
    ]
}

/// Each camera's place, turned into an aim, lies within a step of the
/// crosshair line that aim gives (the line runs along the view through
/// it).
fn round_trip() -> Result<String, String> {
    let mut ps = crate::motion::MotionState::standing([0.0, 0.0, 24.0], 30.0);
    ps.view = [12.0, 30.0, 0.0];
    let (forward, ..) = math::angle_vectors(ps.view);
    let line = |at: Vec3| {
        mind::crosshair(&crate::motion::MotionState {
            command_aim: mind::aim_for(&ps, at),
            ..ps
        })
    };
    let off_line = |camera: Vec3| {
        let from = line(camera);
        let off = math::sub(camera, from);
        math::length(math::mad(off, -math::dot(off, forward), forward))
    };
    let base = mind::crosshair(&ps);
    let cameras = [
        ("third person", math::mad(base, -80.0, forward)),
        ("own eyes", math::add(base, [1.3, -0.8, -9.2])),
        ("far off", math::add(base, [30.0, -20.0, 40.0])),
    ];
    let misses: Vec<(&str, f32)> = cameras
        .iter()
        .map(|&(name, at)| (name, off_line(at)))
        .collect();
    // A step on each axis, rounded: half a step's diagonal at most.
    let most = mind::AIM_STEP * 0.5 * 3f32.sqrt();
    check(
        misses.iter().all(|(_, miss)| *miss <= most),
        format!("off the camera's line (units, at most {most:.2}): {misses:?}"),
    )
}

/// Fires the player's pistol with `aim` (facing +X): its first bolt's
/// way, the point its line meets the level, and the bolt's start.
fn fire(aim: [i8; 3]) -> Result<(Vec3, Vec3, Vec3), String> {
    let level = crate::determinism::proving_ground();
    let mut world =
        SimWorld::new(WorldConfig::one_seat(), level).map_err(|error| error.to_string())?;
    let me = world
        .snapshot()
        .own(Seat(0))
        .map(|entity| entity.id)
        .ok_or("no player")?;
    let command = Command {
        angles: crate::moves::view_angles(2.0, 0.0),
        buttons: buttons::ATTACK,
        aim,
        ..Command::default()
    };
    for tick in 1..400 {
        let mut input = TickInput::default();
        input.insert(Seat(0), command);
        let snapshot = crate::step(&mut world, tick, &input, MSEC, StepReason::Authority);
        let bolt = snapshot
            .entities
            .iter()
            .find_map(|entity| entity.missile.filter(|missile| missile.owner == me));
        let ps = snapshot.own(Seat(0)).and_then(|entity| entity.player);
        if let (Some(bolt), Some(ps)) = (bolt, ps) {
            let from = mind::crosshair(&ps);
            let (forward, ..) = math::angle_vectors(ps.view);
            let far = math::mad(from, crate::combat::weapons::HITSCAN_RANGE, forward);
            let states = world.level().states_in(&snapshot);
            let met = world.level().with(states.as_deref(), |level| {
                level.trace(
                    math::scale(from, UNIT),
                    math::scale(far, UNIT),
                    &Extents::POINT,
                    Contents::SOLID,
                )
            });
            return Ok((bolt.velocity, math::scale(met.end, 1.0 / UNIT), bolt.base));
        }
    }
    Err("no bolt fired".into())
}

/// The angle between two ways (degrees).
fn between(mut a: Vec3, mut b: Vec3) -> f32 {
    math::normalize(&mut a);
    math::normalize(&mut b);
    math::dot(a, b).clamp(-1.0, 1.0).acos().to_degrees()
}

/// With the own eyes' aim and with none, the bolt flies from the gun at
/// where its line meets the level (within 0.05°), and the lines meet it
/// apart.
fn bolt() -> Result<String, String> {
    let (eyes_way, eyes_point, eyes_from) = fire(EYES)?;
    let (third_way, third_point, third_from) = fire([0; 3])?;
    let eyes_off = between(eyes_way, math::sub(eyes_point, eyes_from));
    let third_off = between(third_way, math::sub(third_point, third_from));
    let apart = math::length(math::sub(eyes_point, third_point));
    check(
        eyes_off < 0.05 && third_off < 0.05 && apart > 8.0,
        format!(
            "own eyes: {eyes_off:.3}° off its line's point; third person: {third_off:.3}°; the \
             points {apart:.1} units apart"
        ),
    )
}
