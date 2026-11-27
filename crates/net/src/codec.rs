//! Snapshots on the wire: exact deltas of a snapshot's encoding against one
//! the peer already holds (`docs/NET.md`).
//!
//! # What is diffed
//! The sim cuts a snapshot's canonical encoding into `sim::Parts`: the
//! head (signature, tick, time, random streams, next id, module), one
//! record per live entity by id, the tail (the transition asked for, the
//! modules left's digest, what the party shares) and the modules left
//! (`sim::Stash`). Records of one kind have one length, so two
//! snapshots' records line up byte for byte. The codec only ever diffs
//! bytes: it names no field, so when the sim's records grow, deltas follow
//! without a change here. The modules left change only at an install,
//! which starts an epoch: a delta says "the baseline's" by their digest,
//! never comparing or sending their bytes, so a frame costs the same
//! however many modules were left.
//!
//! Nothing is approximate. A client rebuilds the host's parts exactly and
//! restores worlds from them; a rounded position would turn every
//! prediction into a correction.
//!
//! # The delta
//! Against a baseline (`Option<&Parts>`; none is a keyframe, where every
//! record is new):
//!
//! ```text
//! delta    = blob(head) entities blob(tail) stash
//! blob     = 0                     the baseline's, unchanged
//!          | 1 masked              the same length, XORed (below)
//!          | 2 varint(len) bytes   whole
//!          | 3 varint(prefix) varint(suffix) varint(len) bytes
//!                                  the baseline's first `prefix` and
//!                                  last `suffix` bytes around new ones
//! entities = op* 0                 0 ends the list: the rest of the
//!                                  baseline is kept as it is
//! op       = varint(count << 2 | kind), count >= 1, then per kind:
//!   0 keep   the next `count` baseline records, unchanged
//!   1 drop   the next `count` baseline records, removed
//!   2 xor    the next `count` baseline records, changed: `masked` each
//!   3 new    `count` records inserted here: varint(id gap) varint(len)
//!            bytes each; the gap is from the id before it (so a run of
//!            consecutive ids costs a zero byte each)
//! stash    = 0                     the baseline's (the same digest)
//!          | 2 varint(len) bytes   whole (`sim::Stash::bytes`)
//! masked   = a group mask (a bit per 8 bytes of the record, least
//!            significant first), then per set group a byte mask (a bit
//!            per byte that changed) and the changed bytes XORed with
//!            the baseline's: never zero
//! ```
//!
//! A record whose length changed (it cannot today: lengths are fixed per
//! kind) is dropped and sent new under the same id. Varints are LEB128,
//! at most 10 bytes, minimal. The encoder picks the smallest form of a
//! blob: masked or whole when its length is the baseline's, spliced or
//! whole when not. The tail's length changes when a step asks for a
//! transition; the rest of it then stays a spliced suffix instead of
//! going whole.
//!
//! A quiet module's 218 unchanged objects cost the end byte; a moving
//! avatar costs the bytes of its floats that moved and its masks.
//!
//! # Robustness
//! Decoding never panics, whatever the bytes: every read is checked, every
//! count is bounded before a loop runs, and entity ids must rise strictly
//! (which also rules out duplicates). What a hostile delta can make the
//! decoder allocate is bounded by the baseline (kept and XORed records are
//! the baseline's length) plus the delta's own bytes (new records are
//! copied from it), and at most `MAX_ENTITIES` records, each at most
//! `MAX_RECORD` bytes.
//!
//! # Frames
//! A `Snapshot` frame (`FrameHeader`) carries the install epoch, the tick,
//! how many ticks back its baseline is (0: a keyframe) and 8 bytes of the
//! snapshot's BLAKE3 hash (`Snapshot::hash`), then the delta packed
//! (`pack`: a byte, then the delta as it is or deflated when that is
//! smaller; unpacked at most to `MAX_DELTA`). The receiver
//! rebuilds the parts, checks their hash (`Parts::hash`), and only then
//! decodes them with `Snapshot::join` (every rule that validates a
//! snapshot validates a network one). A hash mismatch is a codec bug or a
//! corrupt packet: a typed error, the snapshot dropped, never confirmed.
//! Equal hashes mean equal encodings, so a snapshot that passes is the
//! host's, bit for bit.
//!
//! # Baselines
//! `ServerRing` keeps the last `SERVER_RING` stepped snapshots of the
//! current epoch (`Stepped`: parts shared by `Arc`, and the check), shared
//! by every client; `Confirmed` is one client's newest confirmation, which
//! picks its baseline. `ClientRing` keeps the last `CLIENT_RING` rebuilt
//! snapshots and rebuilds frames against them. Deltas never cross an
//! epoch: a module install starts one, and its first baseline is the
//! install's own result, which a client computes itself.
//!
//! Here: the delta. `frame`: frames, checks and rebuilds. `ring`:
//! baselines. Evidence: `probes` (run by `kjk net codec`).

use std::fmt;
use std::sync::Arc;

use sim::{DecodeError, EntityPart, Parts, SimId, Stash};

use crate::wire::{self, put_varint, varint_len};

mod frame;
#[cfg(feature = "probes")]
pub mod probes;
mod ring;

pub use frame::{
    Check, FrameHeader, Rebuilt, Stepped, decode_frame, encode_frame, pack, rebuild, unpack,
};
pub use ring::{Begun, ClientRing, Confirmed, ServerRing};

/// The most entities a rebuilt snapshot may have.
pub const MAX_ENTITIES: usize = 65_536;
/// The longest record a delta may carry whole.
pub const MAX_RECORD: usize = 1 << 20;
/// Stepped snapshots the host keeps for deltas: two seconds at 125 Hz.
pub const SERVER_RING: usize = 256;
/// Rebuilt snapshots a client keeps: a second at 125 Hz.
pub const CLIENT_RING: usize = 128;

const BLOB_SAME: u8 = 0;
const BLOB_MASKED: u8 = 1;
const BLOB_WHOLE: u8 = 2;
const BLOB_SPLICED: u8 = 3;
const BLOB_DIGEST: u8 = 4;

const OP_KEEP: u64 = 0;
const OP_DROP: u64 = 1;
const OP_HOT_XOR: u64 = 2;
const OP_XOR: u64 = 3;
const OP_NEW: u64 = 4;
const OP_HOT_BLOB: u64 = 5;

fn cold_eq(a: &Arc<[u8]>, b: &Arc<[u8]>) -> bool {
    Arc::ptr_eq(a, b) || a == b
}

/// Why bytes did not rebuild a snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// The bytes end before a field they announce.
    Truncated { what: &'static str, at: usize },
    /// A field holds a value no encoder writes.
    Invalid { what: &'static str, at: usize },
    /// A count or length beyond the codec's limits.
    Limit { what: &'static str, at: usize },
    /// Bytes are left after the delta's end.
    Trailing { extra: usize },
    /// The frame's baseline is not held here (too old, or never received).
    NoBaseline { tick: u64 },
    /// The frame belongs to another install epoch.
    Epoch { frame: u16, here: u16 },
    /// Older than every snapshot held, with the ring full.
    Stale { tick: u64 },
    /// The rebuilt bytes do not hash to the frame's check.
    Check { tick: u64 },
    /// The rebuilt bytes hash right but are not a snapshot this build
    /// reads (a host of another build would be refused at the handshake).
    Snapshot(DecodeError),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Truncated { what, at } => write!(f, "delta: truncated {what} at byte {at}"),
            CodecError::Invalid { what, at } => write!(f, "delta: invalid {what} at byte {at}"),
            CodecError::Limit { what, at } => {
                write!(f, "delta: {what} over the limit at byte {at}")
            }
            CodecError::Trailing { extra } => write!(f, "delta: {extra} bytes after the end"),
            CodecError::NoBaseline { tick } => {
                write!(f, "snapshot frame: baseline {tick} not held")
            }
            CodecError::Epoch { frame, here } => {
                write!(f, "snapshot frame: epoch {frame}, this side is in {here}")
            }
            CodecError::Stale { tick } => {
                write!(f, "snapshot frame: tick {tick} older than all held")
            }
            CodecError::Check { tick } => {
                write!(f, "snapshot frame: tick {tick} rebuilt to another hash")
            }
            CodecError::Snapshot(error) => write!(f, "snapshot frame: {error}"),
        }
    }
}

impl std::error::Error for CodecError {}

impl CodecError {
    /// The variant's name, for counting errors by kind.
    pub fn kind(&self) -> &'static str {
        match self {
            CodecError::Truncated { .. } => "truncated",
            CodecError::Invalid { .. } => "invalid",
            CodecError::Limit { .. } => "limit",
            CodecError::Trailing { .. } => "trailing",
            CodecError::NoBaseline { .. } => "no baseline",
            CodecError::Epoch { .. } => "epoch",
            CodecError::Stale { .. } => "stale",
            CodecError::Check { .. } => "check",
            CodecError::Snapshot(_) => "snapshot",
        }
    }
}

// ---------------------------------------------------------------------------
// Encoding

/// Appends `now` as a delta against `base` (`None`: a keyframe) to `out`.
/// Allocation-free apart from `out` growing.
pub fn encode(base: Option<&Parts>, now: &Parts, out: &mut Vec<u8>) {
    encode_blob(base.map(|base| base.head.as_slice()), &now.head, out);
    let records: &[EntityPart] = base.map_or(&[], |base| &base.entities);
    Walk {
        base: records,
        now: &now.entities,
        out: &mut *out,
        kind: OP_KEEP,
        count: 0,
        base_at: 0,
        now_at: 0,
    }
    .run();
    encode_blob(base.map(|base| base.tail.as_slice()), &now.tail, out);
    for i in 0..now.tail_cold.len() {
        match base {
            Some(base) if Arc::ptr_eq(&base.tail_cold[i], &now.tail_cold[i]) => {
                out.push(BLOB_SAME);
            }
            _ => encode_blob(
                base.map(|base| base.tail_cold[i].as_ref()),
                &now.tail_cold[i],
                out,
            ),
        }
    }
    match base {
        Some(base) if base.stash == now.stash => out.push(BLOB_SAME),
        _ => {
            out.push(BLOB_DIGEST);
            out.extend_from_slice(&now.stash.digest());
            put_varint(out, u64::from(now.stash.ids_end()));
        }
    }
}

/// The smallest form of `now` against `base` (see the module's grammar).
fn encode_blob(base: Option<&[u8]>, now: &[u8], out: &mut Vec<u8>) {
    let start = out.len();
    match base {
        Some(base) if base == now => {
            out.push(BLOB_SAME);
            return;
        }
        Some(base) if base.len() == now.len() => {
            out.push(BLOB_MASKED);
            put_masked(base, now, out);
        }
        Some(base) => {
            let prefix = base.iter().zip(now).take_while(|(a, b)| a == b).count();
            let room = base.len().min(now.len()) - prefix;
            let suffix = base
                .iter()
                .rev()
                .zip(now.iter().rev())
                .take(room)
                .take_while(|(a, b)| a == b)
                .count();
            let middle = &now[prefix..now.len() - suffix];
            out.push(BLOB_SPLICED);
            put_varint(out, prefix as u64);
            put_varint(out, suffix as u64);
            put_varint(out, middle.len() as u64);
            out.extend_from_slice(middle);
        }
        None => {}
    }
    let whole = 1 + varint_len(now.len() as u64) + now.len();
    if out.len() > start && out.len() - start <= whole {
        return;
    }
    out.truncate(start);
    out.push(BLOB_WHOLE);
    put_varint(out, now.len() as u64);
    out.extend_from_slice(now);
}

/// `now` XOR `base` (equal lengths) as masks and the non-zero bytes.
fn put_masked(base: &[u8], now: &[u8], out: &mut Vec<u8>) {
    let groups = now.len().div_ceil(8);
    let mask_at = out.len();
    out.resize(mask_at + groups.div_ceil(8), 0);
    for (group, (before, after)) in base.chunks(8).zip(now.chunks(8)).enumerate() {
        if before == after {
            continue;
        }
        out[mask_at + group / 8] |= 1 << (group % 8);
        let bytes_at = out.len();
        out.push(0);
        let mut bytes = 0u8;
        for (bit, (a, b)) in before.iter().zip(after).enumerate() {
            if a != b {
                bytes |= 1 << bit;
                out.push(a ^ b);
            }
        }
        out[bytes_at] = bytes;
    }
}

/// The entity list's walk: both lists by id, runs of one op kind written
/// as they end.
struct Walk<'a> {
    base: &'a [EntityPart],
    now: &'a [EntityPart],
    out: &'a mut Vec<u8>,
    /// The run in progress: its op, length and where it starts in each
    /// list.
    kind: u64,
    count: usize,
    base_at: usize,
    now_at: usize,
}

impl Walk<'_> {
    fn run(mut self) {
        let (mut i, mut j) = (0, 0);
        loop {
            match (self.base.get(i), self.now.get(j)) {
                (None, None) => break,
                (Some(before), Some(after)) if after.id < before.id => {
                    self.add(OP_NEW, i, j);
                    j += 1;
                }
                (Some(_), None) => {
                    self.add(OP_DROP, i, j);
                    i += 1;
                }
                (Some(before), Some(after)) if before.id < after.id => {
                    self.add(OP_DROP, i, j);
                    i += 1;
                }
                (None, Some(_)) => {
                    self.add(OP_NEW, i, j);
                    j += 1;
                }
                (Some(before), Some(after)) => {
                    let same_cold = cold_eq(&before.cold, &after.cold);
                    if before.hot == after.hot && same_cold {
                        self.add(OP_KEEP, i, j);
                    } else if before.hot.len() == after.hot.len() && same_cold {
                        self.add(OP_HOT_XOR, i, j);
                    } else if before.hot.len() == after.hot.len() {
                        self.add(OP_XOR, i, j);
                    } else if same_cold {
                        self.add(OP_HOT_BLOB, i, j);
                    } else {
                        self.add(OP_DROP, i, j);
                        self.add(OP_NEW, i + 1, j);
                    }
                    i += 1;
                    j += 1;
                }
            }
        }
        // The end keeps the rest of the baseline: a last run of keeps
        // needs no bytes.
        if self.kind != OP_KEEP {
            self.flush();
        }
        self.out.push(0);
    }

    fn add(&mut self, kind: u64, base_at: usize, now_at: usize) {
        if self.count > 0 && self.kind == kind {
            self.count += 1;
            return;
        }
        self.flush();
        (self.kind, self.count, self.base_at, self.now_at) = (kind, 1, base_at, now_at);
    }

    fn flush(&mut self) {
        if self.count == 0 {
            return;
        }
        put_varint(self.out, ((self.count as u64) << 3) | self.kind);
        match self.kind {
            OP_HOT_XOR => {
                let pairs = self.base[self.base_at..]
                    .iter()
                    .zip(&self.now[self.now_at..]);
                for (before, after) in pairs.take(self.count) {
                    put_masked(&before.hot, &after.hot, self.out);
                }
            }
            OP_XOR => {
                let pairs = self.base[self.base_at..]
                    .iter()
                    .zip(&self.now[self.now_at..]);
                for (before, after) in pairs.take(self.count) {
                    put_masked(&before.hot, &after.hot, self.out);
                    encode_blob(Some(&before.cold), &after.cold, self.out);
                }
            }
            OP_HOT_BLOB => {
                let pairs = self.base[self.base_at..]
                    .iter()
                    .zip(&self.now[self.now_at..]);
                for (before, after) in pairs.take(self.count) {
                    encode_blob(Some(&before.hot), &after.hot, self.out);
                }
            }
            OP_NEW => {
                for at in self.now_at..self.now_at + self.count {
                    let entity = &self.now[at];
                    let after = at
                        .checked_sub(1)
                        .map_or(0, |before| u64::from(self.now[before].id.0) + 1);
                    put_varint(self.out, u64::from(entity.id.0).saturating_sub(after));
                    put_varint(self.out, entity.hot.len() as u64);
                    self.out.extend_from_slice(&entity.hot);
                    put_varint(self.out, entity.cold.len() as u64);
                    self.out.extend_from_slice(&entity.cold);
                }
            }
            _ => {}
        }
        self.count = 0;
    }
}

// ---------------------------------------------------------------------------
// Decoding

/// Rebuilds the parts `delta` was encoded from against `base`: exactly, or
/// an error. Never panics.
pub fn decode(base: Option<&Parts>, delta: &[u8]) -> Result<Parts, CodecError> {
    let mut input = Reader {
        bytes: delta,
        at: 0,
    };
    let head = decode_blob(&mut input, base.map(|base| base.head.as_slice()), "head")?;
    let records: &[EntityPart] = base.map_or(&[], |base| &base.entities);
    let entities = decode_entities(&mut input, records)?;
    let tail = decode_blob(&mut input, base.map(|base| base.tail.as_slice()), "tail")?;
    let mut tail_cold: [Arc<[u8]>; 7] = std::array::from_fn(|_| Arc::from(&[][..]));
    for (i, slot) in tail_cold.iter_mut().enumerate() {
        *slot = decode_arc_blob(&mut input, base.map(|base| &base.tail_cold[i]), "tail cold")?;
    }
    let at = input.at;
    let stash = match (input.u8("stash")?, base) {
        (BLOB_SAME, Some(base)) => base.stash.clone(),
        (BLOB_DIGEST, _) => {
            let digest = input.array::<32>("stash digest")?;
            let ids_end =
                u32::try_from(input.varint("stash ids_end")?).map_err(|_| CodecError::Limit {
                    what: "stash ids_end",
                    at,
                })?;
            Stash::digest_only(digest, ids_end)
        }
        _ => return Err(CodecError::Invalid { what: "stash", at }),
    };
    let extra = delta.len() - input.at;
    if extra > 0 {
        return Err(CodecError::Trailing { extra });
    }
    Ok(Parts {
        head,
        entities,
        tail,
        tail_cold,
        stash,
    })
}

/// A checked cursor over received bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize, what: &'static str) -> Result<&'a [u8], CodecError> {
        let taken = self
            .at
            .checked_add(len)
            .and_then(|end| self.bytes.get(self.at..end))
            .ok_or(CodecError::Truncated { what, at: self.at })?;
        self.at += len;
        Ok(taken)
    }

    fn u8(&mut self, what: &'static str) -> Result<u8, CodecError> {
        let byte = *self
            .bytes
            .get(self.at)
            .ok_or(CodecError::Truncated { what, at: self.at })?;
        self.at += 1;
        Ok(byte)
    }

    fn array<const N: usize>(&mut self, what: &'static str) -> Result<[u8; N], CodecError> {
        let mut array = [0; N];
        array.copy_from_slice(self.take(N, what)?);
        Ok(array)
    }

    /// LEB128, at most 10 bytes, minimal (`wire::read_varint`).
    fn varint(&mut self, what: &'static str) -> Result<u64, CodecError> {
        let at = self.at;
        let (value, len) = wire::read_varint(&self.bytes[at..]).map_err(|error| match error {
            wire::VarintError::Truncated => CodecError::Truncated {
                what,
                at: self.bytes.len(),
            },
            wire::VarintError::Invalid => CodecError::Invalid { what, at },
        })?;
        self.at += len;
        Ok(value)
    }

    /// A varint that must be at most `limit`.
    fn bounded(&mut self, what: &'static str, limit: usize) -> Result<usize, CodecError> {
        let at = self.at;
        usize::try_from(self.varint(what)?)
            .ok()
            .filter(|&value| value <= limit)
            .ok_or(CodecError::Limit { what, at })
    }
}

fn decode_blob(
    input: &mut Reader,
    base: Option<&[u8]>,
    what: &'static str,
) -> Result<Vec<u8>, CodecError> {
    let at = input.at;
    let tag = input.u8(what)?;
    decode_blob_tag(tag, at, input, base, what)
}

fn decode_arc_blob(
    input: &mut Reader,
    base: Option<&Arc<[u8]>>,
    what: &'static str,
) -> Result<Arc<[u8]>, CodecError> {
    let at = input.at;
    let tag = input.u8(what)?;
    if tag == BLOB_SAME {
        return base.cloned().ok_or(CodecError::Invalid { what, at });
    }
    Ok(Arc::from(decode_blob_tag(
        tag,
        at,
        input,
        base.map(|b| b.as_ref()),
        what,
    )?))
}

fn decode_blob_tag(
    tag: u8,
    at: usize,
    input: &mut Reader,
    base: Option<&[u8]>,
    what: &'static str,
) -> Result<Vec<u8>, CodecError> {
    match (tag, base) {
        (BLOB_SAME, Some(base)) => Ok(base.to_vec()),
        (BLOB_MASKED, Some(base)) => take_masked(input, base, what),
        (BLOB_WHOLE, _) => {
            let len = input.bounded(what, input.bytes.len())?;
            Ok(input.take(len, what)?.to_vec())
        }
        (BLOB_SPLICED, Some(base)) => {
            let prefix = input.bounded(what, base.len())?;
            let suffix = input.bounded(what, base.len() - prefix)?;
            let len = input.bounded(what, input.bytes.len())?;
            let middle = input.take(len, what)?;
            let (head, rest) = base.split_at(prefix);
            let tail = rest.split_at(rest.len() - suffix).1;
            Ok([head, middle, tail].concat())
        }
        _ => Err(CodecError::Invalid { what, at }),
    }
}

/// `base` with a `masked` delta applied.
fn take_masked(input: &mut Reader, base: &[u8], what: &'static str) -> Result<Vec<u8>, CodecError> {
    let groups = base.len().div_ceil(8);
    let at = input.at;
    let mask = input.take(groups.div_ceil(8), what)?;
    // Bits past the last group are never set.
    if !groups.is_multiple_of(8) && mask.last().is_some_and(|last| last >> (groups % 8) != 0) {
        return Err(CodecError::Invalid { what, at });
    }
    let mut out = base.to_vec();
    for (index, &byte) in mask.iter().enumerate() {
        let mut set = byte;
        while set != 0 {
            let group = index * 8 + set.trailing_zeros() as usize;
            set &= set - 1;
            let start = group * 8;
            let len = (base.len() - start).min(8);
            let at = input.at;
            let bytes = input.u8(what)?;
            if bytes == 0 || (len < 8 && bytes >> len != 0) {
                return Err(CodecError::Invalid { what, at });
            }
            let mut set = bytes;
            while set != 0 {
                let offset = start + set.trailing_zeros() as usize;
                set &= set - 1;
                let at = input.at;
                let change = input.u8(what)?;
                match out.get_mut(offset) {
                    Some(byte) if change != 0 => *byte ^= change,
                    _ => return Err(CodecError::Invalid { what, at }),
                }
            }
        }
    }
    Ok(out)
}

fn decode_entities(input: &mut Reader, base: &[EntityPart]) -> Result<Vec<EntityPart>, CodecError> {
    let mut out: Vec<EntityPart> = Vec::with_capacity(base.len());
    let mut next = 0usize;
    loop {
        let at = input.at;
        let tag = input.varint("entity op")?;
        let (count, kind) = (tag >> 3, tag & 7);
        if count == 0 {
            if kind == OP_KEEP {
                break;
            }
            return Err(CodecError::Invalid {
                what: "empty entity op",
                at,
            });
        }
        if kind >= 6 {
            return Err(CodecError::Invalid {
                what: "entity op",
                at,
            });
        }
        if kind == OP_NEW {
            let room = MAX_ENTITIES - out.len();
            if count > room as u64 {
                return Err(CodecError::Limit {
                    what: "entities",
                    at,
                });
            }
            for _ in 0..count {
                let at = input.at;
                let after = out.last().map_or(0, |part| u64::from(part.id.0) + 1);
                let id = input
                    .varint("entity id")?
                    .checked_add(after)
                    .and_then(|id| u32::try_from(id).ok())
                    .ok_or(CodecError::Invalid {
                        what: "entity id",
                        at,
                    })?;
                let len_hot = input.bounded("record length", MAX_RECORD)?;
                let hot = input.take(len_hot, "record")?.to_vec();
                let len_cold = input.bounded("record length", MAX_RECORD - len_hot)?;
                let cold = Arc::from(input.take(len_cold, "record")?);
                push(
                    &mut out,
                    EntityPart {
                        id: SimId(id),
                        hot,
                        cold,
                    },
                    at,
                )?;
            }
            continue;
        }
        let records = usize::try_from(count)
            .ok()
            .and_then(|count| next.checked_add(count))
            .and_then(|end| base.get(next..end))
            .ok_or(CodecError::Invalid {
                what: "entity op past the baseline",
                at,
            })?;
        next += records.len();
        match kind {
            OP_KEEP => {
                for part in records {
                    push(&mut out, part.clone(), at)?;
                }
            }
            OP_HOT_XOR => {
                for part in records {
                    let at = input.at;
                    let hot = take_masked(input, &part.hot, "record")?;
                    push(
                        &mut out,
                        EntityPart {
                            id: part.id,
                            hot,
                            cold: part.cold.clone(),
                        },
                        at,
                    )?;
                }
            }
            OP_XOR => {
                for part in records {
                    let at = input.at;
                    let hot = take_masked(input, &part.hot, "record")?;
                    let cold = decode_arc_blob(input, Some(&part.cold), "record cold")?;
                    push(
                        &mut out,
                        EntityPart {
                            id: part.id,
                            hot,
                            cold,
                        },
                        at,
                    )?;
                }
            }
            OP_HOT_BLOB => {
                for part in records {
                    let at = input.at;
                    let hot = decode_blob(input, Some(&part.hot), "record")?;
                    push(
                        &mut out,
                        EntityPart {
                            id: part.id,
                            hot,
                            cold: part.cold.clone(),
                        },
                        at,
                    )?;
                }
            }
            _ => {}
        }
    }
    for part in &base[next..] {
        push(&mut out, part.clone(), input.at)?;
    }
    Ok(out)
}

/// Appends a record, keeping ids strictly rising and the count bounded.
fn push(out: &mut Vec<EntityPart>, part: EntityPart, at: usize) -> Result<(), CodecError> {
    if out.last().is_some_and(|last| last.id >= part.id) {
        return Err(CodecError::Invalid {
            what: "entity order",
            at,
        });
    }
    if out.len() >= MAX_ENTITIES {
        return Err(CodecError::Limit {
            what: "entities",
            at,
        });
    }
    out.push(part);
    Ok(())
}
