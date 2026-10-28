//! Transports: `Udp` (a socket), `Steam` (Steam's relayed messages, with
//! the `steam` feature), `Socket` (a session's UDP and Steam together),
//! `Loopback` (two ends of in-process channels) and `SimNet` (a virtual
//! network in virtual time).
//!
//! All keep UDP's contract (`Transport`): sending never blocks and may
//! drop (a datagram over `MAX_DATAGRAM`, or to nobody), and received
//! datagrams carry the time they arrived. None reads a clock of its own:
//! `Udp` and `Loopback` stamp arrivals with a clock function their owner
//! supplies (or, a `Loopback`, with a time passed to `recv_at`); `SimNet`'s
//! time moves only when its owner calls `advance_to`.

#[cfg(feature = "steam")]
pub mod steam;
mod udp;

#[cfg(feature = "steam")]
pub use steam::{Steam, SteamStats};
pub use udp::{Udp, UdpStats};

use crate::netem::{Conditions, Shaper, ShaperStats};
use crate::{Datagram, MAX_DATAGRAM, NetTime, Peer, Transport};
use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A clock a `Loopback` end stamps arrivals with (the owner's monotonic
/// clock, or a virtual one).
pub type Clock = Arc<dyn Fn() -> NetTime + Send + Sync>;

/// How long a socket with Steam beside it waits at most: Steam's messages
/// wake nobody, so they are looked for this often.
#[cfg(feature = "steam")]
const STEAM_POLL: Duration = Duration::from_millis(1);

/// A session's socket: UDP, Steam's messages, or both at once (a host that
/// takes players by address and through Steam alike). A datagram leaves by
/// its peer's kind; receiving takes from each in turn.
pub struct Socket {
    udp: Option<Udp>,
    #[cfg(feature = "steam")]
    steam: Option<Steam>,
    /// Steam is read first next (they take turns).
    #[cfg(feature = "steam")]
    steam_next: bool,
}

impl Socket {
    /// A socket by address alone.
    pub fn udp(udp: Udp) -> Socket {
        Socket {
            udp: Some(udp),
            #[cfg(feature = "steam")]
            steam: None,
            #[cfg(feature = "steam")]
            steam_next: false,
        }
    }

    /// Steam alone (a player joining a host through Steam).
    #[cfg(feature = "steam")]
    pub fn steam(steam: Steam) -> Socket {
        Socket {
            udp: None,
            steam: Some(steam),
            steam_next: true,
        }
    }

    /// This socket with Steam beside its UDP.
    #[cfg(feature = "steam")]
    pub fn with_steam(self, steam: Steam) -> Socket {
        Socket {
            steam: Some(steam),
            ..self
        }
    }

    /// The UDP end, where it has one.
    pub fn udp_end(&self) -> Option<&Udp> {
        self.udp.as_ref()
    }

    /// The Steam end, where it has one.
    #[cfg(feature = "steam")]
    pub fn steam_end(&self) -> Option<&Steam> {
        self.steam.as_ref()
    }

    /// The UDP receive buffer the OS gave, where it was asked.
    pub fn recv_buffer(&self) -> Option<usize> {
        self.udp.as_ref()?.recv_buffer()
    }

    fn has_steam(&self) -> bool {
        #[cfg(feature = "steam")]
        return self.steam.is_some();
        #[cfg(not(feature = "steam"))]
        false
    }

    /// Waits until a datagram is ready on the UDP end or `timeout` passes;
    /// with Steam beside it, `STEAM_POLL` at most.
    pub fn wait(&self, timeout: Duration) {
        #[cfg(feature = "steam")]
        let timeout = if self.has_steam() {
            timeout.min(STEAM_POLL)
        } else {
            timeout
        };
        match &self.udp {
            Some(udp) => udp.wait(timeout),
            None => std::thread::sleep(timeout),
        }
    }

    fn recv_udp(&mut self) -> Option<Datagram> {
        self.udp.as_mut()?.recv()
    }

    #[cfg(feature = "steam")]
    fn recv_steam(&mut self) -> Option<Datagram> {
        self.steam.as_mut()?.recv()
    }
}

impl Transport for Socket {
    fn local(&self) -> Peer {
        #[cfg(feature = "steam")]
        if let (None, Some(steam)) = (&self.udp, &self.steam) {
            return steam.local();
        }
        self.udp.as_ref().map_or(Peer::Local(0), Udp::local)
    }

    fn send(&mut self, to: Peer, bytes: &[u8]) {
        match to {
            #[cfg(feature = "steam")]
            Peer::Steam(_) => {
                if let Some(steam) = &mut self.steam {
                    steam.send(to, bytes);
                }
            }
            _ => {
                if let Some(udp) = &mut self.udp {
                    udp.send(to, bytes);
                }
            }
        }
    }

    fn recv(&mut self) -> Option<Datagram> {
        if !self.has_steam() {
            return self.recv_udp();
        }
        #[cfg(feature = "steam")]
        {
            let steam_first = self.steam_next;
            self.steam_next = !steam_first;
            if steam_first {
                return self.recv_steam().or_else(|| self.recv_udp());
            }
            self.recv_udp().or_else(|| self.recv_steam())
        }
        #[cfg(not(feature = "steam"))]
        None
    }
}

/// One end of an in-process pair: what one end sends, the other
/// receives, in order and without loss. The two ends may live on
/// different threads (single player: the authority thread and the game),
/// and an end may be shared (`Sync`: its receiver sits behind a lock only
/// its owner takes).
pub struct Loopback {
    local: Peer,
    remote: Peer,
    to_remote: Sender<Vec<u8>>,
    from_remote: Mutex<Receiver<Vec<u8>>>,
    clock: Clock,
    dropped: u64,
}

impl Loopback {
    /// Two connected ends, `Peer::Local(0)` and `Peer::Local(1)`, both
    /// stamping arrivals with `clock`.
    pub fn pair(clock: Clock) -> (Loopback, Loopback) {
        let (to_b, from_a) = channel();
        let (to_a, from_b) = channel();
        let a = Loopback {
            local: Peer::Local(0),
            remote: Peer::Local(1),
            to_remote: to_b,
            from_remote: Mutex::new(from_b),
            clock: clock.clone(),
            dropped: 0,
        };
        let b = Loopback {
            local: Peer::Local(1),
            remote: Peer::Local(0),
            to_remote: to_a,
            from_remote: Mutex::new(from_a),
            clock,
            dropped: 0,
        };
        (a, b)
    }

    /// The other end's address.
    pub fn remote(&self) -> Peer {
        self.remote
    }

    /// The next datagram that arrived, stamped `now` (for a caller that
    /// keeps its own time).
    pub fn recv_at(&mut self, now: NetTime) -> Option<Datagram> {
        let receiver = self.from_remote.get_mut().ok()?;
        let bytes = receiver.try_recv().ok()?;
        Some(Datagram {
            from: self.remote,
            at: now,
            bytes,
        })
    }

    /// Datagrams dropped on send: too long, to another address, or after
    /// the other end was dropped.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

impl Transport for Loopback {
    fn local(&self) -> Peer {
        self.local
    }

    fn send(&mut self, to: Peer, bytes: &[u8]) {
        let sent = to == self.remote
            && bytes.len() <= MAX_DATAGRAM
            && self.to_remote.send(bytes.to_vec()).is_ok();
        self.dropped += u64::from(!sent);
    }

    fn recv(&mut self) -> Option<Datagram> {
        let now = (self.clock)();
        self.recv_at(now)
    }
}

/// A virtual network: endpoints `Peer::Local(0..n)`, one `Shaper` per
/// direction (a perfect one unless `set_path` says otherwise), and a
/// virtual time that moves only by `advance_to`. Single-threaded and
/// deterministic: the same seed and the same calls give the same
/// arrivals, byte for byte.
pub struct SimNet {
    now: NetTime,
    seed: u64,
    inboxes: Vec<VecDeque<Datagram>>,
    /// Directions `(from, to)`, in a fixed order, so equal arrival times
    /// on two paths resolve the same way every run.
    paths: BTreeMap<(u32, u32), Shaper>,
    dropped: u64,
}

impl SimNet {
    /// An empty network at time 0. `seed` seeds every path whose
    /// conditions do not name their own seed.
    pub fn new(seed: u64) -> SimNet {
        SimNet {
            now: NetTime(0),
            seed,
            inboxes: Vec::new(),
            paths: BTreeMap::new(),
            dropped: 0,
        }
    }

    /// Adds an endpoint and returns its address.
    pub fn endpoint(&mut self) -> Peer {
        self.inboxes.push(VecDeque::new());
        Peer::Local(self.inboxes.len() as u32 - 1)
    }

    /// The network's virtual time.
    pub fn now(&self) -> NetTime {
        self.now
    }

    fn index(&self, peer: Peer) -> Option<u32> {
        match peer {
            Peer::Local(index) if (index as usize) < self.inboxes.len() => Some(index),
            _ => None,
        }
    }

    /// Sets the conditions from `from` to `to` (one direction). A seed of
    /// 0 in `conditions` takes the network's seed; each direction draws
    /// from its own stream either way. Datagrams already in flight on the
    /// path are kept.
    pub fn set_path(&mut self, from: Peer, to: Peer, conditions: Conditions) {
        let (Some(from), Some(to)) = (self.index(from), self.index(to)) else {
            return;
        };
        let mut conditions = conditions;
        if conditions.seed == 0 {
            conditions.seed = self.seed;
        }
        let fresh = Shaper::new(conditions, stream(from, to));
        match self.paths.get_mut(&(from, to)) {
            Some(path) => {
                let previous = std::mem::replace(path, fresh);
                path.adopt(previous);
            }
            None => {
                self.paths.insert((from, to), fresh);
            }
        }
    }

    /// Sets the same conditions both ways between `a` and `b`.
    pub fn set_link(&mut self, a: Peer, b: Peer, conditions: Conditions) {
        self.set_path(a, b, conditions);
        self.set_path(b, a, conditions);
    }

    /// Sends a datagram from `from` at the network's current time.
    pub fn send(&mut self, from: Peer, to: Peer, bytes: &[u8]) {
        let (Some(from), Some(to_index)) = (self.index(from), self.index(to)) else {
            self.dropped += 1;
            return;
        };
        if bytes.len() > MAX_DATAGRAM {
            self.dropped += 1;
            return;
        }
        let seed = self.seed;
        let now = self.now;
        self.paths
            .entry((from, to_index))
            .or_insert_with(|| {
                let conditions = Conditions {
                    seed,
                    ..Conditions::default()
                };
                Shaper::new(conditions, stream(from, to_index))
            })
            .send(now, to, bytes);
    }

    /// When the next datagram in flight arrives.
    pub fn next_arrival(&self) -> Option<NetTime> {
        self.paths.values().filter_map(Shaper::next_due).min()
    }

    /// Moves time to `t` (never back), delivering every datagram due by
    /// then to its inbox, stamped with its arrival time, in arrival order.
    pub fn advance_to(&mut self, t: NetTime) {
        self.now = self.now.max(t);
        loop {
            let next = self
                .paths
                .iter()
                .filter_map(|(key, shaper)| shaper.next_due().map(|due| (due, *key)))
                .min();
            let Some((due, (from, to))) = next.filter(|(due, _)| *due <= self.now) else {
                break;
            };
            let Some(flight) = self.paths.get_mut(&(from, to)).and_then(|s| s.pop_due(due)) else {
                continue;
            };
            self.inboxes[to as usize].push_back(Datagram {
                from: Peer::Local(from),
                at: flight.arrival,
                bytes: flight.bytes,
            });
        }
    }

    /// The next datagram in `at`'s inbox.
    pub fn recv(&mut self, at: Peer) -> Option<Datagram> {
        let index = self.index(at)?;
        self.inboxes[index as usize].pop_front()
    }

    /// `at`'s end of the network as a `Transport`.
    pub fn port(&mut self, at: Peer) -> Port<'_> {
        Port { net: self, at }
    }

    /// A direction's counts, once anything was sent on it.
    pub fn path_stats(&self, from: Peer, to: Peer) -> Option<ShaperStats> {
        let key = (self.index(from)?, self.index(to)?);
        self.paths.get(&key).map(Shaper::stats)
    }

    /// Datagrams dropped on send: too long, or to no endpoint.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// Each direction's own stream, so two paths with one seed differ.
fn stream(from: u32, to: u32) -> u64 {
    (u64::from(from) << 32 | u64::from(to)).wrapping_mul(0x9e37_79b9_7f4a_7c15)
}

/// One endpoint of a `SimNet`, borrowed as a `Transport`.
pub struct Port<'a> {
    net: &'a mut SimNet,
    at: Peer,
}

impl Transport for Port<'_> {
    fn local(&self) -> Peer {
        self.at
    }

    fn send(&mut self, to: Peer, bytes: &[u8]) {
        self.net.send(self.at, to, bytes);
    }

    fn recv(&mut self) -> Option<Datagram> {
        self.net.recv(self.at)
    }
}
