//! The flight recorder's file (`KJKF`): a recording written as it is made,
//! so a crash or a kill leaves all of it but the last second. A header
//! (magic, version, the build that wrote it), then chunks: kind, length,
//! payload, and a check of the payload (the first four bytes of its
//! blake3). The first chunk is the start (rate, level, snapshot, scripts'
//! state); each after it is `BATCH` ticks, or fewer at the end, closed by
//! the hash of the world after them. A segment starts at a module's entry,
//! so a segment installs nothing.
//!
//! Reading stops at the first chunk cut short or failing its check: what
//! came before is the recording, its last whole chunk its end.

use crate::input::TickInput;
use crate::replay::{Recording, decode_tick, encode_tick};
use crate::save::ScriptVmState;
use crate::snapshot::{Snapshot, SnapshotHash};
use crate::wire::{DecodeError, Reader, Writer};
use crate::world::LevelId;

pub const MAGIC: &[u8; 4] = b"KJKF";
pub const VERSION: u32 = 1;

/// Ticks a chunk holds: a second at the default rate.
pub const BATCH: u64 = 60;

const START: u8 = 0;
const TICKS: u8 = 1;

/// Makes the chunks of one segment as the host steps.
pub struct FlightWriter {
    pending: Writer,
    count: u64,
}

impl FlightWriter {
    /// A segment starting from `start` in `level`, written by `build`: the
    /// writer, and the header with the start chunk.
    pub fn new(
        hz: u32,
        start: &Snapshot,
        level: &LevelId,
        script: &ScriptVmState,
        build: &str,
    ) -> (Self, Vec<u8>) {
        let mut out = Writer::default();
        out.bytes(MAGIC);
        out.u32(VERSION);
        out.string(build);
        let mut payload = Writer::default();
        payload.u32(hz);
        level.write(&mut payload);
        payload.section(&start.encode());
        let mut scripts = Writer::default();
        script.encode(&mut scripts);
        payload.section(&scripts.0);
        chunk(&mut out, START, &payload.0);
        let writer = FlightWriter {
            pending: Writer::default(),
            count: 0,
        };
        (writer, out.0)
    }

    /// One stepped tick; every `BATCH` of them, their chunk.
    pub fn tick(&mut self, msec: u32, input: &TickInput, after: &Snapshot) -> Option<Vec<u8>> {
        encode_tick(msec, input, &mut self.pending);
        self.count += 1;
        (self.count == BATCH).then(|| self.flush(after))
    }

    /// The ticks not yet in a chunk, as one ending at `last` (nothing when
    /// there are none).
    pub fn flush(&mut self, last: &Snapshot) -> Vec<u8> {
        if self.count == 0 {
            return Vec::new();
        }
        let mut payload = Writer::default();
        payload.u64(self.count);
        payload.bytes(&std::mem::take(&mut self.pending).0);
        payload.u64(last.tick);
        payload.bytes(&last.hash().0);
        self.count = 0;
        let mut out = Writer::default();
        chunk(&mut out, TICKS, &payload.0);
        out.0
    }
}

fn check(payload: &[u8]) -> [u8; 4] {
    let hash = blake3::hash(payload);
    let mut first = [0; 4];
    first.copy_from_slice(&hash.as_bytes()[..4]);
    first
}

fn chunk(out: &mut Writer, kind: u8, payload: &[u8]) {
    out.u8(kind);
    out.u32(u32::try_from(payload.len()).expect("a chunk under 4 GiB"));
    out.bytes(payload);
    out.bytes(&check(payload));
}

/// How long the header is (magic, version, build), where the chunks start.
pub fn header_len(bytes: &[u8]) -> usize {
    let mut input = Reader::new("flight", bytes);
    match input.header(MAGIC, VERSION).and_then(|()| input.string()) {
        Ok(_) => input.at(),
        Err(_) => 0,
    }
}

/// Whether `bytes` are a flight recorder's file.
pub fn sniff(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// The build that wrote a flight recorder's file.
pub fn build(bytes: &[u8]) -> Result<String, DecodeError> {
    let mut input = Reader::new("flight", bytes);
    input.header(MAGIC, VERSION)?;
    input.string()
}

/// A segment as a recording, up to its last whole chunk; refused without
/// a start.
pub fn read(bytes: &[u8]) -> Result<Recording, DecodeError> {
    let mut input = Reader::new("flight", bytes);
    input.header(MAGIC, VERSION)?;
    input.string()?;
    let (kind, payload) = next_chunk(&mut input).ok_or_else(|| input.invalid("a start chunk", input.at()))?;
    if kind != START {
        return Err(input.invalid("a start chunk", input.at()));
    }
    let mut start = Reader::new("flight start", payload);
    let hz = start.u32()?;
    let level = LevelId::read(&mut start)?;
    let first = Snapshot::decode(start.section()?)?;
    let mut scripts = Reader::new("flight scripts", start.section()?);
    let script = ScriptVmState::decode(&mut scripts)?;
    let end = first.hash();
    let mut recording = Recording {
        hz,
        level,
        start: first,
        script,
        ticks: Vec::new(),
        checkpoints: Vec::new(),
        end,
        installs: Vec::new(),
    };
    while let Some((TICKS, payload)) = next_chunk(&mut input) {
        let Ok((ticks, at)) = read_ticks(payload) else {
            break;
        };
        recording.ticks.extend(ticks);
        recording.checkpoints.push(at);
        recording.end = at.1;
    }
    Ok(recording)
}

/// The next whole chunk whose check holds, if any.
fn next_chunk<'a>(input: &mut Reader<'a>) -> Option<(u8, &'a [u8])> {
    let kind = input.u8().ok()?;
    let len = input.u32().ok()? as usize;
    let payload = input.take(len).ok()?;
    let sum: [u8; 4] = input.array().ok()?;
    (sum == check(payload)).then_some((kind, payload))
}

/// A tick chunk's ticks, and the tick and hash it ends at.
fn read_ticks(payload: &[u8]) -> Result<(Vec<crate::replay::RecordedTick>, (u64, SnapshotHash)), DecodeError> {
    let mut input = Reader::new("flight ticks", payload);
    let count = input.u64()?;
    let ticks = (0..count)
        .map(|_| decode_tick(&mut input))
        .collect::<Result<Vec<_>, _>>()?;
    let at = (input.u64()?, SnapshotHash(input.array()?));
    input.finish()?;
    Ok((ticks, at))
}

#[cfg(feature = "probes")]
mod probes;
#[cfg(feature = "probes")]
pub use probes::probes;
