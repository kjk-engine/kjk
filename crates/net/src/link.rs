//! One peer's connection: packet numbers and acks, RTT, jitter and loss,
//! the reliable ordered channel, keepalive and timeout (design sections
//! 4 and 9). Sans-IO: `receive` takes a datagram and the time it arrived,
//! `poll_transmit` gives the next datagram to send, `next_deadline` says
//! when to ask again.
//!
//! **Packets and acks.** Every packet has the next number (`u16`,
//! wrapping) and repeats which of the peer's packets this side has
//! received: the newest and a bit for each of the 32 before it, with how
//! long ago the newest arrived. So a lost ack costs nothing, and the
//! round trip is measured without shared clocks: the time from sending a
//! packet to its first ack, minus the delay the peer reports. A packet
//! that carries frames asks for an ack; one is sent within
//! `Config::max_ack_delay_us` even when there is nothing else to say.
//!
//! **RTT and loss.** RTT is smoothed (1/8 per sample), with its mean
//! deviation as jitter (1/4 per sample) and the minimum ever seen. A
//! packet not acked within `RTT + max(4 × jitter, MIN_MARGIN)` plus the
//! peer's ack delay is declared lost, once a packet sent after it has
//! been acked: without that evidence the acks may only be late (the
//! return path lost a few in a row), and nothing is declared. An ack that
//! comes after all still counts (a late ack). The loss estimate is a
//! moving average (1/32 per packet) over packets acked or declared lost.
//!
//! **Jumps.** A packet numbered further ahead of the newest received than
//! the ack bits reach (`JUMP_WINDOW`) is refused unless the packet before
//! it was one such too and this one follows it closely: after a long
//! outage the peer's next two packets get through, while one stray or
//! forged number (the session id is no proof, D12) cannot make every real
//! packet after it stale.
//!
//! **The reliable channel.** Messages (at most `MAX_MESSAGE` bytes) are
//! numbered with a `u16` id and cut into slices of `SLICE` bytes, "part k
//! of n". A slice rides in packets until one carrying it is acked. It is
//! sent again when that packet is declared lost or after
//! `RTT + max(min(RTT / 4, 4 × jitter), MIN_MARGIN)` plus the ack delay
//! (the design's 1.25 × RTT, whichever first), resends before new
//! slices. The receiver keeps slices by message, delivers messages
//! strictly in id order, each exactly once, and ignores what it already
//! has.
//!
//! Limits, the same on both sides so a conforming sender never trips the
//! receiver's: the sender's window runs from its oldest message not yet
//! wholly acked to its newest, at most `MAX_IN_FLIGHT` messages and
//! `MAX_WINDOW_BYTES`, and `send_reliable` refuses beyond (the receiver
//! can only hold slices of messages in that window). The receiver
//! accepts ids up to `MAX_IN_FLIGHT` ahead of the next one it delivers
//! and holds at most `MAX_WINDOW_BYTES` in `MAX_HELD_SLICES` slices; a
//! peer beyond that, or one whose slices contradict each other, breaks
//! the protocol, and the link refuses everything after (`broken`).
//!
//! **Pacing.** Slices spend a byte budget (`Config::budget`, default
//! 256 KiB/s, or unlimited), refilled continuously with a burst of 20 ms
//! (at least two datagrams), so a keyframe never crowds out the
//! session's own frames. At most `MAX_SLICE_PACKETS` packets carrying
//! slices are out at once, so every one of them is inside the ack bits of
//! the peer's next ack.
//!
//! **Unreliable frames** (`queue_unreliable`) are the session's
//! latest-wins payloads (snapshots, commands): sent in the next packet,
//! never again. `take_acked` tells the session which of its packets
//! arrived.
//!
//! **Keepalive and timeout.** A packet goes out after `KEEPALIVE_US` with
//! nothing sent; the link has timed out after `TIMEOUT_US` without a
//! valid packet from the peer (`timed_out`), and the session drops it.

use crate::wire::{
    self, Ack, Frame, Frames, HEADER_LEN, Header, MAX_MESSAGE, SLICE, Slice, WireError, frame,
};
use crate::{MAX_DATAGRAM, NetTime};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;

/// The most reliable messages in the sender's window.
pub const MAX_IN_FLIGHT: usize = 256;
/// The most bytes of reliable messages in the sender's window, and held
/// by the receiver.
pub const MAX_WINDOW_BYTES: usize = 2 * MAX_MESSAGE;
/// The most slices the receiver holds (the window's bytes in whole
/// slices, plus a last short slice per message).
pub const MAX_HELD_SLICES: usize = MAX_WINDOW_BYTES / SLICE + MAX_IN_FLIGHT;
/// The default byte budget for reliable slices, bytes per second.
pub const DEFAULT_BUDGET: u64 = 256 * 1024;
/// The longest a link stays silent before it sends a keepalive.
pub const KEEPALIVE_US: u64 = 100_000;
/// The longest a link waits for a valid packet before it has timed out.
pub const TIMEOUT_US: u64 = 5_000_000;
/// The default longest a link holds an ack it owes.
pub const DEFAULT_ACK_DELAY_US: u64 = 5_000;
/// The least margin over RTT before a packet is declared lost or its
/// slices are sent again.
pub const MIN_MARGIN_US: u64 = 5_000;
/// The RTT assumed before the first sample.
pub const INITIAL_RTT_US: u64 = 100_000;
/// The most packets carrying slices that are neither acked nor given up
/// on (one fewer than an ack's reach of 33 packets).
pub const MAX_SLICE_PACKETS: usize = 32;
/// How far ahead of the newest packet received a number is taken without
/// a second packet to confirm it: the ack bits' reach.
pub const JUMP_WINDOW: u32 = 32;
/// Sent-packet records kept; a packet this many sends old that is still
/// unacked is declared lost.
const SENT_RING: usize = 256;
/// Newly acked packet numbers kept for `take_acked`; older ones are
/// forgotten if the session does not take them.
const ACKED_KEEP: usize = 1024;
/// A second, the longest refill the budget counts at once.
const SECOND_US: u64 = 1_000_000;

/// How a link paces and acks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// Bytes per second of reliable slices; `None` is unlimited (loopback).
    pub budget: Option<u64>,
    /// The longest an owed ack waits for a packet to ride in. Both sides
    /// should use the same: a sender allows the peer this much before
    /// resending.
    pub max_ack_delay_us: u64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            budget: Some(DEFAULT_BUDGET),
            max_ack_delay_us: DEFAULT_ACK_DELAY_US,
        }
    }
}

impl Config {
    /// In-process: no budget.
    pub fn loopback() -> Config {
        Config {
            budget: None,
            ..Config::default()
        }
    }
}

/// Why a datagram was not taken, or a message not queued.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkError {
    /// Bytes that do not decode.
    Wire(WireError),
    /// Another session's packet (dropped silently).
    Session { got: u64 },
    /// A packet received before (counted, dropped).
    Duplicate,
    /// A packet older than the ack bits reach: whether it is a duplicate
    /// cannot be told, so it is dropped.
    Stale,
    /// A packet numbered further ahead of the newest than the ack bits
    /// reach, not (yet) confirmed by a second one close after it: a stray
    /// or forged number would otherwise make every real packet stale.
    Jump,
    /// The peer broke the protocol (the reason); the link takes nothing
    /// more.
    Broken(&'static str),
    /// A message over `MAX_MESSAGE`, or an unreliable frame that does not
    /// fit a packet.
    TooLarge { len: usize },
    /// The sender's window is full (`MAX_IN_FLIGHT` or
    /// `MAX_WINDOW_BYTES`): try again once earlier messages are acked.
    WindowFull,
    /// `frame::RELIABLE` is the link's own frame type.
    FrameType,
}

impl From<WireError> for LinkError {
    fn from(error: WireError) -> LinkError {
        LinkError::Wire(error)
    }
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LinkError::Wire(error) => write!(f, "{error}"),
            LinkError::Session { got } => write!(f, "packet of another session ({got:016x})"),
            LinkError::Duplicate => write!(f, "duplicate packet"),
            LinkError::Stale => write!(f, "packet older than the ack window"),
            LinkError::Jump => write!(f, "packet number far ahead, unconfirmed"),
            LinkError::Broken(reason) => write!(f, "the peer broke the protocol: {reason}"),
            LinkError::TooLarge { len } => write!(f, "{len} bytes do not fit"),
            LinkError::WindowFull => write!(f, "the reliable window is full"),
            LinkError::FrameType => write!(f, "the reliable frame type is the link's"),
        }
    }
}

impl std::error::Error for LinkError {}

/// A link's measures and counts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Smoothed round trip, microseconds.
    pub rtt_us: u64,
    /// The smallest round trip sampled (0 before the first sample).
    pub rtt_min_us: u64,
    /// Mean deviation of the round trip, microseconds.
    pub jitter_us: u64,
    /// Recent loss, parts per million of packets resolved.
    pub loss_ppm: u64,
    /// Round trips sampled.
    pub rtt_samples: u64,
    /// Packets sent, keepalives and acks included.
    pub packets_sent: u64,
    /// Valid packets taken (duplicates and stale ones not).
    pub packets_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    /// Packets received twice.
    pub duplicates: u64,
    /// Packets too old to place.
    pub stale: u64,
    /// Packets numbered far ahead and refused until a second confirmed the
    /// jump.
    pub jumps: u64,
    /// Our packets declared lost.
    pub declared_lost: u64,
    /// Our packets acked after they were declared lost.
    pub late_acks: u64,
    /// Slices sent, first times and resends.
    pub slices_sent: u64,
    /// Bytes of message data in those slices.
    pub slice_bytes_sent: u64,
    /// Slices sent again.
    pub resends: u64,
    /// Slices received that were already held or delivered.
    pub duplicate_slices: u64,
    /// Reliable messages queued by `send_reliable`.
    pub messages_sent: u64,
    /// The peer's reliable messages delivered, and their bytes.
    pub messages_delivered: u64,
    pub message_bytes_delivered: u64,
}

/// A packet taken by `receive`: its number and the frames that are the
/// session's (every frame but the link's own reliable slices).
#[derive(Debug, Clone, Copy)]
pub struct Received<'a> {
    pub seq: u16,
    body: &'a [u8],
}

impl<'a> Received<'a> {
    /// The session's frames, in packet order. The packet was checked
    /// whole before it was taken, so none is malformed.
    pub fn frames(&self) -> impl Iterator<Item = Frame<'a>> + use<'a> {
        Frames::new(self.body)
            .map_while(Result::ok)
            .filter(|frame| frame.kind != frame::RELIABLE)
    }
}

/// A slice of one of our messages: message id and part.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SliceRef {
    id: u16,
    part: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SentState {
    #[default]
    Empty,
    InFlight,
    Acked,
    Lost,
}

/// A packet we sent, kept until it is acked or declared lost (and a
/// while after, so a late ack still counts).
#[derive(Debug, Clone, Default)]
struct Sent {
    seq: u16,
    at: NetTime,
    state: SentState,
    /// Its slices were queued to be sent again.
    requeued: bool,
    /// It counts toward `MAX_SLICE_PACKETS`.
    holds_slot: bool,
    slices: Vec<SliceRef>,
}

#[derive(Debug, Clone, Copy, Default)]
struct PartState {
    acked: bool,
    /// Waiting in the resend queue.
    queued: bool,
    /// The packet that last carried it.
    last_seq: Option<u16>,
}

/// One of our reliable messages in the window.
#[derive(Debug)]
struct Outgoing {
    data: Vec<u8>,
    parts: Vec<PartState>,
    acked: u32,
}

impl Outgoing {
    fn count(&self) -> u32 {
        self.parts.len() as u32
    }

    fn slice(&self, id: u16, part: u32) -> Slice<'_> {
        let start = part as usize * SLICE;
        let end = (start + SLICE).min(self.data.len());
        Slice {
            id,
            part,
            count: self.count(),
            data: &self.data[start..end],
        }
    }
}

/// A message being reassembled.
#[derive(Debug)]
struct Incoming {
    count: u32,
    parts: BTreeMap<u32, Vec<u8>>,
}

/// The round-trip estimator, in TCP's scaled integers.
#[derive(Debug, Clone, Copy)]
struct Rtt {
    /// Smoothed RTT × 8.
    srtt8: u64,
    /// Mean deviation × 4.
    jitter4: u64,
    min: u64,
    samples: u64,
}

impl Rtt {
    fn new() -> Rtt {
        Rtt {
            srtt8: INITIAL_RTT_US * 8,
            jitter4: INITIAL_RTT_US / 2 * 4,
            min: 0,
            samples: 0,
        }
    }

    fn srtt(&self) -> u64 {
        self.srtt8 / 8
    }

    fn jitter(&self) -> u64 {
        self.jitter4 / 4
    }

    fn sample(&mut self, rtt: u64) {
        if self.samples == 0 {
            self.srtt8 = rtt * 8;
            self.jitter4 = rtt / 2 * 4;
            self.min = rtt;
        } else {
            let deviation = rtt.abs_diff(self.srtt());
            self.jitter4 = self.jitter4 - self.jitter4 / 4 + deviation;
            self.srtt8 = self.srtt8 - self.srtt8 / 8 + rtt;
            self.min = self.min.min(rtt);
        }
        self.samples += 1;
    }
}

/// The byte budget for slices: a continuous refill with a small burst.
#[derive(Debug, Clone, Copy)]
struct Budget {
    /// Bytes per second; `None` is unlimited.
    rate: Option<u64>,
    /// Credit in millionths of a byte; sending may take it below zero.
    credit: i64,
    at: NetTime,
}

impl Budget {
    fn new(rate: Option<u64>, now: NetTime) -> Budget {
        let mut budget = Budget {
            rate: rate.map(|rate| rate.max(1)),
            credit: 0,
            at: now,
        };
        budget.credit = budget.cap();
        budget
    }

    /// The burst: 20 ms of the rate, at least two datagrams.
    fn cap(&self) -> i64 {
        let bytes = self
            .rate
            .map_or(0, |rate| (rate / 50).max(2 * MAX_DATAGRAM as u64));
        (bytes as i64).saturating_mul(1_000_000)
    }

    fn refill(&mut self, now: NetTime) {
        if let Some(rate) = self.rate {
            let elapsed = now.since(self.at).min(SECOND_US);
            let gained = (elapsed as i64).saturating_mul(rate as i64);
            self.credit = self.credit.saturating_add(gained).min(self.cap());
        }
        self.at = self.at.max(now);
    }

    fn open(&self) -> bool {
        self.rate.is_none() || self.credit > 0
    }

    fn spend(&mut self, bytes: usize) {
        if self.rate.is_some() {
            self.credit -= bytes as i64 * 1_000_000;
        }
    }

    /// When the budget opens again.
    fn opens_at(&self) -> NetTime {
        match self.rate {
            Some(rate) if self.credit <= 0 => {
                let wait = (self.credit.unsigned_abs()).div_ceil(rate) + 1;
                self.at.plus(wait)
            }
            _ => self.at,
        }
    }
}

/// Which of the peer's packets we have: the newest and 32 bits behind it.
#[derive(Debug, Clone, Copy)]
struct Received32 {
    newest: Option<u16>,
    bits: u32,
    newest_at: NetTime,
}

/// Where a packet number falls against what was received.
enum Arrival {
    /// Newer than the newest, by this much.
    Newer(u32),
    /// Older than the newest and not seen: its bit.
    Older(u32),
    Duplicate,
    Stale,
}

impl Received32 {
    fn place(&self, seq: u16) -> Arrival {
        let Some(newest) = self.newest else {
            return Arrival::Newer(0);
        };
        let ahead = i32::from(seq.wrapping_sub(newest) as i16);
        match ahead {
            1.. => Arrival::Newer(ahead as u32),
            0 => Arrival::Duplicate,
            _ => {
                let bit = (-ahead - 1) as u32;
                if bit >= 32 {
                    Arrival::Stale
                } else if self.bits & (1 << bit) != 0 {
                    Arrival::Duplicate
                } else {
                    Arrival::Older(bit)
                }
            }
        }
    }

    fn record(&mut self, seq: u16, arrival: &Arrival, now: NetTime) {
        match *arrival {
            Arrival::Newer(ahead) => {
                self.bits = match (self.newest, ahead) {
                    (None, _) | (_, 33..) => 0,
                    (Some(_), _) => ((u64::from(self.bits) << ahead) | 1 << (ahead - 1)) as u32,
                };
                self.newest = Some(seq);
                self.newest_at = now;
            }
            Arrival::Older(bit) => self.bits |= 1 << bit,
            Arrival::Duplicate | Arrival::Stale => {}
        }
    }

    fn ack(&self, now: NetTime) -> Option<Ack> {
        let seq = self.newest?;
        Some(Ack {
            seq,
            bits: self.bits,
            delay: (now.since(self.newest_at) / 100).min(u64::from(u16::MAX)) as u16,
        })
    }
}

/// One peer's connection state (see the module docs).
pub struct Link {
    session: u64,
    config: Config,
    /// The peer runs in this process (`in_process`): its silence is a stall
    /// of this process, so the link does not time out.
    in_process: bool,
    next_seq: u16,
    sent: Vec<Sent>,
    /// The oldest packet number that may still be in flight.
    oldest: u16,
    slice_packets: usize,
    /// The newest of our packets acked so far.
    newest_acked: Option<u16>,
    received: Received32,
    ack_owed_since: Option<NetTime>,
    last_sent: NetTime,
    last_received: NetTime,
    rtt: Rtt,
    loss_ppm: u64,
    budget: Budget,

    /// Encoded unreliable frames waiting for the next packet, and their
    /// lengths.
    unreliable: Vec<u8>,
    unreliable_lens: VecDeque<usize>,

    next_id: u16,
    /// Our messages from the oldest not wholly acked; the front's id is
    /// `next_id - window.len()`.
    window: VecDeque<Outgoing>,
    window_bytes: usize,
    /// The next slice never sent (id, part); id `next_id` means none.
    fresh: SliceRef,
    resend: VecDeque<SliceRef>,

    /// The next message id to deliver.
    deliver_id: u16,
    /// Messages being reassembled, by id from `deliver_id`.
    incoming: VecDeque<Option<Incoming>>,
    held_bytes: usize,
    held_slices: usize,
    delivered: VecDeque<Vec<u8>>,

    acked: VecDeque<u16>,
    broken: Option<&'static str>,
    /// A packet number far ahead, waiting for a second to confirm it.
    jump: Option<u16>,
    stats: Stats,
}

impl Link {
    /// A link for `session` with the default config, created at `now`.
    pub fn new(session: u64, now: NetTime) -> Link {
        Link::with_config(session, now, Config::default())
    }

    /// A link for `session` with `config`, created at `now`.
    pub fn with_config(session: u64, now: NetTime, config: Config) -> Link {
        Link {
            session,
            config,
            next_seq: 0,
            sent: vec![Sent::default(); SENT_RING],
            oldest: 0,
            slice_packets: 0,
            newest_acked: None,
            received: Received32 {
                newest: None,
                bits: 0,
                newest_at: now,
            },
            ack_owed_since: None,
            last_sent: now,
            last_received: now,
            in_process: false,
            rtt: Rtt::new(),
            loss_ppm: 0,
            budget: Budget::new(config.budget, now),
            unreliable: Vec::new(),
            unreliable_lens: VecDeque::new(),
            next_id: 0,
            window: VecDeque::new(),
            window_bytes: 0,
            fresh: SliceRef { id: 0, part: 0 },
            resend: VecDeque::new(),
            deliver_id: 0,
            incoming: VecDeque::new(),
            held_bytes: 0,
            held_slices: 0,
            delivered: VecDeque::new(),
            acked: VecDeque::new(),
            broken: None,
            jump: None,
            stats: Stats::default(),
        }
    }

    /// The session id every packet carries.
    pub fn session(&self) -> u64 {
        self.session
    }

    /// How this link paces and acks.
    pub fn config(&self) -> Config {
        self.config
    }

    /// The measures and counts so far.
    pub fn stats(&self) -> Stats {
        Stats {
            rtt_us: self.rtt.srtt(),
            rtt_min_us: self.rtt.min,
            jitter_us: self.rtt.jitter(),
            loss_ppm: self.loss_ppm,
            rtt_samples: self.rtt.samples,
            ..self.stats
        }
    }

    /// Why the peer broke the protocol, once it has.
    pub fn broken(&self) -> Option<&'static str> {
        self.broken
    }

    /// How long since the peer's last packet was taken (or the link was
    /// made).
    pub fn quiet_us(&self, now: NetTime) -> u64 {
        now.since(self.last_received)
    }

    /// No valid packet from the peer for `TIMEOUT_US` (never for one in
    /// this process).
    pub fn timed_out(&self, now: NetTime) -> bool {
        !self.in_process && now.since(self.last_received) >= TIMEOUT_US
    }

    /// The peer is in this process (a host's own player over a loopback):
    /// both sides stop together when the process stalls (a long frame, a
    /// pipeline warm-up), so silence never means it is gone; it ends only
    /// by leaving or breaking the protocol.
    pub fn set_in_process(&mut self) {
        self.in_process = true;
    }

    /// Our messages not yet wholly acked (with those before them).
    pub fn reliable_in_flight(&self) -> usize {
        self.window.len()
    }

    /// Bytes of our messages in the window.
    pub fn window_bytes(&self) -> usize {
        self.window_bytes
    }

    /// Bytes of the peer's messages held for reassembly.
    pub fn held_bytes(&self) -> usize {
        self.held_bytes
    }

    // -----------------------------------------------------------------
    // Sending.

    /// Queues a reliable message; it is delivered to the peer in order,
    /// exactly once. Returns its id.
    pub fn send_reliable(&mut self, data: &[u8]) -> Result<u16, LinkError> {
        if data.len() > MAX_MESSAGE {
            return Err(LinkError::TooLarge { len: data.len() });
        }
        if self.window.len() >= MAX_IN_FLIGHT || self.window_bytes + data.len() > MAX_WINDOW_BYTES {
            return Err(LinkError::WindowFull);
        }
        let count = data.len().div_ceil(SLICE).max(1);
        self.window.push_back(Outgoing {
            data: data.to_vec(),
            parts: vec![PartState::default(); count],
            acked: 0,
        });
        self.window_bytes += data.len();
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.stats.messages_sent += 1;
        Ok(id)
    }

    /// Queues a latest-wins frame of the session's (`frame::SNAPSHOT`,
    /// `frame::COMMANDS`, …) for the next packet. It is never sent again.
    pub fn queue_unreliable(&mut self, kind: u8, payload: &[u8]) -> Result<(), LinkError> {
        if kind == frame::RELIABLE {
            return Err(LinkError::FrameType);
        }
        let len = wire::frame_len(payload.len());
        if HEADER_LEN + len > MAX_DATAGRAM {
            return Err(LinkError::TooLarge { len });
        }
        wire::put_frame(&mut self.unreliable, kind, payload);
        self.unreliable_lens.push_back(len);
        Ok(())
    }

    /// The next packet to send at `now`, if one is due. Call until `None`.
    pub fn poll_transmit(&mut self, now: NetTime) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        self.poll_transmit_into(now, &mut out).then_some(out)
    }

    /// `poll_transmit` into a buffer the caller reuses: `out` is cleared
    /// and, if a packet is due, filled with it (and `true` returned).
    pub fn poll_transmit_into(&mut self, now: NetTime, out: &mut Vec<u8>) -> bool {
        out.clear();
        if self.broken.is_some() {
            return false;
        }
        self.expire(now);
        self.budget.refill(now);
        self.prune_resend();
        let ack_due = self
            .ack_owed_since
            .is_some_and(|since| now.since(since) >= self.config.max_ack_delay_us);
        let keepalive_due = now.since(self.last_sent) >= KEEPALIVE_US;
        let due =
            !self.unreliable_lens.is_empty() || self.slices_ready() || ack_due || keepalive_due;
        if !due {
            return false;
        }
        self.build(now, out);
        true
    }

    /// Whether a slice may go now: one waiting, budget, a packet slot.
    fn slices_ready(&self) -> bool {
        self.next_slice().is_some() && self.budget.open() && self.slice_packets < MAX_SLICE_PACKETS
    }

    /// The slice to send next: resends first, then the first never sent.
    fn next_slice(&self) -> Option<SliceRef> {
        if let Some(&slice) = self.resend.front() {
            return Some(slice);
        }
        let index = self.index(self.fresh.id)?;
        (self.fresh.part < self.window[index].count()).then_some(self.fresh)
    }

    /// Where message `id` is in the window.
    fn index(&self, id: u16) -> Option<usize> {
        let front = self.next_id.wrapping_sub(self.window.len() as u16);
        let index = id.wrapping_sub(front) as usize;
        (index < self.window.len()).then_some(index)
    }

    fn part(&mut self, slice: SliceRef) -> Option<&mut PartState> {
        let index = self.index(slice.id)?;
        self.window[index].parts.get_mut(slice.part as usize)
    }

    /// Drops resend entries that no longer need sending (acked since, or
    /// their message retired), so the queue's front is live.
    fn prune_resend(&mut self) {
        while let Some(&slice) = self.resend.front() {
            match self.part(slice) {
                Some(part) if !part.acked => break,
                Some(part) => part.queued = false,
                None => {}
            }
            self.resend.pop_front();
        }
    }

    fn build(&mut self, now: NetTime, out: &mut Vec<u8>) {
        let seq = self.next_seq;
        self.next_seq = seq.wrapping_add(1);
        let slot = seq as usize % SENT_RING;
        if self.sent[slot].state == SentState::InFlight {
            let old = self.sent[slot].seq;
            self.declare_lost(old);
        }
        let header = Header {
            session: self.session,
            seq,
            ack: self.received.ack(now),
        };
        header.encode(out);

        let mut taken = 0;
        while let Some(&len) = self.unreliable_lens.front() {
            if out.len() + len > MAX_DATAGRAM {
                break;
            }
            out.extend_from_slice(&self.unreliable[taken..taken + len]);
            taken += len;
            self.unreliable_lens.pop_front();
        }
        self.unreliable.drain(..taken);

        let mut slices = std::mem::take(&mut self.sent[slot].slices);
        slices.clear();
        if self.slice_packets < MAX_SLICE_PACKETS {
            self.put_slices(seq, out, &mut slices);
        }
        let holds_slot = !slices.is_empty();
        self.slice_packets += usize::from(holds_slot);
        self.sent[slot] = Sent {
            seq,
            at: now,
            state: SentState::InFlight,
            requeued: false,
            holds_slot,
            slices,
        };
        self.stats.packets_sent += 1;
        self.stats.bytes_sent += out.len() as u64;
        self.last_sent = now;
        self.ack_owed_since = None;
    }

    /// Fills the rest of the packet with slices, within the budget.
    fn put_slices(&mut self, seq: u16, out: &mut Vec<u8>, carried: &mut Vec<SliceRef>) {
        while self.budget.open() {
            self.prune_resend();
            let Some(next) = self.next_slice() else {
                break;
            };
            let Some(index) = self.index(next.id) else {
                break;
            };
            let slice = self.window[index].slice(next.id, next.part);
            let len = slice.frame_len();
            if out.len() + len > MAX_DATAGRAM {
                break;
            }
            slice.put_frame(out);
            self.stats.slice_bytes_sent += slice.data.len() as u64;
            self.budget.spend(len);
            let part = &mut self.window[index].parts[next.part as usize];
            let again = part.last_seq.is_some();
            part.last_seq = Some(seq);
            part.queued = false;
            if again {
                self.resend.pop_front();
                self.stats.resends += 1;
            } else {
                self.fresh = self.advance(self.fresh);
            }
            self.stats.slices_sent += 1;
            carried.push(next);
        }
    }

    /// The slice after `slice` in the stream of never-sent slices.
    fn advance(&self, slice: SliceRef) -> SliceRef {
        let count = self
            .index(slice.id)
            .map_or(0, |index| self.window[index].count());
        if slice.part + 1 < count {
            SliceRef {
                part: slice.part + 1,
                ..slice
            }
        } else {
            SliceRef {
                id: slice.id.wrapping_add(1),
                part: 0,
            }
        }
    }

    /// Margins over RTT: (resend, declare lost), each with the peer's ack
    /// delay.
    fn timeouts(&self) -> (u64, u64) {
        let srtt = self.rtt.srtt();
        let jitter4 = 4 * self.rtt.jitter();
        let ack = self.config.max_ack_delay_us;
        let resend = srtt + (srtt / 4).min(jitter4).max(MIN_MARGIN_US) + ack;
        let lost = srtt + jitter4.max(MIN_MARGIN_US) + ack;
        (resend, lost)
    }

    /// Queues again the slices of packets past their resend time, and
    /// declares lost those past their loss time.
    fn expire(&mut self, now: NetTime) {
        let (resend, lost) = self.timeouts();
        let mut seq = self.oldest;
        while seq != self.next_seq {
            let record = &self.sent[seq as usize % SENT_RING];
            if record.state == SentState::InFlight {
                let age = now.since(record.at);
                if age < resend {
                    break;
                }
                if age >= lost && self.acked_after(seq) {
                    self.declare_lost(seq);
                } else if !record.requeued {
                    self.requeue(seq);
                }
            }
            seq = seq.wrapping_add(1);
        }
        self.advance_oldest();
    }

    /// Whether a packet sent after `seq` has been acked.
    fn acked_after(&self, seq: u16) -> bool {
        self.newest_acked
            .is_some_and(|newest| newest.wrapping_sub(seq) as i16 > 0)
    }

    fn advance_oldest(&mut self) {
        while self.oldest != self.next_seq
            && self.sent[self.oldest as usize % SENT_RING].state != SentState::InFlight
        {
            self.oldest = self.oldest.wrapping_add(1);
        }
    }

    /// Queues packet `seq`'s slices to be sent again, those it was the
    /// last to carry and not acked.
    fn requeue(&mut self, seq: u16) {
        let slot = seq as usize % SENT_RING;
        let slices = std::mem::take(&mut self.sent[slot].slices);
        for &slice in &slices {
            if let Some(part) = self.part(slice)
                && !part.acked
                && !part.queued
                && part.last_seq == Some(seq)
            {
                part.queued = true;
                self.resend.push_back(slice);
            }
        }
        let record = &mut self.sent[slot];
        record.slices = slices;
        record.requeued = true;
        self.release_slot(slot);
    }

    fn release_slot(&mut self, slot: usize) {
        let record = &mut self.sent[slot];
        if record.holds_slot {
            record.holds_slot = false;
            self.slice_packets -= 1;
        }
    }

    fn declare_lost(&mut self, seq: u16) {
        let slot = seq as usize % SENT_RING;
        if !self.sent[slot].requeued {
            self.requeue(seq);
        }
        self.sent[slot].state = SentState::Lost;
        self.stats.declared_lost += 1;
        self.loss_ppm = self.loss_ppm - self.loss_ppm / 32 + 1_000_000 / 32;
    }

    /// When `poll_transmit` next has something to send, or the link times
    /// out. A time at or before now means now.
    pub fn next_deadline(&self) -> Option<NetTime> {
        if self.broken.is_some() {
            return None;
        }
        let mut deadline = if self.in_process {
            NetTime(u64::MAX)
        } else {
            self.last_received.plus(TIMEOUT_US)
        };
        let mut consider = |at: NetTime| deadline = deadline.min(at);
        consider(self.last_sent.plus(KEEPALIVE_US));
        if !self.unreliable_lens.is_empty() {
            consider(NetTime(0));
        }
        if let Some(since) = self.ack_owed_since {
            consider(since.plus(self.config.max_ack_delay_us));
        }
        if self.next_slice().is_some() && self.slice_packets < MAX_SLICE_PACKETS {
            consider(self.budget.opens_at());
        }
        let (resend, lost) = self.timeouts();
        let mut seq = self.oldest;
        let mut resend_found = false;
        let mut lost_found = false;
        while seq != self.next_seq && !(resend_found && lost_found) {
            let record = &self.sent[seq as usize % SENT_RING];
            if record.state == SentState::InFlight {
                if !lost_found {
                    if self.acked_after(record.seq) {
                        consider(record.at.plus(lost));
                    }
                    lost_found = true;
                }
                if !resend_found && !record.requeued {
                    consider(record.at.plus(resend));
                    resend_found = true;
                }
            }
            seq = seq.wrapping_add(1);
        }
        Some(deadline)
    }

    // -----------------------------------------------------------------
    // Receiving.

    /// Takes a datagram that arrived at `now`. The whole packet is
    /// checked before anything is used; acks and reliable slices are
    /// applied, and the session's frames returned. An error means the
    /// packet was not used (only `Broken` is final).
    pub fn receive<'a>(
        &mut self,
        now: NetTime,
        bytes: &'a [u8],
    ) -> Result<Received<'a>, LinkError> {
        if let Some(reason) = self.broken {
            return Err(LinkError::Broken(reason));
        }
        let (header, body) = Header::decode(bytes)?;
        if header.session != self.session {
            return Err(LinkError::Session {
                got: header.session,
            });
        }
        for frame in Frames::new(body) {
            let frame = frame?;
            if frame.kind == frame::RELIABLE {
                Slice::decode(frame.payload)?;
            }
        }
        let arrival = self.received.place(header.seq);
        // Further ahead than the ack bits reach: taken only when a second
        // packet close after it says the peer really is there (a long
        // outage), never on one number alone.
        if let Arrival::Newer(ahead) = arrival
            && ahead > JUMP_WINDOW
        {
            let confirmed = self.jump.is_some_and(|first| {
                let after = header.seq.wrapping_sub(first) as i16;
                after > 0 && after as u32 <= JUMP_WINDOW
            });
            if !confirmed {
                self.jump = Some(header.seq);
                self.stats.jumps += 1;
                return Err(LinkError::Jump);
            }
        }
        self.jump = None;
        match arrival {
            Arrival::Duplicate => {
                self.stats.duplicates += 1;
                return Err(LinkError::Duplicate);
            }
            Arrival::Stale => {
                self.stats.stale += 1;
                return Err(LinkError::Stale);
            }
            Arrival::Newer(_) | Arrival::Older(_) => {}
        }
        self.received.record(header.seq, &arrival, now);
        self.last_received = self.last_received.max(now);
        self.stats.packets_received += 1;
        self.stats.bytes_received += bytes.len() as u64;
        if let Some(ack) = header.ack {
            self.take_acks(now, ack);
        }
        if !body.is_empty() && self.ack_owed_since.is_none() {
            self.ack_owed_since = Some(now);
        }
        for frame in Frames::new(body).map_while(Result::ok) {
            if frame.kind != frame::RELIABLE {
                continue;
            }
            let Ok(slice) = Slice::decode(frame.payload) else {
                continue;
            };
            if let Err(reason) = self.accept_slice(&slice) {
                self.broken = Some(reason);
                return Err(LinkError::Broken(reason));
            }
        }
        Ok(Received {
            seq: header.seq,
            body,
        })
    }

    fn take_acks(&mut self, now: NetTime, ack: Ack) {
        self.acknowledge(now, ack.seq, Some(ack.delay));
        for bit in 0..32 {
            if ack.bits & (1 << bit) != 0 {
                self.acknowledge(now, ack.seq.wrapping_sub(bit + 1), None);
            }
        }
        self.advance_oldest();
        self.retire();
    }

    /// Our packet `seq` arrived; `delay` is the peer's ack delay when it
    /// is the newest the peer has (an RTT sample).
    fn acknowledge(&mut self, now: NetTime, seq: u16, delay: Option<u16>) {
        let slot = seq as usize % SENT_RING;
        let record = &self.sent[slot];
        if record.seq != seq || !matches!(record.state, SentState::InFlight | SentState::Lost) {
            return;
        }
        if record.state == SentState::Lost {
            self.stats.late_acks += 1;
        } else {
            self.loss_ppm -= self.loss_ppm / 32;
        }
        if let Some(delay) = delay {
            let rtt = now.since(record.at).saturating_sub(u64::from(delay) * 100);
            self.rtt.sample(rtt);
        }
        self.sent[slot].state = SentState::Acked;
        if self
            .newest_acked
            .is_none_or(|newest| seq.wrapping_sub(newest) as i16 > 0)
        {
            self.newest_acked = Some(seq);
        }
        self.release_slot(slot);
        let slices = std::mem::take(&mut self.sent[slot].slices);
        for &slice in &slices {
            let Some(index) = self.index(slice.id) else {
                continue;
            };
            let message = &mut self.window[index];
            if let Some(part) = message.parts.get_mut(slice.part as usize)
                && !part.acked
            {
                part.acked = true;
                message.acked += 1;
            }
        }
        self.sent[slot].slices = slices;
        if self.acked.len() == ACKED_KEEP {
            self.acked.pop_front();
        }
        self.acked.push_back(seq);
    }

    /// Retires wholly acked messages from the window's front.
    fn retire(&mut self) {
        while let Some(front) = self.window.front() {
            if front.acked < front.count() {
                break;
            }
            self.window_bytes -= front.data.len();
            self.window.pop_front();
        }
    }

    /// Stores a slice of the peer's; delivers what it completes. `Err` is
    /// a protocol break.
    fn accept_slice(&mut self, slice: &Slice<'_>) -> Result<(), &'static str> {
        let offset = slice.id.wrapping_sub(self.deliver_id) as usize;
        if offset >= 1 << 15 {
            self.stats.duplicate_slices += 1;
            return Ok(());
        }
        if offset >= MAX_IN_FLIGHT {
            return Err("message id beyond the window");
        }
        if self.incoming.len() <= offset {
            self.incoming.resize_with(offset + 1, || None);
        }
        let message = self.incoming[offset].get_or_insert_with(|| Incoming {
            count: slice.count,
            parts: BTreeMap::new(),
        });
        if message.count != slice.count {
            return Err("a message's slice count changed");
        }
        if message.parts.contains_key(&slice.part) {
            self.stats.duplicate_slices += 1;
            return Ok(());
        }
        if self.held_bytes + slice.data.len() > MAX_WINDOW_BYTES
            || self.held_slices + 1 > MAX_HELD_SLICES
        {
            return Err("more held than the window allows");
        }
        message.parts.insert(slice.part, slice.data.to_vec());
        self.held_bytes += slice.data.len();
        self.held_slices += 1;
        self.deliver();
        Ok(())
    }

    fn deliver(&mut self) {
        while let Some(Some(front)) = self.incoming.front() {
            if front.parts.len() < front.count as usize {
                break;
            }
            let Some(Some(message)) = self.incoming.pop_front() else {
                break;
            };
            let parts = message.parts.len();
            let mut data = Vec::with_capacity(message.parts.values().map(Vec::len).sum());
            for part in message.parts.into_values() {
                data.extend_from_slice(&part);
            }
            self.held_bytes -= data.len();
            self.held_slices -= parts;
            self.stats.messages_delivered += 1;
            self.stats.message_bytes_delivered += data.len() as u64;
            self.delivered.push_back(data);
            self.deliver_id = self.deliver_id.wrapping_add(1);
        }
    }

    /// The peer's reliable messages delivered so far, in order.
    pub fn take_messages(&mut self) -> std::collections::vec_deque::Drain<'_, Vec<u8>> {
        self.delivered.drain(..)
    }

    /// Our packet numbers acked since the last call, in the order the
    /// acks arrived (the session learns which snapshot or commands got
    /// through). At most the last 1024 are kept.
    pub fn take_acked(&mut self) -> std::collections::vec_deque::Drain<'_, u16> {
        self.acked.drain(..)
    }
}
