//! Snapshot frames: the header before a delta, the delta packed (`pack`),
//! the check that proves a rebuild exact, and the rebuild itself
//! (`rebuild`), which only returns a snapshot whose bytes hash to the
//! host's.

use std::borrow::Cow;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use sim::{Parts, Snapshot, SnapshotHash};

use super::{CodecError, Reader, decode, encode, put_varint};

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

/// A stepped snapshot's parts and check, made once, from the snapshot
/// kept until then.
#[derive(Debug)]
struct Split {
    snapshot: Mutex<Option<Arc<Snapshot>>>,
    done: OnceLock<(Arc<Parts>, Check)>,
}

impl Stepped {
    pub fn new(snapshot: &Snapshot) -> Stepped {
        let parts = snapshot.split();
        let check = Check::of(&parts.hash());
        Stepped {
            tick: snapshot.tick,
            split: Arc::new(Split {
                snapshot: Mutex::new(None),
                done: OnceLock::from((Arc::new(parts), check)),
            }),
        }
    }

    /// A shared snapshot, split when its parts or check are first read.
    pub fn shared(snapshot: Arc<Snapshot>) -> Stepped {
        Stepped {
            tick: snapshot.tick,
            split: Arc::new(Split {
                snapshot: Mutex::new(Some(snapshot)),
                done: OnceLock::new(),
            }),
        }
    }

    fn done(&self) -> &(Arc<Parts>, Check) {
        self.split.done.get_or_init(|| {
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
    let mut delta = Vec::new();
    encode(base.map(Stepped::parts), now.parts(), &mut delta);
    out.extend_from_slice(&pack(&delta));
}

/// A packed delta's first byte: the delta as it is, or deflated.
const RAW: u8 = 0;
const DEFLATED: u8 = 1;
/// The deflate level: 9 packs no smaller on snapshots, 1 about 8% larger.
const LEVEL: u8 = 6;
/// The longest delta a deflated one may unpack to (a keyframe of a full
/// module is a few hundred KB).
pub const MAX_DELTA: usize = 32 << 20;

/// A delta packed for the wire: one byte, then the delta as it is or, when
/// smaller, deflated (raw DEFLATE, RFC 1951). A delta's records of one kind
/// change alike (their masks and the bytes of the same fields), which
/// halves a fight's deltas.
pub fn pack(delta: &[u8]) -> Vec<u8> {
    let deflated = miniz_oxide::deflate::compress_to_vec(delta, LEVEL);
    let (kind, body) = if deflated.len() < delta.len() {
        (DEFLATED, &deflated[..])
    } else {
        (RAW, delta)
    };
    let mut out = Vec::with_capacity(1 + body.len());
    out.push(kind);
    out.extend_from_slice(body);
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
        Some((&DEFLATED, body)) => {
            miniz_oxide::inflate::decompress_to_vec_with_limit(body, MAX_DELTA)
                .map(Cow::Owned)
                .map_err(|_| CodecError::Invalid {
                    what: "deflated delta",
                    at: 1,
                })
        }
        Some(_) => Err(CodecError::Invalid {
            what: "packing",
            at: 0,
        }),
    }
}

/// A snapshot rebuilt from a frame and checked: its parts (a later frame's
/// baseline) and the snapshot (what prediction restores from).
#[derive(Debug, Clone)]
pub struct Rebuilt {
    pub tick: u64,
    pub parts: Parts,
    pub snapshot: Snapshot,
}

impl Rebuilt {
    /// A snapshot this side has itself (an install's result): the first
    /// baseline of an epoch.
    pub fn of(snapshot: Snapshot) -> Rebuilt {
        Rebuilt {
            tick: snapshot.tick,
            parts: snapshot.split(),
            snapshot,
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
            .all(|(entity, (id, _))| entity.id == *id);
    if snapshot.tick != header.tick || !ids_match {
        return Err(CodecError::Invalid {
            what: "frame against its snapshot",
            at: 0,
        });
    }
    Ok(Rebuilt {
        tick: header.tick,
        parts,
        snapshot,
    })
}

/// `FrameHeader::read` and `rebuild` in one, for a caller that knows the
/// baseline.
pub fn decode_frame(frame: &[u8], base: Option<&Parts>) -> Result<Rebuilt, CodecError> {
    let (header, delta) = FrameHeader::read(frame)?;
    rebuild(&header, delta, base)
}
