//! The codec's evidence, as pure functions that return what they found
//! (`kjk net codec` prints it and times the codec on the same runs):
//! - `synthetic`: made-up parts with known answers: sizes, every entity
//!   op, and each error a frame can meet;
//! - `exactness`: every snapshot of a recorded run rebuilt through frames
//!   from every baseline up to 64 back and from none, compared bit for
//!   bit and by hash; delta sizes by baseline distance;
//! - `session`: a host's and a client's rings exchanging frames over a
//!   lossy, delayed link through the run's installs;
//! - `mutations`: seeded corruptions of valid frames through the decoder,
//!   under `catch_unwind`.
//!
//! Recorded runs come from `determinism_run` (the pinned determinism
//! scenario for any seat count) or from the caller (`Recorded::push`: the
//! launcher records a KOTOR module walk).

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use sim::determinism::{self, Run, Setup};
use sim::{Parts, SimId, SimRng, Snapshot, SnapshotHash, Stash, StepReason};

use super::{
    BLOB_WHOLE, CLIENT_RING, ClientRing, CodecError, Confirmed, FrameHeader, OP_NEW, Rebuilt,
    ServerRing, Stepped, decode, decode_frame, encode, encode_frame, put_varint,
};

/// One check: what it expects, what it found, and whether that passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub name: String,
    pub expect: String,
    pub found: String,
    pub pass: bool,
}

/// Checks in the order they ran.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Probes(pub Vec<Probe>);

impl Probes {
    pub fn check(&mut self, name: &str, expect: &str, found: String, pass: bool) {
        self.0.push(Probe {
            name: name.to_string(),
            expect: expect.to_string(),
            found,
            pass,
        });
    }

    pub fn failed(&self) -> usize {
        self.0.iter().filter(|probe| !probe.pass).count()
    }
}

/// One snapshot of a run as a host would send it.
#[derive(Debug, Clone)]
pub struct Shot {
    /// The install it belongs to: every install starts an epoch.
    pub epoch: u16,
    pub stepped: Stepped,
    pub hash: SnapshotHash,
}

/// Every snapshot of a run, in order: each step's, and each install's
/// before and after (the same tick in two epochs).
#[derive(Debug, Clone)]
pub struct Recorded {
    pub name: String,
    pub seats: u8,
    pub shots: Vec<Shot>,
}

impl Recorded {
    pub fn new(name: String, seats: u8) -> Recorded {
        Recorded {
            name,
            seats,
            shots: Vec::new(),
        }
    }

    pub fn push(&mut self, epoch: u16, snapshot: &Snapshot) {
        let hash = snapshot.hash();
        self.shots.push(Shot {
            epoch,
            stepped: Stepped::new(snapshot),
            hash,
        });
    }

    /// The last snapshot's hash.
    pub fn end(&self) -> Option<SnapshotHash> {
        self.shots.last().map(|shot| shot.hash)
    }
}

/// The pinned determinism scenario (`Setup::pinned`: wanderers, the
/// spawner, the proving ground installed after a third of the run) with
/// `seats` seats, stepped as its authority run A is. With 4 seats and
/// `PINNED_TICKS` it ends at CI's pinned hash.
pub fn determinism_run(seats: u8, ticks: u64) -> Recorded {
    let mut setup = Setup::pinned();
    setup.world.seats = seats;
    let ground = determinism::proving_ground();
    let install_at = ticks / 3;
    let mut run = Run::new(&setup);
    let mut recorded = Recorded::new(format!("determinism, {}", seats_label(seats)), seats);
    let mut epoch = 0;
    recorded.push(epoch, &run.last);
    for tick in 1..=ticks {
        let input = determinism::pilot(setup.pilot, tick, seats);
        run.step(&input, StepReason::Authority);
        recorded.push(epoch, &run.last);
        if tick == install_at {
            run.install(&ground);
            epoch += 1;
            recorded.push(epoch, &run.last);
        }
    }
    recorded
}

/// "1 seat", "4 seats": for run names.
pub fn seats_label(seats: u8) -> String {
    match seats {
        1 => "1 seat".to_string(),
        seats => format!("{seats} seats"),
    }
}

/// Byte counts, summarised.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sizes(pub Vec<usize>);

impl Sizes {
    pub fn mean(&self) -> f64 {
        self.0.iter().sum::<usize>() as f64 / self.0.len().max(1) as f64
    }

    pub fn max(&self) -> usize {
        self.0.iter().copied().max().unwrap_or(0)
    }

    /// The 95th percentile (nearest rank).
    pub fn p95(&self) -> usize {
        let mut sorted = self.0.clone();
        sorted.sort_unstable();
        let rank = (sorted.len() * 95).div_ceil(100);
        sorted.get(rank.saturating_sub(1)).copied().unwrap_or(0)
    }
}

/// Baseline distances whose frame sizes `exactness` reports: every tick,
/// every second tick (the default send rate), about a 100 ms round trip,
/// and the client ring's depth.
pub const DISTANCES: [usize; 4] = [1, 2, 13, CLIENT_RING];

/// What `exactness` found.
#[derive(Debug, Clone, Default)]
pub struct Exactness {
    /// Rebuilds tried and those equal to the original, bytes and hash.
    pub total: usize,
    pub exact: usize,
    /// Of those, deltas across an install (not frames: deltas never cross
    /// an epoch on the wire, but the codec must still be exact there).
    pub across: usize,
    /// The first few that were not exact.
    pub failures: Vec<String>,
    /// Frame bytes (header and delta) of keyframes, and by distance within
    /// an epoch (`DISTANCES`).
    pub keyframes: Sizes,
    pub by_distance: BTreeMap<usize, Sizes>,
    /// Of the frames one tick apart, those larger than a datagram's
    /// payload less the packet header (1200 − 19), which would need
    /// fragments.
    pub over_datagram: usize,
    /// The largest snapshot encoding (`Snapshot::encode`), bytes.
    pub encoding_max: usize,
}

impl Exactness {
    /// `later`'s findings after these, as one pass over both stretches
    /// would have found them.
    fn then(mut self, later: Exactness) -> Exactness {
        self.total += later.total;
        self.exact += later.exact;
        self.across += later.across;
        self.failures.extend(later.failures);
        self.failures.truncate(5);
        self.keyframes.0.extend(later.keyframes.0);
        for (distance, sizes) in later.by_distance {
            self.by_distance
                .entry(distance)
                .or_default()
                .0
                .extend(sizes.0);
        }
        self.over_datagram += later.over_datagram;
        self.encoding_max = self.encoding_max.max(later.encoding_max);
        self
    }
}

/// Rebuilds every snapshot of `run` from a keyframe and from each of the
/// `back` snapshots before it. Stretches of the run on threads of their
/// own (every rebuild stands alone), their findings joined in order.
pub fn exactness(run: &Recorded, back: usize) -> Exactness {
    let shots = run.shots.len();
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    let stretch = shots.div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        let stretches: Vec<_> = (0..shots)
            .step_by(stretch)
            .map(|first| {
                scope.spawn(move || exactness_in(run, back, first..(first + stretch).min(shots)))
            })
            .collect();
        stretches
            .into_iter()
            .map(|stretch| stretch.join().expect("an exactness stretch finishes"))
            .fold(Exactness::default(), Exactness::then)
    })
}

/// `exactness` over the snapshots `shots` of `run`.
fn exactness_in(run: &Recorded, back: usize, shots: std::ops::Range<usize>) -> Exactness {
    let mut found = Exactness::default();
    let mut frame = Vec::new();
    for index in shots {
        let shot = &run.shots[index];
        found.encoding_max = found.encoding_max.max(shot.stepped.parts().bytes().len());
        for distance in 0..=back.min(index) {
            let base = index
                .checked_sub(distance)
                .filter(|_| distance > 0)
                .map(|at| &run.shots[at]);
            let within = base.is_none_or(|base| base.epoch == shot.epoch);
            let result = if within {
                frame.clear();
                encode_frame(
                    shot.epoch,
                    &shot.stepped,
                    base.map(|base| &base.stepped),
                    &mut frame,
                );
                if distance == 0 {
                    found.keyframes.0.push(frame.len());
                } else if DISTANCES.contains(&distance) {
                    found
                        .by_distance
                        .entry(distance)
                        .or_default()
                        .0
                        .push(frame.len());
                }
                if distance == 1 && frame.len() > crate::MAX_DATAGRAM - 19 {
                    found.over_datagram += 1;
                }
                decode_frame(&frame, base.map(|base| base.stepped.parts()))
            } else {
                found.across += 1;
                frame.clear();
                let base = base.map(|base| base.stepped.parts());
                encode(base, shot.stepped.parts(), &mut frame);
                decode(base, &frame).and_then(|parts| {
                    let snapshot = Snapshot::join(&parts).map_err(CodecError::Snapshot)?;
                    Ok(Rebuilt {
                        tick: snapshot.tick,
                        parts,
                        snapshot,
                    })
                })
            };
            found.total += 1;
            let problem = result.as_ref().map_or_else(
                |error| error.to_string(),
                |rebuilt| {
                    if rebuilt.parts != *shot.stepped.parts() {
                        "different parts".to_string()
                    } else if rebuilt.snapshot.hash() != shot.hash {
                        "different hash".to_string()
                    } else {
                        String::new()
                    }
                },
            );
            if problem.is_empty() {
                found.exact += 1;
            } else if found.failures.len() < 5 {
                found.failures.push(format!(
                    "tick {} from {}: {problem}",
                    shot.stepped.tick,
                    base.map_or("none".to_string(), |base| base.stepped.tick.to_string())
                ));
            }
        }
    }
    found
}

/// What `session` found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Session {
    pub sent: usize,
    pub lost: usize,
    /// Frames rebuilt and equal to what was sent.
    pub rebuilt: usize,
    /// Refused as expected: of an epoch already left, or older than the
    /// ring.
    pub refused: usize,
    /// Rebuilt to another snapshot, or refused for any other reason.
    pub wrong: Vec<String>,
    pub keyframes: usize,
    pub bytes: usize,
}

/// A host sending every snapshot of `run` to one client over a link that
/// drops `loss_percent` of frames (seeded) and delivers the rest, and the
/// client's confirmations, `delay` snapshots later. At each install both
/// sides start the epoch; the client holds the install's result itself,
/// its first baseline, and the host resumes once it said so (the design's
/// Loaded barrier). Frames still in flight from the old epoch are refused.
pub fn session(run: &Recorded, loss_percent: u32, delay: usize, seed: u64) -> Session {
    let mut found = Session::default();
    let mut rng = SimRng::new(seed, 0x5e55);
    let first_epoch = run.shots.first().map_or(0, |shot| shot.epoch);
    let mut server = ServerRing::new(first_epoch);
    let mut client = ClientRing::new(first_epoch, CLIENT_RING);
    let mut confirmed = Confirmed::default();
    // (due at, what): frames to the client, confirmations to the host.
    let mut frames: Vec<(usize, Vec<u8>, usize)> = Vec::new();
    let mut confirmations: Vec<(usize, (u16, u64))> = Vec::new();
    for (index, shot) in run.shots.iter().enumerate() {
        let installed = shot.epoch != server.epoch();
        if installed {
            server.begin_epoch(shot.epoch);
            let own = Snapshot::join(shot.stepped.parts()).expect("a recorded snapshot joins");
            client.begin_epoch(shot.epoch, Some(Rebuilt::of(own)));
        }
        server.push(shot.stepped.clone());
        if installed {
            // The world resumes once the client is loaded: its Loaded
            // confirms the install's result before the next step.
            server.confirm(&mut confirmed, shot.epoch, shot.stepped.tick);
        }
        for &(_, (epoch, tick)) in confirmations.iter().filter(|(due, _)| *due <= index) {
            server.confirm(&mut confirmed, epoch, tick);
        }
        confirmations.retain(|(due, _)| *due > index);
        let mut frame = Vec::new();
        server.frame(&confirmed, &mut frame);
        found.sent += 1;
        found.bytes += frame.len();
        if FrameHeader::read(&frame).is_ok_and(|(header, _)| header.baseline.is_none()) {
            found.keyframes += 1;
        }
        if rng.below(100) < loss_percent {
            found.lost += 1;
        } else {
            frames.push((index + delay, frame, index));
        }
        for (_, frame, sent_at) in frames.iter().filter(|(due, _, _)| *due <= index) {
            match client.receive(frame) {
                Ok(rebuilt) if rebuilt.parts == *run.shots[*sent_at].stepped.parts() => {
                    found.rebuilt += 1;
                }
                Ok(rebuilt) => found
                    .wrong
                    .push(format!("tick {}: another snapshot", rebuilt.tick)),
                Err(CodecError::Epoch { .. } | CodecError::Stale { .. }) => found.refused += 1,
                Err(error) => found.wrong.push(error.to_string()),
            }
        }
        frames.retain(|(due, _, _)| *due > index);
        if let Some(newest) = client.newest() {
            confirmations.push((index + delay, (client.epoch(), newest.tick)));
        }
    }
    found
}

/// What `mutations` found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mutations {
    pub tried: usize,
    /// Must be 0.
    pub panics: usize,
    /// Corrupted frames that still rebuilt: only ones whose change undid
    /// itself or touched nothing read, so they rebuild the original.
    pub accepted: usize,
    /// Accepted but not the original: must be 0 (the hash check).
    pub accepted_wrong: usize,
    pub errors: BTreeMap<&'static str, usize>,
}

/// A valid frame, its baseline and the hash it rebuilds to.
struct Sample {
    frame: Vec<u8>,
    base: Option<Arc<Parts>>,
    hash: SnapshotHash,
}

/// `count` seeded corruptions of valid frames of `run` (keyframes and
/// deltas 1, 13 and 64 back, from 64 places in the run) through
/// `decode_frame`: bits flipped, bytes set, cut, inserted, deleted,
/// spliced from another frame, runs of `0xff` (varint bombs), and valid
/// frames against the wrong baseline.
pub fn mutations(run: &Recorded, count: usize, seed: u64) -> Mutations {
    let mut samples = Vec::new();
    let stride = (run.shots.len() / 64).max(1);
    for index in (0..run.shots.len()).step_by(stride) {
        let shot = &run.shots[index];
        for distance in [0, 1, 13, 64] {
            let base = index
                .checked_sub(distance)
                .filter(|_| distance > 0)
                .map(|at| &run.shots[at]);
            if base.is_some_and(|base| base.epoch != shot.epoch) || distance > index {
                continue;
            }
            let mut frame = Vec::new();
            encode_frame(
                shot.epoch,
                &shot.stepped,
                base.map(|base| &base.stepped),
                &mut frame,
            );
            samples.push(Sample {
                frame,
                base: base.map(|base| base.stepped.shared_parts().clone()),
                hash: shot.hash,
            });
        }
    }
    let mut found = Mutations::default();
    if samples.is_empty() {
        return found;
    }
    let mut rng = SimRng::new(seed, 0xc0de);
    for _ in 0..count {
        let sample = &samples[pick(&mut rng, samples.len())];
        let mut bytes = sample.frame.clone();
        let mut base = sample.base.as_deref();
        let len = bytes.len();
        match rng.below(8) {
            0 => {
                for _ in 0..1 + rng.below(4) {
                    let at = pick(&mut rng, len);
                    bytes[at] ^= 1 << rng.below(8);
                }
            }
            1 => {
                let at = pick(&mut rng, len);
                bytes[at] = rng.below(256) as u8;
            }
            2 => bytes.truncate(pick(&mut rng, len)),
            3 => {
                let at = pick(&mut rng, len + 1);
                let extra: Vec<u8> = (0..1 + rng.below(16))
                    .map(|_| rng.below(256) as u8)
                    .collect();
                bytes.splice(at..at, extra);
            }
            4 => {
                let at = pick(&mut rng, len);
                let end = (at + 1 + pick(&mut rng, 32)).min(len);
                bytes.drain(at..end);
            }
            5 => {
                let other = &samples[pick(&mut rng, samples.len())].frame;
                let from = pick(&mut rng, other.len());
                let piece = &other[from..(from + 1 + pick(&mut rng, 64)).min(other.len())];
                let at = pick(&mut rng, len);
                let end = (at + piece.len()).min(len);
                bytes.splice(at..end, piece.iter().copied());
            }
            6 => {
                let at = pick(&mut rng, len);
                let end = (at + 1 + pick(&mut rng, 12)).min(len);
                bytes[at..end].fill(0xff);
            }
            _ => base = samples[pick(&mut rng, samples.len())].base.as_deref(),
        }
        found.tried += 1;
        match catch_unwind(AssertUnwindSafe(|| decode_frame(&bytes, base))) {
            Err(_) => found.panics += 1,
            Ok(Ok(rebuilt)) => {
                found.accepted += 1;
                if rebuilt.snapshot.hash() != sample.hash {
                    found.accepted_wrong += 1;
                }
            }
            Ok(Err(error)) => *found.errors.entry(error.kind()).or_insert(0) += 1,
        }
    }
    found
}

/// Uniform in `0..len` (0 for an empty range).
fn pick(rng: &mut SimRng, len: usize) -> usize {
    rng.below(u32::try_from(len.max(1)).unwrap_or(u32::MAX)) as usize
}

/// Parts of made-up records: (id, length) each, the bytes from `seed`.
fn made_up(records: &[(u32, usize)], seed: u64) -> Parts {
    let mut rng = SimRng::new(seed, 0xfab);
    let mut bytes = |len: usize| (0..len).map(|_| rng.below(256) as u8).collect::<Vec<u8>>();
    Parts {
        head: bytes(112),
        entities: records
            .iter()
            .map(|&(id, len)| (SimId(id), bytes(len)))
            .collect(),
        tail: bytes(40),
        stash: Stash::default(),
    }
}

/// The delta of `now` against `base`, and whether it decodes to `now`.
fn round_trip(base: Option<&Parts>, now: &Parts) -> (Vec<u8>, bool) {
    let mut delta = Vec::new();
    encode(base, now, &mut delta);
    let same = decode(base, &delta).is_ok_and(|parts| parts == *now);
    (delta, same)
}

/// Known answers on made-up parts and on the first snapshots of a
/// determinism run.
pub fn synthetic() -> Probes {
    let mut probes = Probes::default();
    let base = made_up(&[(1, 154), (2, 54), (3, 54), (5, 41), (8, 41)], 1);

    let (delta, same) = round_trip(Some(&base), &base);
    probes.check(
        "unchanged",
        "a snapshot against itself: 4 bytes (head same, end, tail same, stash same)",
        format!("{} bytes {delta:?}, rebuilt {same}", delta.len()),
        same && delta == [0, 0, 0, 0],
    );

    let mut now = base.clone();
    now.entities[0].1[17] ^= 0x40;
    let (delta, same) = round_trip(Some(&base), &now);
    probes.check(
        "one byte",
        "one byte of the first record: 6 bytes (op, 3-byte group mask of a 154-byte record, byte mask, the byte), plus 4",
        format!("{} bytes, rebuilt {same}", delta.len()),
        same && delta.len() == 4 + 6,
    );

    let mut now = base.clone();
    now.head[8] ^= 1;
    now.tail[4] ^= 1;
    let (delta, same) = round_trip(Some(&base), &now);
    probes.check(
        "head and tail",
        "a byte of each: masked (tag, 2-byte mask of 112 bytes, byte mask, byte = 5) and (tag, mask of 40 bytes, byte mask, byte = 4), plus the end and the stash",
        format!("{} bytes, rebuilt {same}", delta.len()),
        same && delta.len() == 5 + 1 + 4 + 1,
    );

    let mut now = base.clone();
    now.tail.splice(1..1, [7, 7, 7]);
    let (delta, same) = round_trip(Some(&base), &now);
    probes.check(
        "tail grows",
        "3 bytes inserted after the tail's first (a transition asked): spliced (tag, prefix, suffix, length, 3 bytes = 7), plus 3",
        format!("{} bytes, rebuilt {same}", delta.len()),
        same && delta.len() == 7 + 3,
    );

    let edits: [(&str, &[(u32, usize)]); 7] = [
        (
            "new between",
            &[(1, 154), (2, 54), (3, 54), (4, 54), (5, 41), (8, 41)],
        ),
        ("removed first", &[(2, 54), (3, 54), (5, 41), (8, 41)]),
        ("removed last", &[(1, 154), (2, 54), (3, 54), (5, 41)]),
        (
            "new after the last",
            &[
                (1, 154),
                (2, 54),
                (3, 54),
                (5, 41),
                (8, 41),
                (9, 41),
                (400, 3),
            ],
        ),
        (
            "longer record",
            &[(1, 154), (2, 55), (3, 54), (5, 41), (8, 41)],
        ),
        ("all replaced", &[(9, 10), (10, 10)]),
        ("none left", &[]),
    ];
    for (name, records) in edits {
        // Records keep the baseline's bytes where the id and length agree.
        let mut now = made_up(records, 2);
        now.head.clone_from(&base.head);
        for (id, record) in &mut now.entities {
            if let Some((_, before)) = base.entities.iter().find(|(other, _)| other == id)
                && before.len() == record.len()
            {
                record.clone_from(before);
            }
        }
        let (delta, same) = round_trip(Some(&base), &now);
        probes.check(
            name,
            "rebuilt exactly",
            format!("{} bytes, rebuilt {same}", delta.len()),
            same,
        );
    }

    let (delta, same) = round_trip(None, &base);
    let whole = base.bytes().len();
    probes.check(
        "keyframe",
        "every record new: the encoding plus 2 to 4 bytes a record",
        format!(
            "{} bytes for a {whole}-byte encoding, rebuilt {same}",
            delta.len()
        ),
        same && delta.len() > whole && delta.len() <= whole + 4 * base.entities.len() + 8,
    );

    frame_errors(&mut probes);
    decoder_errors(&mut probes, &base);
    probes
}

/// Frames of a determinism run's first ticks against the errors a client
/// meets: no baseline, a wrong check, another epoch, a stale tick.
fn frame_errors(probes: &mut Probes) {
    // Installs after tick 70: shots 0 to 70 are ticks 0 to 70, epoch 0.
    let run = determinism_run(2, 210);
    let shots = &run.shots;
    let stepped = |tick: usize| &shots[tick].stepped;
    let mut frame = Vec::new();
    encode_frame(0, stepped(10), Some(stepped(9)), &mut frame);
    let found = decode_frame(&frame, None);
    probes.check(
        "no baseline",
        "a delta without its baseline: NoBaseline",
        format!("{found:?}"),
        found.is_err_and(|error| error == CodecError::NoBaseline { tick: 9 }),
    );
    let found = decode_frame(&frame, Some(stepped(8).parts())).map(|rebuilt| rebuilt.tick);
    probes.check(
        "wrong baseline",
        "a delta against another snapshot: refused (the check or the decoder)",
        format!("{found:?}"),
        found.is_err(),
    );
    // The check follows the epoch (2 bytes), the tick and the distance
    // back (a byte each here).
    let mut tampered = frame.clone();
    tampered[4] ^= 1;
    let found = decode_frame(&tampered, Some(stepped(9).parts())).map(|rebuilt| rebuilt.tick);
    probes.check(
        "check",
        "a check byte flipped: Check",
        format!("{found:?}"),
        found.is_err_and(|error| error == CodecError::Check { tick: 10 }),
    );

    let mut client = ClientRing::new(1, CLIENT_RING);
    let found = client.receive(&frame).map(|rebuilt| rebuilt.tick);
    probes.check(
        "epoch",
        "a frame of epoch 0 at a client in epoch 1: Epoch",
        format!("{found:?}"),
        found.is_err_and(|error| error == CodecError::Epoch { frame: 0, here: 1 }),
    );

    // A client fed keyframes of ticks 1..=66 holds the last 64; tick 2 is
    // then older than all of them.
    let mut client = ClientRing::new(0, CLIENT_RING);
    let mut received = 0;
    for tick in 1..=66 {
        frame.clear();
        encode_frame(0, stepped(tick), None, &mut frame);
        received += usize::from(client.receive(&frame).is_ok());
    }
    frame.clear();
    encode_frame(0, stepped(2), None, &mut frame);
    let stale = client.receive(&frame).map(|rebuilt| rebuilt.tick);
    frame.clear();
    encode_frame(0, stepped(67), Some(stepped(2)), &mut frame);
    let gone = client.receive(&frame).map(|rebuilt| rebuilt.tick);
    frame.clear();
    encode_frame(0, stepped(67), Some(stepped(66)), &mut frame);
    let delta = client.receive(&frame).map(|rebuilt| rebuilt.tick);
    let again = client.receive(&frame).map(|rebuilt| rebuilt.tick);
    let held = client.newest().map(|rebuilt| rebuilt.tick);
    probes.check(
        "client ring",
        "66 keyframes received; tick 2 then Stale; a delta on tick 2 NoBaseline; on 66 rebuilt, twice (a duplicate), newest 67",
        format!("{received} received, {stale:?}, {gone:?}, {delta:?}, {again:?}, newest {held:?}"),
        received == 66
            && stale == Err(CodecError::Stale { tick: 2 })
            && gone == Err(CodecError::NoBaseline { tick: 2 })
            && delta == Ok(67)
            && again == Ok(67)
            && held == Some(67),
    );

    let mut server = ServerRing::new(3);
    for tick in 1..=5 {
        server.push(stepped(tick).clone());
    }
    let mut confirmed = Confirmed::default();
    let takes = [
        server.confirm(&mut confirmed, 2, 4),
        server.confirm(&mut confirmed, 3, 6),
        server.confirm(&mut confirmed, 3, 4),
        server.confirm(&mut confirmed, 3, 3),
    ];
    let base = server.baseline(&confirmed).map(|base| base.tick);
    probes.check(
        "server ring",
        "confirmations: another epoch, an unstepped tick, 4, then the older 3: only 4 taken, the baseline",
        format!("{takes:?}, baseline {base:?}"),
        takes == [false, false, true, false] && base == Some(4),
    );
}

/// A hand-made delta and the error it must meet.
struct Refused {
    name: &'static str,
    expect: &'static str,
    delta: Vec<u8>,
    /// Against the made-up baseline, or as a keyframe.
    against_base: bool,
    kind: &'static str,
}

/// Hand-made deltas the decoder must refuse without allocating for them.
fn decoder_errors(probes: &mut Probes, base: &Parts) {
    // Head whole and empty, then 70 000 new records announced.
    let mut count_bomb = vec![BLOB_WHOLE, 0];
    put_varint(&mut count_bomb, (70_000 << 2) | OP_NEW);
    let refused = |name, expect, delta, against_base, kind| Refused {
        name,
        expect,
        delta,
        against_base,
        kind,
    };
    let cases = [
        refused(
            "count bomb",
            "70 000 new records: Limit, before any is read",
            count_bomb,
            false,
            "limit",
        ),
        refused(
            "record bomb",
            "a new record of 2^40 bytes: Limit",
            vec![2, 0, 7, 0, 0x80, 0x80, 0x80, 0x80, 0x80, 0x20],
            false,
            "limit",
        ),
        refused(
            "long varint",
            "an 11-byte varint: Invalid",
            [2].into_iter().chain([0xff; 10]).chain([1]).collect(),
            false,
            "invalid",
        ),
        refused(
            "overlong varint",
            "0x80 0x00: Invalid (not minimal)",
            vec![2, 0x80, 0],
            false,
            "invalid",
        ),
        refused(
            "keep without baseline",
            "a keep in a keyframe: Invalid",
            vec![2, 0, 4, 0, 2, 0],
            false,
            "invalid",
        ),
        refused(
            "zero XOR byte",
            "a changed byte of 0: Invalid",
            vec![0, 6, 1, 0, 0, 1, 0],
            true,
            "invalid",
        ),
        refused(
            "trailing",
            "a byte after the stash: Trailing",
            vec![0, 0, 0, 0, 9],
            true,
            "trailing",
        ),
    ];
    for case in cases {
        let found = decode(case.against_base.then_some(base), &case.delta);
        probes.check(
            case.name,
            case.expect,
            format!("{:?}", found.as_ref().map(|parts| parts.entities.len())),
            found.is_err_and(|error| error.kind() == case.kind),
        );
    }
}
