//! A droid changes nothing a story sees: the same world stepped with a
//! guest flying one wild, pressing every button, and with that guest's
//! droid hovering idle is the same world but for the droids, scripts'
//! state and all.

use super::super::super::SimWorld;
use super::super::super::drivers::probes::{party_world, poke, scripts_of};
use super::follows;
use crate::Guests;
use crate::input::{Command, Seat, TickInput};
use crate::probe::{Checks, check};
use crate::snapshot::Snapshot;

pub(super) fn probes() -> Checks {
    vec![("changes_nothing_else", unchanged())]
}

/// Seats 0 and 1 playing, the second companion out of the party: seat 2
/// has no body.
fn world() -> Result<SimWorld, String> {
    let (mut world, _, [_, b]) = party_world(Guests::Companions, 3)?;
    poke(&mut world, b, follows(None));
    scripts_of(&mut world).state.party.leave(1);
    Ok(world)
}

/// Seat 2's keys at `step`: turning, flying every way, every button in
/// turn (Use the recall among them).
fn keys(step: u32) -> Command {
    let turn = (step % 360) as f32;
    let axis = |phase: u32| {
        if (step / 40 + phase).is_multiple_of(3) {
            -127
        } else {
            127
        }
    };
    Command {
        angles: crate::moves::view_angles(0.0, turn),
        forward: axis(0),
        right: axis(1),
        up: axis(2),
        buttons: 1 << (step % 16),
        ..Command::default()
    }
}

/// One step, seat 2 flying wild or hovering idle.
fn step(world: &mut SimWorld, at: u32, wild: bool) -> Snapshot {
    let mut input = TickInput::default();
    let walk = Command {
        angles: crate::moves::view_angles(0.0, (at / 100 * 90) as f32),
        forward: if at % 200 < 150 { 127 } else { 0 },
        ..Command::default()
    };
    input.insert(Seat(0), walk);
    input.insert(Seat(1), walk);
    input.insert(Seat(2), if wild { keys(at) } else { Command::default() });
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, crate::StepReason::Authority)
}

/// 900 steps of both worlds; the first step their snapshots (droids put
/// aside) or scripts' states differ.
fn unchanged() -> Result<String, String> {
    let (mut with, mut without) = (world()?, world()?);
    let mut flew = 0;
    for at in 0..900 {
        let mut a = step(&mut with, at, true);
        let mut b = step(&mut without, at, false);
        flew += usize::from(!a.remotes.is_empty() && !b.remotes.is_empty());
        a.remotes.clear();
        b.remotes.clear();
        if a != b {
            return check(false, format!("the worlds differ at step {at}"));
        }
        if with.script_state() != without.script_state() {
            return check(false, format!("the scripts' states differ at step {at}"));
        }
    }
    check(
        flew == 900,
        format!("900 steps alike, a droid flying in {flew} of them"),
    )
}
