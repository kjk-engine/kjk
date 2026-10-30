//! The probes of the remote droid (`kjk sim probes --only remote`).

use super::super::SimWorld;
use super::super::drivers::probes::{party_world, poke, run, scripts_of};
use crate::combat::Fighter;
use crate::input::Seat;
use crate::motion::MotionState;
use crate::probe::{Checks, check};
use crate::snapshot::{SimId, Snapshot};
use crate::{Guests, WorldConfig};

mod flight;
mod gate;
mod holds;
mod inert;
mod kit;

/// The checks (`kjk sim probes`; see each).
pub fn probes() -> Checks {
    let mut checks = vec![
        ("droid_for_seatless_guest", seatless()),
        ("no_droid_solo", solo()),
        ("docks_when_companion_frees", docks()),
        ("launches_where_body_was", launches()),
        ("not_saved", not_saved()),
        ("minigame_no_droid", minigame()),
        ("phantom_never_drives", phantom()),
    ];
    checks.extend(flight::probes());
    checks.extend(inert::probes());
    checks.extend(gate::probes());
    checks.extend(kit::probes());
    checks.extend(holds::probes());
    checks
}

/// `seats` on the proving ground, no party: the guests have no body.
fn bare(seats: u8) -> Result<SimWorld, String> {
    bare_on(seats, crate::determinism::proving_ground())
}

fn bare_on(seats: u8, level: crate::Level) -> Result<SimWorld, String> {
    let config = WorldConfig {
        seats,
        guests: Guests::Companions,
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level).map_err(|why| why.to_string())?;
    settle(&mut world);
    Ok(world)
}

/// `party_world`, the fade its module loads under lifted.
fn companions(seats: u8) -> Result<(SimWorld, crate::Level, [SimId; 2]), String> {
    let (mut world, level, ids) = party_world(Guests::Companions, seats)?;
    settle(&mut world);
    Ok((world, level, ids))
}

/// Steps `world`, seat 0 alone standing, until the fade a module loads
/// under no longer covers the screen: the droids wait it out
/// (`remote::holds`).
pub(super) fn settle(world: &mut SimWorld) {
    for _ in 0..600 {
        if !run(world, &[(0, 0)]).fade.is_some_and(|fade| fade.out) {
            return;
        }
    }
}

/// `id` follows seat 0 (in the party) or no one (out of it).
fn follows(leader: Option<Seat>) -> impl Fn(&mut Fighter, &mut MotionState) {
    move |fighter: &mut Fighter, _: &mut MotionState| fighter.mind.leader = leader
}

fn droids(snapshot: &Snapshot) -> Vec<u8> {
    snapshot
        .remotes
        .iter()
        .map(|remote| remote.seat.0)
        .collect()
}

/// A guest with no party member to drive flies a droid, and has no body.
fn seatless() -> Result<String, String> {
    let mut world = bare(2)?;
    let after = run(&mut world, &[(0, 0), (1, 0)]);
    check(
        after.remote(Seat(1)).is_some() && after.body(Seat(1)).is_none(),
        format!(
            "droids {:?}, seat 1's body {:?}",
            droids(&after),
            after.body(Seat(1)).map(|e| e.id)
        ),
    )
}

/// One seat alone never makes a droid, nor seat 0 ever.
fn solo() -> Result<String, String> {
    let mut world = bare(1)?;
    let mut seen = Vec::new();
    for _ in 0..600 {
        seen.extend(droids(&run(&mut world, &[(0, 0)])));
    }
    check(seen.is_empty(), format!("droids over 600 steps {seen:?}"))
}

/// Seat 2 flies while the second companion is out of the party; it joins,
/// and seat 2 drives it that step, its droid docked.
fn docks() -> Result<String, String> {
    let (mut world, _, [_, b]) = companions(3)?;
    poke(&mut world, b, follows(None));
    scripts_of(&mut world).state.party.leave(1);
    let flying = run(&mut world, &[(0, 0), (1, 0), (2, 0)]);
    poke(&mut world, b, follows(Some(Seat(0))));
    if !scripts_of(&mut world).state.party.join(1, b.0) {
        return Err("the second companion did not join again".into());
    }
    let after = run(&mut world, &[(0, 0), (1, 0), (2, 0)]);
    let body = after.body(Seat(2)).map(|entity| entity.id);
    check(
        droids(&flying) == [2] && body == Some(b) && after.remote(Seat(2)).is_none(),
        format!(
            "droids {:?} then {:?}, seat 2's body {body:?} (the second {b:?})",
            droids(&flying),
            droids(&after)
        ),
    )
}

/// Seat 2 drives the second companion, which leaves the party: its droid
/// launches 1.5 m above where the companion stood.
fn launches() -> Result<String, String> {
    let (mut world, _, [_, b]) = companions(3)?;
    let driving = run(&mut world, &[(0, 0), (1, 0), (2, 0)]);
    if driving.body(Seat(2)).map(|entity| entity.id) != Some(b) {
        return Err("seat 2 does not drive the second companion".into());
    }
    let stood = position(&driving, b)?;
    poke(&mut world, b, follows(None));
    scripts_of(&mut world).state.party.leave(1);
    let after = run(&mut world, &[(0, 0), (1, 0), (2, 0)]);
    let at = after.remote(Seat(2)).map(|remote| remote.at);
    let near = at.is_some_and(|at| {
        let wanted = [stood[0], stood[1], stood[2] + 1.5];
        (0..3)
            .map(|i| (at[i] - wanted[i]).powi(2))
            .sum::<f32>()
            .sqrt()
            <= 1.0
    });
    check(
        near,
        format!("the droid at {at:?}, the companion stood at {stood:?}"),
    )
}

/// A save holds no droid, encoded or loaded; the loaded world makes the
/// guest's again on its first step.
fn not_saved() -> Result<String, String> {
    let mut world = bare(2)?;
    let flying = run(&mut world, &[(0, 0), (1, 0)]);
    let level = crate::determinism::proving_ground();
    let save = crate::Save {
        hz: crate::TICK_HZ,
        level: level.id(),
        snapshot: flying.clone(),
        script: world.script_state().clone(),
    };
    let decoded = crate::Save::decode(&save.encode()).map_err(|why| why.to_string())?;
    let mut loaded = save.world(&level, 2, None)?;
    let at_load = loaded.snapshot();
    let after = run(&mut loaded, &[(0, 0), (1, 0)]);
    check(
        !flying.remotes.is_empty()
            && decoded.snapshot.remotes.is_empty()
            && at_load.remotes.is_empty()
            && droids(&after) == [1],
        format!(
            "flying {:?}, saved {:?}, loaded {:?}, a step on {:?}",
            droids(&flying),
            droids(&decoded.snapshot),
            droids(&at_load),
            droids(&after)
        ),
    )
}

/// In a minigame each seat has its own run or gun: no droid.
fn minigame() -> Result<String, String> {
    let rules = crate::minigame::probes::race();
    let level = crate::determinism::proving_ground().with_minigame(rules.clone());
    let mut start = bare_on(2, level.clone())?.snapshot();
    start.minigame = Some(crate::minigame::Course::new(&rules));
    let mut world = SimWorld::from_snapshot(&start, level).map_err(|why| why.to_string())?;
    let after = run(&mut world, &[(0, 0), (1, 0)]);
    check(
        after.remotes.is_empty(),
        format!("droids {:?}", droids(&after)),
    )
}

fn position(snapshot: &Snapshot, id: SimId) -> Result<[f32; 3], String> {
    snapshot
        .get(id)
        .map(|entity| entity.position)
        .ok_or(format!("no {id:?}"))
}

/// A phantom seat (`SimWorld::set_phantoms`: the chain's chaos droids)
/// flies a droid with both companions free, and seat 1 still takes one.
fn phantom() -> Result<String, String> {
    let (mut world, _, ids) = companions(3)?;
    world.set_phantoms(1 << 2);
    let after = run(&mut world, &[(0, 0), (1, 0), (2, 0)]);
    let one = after.body(Seat(1)).map(|entity| entity.id);
    check(
        one.is_some_and(|id| ids.contains(&id))
            && after.body(Seat(2)).is_none()
            && droids(&after) == [2],
        format!(
            "seat 1 drives {one:?}, seat 2 {:?}, droids {:?}",
            after.body(Seat(2)).map(|entity| entity.id),
            droids(&after)
        ),
    )
}
