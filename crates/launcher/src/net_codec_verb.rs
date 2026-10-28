//! `kjk net codec`: the snapshot codec (`net::codec`) over recorded runs.
//! The pinned determinism scenario with 1, 2, 4 and 8 seats and, when
//! KOTOR is installed, the `lifecycle-walk` from `end_m01aa` through
//! `end_m01ab` and back with as many seats (every seat walking it). For
//! each: every snapshot rebuilt from every baseline up to 64 back and from
//! none, which must all be exact; frame sizes by baseline distance; a
//! host's and a client's rings over a lossy link; the codec's time per
//! frame. Then seeded corruptions of valid frames through the decoder,
//! which must never panic. Writes `facts.json`.

use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Instant;

use net::codec::probes::{self, Exactness, Mutations, Probe, Recorded, Session, Sizes};
use net::codec::{self, FrameHeader};
use sim::moves::Script;
use sim::motion::AnimTable;
use sim::{Seat, StepReason, TickInput, TickRate, step};
use vfs::Game;

use crate::stage::Stage;

const SEATS: [u8; 4] = [1, 2, 4, 8];
/// Baselines tried behind every snapshot.
const BACK: usize = codec::CLIENT_RING;
const MUTATIONS: usize = 200_000;
/// The session's link: frames lost, and snapshots until a frame (and a
/// confirmation) arrives: 6 ticks each way, a 96 ms round trip.
const LOSS_PERCENT: u32 = 5;
const DELAY: usize = 6;
/// The frame distance the timing reports besides keyframes: every second
/// tick, the default send rate.
const TIMED_BACK: usize = 2;

/// Everything measured on one run.
struct Measured {
    run: Recorded,
    exactness: Exactness,
    session: Session,
    timing: Timing,
}

/// Mean microseconds per frame.
#[derive(Default)]
struct Timing {
    encode: f64,
    decode: f64,
    rebuild: f64,
    keyframe_encode: f64,
    keyframe_rebuild: f64,
}

pub fn codec(discovery: &vfs::Discovery) -> ExitCode {
    let mut failed = 0;
    outln!("synthetic:");
    let synthetic = probes::synthetic();
    for probe in &synthetic.0 {
        print(probe);
    }
    failed += synthetic.failed();

    let started = Instant::now();
    let mut runs: Vec<Recorded> = SEATS
        .iter()
        .map(|&seats| probes::determinism_run(seats, sim::determinism::PINNED_TICKS))
        .collect();
    match kotor_runs(discovery) {
        Ok(Some(walks)) => runs.extend(walks),
        Ok(None) => outln!("KOTOR not found: its module walk is skipped"),
        Err(error) => {
            eprintln!("net codec: the KOTOR walk: {error}");
            failed += 1;
        }
    }
    outln!(
        "recorded {} runs in {:.1} s",
        runs.len(),
        started.elapsed().as_secs_f64()
    );

    // Exactness, the session and the corruptions are pure and long: one
    // thread per run. Timing runs afterwards, alone.
    let started = Instant::now();
    let checked: Vec<(Recorded, Exactness, Session, Option<Mutations>)> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = runs
                .into_iter()
                .map(|run| {
                    scope.spawn(move || {
                        let exactness = probes::exactness(&run, BACK);
                        let session = probes::session(&run, LOSS_PERCENT, DELAY, 1);
                        let mutations = (run.seats == 4)
                            .then(|| probes::mutations(&run, MUTATIONS, u64::from(run.seats)));
                        (run, exactness, session, mutations)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().expect("a probe thread finishes"))
                .collect()
        });
    outln!(
        "checked in {:.1} s ({} threads)",
        started.elapsed().as_secs_f64(),
        checked.len()
    );

    let mut measured = Vec::new();
    let mut corrupted = Vec::new();
    for (run, exactness, session, mutations) in checked {
        let timing = time(&run);
        if let Some(mutations) = mutations {
            corrupted.push((run.name.clone(), mutations));
        }
        measured.push(Measured {
            run,
            exactness,
            session,
            timing,
        });
    }
    for one in &measured {
        failed += report(one);
    }
    table(&measured);
    for (name, mutations) in &corrupted {
        failed += report_mutations(name, mutations);
    }

    crate::runtime::write_facts(serde_json::json!({
        "verb": "net codec",
        "back": BACK,
        "synthetic": synthetic.0.iter().map(|probe| serde_json::json!({
            "name": probe.name, "found": probe.found, "pass": probe.pass,
        })).collect::<Vec<_>>(),
        "runs": measured.iter().map(facts).collect::<Vec<_>>(),
        "mutations": corrupted.iter().map(|(name, m)| serde_json::json!({
            "run": name, "tried": m.tried, "panics": m.panics, "accepted": m.accepted,
            "accepted_wrong": m.accepted_wrong, "errors": m.errors,
        })).collect::<Vec<_>>(),
        "failed": failed,
    }));
    if failed == 0 {
        outln!("net codec: all passed");
        ExitCode::SUCCESS
    } else {
        outln!("net codec: {failed} FAILED");
        ExitCode::FAILURE
    }
}

fn print(probe: &Probe) {
    let mark = if probe.pass { "ok  " } else { "FAIL" };
    outln!(
        "  {mark} {}: {} ({})",
        probe.name,
        probe.found,
        probe.expect
    );
}

/// The `lifecycle-walk` from `end_m01aa` (arriving at `wp_player01`) with
/// each seat count, every seat walking it; `None` without KOTOR.
fn kotor_runs(discovery: &vfs::Discovery) -> Result<Option<Vec<Recorded>>, String> {
    if discovery.get(Game::Kotor1).is_none() {
        return Ok(None);
    }
    let (anims, note) = crate::stage::anims(discovery);
    outln!("KOTOR walk: {note}");
    let anims = Arc::new(anims);
    SEATS
        .iter()
        .map(|&seats| kotor_run(discovery, anims.clone(), seats))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn kotor_run(
    discovery: &vfs::Discovery,
    anims: Arc<AnimTable>,
    seats: u8,
) -> Result<Recorded, String> {
    let stage = Stage::named(
        discovery,
        "end_m01aa",
        Some("wp_player01"),
        anims,
        false,
        seats,
        1,
    )?;
    let installer = stage
        .installer
        .clone()
        .ok_or("end_m01aa has no installer")?;
    let script = Script::parse("lifecycle-walk")?;
    let (mut world, _) = stage.world(1)?;
    let rate = TickRate::default();
    let mut recorded = Recorded::new(
        format!("end_m01aa walk, {}", probes::seats_label(seats)),
        seats,
    );
    let mut epoch = 0;
    let mut last = world.snapshot();
    recorded.push(epoch, &last);
    let end_ms = script.length_ms() + 500;
    while rate.time_ms(last.tick) < end_ms {
        let tick = last.tick + 1;
        let command = script.command(rate.time_ms(tick - 1));
        let mut input = TickInput::default();
        for seat in 0..seats {
            input.insert(Seat(seat), command);
        }
        last = step(
            &mut world,
            tick,
            &input,
            rate.msec(tick),
            StepReason::Authority,
        );
        recorded.push(epoch, &last);
        if let Some(transition) = &last.transition {
            let request = installer.request_from(&last, &transition.module, Some(&transition.tag));
            installer
                .prepare(&request, false)?
                .plan
                .commit(&mut world)
                .map_err(|error| error.to_string())?;
            epoch += 1;
            last = world.snapshot();
            recorded.push(epoch, &last);
        }
    }
    Ok(recorded)
}

/// Mean microseconds per frame over the run: encoding and decoding frames
/// `TIMED_BACK` behind (within an epoch) and keyframes; `rebuild` is the
/// decode plus the check and `Snapshot::decode`.
fn time(run: &Recorded) -> Timing {
    let shots = &run.shots;
    let pairs: Vec<(usize, Option<usize>)> = (TIMED_BACK..shots.len())
        .filter(|&at| shots[at - TIMED_BACK].epoch == shots[at].epoch)
        .map(|at| (at, Some(at - TIMED_BACK)))
        .collect();
    let keyframes: Vec<(usize, Option<usize>)> = (0..shots.len()).map(|at| (at, None)).collect();
    let (encode, decode, rebuild) = time_pairs(run, &pairs);
    let (keyframe_encode, _, keyframe_rebuild) = time_pairs(run, &keyframes);
    Timing {
        encode,
        decode,
        rebuild,
        keyframe_encode,
        keyframe_rebuild,
    }
}

fn time_pairs(run: &Recorded, pairs: &[(usize, Option<usize>)]) -> (f64, f64, f64) {
    let shots = &run.shots;
    let base = |at: Option<usize>| at.map(|at| &shots[at].stepped);
    let per = |started: Instant| started.elapsed().as_secs_f64() * 1e6 / pairs.len().max(1) as f64;

    let mut out = Vec::with_capacity(64 * 1024);
    let started = Instant::now();
    for &(at, from) in pairs {
        out.clear();
        codec::encode_frame(shots[at].epoch, &shots[at].stepped, base(from), &mut out);
        black_box(&out);
    }
    let encode = per(started);

    let frames: Vec<Vec<u8>> = pairs
        .iter()
        .map(|&(at, from)| {
            let mut frame = Vec::new();
            codec::encode_frame(shots[at].epoch, &shots[at].stepped, base(from), &mut frame);
            frame
        })
        .collect();
    let parts = |from: Option<usize>| base(from).map(net::codec::Stepped::parts);
    let started = Instant::now();
    for (frame, &(_, from)) in frames.iter().zip(pairs) {
        let (_, packed) = FrameHeader::read(frame).expect("a frame this run encoded");
        let delta = codec::unpack(packed).expect("a frame this run packed");
        black_box(codec::decode(parts(from), &delta).is_ok());
    }
    let decode = per(started);

    let started = Instant::now();
    for (frame, &(_, from)) in frames.iter().zip(pairs) {
        black_box(codec::decode_frame(frame, parts(from)).is_ok());
    }
    (encode, decode, per(started))
}

fn row(label: &str, sizes: &Sizes) {
    outln!(
        "    {label:<10} {:>9.1} {:>7} {:>7}",
        sizes.mean(),
        sizes.p95(),
        sizes.max()
    );
}

/// Prints one run's findings; the number of failures.
fn report(one: &Measured) -> usize {
    let (run, exact, session, timing) = (&one.run, &one.exactness, &one.session, &one.timing);
    let epochs = run.shots.last().map_or(0, |shot| shot.epoch) + 1;
    outln!(
        "{}: {} snapshots, {epochs} epochs, largest encoding {} B, ends {}",
        run.name,
        run.shots.len(),
        exact.encoding_max,
        run.end().map_or("-".to_string(), |hash| hash.short()),
    );
    let exact_ok = exact.exact == exact.total && exact.total > 0;
    outln!(
        "  {} rebuilt exactly: {} of {} ({} deltas across an install)",
        if exact_ok { "ok  " } else { "FAIL" },
        exact.exact,
        exact.total,
        exact.across
    );
    for failure in &exact.failures {
        outln!("       {failure}");
    }
    outln!("    frame B        mean     p95     max");
    row("keyframe", &exact.keyframes);
    for (distance, sizes) in &exact.by_distance {
        row(&format!("{distance} back"), sizes);
    }
    outln!(
        "    frames 1 back over a datagram (1181 B): {}",
        exact.over_datagram
    );
    outln!(
        "    µs per frame, {TIMED_BACK} back: encode {:.2}, decode {:.2}, rebuild and check {:.2}; keyframe: encode {:.2}, rebuild and check {:.2}",
        timing.encode,
        timing.decode,
        timing.rebuild,
        timing.keyframe_encode,
        timing.keyframe_rebuild
    );
    let session_ok = session.wrong.is_empty() && session.rebuilt > 0;
    outln!(
        "  {} session ({LOSS_PERCENT}% loss, {DELAY} snapshots each way): {} sent, {} lost, {} rebuilt, {} refused (old epoch or stale), {} wrong, {} keyframes, {:.1} B a frame",
        if session_ok { "ok  " } else { "FAIL" },
        session.sent,
        session.lost,
        session.rebuilt,
        session.refused,
        session.wrong.len(),
        session.keyframes,
        session.bytes as f64 / session.sent.max(1) as f64,
    );
    for wrong in session.wrong.iter().take(5) {
        outln!("       {wrong}");
    }
    usize::from(!exact_ok) + usize::from(!session_ok)
}

/// The design's section 5 table, measured: frames every second tick.
fn table(measured: &[Measured]) {
    outln!("frames {TIMED_BACK} back (the default send rate), bytes:");
    outln!(
        "  {:<28} {:>7} {:>7} {:>7} {:>9} {:>9}",
        "run",
        "mean",
        "p95",
        "max",
        "keyframe",
        "encode µs"
    );
    for one in measured {
        let sizes = one.exactness.by_distance.get(&TIMED_BACK);
        outln!(
            "  {:<28} {:>7.1} {:>7} {:>7} {:>9.0} {:>9.2}",
            one.run.name,
            sizes.map_or(0.0, Sizes::mean),
            sizes.map_or(0, Sizes::p95),
            sizes.map_or(0, Sizes::max),
            one.exactness.keyframes.mean(),
            one.timing.encode,
        );
    }
}

fn report_mutations(name: &str, mutations: &Mutations) -> usize {
    let pass = mutations.panics == 0 && mutations.accepted_wrong == 0 && mutations.tried > 0;
    outln!(
        "  {} corrupt frames of {name}: {} tried, {} panics, {} accepted ({} not the original), refused by kind {:?}",
        if pass { "ok  " } else { "FAIL" },
        mutations.tried,
        mutations.panics,
        mutations.accepted,
        mutations.accepted_wrong,
        mutations.errors,
    );
    usize::from(!pass)
}

fn sizes_json(sizes: &Sizes) -> serde_json::Value {
    serde_json::json!({ "mean": sizes.mean(), "p95": sizes.p95(), "max": sizes.max() })
}

fn facts(one: &Measured) -> serde_json::Value {
    let (exact, session, timing) = (&one.exactness, &one.session, &one.timing);
    serde_json::json!({
        "run": one.run.name,
        "seats": one.run.seats,
        "snapshots": one.run.shots.len(),
        "end": one.run.end().map(|hash| hash.to_string()),
        "rebuilt": { "total": exact.total, "exact": exact.exact, "across_installs": exact.across },
        "encoding_max": exact.encoding_max,
        "keyframe": sizes_json(&exact.keyframes),
        "by_distance": exact.by_distance.iter()
            .map(|(distance, sizes)| (distance.to_string(), sizes_json(sizes)))
            .collect::<serde_json::Map<_, _>>(),
        "over_datagram": exact.over_datagram,
        "session": {
            "sent": session.sent, "lost": session.lost, "rebuilt": session.rebuilt,
            "refused": session.refused, "wrong": session.wrong.len(),
            "keyframes": session.keyframes, "bytes": session.bytes,
        },
        "micros": {
            "back": TIMED_BACK, "encode": timing.encode, "decode": timing.decode,
            "rebuild": timing.rebuild, "keyframe_encode": timing.keyframe_encode,
            "keyframe_rebuild": timing.keyframe_rebuild,
        },
    })
}
