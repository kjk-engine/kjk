//! Whole sessions in one thread, in virtual time over `io::SimNet`, from a
//! seed: a host stepping a world at 125 Hz through a `Server`, clients
//! joining, predicting and drawing at their own frame rates, modules
//! installed through the D13 flow, all under `netem` conditions. The same
//! seed runs the same session, byte for byte. `kjk net session` runs these
//! and prints the checks.
//!
//! `run` takes a `Scenario` (the world, the pilots, how levels are built)
//! and a `Setup` (who joins when, the link, how long); `determinism` is
//! the pinned determinism world (the arena, the proving ground installed a
//! third of the way in); the launcher adds a KOTOR module.

use std::collections::BTreeMap;

use sim::determinism::{self as pinned, GROUND};
use sim::{
    Command, Level, Recorder, Seat, SimRng, SimWorld, Snapshot, SnapshotHash, StepReason,
    TickInput, TickRate, step,
};

use super::client::{Client, ClientConfig, ClientEvent, ClientStats, ClockView, Memory};
use super::clock::Margins;
use super::msg::{
    ClockReport, Commands, CopyAssembly, Fragment, InstallRequest, LevelRequest, MAX_SAVE_PARTS,
    Message, Payload, SAVE_PART, SavePart,
};
use super::server::{
    CopyRefusal, CopySent, Server, ServerConfig, ServerEvent, ServerStats, TAKEOVER_QUIET_US,
};
use crate::codec::FrameHeader;
use crate::handshake::{self, Credentials, Refusal};
use crate::io::SimNet;
use crate::link::{self, Config, Link};
use crate::netem::Conditions;
use crate::probes::quietly;
use crate::wire::{self, Class, HEADER_LEN, frame};
use crate::{NetTime, Peer};

/// What a session is played in and with.
pub trait Scenario {
    /// The host's world, the level it is in, and what joiners build.
    fn start(&mut self) -> (SimWorld, Level, LevelRequest);
    /// A seat's command for a tick, seeing what its client shows (`None`:
    /// it presses nothing and keeps its view).
    fn command(&mut self, seat: Seat, tick: u64, view: Option<&Snapshot>) -> Option<Command>;
    /// Builds the level a joiner is told to.
    fn level(&mut self, request: &LevelRequest) -> Result<Level, String>;
    /// Plans a module install (the host's and each client's).
    fn plan(&mut self, request: &InstallRequest) -> Result<Level, String>;
    /// Whether the host installs a module after this step.
    fn install_after(&mut self, snapshot: &Snapshot) -> Option<InstallRequest>;
    /// A save the host makes after this step, as the bytes it sends its
    /// players (`Server::save_copy`, I30 P17); none by default.
    fn save_after(&mut self, _snapshot: &Snapshot) -> Option<Vec<u8>> {
        None
    }
    /// The state a reload puts in place of this install's (I30 P13): the
    /// host makes its world from it, numbered from the tick it stopped at,
    /// and every client takes it whole; none (an install) by default.
    fn restored(&mut self, _request: &InstallRequest) -> Option<Snapshot> {
        None
    }
}

/// One client: when it joins, how often it draws, how long its plans take.
#[derive(Debug, Clone, Copy)]
pub struct Joiner {
    pub at_us: u64,
    pub frame_us: u64,
    pub plan_us: u64,
}

/// A session's shape.
#[derive(Debug, Clone)]
pub struct Setup {
    pub name: String,
    pub clients: Vec<Joiner>,
    /// Each way, host to every client and back.
    pub conditions: Conditions,
    pub seconds: u64,
    /// Snapshots every this many ticks.
    pub every: u8,
    pub seed: u64,
    pub load_wait_us: u64,
    /// How long the host's own plan takes.
    pub host_plan_us: u64,
    /// The first client's datagrams to the host replaced by a corrupted
    /// copy, parts per million: a hostile client (`corrupt`). The host and
    /// the other clients must not notice.
    pub corrupt_ppm: u64,
    /// The second client's link cut, and how it comes back.
    pub cut: Option<Cut>,
    /// The host holds its world (a movie, a break: it steps no ticks)
    /// from and until these times (µs).
    pub hold: Option<(u64, u64)>,
    /// The host's frame, µs: it steps the ticks due at each frame, as a
    /// process stepping in its frames does (0: each on its tick).
    pub host_frame_us: u64,
    /// Frames that run long, the host's (when it has frames) and the
    /// clients'.
    pub hitches: Hitches,
    /// The clients' clock margins.
    pub margins: Margins,
}

/// The second client's link cut both ways from `from_us` until `until_us`;
/// it comes back from a new address (a new port), with its seat's token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cut {
    pub from_us: u64,
    pub until_us: u64,
    pub comeback: Comeback,
}

/// How a cut client comes back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comeback {
    /// It times out and joins again by itself (`ClientConfig::rejoin_us`);
    /// past the host's timeout, its seat is kept for it.
    Rejoins,
    /// Its process starts again at `until_us`, a new client from the new
    /// address with the old one's token, before the host's timeout: the
    /// host hands it the seat of the connection still open.
    Restarts,
}

/// A client's session, measured.
#[derive(Debug, Clone)]
pub struct ClientReport {
    pub seat: Option<Seat>,
    pub joined_at_us: u64,
    pub stats: ClientStats,
    pub link: link::Stats,
    /// Bytes per second up (to the host) and down, over its time joined.
    pub up_bps: f64,
    pub down_bps: f64,
    /// Rebuilt snapshots compared with the host's, and those that differed.
    pub checked: u64,
    pub wrong: u64,
    /// Each time the clocks started (a join, each resume).
    pub starts: Vec<Start>,
    /// The largest error (ticks) after converging, lead and delay.
    pub worst_after: (f64, f64),
    /// Lead and delay (ticks), the mean over the frames after the first
    /// start converged.
    pub mean_after: (f64, f64),
    pub clock: Option<ClockView>,
    pub memory_max: Memory,
    pub memory_end: Memory,
    pub ended: Option<Refusal>,
    /// Installs this client took part in: agreed, and late (loaded after
    /// the resume).
    pub installs: u64,
    pub late_loads: u64,
    /// Times the host held this client.
    pub holds: u64,
    /// Its seat before its link was cut and after it came back, if it did.
    pub reconnected: Option<(Option<Seat>, Option<Seat>)>,
    /// Why the host ended its connection from the old address.
    pub old_left: Option<Refusal>,
    /// The host's save copies it received whole, in order (I30 P17).
    pub copies: Vec<Vec<u8>>,
    /// The longest from the host's `installing` to this client's
    /// `Installing` event, µs: how long a save copy held an install up.
    pub install_delay_max_us: u64,
}

/// One run of a client's clocks, from a start (a join, a resume) to the
/// next install or the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    /// How long the clocks ran, µs.
    pub lasted_us: u64,
    /// When lead and remote delay were first both within half a tick of
    /// their targets, µs after the start.
    pub converged_us: Option<u64>,
}

impl Start {
    /// Converged within `limit_us`, or stopped (an install) before it could
    /// be told.
    pub fn converged_within(&self, limit_us: u64) -> bool {
        self.converged_us.is_some_and(|us| us <= limit_us) || self.lasted_us < limit_us
    }
}

/// A few host states and the ticks after them, for timing restores and
/// re-steps (`bench`).
#[derive(Clone)]
pub struct Sample {
    pub level: Level,
    pub snapshot: Snapshot,
    pub inputs: Vec<TickInput>,
}

/// What a session did.
pub struct Report {
    pub name: String,
    pub seconds: u64,
    pub ticks: u64,
    pub installs: u64,
    /// Steps that asked for a transition.
    pub asks: u64,
    /// Where the host's world ended: its module (empty: a stage), and each
    /// seat's avatar (metres).
    pub place: String,
    pub avatars: Vec<[f32; 3]>,
    /// The host's last snapshot, and its recording replayed.
    pub end: SnapshotHash,
    pub replay: Result<SnapshotHash, String>,
    pub clients: Vec<ClientReport>,
    pub server: ServerStats,
    pub server_ring_max: usize,
    pub events: Vec<String>,
    pub samples: Vec<Sample>,
    /// Datagrams delivered corrupted (`Setup::corrupt_ppm`).
    pub corrupted: u64,
    /// By seat: the view lags (ms) the host applied, with the ticks that
    /// had one: (seat, ticks, mean, max).
    pub lags: Vec<(u8, u64, f64, u16)>,
    /// The saves the host sent its players, in order (`Scenario::save_after`).
    pub saves: Vec<Vec<u8>>,
    /// Of `installs`, the reloads (`Scenario::restored`).
    pub reloads: u64,
    /// How the cut client came back, if one was cut.
    pub comeback: Option<Comeback>,
}

impl Report {
    /// Every client's rebuilt snapshots matched the host's (and there were
    /// some), and no rebuild failed its check.
    pub fn exact(&self) -> bool {
        self.clients
            .iter()
            .all(|c| c.checked > 0 && c.wrong == 0 && c.stats.check_failures == 0)
    }

    pub fn replayed(&self) -> bool {
        self.replay.as_ref().is_ok_and(|hash| *hash == self.end)
    }
}

enum Host {
    Stepping {
        next: NetTime,
    },
    Planning {
        due: NetTime,
        request: InstallRequest,
    },
    Waiting,
}

enum Pending {
    Join(LevelRequest),
    Install(InstallRequest),
}

struct Player {
    joiner: Joiner,
    at: Peer,
    client: Option<Client>,
    next_frame: NetTime,
    plan: Option<(NetTime, Pending)>,
    last_angles: [i16; 3],
    /// The clocks' current start, while they run.
    start: Option<NetTime>,
    starts: Vec<Start>,
    worst_after: (f64, f64),
    /// Lead and delay summed over the frames after converging, and the
    /// frames.
    sum_after: (f64, f64, u64),
    memory_max: Memory,
    checked: u64,
    wrong: u64,
    installs: u64,
    late_loads: u64,
    holds: u64,
    ended: Option<Refusal>,
    /// The address it sent from before a cut.
    old_at: Option<Peer>,
    reconnected: Option<(Option<Seat>, Option<Seat>)>,
    old_left: Option<Refusal>,
    copies: Vec<Vec<u8>>,
    install_delay_max_us: u64,
}

/// A client's clocks stop (an install, a new start, the end).
fn stop(start: &mut Option<NetTime>, starts: &mut [Start], now: NetTime) {
    if let (Some(start), Some(run)) = (start.take(), starts.last_mut()) {
        run.lasted_us = now.since(start);
    }
}

const SAMPLES: usize = 12;
/// How long a cut client tries to join again: inside the host's
/// `RECONNECT_US`.
const REJOIN_US: u64 = 30_000_000;
const SAMPLE_TICKS: usize = 25;

fn credentials(name: &str) -> Credentials {
    Credentials {
        build: "session-probe".to_string(),
        games: Vec::new(),
        name: name.to_string(),
        reconnect: None,
    }
}

fn wider(max: &mut Memory, now: Memory) {
    max.ring = max.ring.max(now.ring);
    max.seat_commands = max.seat_commands.max(now.seat_commands);
    max.reassembly = max.reassembly.max(now.reassembly);
    max.own = max.own.max(now.own);
    max.history = max.history.max(now.history);
    max.held_bytes = max.held_bytes.max(now.held_bytes);
    max.window_bytes = max.window_bytes.max(now.window_bytes);
}

/// Frames that run long (a loaded machine, a hitch): each frame, with
/// `ppm` in a million, lasts up to `max_us` longer (uniformly).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hitches {
    pub ppm: u32,
    pub max_us: u32,
}

impl Hitches {
    /// How much longer the next frame lasts.
    fn extra(&self, rng: &mut SimRng) -> u64 {
        if self.ppm > 0 && rng.below(1_000_000) < self.ppm {
            u64::from(rng.below(self.max_us.max(1)))
        } else {
            0
        }
    }
}

/// A host stepping in its frames: each frame steps the ticks due by its
/// start. Without a frame length, each tick is stepped when due.
struct HostFrames {
    frame_us: u64,
    hitches: Hitches,
    rng: SimRng,
    /// The current frame's start, and the next's.
    at: NetTime,
    next: NetTime,
}

impl HostFrames {
    /// A frame starts if one is due at `now`.
    fn advance(&mut self, now: NetTime) {
        if self.frame_us > 0 && now >= self.next {
            self.at = now;
            self.next = now.plus(self.frame_us + self.hitches.extra(&mut self.rng));
        }
    }

    /// Whether the tick due at `due` is stepped at `now`.
    fn steps(&self, due: NetTime, now: NetTime) -> bool {
        if self.frame_us == 0 {
            now >= due
        } else {
            due <= self.at
        }
    }

    /// When the host next steps the tick due at `due`.
    fn when(&self, due: NetTime, now: NetTime) -> NetTime {
        if self.steps(due, now) {
            now
        } else if self.frame_us == 0 {
            due
        } else {
            self.next
        }
    }
}

/// Plays `setup` in `scenario` (see the module docs).
pub fn run(scenario: &mut dyn Scenario, setup: &Setup) -> Report {
    let rate = TickRate::default();
    let (mut world, start_level, request) = scenario.start();
    let mut net = SimNet::new(setup.seed);
    let host_at = net.endpoint();
    let mut server = Server::new(
        ServerConfig {
            seats: world.snapshot().seats(),
            rate,
            every: setup.every,
            link: Config::default(),
            load_wait_us: setup.load_wait_us,
            secret: [setup.seed as u8 ^ 0x5a; 32],
            join_key: None,
            credentials: credentials("host"),
        },
        request,
    );
    let mut last = world.snapshot();
    let recording_from = |world: &SimWorld, start: &Snapshot, level: &Level| {
        Recorder::new(rate.hz(), start.clone(), level.id(), 100)
            .with_script(world.script_state().clone())
    };
    let mut recorder = recording_from(&world, &last, &start_level);
    // A reload ends a recording (it does not replay) and starts the next:
    // each with the level it starts in, and its end.
    let mut recordings: Vec<(sim::Recording, Level)> = Vec::new();
    let mut recording_level = start_level.clone();
    let mut reloads = 0;
    let mut level_now = start_level.clone();
    let mut installed_levels: Vec<Level> = Vec::new();
    let mut epoch = 0u16;
    let mut hashes: BTreeMap<(u16, u64), SnapshotHash> = BTreeMap::new();
    hashes.insert((epoch, last.tick), last.hash());
    server.stepped(NetTime(0), &TickInput::default(), &last);
    let mut host = Host::Stepping {
        next: NetTime(rate.msec(last.tick + 1) as u64 * 1000),
    };
    let mut host_frames = HostFrames {
        frame_us: setup.host_frame_us,
        hitches: setup.hitches,
        rng: SimRng::new(setup.seed, 0x4057),
        at: NetTime(0),
        next: NetTime(0),
    };
    let mut hitches = SimRng::new(setup.seed, 0x417c);
    let mut players: Vec<Player> = setup
        .clients
        .iter()
        .map(|joiner| {
            let at = net.endpoint();
            net.set_link(host_at, at, setup.conditions);
            Player {
                joiner: *joiner,
                at,
                client: None,
                next_frame: NetTime(joiner.at_us),
                plan: None,
                last_angles: [0; 3],
                start: None,
                starts: Vec::new(),
                worst_after: (0.0, 0.0),
                sum_after: (0.0, 0.0, 0),
                memory_max: Memory::default(),
                checked: 0,
                wrong: 0,
                installs: 0,
                late_loads: 0,
                holds: 0,
                ended: None,
                old_at: None,
                reconnected: None,
                old_left: None,
                copies: Vec::new(),
                install_delay_max_us: 0,
            }
        })
        .collect();
    let end = NetTime(setup.seconds * 1_000_000);
    let mut events = Vec::new();
    let mut installs = 0;
    let mut asks = 0;
    let mut lags: BTreeMap<u8, (u64, u64, u16)> = BTreeMap::new();
    let mut ring_max = 0;
    let mut samples: Vec<Sample> = Vec::new();
    let sample_every = (setup.seconds * u64::from(rate.hz()) / SAMPLES as u64).max(1);
    let mut now = NetTime(0);
    let ms = |at: NetTime| at.0 as f64 / 1000.0;
    let mut spoiler = SimRng::new(setup.seed, 0xc0ff);
    let mut corrupted = 0;
    let hostile = players.first().map(|player| player.at);
    let cut = setup.cut.filter(|_| players.len() > 1);
    let mut cut_state = 0;
    let client_config = ClientConfig {
        link: Config::default(),
        audit: true,
        keep: false,
        margins: setup.margins,
        rejoin_us: if cut.is_some() { REJOIN_US } else { 0 },
        key: None,
        nonce_key: [7; 32],
    };
    let mut saves: Vec<Vec<u8>> = Vec::new();
    let mut installing_at: Option<NetTime> = None;
    let mut holding = false;

    while now < end {
        // The second client's link, cut; it comes back from a new address.
        if let Some(cut) = cut {
            let state = u8::from(now.0 >= cut.from_us) + u8::from(now.0 >= cut.until_us);
            if state != cut_state {
                cut_state = state;
                let player = &mut players[1];
                if state == 1 {
                    let lost = Conditions {
                        loss_ppm: 1_000_000,
                        ..setup.conditions
                    };
                    net.set_link(host_at, player.at, lost);
                    player.old_at = Some(player.at);
                    player.reconnected =
                        Some((player.client.as_ref().and_then(Client::seat), None));
                } else {
                    player.at = net.endpoint();
                    net.set_link(host_at, player.at, setup.conditions);
                    if cut.comeback == Comeback::Restarts {
                        let token = player.client.as_ref().and_then(Client::token);
                        let mut credentials = credentials("client 1");
                        credentials.reconnect = token;
                        player.client = Client::connect(
                            credentials,
                            host_at,
                            setup.seed ^ 0x7e57,
                            now,
                            client_config,
                        )
                        .ok();
                        player.ended = None;
                    }
                }
                events.push(format!(
                    "{:.0} ms: client 1's link {}",
                    ms(now),
                    if state == 1 {
                        "cut"
                    } else {
                        "back, from a new address"
                    }
                ));
            }
        }
        // The host holds its world, and lets it go.
        if let Some((from, until)) = setup.hold {
            let held = now.0 >= from && now.0 < until;
            if held != holding {
                holding = held;
                if held {
                    server.hold();
                } else {
                    server.release(now);
                    host = Host::Stepping {
                        next: now.plus(u64::from(rate.msec(last.tick + 1)) * 1000),
                    };
                }
                events.push(format!(
                    "{:.0} ms: host {} after tick {}",
                    ms(now),
                    if held { "holds" } else { "lets go" },
                    last.tick
                ));
            }
        }
        net.advance_to(now);
        while let Some(datagram) = net.recv(host_at) {
            let ppm = if Some(datagram.from) == hostile {
                setup.corrupt_ppm
            } else {
                0
            };
            let bytes = corrupt(&mut spoiler, ppm, datagram.bytes, &mut corrupted);
            server.receive(datagram.at, datagram.from, &bytes);
        }
        for player in &mut players {
            if let Some(client) = &mut player.client {
                while let Some(datagram) = net.recv(player.at) {
                    client.receive(datagram.at, datagram.from, &datagram.bytes);
                }
            }
        }

        // The host.
        host_frames.advance(now);
        match &host {
            Host::Stepping { next } if !holding && host_frames.steps(*next, now) => {
                let next = *next;
                let tick = last.tick + 1;
                let input = server.commands_for(tick);
                for (seat, _) in input.iter() {
                    let lag = input.view_lag(seat);
                    if lag > 0 {
                        let entry = lags.entry(seat.0).or_insert((0u64, 0u64, 0u16));
                        entry.0 += 1;
                        entry.1 += u64::from(lag);
                        entry.2 = entry.2.max(lag);
                    }
                }
                last = step(
                    &mut world,
                    tick,
                    &input,
                    rate.msec(tick),
                    StepReason::Authority,
                );
                recorder.record(rate.msec(tick), &input, &last);
                asks += u64::from(last.transition.is_some());
                hashes.insert((epoch, tick), last.hash());
                server.stepped(now, &input, &last);
                if let Some(blob) = scenario.save_after(&last) {
                    let to = server.save_copy(&blob, None).map(|sent| sent.to);
                    events.push(format!(
                        "{:.0} ms: host saves, {} bytes, a copy for seats {to:?}",
                        ms(now),
                        blob.len()
                    ));
                    saves.push(blob);
                }
                for sample in samples.iter_mut() {
                    if sample.inputs.len() < SAMPLE_TICKS && sample.snapshot.tick < tick {
                        sample.inputs.push(input.clone());
                    }
                }
                if tick % sample_every == 0 && samples.len() < SAMPLES {
                    samples.push(Sample {
                        level: level_now.clone(),
                        snapshot: last.clone(),
                        inputs: Vec::new(),
                    });
                }
                host = match scenario.install_after(&last) {
                    Some(request) => {
                        events.push(format!(
                            "{:.0} ms: host installs {} after tick {tick}",
                            ms(now),
                            request.module
                        ));
                        server.installing(request.clone());
                        installing_at = Some(now);
                        Host::Planning {
                            due: now.plus(setup.host_plan_us),
                            request,
                        }
                    }
                    None => Host::Stepping {
                        next: next.plus(u64::from(rate.msec(tick + 1)) * 1000),
                    },
                };
            }
            Host::Planning { due, request } if now >= *due => {
                let request = request.clone();
                let restored = scenario.restored(&request);
                let reload = restored.is_some();
                let committed = scenario.plan(&request).and_then(|level| {
                    let after = match restored {
                        Some(mut snapshot) => {
                            snapshot.tick = last.tick;
                            world = SimWorld::from_snapshot(&snapshot, level.clone())
                                .map_err(|error| error.to_string())?;
                            world.snapshot()
                        }
                        None => world
                            .install(level.clone(), request.arrival())
                            .map_err(|error| error.to_string())?,
                    };
                    let announcement = sim::Announcement::new(&level, request.arrival(), &after)
                        .ok_or("the plan has no module")?;
                    Ok((level, after, announcement))
                });
                match committed {
                    Ok((level, after, announcement)) => {
                        if reload {
                            let next = recording_from(&world, &after, &level);
                            let ended = std::mem::replace(&mut recorder, next).finish(&last);
                            recordings.push((
                                ended,
                                std::mem::replace(&mut recording_level, level.clone()),
                            ));
                            reloads += 1;
                            events.push(format!(
                                "{:.0} ms: host reloads after tick {}",
                                ms(now),
                                after.tick
                            ));
                        } else {
                            recorder.install(announcement.clone());
                        }
                        epoch = epoch.wrapping_add(1);
                        hashes.insert((epoch, after.tick), after.hash());
                        server.installed(now, announcement, &after);
                        installed_levels.push(level.clone());
                        level_now = level;
                        last = after;
                        installs += 1;
                        host = Host::Waiting;
                    }
                    Err(error) => {
                        events.push(format!("{:.0} ms: host install failed: {error}", ms(now)));
                        break;
                    }
                }
            }
            Host::Waiting if server.can_step(now) => {
                host = Host::Stepping {
                    next: now.plus(u64::from(rate.msec(last.tick + 1)) * 1000),
                };
            }
            _ => {}
        }
        while let Some(event) = server.poll_event() {
            match event {
                ServerEvent::Resumed { tick, late } => {
                    events.push(format!(
                        "{:.0} ms: resumed after tick {tick}, {} late loader(s)",
                        ms(now),
                        late.len()
                    ));
                    for player in &mut players {
                        if late.contains(&player.at) {
                            player.late_loads += 1;
                        }
                    }
                }
                ServerEvent::Joined { peer, seat, .. } => {
                    events.push(format!("{:.0} ms: {peer:?} took seat {}", ms(now), seat.0));
                }
                ServerEvent::InstallDiffers { peer, epoch } => events.push(format!(
                    "{:.0} ms: {peer:?}'s install of epoch {epoch} differs",
                    ms(now)
                )),
                ServerEvent::Left { peer, why, .. } => {
                    events.push(format!("{:.0} ms: {peer:?} left: {why}", ms(now)));
                    for player in &mut players {
                        if player.old_at == Some(peer) {
                            player.old_left = Some(why.clone());
                        }
                    }
                }
                ServerEvent::Refused { peer, refusal } => {
                    events.push(format!("{:.0} ms: {peer:?} refused: {refusal}", ms(now)));
                }
                ServerEvent::Loaded { .. } => {}
            }
        }
        ring_max = ring_max.max(server.held());

        // The clients.
        for (index, player) in players.iter_mut().enumerate() {
            if player.client.is_none() && now.0 >= player.joiner.at_us {
                player.client = Client::connect(
                    credentials(&format!("client {index}")),
                    host_at,
                    setup.seed.wrapping_mul(31).wrapping_add(index as u64 + 1),
                    now,
                    client_config,
                )
                .ok();
            }
            let Some(client) = &mut player.client else {
                continue;
            };
            for (tick_epoch, tick, hash) in client.take_audit() {
                player.checked += 1;
                if hashes.get(&(tick_epoch, tick)) != Some(&hash) {
                    player.wrong += 1;
                }
            }
            while let Some(event) = client.poll_event() {
                match event {
                    ClientEvent::NeedLevel(request) => {
                        player.plan =
                            Some((now.plus(player.joiner.plan_us), Pending::Join(request)));
                    }
                    ClientEvent::Installing(request) => {
                        player.plan =
                            Some((now.plus(player.joiner.plan_us), Pending::Install(request)));
                        stop(&mut player.start, &mut player.starts, now);
                        let delay = installing_at.map_or(0, |at| now.since(at));
                        player.install_delay_max_us = player.install_delay_max_us.max(delay);
                    }
                    ClientEvent::SaveCopy(copy) => player.copies.push(copy),
                    ClientEvent::Installed { agreed, .. } => {
                        player.installs += u64::from(agreed);
                    }
                    ClientEvent::Held { .. } => {
                        player.holds += 1;
                        stop(&mut player.start, &mut player.starts, now);
                    }
                    ClientEvent::Playing { .. } => {
                        stop(&mut player.start, &mut player.starts, now);
                        player.start = Some(now);
                        player.starts.push(Start {
                            lasted_us: 0,
                            converged_us: None,
                        });
                    }
                    ClientEvent::Rejoining { .. } => {
                        events.push(format!("{:.0} ms: client {index} rejoins", ms(now)));
                    }
                    ClientEvent::Ended(why) => {
                        events.push(format!("{:.0} ms: client {index} ended: {why}", ms(now)));
                        player.ended = Some(why);
                    }
                    ClientEvent::Welcome(welcome) => {
                        if let Some((_, after)) = &mut player.reconnected {
                            *after = Some(welcome.seat);
                        }
                    }
                    _ => {}
                }
            }
            if let Some((due, _)) = &player.plan
                && now >= *due
                && let Some((_, pending)) = player.plan.take()
            {
                let level = match &pending {
                    Pending::Join(request) => scenario.level(request),
                    Pending::Install(request) => scenario.plan(request),
                };
                client.provide_level(now, level);
            }
            if now >= player.next_frame {
                player.next_frame = player
                    .next_frame
                    .plus(player.joiner.frame_us + setup.hitches.extra(&mut hitches));
                let command = client.seat().zip(client.next_tick()).map(|(seat, tick)| {
                    scenario
                        .command(seat, tick, client.predicted())
                        .unwrap_or(Command {
                            angles: player.last_angles,
                            ..Command::default()
                        })
                });
                let command = command.unwrap_or(Command::default());
                player.last_angles = command.angles;
                let _ = client.frame(now, command);
                if let (Some(view), Some(start), Some(run)) =
                    (client.clock(), player.start, player.starts.last_mut())
                {
                    let within = view.lead_error.abs() < 0.5 && view.delay_error.abs() < 0.5;
                    match run.converged_us {
                        None if within => run.converged_us = Some(now.since(start)),
                        Some(_) => {
                            let worst = &mut player.worst_after;
                            worst.0 = worst.0.max(view.lead_error.abs());
                            worst.1 = worst.1.max(view.delay_error.abs());
                            let sum = &mut player.sum_after;
                            *sum = (sum.0 + view.lead, sum.1 + view.delay, sum.2 + 1);
                        }
                        None => {}
                    }
                }
                wider(&mut player.memory_max, client.memory());
            }
        }

        // Sending.
        while let Some((to, bytes)) = server.poll_transmit(now) {
            net.send(host_at, to, &bytes);
        }
        for player in &mut players {
            if let Some(client) = &mut player.client {
                while let Some((to, bytes)) = client.poll_transmit(now) {
                    net.send(player.at, to, &bytes);
                }
            }
        }

        // The next moment anything happens.
        let mut next = end;
        let mut consider = |at: Option<NetTime>| {
            if let Some(at) = at {
                next = next.min(at);
            }
        };
        consider(match &host {
            Host::Stepping { next } => Some(host_frames.when(*next, now)),
            Host::Planning { due, .. } => Some(*due),
            Host::Waiting => Some(now.plus(1000)),
        });
        consider(net.next_arrival());
        consider(server.next_deadline());
        for player in &players {
            match &player.client {
                None => consider(Some(NetTime(player.joiner.at_us))),
                Some(client) => {
                    consider(Some(player.next_frame));
                    consider(client.next_deadline());
                    consider(player.plan.as_ref().map(|(due, _)| *due));
                }
            }
        }
        now = next.max(now.plus(1));
    }

    for player in &mut players {
        stop(&mut player.start, &mut player.starts, now);
    }
    let end_hash = last.hash();
    recordings.push((recorder.finish(&last), recording_level));
    // Each recording replays to its own end; the last one's is the report's.
    let mut replay = Err("no recording".to_string());
    for (recording, level) in &recordings {
        replay = recording
            .play_with(level, &mut |announcement| {
                installed_levels
                    .iter()
                    .find(|level| announcement.check(level).is_ok())
                    .cloned()
                    .ok_or_else(|| format!("no level for {}", announcement.module))
            })
            .map(|snapshot| snapshot.hash())
            .map_err(|divergence| {
                format!(
                    "diverged at tick {}{}",
                    divergence.tick,
                    divergence
                        .reason
                        .map_or_else(String::new, |reason| format!(": {reason}"))
                )
            })
            .and_then(|hash| {
                if hash == recording.end {
                    Ok(hash)
                } else {
                    Err(format!(
                        "a recording ends at {}, not {}",
                        hash.short(),
                        recording.end.short()
                    ))
                }
            });
        if replay.is_err() {
            break;
        }
    }

    let seconds_of =
        |player: &Player| (setup.seconds as f64 - player.joiner.at_us as f64 / 1e6).max(1e-3);
    let clients = players
        .iter()
        .map(|player| {
            let client = player.client.as_ref();
            let link = client
                .and_then(Client::link_stats)
                .unwrap_or(link::Stats::default());
            ClientReport {
                seat: client.and_then(Client::seat),
                joined_at_us: player.joiner.at_us,
                stats: client.map_or_else(ClientStats::default, Client::stats),
                link,
                up_bps: link.bytes_sent as f64 / seconds_of(player),
                down_bps: link.bytes_received as f64 / seconds_of(player),
                checked: player.checked,
                wrong: player.wrong,
                starts: player.starts.clone(),
                worst_after: player.worst_after,
                mean_after: {
                    let (lead, delay, frames) = player.sum_after;
                    let frames = frames.max(1) as f64;
                    (lead / frames, delay / frames)
                },
                clock: client.and_then(Client::clock),
                memory_max: player.memory_max,
                memory_end: client.map_or(Memory::default(), Client::memory),
                ended: player.ended.clone(),
                reconnected: player.reconnected,
                old_left: player.old_left.clone(),
                installs: player.installs,
                late_loads: player.late_loads,
                copies: player.copies.clone(),
                install_delay_max_us: player.install_delay_max_us,
                holds: player.holds,
            }
        })
        .collect();
    Report {
        name: setup.name.clone(),
        seconds: setup.seconds,
        ticks: recordings
            .iter()
            .map(|(recording, _)| recording.ticks.len() as u64)
            .sum(),
        installs,
        asks,
        place: last
            .place
            .as_ref()
            .map_or_else(String::new, |place| place.module.clone()),
        avatars: last
            .entities
            .iter()
            .filter(|entity| matches!(entity.kind, sim::Kind::Avatar(_)))
            .map(|entity| entity.position)
            .collect(),
        end: end_hash,
        replay,
        clients,
        server: server.stats(),
        server_ring_max: ring_max,
        events,
        samples,
        corrupted,
        lags: lags
            .into_iter()
            .map(|(seat, (ticks, sum, max))| (seat, ticks, sum as f64 / ticks.max(1) as f64, max))
            .collect(),
        saves,
        reloads,
        comeback: cut.map(|cut| cut.comeback),
    }
}

/// With `ppm` chance, a corrupted copy of `bytes` in their place (counted):
/// flipped bits, a cut, or random bytes spliced in; half the time the
/// session header is kept, so the corruption reaches the frames behind it
/// (with a fresh packet number, so the link does not drop it as a
/// duplicate).
fn corrupt(rng: &mut SimRng, ppm: u64, bytes: Vec<u8>, count: &mut u64) -> Vec<u8> {
    if ppm == 0 || u64::from(rng.below(1_000_000)) >= ppm || bytes.is_empty() {
        return bytes;
    }
    *count += 1;
    let mut out = bytes;
    let keep = if rng.below(2) == 0 && out.len() > HEADER_LEN {
        let seq = rng.below(65_536) as u16;
        out[9..11].copy_from_slice(&seq.to_le_bytes());
        HEADER_LEN
    } else {
        0
    };
    mutate(rng, &mut out, keep);
    out
}

/// One seeded mutation of `bytes` after the first `keep`.
fn mutate(rng: &mut SimRng, bytes: &mut Vec<u8>, keep: usize) {
    let span = |rng: &mut SimRng, len: usize| keep + rng.below((len - keep).max(1) as u32) as usize;
    match rng.below(4) {
        0 => {
            for _ in 0..1 + rng.below(4) {
                if bytes.len() > keep {
                    let at = span(rng, bytes.len());
                    bytes[at] ^= 1 << rng.below(8);
                }
            }
        }
        1 => {
            let at = span(rng, bytes.len());
            bytes.truncate(at);
        }
        2 => {
            let at = span(rng, bytes.len()).min(bytes.len());
            let noise: Vec<u8> = (0..1 + rng.below(16))
                .map(|_| rng.below(256) as u8)
                .collect();
            bytes.splice(at..at, noise);
        }
        _ => {
            if bytes.len() > keep {
                let at = span(rng, bytes.len());
                bytes[at] = rng.below(256) as u8;
            }
        }
    }
}

/// What the decoder mutation run found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Mutations {
    pub tried: u64,
    /// Must be 0.
    pub panics: u64,
    /// Corrupted inputs a decoder still took (legal bytes by chance).
    pub accepted: u64,
    /// Commands frames taken whose ticks do not run one by one up to their
    /// newest (the edges unmutated too). Must be 0.
    pub broken: u64,
}

/// Seeded corruptions of valid session bytes (every message, a commands
/// frame of `MAX_COMMANDS`, frames whose ticks end at the last `u64`, a
/// clock report, a snapshot payload with its seat commands, a fragment)
/// through their decoders, `count` in all, each under `catch_unwind`
/// (`quietly`: other threads' panics still print).
/// Commands frames also go, in session packets, to a host with a player
/// loaded, which takes them for the next tick.
pub fn decoder_mutations(count: u64, seed: u64) -> Mutations {
    let mut corpus = corpus();
    let mut rng = SimRng::new(seed, 0xdec0);
    let mut found = Mutations::default();
    let base = {
        let mut base = TickInput::default();
        base.insert(Seat(0), Command::default());
        base
    };
    let mut host = loaded_host();
    if let Some((_, _, snapshot)) = &host {
        corpus.push((Decoder::Commands, commands_from(0, snapshot.tick + 1)));
    }
    let edges: Vec<Vec<u8>> = [u64::MAX, u64::MAX - super::msg::MAX_COMMANDS as u64 + 1]
        .into_iter()
        .map(|first| commands_from(0, first))
        .collect();
    for bytes in &edges {
        match quietly(|| Commands::decode(bytes).map(|c| ticks_hold(&c))) {
            Ok(Ok(false)) => found.broken += 1,
            Ok(_) => {}
            Err(_) => found.panics += 1,
        }
        corpus.push((Decoder::Commands, bytes.clone()));
    }
    let mut seq = 1000u16;
    for _ in 0..count {
        let (kind, original) = &corpus[rng.below(corpus.len() as u32) as usize];
        let mut bytes = original.clone();
        mutate(&mut rng, &mut bytes, 0);
        let mut broken = false;
        let result = quietly(std::panic::AssertUnwindSafe(|| match kind {
            Decoder::Message => Message::decode(&bytes).is_ok(),
            Decoder::Commands => {
                if let Some((server, hand, snapshot)) = &mut host {
                    seq = seq.wrapping_add(1);
                    let mut packet = Vec::new();
                    wire::Header {
                        session: hand.link.session(),
                        seq,
                        ack: None,
                    }
                    .encode(&mut packet);
                    wire::put_frame(&mut packet, frame::COMMANDS, &bytes);
                    server.receive(NetTime(0), hand.at, &packet);
                    let _ = server.commands_for(snapshot.tick + 1);
                }
                let decoded = Commands::decode(&bytes);
                broken = decoded.as_ref().is_ok_and(|commands| !ticks_hold(commands));
                decoded.is_ok()
            }
            Decoder::Clock => ClockReport::decode(&bytes).is_ok(),
            Decoder::Payload => Payload::read(&bytes)
                .and_then(|payload| {
                    let _ = FrameHeader::read(payload.frame);
                    payload.read_seats(&base)
                })
                .is_ok(),
            Decoder::Fragment => Fragment::decode(&bytes).is_ok(),
        }));
        found.tried += 1;
        found.broken += u64::from(broken);
        match result {
            Ok(accepted) => found.accepted += u64::from(accepted),
            Err(_) => {
                found.panics += 1;
                host = loaded_host();
            }
        }
    }
    found
}

/// A commands frame of `epoch`: `MAX_COMMANDS` ticks from `first`, as far
/// as they go before the last `u64`.
fn commands_from(epoch: u16, first: u64) -> Vec<u8> {
    let mut commands = Commands::new(epoch, first.checked_sub(1));
    let mut tick = Some(first);
    while let Some(at) = tick.filter(|_| commands.len() < super::msg::MAX_COMMANDS) {
        let command = pinned::pilot(7, at % 1000, 1)
            .get(Seat(0))
            .copied()
            .unwrap_or(Command::default());
        commands.push(at, command, 60);
        tick = at.checked_add(1);
    }
    let mut bytes = Vec::new();
    commands.encode(&mut bytes);
    bytes
}

/// Whether a commands frame's ticks run one by one up to its newest.
fn ticks_hold(commands: &Commands) -> bool {
    let ticks: Vec<u64> = commands.iter().map(|(tick, _, _)| tick).collect();
    ticks
        .windows(2)
        .all(|pair| pair[0].checked_add(1) == Some(pair[1]))
        && ticks.last().is_none_or(|last| *last == commands.newest)
}

/// Which decoder a corpus entry is for.
#[derive(Debug, Clone, Copy)]
enum Decoder {
    Message,
    Commands,
    Clock,
    Payload,
    Fragment,
}

/// Valid encodings of everything the session sends.
fn corpus() -> Vec<(Decoder, Vec<u8>)> {
    let ground = pinned::proving_ground();
    let mut world = SimWorld::new(pinned::Setup::pinned().world, Level::arena())
        .unwrap_or_else(|error| panic!("the determinism world: {error}"));
    let before = world.snapshot();
    let mut input = TickInput::default();
    for seat in 0..4 {
        input.insert(
            Seat(seat),
            pinned::pilot(7, 1, 4)
                .get(Seat(seat))
                .copied()
                .unwrap_or(Command::default()),
        );
    }
    let after = step(&mut world, 1, &input, 8, StepReason::Authority);
    let installed = world.install(ground.clone(), None).ok();
    let announcement = installed
        .as_ref()
        .and_then(|after| sim::Announcement::new(&ground, None, after));
    let (a, b) = (
        crate::codec::Stepped::new(&before),
        crate::codec::Stepped::new(&after),
    );
    let mut payloads = Vec::new();
    for base in [None, Some(&a)] {
        let mut frame = Vec::new();
        crate::codec::encode_frame(0, &b, base, &mut frame);
        let mut payload = Vec::new();
        super::msg::put_payload_head(&mut payload, 1234, 2);
        super::msg::put_payload_body(&mut payload, &frame, &input, &TickInput::default());
        payloads.push(payload);
    }
    let request = InstallRequest {
        game: "kjk".into(),
        module: GROUND.into(),
        arrival: String::new(),
        seats: 4,
        players: 2,
    };
    let mut messages = vec![
        Message::Welcome(super::msg::Welcome {
            seat: Seat(1),
            seats: 4,
            hz: 125,
            every: 2,
            tick: 1234,
            epoch: 3,
            token: 99,
        }),
        Message::Level(LevelRequest::Stage(Level::arena().id())),
        Message::Snapshot(&payloads[0]),
        Message::Installing {
            request,
            payload: &payloads[1],
        },
        Message::Hold {
            payload: &payloads[1],
        },
        Message::Resume { tick: 99 },
        Message::Loaded {
            epoch: 1,
            tick: 7,
            check: crate::codec::Check([9; 8]),
        },
        Message::Resync,
        Message::Action(sim::action::Action::Take {
            container: 3,
            item: 0x7c00_0001,
        }),
        Message::Action(sim::action::Action::TakeAll(12)),
        Message::Notice("taken by Carth".into()),
        Message::Screen(sim::notice::Screen::Store {
            store: 7,
            markup: -10,
            markdown: 25,
        }),
        Message::Screen(sim::notice::Screen::Upgrade { item: None }),
        Message::Screen(sim::notice::Screen::MessageBox { strref: 42 }),
        Message::Sound(sim::notice::Sound::new(7, "c_tach_bat1")),
        Message::SaveCopy(SavePart {
            copy: 3,
            part: 0,
            parts: 2,
            bytes: &[7; SAVE_PART],
        }),
        Message::SaveCopy(SavePart {
            copy: 3,
            part: 1,
            parts: 2,
            bytes: &[9; 10],
        }),
    ];
    if let Some(announcement) = &announcement {
        messages.push(Message::Level(LevelRequest::Module(announcement.clone())));
        messages.push(Message::Installed {
            epoch: 1,
            announcement: announcement.clone(),
        });
    }
    let mut corpus: Vec<(Decoder, Vec<u8>)> = messages
        .iter()
        .map(|message| (Decoder::Message, message.to_bytes()))
        .collect();
    let mut commands = Commands::new(2, Some(40));
    for tick in 41..41 + super::msg::MAX_COMMANDS as u64 {
        let command = pinned::pilot(7, tick, 1)
            .get(Seat(0))
            .copied()
            .unwrap_or(Command::default());
        commands.push(tick, command, 60 + (tick % 3) as u16);
    }
    let mut bytes = Vec::new();
    commands.encode(&mut bytes);
    corpus.push((Decoder::Commands, bytes));
    let mut bytes = Vec::new();
    ClockReport {
        least: Some((77, -1500)),
        late: 2,
        substituted: 3,
        miss: Some(75),
    }
    .encode(&mut bytes);
    corpus.push((Decoder::Clock, bytes));
    for payload in &payloads {
        corpus.push((Decoder::Payload, payload.clone()));
        let chunk = &payload[..payload.len().min(super::msg::FRAGMENT)];
        let mut bytes = Vec::new();
        Fragment {
            tick: 1,
            index: 0,
            count: 1,
            bytes: chunk,
        }
        .encode(&mut bytes);
        corpus.push((Decoder::Fragment, bytes));
    }
    corpus
}

// ---------------------------------------------------------------------------
// Save copies.

/// Save copies in parts (I30 P17). A copy is split, coded, decoded and
/// joined again. A copy cut short is dropped whole when the next begins. A
/// part out of turn is not held. The decoder refuses malformed parts. A
/// host refuses an empty copy and one over the cap, and sends one to no one
/// when no player has joined.
pub fn copy_probes() -> crate::probes::Probes {
    let mut probes = crate::probes::Probes::default();
    let blob: Vec<u8> = (0..SAVE_PART * 3 + SAVE_PART / 2)
        .map(|at| (at * 7 % 251) as u8)
        .collect();
    let coded = |copy: u32, blob: &[u8]| -> Vec<Vec<u8>> {
        SavePart::split(copy, blob)
            .map(|part| Message::SaveCopy(part).to_bytes())
            .collect()
    };
    // The copies the messages join to, and the bytes still held after.
    let joined = |messages: &[Vec<u8>]| -> (Vec<Vec<u8>>, usize) {
        let mut assembly = CopyAssembly::default();
        let out = messages
            .iter()
            .filter_map(|bytes| match Message::decode(bytes) {
                Ok(Message::SaveCopy(part)) => assembly.take(&part),
                _ => None,
            })
            .collect();
        (out, assembly.held())
    };
    let whole = coded(1, &blob);
    let (out, held) = joined(&whole);
    probes.check(
        "save copies: joined",
        "four parts decode and join to the same bytes",
        whole.len() == 4 && out == [blob.clone()] && held == 0,
        format!(
            "{} parts, {} copies out, {held} bytes held",
            whole.len(),
            out.len()
        ),
    );
    let short = &blob[..SAVE_PART + 10];
    let mut cut = coded(7, &blob)[..2].to_vec();
    cut.extend(coded(8, short));
    let (out, held) = joined(&cut);
    probes.check(
        "save copies: cut copy dropped",
        "a copy cut short is dropped whole when the next begins",
        out == [short.to_vec()] && held == 0,
        format!("{} copies out, {held} bytes held", out.len()),
    );
    let (out, held) = joined(&whole[2..]);
    probes.check(
        "save copies: part out of turn",
        "parts without their first are not held",
        out.is_empty() && held == 0,
        format!("{} out, {held} bytes held", out.len()),
    );
    // (what, part, parts, bytes)
    let malformed = [
        ("a short middle part", 0, 2, 10),
        ("a part past the count", 2, 2, 10),
        ("too many parts", 0, MAX_SAVE_PARTS + 1, 10),
        ("an empty part", 0, 1, 0),
        ("a part too long", 0, 1, SAVE_PART + 1),
    ];
    let accepted: Vec<&str> = malformed
        .iter()
        .filter(|(_, part, parts, len)| {
            let bytes = vec![1; *len];
            let message = Message::SaveCopy(SavePart {
                copy: 1,
                part: *part,
                parts: *parts,
                bytes: &bytes,
            });
            Message::decode(&message.to_bytes()).is_ok()
        })
        .map(|(what, ..)| *what)
        .collect();
    probes.check(
        "save copies: decode rules",
        "malformed parts are refused",
        accepted.is_empty(),
        format!("accepted: {accepted:?}"),
    );
    let mut server = Server::new(
        ServerConfig {
            seats: 2,
            rate: TickRate::default(),
            every: 2,
            link: Config::default(),
            load_wait_us: 1_000_000,
            secret: [1; 32],
            join_key: None,
            credentials: credentials("Ana"),
        },
        LevelRequest::Stage(Level::arena().id()),
    );
    let over = server.save_copy(&vec![0; SAVE_PART * MAX_SAVE_PARTS + 1], None);
    let empty = server.save_copy(&[], None);
    let alone = server.save_copy(&blob, None);
    probes.check(
        "save copies: caps",
        "over 32 MiB and empty refused; with no player joined, sent to no one",
        matches!(over, Err(CopyRefusal::TooLarge { .. }))
            && empty == Err(CopyRefusal::Empty)
            && alone == Ok(CopySent::default())
            && server.name() == "Ana",
        format!("{over:?}, {empty:?}, {alone:?}"),
    );
    probes
}

// ---------------------------------------------------------------------------
// Seats and actions, by hand.

/// Where a hand-driven peer's host is (its handshake checks replies come
/// from it).
const HOST: Peer = Peer::Local(0);

/// A peer driven by hand: its handshake, then a link of its own; what the
/// host's welcome said.
struct Hand {
    at: Peer,
    link: Link,
    seat: Option<Seat>,
    token: Option<u64>,
}

impl Hand {
    /// Joins `server` from `at` at `now`, naming `token`; the refusal if
    /// it is refused.
    fn join(
        server: &mut Server,
        now: NetTime,
        at: Peer,
        token: Option<u64>,
    ) -> Result<Hand, Option<Refusal>> {
        let credentials = Credentials {
            reconnect: token,
            ..credentials("hand")
        };
        let nonce = match at {
            Peer::Local(index) => u64::from(index) + 1,
            Peer::Addr(_) | Peer::Steam(_) => 1,
        };
        let mut handshake =
            handshake::Client::new(now, HOST, nonce, credentials, None).map_err(|_| None)?;
        let mut early = Vec::new();
        for _ in 0..4 {
            if let Some(bytes) = handshake.poll_transmit(now) {
                server.receive(now, at, &bytes);
            }
            while let Some((to, bytes)) = server.poll_transmit(now) {
                if to != at {
                    continue;
                }
                match wire::classify(&bytes) {
                    Class::Handshake => handshake.receive(now, HOST, &bytes),
                    _ => early.push(bytes),
                }
            }
        }
        let session = match handshake.outcome() {
            Some(Ok(session)) => *session,
            Some(Err(refusal)) => return Err(Some(refusal.clone())),
            None => return Err(None),
        };
        let mut hand = Hand {
            at,
            link: Link::new(session, now),
            seat: None,
            token: None,
        };
        for bytes in early {
            let _ = hand.link.receive(now, &bytes);
        }
        hand.pump(server, now);
        Ok(hand)
    }

    /// Takes what the host sends it (the rest is dropped) and reads the
    /// welcome.
    fn pump(&mut self, server: &mut Server, now: NetTime) {
        while let Some((to, bytes)) = server.poll_transmit(now) {
            if to == self.at {
                let _ = self.link.receive(now, &bytes);
            }
        }
        for message in self.link.take_messages() {
            if let Ok(Message::Welcome(welcome)) = Message::decode(&message) {
                self.seat = Some(welcome.seat);
                self.token = Some(welcome.token);
            }
        }
    }

    /// Sends `message` reliably, straight into `server`.
    fn send(&mut self, server: &mut Server, now: NetTime, message: &Message<'_>) {
        let _ = self.link.send_reliable(&message.to_bytes());
        self.keep(server, now);
    }

    /// Sends what its link has due at `now` (a keepalive when idle).
    fn keep(&mut self, server: &mut Server, now: NetTime) {
        while let Some(bytes) = self.link.poll_transmit(now) {
            server.receive(now, self.at, &bytes);
        }
    }
}

/// A handshake from `at` naming `token` whose Accept never arrives (nor
/// what follows it): the host opens a connection its joiner never hears
/// of. Whether the host opened it.
fn unanswered(server: &mut Server, now: NetTime, at: Peer, token: Option<u64>) -> bool {
    let credentials = Credentials {
        reconnect: token,
        ..credentials("hand")
    };
    let Ok(mut handshake) = handshake::Client::new(now, HOST, 99, credentials, None) else {
        return false;
    };
    for _ in 0..2 {
        if let Some(bytes) = handshake.poll_transmit(now) {
            server.receive(now, at, &bytes);
        }
        while let Some((to, bytes)) = server.poll_transmit(now) {
            if to == at
                && matches!(
                    wire::Handshake::decode(&bytes),
                    Ok(wire::Handshake::Challenge { .. })
                )
            {
                handshake.receive(now, HOST, &bytes);
            }
        }
    }
    server
        .stats()
        .connections
        .iter()
        .any(|connection| connection.peer == at)
}

/// A host of one seat, its world stepped once.
fn one_seat() -> (Server, Snapshot) {
    let mut world = SimWorld::new(pinned::Setup::pinned().world, Level::arena())
        .unwrap_or_else(|error| panic!("the determinism world: {error}"));
    let snapshot = world.snapshot();
    let mut server = Server::new(
        ServerConfig {
            seats: 1,
            rate: TickRate::default(),
            every: 2,
            link: Config::default(),
            load_wait_us: 1_000_000,
            secret: [3; 32],
            join_key: None,
            credentials: credentials("host"),
        },
        LevelRequest::Stage(Level::arena().id()),
    );
    server.stepped(NetTime(0), &TickInput::default(), &snapshot);
    (server, snapshot)
}

/// A one-seat host with a player in it, loaded in its epoch.
fn loaded_host() -> Option<(Server, Hand, Snapshot)> {
    let (mut server, snapshot) = one_seat();
    let mut hand = Hand::join(&mut server, NetTime(0), Peer::Local(1), None).ok()?;
    let check = crate::codec::Stepped::new(&snapshot).check();
    let loaded = Message::Loaded {
        epoch: 0,
        tick: snapshot.tick,
        check,
    };
    hand.send(&mut server, NetTime(0), &loaded);
    let joined = server.stats().connections.first()?.loaded == Some(0);
    joined.then_some((server, hand, snapshot))
}

/// Why the host ended connections, by peer, from its events.
fn left(server: &mut Server) -> Vec<(Peer, Refusal)> {
    std::iter::from_fn(|| server.poll_event())
        .filter_map(|event| match event {
            ServerEvent::Left { peer, why, .. } => Some((peer, why)),
            _ => None,
        })
        .collect()
}

/// The host's seat rules on a host of one seat, peers driven by hand: a
/// protocol break frees its seat at once; a timeout keeps it for the
/// token (another player is refused, the token from a new address gets
/// it); a token from a new address takes over a connection still open
/// once it has gone quiet, and is refused while it is heard; a rejoin
/// whose accept was lost leaves the next attempt the same token and seat.
/// And players' actions do not cross a module change.
pub fn seat_probes() -> crate::probes::Probes {
    let mut probes = crate::probes::Probes::default();
    let seat = |result: &Result<Hand, Option<Refusal>>| match result {
        Ok(hand) => format!("seat {:?}", hand.seat.map(|seat| seat.0)),
        Err(refusal) => format!("refused: {refusal:?}"),
    };
    let full = |result: &Result<Hand, Option<Refusal>>| {
        matches!(result, Err(Some(Refusal::Full { seats: 1 })))
    };
    let seated = |result: &Result<Hand, Option<Refusal>>| {
        result.as_ref().is_ok_and(|hand| hand.seat == Some(Seat(0)))
    };

    // A protocol break: a reliable slice far past the window.
    let (mut server, _) = one_seat();
    let a = Hand::join(&mut server, NetTime(0), Peer::Local(1), None);
    if let Ok(a) = &a {
        let mut packet = Vec::new();
        wire::Header {
            session: a.link.session(),
            seq: 1,
            ack: None,
        }
        .encode(&mut packet);
        wire::Slice {
            id: 2000,
            part: 0,
            count: 1,
            data: &[],
        }
        .put_frame(&mut packet);
        server.receive(NetTime(1000), a.at, &packet);
    }
    let _ = server.poll_transmit(NetTime(1000));
    let why = left(&mut server);
    let b = Hand::join(&mut server, NetTime(2000), Peer::Local(2), None);
    probes.check(
        "seats: a protocol break frees the seat",
        "a player whose link broke the protocol is ended as such, and its seat is free at once for another",
        seated(&a)
            && matches!(why.as_slice(), [(Peer::Local(1), Refusal::Kicked(_))])
            && seated(&b),
        format!("ended {why:?}; another player: {}", seat(&b)),
    );

    // A timeout: the seat waits for the token.
    let (mut server, _) = one_seat();
    let a = Hand::join(&mut server, NetTime(0), Peer::Local(1), None);
    let token = a.as_ref().ok().and_then(|a| a.token);
    let later = NetTime(link::TIMEOUT_US + 1_000_000);
    while server.poll_transmit(later).is_some() {}
    let why = left(&mut server);
    let b = Hand::join(&mut server, later, Peer::Local(2), None);
    let back = Hand::join(&mut server, later, Peer::Local(3), token);
    probes.check(
        "seats: a timeout keeps the seat for its token",
        "a player who went quiet times out; another player is refused (the seat is kept), and the token from a new address takes it back",
        token.is_some()
            && why == [(Peer::Local(1), Refusal::Timeout)]
            && full(&b)
            && seated(&back),
        format!("ended {why:?}; another player: {}; the token: {}", seat(&b), seat(&back)),
    );

    // The token while the old connection is still open, and quiet.
    let (mut server, _) = one_seat();
    let a = Hand::join(&mut server, NetTime(0), Peer::Local(1), None);
    let token = a.as_ref().ok().and_then(|a| a.token);
    let soon = NetTime(2 * TAKEOVER_QUIET_US);
    let wrong = Hand::join(
        &mut server,
        soon,
        Peer::Local(4),
        token.map(|token| token ^ 1),
    );
    let back = Hand::join(&mut server, soon, Peer::Local(3), token);
    let why = left(&mut server);
    let b = Hand::join(&mut server, soon, Peer::Local(2), None);
    probes.check(
        "seats: a token takes over a quiet connection",
        "before the host timed the old connection out, but 2 s after it last heard it, its token from a new address ends it and takes its seat; a wrong token and another player are refused",
        token.is_some()
            && full(&wrong)
            && seated(&back)
            && matches!(why.as_slice(), [(Peer::Local(1), Refusal::Kicked(_))])
            && full(&b),
        format!(
            "wrong token: {}; the token: {}; ended {why:?}; another player: {}",
            seat(&wrong),
            seat(&back),
            seat(&b)
        ),
    );

    // The token while the old connection is still heard: refused, until
    // the old one has been quiet `TAKEOVER_QUIET_US`.
    let (mut server, _) = one_seat();
    let mut a = Hand::join(&mut server, NetTime(0), Peer::Local(1), None);
    let token = a.as_ref().ok().and_then(|a| a.token);
    let heard = NetTime(1_500_000);
    if let Ok(a) = &mut a {
        a.keep(&mut server, heard);
    }
    let early = Hand::join(&mut server, heard.plus(500_000), Peer::Local(3), token);
    let kept = left(&mut server);
    let late = Hand::join(
        &mut server,
        heard.plus(TAKEOVER_QUIET_US),
        Peer::Local(3),
        token,
    );
    let why = left(&mut server);
    probes.check(
        "seats: a token is refused while its connection is heard",
        "0.5 s after the host last heard the seat's player, its token from another address is refused (in use) and nothing ends; 1 s after, it takes the seat",
        token.is_some()
            && matches!(early, Err(Some(Refusal::InUse)))
            && kept.is_empty()
            && seated(&late)
            && matches!(why.as_slice(), [(Peer::Local(1), Refusal::Kicked(_))]),
        format!(
            "0.5 s: {}, ended {kept:?}; 1 s: {}, ended {why:?}",
            seat(&early),
            seat(&late)
        ),
    );

    // A rejoin whose accept is lost: the host opened the connection, the
    // joiner gave up on it and tries again from yet another address, just
    // before the host times that connection out, or just after.
    for (when, gap, timed) in [
        ("before", handshake::GIVE_UP_US - handshake::RETRY_US, false),
        ("after", handshake::GIVE_UP_US + handshake::RETRY_US, true),
    ] {
        let (mut server, _) = one_seat();
        let a = Hand::join(&mut server, NetTime(0), Peer::Local(1), None);
        let token = a.as_ref().ok().and_then(|a| a.token);
        let later = NetTime(link::TIMEOUT_US + 1_000_000);
        while server.poll_transmit(later).is_some() {}
        let timed_out = left(&mut server);
        let opened = unanswered(&mut server, later, Peer::Local(3), token);
        let again = later.plus(gap);
        let back = Hand::join(&mut server, again, Peer::Local(4), token);
        let why = left(&mut server);
        let other = Hand::join(&mut server, again, Peer::Local(2), None);
        let same_token = back.as_ref().is_ok_and(|back| back.token == token);
        let ended = match why.as_slice() {
            [(Peer::Local(3), Refusal::Timeout)] => timed,
            [(Peer::Local(3), Refusal::Kicked(_))] => !timed,
            _ => false,
        };
        probes.check(
            &format!("seats: a rejoin whose accept was lost ({when} the host's timeout)"),
            "the player timed out; its first rejoin is opened by the host but the accept never arrives; the next attempt, from another address with the same token, gets the seat and the same token back (taking over the quiet connection, or its seat kept after it timed out); another player is still refused",
            token.is_some()
                && timed_out == [(Peer::Local(1), Refusal::Timeout)]
                && opened
                && seated(&back)
                && same_token
                && ended
                && full(&other),
            format!(
                "first attempt opened {opened}; the next: {} (same token {same_token}); ended {why:?}; another player: {}",
                seat(&back),
                seat(&other)
            ),
        );
    }

    // Actions around a module change.
    let ground = pinned::proving_ground();
    let taken = loaded_host().and_then(|(mut server, mut hand, snapshot)| {
        let now = NetTime(1000);
        let act = |server: &mut Server, hand: &mut Hand, container: u32| {
            let action = sim::action::Action::TakeAll(container);
            hand.send(server, now, &Message::Action(action));
            server
                .actions()
                .into_iter()
                .map(|(_, action)| action)
                .collect::<Vec<_>>()
        };
        let running = act(&mut server, &mut hand, 1);
        let action = sim::action::Action::TakeAll(2);
        hand.send(&mut server, now, &Message::Action(action));
        server.installing(InstallRequest {
            game: "kjk".into(),
            module: GROUND.into(),
            arrival: String::new(),
            seats: 1,
            players: 1,
        });
        let queued = server.actions();
        let installing = act(&mut server, &mut hand, 3);
        let mut world = SimWorld::from_snapshot(&snapshot, Level::arena()).ok()?;
        let after = world.install(ground.clone(), None).ok()?;
        let announcement = sim::Announcement::new(&ground, None, &after)?;
        server.installed(now, announcement, &after);
        let unloaded = act(&mut server, &mut hand, 4);
        let loaded = Message::Loaded {
            epoch: 1,
            tick: after.tick,
            check: crate::codec::Stepped::new(&after).check(),
        };
        hand.send(&mut server, now, &loaded);
        let reloaded = act(&mut server, &mut hand, 5);
        Some((running, queued.len(), installing, unloaded, reloaded))
    });
    let take = |container| vec![sim::action::Action::TakeAll(container)];
    probes.check(
        "actions: not across a module change",
        "an action is taken while the world runs; one waiting when an install starts is dropped, as are those sent while it loads and before the player loaded the new module; after, they are taken again",
        taken.as_ref().is_some_and(|(running, queued, installing, unloaded, reloaded)| {
            *running == take(1)
                && *queued == 0
                && installing.is_empty()
                && unloaded.is_empty()
                && *reloaded == take(5)
        }),
        format!("{taken:?}"),
    );
    probes
}

// ---------------------------------------------------------------------------
// The determinism world.

/// The pinned determinism world (`sim::determinism`): four seats, 32
/// wanderers and the spawner in the arena, the proving ground installed
/// after `install_at`; seats driven by the determinism pilot.
pub struct Determinism {
    setup: pinned::Setup,
    install_at: u64,
    /// The host saves after this tick (`with_save`).
    save_at: Option<u64>,
    /// The host keeps the world after the first tick and reloads it in
    /// place after the second (`with_reload`), and the world kept.
    reload: Option<(u64, u64)>,
    kept: Option<Snapshot>,
    arena: Level,
    ground: Level,
}

impl Determinism {
    pub fn new(install_at: u64) -> Determinism {
        Determinism {
            setup: pinned::Setup::pinned(),
            install_at,
            save_at: None,
            reload: None,
            kept: None,
            arena: Level::arena(),
            ground: pinned::proving_ground(),
        }
    }

    /// The host also saves after `at`, if given.
    pub fn with_save(self, at: Option<u64>) -> Determinism {
        Determinism {
            save_at: at,
            ..self
        }
    }

    /// The host keeps its world after `keep` and reloads it after
    /// `reload` (ticks; I30 P13).
    pub fn with_reload(self, keep: u64, reload: u64) -> Determinism {
        Determinism {
            reload: Some((keep, reload)),
            ..self
        }
    }
}

impl Scenario for Determinism {
    fn start(&mut self) -> (SimWorld, Level, LevelRequest) {
        let world = SimWorld::new(self.setup.world, self.arena.clone())
            .unwrap_or_else(|error| panic!("the determinism world: {error}"));
        let request = LevelRequest::Stage(self.arena.id());
        (world, self.arena.clone(), request)
    }

    fn command(&mut self, seat: Seat, tick: u64, _view: Option<&Snapshot>) -> Option<Command> {
        pinned::pilot(self.setup.pilot, tick, self.setup.world.seats)
            .get(seat)
            .copied()
    }

    fn level(&mut self, request: &LevelRequest) -> Result<Level, String> {
        match request {
            LevelRequest::Stage(id) if *id == self.arena.id() => Ok(self.arena.clone()),
            LevelRequest::Module(announcement) if announcement.module == GROUND => {
                Ok(self.ground.clone())
            }
            other => Err(format!("not this world's level: {other:?}")),
        }
    }

    fn plan(&mut self, request: &InstallRequest) -> Result<Level, String> {
        if request.module == GROUND {
            Ok(self.ground.clone())
        } else {
            Err(format!("{}: not this world's module", request.module))
        }
    }

    fn install_after(&mut self, snapshot: &Snapshot) -> Option<InstallRequest> {
        let ground = |players: u8| InstallRequest {
            game: "kjk".to_string(),
            module: GROUND.to_string(),
            arrival: String::new(),
            seats: snapshot.seats(),
            players,
        };
        if snapshot.tick == self.install_at && snapshot.place.is_none() {
            return Some(ground(snapshot.players_for(GROUND)));
        }
        let (keep, reload) = self.reload?;
        if snapshot.tick == keep {
            self.kept = Some(snapshot.clone());
        }
        let kept = self.kept.as_ref().filter(|_| snapshot.tick == reload)?;
        Some(ground(kept.place.as_ref()?.players))
    }

    fn restored(&mut self, _request: &InstallRequest) -> Option<Snapshot> {
        self.kept.take()
    }

    /// Eight full parts and one byte more of the snapshot's bytes, over and
    /// over: sent whole, the install after it would wait 2 s at the budget.
    fn save_after(&mut self, snapshot: &Snapshot) -> Option<Vec<u8>> {
        (Some(snapshot.tick) == self.save_at).then(|| {
            let bytes = snapshot.encode();
            let len = 8 * SAVE_PART + 1;
            bytes.iter().copied().cycle().take(len).collect()
        })
    }
}

/// Netem conditions: one-way delay and jitter (ms), loss (percent) in
/// bursts, duplication and reordering (per mille).
pub fn conditions(delay_ms: u64, jitter_ms: u64, loss_percent: u64, burst: u64) -> Conditions {
    Conditions {
        delay_us: delay_ms * 1000,
        jitter_us: jitter_ms * 1000,
        loss_ppm: loss_percent * 10_000,
        burst_milli: burst.max(1) * 1000,
        ..Conditions::default()
    }
}

/// Clients at 60, 144 and 120 frames a second; the third joins at
/// `third_at_us` (none if 0).
pub fn joiners(third_at_us: u64, plan_us: u64) -> Vec<Joiner> {
    let mut joiners = vec![
        Joiner {
            at_us: 0,
            frame_us: 16_667,
            plan_us,
        },
        Joiner {
            at_us: 3_000,
            frame_us: 6_944,
            plan_us: plan_us + plan_us / 2,
        },
    ];
    if third_at_us > 0 {
        joiners.push(Joiner {
            at_us: third_at_us,
            frame_us: 8_333,
            plan_us: plan_us * 4,
        });
    }
    joiners
}

/// Times restoring a world from each sample and stepping its ticks
/// again as prediction does: (restore µs, re-step µs per tick) over every
/// sample, from a clock the caller supplies (in microseconds). A first,
/// untimed pass warms caches and allocators.
pub fn bench(samples: &[Sample], ticks: usize, clock: &dyn Fn() -> f64) -> (f64, f64) {
    let rate = TickRate::default();
    let pass = || {
        let (mut restore, mut stepping, mut restores, mut steps) = (0.0, 0.0, 0u32, 0u32);
        for sample in samples {
            let started = clock();
            let Ok(mut world) = SimWorld::from_snapshot(&sample.snapshot, sample.level.clone())
            else {
                continue;
            };
            let restored = clock();
            let mut tick = sample.snapshot.tick;
            for input in sample.inputs.iter().take(ticks) {
                tick += 1;
                let reason = StepReason::Predict;
                std::hint::black_box(step(&mut world, tick, input, rate.msec(tick), reason));
                steps += 1;
            }
            restore += restored - started;
            stepping += clock() - restored;
            restores += 1;
        }
        (
            restore / f64::from(restores.max(1)),
            stepping / f64::from(steps.max(1)),
        )
    };
    pass();
    pass()
}
