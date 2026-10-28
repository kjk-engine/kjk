//! Encounter scaling counts the players driving a body (I30 P21). A
//! module is planned for the host and each guest who drives a party member
//! at that moment (`Snapshot::players_driving`), and keeps that count for
//! good (`Place::players`). Coming back to it, installing it again from its
//! own link, or loading a save made in it plans it for as many again
//! (`Snapshot::players_for`).
//!
//! Probes on the proving ground under made-up module names. A guest drives
//! a companion as the skeleton makes one: the companion follows seat 0 and
//! names its driver (`Mind::driver`), and the world is rebuilt from that
//! snapshot. A watching guest sends commands and has no body. For
//! `kjk sim probes`.

use crate::combat::Sheet;
use crate::combat::ai::Mind;
use crate::combat::fighter::Life;
use crate::module::{ObjectData, ObjectKind, Placement};
use crate::probe::{Checks, check};
use crate::{
    Command, Guests, InstallError, Kind, Level, Seat, SimWorld, Snapshot, StepReason, TickInput,
    WorldConfig,
};

/// The companions' tag (numbered) and template.
const MATE: &str = "probe_mate";

/// A party to count: its avatars (seats from 0 with a body of their own),
/// its companions, the seats driving them (in order), and the seats
/// sending commands.
struct Party {
    avatars: u8,
    mates: usize,
    drivers: &'static [u8],
    sending: &'static [u8],
}

/// (name, the party, the players it counts).
const COUNTS: [(&str, Party, u8); 6] = [
    ("host_alone", Party::host(1, &[], &[0]), 1),
    ("driving_guest", Party::host(1, &[1], &[0, 1]), 2),
    ("watching_guest", Party::host(0, &[], &[0, 1]), 1),
    ("third_seat_watches", Party::host(1, &[1], &[0, 1, 2]), 2),
    ("two_guests_driving", Party::host(2, &[1, 2], &[0, 1, 2]), 3),
    (
        "avatars_each_count",
        Party {
            avatars: 3,
            mates: 0,
            drivers: &[],
            sending: &[0, 1, 2],
        },
        3,
    ),
];

impl Party {
    /// The host's avatar, `mates` companions, `drivers` driving them.
    const fn host(mates: usize, drivers: &'static [u8], sending: &'static [u8]) -> Party {
        Party {
            avatars: 1,
            mates,
            drivers,
            sending,
        }
    }
}

pub fn probes() -> Checks {
    let mut list: Vec<_> = COUNTS
        .iter()
        .map(|(name, party, expect)| (*name, count(party, *expect)))
        .collect();
    list.push(("dead_body_not_counted", dead_body()));
    list.extend(kept().unwrap_or_else(|why| vec![("kept_scenario", Err(why))]));
    list.push(("module_keeps_its_queues", queues_kept()));
    list
}

/// A module keeps what its scripts left waiting (KOTOR's stands still while
/// the party is away): a queue and a lock on `probe_a`'s gate go with it to
/// the scripts' `left` when the party goes to `probe_b`, and are the gate's
/// again on coming back, the queue's start moved on by the time away.
fn queues_kept() -> Result<String, String> {
    use crate::script::actions::{Action, Queued};
    let error = |error: InstallError| error.to_string();
    let party = Party::host(0, &[], &[0]);
    let mut world = party_world(&ground_as("probe_a", 1, 0)?, &party)?;
    let start = drive(&mut world, party.sending);
    let gate = start
        .entities
        .iter()
        .find(|entity| matches!(entity.kind, Kind::Object(..)))
        .map(|entity| entity.id.0)
        .ok_or("probe_a has no object")?;
    let begun = start.time_ms;
    let mut state = world.script_state().clone();
    let queue = vec![Queued {
        action: Action::Wait { ms: 60_000 },
        started_ms: Some(begun),
    }];
    state.actions.insert(gate, queue);
    state.uncommandable.insert(gate);
    world.set_script_state(state);
    world
        .install(ground_as("probe_b", 1, 0)?, None)
        .map_err(error)?;
    let away = world.script_state();
    let stashed = away
        .left
        .get("probe_a")
        .is_some_and(|left| left.actions.contains_key(&gate) && left.uncommandable.contains(&gate));
    let gone_here = !away.actions.contains_key(&gate) && !away.uncommandable.contains(&gate);
    for _ in 0..5 {
        drive(&mut world, party.sending);
    }
    let left_at = away_since(&world, "probe_a");
    let back = world
        .install(ground_as("probe_a", 1, 0)?, None)
        .map_err(error)?;
    let state = world.script_state();
    let started = state
        .actions
        .get(&gate)
        .and_then(|queue| queue.first())
        .and_then(|queued| queued.started_ms);
    let moved = left_at.map(|at| begun + (back.time_ms - at));
    check(
        stashed
            && gone_here
            && started.is_some()
            && started == moved
            && state.uncommandable.contains(&gate)
            && !state.left.contains_key("probe_a"),
        format!(
            "stashed {stashed}, gone from probe_b {gone_here}; back: the queue started at {started:?} \
             (was {begun}, {moved:?} expected), locked {}",
            state.uncommandable.contains(&gate)
        ),
    )
}

/// When the party left `module`, if it waits in the scripts' `left`.
fn away_since(world: &SimWorld, module: &str) -> Option<u64> {
    world.script_state().left.get(module).map(|left| left.at_ms)
}

/// The proving ground as module `name`, planned for `players`, with
/// `mates` creatures behind its entry. Its library has their template, so
/// they follow the party out and back (`install::companions`).
fn ground_as(name: &str, players: u8, mates: usize) -> Result<Level, String> {
    let ground = crate::determinism::proving_ground();
    let mut data = (**ground.module().ok_or("the ground has no module")?).clone();
    data.name = name.into();
    data.players = players;
    let [x, y, z] = data.entry.position;
    let mate = |n: usize| {
        let at = [x - 1.5 * (n + 1) as f32, y, z];
        ObjectData::creature(&format!("{MATE}{n}"), MATE, at, Sheet::dev_soldier())
    };
    data.placements
        .extend((0..mates).map(|n| Placement::Own(mate(n))));
    let library =
        crate::script::Library::new(Default::default(), Default::default(), Default::default())
            .with_creatures([(MATE.to_string(), std::sync::Arc::new(mate(0)))].into());
    Ok(ground.with_module(data).with_scripts(library))
}

/// `party` in `level`: its `MATE` creatures follow seat 0, the first ones
/// driven by `party.drivers` in order. The session holds every seat that
/// sends; where those are more than the avatars, guests play companions.
fn party_world(level: &Level, party: &Party) -> Result<SimWorld, String> {
    let held = party
        .sending
        .iter()
        .map(|&seat| seat + 1)
        .fold(party.avatars, u8::max);
    let config = WorldConfig {
        seats: held,
        guests: if held == party.avatars {
            Guests::Avatars
        } else {
            Guests::Companions
        },
        ..WorldConfig::one_seat()
    };
    let mut world = SimWorld::new(config, level.clone()).map_err(|error| error.to_string())?;
    let mut start = world.snapshot();
    let module = level.module().ok_or("no module")?;
    let mut drivers = party.drivers.iter();
    for entity in &mut start.entities {
        let Kind::Object(ObjectKind::Creature, index) = entity.kind else {
            continue;
        };
        let mate = module
            .object(index as usize)
            .is_some_and(|object| object.tag.starts_with(MATE));
        let Some(fighter) = entity.fighter.as_mut().filter(|_| mate) else {
            continue;
        };
        fighter.mind = Mind {
            driver: drivers.next().map(|&seat| Seat(seat)),
            ..Mind::companion(Seat(0))
        };
    }
    SimWorld::from_snapshot(&start, level.clone()).map_err(|error| error.to_string())
}

/// Two ticks with a command from each of `seats`.
fn drive(world: &mut SimWorld, seats: &[u8]) -> Snapshot {
    let from = world.snapshot().tick;
    let mut input = TickInput::default();
    for &seat in seats {
        input.insert(Seat(seat), Command::default());
    }
    crate::step(world, from + 1, &input, 8, StepReason::Authority);
    crate::step(world, from + 2, &input, 8, StepReason::Authority)
}

fn count(party: &Party, expect: u8) -> Result<String, String> {
    let mut world = party_world(&ground_as("probe_a", 1, party.mates)?, party)?;
    let after = drive(&mut world, party.sending);
    let players = after.players_driving();
    check(
        players == expect,
        format!(
            "{} avatar(s), {} companion(s) driven by {:?}, {:?} sending: {players} player(s), \
             expected {expect}",
            party.avatars, party.mates, party.drivers, party.sending
        ),
    )
}

/// A guest's companion dies with its driver still set: it counts nothing.
fn dead_body() -> Result<String, String> {
    let party = Party::host(1, &[1], &[0, 1]);
    let mut world = party_world(&ground_as("probe_a", 1, party.mates)?, &party)?;
    let mut after = drive(&mut world, party.sending);
    let body = after
        .body(Seat(1))
        .map(|body| body.id)
        .ok_or("seat 1 drives nothing")?;
    let fighter = after
        .entities
        .iter_mut()
        .find(|entity| entity.id == body)
        .and_then(|entity| entity.fighter.as_mut())
        .ok_or("the body does not fight")?;
    fighter.life = Life::Dead;
    let players = after.players_driving();
    check(
        players == 1,
        format!("seat 1's body dead: {players} player(s)"),
    )
}

/// Planned for one in `probe_a`; a guest drives the companion; `probe_b` is
/// planned for two. Back in `probe_a` it is one again; a plan for two
/// would not install (the stashed place differs). `probe_a` installed from
/// itself while two drive keeps one. The counts survive the encoding.
fn kept() -> Result<Checks, String> {
    let error = |error: InstallError| error.to_string();
    let party = Party::host(1, &[1], &[0, 1]);
    let mut world = party_world(&ground_as("probe_a", 1, party.mates)?, &party)?;
    let driving = drive(&mut world, party.sending);
    let for_b = driving.players_for("probe_b");
    world
        .install(ground_as("probe_b", for_b, 0)?, None)
        .map_err(error)?;
    let in_b = drive(&mut world, party.sending);
    let for_a = in_b.players_for("probe_a");
    let recounted = world.install(ground_as("probe_a", 2, 1)?, None).err();
    world
        .install(ground_as("probe_a", for_a, 1)?, None)
        .map_err(error)?;
    let here = drive(&mut world, party.sending);
    let again = here.players_for("probe_a");
    let same = world
        .install(ground_as("probe_a", again, 1)?, None)
        .map_err(error);
    let decoded = Snapshot::decode(&here.encode()).map_err(|error| error.to_string())?;
    let counts = |snapshot: &Snapshot| -> Vec<(String, u8)> {
        let stashed = snapshot.stash.iter().map(|stashed| &stashed.place);
        let places = snapshot.place.iter().chain(stashed);
        places
            .map(|place| (place.module.clone(), place.players))
            .collect()
    };
    Ok(vec![
        (
            "new_module_counts_drivers",
            check(
                for_b == 2,
                format!("probe_b, never planned, with a guest driving: {for_b}"),
            ),
        ),
        (
            "stash_keeps_count",
            check(
                for_a == 1,
                format!("probe_a, left planned for one: {for_a}"),
            ),
        ),
        (
            "recount_strands",
            check(
                matches!(recounted, Some(InstallError::StashChanged { .. })),
                format!("probe_a planned for two instead: {recounted:?}"),
            ),
        ),
        (
            "same_module_again",
            check(
                here.players_driving() == 2 && again == 1 && same.is_ok(),
                format!(
                    "{} driving in probe_a, planned again for {again}: {:?}",
                    here.players_driving(),
                    same.map(|after| after.tick)
                ),
            ),
        ),
        (
            "place_encodes_players",
            check(
                counts(&decoded) == counts(&here) && counts(&here).len() == 2,
                format!("{:?}", counts(&decoded)),
            ),
        ),
    ])
}
