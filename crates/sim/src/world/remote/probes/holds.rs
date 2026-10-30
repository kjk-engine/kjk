//! A droid waits out what holds the world (a conversation holding every
//! seat's body, a covering fade): hidden, still, its kit off; shown again
//! behind the leader. A guest who first speaks during one gets no droid
//! until it ends. A movie or a break holds the session itself: no step
//! runs (`docs/COOP.md`), so neither needs the sim's own rule.

use script_ncs::vm::ObjectId;

use super::super::super::SimWorld;
use super::super::super::drivers::probes::run;
use super::bare;
use crate::input::{Command, Seat, TickInput, buttons};
use crate::probe::{Checks, check};
use crate::remote::Remote;
use crate::snapshot::{Fade, Snapshot};

pub(super) fn probes() -> Checks {
    vec![
        ("held_in_conversation", conversation()),
        ("join_mid_hold", join()),
        ("hidden_in_fade", fade()),
    ]
}

/// A party conversation with seat 0's body: it holds the world.
fn talk(world: &mut SimWorld, on: bool) -> Result<(), String> {
    let leader = world
        .snapshot()
        .body(Seat(0))
        .map(|entity| entity.id)
        .ok_or("seat 0 has no body")?;
    let mut state = world.script_state().clone();
    state.talk.conversations.clear();
    if on {
        state
            .talk
            .conversations
            .push(crate::script::dialogue::Conversation {
                listener: ObjectId(leader.0),
                pc: ObjectId(leader.0),
                party: true,
                ..Default::default()
            });
    }
    // What the step reads (the scripts copy it there; the bare ground runs
    // none).
    world.world.resource_mut::<super::super::super::Talking>().0 = state.talk.told();
    world.set_script_state(state);
    Ok(())
}

fn screen(world: &mut SimWorld, covered: bool) {
    let now = world.world.resource::<super::super::super::Clock>().time_ms;
    world
        .world
        .resource_mut::<super::super::super::Module>()
        .fade = covered.then(|| Fade::level_load(now));
}

/// Seat 1 flying ahead and firing everything.
fn wild() -> Command {
    Command {
        forward: 127,
        up: 127,
        buttons: buttons::ATTACK | buttons::ALT_ATTACK | buttons::MEDPAC,
        ..Command::default()
    }
}

fn step(world: &mut SimWorld, guest: Option<Command>) -> Snapshot {
    let mut input = TickInput::default();
    input.insert(Seat(0), Command::default());
    if let Some(command) = guest {
        input.insert(Seat(1), command);
    }
    let tick = world.tick() + 1;
    crate::step(world, tick, &input, 8, crate::StepReason::Authority)
}

fn droid(snapshot: &Snapshot) -> Option<Remote> {
    snapshot.remote(Seat(1)).copied()
}

fn from_leader(snapshot: &Snapshot, remote: &Remote) -> f32 {
    let leader = snapshot
        .body(Seat(0))
        .map_or([0.0; 3], |entity| entity.position);
    crate::math::length(crate::math::sub(remote.at, leader))
}

/// Held `steps` steps with seat 1 flying wild: whether its droid stayed
/// hidden where the hold found it, its kit unspent.
fn held(world: &mut SimWorld, steps: u32) -> Result<(bool, Remote), String> {
    let first = droid(&step(world, Some(wild()))).ok_or("no droid in the hold")?;
    let mut still = first.hidden && first.ready_ms == [0; 3];
    for _ in 0..steps {
        let now = droid(&step(world, Some(wild()))).ok_or("the droid went in the hold")?;
        still &= now.hidden && now.at == first.at && now.ready_ms == [0; 3];
    }
    Ok((still, first))
}

/// The held case `hold` (on, then off): hidden and still through it, shown
/// behind the leader after.
fn waits(hold: impl Fn(&mut SimWorld, bool) -> Result<(), String>) -> Result<String, String> {
    let mut world = bare(2)?;
    for _ in 0..20 {
        step(&mut world, Some(Command::default()));
    }
    hold(&mut world, true)?;
    let (still, at) = held(&mut world, 60)?;
    hold(&mut world, false)?;
    let after = step(&mut world, Some(Command::default()));
    let shown = droid(&after).ok_or("no droid after the hold")?;
    let behind = from_leader(&after, &shown);
    check(
        still && !shown.hidden && behind < 2.5,
        format!(
            "held still and unspent {still} at {:?}; after, hidden {} and {behind:.1} m from the leader",
            at.at, shown.hidden
        ),
    )
}

fn conversation() -> Result<String, String> {
    waits(talk)
}

fn fade() -> Result<String, String> {
    waits(|world, covered| {
        screen(world, covered);
        Ok(())
    })
}

/// Seat 1's first command comes in a conversation: no droid until it
/// ends, then one.
fn join() -> Result<String, String> {
    let mut world = bare(2)?;
    run(&mut world, &[(0, 0)]);
    talk(&mut world, true)?;
    let mut during = Vec::new();
    for _ in 0..30 {
        during.extend(droid(&step(&mut world, Some(wild()))).map(|remote| remote.seat.0));
    }
    talk(&mut world, false)?;
    let after = droid(&step(&mut world, Some(Command::default())));
    check(
        during.is_empty() && after.is_some_and(|remote| !remote.hidden),
        format!("droids in the conversation {during:?}, after {after:?}"),
    )
}
