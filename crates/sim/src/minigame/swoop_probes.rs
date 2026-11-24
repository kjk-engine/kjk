//! A swoop's sideways motion and what it meets, as both executables have
//! them, on courses of our own making, no game files (`kjk sim probes
//! "minigame swoop"`):
//! - full steer from rest: the drift and offset after one second, as the
//!   executables' equation has them (the drift nears `MovementPerSec` times
//!   the strafe axis, held back by the lateral acceleration over
//!   `MovementPerSec`), at the races' top lateral acceleration and at a
//!   third of it;
//! - half steer: half the drift;
//! - the walls: K1 keeps the drift, TSL stops a fast one;
//! - the hits: a pad's reach counts the swoop's height above it, a buoy is
//!   met only where the swoop's middle passes into its box.

use super::probes::{MSEC, track};
use super::{Course, Follower, Hook, Kind, Obstacle, Path, Rules};
use crate::input::Command;
use crate::probe::{Checks, check};

pub fn probes() -> Checks {
    vec![
        ("full_steer_at_300", steer(300.0, 127, (68.326, 95.021))),
        ("full_steer_at_100", steer(100.0, 127, (36.788, 63.212))),
        ("half_steer", steer(300.0, 64, (34.432, 47.885))),
        ("k1_wall_keeps_drift", wall(false)),
        ("tsl_wall_stops_drift", wall(true)),
        ("pad_reach_counts_height", pad()),
        ("buoy_met_by_middle", buoy()),
    ]
}

/// K1's races: `MovementPerSec` 100, the lateral acceleration given, a
/// still track, walls `wall` metres out.
fn race(lateral_accel: f32, wall: f32) -> Rules {
    let mut rules = Rules::bare(Kind::Swoop);
    rules.movement_per_sec = 100.0;
    rules.lateral_accel = lateral_accel;
    rules.use_inertia = true;
    rules.player = Follower {
        path: track(0.0, 0.0),
        radius: 3.0,
        hp: 1,
        max_hp: 1,
        tunnel_pos: [wall, 0.0, 0.0],
        tunnel_neg: [-wall, 0.0, 0.0],
        ..Follower::default()
    };
    rules
}

/// Steps `course` `steps` times with the strafe axis at `right`; the hooks
/// raised on the player.
fn steps(rules: &Rules, course: &mut Course, right: i8, steps: u64) -> Vec<Hook> {
    let command = Command {
        right,
        ..Command::default()
    };
    (0..steps)
        .flat_map(|tick| course.step(rules, &[(0, command)], MSEC, tick))
        .filter(|raised| raised.who == 0)
        .map(|raised| raised.hook)
        .collect()
}

/// The strafe axis at `right` for one second from rest: the offset and the
/// drift there, against `want`. The executables' equation, solved:
/// drift = M (1 − e^(−t L/M)) u, offset = M t u − (M² / L)(1 − e^(−t L/M)) u,
/// with M = 100, L the lateral acceleration and u = `right` / 127.
fn steer(lateral_accel: f32, right: i8, want: (f32, f32)) -> Result<String, String> {
    let rules = race(lateral_accel, 1000.0);
    let mut course = Course::new(&rules);
    steps(&rules, &mut course, right, 1000 / u64::from(MSEC));
    let mover = &course.movers[0];
    let (offset, drift) = (mover.offset[0], mover.drift[0]);
    let near = |a: f32, b: f32| (a - b).abs() <= 0.01;
    check(
        near(offset, want.0) && near(drift, want.1),
        format!(
            "at lateral acceleration {lateral_accel}, strafe {right}: after 1 s the swoop is \
             {offset:.3} m across at {drift:.3} m/s (the executables: {:.3} m at {:.3} m/s)",
            want.0, want.1
        ),
    )
}

/// The strafe axis full into a wall 5 m out for a second: K1 keeps the
/// drift there, TSL's executable stops it once the wall holds the swoop
/// back fast enough.
fn wall(tsl: bool) -> Result<String, String> {
    let mut rules = race(300.0, 5.0);
    rules.tsl = tsl;
    let mut course = Course::new(&rules);
    steps(&rules, &mut course, 127, 1000 / u64::from(MSEC));
    let mover = &course.movers[0];
    let (offset, drift) = (mover.offset[0], mover.drift[0]);
    let held = (offset - 5.0).abs() < 1e-3;
    let kept = drift > 90.0;
    let ok = held && if tsl { drift < 65.0 } else { kept };
    check(
        ok,
        format!(
            "{}: pushed into the wall for 1 s, the swoop is {offset:.2} m across, its drift \
             {drift:.2} m/s",
            if tsl { "TSL" } else { "K1" }
        ),
    )
}

/// The swoop 2.655 m above the ground (K1's races' hook), down the track at
/// 10 m/s, `across` metres to the side of a pad of radius 2 on the ground
/// (its own 3): the hooks raised on the player as it passes.
fn passing(across: f32, obstacle: Option<Obstacle>) -> Vec<Hook> {
    let mut rules = race(300.0, 10.0);
    rules.player.path = Path {
        origin: [0.0, 0.0, 2.655],
        ..track(100.0, 10.0)
    };
    rules.player.hooks = super::Hooks::from_labels(&[
        ("OnHitFollower".into(), "pad".into()),
        ("OnHitObstacle".into(), "buoy".into()),
    ]);
    rules.player.max_speed = 100.0;
    if let Some(obstacle) = obstacle {
        rules.obstacles = vec![obstacle];
    } else {
        rules.enemies = vec![Follower {
            path: Path {
                origin: [0.0, 20.0, 0.0],
                ..track(0.0, 0.0)
            },
            radius: 2.0,
            hp: 1,
            max_hp: 1,
            trigger: true,
            ..Follower::default()
        }];
    }
    let mut course = Course::new(&rules);
    course.started = true;
    course.movers[0].speed = 100.0;
    course.movers[0].offset[0] = across;
    steps(&rules, &mut course, 0, 4000 / u64::from(MSEC))
}

/// A pad met within the two radii (5 m) of the swoop's middle, the swoop's
/// height above it counting: passing 4.5 m to its side, 2.655 m above it,
/// the swoop is 5.2 m off and misses; 4 m to its side it touches.
fn pad() -> Result<String, String> {
    let missed = !passing(4.5, None).contains(&Hook::HitFollower);
    let met = passing(4.0, None).contains(&Hook::HitFollower);
    check(
        missed && met,
        format!(
            "a pad 4.5 m to the side, 2.655 m below: {}; 4 m to the side: {}",
            if missed { "missed" } else { "met" },
            if met { "met" } else { "missed" }
        ),
    )
}

/// A buoy (K1's, 2.44 m half its width) met only where the swoop's middle
/// passes into its box: 3 m from its middle the swoop passes, 2 m from it
/// the swoop meets it.
fn buoy() -> Result<String, String> {
    let buoy = Obstacle {
        model: "probe_buoy".into(),
        name: "PROBE_BUOY".into(),
        position: [0.0, 20.0, 0.0],
        centre: [0.0, 20.0, 8.5],
        extent: [2.44, 4.24, 8.5],
        hooks: super::Hooks::default(),
    };
    let passed = !passing(3.0, Some(buoy.clone())).contains(&Hook::HitObstacle);
    let met = passing(2.0, Some(buoy)).contains(&Hook::HitObstacle);
    check(
        passed && met,
        format!(
            "a buoy's middle 3 m to the side: {}; 2 m to the side: {}",
            if passed { "passed" } else { "met" },
            if met { "met" } else { "passed" }
        ),
    )
}
