//! The network core's evidence, run by `kjk net probes`: checks with a
//! known right answer, each with what it expects, what it saw and
//! whether it passed, plus the measures worth keeping (`Measure`).
//!
//! Everything runs in one thread in virtual time over `io::SimNet`, from
//! fixed seeds, so a run repeats exactly:
//! - `wire_probes`: varints at their edges and in their shortest form
//!   only, and every packet kind through encode and decode;
//! - `reliable`: two links exchange messages of random sizes (1 B to
//!   64 KiB) both ways through 20% loss in bursts of 3, 2% duplication,
//!   5% reordering and jitter, at 100 ms RTT, beside a 62.5 Hz stream of
//!   unreliable frames: every message delivered in order exactly once,
//!   the budget kept, and the overhead per packet;
//! - `rtt`: the estimate against the network's own RTT, with a slow
//!   acker and with jitter, and no packet declared lost on a clean link;
//! - `handshake`: accept, every `Refusal`, the join password, the Hello
//!   rate limit, forged, replayed and expired cookies, replies never larger
//!   than requests;
//! - `netem`: the conditions the model promises, measured;
//! - `link_rules`: keepalive, timeout, packet numbers far ahead, the
//!   reliable limits and hostile
//!   peers that break them;
//! - `loopback`: two links over `io::Loopback`;
//! - `determinism`: the same seed gives the same bytes;
//! - `mutation`: a million seeded corruptions of recorded packets through
//!   every decoder and both state machines, counting panics (must be 0)
//!   and limits broken (must be 0).

use crate::handshake::{
    self, Client, Credentials, Dropped, Event, GameFingerprint, JoinKey, Refusal,
};
use crate::io::{Clock, Loopback, SimNet};
use crate::link::{self, Config, Link, LinkError, MAX_IN_FLIGHT, MAX_WINDOW_BYTES};
use crate::netem::{Conditions, Shaper, ShaperStats};
use crate::session::msg::Message;
use crate::wire::{
    self, Frames, HEADER_LEN, Handshake, MAX_GAMES, MAX_GROUPS, MAX_MESSAGE, MAX_PARTS, MAX_TEXT,
    PROOF_LEN, PROTOCOL_VERSION, Reader, SLICE, Slice, frame,
};
use crate::{MAX_DATAGRAM, NetTime, Peer, Transport};
use sim::SimRng;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Virtual seconds of the reliable-delivery run in `all`.
pub const RELIABLE_SECONDS: u64 = 10_000;
/// Corrupted packets in `all`'s mutation run.
pub const MUTATIONS: u64 = 1_000_000;

/// One check: what it expects, what it saw, and whether that is right.
#[derive(Debug, Clone, PartialEq)]
pub struct Probe {
    pub name: String,
    pub expect: String,
    pub detail: String,
    pub pass: bool,
}

/// A number worth keeping (in `facts.json`).
#[derive(Debug, Clone, PartialEq)]
pub struct Measure {
    pub name: String,
    pub value: f64,
    pub unit: &'static str,
}

/// Checks and measures in the order they ran.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probes {
    pub list: Vec<Probe>,
    pub measures: Vec<Measure>,
}

impl Probes {
    /// Records a check.
    pub fn check(&mut self, name: &str, expect: &str, pass: bool, detail: String) {
        self.list.push(Probe {
            name: name.to_string(),
            expect: expect.to_string(),
            detail,
            pass,
        });
    }

    /// Records a measure.
    pub fn measure(&mut self, name: &str, value: f64, unit: &'static str) {
        self.measures.push(Measure {
            name: name.to_string(),
            value,
            unit,
        });
    }

    /// Checks that did not pass.
    pub fn failed(&self) -> usize {
        self.list.iter().filter(|probe| !probe.pass).count()
    }

    /// Appends another group's checks and measures.
    pub fn extend(&mut self, other: Probes) {
        self.list.extend(other.list);
        self.measures.extend(other.measures);
    }
}

/// A group of probes by name, so the launcher can print and time each.
pub type Group = (&'static str, fn() -> Probes);

/// Every group, in the order `all` runs them.
pub fn groups() -> Vec<Group> {
    vec![
        ("wire", wire_probes),
        ("reliable", || reliable(RELIABLE_SECONDS)),
        ("rtt", rtt),
        ("handshake", handshake),
        ("netem", netem),
        ("link rules", link_rules),
        ("loopback", loopback),
        ("determinism", determinism),
        ("mutation", || mutation(MUTATIONS)),
        ("save copies", crate::session::probes::copy_probes),
        ("seats", crate::session::probes::seat_probes),
    ]
}

/// Every probe.
pub fn all() -> Probes {
    let mut probes = Probes::default();
    for (_, run) in groups() {
        probes.extend(run());
    }
    probes
}

// ---------------------------------------------------------------------------
// The harness: two links over a SimNet, in virtual time.

/// Frames and errors one side saw.
#[derive(Debug, Clone, Copy, Default)]
struct Seen {
    frames: u64,
    duplicates: u64,
    stale: u64,
    errors: u64,
}

/// Two links, `Local(0)` and `Local(1)`, over a `SimNet`.
struct Duo {
    net: SimNet,
    peers: [Peer; 2],
    links: [Link; 2],
    seen: [Seen; 2],
    buf: Vec<u8>,
    /// Every datagram sent, hashed in order.
    digest: blake3::Hasher,
    /// The first datagrams sent, kept (a corpus for the mutation run).
    tap: Vec<Vec<u8>>,
    tap_limit: usize,
    /// Times a link's deadline was already past after it had sent all it
    /// would (a busy loop in a real driver).
    stalls: u64,
}

const SESSION: u64 = 0x6b6a_6b5f_6e65_7431;

impl Duo {
    /// `ab` from 0 to 1, `ba` back.
    fn new(seed: u64, ab: Conditions, ba: Conditions, config: Config) -> Duo {
        let mut net = SimNet::new(seed);
        let peers = [net.endpoint(), net.endpoint()];
        net.set_path(peers[0], peers[1], ab);
        net.set_path(peers[1], peers[0], ba);
        Duo {
            net,
            peers,
            links: [
                Link::with_config(SESSION, NetTime(0), config),
                Link::with_config(SESSION, NetTime(0), config),
            ],
            seen: [Seen::default(); 2],
            buf: Vec::with_capacity(MAX_DATAGRAM),
            digest: blake3::Hasher::new(),
            tap: Vec::new(),
            tap_limit: 0,
            stalls: 0,
        }
    }

    fn now(&self) -> NetTime {
        self.net.now()
    }

    /// Hands every datagram that arrived to its link.
    fn receive_all(&mut self) {
        for side in 0..2 {
            while let Some(datagram) = self.net.recv(self.peers[side]) {
                let seen = &mut self.seen[side];
                match self.links[side].receive(datagram.at, &datagram.bytes) {
                    Ok(received) => seen.frames += received.frames().count() as u64,
                    Err(LinkError::Duplicate) => seen.duplicates += 1,
                    Err(LinkError::Stale) => seen.stale += 1,
                    Err(_) => seen.errors += 1,
                }
            }
        }
    }

    /// Sends everything both links have to send now.
    fn transmit(&mut self) {
        let now = self.now();
        for side in 0..2 {
            while self.links[side].poll_transmit_into(now, &mut self.buf) {
                self.digest.update(&self.buf);
                if self.tap.len() < self.tap_limit {
                    self.tap.push(self.buf.clone());
                }
                self.net
                    .send(self.peers[side], self.peers[1 - side], &self.buf);
            }
        }
    }

    /// Moves time to the next event: an arrival, a link's deadline, or
    /// `wake`.
    fn advance(&mut self, wake: NetTime) {
        let now = self.now();
        let mut next = wake;
        if let Some(arrival) = self.net.next_arrival() {
            next = next.min(arrival);
        }
        for link in &self.links {
            match link.next_deadline() {
                Some(deadline) if deadline <= now => self.stalls += 1,
                Some(deadline) => next = next.min(deadline),
                None => {}
            }
        }
        self.net.advance_to(next.max(now.plus(1)));
    }

    /// Runs until `app` says stop: each turn, arrivals, then `app` (which
    /// returns when it next wants to run), then sends.
    fn run(&mut self, mut app: impl FnMut(&mut Duo) -> Option<NetTime>) {
        loop {
            self.receive_all();
            let Some(wake) = app(self) else {
                break;
            };
            self.transmit();
            self.advance(wake);
        }
    }
}

/// Messages of log-uniform sizes from 1 byte to 64 KiB, each filled with
/// bytes that depend on its index and position, so a lost, repeated,
/// reordered or misplaced slice shows. The receiver runs the same stream
/// to know what must arrive.
struct Stream {
    rng: SimRng,
    index: u64,
}

impl Stream {
    fn new(seed: u64) -> Stream {
        Stream {
            rng: SimRng::new(seed, 0x6d65_7373),
            index: 0,
        }
    }

    fn next(&mut self) -> Vec<u8> {
        let bits = self.rng.below(17);
        let len = ((1u32 << bits) + self.rng.below(1 << bits)).clamp(1, 65_536) as usize;
        let index = self.index;
        self.index += 1;
        // Eight bytes at a time: word `k` of message `index`, mixed.
        let mut bytes = Vec::with_capacity(len.next_multiple_of(8));
        for word in 0..len.div_ceil(8) as u64 {
            let mixed = (index << 32 | word).wrapping_mul(0x9e37_79b9_7f4a_7c15);
            bytes.extend_from_slice(&(mixed ^ mixed >> 29).to_le_bytes());
        }
        bytes.truncate(len);
        bytes
    }
}

fn ms(us: u64) -> f64 {
    us as f64 / 1000.0
}

fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        0.0
    } else {
        100.0 * part as f64 / whole as f64
    }
}

fn conditions(delay_ms: u64, jitter_ms: u64) -> Conditions {
    Conditions {
        delay_us: delay_ms * 1000,
        jitter_us: jitter_ms * 1000,
        ..Conditions::default()
    }
}

// ---------------------------------------------------------------------------
// 0. Wire.

/// Varints at their edges, zigzag, the shortest-form rule, and every
/// packet kind through encode and decode.
pub fn wire_probes() -> Probes {
    let mut probes = Probes::default();
    let mut rng = SimRng::new(70, 0x7769_7265);
    let edges = [
        0,
        1,
        127,
        128,
        16_383,
        16_384,
        u64::from(u32::MAX),
        u64::MAX >> 1,
        u64::MAX,
    ];
    let random: Vec<u64> = (0..100_000)
        .map(|_| {
            let wide = u64::from(rng.next_u32()) << 32 | u64::from(rng.next_u32());
            wide >> rng.below(64)
        })
        .collect();
    let mut out = Vec::new();
    let mut failures = 0;
    for value in edges.into_iter().chain(random) {
        out.clear();
        wire::put_varint(&mut out, value);
        let back = Reader::new(&out).varint("probe");
        failures += u32::from(back != Ok(value) || out.len() != wire::varint_len(value));
        let signed = value as i64;
        failures += u32::from(wire::unzigzag(wire::zigzag(signed)) != signed);
    }
    let small = [0i64, -1, 1, -64, 63].map(|value| wire::varint_len(wire::zigzag(value)));
    let refused: [&[u8]; 4] = [
        &[0x80, 0x00],
        &[0x80; 11],
        &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02],
        &[0x80, 0x80],
    ];
    let accepted = refused
        .iter()
        .filter(|bytes| Reader::new(bytes).varint("probe").is_ok())
        .count();
    let longest = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
    let longest = Reader::new(&longest).varint("probe");
    probes.check(
        "wire: varints",
        "LEB128 and zigzag round-trip at the edges and on 100 000 random values, in the length varint_len says; small magnitudes of either sign take one byte; padded, overlong, over-64-bit and cut varints are refused",
        failures == 0
            && small.iter().all(|&len| len == 1)
            && accepted == 0
            && longest == Ok(u64::MAX),
        format!("{failures} failures, {accepted} bad encodings accepted"),
    );

    let acked = wire::Header {
        session: SESSION,
        seq: 65_535,
        ack: Some(wire::Ack {
            seq: 7,
            bits: 0x8000_0001,
            delay: 12,
        }),
    };
    let first = wire::Header { ack: None, ..acked };
    let slice = Slice {
        id: 9,
        part: 2,
        count: 3,
        data: b"end",
    };
    let round = |header: &wire::Header| {
        let mut out = Vec::new();
        header.encode(&mut out);
        wire::put_frame(&mut out, 200, b"from a newer build");
        slice.put_frame(&mut out);
        let decoded = wire::Header::decode(&out).map(|(header, body)| {
            let frames: Vec<_> = Frames::new(body).map_while(Result::ok).collect();
            let same = frames
                .get(1)
                .is_some_and(|frame| Slice::decode(frame.payload) == Ok(slice));
            (
                header,
                frames.len(),
                frames.first().map(|frame| frame.kind),
                same,
            )
        });
        (out.len(), decoded)
    };
    let (len, decoded) = round(&acked);
    let (_, decoded_first) = round(&first);
    let mut first_with_ack = Vec::new();
    first.encode(&mut first_with_ack);
    first_with_ack[11] = 1;
    probes.check(
        "wire: session packets",
        "a header with and without acks, an unknown frame type (skipped by its length) and a slice round-trip; a first packet with non-zero ack fields is refused",
        decoded == Ok((acked, 2, Some(200), true))
            && decoded_first == Ok((first, 2, Some(200), true))
            && wire::Header::decode(&first_with_ack).is_err()
            && wire::classify(&first_with_ack) == wire::Class::Session,
        format!("{len} B: {decoded:?}"),
    );

    let packets = [
        Handshake::Hello { nonce: 1 },
        Handshake::Challenge {
            nonce: 2,
            expiry: 3,
            cookie: [4; wire::COOKIE_LEN],
        },
        Handshake::Connect {
            nonce: 5,
            expiry: 6,
            cookie: [7; wire::COOKIE_LEN],
            proof: Some([13; PROOF_LEN]),
            credentials: Credentials {
                reconnect: Some(8),
                ..host_credentials()
            },
        },
        Handshake::Connect {
            nonce: 5,
            expiry: 6,
            cookie: [7; wire::COOKIE_LEN],
            proof: None,
            credentials: host_credentials(),
        },
        Handshake::Accept {
            nonce: 9,
            session: 10,
        },
        Handshake::Deny {
            nonce: 11,
            refusal: Refusal::Full { seats: 12 },
        },
        Handshake::Deny {
            nonce: 11,
            refusal: Refusal::Password { given: true },
        },
        Handshake::Deny {
            nonce: 11,
            refusal: Refusal::InUse,
        },
    ];
    let sizes: Vec<(usize, bool)> = packets
        .iter()
        .map(|packet| {
            let encoded = bytes(packet.encode());
            let fine = Handshake::decode(&encoded).as_ref() == Ok(packet)
                && wire::classify(&encoded) == wire::Class::Handshake
                && (encoded.len() == MAX_DATAGRAM) == packet.padded();
            (encoded.len(), fine)
        })
        .collect();
    probes.check(
        "wire: handshake packets",
        "every handshake packet round-trips; requests are exactly 1200 bytes, replies shorter",
        sizes.iter().all(|&(_, fine)| fine),
        format!(
            "sizes {:?}",
            sizes.iter().map(|&(len, _)| len).collect::<Vec<_>>()
        ),
    );
    let request = crate::session::InstallRequest {
        game: "kjk".into(),
        module: "probe".into(),
        arrival: String::new(),
        seats: 3,
        players: 2,
    };
    let bytes = Message::Installing {
        request,
        payload: &[],
    }
    .to_bytes();
    let back = Message::decode(&bytes);
    probes.check(
        "wire: installing carries players",
        "an Installing message keeps the players its module is planned for (I30 P21)",
        matches!(&back, Ok(Message::Installing { request, .. }) if request.players == 2),
        format!("{back:?}"),
    );
    probes
}

// ---------------------------------------------------------------------------
// 1. Reliable delivery under loss.

/// What one direction of the reliable run delivered.
#[derive(Debug, Clone, Copy, Default)]
struct Delivery {
    sent: u64,
    delivered: u64,
    wrong: u64,
    first_wrong: Option<u64>,
}

/// The adversarial link: 100 ms RTT with jitter, 20% loss in bursts of
/// 3, 2% duplication, 5% reordering, each way.
fn hostile(seed: u64) -> Conditions {
    Conditions {
        delay_us: 50_000,
        jitter_us: 5_000,
        loss_ppm: 200_000,
        burst_milli: 3000,
        dup_ppm: 20_000,
        reorder_ppm: 50_000,
        rate_bps: None,
        seed,
    }
}

const TICK_US: u64 = 16_000;
const TICK_FRAME: usize = 64;

/// Reliable messages both ways for `seconds` of virtual time over the
/// hostile link, beside a 62.5 Hz stream of unreliable frames; then no
/// new messages until everything sent is delivered (at most two virtual
/// minutes).
pub fn reliable(seconds: u64) -> Probes {
    let mut probes = Probes::default();
    let end = NetTime(seconds * 1_000_000);
    let drain_limit = end.plus(120_000_000);
    let mut duo = Duo::new(1, hostile(11), hostile(12), Config::default());
    let mut sending = [Stream::new(21), Stream::new(22)];
    let mut expecting = [Stream::new(22), Stream::new(21)];
    let mut delivery = [Delivery::default(); 2];
    let mut next_tick = NetTime(0);
    let mut ticks = 0u64;
    let mut drained = false;
    duo.run(|duo| {
        let now = duo.now();
        for side in 0..2 {
            for message in duo.links[side].take_messages() {
                let want = expecting[side].next();
                let got = &mut delivery[1 - side];
                if message != want {
                    got.wrong += 1;
                    got.first_wrong.get_or_insert(got.delivered);
                }
                got.delivered += 1;
            }
            duo.links[side].take_acked().for_each(drop);
        }
        if now < end {
            for side in 0..2 {
                while duo.links[side].reliable_in_flight() < 64 {
                    let message = sending[side].next();
                    if duo.links[side].send_reliable(&message).is_err() {
                        break;
                    }
                    delivery[side].sent += 1;
                }
            }
        } else if delivery.iter().all(|d| d.delivered == d.sent)
            && duo.links.iter().all(|link| link.reliable_in_flight() == 0)
        {
            drained = true;
            return None;
        } else if now >= drain_limit {
            return None;
        }
        if now >= next_tick {
            let mut frame = [0u8; TICK_FRAME];
            frame[..8].copy_from_slice(&ticks.to_le_bytes());
            for link in &mut duo.links {
                let _ = link.queue_unreliable(frame::SNAPSHOT, &frame);
            }
            ticks += 1;
            next_tick = next_tick.plus(TICK_US);
        }
        Some(next_tick)
    });

    let stats = [duo.links[0].stats(), duo.links[1].stats()];
    let paths = [
        duo.net.path_stats(duo.peers[0], duo.peers[1]),
        duo.net.path_stats(duo.peers[1], duo.peers[0]),
    ]
    .map(|path| path.unwrap_or(ShaperStats::default()));
    let virtual_s = duo.now().0 as f64 / 1e6;
    let ok = delivery
        .iter()
        .all(|d| d.sent > 0 && d.delivered == d.sent && d.wrong == 0)
        && drained;
    probes.check(
        "reliable: delivery",
        "over 20% loss in bursts of 3, 2% duplication, 5% reordering, 5 ms jitter and 100 ms RTT, every message of 1 B to 64 KiB arrives both ways, in order, exactly once, byte for byte",
        ok,
        format!(
            "{seconds} s sending + {:.1} s draining; a→b {} sent, {} delivered, {} wrong; b→a {} sent, {} delivered, {} wrong{}",
            virtual_s - seconds as f64,
            delivery[0].sent,
            delivery[0].delivered,
            delivery[0].wrong,
            delivery[1].sent,
            delivery[1].delivered,
            delivery[1].wrong,
            delivery
                .iter()
                .filter_map(|d| d.first_wrong)
                .map(|at| format!(", first wrong message #{at}"))
                .collect::<String>()
        ),
    );
    let rates = paths.map(|path| Rates::of(&path));
    probes.check(
        "reliable: the network was hostile",
        "netem measured about 20% loss in runs averaging 3, 2% duplicates and 5% reordered on both paths",
        rates.iter().all(|r| r.within(2.0, 0.3, 0.3, 0.5)),
        format!("a→b: {}; b→a: {}", rates[0], rates[1]),
    );
    let budget = link::DEFAULT_BUDGET as f64;
    let rates = stats.map(|s| s.slice_bytes_sent as f64 / virtual_s);
    probes.check(
        "reliable: pacing",
        "slice bytes per second stay within the 256 KiB/s budget, and use most of it while messages wait",
        rates.iter().all(|&rate| rate <= budget && rate >= 0.8 * budget * seconds as f64 / virtual_s),
        format!(
            "a {:.1} KiB/s, b {:.1} KiB/s of slices (budget {:.0} KiB/s)",
            rates[0] / 1024.0,
            rates[1] / 1024.0,
            budget / 1024.0
        ),
    );
    probes.check(
        "reliable: wraps",
        "the run wraps packet numbers and message ids (u16) several times",
        stats.iter().all(|s| s.packets_sent > 3 * 65_536)
            && delivery.iter().all(|d| d.sent > 65_536),
        format!(
            "packets a {} b {}; messages a {} b {}",
            stats[0].packets_sent, stats[1].packets_sent, delivery[0].sent, delivery[1].sent
        ),
    );
    probes.check(
        "reliable: deadlines",
        "after a link has sent all it will, its next deadline is always in the future (a driver never spins)",
        duo.stalls == 0,
        format!("{} stalls", duo.stalls),
    );
    for (side, name) in [(0, "a"), (1, "b")] {
        let s = &stats[side];
        let seen = &duo.seen[side];
        let peer = &stats[1 - side];
        let table = [
            ("messages_sent", delivery[side].sent as f64, "messages"),
            (
                "message_mib_delivered_to_peer",
                peer.message_bytes_delivered as f64 / (1 << 20) as f64,
                "MiB",
            ),
            ("packets_sent", s.packets_sent as f64, "packets"),
            ("resends", s.resends as f64, "slices"),
            ("resent_share", percent(s.resends, s.slices_sent), "%"),
            ("declared_lost", s.declared_lost as f64, "packets"),
            (
                "declared_lost_share",
                percent(s.declared_lost, s.packets_sent),
                "%",
            ),
            ("late_acks", s.late_acks as f64, "packets"),
            (
                "duplicate_packets_received",
                seen.duplicates as f64,
                "packets",
            ),
            (
                "duplicate_slices_received",
                s.duplicate_slices as f64,
                "slices",
            ),
            ("stale_packets_received", seen.stale as f64, "packets"),
            ("rtt", ms(s.rtt_us), "ms"),
            ("jitter", ms(s.jitter_us), "ms"),
            ("recent_loss", s.loss_ppm as f64 / 1e4, "%"),
        ];
        for (key, value, unit) in table {
            probes.measure(&format!("reliable.{name}.{key}"), value, unit);
        }
    }

    // Overhead: every byte sent that is neither slice data nor the
    // session's unreliable payloads, per packet.
    let unreliable_payload = ticks * TICK_FRAME as u64;
    let overhead = stats.map(|s| {
        (s.bytes_sent - s.slice_bytes_sent - unreliable_payload) as f64 / s.packets_sent as f64
    });
    let slice_head = wire::frame_len(Slice::head_len(1, 30) + SLICE) - SLICE;
    let keepalive = keepalive_len();
    probes.check(
        "overhead: header",
        "the session header is 19 bytes (design section 4), and a packet with nothing to say is only that",
        HEADER_LEN == 19 && keepalive == HEADER_LEN,
        format!(
            "header {HEADER_LEN} B (47 B with UDP/IPv4), keepalive packet {keepalive} B, slice framing {slice_head} B (type, length, id, part, count), unreliable frame framing {} B for a {TICK_FRAME}-byte payload; mean per packet in the reliable run: a {:.1} B, b {:.1} B (header, acks and framing)",
            wire::frame_len(TICK_FRAME) - TICK_FRAME,
            overhead[0],
            overhead[1],
        ),
    );
    probes.measure("overhead.header", HEADER_LEN as f64, "B");
    probes.measure("overhead.header_udp_ipv4", (HEADER_LEN + 28) as f64, "B");
    probes.measure("overhead.slice_framing", slice_head as f64, "B");
    probes.measure("overhead.per_packet_a", overhead[0], "B");
    probes.measure("overhead.per_packet_b", overhead[1], "B");
    probes
}

/// The size of a packet from a link with nothing queued (a keepalive).
fn keepalive_len() -> usize {
    let mut link = Link::new(SESSION, NetTime(0));
    link.poll_transmit(NetTime(link::KEEPALIVE_US))
        .map_or(0, |packet| packet.len())
}

// ---------------------------------------------------------------------------
// 2. RTT.

/// The RTT estimate on a 100 ms link: one side sends at 125 Hz, the other
/// at 20 Hz (so most acks wait and the ack delay matters), without and
/// with jitter.
pub fn rtt() -> Probes {
    let mut probes = Probes::default();
    for (label, jitter_ms) in [("no jitter", 0), ("5 ms jitter", 5)] {
        let path = conditions(50, jitter_ms);
        let mut duo = Duo::new(2, path, path, Config::default());
        let end = NetTime(20_000_000);
        let periods = [8_000, 50_000];
        let mut next = [NetTime(0); 2];
        duo.run(|duo| {
            let now = duo.now();
            if now >= end {
                return None;
            }
            for side in 0..2 {
                if now >= next[side] {
                    let _ = duo.links[side].queue_unreliable(frame::COMMANDS, &[0; 40]);
                    next[side] = next[side].plus(periods[side]);
                }
                duo.links[side].take_acked().for_each(drop);
            }
            Some(next[0].min(next[1]))
        });
        let stats = [duo.links[0].stats(), duo.links[1].stats()];
        let within = |rtt: u64| rtt.abs_diff(100_000) <= 10_000;
        probes.check(
            &format!("rtt: estimate, {label}"),
            "both sides' smoothed RTT within 10% of the network's 100 ms, though one side acks at 20 Hz",
            stats.iter().all(|s| within(s.rtt_us)),
            format!(
                "fast side {:.2} ms (min {:.2}, jitter {:.2}, {} samples), slow side {:.2} ms (min {:.2}, jitter {:.2}, {} samples)",
                ms(stats[0].rtt_us),
                ms(stats[0].rtt_min_us),
                ms(stats[0].jitter_us),
                stats[0].rtt_samples,
                ms(stats[1].rtt_us),
                ms(stats[1].rtt_min_us),
                ms(stats[1].jitter_us),
                stats[1].rtt_samples,
            ),
        );
        probes.check(
            &format!("rtt: no false loss, {label}"),
            "on a link that loses nothing, no packet is declared lost",
            stats.iter().all(|s| s.declared_lost == 0),
            format!(
                "declared lost: {} and {}",
                stats[0].declared_lost, stats[1].declared_lost
            ),
        );
        let key = label.replace(' ', "_");
        probes.measure(&format!("rtt.{key}.fast"), ms(stats[0].rtt_us), "ms");
        probes.measure(&format!("rtt.{key}.slow"), ms(stats[1].rtt_us), "ms");
    }
    probes
}

// ---------------------------------------------------------------------------
// 3. Handshake.

fn host_credentials() -> Credentials {
    Credentials {
        build: "0192afa".to_string(),
        games: vec![GameFingerprint {
            game: "k1".to_string(),
            digest: [1; 32],
            groups: ["chitin.key", "data", "modules", "override"]
                .iter()
                .enumerate()
                .map(|(at, name)| (name.to_string(), [at as u8 + 10; 32]))
                .collect(),
        }],
        name: "host".to_string(),
        reconnect: None,
    }
}

/// Tracks that no server reply is ever larger than its request.
#[derive(Default)]
struct Amplification {
    replies: u64,
    worst: i64,
}

impl Amplification {
    fn see(&mut self, request: &[u8], outcome: &handshake::Outcome) {
        if let Some(reply) = &outcome.reply {
            self.replies += 1;
            self.worst = self.worst.max(reply.len() as i64 - request.len() as i64);
        }
    }
}

/// The host's join password and the client's, if any.
type Keys = [Option<JoinKey>; 2];
const NO_KEYS: Keys = [None, None];

/// A client and a server over a SimNet (20 ms each way) until the client
/// has an outcome; `accept` is the server's check.
fn join(
    credentials: Credentials,
    accept: &mut dyn FnMut(Peer, u64, &Credentials) -> Result<(), Refusal>,
    amplification: &mut Amplification,
    [host_key, key]: Keys,
) -> (Option<Result<u64, Refusal>>, Option<u64>, Option<u64>) {
    let mut net = SimNet::new(3);
    let server_at = net.endpoint();
    let client_at = net.endpoint();
    net.set_link(server_at, client_at, conditions(20, 0));
    let mut server = handshake::Server::new([7; 32], host_key);
    let Ok(mut client) = Client::new(NetTime(0), server_at, 0x1234_5678, credentials, key) else {
        return (None, None, None);
    };
    let mut accepted = None;
    while client.outcome().is_none() {
        let now = net.now();
        while let Some(datagram) = net.recv(server_at) {
            let outcome = server.receive(datagram.at, datagram.from, &datagram.bytes, &mut *accept);
            amplification.see(&datagram.bytes, &outcome);
            if let Event::Accepted { session, .. } = outcome.event {
                accepted = Some(session);
            }
            if let Some(reply) = outcome.reply {
                net.send(server_at, datagram.from, &reply);
            }
        }
        while let Some(datagram) = net.recv(client_at) {
            client.receive(datagram.at, datagram.from, &datagram.bytes);
        }
        if let Some(request) = client.poll_transmit(now) {
            net.send(client_at, server_at, &request);
        }
        let wake = [net.next_arrival(), client.next_deadline()]
            .into_iter()
            .flatten()
            .filter(|&at| at > now)
            .min();
        match wake {
            Some(at) => net.advance_to(at),
            None if client.outcome().is_none() => net.advance_to(now.plus(1)),
            None => {}
        }
    }
    (client.outcome().cloned(), accepted, client.rtt())
}

/// Joining: accept, every refusal, the Hello rate limit, cookies, sizes
/// and amplification.
pub fn handshake() -> Probes {
    let mut probes = Probes::default();
    let mut amplification = Amplification::default();
    let host = host_credentials();
    let seats = 1u8;
    let mut taken = 0u8;

    // Accept, with the host's own check.
    let mut check = |_: Peer, _: u64, here: &Credentials| {
        Credentials::check(&host, here)?;
        if taken == seats {
            return Err(Refusal::Full { seats });
        }
        taken += 1;
        Ok(())
    };
    let client = Credentials {
        name: "guest".to_string(),
        ..host.clone()
    };
    let (outcome, accepted, rtt) = join(client.clone(), &mut check, &mut amplification, NO_KEYS);
    probes.check(
        "handshake: accept",
        "a client with the host's build and data is accepted, with the session id the server chose, in one round trip each for hello and connect",
        matches!(outcome, Some(Ok(session)) if Some(session) == accepted),
        format!("outcome {outcome:?}, server accepted {accepted:?}, last round trip {:?} ms", rtt.map(ms)),
    );

    // Every refusal the server path can give, and the one the client makes.
    let mut override_differs = client.clone();
    override_differs.games[0].digest = [2; 32];
    override_differs.games[0].groups[3].1 = [99; 32];
    let dirty = Credentials {
        build: "0192afa-dirty".to_string(),
        ..client.clone()
    };
    let cases: Vec<(&str, Credentials, Refusal)> = vec![
        (
            "build",
            dirty,
            Refusal::Build {
                host: host.build.clone(),
                here: "0192afa-dirty".to_string(),
            },
        ),
        (
            "game data",
            override_differs,
            Refusal::GameData {
                game: "k1".to_string(),
                groups: vec!["override".to_string()],
            },
        ),
        ("full", client.clone(), Refusal::Full { seats }),
    ];
    let mut produced = Vec::new();
    for (label, credentials, want) in cases {
        let (outcome, _, _) = join(credentials, &mut check, &mut amplification, NO_KEYS);
        let got = matches!(&outcome, Some(Err(refusal)) if *refusal == want);
        produced.push(format!("{label}: {}", outcome_text(&outcome)));
        probes.check(
            &format!("handshake: refusal {label}"),
            "the host's check refuses, and the client decodes the same typed refusal",
            got,
            outcome_text(&outcome),
        );
    }
    for want in [
        Refusal::Level("k1 end_m01aa: another digest".to_string()),
        Refusal::Plan("missing end_m01aa.rim".to_string()),
        Refusal::Kicked("too many cookies".to_string()),
        Refusal::HostQuit,
    ] {
        let expected = want.clone();
        let mut refuse = |_: Peer, _: u64, _: &Credentials| Err(expected.clone());
        let (outcome, _, _) = join(client.clone(), &mut refuse, &mut amplification, NO_KEYS);
        let got = matches!(&outcome, Some(Err(refusal)) if *refusal == want);
        produced.push(outcome_text(&outcome));
        probes.check(
            &format!("handshake: refusal {}", variant(&want)),
            "a refusal the session layer makes after accept travels in a Deny and decodes the same",
            got,
            outcome_text(&outcome),
        );
    }

    // A client of another version, and a host of another version.
    let (here, newer) = (PROTOCOL_VERSION, PROTOCOL_VERSION + 1);
    let mut server = handshake::Server::new([7; 32], None);
    let other = bytes(Handshake::Hello { nonce: 5 }.encode_as(newer));
    let outcome = server.receive(NetTime(0), Peer::Local(9), &other, |_, _, _| Ok(()));
    amplification.see(&other, &outcome);
    let decoded = outcome.reply.as_deref().map(Handshake::decode);
    probes.check(
        "handshake: refusal protocol (host side)",
        &format!(
            "a Hello of protocol {newer} is answered with Deny Protocol {{ host: {here}, here: {newer} }}"
        ),
        decoded
            == Some(Ok(Handshake::Deny {
                nonce: 5,
                refusal: Refusal::Protocol { host: here, here: newer },
            })),
        format!("{decoded:?}"),
    );
    let mut client_side = Client::new(NetTime(0), Peer::Local(0), 77, client.clone(), None).ok();
    let mut from_newer = bytes(
        Handshake::Challenge {
            nonce: 77,
            expiry: 1,
            cookie: [0; wire::COOKIE_LEN],
        }
        .encode(),
    );
    if from_newer.len() > 6 {
        from_newer[4..6].copy_from_slice(&newer.to_le_bytes());
    }
    let outcome = client_side.as_mut().and_then(|client| {
        client.poll_transmit(NetTime(0));
        client.receive(NetTime(1000), Peer::Local(0), &from_newer);
        client.outcome().cloned()
    });
    probes.check(
        "handshake: refusal protocol (client side)",
        &format!(
            "a reply from a host of protocol {newer} ends the attempt with Protocol {{ host: {newer}, here: {here} }}, read from the frozen prefix"
        ),
        outcome == Some(Err(Refusal::Protocol { host: newer, here })),
        format!("{outcome:?}"),
    );

    // Timeout: nobody answers.
    let mut lonely = Client::new(NetTime(0), Peer::Local(0), 1, client.clone(), None).ok();
    let mut sends = 0;
    let mut now = NetTime(0);
    while let Some(client) = lonely.as_mut() {
        sends += u32::from(client.poll_transmit(now).is_some());
        match client.next_deadline() {
            Some(at) => now = at.max(now.plus(1)),
            None => break,
        }
    }
    let outcome = lonely.as_ref().and_then(|client| client.outcome().cloned());
    probes.check(
        "handshake: refusal timeout",
        "with no answer the client sends every 250 ms (20 times) and gives up after 5 s with Timeout",
        outcome == Some(Err(Refusal::Timeout)) && sends == 20 && now.0 == handshake::GIVE_UP_US,
        format!("{outcome:?} after {sends} sends at {} ms", ms(now.0)),
    );
    produced.push(outcome_text(&outcome));
    probes.check(
        "handshake: refusals in words",
        "each refusal prints as a sentence that names what differs",
        produced.iter().any(|text| text.contains("override"))
            && produced.iter().all(|text| text.len() > 20),
        produced.join(" | "),
    );

    // The join password: with one, only a client that proves it joins.
    let mut open = |_: Peer, _: u64, _: &Credentials| Ok(());
    let key = JoinKey::new("blue milk");
    let (right, wrong) = (Some(key), Some(JoinKey::new("bantha")));
    let passwords: [(&str, Keys, Option<Refusal>); 4] = [
        ("right", [right, right], None),
        (
            "none given",
            [right, None],
            Some(Refusal::Password { given: false }),
        ),
        (
            "wrong",
            [right, wrong],
            Some(Refusal::Password { given: true }),
        ),
        ("host has none", [None, right], None),
    ];
    for (label, keys, want) in passwords {
        let (outcome, _, _) = join(client.clone(), &mut open, &mut amplification, keys);
        let got = match (&outcome, &want) {
            (Some(Ok(_)), None) => true,
            (Some(Err(refusal)), Some(want)) => refusal == want,
            _ => false,
        };
        probes.check(
            &format!("handshake: join password, {label}"),
            "the right password joins, none or another is refused as such, and a host without one lets anyone in",
            got,
            outcome_text(&outcome),
        );
    }
    // What a client sends: a proof per cookie, never the password.
    let connect_for = |cookie: [u8; wire::COOKIE_LEN]| {
        let mut attempt =
            Client::new(NetTime(0), Peer::Local(0), 77, client.clone(), right).ok()?;
        attempt.poll_transmit(NetTime(0));
        let challenge = Handshake::Challenge {
            nonce: 77,
            expiry: 9,
            cookie,
        };
        attempt.receive(NetTime(10), Peer::Local(0), &challenge.encode().ok()?);
        attempt.poll_transmit(NetTime(10))
    };
    let sent: Vec<Vec<u8>> = [[1; wire::COOKIE_LEN], [2; wire::COOKIE_LEN]]
        .into_iter()
        .filter_map(connect_for)
        .collect();
    let proofs: Vec<Option<[u8; PROOF_LEN]>> = sent
        .iter()
        .map(|bytes| match Handshake::decode(bytes) {
            Ok(Handshake::Connect { proof, .. }) => proof,
            _ => None,
        })
        .collect();
    let leaks = sent
        .iter()
        .filter(|bytes| bytes.windows(9).any(|window| window == b"blue milk"))
        .count();
    probes.check(
        "handshake: join password, never sent",
        "a Connect carries a proof made over the host's cookie (another cookie, another proof), and not the password",
        proofs.len() == 2 && proofs[0].is_some() && proofs[0] != proofs[1] && leaks == 0,
        format!("proofs {proofs:?}, {leaks} Connects holding the password"),
    );
    // Wrong passwords have limits of their own; the right one still gets
    // in, and the guesser's Hellos are still answered.
    let mut server = handshake::Server::new([10; 32], right);
    let guesser = Peer::Local(5);
    let (nonce, expiry) = (42, handshake::COOKIE_LIFETIME_US);
    let cookie = server.cookie(guesser, nonce, expiry);
    let connect = |cookie, proof| {
        bytes(
            Handshake::Connect {
                nonce,
                expiry,
                cookie,
                proof,
                credentials: client.clone(),
            }
            .encode(),
        )
    };
    let guess = connect(cookie, Some([0; PROOF_LEN]));
    let mut tally = Guesses::default();
    for _ in 0..20 {
        let outcome = server.receive(NetTime(1000), guesser, &guess, |_, _, _| Ok(()));
        amplification.see(&guess, &outcome);
        tally.count(&outcome.event);
    }
    let proven = connect(cookie, Some(key.proof(&cookie, nonce, expiry)));
    let accepted = matches!(
        server
            .receive(NetTime(1000), guesser, &proven, |_, _, _| Ok(()))
            .event,
        Event::Accepted { .. }
    );
    let hello = bytes(Handshake::Hello { nonce: 1 }.encode());
    let hellos = (0..handshake::HELLO_RATE)
        .filter(|_| {
            server
                .receive(NetTime(1000), guesser, &hello, |_, _, _| Ok(()))
                .event
                == Event::Challenged
        })
        .count() as u64;
    probes.check(
        "handshake: join password, guesses limited",
        "20 wrong proofs at once from one host: 4 refused, the rest dropped; the right proof is still accepted, and the host's Hellos are all still answered (guesses spend none of them)",
        tally.refused == handshake::GUESS_RATE
            && tally.dropped == 20 - handshake::GUESS_RATE
            && accepted
            && hellos == handshake::HELLO_RATE,
        format!(
            "{} refused, {} dropped; the right proof accepted {accepted}; {hellos} Hellos answered",
            tally.refused, tally.dropped
        ),
    );

    // One host spraying from 40 000 ports in a second: its Hellos and its
    // guesses stay within its own limits, and players on other hosts get in
    // (the right password) or are told theirs is wrong meanwhile.
    let mut server = handshake::Server::new([11; 32], right);
    let sprayer = |port: u16| Peer::Addr(std::net::SocketAddr::from(([10, 0, 0, 9], port)));
    let player = Peer::Addr(std::net::SocketAddr::from(([10, 0, 0, 10], 5000)));
    let mistyped = Peer::Addr(std::net::SocketAddr::from(([10, 0, 0, 11], 5000)));
    let wrong_key = JoinKey::new("bantha");
    let mut spray = Guesses::default();
    let mut cookies = 0u64;
    let (mut player_in, mut mistyped_told) = (false, false);
    for step in 0..40_000u32 {
        let now = NetTime(u64::from(step) * 25);
        let at = sprayer(1024 + (step % 60_000) as u16);
        let hello = bytes(
            Handshake::Hello {
                nonce: u64::from(step),
            }
            .encode(),
        );
        let outcome = server.receive(now, at, &hello, |_, _, _| Ok(()));
        amplification.see(&hello, &outcome);
        if let Some(Ok(Handshake::Challenge {
            nonce,
            expiry,
            cookie,
        })) = outcome.reply.as_deref().map(Handshake::decode)
        {
            cookies += 1;
            for _ in 0..50 {
                let guess = bytes(
                    Handshake::Connect {
                        nonce,
                        expiry,
                        cookie,
                        proof: Some([0; PROOF_LEN]),
                        credentials: client.clone(),
                    }
                    .encode(),
                );
                spray.count(&server.receive(now, at, &guess, |_, _, _| Ok(())).event);
            }
        }
        if step == 20_000 {
            let mut join_as = |from: Peer, key: JoinKey| {
                let mut attempt =
                    Client::new(now, Peer::Local(0), 7, client.clone(), Some(key)).ok()?;
                for _ in 0..2 {
                    let request = attempt.poll_transmit(now)?;
                    let outcome = server.receive(now, from, &request, |_, _, _| Ok(()));
                    attempt.receive(now, Peer::Local(0), &outcome.reply?);
                }
                attempt.outcome().cloned()
            };
            player_in = matches!(join_as(player, key), Some(Ok(_)));
            mistyped_told =
                join_as(mistyped, wrong_key) == Some(Err(Refusal::Password { given: true }));
        }
    }
    let most = 2 * handshake::GUESS_RATE;
    probes.check(
        "handshake: join password, guesses across ports",
        "one host sending from 40 000 ports in a second gets at most 16 cookies (the Hello limit is the host's, not the port's) and at most 8 wrong passwords answered (4 at once, then 4 a second); meanwhile a player on another host joins with the right password and one with a wrong one is told so",
        cookies <= 2 * handshake::HELLO_RATE && spray.refused <= most && player_in && mistyped_told,
        format!(
            "{cookies} cookies, {} wrong passwords answered, {} dropped; the other player joined {player_in}, the mistyped one told {mistyped_told}",
            spray.refused, spray.dropped
        ),
    );

    // Many hosts guessing together: the budget for everyone holds them,
    // and the right password still gets in.
    let mut server = handshake::Server::new([12; 32], right);
    let mut many = Guesses::default();
    for index in 0..2000u32 {
        let now = NetTime(u64::from(index) * 500);
        let at = Peer::Addr(std::net::SocketAddr::from((index.to_be_bytes(), 5000)));
        let cookie = server.cookie(at, nonce, expiry);
        let guess = connect(cookie, Some([0; PROOF_LEN]));
        many.count(&server.receive(now, at, &guess, |_, _, _| Ok(())).event);
    }
    let late = NetTime(1_000_000);
    let cookie = server.cookie(Peer::Local(9), nonce, expiry);
    let proven = connect(cookie, Some(key.proof(&cookie, nonce, expiry)));
    let accepted = matches!(
        server
            .receive(late, Peer::Local(9), &proven, |_, _, _| Ok(()))
            .event,
        Event::Accepted { .. }
    );
    probes.check(
        "handshake: join password, guesses from many hosts",
        "2000 hosts guessing once each in a second get at most 64 wrong passwords answered (32 at once, then 32 a second) and at least 32; the right password is still accepted",
        (handshake::GUESS_BUDGET..=2 * handshake::GUESS_BUDGET).contains(&many.refused) && accepted,
        format!(
            "{} answered, {} dropped; the right password accepted {accepted}",
            many.refused, many.dropped
        ),
    );

    // The Hello flood.
    let mut server = handshake::Server::new([8; 32], None);
    let flooder = Peer::Addr(std::net::SocketAddr::from(([10, 0, 0, 1], 5000)));
    let bystander = Peer::Addr(std::net::SocketAddr::from(([10, 0, 0, 2], 5000)));
    let hello = bytes(Handshake::Hello { nonce: 1 }.encode());
    let mut challenged = 0;
    let mut bystander_ok = false;
    for at in 0..1000u64 {
        let now = NetTime::from_millis(at);
        let outcome = server.receive(now, flooder, &hello, |_, _, _| Ok(()));
        amplification.see(&hello, &outcome);
        challenged += u32::from(outcome.event == Event::Challenged);
        if at == 500 {
            bystander_ok = server
                .receive(now, bystander, &hello, |_, _, _| Ok(()))
                .event
                == Event::Challenged;
        }
    }
    let after = server.receive(
        NetTime::from_millis(3000),
        flooder,
        &hello,
        |_, _, _| Ok(()),
    );
    probes.check(
        "handshake: hello rate limit",
        "1000 Hellos in a second from one address get 8 at once then one per 125 ms (15 or 16); another address is answered meanwhile; the flooder is answered again after a pause",
        (15..=16).contains(&challenged) && bystander_ok && after.event == Event::Challenged,
        format!(
            "{challenged} challenged, bystander {}, after a pause {:?}",
            if bystander_ok { "answered" } else { "NOT answered" },
            after.event
        ),
    );

    // Cookies: forged, replayed from another address, expired, tampered.
    let mut server = handshake::Server::new([9; 32], None);
    let me = Peer::Local(1);
    let expiry = handshake::COOKIE_LIFETIME_US;
    let cookie = server.cookie(me, 42, expiry);
    let connect = |nonce, expiry, cookie| {
        bytes(
            Handshake::Connect {
                nonce,
                expiry,
                cookie,
                proof: None,
                credentials: client.clone(),
            }
            .encode(),
        )
    };
    let mut try_connect = |at: u64, from: Peer, bytes: &[u8]| {
        let outcome = server.receive(NetTime(at), from, bytes, |_, _, _| Ok(()));
        amplification.see(bytes, &outcome);
        outcome
    };
    let forged = try_connect(1000, me, &connect(42, expiry, [0x5a; wire::COOKIE_LEN]));
    let replayed = try_connect(1000, Peer::Local(2), &connect(42, expiry, cookie));
    let stretched = try_connect(1000, me, &connect(42, expiry + 10_000_000, cookie));
    let renonced = try_connect(1000, me, &connect(43, expiry, cookie));
    let expired = try_connect(expiry + 1, me, &connect(42, expiry, cookie));
    let good = try_connect(1000, me, &connect(42, expiry, cookie));
    let again = try_connect(2000, me, &connect(42, expiry, cookie));
    let session = |outcome: &handshake::Outcome| match &outcome.event {
        Event::Accepted { session, .. } => Some(*session),
        _ => None,
    };
    let rejected = [&forged, &replayed, &stretched, &renonced]
        .iter()
        .all(|outcome| outcome.event == Event::Dropped(Dropped::Cookie) && outcome.reply.is_none());
    probes.check(
        "handshake: cookies",
        "a forged cookie, one replayed from another address, a stretched expiry and another nonce are dropped without a reply; an expired one too; the real one is accepted, and a repeated Connect gets the same session",
        rejected
            && expired.event == Event::Dropped(Dropped::Expired)
            && expired.reply.is_none()
            && session(&good).is_some()
            && session(&good) == session(&again),
        format!(
            "forged {:?}, replayed {:?}, stretched {:?}, other nonce {:?}, expired {:?}, good {:?}, again {:?}",
            forged.event,
            replayed.event,
            stretched.event,
            renonced.event,
            expired.event,
            session(&good),
            session(&again)
        ),
    );

    let short = &hello[..wire::HANDSHAKE_PREFIX_LEN];
    let short_outcome = try_connect(3000, me, short);
    let challenge = bytes(
        Handshake::Challenge {
            nonce: 1,
            expiry: 1,
            cookie: [0; wire::COOKIE_LEN],
        }
        .encode(),
    );
    let backwards = try_connect(3000, me, &challenge);
    probes.check(
        "handshake: unpadded and backwards",
        "a Hello not padded to 1200 bytes and a Challenge sent to the server get no reply",
        short_outcome.reply.is_none()
            && matches!(short_outcome.event, Event::Dropped(Dropped::Malformed(_)))
            && backwards.event == Event::Dropped(Dropped::NotARequest),
        format!(
            "short {:?}, challenge {:?}",
            short_outcome.event, backwards.event
        ),
    );

    // The largest refusal still fits, and big credentials are refused
    // before they are sent.
    let longest = Handshake::Deny {
        nonce: 1,
        refusal: Refusal::GameData {
            game: "é".repeat(200),
            groups: vec!["g".repeat(500); 20],
        },
    }
    .encode();
    let too_many = Credentials {
        games: vec![host.games[0].clone(); MAX_GAMES + 1],
        ..client.clone()
    };
    probes.check(
        "handshake: sizes",
        "the longest Deny (texts and groups cut to their limits) fits a datagram; credentials over the limits are refused by Client::new",
        longest.as_ref().is_ok_and(|bytes| bytes.len() <= MAX_DATAGRAM)
            && Client::new(NetTime(0), me, 1, too_many, None).is_err(),
        format!("longest Deny {:?} B", longest.map(|bytes| bytes.len())),
    );
    probes.check(
        "handshake: no amplification",
        "no reply of the server in this probe is larger than the request it answers",
        amplification.worst <= 0 && amplification.replies > 0,
        format!(
            "{} replies; the largest is {} B smaller than its request",
            amplification.replies, -amplification.worst
        ),
    );
    probes
}

/// An encoding's bytes; empty if it failed (the probe then fails on it).
fn bytes(encoded: Result<Vec<u8>, wire::WireError>) -> Vec<u8> {
    encoded.unwrap_or_else(|_| Vec::new())
}

/// Wrong passwords told so, and dropped by a limit.
#[derive(Default)]
struct Guesses {
    refused: u64,
    dropped: u64,
}

impl Guesses {
    fn count(&mut self, event: &Event) {
        match event {
            Event::Refused(Refusal::Password { given: true }) => self.refused += 1,
            Event::Dropped(Dropped::RateLimited) => self.dropped += 1,
            _ => {}
        }
    }
}

fn outcome_text(outcome: &Option<Result<u64, Refusal>>) -> String {
    match outcome {
        Some(Ok(session)) => format!("accepted, session {session:016x}"),
        Some(Err(refusal)) => format!("{} ({refusal})", variant(refusal)),
        None => "no outcome".to_string(),
    }
}

fn variant(refusal: &Refusal) -> &'static str {
    match refusal {
        Refusal::Protocol { .. } => "protocol",
        Refusal::Build { .. } => "build",
        Refusal::GameData { .. } => "game data",
        Refusal::Full { .. } => "full",
        Refusal::Level(_) => "level",
        Refusal::Plan(_) => "plan",
        Refusal::Timeout => "timeout",
        Refusal::Kicked(_) => "kicked",
        Refusal::HostQuit => "host quit",
        Refusal::Left => "left",
        Refusal::Password { .. } => "password",
        Refusal::InUse => "in use",
    }
}

// ---------------------------------------------------------------------------
// 4. Netem.

/// What a shaper did to the datagrams it was given, as rates.
struct Rates {
    offered: u64,
    /// Percent lost, to the loss chain or the rate cap's queue.
    loss: f64,
    /// Mean run of consecutive losses.
    burst: f64,
    /// Percent of the datagrams kept that were duplicated, and reordered.
    duplicated: f64,
    reordered: f64,
}

impl Rates {
    fn of(stats: &ShaperStats) -> Rates {
        let kept = stats.offered - stats.lost;
        Rates {
            offered: stats.offered,
            loss: percent(stats.lost + stats.queue_drops, stats.offered),
            burst: stats.lost as f64 / stats.loss_runs.max(1) as f64,
            duplicated: percent(stats.duplicated, kept),
            reordered: percent(stats.reordered, kept),
        }
    }

    /// Whether these are the hostile link's 20% loss in runs of 3, 2%
    /// duplicated and 5% reordered, each within its tolerance.
    fn within(&self, loss: f64, burst: f64, duplicated: f64, reordered: f64) -> bool {
        (self.loss - 20.0).abs() <= loss
            && (self.burst - 3.0).abs() <= burst
            && (self.duplicated - 2.0).abs() <= duplicated
            && (self.reordered - 5.0).abs() <= reordered
    }
}

impl std::fmt::Display for Rates {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} datagrams, {:.2}% lost in runs of {:.2}, {:.2}% duplicated, {:.2}% reordered",
            self.offered, self.loss, self.burst, self.duplicated, self.reordered
        )
    }
}

/// Sends `count` numbered datagrams through a shaper, one every
/// `every_us`, and returns (send index, send time, arrival) in arrival
/// order.
fn shape(
    conditions: Conditions,
    count: u64,
    every_us: u64,
    len: usize,
) -> (Shaper, Vec<(u64, u64, u64)>) {
    let mut shaper = Shaper::new(conditions, 1);
    let mut arrivals = Vec::new();
    let mut bytes = vec![0u8; len.max(8)];
    let drain = |shaper: &mut Shaper, now: NetTime, arrivals: &mut Vec<(u64, u64, u64)>| {
        while let Some(flight) = shaper.pop_due(now) {
            let mut index = [0u8; 8];
            index.copy_from_slice(&flight.bytes[..8]);
            let index = u64::from_le_bytes(index);
            arrivals.push((index, index * every_us, flight.arrival.0));
        }
    };
    for index in 0..count {
        let now = NetTime(index * every_us);
        drain(&mut shaper, now, &mut arrivals);
        bytes[..8].copy_from_slice(&index.to_le_bytes());
        shaper.send(now, Peer::Local(0), &bytes);
    }
    drain(&mut shaper, NetTime(u64::MAX), &mut arrivals);
    (shaper, arrivals)
}

/// The conditions netem promises, measured on a shaper alone.
pub fn netem() -> Probes {
    let mut probes = Probes::default();
    let example = "delay=50ms,jitter=5ms,loss=2%,burst=3,dup=0.5%,reorder=1%,rate=2000kbit,seed=7";
    let parsed = Conditions::parse(example);
    let round = parsed
        .as_ref()
        .ok()
        .map(|c| (c.to_string(), Conditions::parse(&c.to_string())));
    let bad = [
        "delay=50",
        "loss=101%",
        "loss=2",
        "burst=0.5",
        "rate=0kbit",
        "rate=2000",
        "foo=1",
        "delay=5ms,delay=6ms",
        "jitter=2s",
        "seed=-1",
        "dup=1.23456%",
        "delay=.5ms",
        "loss=%",
    ];
    let refused: Vec<&str> = bad
        .iter()
        .copied()
        .filter(|text| Conditions::parse(text).is_err())
        .collect();
    probes.check(
        "netem: parse",
        "the design's example parses, prints in canonical form and parses back to the same; malformed, out-of-range, unknown and repeated items are refused; empty means perfect",
        matches!(&round, Some((_, Ok(again))) if Ok(again) == parsed.as_ref())
            && refused.len() == bad.len()
            && Conditions::parse("").is_ok_and(|c| c.is_perfect()),
        format!(
            "{:?}; refused {}/{}",
            round.as_ref().map(|(text, _)| text),
            refused.len(),
            bad.len()
        ),
    );

    let count = 200_000;
    let (shaper, _) = shape(hostile(5), count, 1000, 100);
    let rates = Rates::of(&shaper.stats());
    probes.check(
        "netem: rates",
        "over 200 000 datagrams: loss 20% ± 1 in runs averaging 3 ± 0.15, duplication 2% ± 0.2, reordering 5% ± 0.3",
        rates.within(1.0, 0.15, 0.2, 0.3),
        rates.to_string(),
    );

    let (_, arrivals) = shape(conditions(50, 5), 20_000, 1000, 100);
    let in_order = arrivals.windows(2).all(|pair| pair[0].0 < pair[1].0);
    let bounded = arrivals
        .iter()
        .all(|&(_, sent, at)| (45_000..=60_000).contains(&(at - sent)));
    probes.check(
        "netem: jitter keeps order",
        "with 50 ms delay and 5 ms jitter and no reorder, datagrams arrive in the order sent, each 45 to 60 ms after it left",
        in_order && bounded && arrivals.len() == 20_000,
        format!("{} arrived, in order {in_order}, delays bounded {bounded}", arrivals.len()),
    );

    let reorder = Conditions {
        reorder_ppm: 50_000,
        ..conditions(50, 0)
    };
    let (shaper, arrivals) = shape(reorder, 20_000, 1000, 100);
    let passed_by_one = arrivals
        .iter()
        .enumerate()
        .all(|(at, &(index, _, _))| (index as i64 - at as i64).abs() <= 1);
    let exact = conditions(50, 0);
    let (_, plain) = shape(exact, 1000, 1000, 100);
    probes.check(
        "netem: reorder and delay",
        "a reordered datagram arrives just after the next one (nothing moves more than one place); with no jitter every delay is exactly 50 ms",
        passed_by_one
            && shaper.stats().reordered > 0
            && plain.iter().all(|&(_, sent, at)| at - sent == 50_000),
        format!("{} reordered, all within one place {passed_by_one}", shaper.stats().reordered),
    );

    let capped = Conditions {
        rate_bps: Some(2_000_000),
        ..Conditions::default()
    };
    let (shaper, arrivals) = shape(capped, 2000, 1000, 1200);
    let s = shaper.stats();
    let last = arrivals.last().map_or(0, |&(_, _, at)| at);
    let first = arrivals.first().map_or(0, |&(_, _, at)| at);
    let bits = (arrivals.len() as u64 - 1) * 1200 * 8;
    let rate = bits as f64 * 1e6 / (last - first).max(1) as f64;
    probes.check(
        "netem: rate cap",
        "9.6 Mbit/s offered to a 2 Mbit/s cap: delivered at 2 Mbit/s ± 1%, the queue never over 100 ms, the rest dropped",
        (1_980_000.0..=2_020_000.0).contains(&rate)
            && s.max_queue_us <= 100_000
            && s.queue_drops > 0,
        format!(
            "{:.0} kbit/s, longest wait {:.1} ms, {} dropped of {}",
            rate / 1000.0,
            ms(s.max_queue_us),
            s.queue_drops,
            s.offered
        ),
    );

    let (a, _) = shape(hostile(5), 10_000, 1000, 100);
    let (b, _) = shape(hostile(5), 10_000, 1000, 100);
    let (c, _) = shape(hostile(6), 10_000, 1000, 100);
    probes.check(
        "netem: seeded",
        "the same seed gives the same fates; another seed other ones",
        a.stats() == b.stats() && a.stats() != c.stats(),
        format!("seed 5: {:?}; seed 6: {:?}", a.stats(), c.stats()),
    );
    probes
}

// ---------------------------------------------------------------------------
// 5. Link rules and hostile peers.

/// A packet of `session` numbered `seq`, carrying `slices`, acking nothing.
fn crafted(session: u64, seq: u16, slices: &[Slice<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    wire::Header {
        session,
        seq,
        ack: None,
    }
    .encode(&mut out);
    for slice in slices {
        slice.put_frame(&mut out);
    }
    out
}

/// A packet numbered far ahead on its own is refused, and the real ones
/// after it are taken; after a long outage, the peer's second packet far
/// ahead confirms the jump.
fn jumps(probes: &mut Probes) {
    let mut link = Link::new(SESSION, NetTime(0));
    let mut take = |seq: u16| {
        link.receive(NetTime(1000), &crafted(SESSION, seq, &[]))
            .is_ok()
    };
    let first = (0..10).all(&mut take);
    let forged = take(30_000);
    let after = (10..20).all(&mut take);
    let outage = [take(5_000), take(5_001), take(5_002)];
    probes.check(
        "link: jumps",
        "a lone packet numbered far ahead (a stray or forged number) is refused and the real ones go on; after an outage the second of the peer's packets far ahead is taken",
        first && !forged && after && outage == [false, true, true],
        format!(
            "0..10 taken {first}, 30000 taken {forged}, 10..20 taken {after}, 5000/5001/5002 taken {outage:?}; jumps refused {}",
            link.stats().jumps
        ),
    );
}

/// Keepalive, timeout, the reliable limits on both sides, and hostile or
/// confused peers.
pub fn link_rules() -> Probes {
    let mut probes = Probes::default();
    jumps(&mut probes);

    // Keepalive and timeout.
    let mut link = Link::new(SESSION, NetTime(0));
    let mut sent = 0;
    let mut now = NetTime(0);
    let mut first_timeout = None;
    while now.0 <= 6_000_000 {
        while link.poll_transmit(now).is_some() {
            sent += 1;
        }
        if first_timeout.is_none() && link.timed_out(now) {
            first_timeout = Some(now);
        }
        now = link
            .next_deadline()
            .filter(|&at| at > now)
            .unwrap_or(now.plus(1000));
    }
    probes.check(
        "link: keepalive and timeout",
        "an idle link sends one packet every 100 ms, and a silent peer times it out at exactly 5 s",
        (59..=61).contains(&sent) && first_timeout == Some(NetTime(link::TIMEOUT_US)),
        format!(
            "{sent} keepalives in 6 s, timed out at {:?} ms",
            first_timeout.map(|t| ms(t.0))
        ),
    );

    // An in-process peer (the host's own player) through a 10 s stall.
    let mut link = Link::new(SESSION, NetTime(0));
    link.set_in_process();
    let stall = NetTime(10_000_000);
    let deadline = link.next_deadline();
    probes.check(
        "link: an in-process peer does not time out",
        "a stall of the process (both sides silent 10 s) neither times the link out nor makes timing out its next deadline",
        !link.timed_out(stall) && deadline.is_some_and(|at| at.0 < link::TIMEOUT_US),
        format!(
            "timed out after 10 s: {}, next deadline {:?} ms",
            link.timed_out(stall),
            deadline.map(|t| ms(t.0))
        ),
    );

    // Sender limits.
    let mut link = Link::new(SESSION, NetTime(0));
    let big = vec![7u8; MAX_MESSAGE];
    let over = link.send_reliable(&vec![0; MAX_MESSAGE + 1]);
    let two = [link.send_reliable(&big), link.send_reliable(&big)];
    let third = link.send_reliable(&[1]);
    let mut small = Link::new(SESSION, NetTime(0));
    let fits = (0..MAX_IN_FLIGHT)
        .filter(|_| small.send_reliable(&[1]).is_ok())
        .count();
    let one_more = small.send_reliable(&[1]);
    probes.check(
        "link: sender limits",
        "a message over 4 MiB is refused; two of 4 MiB fill the 8 MiB window; 256 messages fill the count",
        over == Err(LinkError::TooLarge {
            len: MAX_MESSAGE + 1,
        }) && two.iter().all(Result::is_ok)
            && third == Err(LinkError::WindowFull)
            && fits == MAX_IN_FLIGHT
            && one_more == Err(LinkError::WindowFull),
        format!("over {over:?}, two {two:?}, third {third:?}, {fits} small then {one_more:?}"),
    );

    // A 4 MiB message across a lossy link without a budget.
    let lossy = Conditions {
        loss_ppm: 50_000,
        burst_milli: 2000,
        reorder_ppm: 10_000,
        dup_ppm: 10_000,
        ..conditions(5, 1)
    };
    let mut duo = Duo::new(4, lossy, lossy, Config::loopback());
    let message: Vec<u8> = (0..MAX_MESSAGE)
        .map(|at| (at * 7 + at / 1000) as u8)
        .collect();
    let queued = duo.links[0].send_reliable(&message).is_ok();
    let mut got = Vec::new();
    duo.run(|duo| {
        got.extend(duo.links[1].take_messages());
        (got.is_empty() && duo.now().0 < 60_000_000).then(|| duo.now().plus(1_000_000))
    });
    probes.check(
        "link: largest message",
        "one 4 MiB message (3814 slices) crosses 5% bursty loss with duplication and reordering, unpaced, intact",
        queued && got.len() == 1 && got[0] == message,
        format!(
            "{} delivered at {:.3} s, {} slices resent, {} stalls",
            got.len(),
            duo.now().0 as f64 / 1e6,
            duo.links[0].stats().resends,
            duo.stalls
        ),
    );

    // Hostile slices.
    let data = [0u8; SLICE];
    let beyond = Slice {
        id: MAX_IN_FLIGHT as u16,
        part: 0,
        count: 1,
        data: &data[..10],
    };
    let mut victim = Link::new(SESSION, NetTime(0));
    let first = victim
        .receive(NetTime(0), &crafted(SESSION, 0, &[beyond]))
        .err();
    let after = victim.receive(NetTime(1), &crafted(SESSION, 1, &[])).err();
    let mut victim = Link::new(SESSION, NetTime(0));
    let one = Slice {
        id: 3,
        part: 0,
        count: 5,
        data: &data,
    };
    let other = Slice { count: 6, ..one };
    let _ = victim.receive(NetTime(0), &crafted(SESSION, 0, &[one]));
    let changed = victim
        .receive(NetTime(1), &crafted(SESSION, 1, &[other]))
        .err();
    probes.check(
        "link: contradictions break the link",
        "a message id 256 ahead, or a slice count that changes, breaks the link, and it takes nothing after",
        [&first, &after, &changed]
            .iter()
            .all(|error| matches!(error, Some(LinkError::Broken(_)))),
        format!("{first:?}; {after:?}; {changed:?}"),
    );

    // Slices of messages that never complete: held bytes grow to the
    // window, then the link breaks rather than hold more.
    let mut victim_more = Link::new(SESSION, NetTime(0));
    let mut held_more = 0;
    let mut broke_more = None;
    let mut seq = 0u16;
    'more: for part in 0..MAX_PARTS - 1 {
        for id in 1..MAX_IN_FLIGHT as u16 {
            let slice = Slice {
                id,
                part,
                count: MAX_PARTS,
                data: &data,
            };
            if let Err(LinkError::Broken(_)) =
                victim_more.receive(NetTime(0), &crafted(SESSION, seq, &[slice]))
            {
                broke_more = Some(held_more);
                break 'more;
            }
            held_more = held_more.max(victim_more.held_bytes());
            seq = seq.wrapping_add(1);
        }
    }
    probes.check(
        "link: held bytes are bounded",
        "slices of 255 never-completed 4 MiB messages are held only up to the 8 MiB window; one more breaks the link",
        broke_more.is_some_and(|held| held <= MAX_WINDOW_BYTES && held + SLICE > MAX_WINDOW_BYTES),
        format!(
            "most held {:.2} MiB when it broke (window {} MiB)",
            held_more as f64 / (1 << 20) as f64,
            MAX_WINDOW_BYTES >> 20
        ),
    );

    // Wrong session, duplicates, and acks that are not ours.
    let mut victim = Link::new(SESSION, NetTime(0));
    let foreign = victim
        .receive(NetTime(0), &crafted(SESSION ^ 1, 0, &[]))
        .map(|r| r.seq);
    let tick = {
        let mut out = crafted(SESSION, 0, &[]);
        wire::put_frame(&mut out, frame::SNAPSHOT, b"tick");
        out
    };
    let first = victim
        .receive(NetTime(1), &tick)
        .map(|r| r.frames().count());
    let again = victim
        .receive(NetTime(2), &tick)
        .map(|r| r.frames().count());
    let mut acking = Vec::new();
    wire::Header {
        session: SESSION,
        seq: 1,
        ack: Some(wire::Ack {
            seq: 500,
            bits: u32::MAX,
            delay: 0,
        }),
    }
    .encode(&mut acking);
    let _ = victim.receive(NetTime(3), &acking);
    let phantom = victim.take_acked().count();
    probes.check(
        "link: foreign, duplicate and phantom",
        "another session's packet is refused and not counted; a duplicate's frames are not returned twice; acks for packets never sent are ignored",
        foreign.as_ref().err() == Some(&LinkError::Session { got: SESSION ^ 1 })
            && first == Ok(1)
            && again == Err(LinkError::Duplicate)
            && phantom == 0
            && victim.stats().packets_received == 2,
        format!("foreign {foreign:?}, first {first:?}, again {again:?}, phantom acks {phantom}"),
    );

    // A peer that has received nothing must not ack our packet 0.
    let mut a = Link::new(SESSION, NetTime(0));
    let mut b = Link::new(SESSION, NetTime(0));
    let _ = a.send_reliable(b"hello");
    let from_a = a.poll_transmit(NetTime(0));
    let from_b = b.poll_transmit(NetTime(link::KEEPALIVE_US));
    let taken = from_b
        .as_ref()
        .map(|packet| a.receive(NetTime(link::KEEPALIVE_US), packet).is_ok());
    probes.check(
        "link: no ack before receipt",
        "a peer's packets before it has received anything carry no ack, so our packet 0 is not taken as delivered",
        from_a.is_some()
            && taken == Some(true)
            && a.take_acked().count() == 0
            && a.reliable_in_flight() == 1,
        format!(
            "b's first packet {} B, a's window still {}",
            from_b.map_or(0, |p| p.len()),
            a.reliable_in_flight()
        ),
    );
    probes
}

// ---------------------------------------------------------------------------
// 6. Loopback.

/// Two links over an in-process `Loopback` pair.
pub fn loopback() -> Probes {
    let mut probes = Probes::default();
    let time = Arc::new(AtomicU64::new(0));
    let clock_time = time.clone();
    let clock: Clock = Arc::new(move || NetTime(clock_time.load(Ordering::Relaxed)));
    let (mut a, mut b) = Loopback::pair(clock);
    let mut links = [
        Link::with_config(SESSION, NetTime(0), Config::loopback()),
        Link::with_config(SESSION, NetTime(0), Config::loopback()),
    ];
    let mut stream = Stream::new(31);
    let mut expect = Stream::new(31);
    for _ in 0..200 {
        let _ = links[0].send_reliable(&stream.next());
    }
    let mut delivered = 0;
    let mut wrong = 0;
    let mut buf = Vec::new();
    for step in 0..10_000u64 {
        let now = NetTime(step * 100);
        time.store(now.0, Ordering::Relaxed);
        for (link, end) in links.iter_mut().zip([&mut a, &mut b]) {
            while let Some(datagram) = end.recv() {
                let _ = link.receive(datagram.at, &datagram.bytes);
            }
            while link.poll_transmit_into(now, &mut buf) {
                let to = end.remote();
                end.send(to, &buf);
            }
        }
        for message in links[1].take_messages() {
            wrong += u32::from(message != expect.next());
            delivered += 1;
        }
    }
    a.send(Peer::Local(1), &[0; MAX_DATAGRAM + 1]);
    a.send(Peer::Local(7), &[0; 10]);
    probes.check(
        "loopback: a link pair",
        "200 messages over Loopback arrive in order, intact; a datagram over 1200 bytes or to another address is dropped",
        delivered == 200 && wrong == 0 && a.dropped() == 2,
        format!("{delivered} delivered, {wrong} wrong, {} dropped", a.dropped()),
    );
    probes
}

// ---------------------------------------------------------------------------
// 7. Determinism.

/// A short hostile run's digest of every datagram and final counts.
fn fingerprint(seed: u64) -> ([u8; 32], link::Stats) {
    let mut duo = Duo::new(seed, hostile(seed), hostile(seed + 1), Config::default());
    let mut stream = Stream::new(seed);
    duo.run(|duo| {
        if duo.now().0 >= 20_000_000 {
            return None;
        }
        while duo.links[0].reliable_in_flight() < 16 {
            let _ = duo.links[0].send_reliable(&stream.next());
        }
        duo.links[1].take_messages().for_each(drop);
        Some(duo.now().plus(TICK_US))
    });
    (*duo.digest.finalize().as_bytes(), duo.links[0].stats())
}

/// The same seed gives the same datagrams.
pub fn determinism() -> Probes {
    let mut probes = Probes::default();
    let (a, stats_a) = fingerprint(40);
    let (b, stats_b) = fingerprint(40);
    let (c, _) = fingerprint(41);
    probes.check(
        "determinism: same seed, same bytes",
        "two 20-second hostile runs from one seed send the same datagrams byte for byte; another seed does not",
        a == b && stats_a == stats_b && a != c,
        format!(
            "digest {} ({} packets)",
            a[..8].iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
            stats_a.packets_sent
        ),
    );
    probes
}

// ---------------------------------------------------------------------------
// 8. Mutation.

/// Valid packets of every kind: a hostile session's first datagrams and
/// every handshake packet.
fn corpus() -> Vec<Vec<u8>> {
    let mut duo = Duo::new(50, hostile(50), hostile(51), Config::default());
    duo.tap_limit = 600;
    let mut streams = [Stream::new(52), Stream::new(53)];
    let mut ticks = 0u64;
    duo.run(|duo| {
        if duo.tap.len() >= duo.tap_limit || duo.now().0 > 30_000_000 {
            return None;
        }
        for (link, stream) in duo.links.iter_mut().zip(&mut streams) {
            while link.reliable_in_flight() < 8 {
                let _ = link.send_reliable(&stream.next());
            }
            link.take_messages().for_each(drop);
            let _ = link.queue_unreliable(frame::COMMANDS, &ticks.to_le_bytes());
        }
        ticks += 1;
        Some(duo.now().plus(TICK_US))
    });
    let mut packets = duo.tap;
    let credentials = host_credentials();
    let handshakes = [
        Handshake::Hello { nonce: NONCE },
        Handshake::Challenge {
            nonce: NONCE,
            expiry: 5_000_000,
            cookie: [3; wire::COOKIE_LEN],
        },
        Handshake::Connect {
            nonce: NONCE,
            expiry: 5_000_000,
            cookie: [3; wire::COOKIE_LEN],
            proof: Some([4; PROOF_LEN]),
            credentials,
        },
        Handshake::Accept {
            nonce: NONCE,
            session: SESSION,
        },
        Handshake::Deny {
            nonce: NONCE,
            refusal: Refusal::GameData {
                game: "k1".to_string(),
                groups: vec!["override".to_string(), "data".to_string()],
            },
        },
        Handshake::Deny {
            nonce: NONCE,
            refusal: Refusal::Build {
                host: "a".to_string(),
                here: "b".to_string(),
            },
        },
        Handshake::Deny {
            nonce: NONCE,
            refusal: Refusal::Protocol { host: 1, here: 2 },
        },
        Handshake::Deny {
            nonce: NONCE,
            refusal: Refusal::Password { given: false },
        },
        Handshake::Deny {
            nonce: NONCE,
            refusal: Refusal::InUse,
        },
    ];
    packets.extend(handshakes.iter().filter_map(|packet| packet.encode().ok()));
    packets
}

const NONCE: u64 = 0x0bad_c0ff_ee00_d00d;

/// One corruption of a corpus packet.
fn mutate(rng: &mut SimRng, corpus: &[Vec<u8>], out: &mut Vec<u8>) {
    let pick = |rng: &mut SimRng| &corpus[rng.below(corpus.len() as u32) as usize];
    out.clear();
    out.extend_from_slice(pick(rng));
    let below = |rng: &mut SimRng, n: usize| rng.below(n.max(1) as u32) as usize;
    let rounds = 1 + below(rng, 3);
    for _ in 0..rounds {
        match rng.below(7) {
            0 => {
                for _ in 0..1 + below(rng, 4) {
                    if !out.is_empty() {
                        let at = below(rng, out.len());
                        out[at] ^= 1 << rng.below(8);
                    }
                }
            }
            1 => {
                let len = below(rng, out.len() + 1);
                out.truncate(len);
            }
            2 => {
                let other = pick(rng);
                let cut = below(rng, out.len() + 1);
                let from = below(rng, other.len() + 1);
                out.truncate(cut);
                out.extend_from_slice(&other[from..]);
            }
            3 => {
                let at = below(rng, out.len() + 1);
                let len = (1 + below(rng, 8)).min(out.len() - at);
                let fill = [0xff, 0x80, 0x00, rng.next_u32() as u8][below(rng, 4)];
                out[at..at + len].fill(fill);
            }
            4 => {
                let at = below(rng, out.len() + 1);
                let extra: Vec<u8> = (0..1 + below(rng, 64))
                    .map(|_| rng.next_u32() as u8)
                    .collect();
                out.splice(at..at, extra);
            }
            5 => {
                if !out.is_empty() {
                    let start = below(rng, out.len());
                    let end = start + below(rng, out.len() - start + 1);
                    let copy = out[start..end].to_vec();
                    let at = below(rng, out.len() + 1);
                    out.splice(at..at, copy);
                }
            }
            _ => {
                // A plausible length or count, anywhere.
                if out.len() >= 2 {
                    let at = below(rng, out.len() - 1);
                    let value = [0x7f, 0xff, 0x01, 0x00][below(rng, 4)];
                    out[at] = value;
                    out[at + 1] = [0x7f, 0x01, 0xff][below(rng, 3)];
                }
            }
        }
    }
}

/// What the mutation run saw, per target.
#[derive(Debug, Clone, Default)]
struct MutationCounts {
    panics: u64,
    first_panic: Option<String>,
    limits_broken: u64,
    handshake_ok: u64,
    header_ok: u64,
    link_taken: u64,
    link_broken: u64,
    server_replies: u64,
    server_amplified: u64,
    client_done: u64,
    netem_ok: u64,
    most_held: usize,
    largest_message: usize,
}

/// Every decoder and state machine on one input; returns how many limits
/// the decoded values break (must be 0).
struct Targets {
    link: Link,
    server: handshake::Server,
    clients: Vec<Client>,
    now: NetTime,
    buf: Vec<u8>,
    /// The next packet number given to a renumbered input.
    seq: u16,
    renumbered: Vec<u8>,
}

impl Targets {
    fn new() -> Targets {
        Targets {
            link: Link::new(SESSION, NetTime(0)),
            server: handshake::Server::new([1; 32], None),
            clients: Targets::clients(),
            now: NetTime(0),
            buf: Vec::new(),
            seq: 1000,
            renumbered: Vec::new(),
        }
    }

    /// Clients waiting for a Challenge and for an Accept, both from
    /// `Local(0)` with the corpus nonce.
    fn clients() -> Vec<Client> {
        let challenge = bytes(
            Handshake::Challenge {
                nonce: NONCE,
                expiry: 5_000_000,
                cookie: [3; wire::COOKIE_LEN],
            }
            .encode(),
        );
        [false, true]
            .into_iter()
            .filter_map(|challenged| {
                let key = Some(JoinKey::new("mutant"));
                let mut client =
                    Client::new(NetTime(0), Peer::Local(0), NONCE, host_credentials(), key).ok()?;
                client.poll_transmit(NetTime(0));
                if challenged {
                    client.receive(NetTime(0), Peer::Local(0), &challenge);
                }
                Some(client)
            })
            .collect()
    }

    fn run(&mut self, input: &[u8], counts: &mut MutationCounts) {
        self.now = self.now.plus(100);
        let mut broken = 0;
        let _ = wire::classify(input);
        let _ = wire::peek_prefix(input);
        if let Ok(packet) = Handshake::decode(input) {
            counts.handshake_ok += 1;
            broken += handshake_limits(&packet);
        }
        let _ = Refusal::decode(&mut Reader::new(input));
        let _ = Credentials::decode(&mut Reader::new(input));
        let mut reader = Reader::new(input);
        while reader.varint("probe").is_ok() {}
        if let Ok((_, body)) = wire::Header::decode(input) {
            counts.header_ok += 1;
            for frame in Frames::new(body).map_while(Result::ok) {
                if let Ok(slice) = Slice::decode(frame.payload) {
                    let total = (slice.count as usize - 1) * SLICE + slice.data.len();
                    broken += u64::from(slice.data.len() > SLICE || total > MAX_MESSAGE);
                }
            }
        }
        // As received, then renumbered as the link's next packet, so the
        // corrupt frames get past the duplicate check.
        self.renumbered.clear();
        self.renumbered.extend_from_slice(input);
        if self.renumbered.len() >= HEADER_LEN {
            self.renumbered[9..11].copy_from_slice(&self.seq.to_le_bytes());
            self.seq = self.seq.wrapping_add(1);
        }
        for bytes in [input, &self.renumbered[..]] {
            match self.link.receive(self.now, bytes) {
                Ok(received) => {
                    counts.link_taken += 1;
                    received.frames().for_each(drop);
                }
                Err(LinkError::Broken(_)) => {
                    counts.link_broken += 1;
                    self.link = Link::new(SESSION, self.now);
                }
                Err(_) => {}
            }
        }
        counts.most_held = counts.most_held.max(self.link.held_bytes());
        broken += u64::from(self.link.held_bytes() > MAX_WINDOW_BYTES);
        for message in self.link.take_messages() {
            counts.largest_message = counts.largest_message.max(message.len());
            broken += u64::from(message.len() > MAX_MESSAGE);
        }
        self.link.take_acked().for_each(drop);
        if self.link.reliable_in_flight() < 4 {
            let _ = self.link.send_reliable(&input[..input.len().min(3000)]);
        }
        while self.link.poll_transmit_into(self.now, &mut self.buf) {}

        let from = Peer::Local((self.now.0 / 100 % 64) as u32);
        let outcome = self.server.receive(self.now, from, input, |_, _, _| Ok(()));
        if let Some(reply) = &outcome.reply {
            counts.server_replies += 1;
            counts.server_amplified += u64::from(reply.len() > input.len());
        }
        for client in &mut self.clients {
            client.receive(self.now, Peer::Local(0), input);
        }
        if self.clients.iter().any(|client| client.outcome().is_some()) {
            counts.client_done += 1;
            self.clients = Targets::clients();
        }
        counts.netem_ok += u64::from(Conditions::parse(&String::from_utf8_lossy(input)).is_ok());
        counts.limits_broken += broken;
    }
}

fn handshake_limits(packet: &Handshake) -> u64 {
    let text = |text: &str| u64::from(text.len() > MAX_TEXT);
    match packet {
        Handshake::Connect { credentials, .. } => {
            u64::from(credentials.games.len() > MAX_GAMES)
                + text(&credentials.build)
                + text(&credentials.name)
                + credentials
                    .games
                    .iter()
                    .map(|game| {
                        text(&game.game)
                            + u64::from(game.groups.len() > MAX_GROUPS)
                            + game.groups.iter().map(|(name, _)| text(name)).sum::<u64>()
                    })
                    .sum::<u64>()
        }
        Handshake::Deny {
            refusal: Refusal::GameData { game, groups },
            ..
        } => {
            text(game)
                + u64::from(groups.len() > MAX_GROUPS)
                + groups.iter().map(|g| text(g)).sum::<u64>()
        }
        _ => 0,
    }
}

/// Conditions strings to corrupt alongside packets.
const NETEM_CORPUS: [&str; 3] = [
    "delay=50ms,jitter=5ms,loss=2%,burst=3,dup=0.5%,reorder=1%,rate=2000kbit,seed=7",
    "loss=100%,burst=1000,rate=8bit",
    "delay=10s,jitter=1s,seed=18446744073709551615",
];

thread_local! {
    /// This thread's panics are expected (`quietly`): not printed.
    static QUIET: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `f` under `catch_unwind` without printing its panic. The process's
/// panic hook is wrapped once, never swapped back and forth, and stays
/// silent only on a thread inside `quietly`: probes running on other
/// threads meanwhile (the session runs) still print theirs.
pub(crate) fn quietly<R>(f: impl FnOnce() -> R + std::panic::UnwindSafe) -> std::thread::Result<R> {
    static HOOK: std::sync::Once = std::sync::Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if !QUIET.with(std::cell::Cell::get) {
                previous(info);
            }
        }));
    });
    let was = QUIET.with(|quiet| quiet.replace(true));
    let result = catch_unwind(f);
    QUIET.with(|quiet| quiet.set(was));
    result
}

/// `count` corrupted packets through every decoder and both state
/// machines, with panics caught and counted.
pub fn mutation(count: u64) -> Probes {
    let mut probes = Probes::default();
    let mut corpus = corpus();
    corpus.extend(NETEM_CORPUS.iter().map(|text| text.as_bytes().to_vec()));
    let mut rng = SimRng::new(60, 0x6d75_7461);
    let mut counts = MutationCounts::default();
    let mut targets = Targets::new();
    let mut input = Vec::with_capacity(2 * MAX_DATAGRAM);
    for _ in 0..count {
        mutate(&mut rng, &corpus, &mut input);
        let result = quietly(AssertUnwindSafe(|| targets.run(&input, &mut counts)));
        if result.is_err() {
            counts.panics += 1;
            counts.first_panic.get_or_insert_with(|| {
                input
                    .iter()
                    .take(64)
                    .map(|byte| format!("{byte:02x}"))
                    .collect()
            });
            targets = Targets::new();
        }
    }
    let c = &counts;
    probes.check(
        "mutation: no panic",
        "a million seeded flips, cuts, splices, insertions and plausible lengths of recorded packets, through every decoder and both state machines, panic nowhere",
        c.panics == 0,
        format!(
            "{count} inputs from {} packets, {} panics{}",
            corpus.len(),
            c.panics,
            c.first_panic
                .as_ref()
                .map(|hex| format!(", first on {hex}"))
                .unwrap_or_else(String::new)
        ),
    );
    probes.check(
        "mutation: limits",
        "nothing decoded breaks a limit (texts, counts, slices, 4 MiB messages, 8 MiB held), and no server reply is longer than its input",
        c.limits_broken == 0 && c.server_amplified == 0 && c.most_held <= MAX_WINDOW_BYTES,
        format!(
            "{} limits broken, {} amplified replies; most held {:.2} MiB, largest message {} B",
            c.limits_broken,
            c.server_amplified,
            c.most_held as f64 / (1 << 20) as f64,
            c.largest_message
        ),
    );
    probes.check(
        "mutation: depth",
        "the corruptions reach past the first checks: some decode as handshakes, some are taken by the link, some break it, some end a client's attempt, some get server replies",
        [c.handshake_ok, c.link_taken, c.link_broken, c.client_done, c.server_replies]
            .iter()
            .all(|&n| n > 0),
        format!(
            "handshake decoded {}, session header decoded {}, taken by the link {}, broke the link {}, server replies {}, client attempts ended {}, netem strings parsed {}",
            c.handshake_ok,
            c.header_ok,
            c.link_taken,
            c.link_broken,
            c.server_replies,
            c.client_done,
            c.netem_ok
        ),
    );
    probes.measure("mutation.inputs", count as f64, "inputs");
    probes.measure("mutation.panics", c.panics as f64, "panics");
    probes
}
