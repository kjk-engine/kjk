//! The falling lightsaber's probes (`kjk sim probes --only "saber falls"`).
//! A lightsaber knocked out of the air falls with gravity. One knocked into a
//! wall bounces off it and comes to rest on the floor below, as Jedi
//! Academy's knocked-down saber does, instead of staying in the air where it
//! met the wall.

use super::duels::play;
use crate::Seat;
use crate::combat::saber::throw::{Flight, HALF_SIZE};
use crate::probe::{Checks, check};

/// The inner face of the arena's west wall (units): 20 m from the middle.
const WEST_WALL: f32 = -20.0 / collision::UNIT;

/// Where seat 0's lightsaber lies after it was knocked from 40 units east of
/// the west wall at `vx` units per second (west is negative), and how it
/// flew there. The lightsaber starts 16 units above the fighter's origin,
/// and the run lasts 400 ticks of 8 ms, 3.2 seconds in all.
fn rest_after(vx: f32) -> Result<([f32; 3], Flight), String> {
    let x = WEST_WALL + 40.0;
    let text = format!(
        "scenario probe\nrules ja\nseed 7\n\
         fighter 0 medium x={x} yaw=180 defence=2 tiers=2 fp=100 hp=400\n\
         begin\ntick 20\naway 0 falling 1 {vx} 0 0\ntick 400\n"
    );
    let ticks = play(&text)?;
    let last = ticks.last().ok_or("no ticks")?;
    let thrown = last
        .own(Seat(0))
        .and_then(|entity| entity.fighter)
        .and_then(|f| f.saber.thrown)
        .ok_or("the lightsaber came back to the hand")?;
    Ok((thrown.at, thrown.flight))
}

/// A lightsaber knocked into the wall lies on the floor, as low as one that
/// fell in the open, and not inside the wall.
fn knocked_into_a_wall_falls_to_the_floor() -> Result<String, String> {
    let (wall, wall_flight) = rest_after(-200.0)?;
    let (open, open_flight) = rest_after(200.0)?;
    let lying = wall_flight == Flight::Lying && open_flight == Flight::Lying;
    let on_floor = (wall[2] - open[2]).abs() <= 1.0;
    let off_wall = wall[0] >= WEST_WALL + HALF_SIZE - 0.5;
    check(
        lying && on_floor && off_wall,
        format!(
            "knocked into the wall it is {wall_flight:?} at x {:.1}, z {:.1} (the wall is at x {WEST_WALL:.1}); \
             knocked into the open it is {open_flight:?} at z {:.1}",
            wall[0], wall[2], open[2]
        ),
    )
}

pub fn probes() -> Checks {
    vec![(
        "knocked_into_a_wall_falls_to_the_floor",
        knocked_into_a_wall_falls_to_the_floor(),
    )]
}
