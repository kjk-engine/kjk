//! The probes of `flight` (`kjk sim probes`, group "flight").

use super::*;
use crate::determinism::{RECORD_EVERY, Run, Setup, pilot};
use crate::install::Announcement;
use crate::probe::{Checks, check};
use crate::replay::Recorder;
use crate::world::{Level, StepReason};

/// Ticks the probes' run steps: four whole chunks and a part.
const TICKS: u64 = 250;

/// The pinned setup's run stepped `TICKS` ticks, kept by a `Recorder` and
/// a `FlightWriter` side by side; the recorder's recording, the flight's
/// bytes.
fn recorded() -> (Recording, Vec<u8>, Level) {
    let setup = Setup::pinned();
    let mut run = Run::new(&setup);
    let level = run.world.level().clone();
    let id = level.id();
    let mut recorder = Recorder::new(run.rate.hz(), run.last.clone(), id.clone(), RECORD_EVERY)
        .with_script(run.world.script_state().clone());
    let (mut flight, mut bytes) = FlightWriter::new(
        run.rate.hz(),
        &run.last,
        &id,
        run.world.script_state(),
        "probe-build",
    );
    for tick in 1..=TICKS {
        let input = pilot(setup.pilot, tick, setup.world.seats);
        let msec = run.rate.msec(tick);
        let after = run.step(&input, StepReason::Authority).clone();
        recorder.record(msec, &input, &after);
        bytes.extend(flight.tick(msec, &input, &after).into_iter().flatten());
    }
    bytes.extend(flight.flush(&run.last));
    (recorder.finish(&run.last), bytes, level)
}

pub fn probes() -> Checks {
    let (recording, bytes, level) = recorded();
    let whole = TICKS / BATCH * BATCH;
    let read = read(&bytes);
    let last_chunk = last_chunk(&bytes);
    vec![
        (
            "flight_matches_recorder",
            matches(&recording, &read, &level),
        ),
        (
            "flight_truncated_keeps_whole_chunks",
            truncated(&bytes, last_chunk, whole),
        ),
        ("flight_bad_crc_stops", bad_crc(&bytes, last_chunk, whole)),
        ("flight_empty_is_error", empty(&bytes)),
        (
            "flight_sniffed",
            check(
                sniff(&bytes) && !sniff(b"KJKR...."),
                "KJKF told from KJKR".into(),
            ),
        ),
        ("flight_skip_data_check", skip_data(&recording, &level)),
    ]
}

/// Where the last chunk starts in `bytes`: the chunks walked by their
/// lengths (kind, length, payload, crc).
fn last_chunk(bytes: &[u8]) -> usize {
    let (mut at, mut last) = (header_len(bytes), 0);
    while at + 5 <= bytes.len() {
        let len =
            u32::from_le_bytes(bytes[at + 1..at + 5].try_into().expect("four bytes")) as usize;
        last = at;
        at += 1 + 4 + len + 4;
    }
    last
}

fn matches(
    recording: &Recording,
    read: &Result<Recording, DecodeError>,
    level: &Level,
) -> Result<String, String> {
    let read = read.as_ref().map_err(|error| error.to_string())?;
    let same_ticks = read.ticks == recording.ticks;
    let played = read
        .play(level)
        .map_err(|divergence| format!("{divergence:?}"))?;
    let ok = same_ticks && played.hash() == recording.end && read.end == recording.end;
    check(
        ok,
        format!("{} ticks, same: {same_ticks}", read.ticks.len()),
    )
}

fn truncated(bytes: &[u8], last_chunk: usize, whole: u64) -> Result<String, String> {
    let wrong: Vec<usize> = (last_chunk..bytes.len())
        .filter(|&cut| read(&bytes[..cut]).map_or(true, |read| read.ticks.len() as u64 != whole))
        .collect();
    check(
        wrong.is_empty(),
        format!("cuts not read to {whole} ticks: {}", wrong.len()),
    )
}

/// The last chunk's closing hash bent (its payload still decodes): only its
/// check can tell, and drops it.
fn bad_crc(bytes: &[u8], last_chunk: usize, whole: u64) -> Result<String, String> {
    let mut bent = bytes.to_vec();
    debug_assert!(bent.len() - 5 > last_chunk);
    let end = bent.len() - 5;
    bent[end] ^= 1;
    let read = read(&bent).map_err(|error| error.to_string())?;
    check(
        read.ticks.len() as u64 == whole,
        format!("{} ticks", read.ticks.len()),
    )
}

fn empty(bytes: &[u8]) -> Result<String, String> {
    let header = &bytes[..header_len(bytes)];
    check(
        read(&[]).is_err() && read(header).is_err(),
        "nothing, and a header alone, refused".into(),
    )
}

fn skip_data(recording: &Recording, level: &Level) -> Result<String, String> {
    let mut other = recording.clone();
    other.level.digest[0] ^= 1;
    let none =
        &mut |announcement: &Announcement| Err(format!("no level for {}", announcement.module));
    let checked = other.play_checked(level, none, &mut |_| {}, true);
    let skipped = other.play_checked(level, none, &mut |_| {}, false);
    check(
        checked.is_err() && skipped.is_ok(),
        format!("checked {:?}, skipped {:?}", checked.is_ok(), skipped.err()),
    )
}
