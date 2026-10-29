//! How a droid flies: between its floor bounds, never inside the level,
//! leashed to the party, back behind the leader when stuck or recalled.

use super::super::super::SimWorld;
use super::super::Remotes;
use super::super::flight::{self, FLOOR_MAX_M, FLOOR_MIN_M, LEASH_M};
use super::{bare, droids};
use crate::input::{Command, Seat, TickInput, buttons};
use crate::probe::{Checks, check};
use crate::snapshot::Snapshot;

pub(super) fn probes() -> Checks {
    vec![
        ("hovers_between_floor_bounds", hovers()),
        ("never_inside_the_level", outside()),
        ("stays_near_the_party", leashed()),
        ("leash_pulls_to_the_nearest", leash()),
        ("snaps_when_stuck", stuck()),
        ("recall", recall()),
    ]
}

/// Seat 1's command: its keys (forward, right, up, buttons), looking along
/// `yaw` degrees.
fn keys(forward: i8, right: i8, up: i8, pressed: u16, yaw: f32) -> Command {
    Command {
        angles: crate::moves::view_angles(0.0, yaw),
        forward,
        right,
        up,
        buttons: pressed,
        ..Command::default()
    }
}

/// One authority step, seat 0 standing and seat 1 flying by `command`.
fn fly(world: &mut SimWorld, command: Command) -> Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), keys(0, 0, 0, 0, 0.0));
    input.insert(Seat(1), command);
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, crate::StepReason::Authority)
}

/// Seat 1's droid, flying after a first step.
fn flying() -> Result<(SimWorld, Snapshot), String> {
    let mut world = bare(2)?;
    let first = fly(&mut world, keys(0, 0, 0, 0, 0.0));
    if droids(&first) != [1] {
        return Err(format!("no droid to fly: {:?}", droids(&first)));
    }
    Ok((world, first))
}

fn droid(snapshot: &Snapshot) -> Result<[f32; 3], String> {
    snapshot
        .remote(Seat(1))
        .map(|remote| remote.at)
        .ok_or("the droid is gone".into())
}

fn apart(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

/// Seat 0's body (m).
fn leader(snapshot: &Snapshot) -> Result<[f32; 3], String> {
    snapshot
        .body(Seat(0))
        .map(|entity| entity.position)
        .ok_or("no leader".into())
}

/// Held up for 300 steps it stops at most `FLOOR_MAX_M` over its floor;
/// held down, at least `FLOOR_MIN_M`.
fn hovers() -> Result<String, String> {
    let mut heights = Vec::new();
    for up in [127, -127] {
        let (mut world, mut last) = flying()?;
        for _ in 0..300 {
            last = fly(&mut world, keys(0, 0, up, 0, 0.0));
        }
        let at = droid(&last)?;
        let floor = flight::floor(world.level(), at).ok_or("no floor under the droid")?;
        heights.push(at[2] - floor);
    }
    let slack = 0.05;
    check(
        heights[0] <= FLOOR_MAX_M + slack && heights[1] >= FLOOR_MIN_M - slack,
        format!(
            "held up {:.2} m over its floor, held down {:.2} m",
            heights[0], heights[1]
        ),
    )
}

/// Flown full ahead 400 steps in each of eight ways, its middle is never
/// inside the level.
fn outside() -> Result<String, String> {
    let mut inside = Vec::new();
    for way in 0..8 {
        let yaw = way as f32 * 45.0;
        let (mut world, _) = flying()?;
        for step in 0..400 {
            let at = droid(&fly(&mut world, keys(127, 0, 0, 0, yaw)))?;
            if flight::inside(world.level(), at) {
                inside.push((yaw, step));
                break;
            }
        }
    }
    check(
        inside.is_empty(),
        format!("inside the level (yaw, step): {inside:?}"),
    )
}

/// Flown full ahead for 2000 steps it stays within `LEASH_M` of the leader.
fn leashed() -> Result<String, String> {
    let (mut world, _) = flying()?;
    let mut farthest: f32 = 0.0;
    for step in 0..2000 {
        let after = fly(&mut world, keys(127, 0, 0, 0, (step / 400) as f32 * 90.0));
        farthest = farthest.max(apart(droid(&after)?, leader(&after)?));
    }
    check(
        farthest <= LEASH_M + 0.5,
        format!("at most {farthest:.1} m from the leader (leash {LEASH_M} m)"),
    )
}

/// 30 m out from one member and 25 from another, it is pulled to `LEASH_M`
/// from the nearer; within the leash, or with no party, it stays.
fn leash() -> Result<String, String> {
    let party = [[0.0, 0.0, 0.0], [55.0, 0.0, 0.0]];
    let pulled = flight::leashed(&party, [30.0, 0.0, 0.0]);
    let kept = flight::leashed(&party, [10.0, 0.0, 0.0]);
    let alone = flight::leashed(&[], [99.0, 0.0, 0.0]);
    check(
        apart(pulled, [55.0 - LEASH_M, 0.0, 0.0]) < 1e-3
            && kept == [10.0, 0.0, 0.0]
            && alone == [99.0, 0.0, 0.0],
        format!("pulled to {pulled:?}, kept at {kept:?}, alone at {alone:?}"),
    )
}

/// Put inside the level, 50 m under its floor, it is back near the
/// leader the next step.
fn stuck() -> Result<String, String> {
    let (mut world, _) = flying()?;
    if let Some(remote) = world.world.resource_mut::<Remotes>().0.first_mut() {
        remote.at[2] -= 50.0;
    }
    let after = fly(&mut world, keys(0, 0, 0, 0, 0.0));
    let near = apart(droid(&after)?, leader(&after)?);
    check(
        near <= 3.0,
        format!("{near:.1} m from the leader after the snap"),
    )
}

/// Flown away for 300 steps, Use brings it back near the leader at once.
fn recall() -> Result<String, String> {
    let (mut world, _) = flying()?;
    for _ in 0..300 {
        fly(&mut world, keys(127, 0, 0, 0, 0.0));
    }
    let away = fly(&mut world, keys(0, 0, 0, 0, 0.0));
    let back = fly(&mut world, keys(0, 0, 0, buttons::USE, 0.0));
    let (was, is) = (
        apart(droid(&away)?, leader(&away)?),
        apart(droid(&back)?, leader(&back)?),
    );
    check(
        was > 5.0 && is <= 3.0,
        format!("{was:.1} m from the leader, then {is:.1} m after Use"),
    )
}
