//! Snapshot frames: the header before a delta, the delta packed (`pack`),
//! the check that proves a rebuild exact, and the rebuild itself
//! (`rebuild`), which only returns a snapshot whose bytes hash to the
//! host's.

use std::borrow::Cow;
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use sim::{Parts, Snapshot, SnapshotHash};

use crate::worker::{Queued, Worker};

use super::{CodecError, Reader, decode, encode, put_varint, wire};

/// The first 8 bytes of a snapshot's BLAKE3 hash: what a frame is checked
/// against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Check(pub [u8; 8]);

impl Check {
    pub fn of(hash: &SnapshotHash) -> Check {
        let mut check = [0; 8];
        check.copy_from_slice(&hash.0[..8]);
        Check(check)
    }
}

/// What precedes a snapshot frame's packed delta (`pack`):
///
/// ```text
/// epoch  u16, little-endian
/// tick   varint
/// back   varint: ticks back to the baseline; 0 for a keyframe
/// check  8 bytes
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// The module install the snapshot belongs to.
    pub epoch: u16,
    pub tick: u64,
    /// The baseline's tick; `None` for a keyframe.
    pub baseline: Option<u64>,
    pub check: Check,
}

impl FrameHeader {
    /// Appends the header. A baseline must be older than the tick (one
    /// that is not reads back as a keyframe, and its delta fails).
    pub fn write(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.epoch.to_le_bytes());
        put_varint(out, self.tick);
        put_varint(
            out,
            self.baseline
                .map_or(0, |base| self.tick.saturating_sub(base)),
        );
        out.extend_from_slice(&self.check.0);
    }

    /// The header and the packed delta after it (`unpack`).
    pub fn read(frame: &[u8]) -> Result<(FrameHeader, &[u8]), CodecError> {
        let mut input = Reader {
            bytes: frame,
            at: 0,
        };
        let epoch = u16::from_le_bytes(input.array("epoch")?);
        let tick = input.varint("tick")?;
        let at = input.at;
        let back = input.varint("baseline")?;
        let baseline = match back {
            0 => None,
            back => Some(tick.checked_sub(back).ok_or(CodecError::Invalid {
                what: "baseline",
                at,
            })?),
        };
        let check = Check(input.array("check")?);
        let header = FrameHeader {
            epoch,
            tick,
            baseline,
            check,
        };
        Ok((header, &frame[input.at..]))
    }
}

/// A snapshot the host stepped, as it keeps it for deltas: its parts
/// (shared by every client's frames) and its check. Made from a shared
/// snapshot (`shared`), it is split only when first read: a host whose
/// clients take a frame every few ticks splits only those.
#[derive(Debug, Clone)]
pub struct Stepped {
    pub tick: u64,
    split: Arc<Split>,
}

/// Per-entity `(id, avatar_seat, position)` extracted for `seats > 4` interest culling.
pub type EntityPosition = (u32, Option<u8>, [f32; 3]);
pub(crate) type EntityPositions = Arc<[EntityPosition]>;

fn extract_positions(snapshot: &Snapshot) -> Option<EntityPositions> {
    if snapshot.seats() <= 4 {
        return None;
    }
    Some(
        snapshot
            .entities
            .iter()
            .map(|entity| {
                let seat = match entity.kind {
                    sim::Kind::Avatar(seat) => Some(seat.0),
                    _ => None,
                };
                (entity.id.0, seat, entity.position)
            })
            .collect::<Vec<_>>()
            .into(),
    )
}

/// A stepped snapshot's parts and check, made once, from the snapshot
/// kept until then. A snapshot made beside (`Stepped::beside`) takes its
/// parts and check from the net check thread, with the delta that thread
/// packed. It keeps the snapshot only in case that thread's job failed.
#[derive(Debug)]
struct Split {
    snapshot: Mutex<Option<Arc<Snapshot>>>,
    done: OnceLock<(Arc<Parts>, Check)>,
    beside: Mutex<Option<Queued<Made>>>,
    packed: OnceLock<Packed>,
    positions: Option<EntityPositions>,
}

/// What a thread made of a stepped snapshot (`Stepped::beside`).
#[derive(Debug)]
struct Made {
    parts: Parts,
    check: Check,
    packed: Option<Packed>,
}

/// A delta packed beside (`pack`), and the snapshot it is a delta from.
#[derive(Debug)]
struct Packed {
    base: Weak<Split>,
    bytes: Vec<u8>,
}

static CHECKS: Worker = Worker::new("net check");

impl Split {
    fn new(
        snapshot: Option<Arc<Snapshot>>,
        done: OnceLock<(Arc<Parts>, Check)>,
        positions: Option<EntityPositions>,
    ) -> Split {
        Split {
            snapshot: Mutex::new(snapshot),
            done,
            beside: Mutex::new(None),
            packed: OnceLock::new(),
            positions,
        }
    }
}

impl Stepped {
    pub fn new(snapshot: &Snapshot) -> Stepped {
        let positions = extract_positions(snapshot);
        let parts = snapshot.split();
        let check = Check::of(&parts.hash());
        Stepped {
            tick: snapshot.tick,
            split: Arc::new(Split::new(
                None,
                OnceLock::from((Arc::new(parts), check)),
                positions,
            )),
        }
    }

    /// `new`, with the snapshot's hash (`Snapshot::hash`) its check is made
    /// from: a host's step thread splits each tick for its players and its
    /// `hashes.txt` at once (issue 229).
    pub fn new_hashed(snapshot: &Snapshot) -> (Stepped, SnapshotHash) {
        let positions = extract_positions(snapshot);
        let parts = snapshot.split();
        let hash = parts.hash();
        let check = Check::of(&hash);
        let stepped = Stepped {
            tick: snapshot.tick,
            split: Arc::new(Split::new(
                None,
                OnceLock::from((Arc::new(parts), check)),
                positions,
            )),
        };
        (stepped, hash)
    }

    /// A shared snapshot, split when its parts or check are first read.
    pub fn shared(snapshot: Arc<Snapshot>) -> Stepped {
        let positions = extract_positions(&snapshot);
        Stepped {
            tick: snapshot.tick,
            split: Arc::new(Split::new(Some(snapshot), OnceLock::new(), positions)),
        }
    }

    /// A shared snapshot that the net check thread splits and hashes. When
    /// `base` is older, that thread also packs the delta from `base`. The first
    /// read of the parts or the check waits for the thread if it has not
    /// finished. `encode_frame` then uses the packed delta when its
    /// baseline is `base`. The bytes are the same as those made at the
    /// first read. A solo host's step thread makes one of these for each
    /// tick its own client checks, so neither the step thread nor the
    /// frame splits it. If the thread cannot start or the job panics, the
    /// first read splits the snapshot itself.
    pub fn beside(snapshot: Arc<Snapshot>, base: Option<&Stepped>) -> Stepped {
        let positions = extract_positions(&snapshot);
        let base = base.filter(|base| base.tick < snapshot.tick).cloned();
        let kept = snapshot.clone();
        let thread = CHECKS.run(move || {
            let parts = snapshot.split();
            let check = Check::of(&parts.hash());
            let packed = base.map(|base| {
                let mut delta = Vec::new();
                encode(Some(base.parts()), &parts, &mut delta);
                Packed {
                    base: Arc::downgrade(&base.split),
                    bytes: pack(&delta),
                }
            });
            Made {
                parts,
                check,
                packed,
            }
        });
        let split = Split::new(Some(kept.clone()), OnceLock::new(), positions);
        *split.beside.lock().unwrap_or_else(PoisonError::into_inner) = thread;
        Stepped {
            tick: kept.tick,
            split: Arc::new(split),
        }
    }

    fn done(&self) -> &(Arc<Parts>, Check) {
        self.split.done.get_or_init(|| {
            let thread = (self.split.beside.lock())
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(made) = thread.and_then(Queued::wait) {
                self.split
                    .snapshot
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .take();
                if let Some(packed) = made.packed {
                    let _ = self.split.packed.set(packed);
                }
                return (Arc::new(made.parts), made.check);
            }
            let snapshot = self
                .split
                .snapshot
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take()
                .expect("a stepped snapshot is kept until it is split");
            let parts = snapshot.split();
            let check = Check::of(&parts.hash());
            (Arc::new(parts), check)
        })
    }

    pub fn parts(&self) -> &Parts {
        &self.done().0
    }

    /// Its parts, shared.
    pub fn shared_parts(&self) -> &Arc<Parts> {
        &self.done().0
    }

    pub fn check(&self) -> Check {
        self.done().1
    }

    /// Whether its parts were made (it was read: sent, or a baseline).
    pub fn is_split(&self) -> bool {
        self.split.done.get().is_some()
    }

    /// Per-entity `(id, avatar_seat, position)` snapshot metadata when `seats > 4`.
    pub fn positions(&self) -> Option<&[EntityPosition]> {
        self.split.positions.as_deref()
    }

    /// The delta packed beside (`beside`), if it is a delta from `base`.
    fn packed_against(&self, base: &Stepped) -> Option<&[u8]> {
        self.done();
        let packed = self.split.packed.get()?;
        std::ptr::eq(packed.base.as_ptr(), Arc::as_ptr(&base.split)).then_some(&packed.bytes[..])
    }
}

/// Appends a frame of `now_parts` in `epoch` against `base` (`(tick, parts)`,
/// or `None` for a keyframe), computing `Check` directly from `now_parts` so
/// filtered `Parts` verify cleanly with zero wire-format changes.
pub fn encode_parts_frame(
    epoch: u16,
    tick: u64,
    now_parts: &Parts,
    base: Option<(u64, &Parts)>,
    out: &mut Vec<u8>,
) {
    let base = base.filter(|(bt, _)| *bt < tick);
    FrameHeader {
        epoch,
        tick,
        baseline: base.map(|(bt, _)| bt),
        check: Check::of(&now_parts.hash()),
    }
    .write(out);
    let mut delta = Vec::new();
    encode(base.map(|(_, parts)| parts), now_parts, &mut delta);
    out.extend_from_slice(&pack(&delta));
}

/// Appends a frame of `now` in `epoch` against `base` (`None`, or a
/// baseline not older than `now`: a keyframe) to `out`.
pub fn encode_frame(epoch: u16, now: &Stepped, base: Option<&Stepped>, out: &mut Vec<u8>) {
    debug_assert!(base.is_none_or(|base| base.tick < now.tick));
    let base = base.filter(|base| base.tick < now.tick);
    FrameHeader {
        epoch,
        tick: now.tick,
        baseline: base.map(|base| base.tick),
        check: now.check(),
    }
    .write(out);
    if let Some(packed) = base.and_then(|base| now.packed_against(base)) {
        out.extend_from_slice(packed);
        return;
    }
    let mut delta = Vec::new();
    encode(base.map(Stepped::parts), now.parts(), &mut delta);
    out.extend_from_slice(&pack(&delta));
}

/// A packed delta's first byte: the delta as it is, or LZ4-compressed.
const RAW: u8 = 0;
const LZ4: u8 = 1;
/// The longest delta a compressed one may unpack to (a keyframe of a full
/// module is a few hundred KB).
pub const MAX_DELTA: usize = 32 << 20;

/// A delta packed for the wire: one byte, then the delta as it is or, when
/// smaller, `varint(uncompressed_len) + lz4_flex::block`.
pub fn pack(delta: &[u8]) -> Vec<u8> {
    if delta.len() >= 16 {
        let compressed = lz4_flex::block::compress(delta);
        let mut header = Vec::with_capacity(11);
        header.push(LZ4);
        put_varint(&mut header, delta.len() as u64);
        if header.len() + compressed.len() < 1 + delta.len() {
            header.extend_from_slice(&compressed);
            return header;
        }
    }
    let mut out = Vec::with_capacity(1 + delta.len());
    out.push(RAW);
    out.extend_from_slice(delta);
    out
}

/// The delta a packed one holds (`pack`), unpacked at most to `MAX_DELTA`.
pub fn unpack(packed: &[u8]) -> Result<Cow<'_, [u8]>, CodecError> {
    match packed.split_first() {
        None => Err(CodecError::Truncated {
            what: "packing",
            at: 0,
        }),
        Some((&RAW, delta)) => Ok(Cow::Borrowed(delta)),
        Some((&LZ4, rest)) => {
            let (len_u64, consumed) = wire::read_varint(rest).map_err(|_| CodecError::Invalid {
                what: "lz4 delta",
                at: 1,
            })?;
            let body = &rest[consumed..];
            // Bound uncompressed length both by `MAX_DELTA` and the physical LZ4 limit (~260x).
            let max_possible = body.len().saturating_mul(260).saturating_add(32);
            let Some(len) = usize::try_from(len_u64)
                .ok()
                .filter(|&n| n <= MAX_DELTA && n <= max_possible)
            else {
                return Err(CodecError::Limit {
                    what: "lz4 delta",
                    at: 1,
                });
            };
            let out = lz4_flex::block::decompress(body, len).map_err(|_| CodecError::Invalid {
                what: "lz4 delta",
                at: 1 + consumed,
            })?;
            if out.len() != len {
                return Err(CodecError::Invalid {
                    what: "lz4 delta",
                    at: 1 + consumed,
                });
            }
            Ok(Cow::Owned(out))
        }
        Some(_) => Err(CodecError::Invalid {
            what: "packing",
            at: 0,
        }),
    }
}

/// A snapshot rebuilt from a frame and checked: its parts (a later frame's
/// baseline) and the snapshot (what prediction restores from), shared
/// with what presents it: a snapshot is large, and a client hands each on
/// several times (issue 229).
#[derive(Debug, Clone)]
pub struct Rebuilt {
    pub tick: u64,
    pub parts: Arc<Parts>,
    pub snapshot: Arc<Snapshot>,
}

impl Rebuilt {
    /// A snapshot this side has itself (an install's result): the first
    /// baseline of an epoch.
    pub fn of(snapshot: Snapshot) -> Rebuilt {
        Rebuilt {
            tick: snapshot.tick,
            parts: Arc::new(snapshot.split()),
            snapshot: Arc::new(snapshot),
        }
    }
}

/// Rebuilds a frame's snapshot from its packed delta against `base`, which
/// must be the snapshot of `header.baseline` (none for a keyframe), and
/// checks it: its hash, then `Snapshot::join`, the tick, and that the
/// records' ids are the snapshot's entities'.
pub fn rebuild(
    header: &FrameHeader,
    packed: &[u8],
    base: Option<&Parts>,
) -> Result<Rebuilt, CodecError> {
    if let (Some(tick), None) = (header.baseline, base) {
        return Err(CodecError::NoBaseline { tick });
    }
    let base = base.filter(|_| header.baseline.is_some());
    let parts = decode(base, &unpack(packed)?)?;
    if Check::of(&parts.hash()) != header.check {
        return Err(CodecError::Check { tick: header.tick });
    }
    let snapshot = Snapshot::join(&parts).map_err(CodecError::Snapshot)?;
    let ids_match = snapshot.entities.len() == parts.entities.len()
        && snapshot
            .entities
            .iter()
            .zip(&parts.entities)
            .all(|(entity, part)| entity.id == part.id);
    if snapshot.tick != header.tick || !ids_match {
        return Err(CodecError::Invalid {
            what: "frame against its snapshot",
            at: 0,
        });
    }
    Ok(Rebuilt {
        tick: header.tick,
        parts: Arc::new(parts),
        snapshot: Arc::new(snapshot),
    })
}

/// `FrameHeader::read` and `rebuild` in one, for a caller that knows the
/// baseline.
pub fn decode_frame(frame: &[u8], base: Option<&Parts>) -> Result<Rebuilt, CodecError> {
    let (header, delta) = FrameHeader::read(frame)?;
    rebuild(&header, delta, base)
}
