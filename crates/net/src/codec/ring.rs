//! Baselines: which snapshots each side keeps, so a frame's delta has a
//! baseline both ends hold. The host keeps one ring for every client and a
//! confirmation per client; a client keeps the snapshots it rebuilt.

use std::collections::VecDeque;

use super::{CodecError, FrameHeader, Rebuilt, SERVER_RING, Stepped, encode_frame, rebuild};

/// The host's last `SERVER_RING` stepped snapshots of the current epoch,
/// oldest first, shared by every client's frames.
#[derive(Debug, Clone)]
pub struct ServerRing {
    epoch: u16,
    held: VecDeque<Stepped>,
}

/// One client's newest confirmed snapshot (epoch, tick), which is its
/// baseline while the ring holds it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Confirmed(pub Option<(u16, u64)>);

impl ServerRing {
    pub fn new(epoch: u16) -> ServerRing {
        ServerRing {
            epoch,
            held: VecDeque::with_capacity(SERVER_RING),
        }
    }

    pub fn epoch(&self) -> u16 {
        self.epoch
    }

    /// A module install: a new epoch, nothing held from the old one.
    pub fn begin_epoch(&mut self, epoch: u16) {
        self.epoch = epoch;
        self.held.clear();
    }

    /// Keeps a newly stepped snapshot, forgetting the oldest beyond
    /// `SERVER_RING` (and any not older than it: time went back, as after
    /// a restore). The newest before it goes too if it was never read
    /// (`Stepped::is_split`): no frame was made of it, so no client holds
    /// it as a baseline.
    pub fn push(&mut self, stepped: Stepped) {
        while self
            .held
            .back()
            .is_some_and(|last| last.tick >= stepped.tick || !last.is_split())
        {
            self.held.pop_back();
        }
        if self.held.len() == SERVER_RING {
            self.held.pop_front();
        }
        self.held.push_back(stepped);
    }

    pub fn newest(&self) -> Option<&Stepped> {
        self.held.back()
    }

    /// Snapshots held.
    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    pub fn get(&self, tick: u64) -> Option<&Stepped> {
        let at = self
            .held
            .binary_search_by_key(&tick, |held| held.tick)
            .ok()?;
        self.held.get(at)
    }

    /// Takes a client's confirmation of (epoch, tick) if it is of this
    /// epoch, of a snapshot this ring stepped, and newer than the one it
    /// had; whether it did.
    pub fn confirm(&self, confirmed: &mut Confirmed, epoch: u16, tick: u64) -> bool {
        let newer = match confirmed.0 {
            Some((had_epoch, had)) if had_epoch == self.epoch => tick > had,
            _ => true,
        };
        let known = self.newest().is_some_and(|newest| tick <= newest.tick);
        if epoch == self.epoch && known && newer {
            *confirmed = Confirmed(Some((epoch, tick)));
            return true;
        }
        false
    }

    /// The baseline for a client: its confirmed snapshot, if of this epoch
    /// and still held; `None` means a keyframe.
    pub fn baseline(&self, confirmed: &Confirmed) -> Option<&Stepped> {
        match confirmed.0 {
            Some((epoch, tick)) if epoch == self.epoch => self.get(tick),
            _ => None,
        }
    }

    /// Appends the frame of the newest snapshot for a client that
    /// confirmed `confirmed`; false if nothing was stepped yet.
    pub fn frame(&self, confirmed: &Confirmed, out: &mut Vec<u8>) -> bool {
        let Some(now) = self.newest() else {
            return false;
        };
        let base = self.baseline(confirmed).filter(|base| base.tick < now.tick);
        encode_frame(self.epoch, now, base, out);
        true
    }
}

/// A client's last `capacity` rebuilt snapshots of the current epoch (a
/// network's `CLIENT_RING`), by tick: the baselines its frames are rebuilt
/// against.
#[derive(Debug, Clone)]
pub struct ClientRing {
    epoch: u16,
    held: VecDeque<Rebuilt>,
    capacity: usize,
}

impl ClientRing {
    pub fn new(epoch: u16, capacity: usize) -> ClientRing {
        ClientRing {
            epoch,
            held: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn epoch(&self) -> u16 {
        self.epoch
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// A module install: a new epoch, starting from `first` (the install's
    /// result, computed here) when there is one.
    pub fn begin_epoch(&mut self, epoch: u16, first: Option<Rebuilt>) {
        self.epoch = epoch;
        self.held.clear();
        self.held.extend(first);
    }

    /// The newest snapshot held: what the client confirms.
    pub fn newest(&self) -> Option<&Rebuilt> {
        self.held.back()
    }

    /// Snapshots held.
    pub fn len(&self) -> usize {
        self.held.len()
    }

    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }

    /// The newest snapshot held at or before `tick`, and the oldest after
    /// it: the two a presentation blends between at `tick`.
    pub fn around(&self, tick: u64) -> (Option<&Rebuilt>, Option<&Rebuilt>) {
        let after = self.held.partition_point(|held| held.tick <= tick);
        let before = after.checked_sub(1).and_then(|at| self.held.get(at));
        (before, self.held.get(after))
    }

    pub fn get(&self, tick: u64) -> Option<&Rebuilt> {
        let at = self
            .held
            .binary_search_by_key(&tick, |held| held.tick)
            .ok()?;
        self.held.get(at)
    }

    /// Rebuilds a received frame against the baseline it names and keeps
    /// it. A tick already held is returned as it is (a duplicate datagram).
    pub fn receive(&mut self, frame: &[u8]) -> Result<&Rebuilt, CodecError> {
        let (header, delta) = FrameHeader::read(frame)?;
        if header.epoch != self.epoch {
            return Err(CodecError::Epoch {
                frame: header.epoch,
                here: self.epoch,
            });
        }
        let at = match self
            .held
            .binary_search_by_key(&header.tick, |held| held.tick)
        {
            Ok(held) => return Ok(&self.held[held]),
            Err(0) if self.held.len() == self.capacity => {
                return Err(CodecError::Stale { tick: header.tick });
            }
            Err(at) => at,
        };
        let base = match header.baseline {
            None => None,
            Some(tick) => Some(&self.get(tick).ok_or(CodecError::NoBaseline { tick })?.parts),
        };
        let rebuilt = rebuild(&header, delta, base)?;
        self.held.insert(at, rebuilt);
        // Over-full only if it was full, when `at` is past the oldest.
        let at = if self.held.len() > self.capacity {
            self.held.pop_front();
            at - 1
        } else {
            at
        };
        Ok(&self.held[at])
    }
}
