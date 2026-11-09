//! The rewind job's evidence (`rebase`, issue 229), run by `kjk net
//! probes` ("rewinds"): on the proving ground, a player firing its pistol
//! (its bolts in flight), a rewind is made from a snapshot of the
//! authority's and compared with stepping the same world directly:
//! - made with a whole snapshot for every tick, without one for any, and
//!   from ticks in between: this seat's body and the bolts at every tick,
//!   and the world it ends in (its snapshot's hash), are the same each
//!   way, and the same as stepping it directly;
//! - made on the rewinder's thread, job after job: the same as in the
//!   caller, each with its generation;
//! - a snapshot that does not restore in the job's level fails the job,
//!   in the caller and on the thread, and the thread goes on;
//! - the live world's ticks stepped beside the frames (`Beside`), a few
//!   at a time on its thread and in the caller: the same snapshots as
//!   stepping it directly, landing once each, and nothing with none sent.

use std::sync::Arc;

use sim::determinism::{self as pinned};
use sim::{
    Command, Level, Seat, SimWorld, Snapshot, StepReason, TickInput, TickRate, WorldConfig,
    buttons, step,
};

use super::rebase::{Beside, Bolts, Done, Job, RebaseOn, Rebaser, Steps, run};
use crate::probes::Probes;

/// The most ticks stepped to find a base with bolts in flight, and the
/// ticks rewound.
const BEFORE: u64 = 400;
const DEPTH: u64 = 24;

/// The player's command: fire, facing +X.
fn firing() -> Command {
    Command {
        angles: sim::moves::view_angles(2.0, 0.0),
        buttons: buttons::ATTACK,
        ..Command::default()
    }
}

/// The authority's world stepped to the first tick past `DEPTH` with bolts
/// in flight (`BEFORE` at the most), and its snapshot there.
fn authority() -> (Level, Snapshot) {
    let level = pinned::proving_ground();
    let mut world = SimWorld::new(WorldConfig::one_seat(), level.clone())
        .unwrap_or_else(|error| panic!("the proving ground: {error}"));
    let mut last = world.snapshot();
    for tick in 1..=BEFORE {
        let mut input = TickInput::default();
        input.insert(Seat(0), firing());
        last = step(
            &mut world,
            tick,
            &input,
            TickRate::default().msec(tick),
            StepReason::Authority,
        );
        let bolts = Bolts::of(&last);
        if bolts.players + bolts.creatures > 0 && tick > DEPTH {
            break;
        }
    }
    (level, last)
}

/// A rewind of `DEPTH` ticks from `base` for seat 0, still firing, whole
/// snapshots from `full_from` on.
fn job(base: &Arc<Snapshot>, level: &Level, full_from: u64, generation: u64) -> Job {
    let seat = Seat(0);
    let rate = TickRate::default();
    let others = TickInput::default();
    let worked = SimWorld::from_snapshot(base, level.clone())
        .map(|world| world.worked_out())
        .unwrap_or_else(|error| panic!("the base: {error}"));
    Job {
        generation,
        base: base.clone(),
        level: level.clone(),
        worked: Some(worked),
        seat,
        own_id: base.body(seat).map(|entity| entity.id),
        others,
        own: (base.tick + 1..=base.tick + DEPTH)
            .map(|tick| (tick, firing(), 40))
            .collect(),
        to: base.tick + DEPTH,
        rate,
        full_from,
    }
}

/// The same ticks stepped directly: this seat's body and bolts at each,
/// and the world's hash at the end.
fn direct(job: &Job) -> (Vec<(u64, sim::EntityState, Bolts)>, sim::SnapshotHash) {
    let mut world = SimWorld::from_snapshot(&job.base, job.level.clone())
        .unwrap_or_else(|error| panic!("the base: {error}"));
    let mut history = Vec::new();
    let mut last = (*job.base).clone();
    for tick in job.base.tick + 1..=job.to {
        let own = job
            .own
            .iter()
            .find(|(at, ..)| *at == tick)
            .map(|(_, command, lag)| (*command, *lag));
        let input = super::rebase::tick_input(&job.others, job.seat, own);
        last = step(
            &mut world,
            tick,
            &input,
            job.rate.msec(tick),
            StepReason::Predict,
        );
        if let Some(entity) = job.own_id.and_then(|id| last.get(id)) {
            history.push((tick, *entity, Bolts::of(&last)));
        }
    }
    (history, last.hash())
}

/// What a rewind left: this seat's body and bolts, the world's hash, and
/// the ticks it made whole.
fn made(
    done: &mut Done,
) -> (
    Vec<(u64, sim::EntityState, Bolts)>,
    sim::SnapshotHash,
    Vec<u64>,
) {
    let hash = done.world.snapshot().hash();
    let whole = done
        .snapshots
        .iter()
        .map(|snapshot| snapshot.tick)
        .collect();
    (done.history.clone(), hash, whole)
}

pub fn probes() -> Probes {
    let mut probes = Probes::default();
    let (level, base) = authority();
    let base = Arc::new(base);
    let bolts = Bolts::of(&base);
    probes.check(
        "rewind base",
        "the base has seat 0's body and its bolts in flight",
        base.body(Seat(0)).is_some() && bolts.players + bolts.creatures > 0,
        format!(
            "tick {}: seat 0's body {:?}, bolts in flight {} (players') and {} (others')",
            base.tick,
            base.body(Seat(0)).map(|entity| entity.id),
            bolts.players,
            bolts.creatures
        ),
    );
    let reference = job(&base, &level, 0, 1);
    let (direct_history, direct_hash) = direct(&reference);
    let mut results = Vec::new();
    for (name, full_from) in [
        ("every tick whole", 0),
        ("the last two whole", base.tick + DEPTH - 1),
        ("from the middle on whole", base.tick + DEPTH / 2),
        ("none whole", u64::MAX),
    ] {
        let done = run(job(&base, &level, full_from, 1));
        let Some(mut done) = done else {
            probes.check(
                &format!("rewind: {name}"),
                "the job is made",
                false,
                "the base did not restore".into(),
            );
            continue;
        };
        let (history, hash, whole) = made(&mut done);
        let expected_whole: Vec<u64> = (base.tick + 1..=base.tick + DEPTH)
            .filter(|tick| *tick >= full_from)
            .collect();
        probes.check(
            &format!("rewind: {name}"),
            "this seat's body and the bolts at every tick, and the world it ends in, as when stepped directly; whole snapshots for the ticks asked",
            history == direct_history && hash == direct_hash && whole == expected_whole,
            format!(
                "{} ticks of history ({} directly), the same: {}; the world {} ({} directly); {} whole (asked {})",
                history.len(),
                direct_history.len(),
                history == direct_history,
                hash.short(),
                direct_hash.short(),
                whole.len(),
                expected_whole.len()
            ),
        );
        results.push(hash);
    }

    // On the thread: the same, job after job.
    let mut rebaser = Rebaser::new(RebaseOn::Thread);
    let mut same = 0;
    let rounds = 4;
    for round in 0..rounds {
        rebaser.start(job(&base, &level, base.tick + 2 + round, 10 + round));
        let result = rebaser.wait();
        if let Some(Some(mut done)) = result {
            let (history, hash, _) = made(&mut done);
            if done.generation == 10 + round && history == direct_history && hash == direct_hash {
                same += 1;
            }
        }
    }
    probes.check(
        "rewind on its thread",
        "each job made on the rewinder's thread is the same as in the caller, its generation kept",
        same == rounds && !rebaser.lost,
        format!(
            "{same} of {rounds} the same; the thread lost: {}",
            rebaser.lost
        ),
    );

    // A snapshot that does not restore in the job's level fails the job,
    // on the thread too, and the thread goes on.
    let mut wrong = job(&base, &level, 0, 20);
    wrong.level = Level::arena();
    let caller = run(wrong);
    let mut wrong = job(&base, &level, 0, 21);
    wrong.level = Level::arena();
    rebaser.start(wrong);
    let threaded = rebaser.wait();
    rebaser.start(job(&base, &level, 0, 22));
    let after = rebaser.wait();
    let after_same = matches!(&after, Some(Some(done)) if done.generation == 22);
    probes.check(
        "rewind of a snapshot of another level",
        "fails in the caller and on the thread (no world), and the thread makes the next job",
        caller.is_none() && matches!(threaded, Some(None)) && after_same && !rebaser.lost,
        format!(
            "in the caller: failed {}; on the thread: failed {}; the next job made: {after_same}; the thread lost: {}",
            caller.is_none(),
            matches!(threaded, Some(None)),
            rebaser.lost
        ),
    );
    probes.extend(beside(&base, &level));
    probes.check(
        "rewinds agree",
        "every way of making the rewind ends in one world",
        results.windows(2).all(|pair| pair[0] == pair[1]),
        format!("{} ways", results.len()),
    );
    probes
}

/// The live world's ticks beside the frames, on the thread and in the
/// caller, against stepping them directly.
fn beside(base: &Arc<Snapshot>, level: &Level) -> Probes {
    let mut probes = Probes::default();
    let ticks: Vec<(u64, TickInput, u32)> = (base.tick + 1..=base.tick + DEPTH)
        .map(|tick| {
            let mut input = TickInput::default();
            input.insert(Seat(0), firing());
            (tick, input, TickRate::default().msec(tick))
        })
        .collect();
    let world = || {
        Box::new(
            SimWorld::from_snapshot(base, level.clone())
                .unwrap_or_else(|error| panic!("the base: {error}")),
        )
    };
    let mut direct = world();
    let expected: Vec<sim::SnapshotHash> = ticks
        .iter()
        .map(|(tick, input, msec)| {
            step(&mut direct, *tick, input, *msec, StepReason::Predict).hash()
        })
        .collect();
    for on in [RebaseOn::Caller, RebaseOn::Thread] {
        let mut beside = Beside::new(on);
        let nothing = beside.land().is_none();
        let mut world = Some(world());
        let mut hashes = Vec::new();
        let mut landings = 0;
        // Three ticks a frame, the last frame one.
        for chunk in ticks.chunks(3) {
            beside.send(Steps {
                generation: 7,
                world: world.take().expect("home"),
                ticks: chunk.to_vec(),
            });
            let away = beside.away();
            if let Some(stepped) = beside.land() {
                landings += usize::from(away && stepped.generation == 7);
                hashes.extend(stepped.snapshots.iter().map(Snapshot::hash));
                world = Some(stepped.world);
            }
        }
        let again = beside.land().is_none();
        probes.check(
            &format!("live steps beside the frames ({on:?})"),
            "every tick's snapshot as stepped directly, each job landing once, nothing to land with none away",
            hashes == expected
                && landings == ticks.chunks(3).count()
                && nothing
                && again
                && !beside.lost,
            format!(
                "{} of {} ticks the same, {landings} landings of {} jobs; nothing before: {nothing}, after: {again}; the thread lost: {}",
                hashes.iter().zip(&expected).filter(|(a, b)| a == b).count(),
                expected.len(),
                ticks.chunks(3).count(),
                beside.lost
            ),
        );
    }
    probes
}
