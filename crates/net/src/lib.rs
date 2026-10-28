//! The network layer: how the host's world reaches every player's
//! machine exactly, and their commands reach the host. The design, its
//! decisions and why are in `docs/NET.md`.
//!
//! Sans-IO: nothing here reads a clock or a socket. Endpoints are state
//! machines fed with bytes and a time (`NetTime`) and asked what to send;
//! a `Transport` moves datagrams (a UDP socket, in-process channels, or a
//! virtual network in virtual time). So a whole session, loss and all, can
//! run in one thread from a seed, and runs the same every time.
//!
//! Layers, lowest first:
//! - `wire`: the packet header, frames and varints; decoders over `&[u8]`
//!   that return errors and never panic;
//! - `link`: one peer: sequence numbers, acks, RTT, jitter and loss, the
//!   reliable ordered channel with slicing, keepalive and timeout;
//! - `handshake`: hello, challenge, connect, accept or a typed refusal;
//! - `codec`: snapshots as exact deltas against a snapshot the peer has;
//! - `session`: the game over a link: the host's `Server` and a player's
//!   `Client` (seats, commands, clocks, prediction, installs);
//! - `netem`: latency, jitter, loss, duplication, reordering, a rate cap;
//! - `io`: the transports (`Loopback`, `SimNet`, `Udp`, `Steam`, and
//!   `Socket`, a session's UDP and Steam together);
//! - `probes`: the layer's evidence, run by `kjk net probes`.

pub mod codec;
pub mod handshake;
pub mod io;
pub mod link;
pub mod netem;
#[cfg(feature = "probes")]
pub mod probes;
pub mod session;
pub mod wire;

use std::net::SocketAddr;

/// The most a datagram carries (UDP payload): it fits IPv6's minimum MTU
/// and common tunnels, so no path is probed.
pub const MAX_DATAGRAM: usize = 1200;

/// A moment in microseconds on a monotonic clock the caller keeps (the
/// wall clock over a socket, virtual time over `io::SimNet`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct NetTime(pub u64);

impl NetTime {
    pub fn from_millis(ms: u64) -> NetTime {
        NetTime(ms * 1000)
    }

    /// Microseconds from `earlier` to this, 0 if it is later.
    pub fn since(self, earlier: NetTime) -> u64 {
        self.0.saturating_sub(earlier.0)
    }

    pub fn plus(self, micros: u64) -> NetTime {
        NetTime(self.0.saturating_add(micros))
    }
}

/// Who a datagram comes from or goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Peer {
    /// A socket address.
    Addr(SocketAddr),
    /// An endpoint in this process (`io::Loopback`, `io::SimNet`).
    Local(u32),
    /// A player's Steam ID (`io::Steam`).
    Steam(u64),
}

impl std::fmt::Display for Peer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Peer::Addr(address) => address.fmt(f),
            Peer::Local(index) => write!(f, "local {index}"),
            Peer::Steam(id) => f.write_str(&steam_address(*id)),
        }
    }
}

/// The prefix of a Steam ID written as an address.
const STEAM_PREFIX: &str = "steam:";

/// A Steam ID as an address a player types or Steam hands over.
pub fn steam_address(id: u64) -> String {
    format!("{STEAM_PREFIX}{id}")
}

/// The Steam ID a `steam:<id>` address names.
pub fn parse_steam(address: &str) -> Option<u64> {
    address
        .strip_prefix(STEAM_PREFIX)?
        .parse()
        .ok()
        .filter(|&id| id != 0)
}

/// One datagram received, stamped with when it arrived (as close to the
/// socket as the transport can: jitter is measured from it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Datagram {
    pub from: Peer,
    pub at: NetTime,
    pub bytes: Vec<u8>,
}

/// Moves datagrams. Sending never blocks and may drop (UDP's contract);
/// receiving returns what has arrived by now, oldest first.
pub trait Transport {
    /// This end's own address.
    fn local(&self) -> Peer;
    fn send(&mut self, to: Peer, bytes: &[u8]);
    fn recv(&mut self) -> Option<Datagram>;
}
