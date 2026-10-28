//! Network conditions from a seed: latency, jitter, loss in bursts,
//! duplication, reordering and a rate cap, applied to outgoing datagrams
//! (design section 12, D15).
//!
//! `Conditions` is what `KJK_NETEM` (or `--netem`) says, for example
//! `delay=50ms,jitter=5ms,loss=2%,burst=3,dup=0.5%,reorder=1%,rate=2000kbit,seed=7`;
//! every key is optional. `Shaper` is the model of one direction: given a
//! datagram and the time it was sent, it decides whether and when it
//! arrives, and holds it until then. `Netem` wraps any `Transport` with a
//! shaper on its outgoing side; `io::SimNet` keeps one per direction.
//!
//! The model, per datagram, in this order:
//! 1. **Loss**: a two-state chain (good, bad) steps once per datagram; in
//!    the bad state the datagram is lost. From bad it returns to good with
//!    probability `1 / burst`, so a run of losses is `burst` datagrams
//!    long on average, and from good it turns bad with the probability
//!    that makes the long-run loss `loss`. A `burst` too short for the
//!    loss (under `1 / (1 - loss)`) is raised to it.
//! 2. **Rate**: with a cap, datagrams leave one after another at `rate`
//!    bits per second; one that would wait more than 100 ms in the queue
//!    is dropped (a router's tail drop).
//! 3. **Delay and jitter**: it arrives `delay` plus a uniform offset in
//!    `[-jitter, +jitter]` after it left, but never before a datagram
//!    sent earlier: jitter alone does not reorder.
//! 4. **Reorder**: with probability `reorder` a datagram is held back and
//!    arrives just after the next one (or 50 ms after its own time, if no
//!    next one comes).
//! 5. **Duplication**: with probability `dup` a copy arrives right after
//!    it.
//!
//! Every draw comes from one `sim::SimRng` stream seeded by `seed`, and
//! time only from the caller, so a seed replays the same conditions.
//! Probabilities are integers in parts per million and times in
//! microseconds: no float decides anything.

use crate::{Datagram, NetTime, Peer, Transport};
use sim::SimRng;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::fmt;

/// The environment variable the launcher reads conditions from.
pub const ENV_VAR: &str = "KJK_NETEM";
/// The longest a datagram waits behind the rate cap before it is dropped.
pub const QUEUE_LIMIT_US: u64 = 100_000;
/// The longest a reordered datagram waits for a next one to pass it.
pub const HOLD_LIMIT_US: u64 = 50_000;
/// The largest `delay`.
pub const MAX_DELAY_US: u64 = 10_000_000;
/// The largest `jitter`.
pub const MAX_JITTER_US: u64 = 1_000_000;
/// The longest mean loss run, in packets.
pub const MAX_BURST: u64 = 1000;
/// The highest rate cap, in bits per second.
pub const MAX_RATE: u64 = 100_000_000_000;
/// The `SimRng` stream netem draws from (the seed picks the sequence).
pub const STREAM: u64 = 0x006e_6574_656d;

const PPM: u64 = 1_000_000;

/// Network conditions for one direction. The default is a perfect
/// network: no delay, no loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Conditions {
    /// One-way delay, microseconds.
    pub delay_us: u64,
    /// The spread around `delay`, microseconds.
    pub jitter_us: u64,
    /// Long-run loss, parts per million.
    pub loss_ppm: u64,
    /// Mean run of lost datagrams, in thousandths of a datagram (at least
    /// 1000).
    pub burst_milli: u64,
    /// Chance a datagram is duplicated, parts per million.
    pub dup_ppm: u64,
    /// Chance a datagram is held past the next one, parts per million.
    pub reorder_ppm: u64,
    /// Bandwidth cap, bits per second; `None` is unlimited.
    pub rate_bps: Option<u64>,
    /// Seeds the stream every draw comes from.
    pub seed: u64,
}

impl Default for Conditions {
    fn default() -> Conditions {
        Conditions {
            delay_us: 0,
            jitter_us: 0,
            loss_ppm: 0,
            burst_milli: 1000,
            dup_ppm: 0,
            reorder_ppm: 0,
            rate_bps: None,
            seed: 0,
        }
    }
}

/// Why a conditions string was refused: the item and what is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetemError {
    pub item: String,
    pub reason: &'static str,
}

impl fmt::Display for NetemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "netem `{}`: {}", self.item, self.reason)
    }
}

impl std::error::Error for NetemError {}

impl Conditions {
    /// The conditions `KJK_NETEM` sets, if it is set and not empty.
    pub fn from_env() -> Result<Option<Conditions>, NetemError> {
        match std::env::var(ENV_VAR) {
            Ok(text) if !text.trim().is_empty() => Conditions::parse(&text).map(Some),
            _ => Ok(None),
        }
    }

    /// Parses `key=value` items separated by commas; empty items are
    /// ignored, unknown or repeated keys and malformed values refused.
    pub fn parse(text: &str) -> Result<Conditions, NetemError> {
        let mut conditions = Conditions::default();
        let mut seen: Vec<&str> = Vec::new();
        for item in text
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
        {
            let fail = |reason| NetemError {
                item: item.to_string(),
                reason,
            };
            let (key, value) = item
                .split_once('=')
                .ok_or_else(|| fail("expected key=value"))?;
            let (key, value) = (key.trim(), value.trim());
            if seen.contains(&key) {
                return Err(fail("repeated key"));
            }
            seen.push(key);
            match key {
                "delay" => {
                    conditions.delay_us = duration(value)
                        .filter(|&us| us <= MAX_DELAY_US)
                        .ok_or_else(|| fail("a duration up to 10s, e.g. 50ms"))?;
                }
                "jitter" => {
                    conditions.jitter_us = duration(value)
                        .filter(|&us| us <= MAX_JITTER_US)
                        .ok_or_else(|| fail("a duration up to 1s, e.g. 5ms"))?;
                }
                "loss" => {
                    conditions.loss_ppm =
                        percent(value).ok_or_else(|| fail("a percentage, e.g. 2%"))?
                }
                "dup" => {
                    conditions.dup_ppm =
                        percent(value).ok_or_else(|| fail("a percentage, e.g. 0.5%"))?
                }
                "reorder" => {
                    conditions.reorder_ppm =
                        percent(value).ok_or_else(|| fail("a percentage, e.g. 1%"))?;
                }
                "burst" => {
                    conditions.burst_milli = decimal(value, 3)
                        .filter(|milli| (1000..=MAX_BURST * 1000).contains(milli))
                        .ok_or_else(|| fail("a mean loss run from 1 to 1000 packets"))?;
                }
                "rate" => {
                    conditions.rate_bps = Some(
                        rate(value)
                            .filter(|bps| (8..=MAX_RATE).contains(bps))
                            .ok_or_else(|| fail("a rate from 8bit to 100gbit, e.g. 2000kbit"))?,
                    );
                }
                "seed" => {
                    conditions.seed = value.parse().map_err(|_| fail("a whole number"))?;
                }
                _ => return Err(fail("unknown key")),
            }
        }
        Ok(conditions)
    }

    /// The same conditions with every value inside the range `parse`
    /// accepts (the fields are public, so a caller may set any).
    pub fn clamped(self) -> Conditions {
        Conditions {
            delay_us: self.delay_us.min(MAX_DELAY_US),
            jitter_us: self.jitter_us.min(MAX_JITTER_US),
            loss_ppm: self.loss_ppm.min(PPM),
            burst_milli: self.burst_milli.clamp(1000, MAX_BURST * 1000),
            dup_ppm: self.dup_ppm.min(PPM),
            reorder_ppm: self.reorder_ppm.min(PPM),
            rate_bps: self.rate_bps.map(|bps| bps.clamp(8, MAX_RATE)),
            seed: self.seed,
        }
    }

    /// No delay, no loss, nothing: datagrams pass as sent.
    pub fn is_perfect(&self) -> bool {
        Conditions { seed: 0, ..*self } == Conditions::default()
    }
}

impl std::str::FromStr for Conditions {
    type Err = NetemError;

    fn from_str(text: &str) -> Result<Conditions, NetemError> {
        Conditions::parse(text)
    }
}

/// The canonical text: only what differs from a perfect network (and the
/// seed), which `parse` reads back to the same value.
impl fmt::Display for Conditions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let default = Conditions::default();
        let mut items = Vec::new();
        if self.delay_us != 0 {
            items.push(format!("delay={}", show_duration(self.delay_us)));
        }
        if self.jitter_us != 0 {
            items.push(format!("jitter={}", show_duration(self.jitter_us)));
        }
        if self.loss_ppm != 0 {
            items.push(format!("loss={}%", show_decimal(self.loss_ppm, 4)));
        }
        if self.burst_milli != default.burst_milli {
            items.push(format!("burst={}", show_decimal(self.burst_milli, 3)));
        }
        if self.dup_ppm != 0 {
            items.push(format!("dup={}%", show_decimal(self.dup_ppm, 4)));
        }
        if self.reorder_ppm != 0 {
            items.push(format!("reorder={}%", show_decimal(self.reorder_ppm, 4)));
        }
        if let Some(bps) = self.rate_bps {
            items.push(format!("rate={}", show_rate(bps)));
        }
        items.push(format!("seed={}", self.seed));
        f.write_str(&items.join(","))
    }
}

/// A non-negative decimal with at most `scale` fractional digits, times
/// `10^scale`, without floats.
fn decimal(text: &str, scale: u32) -> Option<u64> {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() || !digits(whole) || !digits(fraction) || fraction.len() > scale as usize {
        return None;
    }
    if text.contains('.') && fraction.is_empty() {
        return None;
    }
    let mut value = whole.parse::<u64>().ok()?.checked_mul(10u64.pow(scale))?;
    if !fraction.is_empty() {
        let pad = 10u64.pow(scale - fraction.len() as u32);
        value = value.checked_add(fraction.parse::<u64>().ok()? * pad)?;
    }
    Some(value)
}

/// `50ms`, `1.5ms`, `250us`, `2s`, in microseconds.
fn duration(text: &str) -> Option<u64> {
    if let Some(number) = text.strip_suffix("us") {
        decimal(number, 0)
    } else if let Some(number) = text.strip_suffix("ms") {
        decimal(number, 3)
    } else if let Some(number) = text.strip_suffix('s') {
        decimal(number, 6)
    } else {
        None
    }
}

/// `2%`, `0.5%`, in parts per million, at most 100%.
fn percent(text: &str) -> Option<u64> {
    decimal(text.strip_suffix('%')?, 4).filter(|&ppm| ppm <= PPM)
}

/// `2000kbit`, `1.5mbit`, `300bit`, `1gbit`, in bits per second.
fn rate(text: &str) -> Option<u64> {
    let (number, unit) = [
        ("gbit", 1_000_000_000),
        ("mbit", 1_000_000),
        ("kbit", 1000),
        ("bit", 1),
    ]
    .into_iter()
    .find_map(|(suffix, unit)| text.strip_suffix(suffix).map(|number| (number, unit)))?;
    let milli = decimal(number, 3)?;
    let bits = milli.checked_mul(unit)? / 1000;
    Some(bits)
}

fn show_decimal(value: u64, scale: u32) -> String {
    let unit = 10u64.pow(scale);
    let fraction = value % unit;
    if fraction == 0 {
        return (value / unit).to_string();
    }
    let digits = format!("{fraction:0width$}", width = scale as usize);
    format!("{}.{}", value / unit, digits.trim_end_matches('0'))
}

fn show_duration(us: u64) -> String {
    if us.is_multiple_of(1000) {
        format!("{}ms", us / 1000)
    } else {
        format!("{us}us")
    }
}

fn show_rate(bps: u64) -> String {
    if bps.is_multiple_of(1_000_000) {
        format!("{}mbit", bps / 1_000_000)
    } else if bps.is_multiple_of(1000) {
        format!("{}kbit", bps / 1000)
    } else {
        format!("{bps}bit")
    }
}

/// What a shaper did, counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShaperStats {
    /// Datagrams handed to it.
    pub offered: u64,
    /// Lost by the loss chain.
    pub lost: u64,
    /// Runs of consecutive losses (so `lost / loss_runs` is the mean run).
    pub loss_runs: u64,
    /// Dropped because the rate cap's queue was full.
    pub queue_drops: u64,
    /// Held past the next datagram.
    pub reordered: u64,
    /// Extra copies made.
    pub duplicated: u64,
    /// Datagrams released (copies included).
    pub delivered: u64,
    /// The longest a datagram waited behind the rate cap, microseconds.
    pub max_queue_us: u64,
}

/// A datagram on its way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InFlight {
    /// When it arrives.
    pub arrival: NetTime,
    /// Breaks ties between equal arrivals: the order it was scheduled in.
    order: u64,
    pub to: Peer,
    pub bytes: Vec<u8>,
}

impl Ord for InFlight {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.arrival, self.order).cmp(&(other.arrival, other.order))
    }
}

impl PartialOrd for InFlight {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// One direction's conditions and the datagrams in flight on it.
#[derive(Debug, Clone)]
pub struct Shaper {
    conditions: Conditions,
    /// Chance per datagram, ppm, to turn from good to bad.
    enter_bad: u64,
    /// Chance per datagram, ppm, to return from bad to good.
    leave_bad: u64,
    rng: SimRng,
    bad: bool,
    /// When the rate cap's bottleneck is free again.
    free_at: NetTime,
    /// The latest arrival scheduled in order (jitter never goes before it).
    last_arrival: NetTime,
    order: u64,
    queue: BinaryHeap<Reverse<InFlight>>,
    /// A reordered datagram waiting for the next one to pass it.
    held: Option<InFlight>,
    stats: ShaperStats,
}

impl Shaper {
    /// A shaper drawing from `conditions.seed` on the sequence `stream`
    /// (one per direction, so directions do not share draws).
    /// Values out of the ranges `parse` accepts are brought into them.
    pub fn new(conditions: Conditions, stream: u64) -> Shaper {
        let conditions = conditions.clamped();
        let (enter_bad, leave_bad) = chain(&conditions);
        Shaper {
            conditions,
            enter_bad,
            leave_bad,
            rng: SimRng::new(conditions.seed, STREAM ^ stream),
            bad: false,
            free_at: NetTime(0),
            last_arrival: NetTime(0),
            order: 0,
            queue: BinaryHeap::new(),
            held: None,
            stats: ShaperStats::default(),
        }
    }

    /// The conditions, as clamped.
    pub fn conditions(&self) -> &Conditions {
        &self.conditions
    }

    /// What it has done so far.
    pub fn stats(&self) -> ShaperStats {
        self.stats
    }

    /// Datagrams scheduled and not released yet (a held one included).
    pub fn in_flight(&self) -> usize {
        self.queue.len() + usize::from(self.held.is_some())
    }

    /// A draw in parts per million: true with probability `ppm / 10^6`.
    /// Draws nothing for 0 and 100%, so a condition that is off does not
    /// move the stream.
    fn chance(&mut self, ppm: u64) -> bool {
        match ppm {
            0 => false,
            PPM.. => true,
            _ => u64::from(self.rng.below(PPM as u32)) < ppm,
        }
    }

    /// Takes a datagram sent at `now` and schedules its fate.
    pub fn send(&mut self, now: NetTime, to: Peer, bytes: &[u8]) {
        self.stats.offered += 1;
        let was_bad = self.bad;
        self.bad = if was_bad {
            !self.chance(self.leave_bad)
        } else {
            self.chance(self.enter_bad)
        };
        if self.bad {
            self.stats.lost += 1;
            self.stats.loss_runs += u64::from(!was_bad);
            return;
        }
        let departure = match self.conditions.rate_bps {
            Some(bps) => {
                let start = now.max(self.free_at);
                let wait = start.since(now);
                if wait > QUEUE_LIMIT_US {
                    self.stats.queue_drops += 1;
                    return;
                }
                self.stats.max_queue_us = self.stats.max_queue_us.max(wait);
                let bits = bytes.len() as u64 * 8;
                self.free_at = start.plus((bits * 1_000_000).div_ceil(bps));
                self.free_at
            }
            None => now,
        };
        let jitter = self.conditions.jitter_us;
        // Clamped to a second, so the bound fits a u32.
        let spread = if jitter == 0 {
            0
        } else {
            u64::from(self.rng.below((2 * jitter + 1) as u32))
        };
        let arrival =
            NetTime((departure.0 + self.conditions.delay_us + spread).saturating_sub(jitter))
                .max(departure);
        if self.held.is_none() && self.chance(self.conditions.reorder_ppm) {
            self.stats.reordered += 1;
            self.held = Some(self.flight(arrival, to, bytes.to_vec()));
            return;
        }
        let arrival = arrival.max(self.last_arrival);
        self.last_arrival = arrival;
        let copy = self.chance(self.conditions.dup_ppm).then(|| bytes.to_vec());
        let flight = self.flight(arrival, to, bytes.to_vec());
        self.queue.push(Reverse(flight));
        if let Some(copy) = copy {
            self.stats.duplicated += 1;
            let flight = self.flight(arrival, to, copy);
            self.queue.push(Reverse(flight));
        }
        if let Some(held) = self.held.take() {
            let flight = self.flight(arrival, held.to, held.bytes);
            self.queue.push(Reverse(flight));
        }
    }

    fn flight(&mut self, arrival: NetTime, to: Peer, bytes: Vec<u8>) -> InFlight {
        self.order += 1;
        InFlight {
            arrival,
            order: self.order,
            to,
            bytes,
        }
    }

    /// Takes over `previous`'s datagrams in flight, its ordering and its
    /// counts, so new conditions on a path lose nothing already sent.
    pub fn adopt(&mut self, previous: Shaper) {
        self.queue = previous.queue;
        self.held = previous.held;
        self.order = previous.order;
        self.last_arrival = previous.last_arrival;
        self.free_at = previous.free_at;
        self.stats = previous.stats;
    }

    /// When the next datagram arrives, if any is in flight.
    pub fn next_due(&self) -> Option<NetTime> {
        let held = self
            .held
            .as_ref()
            .map(|held| held.arrival.plus(HOLD_LIMIT_US));
        let queued = self.queue.peek().map(|Reverse(flight)| flight.arrival);
        match (held, queued) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The next datagram that has arrived by `now`, in arrival order.
    pub fn pop_due(&mut self, now: NetTime) -> Option<InFlight> {
        if let Some(held) = self
            .held
            .take_if(|held| held.arrival.plus(HOLD_LIMIT_US) <= now)
        {
            let arrival = held.arrival.plus(HOLD_LIMIT_US);
            let flight = self.flight(arrival, held.to, held.bytes);
            self.queue.push(Reverse(flight));
        }
        if self.queue.peek()?.0.arrival > now {
            return None;
        }
        let Reverse(flight) = self.queue.pop()?;
        self.stats.delivered += 1;
        Some(flight)
    }
}

/// The loss chain's two transition chances, ppm: (good to bad, bad to
/// good), so that runs of losses average `burst` and the long-run loss is
/// `loss` (see the module docs).
fn chain(conditions: &Conditions) -> (u64, u64) {
    let loss = conditions.loss_ppm;
    match loss {
        0 => (0, PPM),
        PPM.. => (PPM, 0),
        _ => {
            // Leave with 1/burst, but no rarer than the loss allows with
            // entering capped at certainty: leave <= (1 - loss) / loss.
            let wanted = PPM * 1000 / conditions.burst_milli;
            let most = (PPM - loss) * PPM / loss;
            let leave = wanted.min(most).clamp(1, PPM);
            let enter = (loss * leave).div_ceil(PPM - loss).min(PPM);
            (enter, leave)
        }
    }
}

/// A transport whose outgoing datagrams pass through a `Shaper` first.
/// It has no clock: `poll(now)` sets the time sends are stamped with and
/// hands the inner transport what is due.
pub struct Netem<T> {
    inner: T,
    shaper: Shaper,
    now: NetTime,
}

impl<T: Transport> Netem<T> {
    /// Wraps `inner`: what is sent through it passes `conditions` first.
    pub fn new(inner: T, conditions: Conditions) -> Netem<T> {
        Netem {
            inner,
            shaper: Shaper::new(conditions, 0),
            now: NetTime(0),
        }
    }

    /// Moves time to `now` (never back) and sends every datagram due.
    pub fn poll(&mut self, now: NetTime) {
        self.now = self.now.max(now);
        while let Some(flight) = self.shaper.pop_due(self.now) {
            self.inner.send(flight.to, &flight.bytes);
        }
    }

    /// When `poll` next has something to send.
    pub fn next_due(&self) -> Option<NetTime> {
        self.shaper.next_due()
    }

    /// What the shaper has done so far.
    pub fn stats(&self) -> ShaperStats {
        self.shaper.stats()
    }

    /// The wrapped transport.
    pub fn inner(&self) -> &T {
        &self.inner
    }

    /// The wrapped transport, to use directly.
    pub fn inner_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: Transport> Transport for Netem<T> {
    fn local(&self) -> Peer {
        self.inner.local()
    }

    /// Schedules the datagram as sent at the last `poll`'s time.
    fn send(&mut self, to: Peer, bytes: &[u8]) {
        self.shaper.send(self.now, to, bytes);
    }

    fn recv(&mut self) -> Option<Datagram> {
        self.inner.recv()
    }
}
