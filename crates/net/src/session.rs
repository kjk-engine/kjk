//! The game over a link (design sections 5 to 10, D5–D10, D13, D19–D20):
//! the host's `Server` and a player's `Client`, sans-IO like the rest of
//! the crate. Neither owns a clock or a socket: time comes in as
//! `NetTime`, datagrams go out through `poll_transmit`, and the caller
//! moves them over any `Transport`.
//!
//! The host's world is the only truth. The server hands the authority
//! loop each tick's commands and sends every client exact deltas of what
//! it stepped; a client rebuilds those snapshots bit for bit, predicts
//! its own seat ahead of the host by restoring the newest one and
//! stepping again, and draws everyone else between two of them. Module
//! installs run on every machine from the same announcement, so their
//! result is computed, not sent.
//!
//! - `msg`: the session's messages and frames;
//! - `server`: seats, the input buffer, slack, snapshots, installs, the
//!   host's save copies to its players (I30 P17);
//! - `client`: joining, rebuilding, the clocks, prediction, presentation;
//! - `clock`: the client's two timelines and how they are steered;
//! - `probes`: whole sessions over `io::SimNet` in virtual time, the
//!   evidence `kjk net session` runs;
//! - `sessions`: the sessions it plays on the determinism world and the
//!   checks every session's report must pass.

pub mod client;
pub mod clock;
pub mod msg;
#[cfg(feature = "probes")]
pub mod probes;
pub mod server;
#[cfg(feature = "probes")]
pub mod sessions;

use crate::NetTime;
use crate::handshake::Refusal;
use crate::link::Link;
use crate::wire::frame;

/// Packets a disconnect goes out in: nothing acks it, so it is repeated.
pub const DISCONNECTS: usize = 3;

/// The packets that tell the peer the session ended and why: a disconnect
/// frame in each of `DISCONNECTS` packets.
pub(crate) fn goodbye(link: &mut Link, now: NetTime, why: &Refusal) -> Vec<Vec<u8>> {
    let mut payload = Vec::new();
    why.encode(&mut payload);
    (0..DISCONNECTS)
        .filter_map(|_| {
            link.queue_unreliable(frame::DISCONNECT, &payload).ok()?;
            link.poll_transmit(now)
        })
        .collect()
}

pub use client::{
    Cause, Client, ClientConfig, ClientEvent, ClientStats, ClockView, Frame, Memory, Mismatch,
};
pub use msg::{
    CopyAssembly, InstallRequest, LevelRequest, MAX_SAVE_PARTS, SAVE_PART, SavePart, Welcome,
};
pub use server::{
    CopyRefusal, CopySent, MAX_QUEUED_COPIES, RECONNECT_US, Server, ServerConfig, ServerEvent,
    ServerStats, TAKEOVER_QUIET_US,
};
