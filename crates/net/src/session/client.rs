//! The joining side of the session: the handshake, the level and the
//! keyframe, then the host's snapshots rebuilt exactly, the clocks, the
//! commands, prediction and the two presentations.
//!
//! **Joining.** After the handshake the host sends a welcome (seat, rate,
//! epoch), the level to build (`ClientEvent::NeedLevel`) and a keyframe.
//! The caller builds the level (a plan, on a worker) and hands it back
//! (`provide_level`); the client checks it is the host's, and plays from
//! the newest snapshot it rebuilt, telling the host it has loaded.
//!
//! **Prediction (D10).** On each new authority snapshot `A` the world is
//! restored from `A` and stepped again, with `StepReason::Predict`, to the
//! predicted tick `P`: this seat with its own commands, every other seat
//! with the command the host last used for it. Before that, this seat's
//! avatar as predicted for `A` is compared with the host's, byte for byte:
//! equal whenever the host used this client's commands and nothing only
//! the host runs touched it; a mismatch is counted with its tick and cause.
//! Each frame then steps the ticks the predicted clock passed, making one
//! command per tick, and sends every command the host has not acked (at
//! most `MAX_COMMANDS`). Prediction runs at most `HORIZON` ticks past `A`.
//!
//! **Presentation.** `predicted` holds the last two predicted snapshots
//! and the predicted clock's blend (this seat's avatar, the camera, the
//! module's objects); `remote` the two authority snapshots around remote
//! time (everyone else). A rewind that moves the avatar as drawn leaves a
//! correction the view adds to it and lets decay (half-life 50 ms); over
//! 64 units it snaps.
//!
//! **Installs (D13).** `Installing` brings the snapshot the host stopped
//! at and the request: the client stops, and the caller plans the module
//! (`ClientEvent::Installing`, then `provide_level`). With the host's
//! announcement, it checks the plan, installs over the stopped snapshot
//! and compares the result with the announced one: equal, the result is
//! the new epoch's first baseline, computed, not sent; different, it says
//! so and gets a keyframe. It plays again at `Resume`.
//!
//! **Rejoining.** A session that times out with a seat token, when
//! `ClientConfig::rejoin_us` allows, joins again (`ClientEvent::Rejoining`):
//! handshakes naming the token, a new one each time one gives up (or,
//! after a pause, when the host still hears the seat's old connection:
//! `Refusal::InUse`), until one is accepted or `rejoin_us` has passed.
//! The host gives the token's seat back, the token with it (so an attempt
//! whose accept was lost leaves the next one the same token); from the
//! welcome it is a join like the first.

use std::collections::VecDeque;

use sim::{
    Command, EntityState, Kind, Level, Presented, Seat, SimId, SimWorld, Snapshot, SnapshotHash,
    StepReason, TickInput, TickRate, step,
};

use super::clock::{Clocks, Margins, tick_at, time_us};
use super::msg::{
    ClockReport, Commands, CopyAssembly, Fragment, InstallRequest, LevelRequest, MAX_COMMANDS,
    Message, Payload, Welcome,
};
use crate::codec::{CLIENT_RING, Check, ClientRing, CodecError, FrameHeader, Rebuilt};
use crate::handshake::{self, Credentials, Refusal};
use crate::link::{self, Link};
use crate::wire::{self, Class, Header, WireError, frame};
use crate::{NetTime, Peer};

/// The most ticks prediction runs past the newest authority snapshot.
pub const HORIZON: u64 = 50;
/// The most ticks one frame steps.
pub const MAX_CATCH_UP: u64 = sim::MAX_CATCH_UP as u64;
/// Snapshots in reassembly at once.
const REASSEMBLY: usize = 4;
/// Packets remembered for their newest command (acks come back within
/// far fewer).
const SENT: usize = 256;
/// Seat commands kept, by tick.
const SEAT_COMMANDS: usize = 128;
/// Own commands and predictions kept, by tick.
const OWN: usize = 128;
/// Snapshots an in-process client keeps (`Client::in_process`): its
/// host's frames come at once and in order, each against what it
/// confirmed a frame or two before, so a few frames' worth do, not a
/// network's `CLIENT_RING` (each holds a decoded snapshot).
const IN_PROCESS_RING: usize = 16;
/// Mismatches logged in full (all are counted).
const LOGGED: usize = 64;
/// The correction's half-life, µs, and the distance it snaps beyond
/// (units, as the correction is kept).
const HALF_LIFE_US: f32 = 50_000.0;
const SNAP_UNITS: f32 = 64.0;
/// Positions in snapshots are metres; the correction is in units.
const UNITS_PER_METRE: f32 = 1.0 / sim::motion::UNIT;

/// How a client runs.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClientConfig {
    pub link: link::Config,
    /// Keeps each rebuilt snapshot's epoch, tick and hash for
    /// `take_audit` (evidence).
    pub audit: bool,
    /// Keeps every authority snapshot rebuilt and every predicted one
    /// stepped (re-steps too) for `take_snapshots`: views that follow each
    /// tick.
    pub keep: bool,
    pub margins: Margins,
    /// How long after a timeout the client tries to join again with its
    /// seat's token (0: a timeout ends the session).
    pub rejoin_us: u64,
    /// The host's join password, proved in each handshake.
    pub key: Option<handshake::JoinKey>,
    /// A random secret from the caller that each rejoin's nonce is made
    /// under (keyed BLAKE3 over the last), so the nonces that travel do
    /// not tell the next one. All zeros (the default) in the probes.
    pub nonce_key: [u8; 32],
}

/// Snapshots since the last `take_snapshots`: the host's, rebuilt, and
/// this side's predictions, each in the order made (a rewind's re-steps
/// repeat ticks: the later one replaces the earlier).
#[derive(Debug, Clone, Default)]
pub struct Kept {
    pub rebuilt: Vec<Snapshot>,
    pub predicted: Vec<Snapshot>,
}

/// What happened, for the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientEvent {
    /// The host accepted the connection.
    Accepted {
        session: u64,
    },
    Welcome(Welcome),
    /// Build this level, then `provide_level`.
    NeedLevel(LevelRequest),
    /// The host is installing a module: plan it, then `provide_level`.
    Installing(InstallRequest),
    /// The install is committed here; `agreed` if its result is the
    /// host's (else a keyframe follows).
    Installed {
        epoch: u16,
        agreed: bool,
    },
    /// Playing in `epoch` from here on.
    Loaded {
        epoch: u16,
    },
    /// The world steps again after `tick`.
    Resumed {
        tick: u64,
    },
    /// The host holds its world after `tick` (a movie, a break): prediction
    /// stops until `Resumed`.
    Held {
        tick: Option<u64>,
    },
    /// Prediction (re)started: the clocks run from their targets, and
    /// the first command is for the tick after `tick`.
    Playing {
        tick: u64,
    },
    /// The session timed out: joining again with the seat's token, until
    /// `until` (`ClientConfig::rejoin_us`). A welcome follows, or `Ended`.
    Rejoining {
        until: NetTime,
    },
    /// The session ended (refused, timed out, left, or the host quit).
    Ended(Refusal),
    /// What became of this player's action, or what a script said to it.
    Notice(String),
    /// A screen a script opened for this player.
    Screen(sim::notice::Screen),
    /// A sound a script played, for this player to hear.
    Sound(sim::notice::Sound),
    /// A save the host made, whole (I30 P17): the bytes it sent, opaque
    /// here; the game keeps them among this player's saves.
    SaveCopy(Vec<u8>),
}

/// Why this seat's predicted avatar differed from the host's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Cause {
    /// The host stepped a tick without this seat's own command (late or
    /// lost; it substituted).
    OwnMissed,
    /// The host's command for this seat differs from the one made here.
    OwnDiffers,
    /// Another seat did something other than repeat its last command.
    OtherSeat,
    /// Something only the host runs (creature minds, damage, spawns,
    /// transitions).
    Authority,
}

impl Cause {
    pub const ALL: [Cause; 4] = [
        Cause::OwnMissed,
        Cause::OwnDiffers,
        Cause::OtherSeat,
        Cause::Authority,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Cause::OwnMissed => "own command missed",
            Cause::OwnDiffers => "own command differs",
            Cause::OtherSeat => "another seat",
            Cause::Authority => "authority only",
        }
    }
}

/// A predicted-versus-authority difference of this seat's avatar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mismatch {
    pub tick: u64,
    pub cause: Cause,
}

/// A client's counts and measures.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClientStats {
    /// Authority snapshots rebuilt and verified, and keyframes among them.
    pub rebuilt: u64,
    pub keyframes: u64,
    /// Frames rebuilt to another hash (a codec bug: must be 0).
    pub check_failures: u64,
    /// Frames whose baseline was not held, of another epoch, older than
    /// everything held, or malformed.
    pub no_baseline: u64,
    pub other_epoch: u64,
    pub stale: u64,
    pub malformed: u64,
    /// Ticks whose predicted bolts differ from the host's: creatures' (their
    /// minds and shots are the host's alone, so prediction lacks bolts they
    /// fired since the snapshot it started from), and players'.
    pub creature_bolt_ticks: u64,
    /// Creature bolts the host had and prediction did not, summed over
    /// those ticks.
    pub creature_bolts_unpredicted: u64,
    pub player_bolt_ticks: u64,
    /// Predicted avatars compared with the host's, and those that differed.
    pub compared: u64,
    pub mismatches: u64,
    pub by_cause: [u64; 4],
    pub mismatch_log: Vec<Mismatch>,
    /// Rewinds, ticks stepped again in them, the deepest.
    pub rewinds: u64,
    pub rewind_ticks: u64,
    pub rewind_max: u64,
    /// Rewinds that moved the drawn avatar, and those that snapped.
    pub corrections: u64,
    pub snapped: u64,
    /// Remote time passed the newest snapshot (episodes), and the frames
    /// that showed the newest held instead of a blend.
    pub starvations: u64,
    pub starved_frames: u64,
    /// Commands made; ticks skipped by a clock jump; frames held at the
    /// horizon.
    pub commands: u64,
    pub skipped: u64,
    pub held: u64,
    /// What the host reported of this seat's commands.
    pub host_late: u64,
    pub host_substituted: u64,
    /// Installs whose result was the host's, and those that differed.
    pub installs_agreed: u64,
    pub installs_differed: u64,
}

/// Where the clocks stand, in ticks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClockView {
    /// The predicted tick ahead of the host's estimated tick.
    pub lead: f64,
    /// The predicted clock's distance from its target.
    pub lead_error: f64,
    /// Remote time behind the host's estimated tick: the view lag (a
    /// one-way trip and the remote delay `D`).
    pub delay: f64,
    pub delay_error: f64,
    /// p95 jitter, µs.
    pub jitter_us: i64,
    /// What the lead and the remote delay keep over the trip, µs: the
    /// target slack and `D`; the loss margin in frames, and the frame it
    /// counts (the longest recent).
    pub slack_us: i64,
    pub d_us: i64,
    pub loss_frames: i64,
    pub frame_us: i64,
}

/// What a client holds, to show memory stays bounded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Memory {
    pub ring: usize,
    pub seat_commands: usize,
    pub reassembly: usize,
    pub own: usize,
    pub history: usize,
    /// Bytes the link holds: the host's messages being reassembled, ours
    /// in flight.
    pub held_bytes: usize,
    pub window_bytes: usize,
}

/// What a frame shows.
pub struct Frame<'a> {
    /// The last two predicted snapshots and the blend between them.
    pub predicted: &'a Presented,
    /// The two authority snapshots around remote time.
    pub remote: &'a Presented,
    /// Added to this seat's avatar and the camera as drawn (units).
    pub correction: [f32; 3],
    /// Ticks predicted this frame.
    pub stepped: u64,
}

/// A snapshot payload in fragments.
struct Reassembly {
    tick: u64,
    count: usize,
    parts: Vec<Option<Vec<u8>>>,
}

/// An install in progress here.
struct Install {
    /// The snapshot the host stopped at, rebuilt.
    before: Option<Snapshot>,
    announced: Option<(u16, sim::Announcement)>,
    level: Option<Level>,
}

/// Bolts in flight by who fired them: creatures (only the host's minds
/// fire, so prediction has none of theirs but those it restored), and
/// players.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Bolts {
    creatures: u32,
    players: u32,
}

impl Bolts {
    fn of(snapshot: &Snapshot) -> Bolts {
        let mut bolts = Bolts::default();
        for entity in &snapshot.entities {
            let Some(missile) = entity.missile else {
                continue;
            };
            match snapshot.get(missile.owner).map(|owner| owner.kind) {
                Some(Kind::Avatar(_)) => bolts.players += 1,
                _ => bolts.creatures += 1,
            }
        }
        bolts
    }
}

/// Prediction and presentation, once loaded.
struct Play {
    world: SimWorld,
    /// The last tick commands were made for (and the world's tick).
    made: u64,
    /// The authority tick the world was last restored from.
    authority: u64,
    /// The first tick this seat made a command for since it started: the
    /// host stepped those before without one, by design.
    first: u64,
    /// The clocks started since the last join or resume.
    started: bool,
    own_id: Option<SimId>,
    /// This seat's commands by tick, each with its view lag (ms).
    own: VecDeque<(u64, Command, u16)>,
    /// The predicted clock's offset when each command was made, by tick
    /// modulo 64 (for the host's slack reports).
    offsets: [(u64, i64); 64],
    /// This seat's avatar as predicted, and the bolts in flight
    /// (creatures', players'), by tick.
    history: VecDeque<(u64, EntityState, Bolts)>,
    /// Other seats' commands as last used by the host.
    others: TickInput,
    predicted: Presented,
    remote: Presented,
    remote_pair: (u64, u64),
    starving: bool,
    correction: [f32; 3],
}

impl Play {
    fn new(world: SimWorld, snapshot: &Snapshot, seat: Seat) -> Play {
        Play {
            world,
            made: snapshot.tick,
            authority: snapshot.tick,
            first: u64::MAX,
            started: false,
            own_id: own_id(snapshot, seat),
            own: VecDeque::new(),
            offsets: [(u64::MAX, 0); 64],
            history: VecDeque::new(),
            others: TickInput::default(),
            predicted: Presented::new(snapshot.clone()),
            remote: Presented::new(snapshot.clone()),
            remote_pair: (snapshot.tick, snapshot.tick),
            starving: false,
            correction: [0.0; 3],
        }
    }

    fn own_entry(&self, tick: u64) -> Option<(Command, u16)> {
        let at = self.own.binary_search_by_key(&tick, |(at, ..)| *at).ok()?;
        let (_, command, lag) = self.own[at];
        Some((command, lag))
    }

    fn own_command(&self, tick: u64) -> Option<Command> {
        self.own_entry(tick).map(|(command, _)| command)
    }

    /// A tick's input as predicted: this seat's command with the view lag
    /// it was made with (the host rewinds its shots by it), and the others'
    /// last.
    fn input(&self, seat: Seat, tick: u64) -> TickInput {
        let mut input = self.others.clone();
        if let Some((command, lag)) = self.own_entry(tick) {
            input.insert(seat, command);
            input.set_view_lag(seat, lag);
        }
        input
    }

    /// The host holds its world at `held`: what this player sees until it
    /// goes on. What was predicted past it is forgotten, its commands too
    /// (the host dropped them, as for an install); prediction starts again
    /// from it at the resume.
    fn hold(&mut self, held: Snapshot) {
        let tick = held.tick;
        self.own.retain(|(at, ..)| *at <= tick);
        self.history.retain(|(at, ..)| *at <= tick);
        self.made = self.made.min(tick);
        self.remote_pair = (tick, tick);
        self.remote = Presented::new(held.clone());
        self.predicted = Presented::new(held);
    }

    /// Where this seat's avatar is drawn (metres).
    fn shown(&self) -> Option<[f32; 3]> {
        let id = self.own_id?;
        self.predicted
            .entities()
            .find(|entity| entity.id == id)
            .map(|entity| entity.position)
    }
}

/// The connected session.
struct Session {
    link: Link,
    rate: TickRate,
    seat: Seat,
    epoch: u16,
    ring: ClientRing,
    /// Seat commands of rebuilt snapshots, by tick.
    seat_commands: VecDeque<(u64, TickInput)>,
    reassembly: VecDeque<Reassembly>,
    /// The level this side's authority copy is in.
    level: Option<Level>,
    /// The level asked for, not yet provided.
    wanted: Option<LevelRequest>,
    install: Option<Install>,
    play: Option<Play>,
    /// The world steps (not stopped for an install).
    running: bool,
    clocks: Clocks,
    /// A snapshot newer than prediction's came.
    fresh: bool,
    /// Packet numbers and the newest command each carried.
    sent: Vec<(u16, u64)>,
    pending_newest: Option<u64>,
    /// Every command up to this tick reached the host.
    acked_through: u64,
    /// Ticks the host reported missed.
    misses: VecDeque<u64>,
    resync_asked: bool,
    /// Frames in a row that named a baseline not held.
    unbased: u32,
    /// The newest snapshot a sent commands frame confirmed.
    confirmed_sent: Option<u64>,
    last_frame: Option<NetTime>,
    kept: Option<Kept>,
    stats: ClientStats,
    /// The host's save copy coming in (I30 P17). A reconnect makes a new
    /// session, so a copy cut by a drop is gone whole.
    copies: CopyAssembly,
}

enum State {
    Joining(Box<handshake::Client>),
    Connected(Box<Session>),
    /// What a session that ended had counted, if it got that far.
    Ended(Option<Box<Final>>),
}

/// An ended session's seat and counts.
struct Final {
    seat: Seat,
    stats: ClientStats,
    link: link::Stats,
}

/// The joining side (see the module docs).
pub struct Client {
    config: ClientConfig,
    server: Peer,
    /// What each handshake sends, and its nonce (the next attempt's is
    /// made from it).
    credentials: Credentials,
    nonce: u64,
    /// The seat's token: the last welcome's, or the one this client was
    /// made with.
    token: Option<u64>,
    /// While joining again: until when.
    rejoin: Option<NetTime>,
    state: State,
    events: VecDeque<ClientEvent>,
    audit: Vec<(u16, u64, SnapshotHash)>,
    /// The last packets of a session this side left.
    goodbye: VecDeque<Vec<u8>>,
    /// The server runs in this process (`in_process`).
    in_process: bool,
}

impl Client {
    /// Starts joining `server` at `now`; `nonce` is random (from the
    /// caller). Fails if the credentials cannot be sent.
    pub fn connect(
        credentials: Credentials,
        server: Peer,
        nonce: u64,
        now: NetTime,
        config: ClientConfig,
    ) -> Result<Client, WireError> {
        Ok(Client {
            config,
            server,
            token: credentials.reconnect,
            state: State::Joining(Box::new(handshake::Client::new(
                now,
                server,
                nonce,
                credentials.clone(),
                config.key,
            )?)),
            credentials,
            nonce,
            rejoin: None,
            events: VecDeque::new(),
            audit: Vec::new(),
            goodbye: VecDeque::new(),
            in_process: false,
        })
    }

    /// The next thing that happened.
    pub fn poll_event(&mut self) -> Option<ClientEvent> {
        self.events.pop_front()
    }

    /// Rebuilt snapshots' epoch, tick and hash since the last call (with
    /// `ClientConfig::audit`).
    pub fn take_audit(&mut self) -> Vec<(u16, u64, SnapshotHash)> {
        std::mem::take(&mut self.audit)
    }

    fn session(&self) -> Option<&Session> {
        match &self.state {
            State::Connected(session) => Some(session),
            _ => None,
        }
    }

    fn finished(&self) -> Option<&Final> {
        match &self.state {
            State::Ended(last) => last.as_deref(),
            _ => None,
        }
    }

    /// The seat, once welcomed.
    pub fn seat(&self) -> Option<Seat> {
        match self.finished() {
            Some(last) => Some(last.seat),
            None => self.session().map(|session| session.seat),
        }
    }

    /// Whether the session ended.
    pub fn ended(&self) -> bool {
        matches!(self.state, State::Ended(_))
    }

    /// The tick the next command will be for, while playing.
    pub fn next_tick(&self) -> Option<u64> {
        let play = self.session()?.play.as_ref()?;
        Some(play.made + 1)
    }

    /// The link's measures (at the end, once ended).
    pub fn link_stats(&self) -> Option<link::Stats> {
        match self.finished() {
            Some(last) => Some(last.link),
            None => self.session().map(|session| session.link.stats()),
        }
    }

    /// The counts so far (at the end, once ended).
    pub fn stats(&self) -> ClientStats {
        match (self.finished(), self.session()) {
            (Some(last), _) => last.stats.clone(),
            (None, Some(session)) => session.stats.clone(),
            (None, None) => ClientStats::default(),
        }
    }

    /// Where the clocks stand, once they run.
    pub fn clock(&self) -> Option<ClockView> {
        let session = self.session()?;
        let play = session.play.as_ref()?;
        if !play.started || !session.running {
            return None;
        }
        let clocks = &session.clocks;
        let tick = clocks.tick_us() as f64;
        let host = clocks.host_offset()?;
        Some(ClockView {
            lead: (clocks.predicted - host) as f64 / tick,
            lead_error: clocks.predicted_error()? as f64 / tick,
            delay: (host - clocks.remote) as f64 / tick,
            delay_error: clocks.remote_error()? as f64 / tick,
            jitter_us: clocks.delays.jitter(),
            slack_us: clocks.target_slack(),
            d_us: clocks.target_delay(),
            loss_frames: clocks.margin,
            frame_us: clocks.frame_us,
        })
    }

    /// What it holds.
    pub fn memory(&self) -> Memory {
        let Some(session) = self.session() else {
            return Memory::default();
        };
        let play = session.play.as_ref();
        Memory {
            ring: session.ring.len(),
            seat_commands: session.seat_commands.len(),
            reassembly: session.reassembly.len(),
            own: play.map_or(0, |play| play.own.len()),
            history: play.map_or(0, |play| play.history.len()),
            held_bytes: session.link.held_bytes(),
            window_bytes: session.link.window_bytes(),
        }
    }

    /// The newest predicted snapshot, while playing.
    pub fn predicted(&self) -> Option<&Snapshot> {
        let play = self.session()?.play.as_ref()?;
        Some(play.predicted.current())
    }

    /// The newest authority snapshot rebuilt.
    pub fn authority(&self) -> Option<&Snapshot> {
        self.session()?
            .ring
            .newest()
            .map(|rebuilt| &rebuilt.snapshot)
    }

    fn end(&mut self, why: Refusal) {
        let last = match &self.state {
            State::Ended(_) => return,
            State::Joining(_) => None,
            State::Connected(session) => Some(Box::new(Final {
                seat: session.seat,
                stats: session.stats.clone(),
                link: session.link.stats(),
            })),
        };
        self.state = State::Ended(last);
        self.rejoin = None;
        self.events.push_back(ClientEvent::Ended(why));
    }

    /// The session timed out, or a rejoin's handshake ended in a way worth
    /// trying again: the next handshake with the seat's token while
    /// `rejoin_us` allows (the first one raises `ClientEvent::Rejoining`),
    /// else the end. `pause` holds its first request `RETRY_US`.
    fn rejoin_attempt(&mut self, now: NetTime, pause: bool) {
        let until = match self.rejoin {
            Some(until) => until,
            None if self.config.rejoin_us > 0 && self.token.is_some() => {
                now.plus(self.config.rejoin_us)
            }
            None => return self.end(Refusal::Timeout),
        };
        if now >= until {
            return self.end(Refusal::Timeout);
        }
        self.nonce = next_nonce(&self.config.nonce_key, self.nonce);
        let credentials = Credentials {
            reconnect: self.token,
            ..self.credentials.clone()
        };
        match handshake::Client::new(now, self.server, self.nonce, credentials, self.config.key) {
            Ok(mut handshake) => {
                if pause {
                    handshake.pause(now);
                }
                self.state = State::Joining(Box::new(handshake));
                if self.rejoin.is_none() {
                    self.events.push_back(ClientEvent::Rejoining { until });
                }
                self.rejoin = Some(until);
            }
            Err(_) => self.end(Refusal::Timeout),
        }
    }

    /// A handshake's refusal while joining again: one that gave up tries
    /// once more at once; one the host refused because the seat's old
    /// connection is still heard (`Refusal::InUse`: it may not have gone
    /// quiet yet) tries again after a pause. Any other ends the session.
    fn refused(&mut self, now: NetTime, refusal: Refusal) {
        match refusal {
            Refusal::Timeout if self.rejoin.is_some() => self.rejoin_attempt(now, false),
            Refusal::InUse if self.rejoin.is_some() => self.rejoin_attempt(now, true),
            refusal => self.end(refusal),
        }
    }

    // -----------------------------------------------------------------
    // Receiving.

    /// Takes a datagram from `from` that arrived at `now`.
    pub fn receive(&mut self, now: NetTime, from: Peer, bytes: &[u8]) {
        if from != self.server {
            return;
        }
        match &mut self.state {
            State::Ended(_) => {}
            State::Joining(handshake) => {
                if wire::classify(bytes) != Class::Handshake {
                    return;
                }
                handshake.receive(now, from, bytes);
                let rtt = handshake.rtt();
                match handshake.outcome().cloned() {
                    Some(Ok(session)) => self.accepted(now, session, rtt),
                    Some(Err(refusal)) => self.refused(now, refusal),
                    None => {}
                }
            }
            State::Connected(_) => self.take_packet(now, bytes),
        }
    }

    /// The server runs in this process (the host's own player): the link
    /// to it never times out (`Link::set_in_process`), now or once joined,
    /// and a session joined after keeps fewer snapshots (`IN_PROCESS_RING`).
    pub fn in_process(&mut self) {
        self.in_process = true;
        if let State::Connected(session) = &mut self.state {
            session.link.set_in_process();
        }
    }

    fn ring_size(&self) -> usize {
        if self.in_process {
            IN_PROCESS_RING
        } else {
            CLIENT_RING
        }
    }

    fn accepted(&mut self, now: NetTime, session: u64, rtt: Option<u64>) {
        let rate = TickRate::default();
        let mut clocks = Clocks::new(rate, 2, self.config.margins);
        let rtt = rtt.map_or(0, |rtt| rtt as i64);
        clocks.rtt_us = rtt;
        clocks.rtt_min_us = rtt;
        let mut link = Link::with_config(session, now, self.config.link);
        if self.in_process {
            link.set_in_process();
        }
        self.rejoin = None;
        self.state = State::Connected(Box::new(Session {
            link,
            rate,
            seat: Seat(0),
            epoch: 0,
            ring: ClientRing::new(0, self.ring_size()),
            seat_commands: VecDeque::new(),
            reassembly: VecDeque::new(),
            level: None,
            wanted: None,
            install: None,
            play: None,
            running: true,
            clocks,
            fresh: false,
            sent: vec![(u16::MAX, 0); SENT],
            pending_newest: None,
            acked_through: 0,
            misses: VecDeque::new(),
            resync_asked: false,
            unbased: 0,
            confirmed_sent: None,
            last_frame: None,
            kept: self.config.keep.then(Kept::default),
            stats: ClientStats::default(),
            copies: CopyAssembly::default(),
        }));
        self.events.push_back(ClientEvent::Accepted { session });
    }

    fn take_packet(&mut self, now: NetTime, bytes: &[u8]) {
        let State::Connected(session) = &mut self.state else {
            return;
        };
        let Ok(received) = session.link.receive(now, bytes) else {
            return;
        };
        let frames: Vec<wire::Frame<'_>> = received.frames().collect();
        let messages: Vec<Vec<u8>> = session.link.take_messages().collect();
        for seq in session.link.take_acked() {
            let (sent, newest) = session.sent[usize::from(seq) % SENT];
            if sent == seq {
                session.acked_through = session.acked_through.max(newest);
            }
        }
        for frame in frames {
            match frame.kind {
                frame::SNAPSHOT => self.take_fragment(now, frame.payload),
                frame::CLOCK => self.take_report(now, frame.payload),
                frame::DISCONNECT => {
                    let why = Refusal::decode(&mut wire::Reader::new(frame.payload))
                        .unwrap_or(Refusal::HostQuit);
                    self.end(why);
                    return;
                }
                _ => {}
            }
        }
        for message in messages {
            match Message::decode(&message) {
                Ok(message) => self.take_message(now, message),
                Err(_) => {
                    self.end(Refusal::Kicked("the host sent a malformed message".into()));
                    return;
                }
            }
        }
    }

    fn take_fragment(&mut self, now: NetTime, payload: &[u8]) {
        let State::Connected(session) = &mut self.state else {
            return;
        };
        let Ok(fragment) = Fragment::decode(payload) else {
            session.stats.malformed += 1;
            return;
        };
        if fragment.count == 1 {
            self.take_payload(now, fragment.bytes, false);
            return;
        }
        if session
            .ring
            .newest()
            .is_some_and(|newest| newest.tick >= fragment.tick)
        {
            return;
        }
        let at = match session
            .reassembly
            .iter()
            .position(|held| held.tick == fragment.tick)
        {
            Some(at) if session.reassembly[at].count == fragment.count => at,
            Some(_) => {
                session.stats.malformed += 1;
                return;
            }
            None => {
                if session.reassembly.len() == REASSEMBLY {
                    session.reassembly.pop_front();
                }
                session.reassembly.push_back(Reassembly {
                    tick: fragment.tick,
                    count: fragment.count,
                    parts: vec![None; fragment.count],
                });
                session.reassembly.len() - 1
            }
        };
        let held = &mut session.reassembly[at];
        if held.parts[fragment.index].is_none() {
            held.parts[fragment.index] = Some(fragment.bytes.to_vec());
        }
        if held.parts.iter().all(Option::is_some) {
            let Some(held) = session.reassembly.remove(at) else {
                return;
            };
            let whole: Vec<u8> = held.parts.into_iter().flatten().flatten().collect();
            self.take_payload(now, &whole, false);
        }
    }

    /// Rebuilds a snapshot payload; the tick rebuilt (or already held).
    fn take_payload(&mut self, now: NetTime, bytes: &[u8], reliable: bool) -> Option<u64> {
        let State::Connected(session) = &mut self.state else {
            return None;
        };
        let stats = &mut session.stats;
        let (payload, header) = match Payload::read(bytes)
            .ok()
            .and_then(|payload| Some((payload, FrameHeader::read(payload.frame).ok()?.0)))
        {
            Some(read) => read,
            None => {
                stats.malformed += 1;
                return None;
            }
        };
        if header.epoch != session.ring.epoch() {
            stats.other_epoch += 1;
            return None;
        }
        if session.ring.get(header.tick).is_some() {
            return Some(header.tick);
        }
        let empty = TickInput::default();
        let base = match header.baseline {
            None => &empty,
            Some(tick) => match seat_commands_at(&session.seat_commands, tick) {
                Some(base) => base,
                None => {
                    stats.no_baseline += 1;
                    lost_baseline(session);
                    return None;
                }
            },
        };
        let Ok(commands) = payload.read_seats(base) else {
            stats.malformed += 1;
            return None;
        };
        let newest = session.ring.newest().map(|newest| newest.tick);
        match session.ring.receive(payload.frame) {
            Ok(rebuilt) => {
                stats.rebuilt += 1;
                stats.keyframes += u64::from(header.baseline.is_none());
                if let Some(kept) = &mut session.kept {
                    kept.rebuilt.push(rebuilt.snapshot.clone());
                }
                if self.config.audit {
                    self.audit
                        .push((header.epoch, rebuilt.tick, rebuilt.snapshot.hash()));
                }
            }
            Err(error) => {
                match error {
                    CodecError::Check { .. } | CodecError::Snapshot(_) => {
                        stats.check_failures += 1;
                        if !session.resync_asked {
                            session.resync_asked = true;
                            let _ = session.link.send_reliable(&Message::Resync.to_bytes());
                        }
                    }
                    CodecError::NoBaseline { .. } => {
                        stats.no_baseline += 1;
                        lost_baseline(session);
                    }
                    CodecError::Epoch { .. } => stats.other_epoch += 1,
                    CodecError::Stale { .. } => stats.stale += 1,
                    _ => stats.malformed += 1,
                }
                return None;
            }
        }
        session.unbased = 0;
        if header.baseline.is_none() {
            session.resync_asked = false;
        }
        let tick = header.tick;
        let at = session
            .seat_commands
            .partition_point(|(held, _)| *held < tick);
        session.seat_commands.insert(at, (tick, commands));
        while session.seat_commands.len() > SEAT_COMMANDS {
            session.seat_commands.pop_front();
        }
        if !reliable {
            session.clocks.sample(now, tick, payload.since_us);
            session.clocks.every = payload.every.max(1);
        }
        if newest.is_none_or(|newest| tick > newest) {
            session.fresh = true;
        }
        Some(tick)
    }

    fn take_report(&mut self, now: NetTime, payload: &[u8]) {
        let State::Connected(session) = &mut self.state else {
            return;
        };
        let Ok(report) = ClockReport::decode(payload) else {
            session.stats.malformed += 1;
            return;
        };
        session.stats.host_late += u64::from(report.late);
        session.stats.host_substituted += u64::from(report.substituted);
        if let Some(miss) = report.miss {
            // Only a command this side made: a tick it skipped after a long
            // frame (or a clock jump) had none to lose.
            if session.play.as_ref().is_some_and(|play| {
                play.started && miss >= play.first && play.offsets[(miss % 64) as usize].0 == miss
            }) {
                session.clocks.missed(now);
            }
            session.misses.push_back(miss);
            while session.misses.len() > 64 {
                session.misses.pop_front();
            }
        }
        // Only commands made since the clocks last started: an older one's
        // offset is on the timeline before an install.
        if let (Some((tick, slack)), Some(play)) = (report.least, &session.play)
            && play.started
            && tick >= play.first
        {
            let (made, offset) = play.offsets[(tick % 64) as usize];
            if made == tick {
                session.clocks.report(offset, slack);
            }
        }
    }

    fn take_message(&mut self, now: NetTime, message: Message<'_>) {
        let State::Connected(session) = &mut self.state else {
            return;
        };
        match message {
            Message::Welcome(welcome) => {
                session.seat = welcome.seat;
                self.token = Some(welcome.token);
                session.rate = TickRate::new(welcome.hz).unwrap_or(TickRate::default());
                session.epoch = welcome.epoch;
                session.ring = ClientRing::new(welcome.epoch, session.ring.capacity());
                let (rtt, rtt_min) = (session.clocks.rtt_us, session.clocks.rtt_min_us);
                session.clocks = Clocks::new(session.rate, welcome.every, self.config.margins);
                session.clocks.rtt_us = rtt;
                session.clocks.rtt_min_us = rtt_min;
                self.events.push_back(ClientEvent::Welcome(welcome));
            }
            Message::Level(request) => {
                session.wanted = Some(request.clone());
                session.play = None;
                self.events.push_back(ClientEvent::NeedLevel(request));
            }
            Message::Snapshot(payload) => {
                if self.take_payload(now, payload, true).is_some() {
                    self.try_start(now);
                }
            }
            Message::Installing { request, payload } => {
                let tick = self.take_payload(now, payload, true);
                let State::Connected(session) = &mut self.state else {
                    return;
                };
                let before = tick
                    .and_then(|tick| session.ring.get(tick))
                    .map(|rebuilt| rebuilt.snapshot.clone());
                session.running = false;
                session.install = Some(Install {
                    before,
                    announced: None,
                    level: None,
                });
                self.events.push_back(ClientEvent::Installing(request));
            }
            Message::Hold { payload } => {
                let tick = self.take_payload(now, payload, true);
                let State::Connected(session) = &mut self.state else {
                    return;
                };
                session.running = false;
                let held = tick
                    .and_then(|tick| session.ring.get(tick))
                    .map(|rebuilt| rebuilt.snapshot.clone());
                if let (Some(held), Some(play)) = (held, session.play.as_mut()) {
                    play.hold(held);
                }
                self.events.push_back(ClientEvent::Held { tick });
            }
            Message::Installed {
                epoch,
                announcement,
            } => {
                match &mut session.install {
                    Some(install) => install.announced = Some((epoch, announcement)),
                    // Joined while the host was installing: nothing to
                    // commit; the level and a keyframe follow the resume.
                    None => {
                        session.epoch = epoch;
                        session.ring.begin_epoch(epoch, None);
                        session.play = None;
                    }
                }
                self.try_commit(now);
            }
            Message::Resume { tick } => {
                // Resumed with no announcement: the host abandoned it.
                if session
                    .install
                    .as_ref()
                    .is_some_and(|install| install.announced.is_none())
                {
                    session.install = None;
                }
                session.running = true;
                session.clocks.restart();
                if let Some(play) = &mut session.play {
                    play.started = false;
                }
                self.events.push_back(ClientEvent::Resumed { tick });
            }

            Message::Notice(text) => self.events.push_back(ClientEvent::Notice(text)),
            Message::Screen(screen) => self.events.push_back(ClientEvent::Screen(screen)),
            Message::Sound(sound) => self.events.push_back(ClientEvent::Sound(sound)),
            Message::SaveCopy(part) => {
                if let Some(copy) = session.copies.take(&part) {
                    self.events.push_back(ClientEvent::SaveCopy(copy));
                }
            }
            Message::Loaded { .. } | Message::Resync | Message::Action(_) => {}
        }
    }

    // -----------------------------------------------------------------
    // Levels, joining and installing.

    /// Hands over the level asked for (`NeedLevel`, `Installing`), or why
    /// it could not be built (the session then ends: `Refusal::Plan`).
    pub fn provide_level(&mut self, now: NetTime, level: Result<Level, String>) {
        let State::Connected(session) = &mut self.state else {
            return;
        };
        let level = match level {
            Ok(level) => level,
            Err(why) => {
                self.leave(now, Refusal::Plan(why));
                return;
            }
        };
        if let Some(install) = session
            .install
            .as_mut()
            .filter(|install| install.level.is_none())
        {
            install.level = Some(level);
            self.try_commit(now);
            return;
        }
        let Some(request) = session.wanted.take() else {
            return;
        };
        let checked = match &request {
            LevelRequest::Stage(id) => id.check(&level).map_err(|error| error.to_string()),
            LevelRequest::Module(announcement) => announcement
                .check(&level)
                .map_err(|mismatch| mismatch.to_string()),
        };
        if let Err(why) = checked {
            self.leave(now, Refusal::Level(why));
            return;
        }
        session.level = Some(level);
        self.try_start(now);
    }

    /// Ends the session from this side, telling the host why (in
    /// `DISCONNECTS` packets `poll_transmit` gives next).
    pub fn leave(&mut self, now: NetTime, why: Refusal) {
        if let State::Connected(session) = &mut self.state {
            self.goodbye
                .extend(super::goodbye(&mut session.link, now, &why));
        }
        self.end(why);
    }

    /// The token that takes this seat back after a timeout
    /// (`Credentials::reconnect` of a new connection): the last welcome's,
    /// or the one this client was made with; kept when a rejoin fails.
    pub fn token(&self) -> Option<u64> {
        self.token
    }

    /// Plays from the newest snapshot once the level and one are here.
    fn try_start(&mut self, now: NetTime) {
        let State::Connected(session) = &mut self.state else {
            return;
        };
        if session.play.is_some() || session.install.is_some() || session.wanted.is_some() {
            return;
        }
        let (Some(level), Some(newest)) = (&session.level, session.ring.newest()) else {
            return;
        };
        let snapshot = newest.snapshot.clone();
        let world = match SimWorld::from_snapshot(&snapshot, level.clone()) {
            Ok(world) => world,
            Err(error) => {
                self.leave(now, Refusal::Level(error.to_string()));
                return;
            }
        };
        let check = Check::of(&snapshot.hash());
        let loaded = Message::Loaded {
            epoch: session.epoch,
            tick: snapshot.tick,
            check,
        };
        let _ = session.link.send_reliable(&loaded.to_bytes());
        session.play = Some(Play::new(world, &snapshot, session.seat));
        session.fresh = false;
        self.events.push_back(ClientEvent::Loaded {
            epoch: session.epoch,
        });
    }

    /// Commits an install here once the plan and the announcement are both
    /// in: see the module docs.
    fn try_commit(&mut self, now: NetTime) {
        let State::Connected(session) = &mut self.state else {
            return;
        };
        let Some(install) = &session.install else {
            return;
        };
        let (Some((epoch, announcement)), Some(level)) = (&install.announced, &install.level)
        else {
            return;
        };
        let (epoch, announcement, level) = (*epoch, announcement.clone(), level.clone());
        if let Err(mismatch) = announcement.check(&level) {
            self.leave(now, Refusal::Level(mismatch.to_string()));
            return;
        }
        let tick = announcement.tick;
        let committed = install
            .before
            .as_ref()
            .zip(session.level.clone())
            .and_then(|(before, old)| SimWorld::from_snapshot(before, old).ok())
            .and_then(|mut world| {
                let after = world.install(level.clone(), announcement.arrival()).ok()?;
                Some((world, after))
            })
            .filter(|(_, after)| announcement.check_after(after).is_ok());
        session.install = None;
        session.epoch = epoch;
        session.level = Some(level);
        session.reassembly.clear();
        let agreed = committed.is_some();
        let check = match committed {
            Some((world, after)) => {
                let check = Check::of(&after.hash());
                session.stats.installs_agreed += 1;
                session
                    .ring
                    .begin_epoch(epoch, Some(Rebuilt::of(after.clone())));
                let mut play = Play::new(world, &after, session.seat);
                if let Some(old) = &session.play {
                    play.correction = old.correction;
                }
                session.play = Some(play);
                session.fresh = false;
                check
            }
            None => {
                session.stats.installs_differed += 1;
                session.ring.begin_epoch(epoch, None);
                session.play = None;
                Check([0; 8])
            }
        };
        let loaded = Message::Loaded { epoch, tick, check };
        let _ = session.link.send_reliable(&loaded.to_bytes());
        self.events
            .push_back(ClientEvent::Installed { epoch, agreed });
        if agreed {
            self.events.push_back(ClientEvent::Loaded { epoch });
        }
    }

    /// Sends an action of this player's to the host (reliably, in order);
    /// nothing before the session is up.
    pub fn act(&mut self, action: sim::action::Action) {
        if let State::Connected(session) = &mut self.state {
            let _ = session
                .link
                .send_reliable(&Message::Action(action).to_bytes());
        }
    }

    // -----------------------------------------------------------------
    // Frames.

    /// One frame at `now` with the input sampled for it: the clocks move
    /// on, a new authority snapshot is predicted from, the ticks the
    /// predicted clock passed are stepped (each with `input`), and their
    /// commands queued. `None` until playing.
    pub fn frame(&mut self, now: NetTime, input: Command) -> Option<Frame<'_>> {
        let State::Connected(session) = &mut self.state else {
            return None;
        };
        let session = &mut **session;
        let elapsed = session.last_frame.map_or(0, |last| now.since(last) as i64);
        session.last_frame = Some(now);
        // A stall (a load, a debugger) is not a frame length.
        session.clocks.frame(elapsed.min(100_000));
        let link = session.link.stats();
        if link.rtt_samples > 0 {
            session.clocks.rtt_us = link.rtt_us as i64;
            session.clocks.rtt_min_us = link.rtt_min_us as i64;
        }
        let started = session.play.as_ref()?.started;
        let mut stepped = 0;
        if session.running {
            if !started && session.clocks.host_offset().is_some() {
                session.clocks.start();
                // The first command is for the tick after the one the
                // clock stands in: the host steps those before it.
                let standing = tick_at(session.rate, session.clocks.predicted_at(now));
                if let Some(play) = session.play.as_mut() {
                    play.started = true;
                    play.made = play.made.max(standing);
                    play.first = play.made + 1;
                    self.events
                        .push_back(ClientEvent::Playing { tick: play.made });
                }
                reconcile(session, true);
            } else if started {
                session.clocks.advance(now, elapsed);
                if session.fresh {
                    reconcile(session, false);
                }
            }
            stepped = predict(session, now, input);
            present(session, now, elapsed);
        }
        let play = session.play.as_ref()?;
        Some(Frame {
            predicted: &play.predicted,
            remote: &play.remote,
            correction: play.correction,
            stepped,
        })
    }

    /// One frame in lockstep with a host in this process (solo over
    /// `Loopback`): no clocks of its own; the caller steps the host to
    /// `tick` right after, so this makes `command` this seat's command for
    /// `tick` and queues it. Nothing is predicted: the host steps that tick
    /// within the same frame and its snapshot is what is drawn, so a
    /// prediction would be thrown away unseen (and restoring the world from
    /// every rebuilt snapshot to predict from it cost more than the host's
    /// step). The frame's `predicted` stays where it was. `None`
    /// until playing (and while an install stops the world).
    pub fn frame_to(&mut self, now: NetTime, tick: u64, command: Command) -> Option<Frame<'_>> {
        let State::Connected(session) = &mut self.state else {
            return None;
        };
        let session = &mut **session;
        session.last_frame = Some(now);
        if !session.running {
            return None;
        }
        let play = session.play.as_mut()?;
        if !play.started {
            play.started = true;
            play.first = play.made + 1;
            self.events
                .push_back(ClientEvent::Playing { tick: play.made });
        }
        session.fresh = false;
        // In lockstep the host's world is shown as it is: no view lag.
        let stepped = queue_lockstep(session, tick, command);
        let play = session.play.as_ref()?;
        Some(Frame {
            predicted: &play.predicted,
            remote: &play.remote,
            correction: play.correction,
            stepped,
        })
    }

    /// The snapshots kept since the last call (`ClientConfig::keep`).
    pub fn take_snapshots(&mut self) -> Kept {
        match &mut self.state {
            State::Connected(session) => session
                .kept
                .as_mut()
                .map_or_else(Kept::default, std::mem::take),
            _ => Kept::default(),
        }
    }

    /// Whether the world steps (no install is stopping it).
    pub fn running(&self) -> bool {
        self.session().is_some_and(|session| session.running)
    }

    // -----------------------------------------------------------------
    // Sending.

    /// The next datagram to send at `now`. Call until `None`.
    pub fn poll_transmit(&mut self, now: NetTime) -> Option<(Peer, Vec<u8>)> {
        if let Some(goodbye) = self.goodbye.pop_front() {
            return Some((self.server, goodbye));
        }
        match &mut self.state {
            State::Ended(_) => None,
            State::Joining(handshake) => {
                let bytes = handshake.poll_transmit(now);
                if let Some(Err(refusal)) = handshake.outcome().cloned() {
                    self.refused(now, refusal);
                    return self.poll_transmit(now);
                }
                bytes.map(|bytes| (self.server, bytes))
            }
            State::Connected(session) => {
                if session.link.broken().is_some() {
                    self.end(Refusal::Timeout);
                    return None;
                }
                if session.link.timed_out(now) {
                    self.rejoin_attempt(now, false);
                    return self.poll_transmit(now);
                }
                confirm_idle(session);
                let bytes = session.link.poll_transmit(now)?;
                if let Some(newest) = session.pending_newest.take()
                    && let Ok((header, _)) = Header::decode(&bytes)
                {
                    session.sent[usize::from(header.seq) % SENT] = (header.seq, newest);
                }
                Some((self.server, bytes))
            }
        }
    }

    /// When `poll_transmit` next has something to do.
    pub fn next_deadline(&self) -> Option<NetTime> {
        if !self.goodbye.is_empty() {
            return Some(NetTime(0));
        }
        match &self.state {
            State::Ended(_) => None,
            State::Joining(handshake) => handshake.next_deadline(),
            State::Connected(session) => session.link.next_deadline(),
        }
    }
}

/// The nonce of the next handshake after one with `nonce`, under `key`.
fn next_nonce(key: &[u8; 32], nonce: u64) -> u64 {
    let hash = blake3::Hasher::new_keyed(key)
        .update(b"kjk-net rejoin nonce")
        .update(&nonce.to_le_bytes())
        .finalize();
    let bytes: [u8; 8] = hash.as_bytes()[..8].try_into().unwrap_or([1; 8]);
    u64::from_le_bytes(bytes)
}

/// The seat commands kept for `tick`.
fn seat_commands_at(held: &VecDeque<(u64, TickInput)>, tick: u64) -> Option<&TickInput> {
    let at = held.binary_search_by_key(&tick, |(held, _)| *held).ok()?;
    held.get(at).map(|(_, commands)| commands)
}

/// This seat's body's id in a snapshot (`Snapshot::body`): its own
/// character, or the party member it drives; none while it watches.
fn own_id(snapshot: &Snapshot, seat: Seat) -> Option<SimId> {
    snapshot.body(seat).map(|entity| entity.id)
}

/// Restores the world from the newest authority snapshot and predicts
/// again to the predicted tick, first comparing this seat's avatar as
/// predicted for that tick with the host's.
fn reconcile(session: &mut Session, starting: bool) {
    session.fresh = false;
    let Some(play) = session.play.as_mut() else {
        return;
    };
    let Some(newest) = session.ring.newest() else {
        return;
    };
    let tick = newest.tick;
    if tick <= play.authority && !starting {
        return;
    }
    let commands = seat_commands_at(&session.seat_commands, tick);
    let mut others = commands.cloned().unwrap_or(TickInput::default());
    let used_here = others.get(session.seat).copied();
    let mut others_now = TickInput::default();
    for (seat, command) in others.iter() {
        if seat != session.seat {
            others_now.insert(seat, *command);
        }
    }
    others = others_now;

    // Exact-match accounting of this seat's avatar at `tick`.
    let predicted = play
        .history
        .binary_search_by_key(&tick, |(at, ..)| *at)
        .ok()
        .map(|at| (play.history[at].1, play.history[at].2));
    if let Some((_, bolts)) = predicted {
        let host = Bolts::of(&newest.snapshot);
        let stats = &mut session.stats;
        if host.creatures != bolts.creatures {
            stats.creature_bolt_ticks += 1;
            stats.creature_bolts_unpredicted +=
                u64::from(host.creatures.saturating_sub(bolts.creatures));
        }
        stats.player_bolt_ticks += u64::from(host.players != bolts.players);
    }
    let predicted = predicted.map(|(own, _)| own);
    if let (Some(predicted), Some(id)) = (predicted, play.own_id) {
        let host = newest
            .parts
            .entities
            .binary_search_by_key(&id, |(id, _)| *id)
            .ok()
            .map(|at| &newest.parts.entities[at].1);
        session.stats.compared += 1;
        if host.is_none_or(|host| *host != Snapshot::record(&predicted)) {
            let missed = session
                .misses
                .iter()
                .any(|&miss| miss > play.authority && miss <= tick);
            let cause = if missed {
                Cause::OwnMissed
            } else if used_here != play.own_command(tick) {
                Cause::OwnDiffers
            } else if others != play.others {
                Cause::OtherSeat
            } else {
                Cause::Authority
            };
            let stats = &mut session.stats;
            stats.mismatches += 1;
            stats.by_cause[cause as usize] += 1;
            if stats.mismatch_log.len() < LOGGED {
                stats.mismatch_log.push(Mismatch { tick, cause });
            }
        }
    }

    let shown = play.shown();
    let snapshot = newest.snapshot.clone();
    let Some(level) = session.level.clone() else {
        return;
    };
    let Ok(world) = SimWorld::from_snapshot(&snapshot, level) else {
        return;
    };
    play.world = world;
    // The seat took a body, or lost it (I30 P2–P5): what was predicted of
    // the old one compares with nothing, and the new one is drawn where it
    // is, uncorrected.
    let id = own_id(&snapshot, session.seat);
    let switched = id != play.own_id;
    if switched {
        play.history.clear();
        play.own_id = id;
        play.correction = [0.0; 3];
    }
    play.others = others;
    play.authority = tick;
    play.made = play.made.max(tick);
    // The same two ticks as before, blended the same: only a real
    // difference moves what is drawn.
    let alpha = play.predicted.alpha();
    let previous = play.predicted.previous();
    play.predicted = if play.made == tick && previous.tick + 1 == tick {
        let mut shown = Presented::new(previous.clone());
        shown.push(snapshot);
        shown
    } else {
        Presented::new(snapshot)
    };
    let depth = play.made - tick;
    for at in tick + 1..=play.made {
        let input = play.input(session.seat, at);
        let snapshot = step(
            &mut play.world,
            at,
            &input,
            session.rate.msec(at),
            StepReason::Predict,
        );
        remember(play, &snapshot);
        if let Some(kept) = &mut session.kept {
            kept.predicted.push(snapshot.clone());
        }
        play.predicted.push(snapshot);
    }
    play.predicted.set_alpha(alpha);
    while play.history.front().is_some_and(|(at, ..)| *at < tick) {
        play.history.pop_front();
    }
    let keep_from = tick.min(session.acked_through);
    while play.own.front().is_some_and(|(at, ..)| *at <= keep_from) {
        play.own.pop_front();
    }
    let stats = &mut session.stats;
    stats.rewinds += 1;
    stats.rewind_ticks += depth;
    stats.rewind_max = stats.rewind_max.max(depth);
    if let (Some(before), Some(after)) = (shown, play.shown())
        && !starting
        && !switched
    {
        let moved: [f32; 3] =
            std::array::from_fn(|axis| (before[axis] - after[axis]) * UNITS_PER_METRE);
        if moved != [0.0; 3] {
            stats.corrections += 1;
            let error: [f32; 3] = std::array::from_fn(|axis| play.correction[axis] + moved[axis]);
            let length = error.iter().map(|e| e * e).sum::<f32>().sqrt();
            if length > SNAP_UNITS {
                stats.snapped += 1;
                play.correction = [0.0; 3];
            } else {
                play.correction = error;
            }
        }
    }
}

/// Keeps this seat's avatar as predicted at the snapshot's tick.
fn remember(play: &mut Play, snapshot: &Snapshot) {
    let Some(id) = play.own_id else {
        return;
    };
    let Some(own) = snapshot.get(id) else {
        return;
    };
    let tick = snapshot.tick;
    while play.history.back().is_some_and(|(at, ..)| *at >= tick) {
        play.history.pop_back();
    }
    play.history.push_back((tick, *own, Bolts::of(snapshot)));
    while play.history.len() > OWN {
        play.history.pop_front();
    }
}

/// Steps the ticks the predicted clock passed, one command each, and
/// queues the commands the host has not acked. The ticks stepped.
fn predict(session: &mut Session, now: NetTime, input: Command) -> u64 {
    let Some(play) = session.play.as_mut() else {
        return 0;
    };
    if !play.started {
        return 0;
    }
    let rate = session.rate;
    // Never past the horizon: the clock holds there.
    let limit = time_us(rate, play.authority + HORIZON);
    if session.clocks.predicted_at(now) > limit {
        session.clocks.predicted = limit - now.0 as i64;
        session.stats.held += 1;
    }
    let target = tick_at(rate, session.clocks.predicted_at(now));
    // After a jump ahead, the ticks passed get no command (the host steps
    // them before one could arrive) but are still predicted; the horizon
    // bounds them.
    let skip_to = target.saturating_sub(MAX_CATCH_UP);
    let offset = session.clocks.predicted;
    let remote = session.clocks.remote_at(now);
    make(session, target, skip_to, input, offset, Some(remote))
}

/// Steps prediction to `target`, `input` as this seat's command for each
/// tick after `skip_to` (made with the predicted clock `offset` ahead of
/// local time, while remote time stood at `remote`: its view lag), and
/// queues every command the host has not acked. The ticks stepped.
fn make(
    session: &mut Session,
    target: u64,
    skip_to: u64,
    input: Command,
    offset: i64,
    remote: Option<i64>,
) -> u64 {
    let Some(play) = session.play.as_mut() else {
        return 0;
    };
    let rate = session.rate;
    let mut stepped = 0;
    while play.made < target {
        let tick = play.made + 1;
        if tick <= skip_to {
            session.stats.skipped += 1;
        } else {
            let lag = remote.map_or(0, |remote| {
                ((time_us(rate, tick) - remote) / 1000).clamp(0, i64::from(u16::MAX)) as u16
            });
            play.own.push_back((tick, input, lag));
            while play.own.len() > OWN {
                play.own.pop_front();
            }
            play.offsets[(tick % 64) as usize] = (tick, offset);
            session.stats.commands += 1;
        }
        let tick_input = play.input(session.seat, tick);
        let snapshot = step(
            &mut play.world,
            tick,
            &tick_input,
            rate.msec(tick),
            StepReason::Predict,
        );
        remember(play, &snapshot);
        if let Some(kept) = &mut session.kept {
            kept.predicted.push(snapshot.clone());
        }
        play.predicted.push(snapshot);
        play.made = tick;
        stepped += 1;
    }
    if stepped > 0 {
        send_commands(session);
    }
    stepped
}

/// Lockstep's `make`: `command` this seat's for every tick to `target`,
/// with no view lag, queued as `make` queues them, and nothing stepped
/// (`Client::frame_to`). The ticks made.
fn queue_lockstep(session: &mut Session, target: u64, command: Command) -> u64 {
    let Some(play) = session.play.as_mut() else {
        return 0;
    };
    let mut made = 0;
    while play.made < target {
        let tick = play.made + 1;
        play.own.push_back((tick, command, 0));
        while play.own.len() > OWN {
            play.own.pop_front();
        }
        play.offsets[(tick % 64) as usize] = (tick, 0);
        session.stats.commands += 1;
        play.made = tick;
        made += 1;
    }
    if made > 0 {
        send_commands(session);
    }
    made
}

/// Queues every command the host has not acked, as many as fit,
/// contiguous, up to the last made.
fn send_commands(session: &mut Session) {
    let Some(play) = session.play.as_mut() else {
        return;
    };
    let confirmed = session.ring.newest().map(|newest| newest.tick);
    let mut frame = Commands::new(session.epoch, confirmed);
    let first =
        (session.acked_through + 1).max((play.made + 1).saturating_sub(MAX_COMMANDS as u64));
    let mut from = play.made + 1;
    while from > first && play.own_command(from - 1).is_some() {
        from -= 1;
    }
    for tick in from..=play.made {
        if let Some((command, lag)) = play.own_entry(tick) {
            frame.push(tick, command, lag);
        }
    }
    let mut payload = Vec::with_capacity(16 + 4 * frame.len());
    frame.encode(&mut payload);
    if session
        .link
        .queue_unreliable(frame::COMMANDS, &payload)
        .is_ok()
    {
        session.pending_newest = Some(play.made);
        session.confirmed_sent = confirmed;
    }
}

/// Frames in a row naming a baseline this side does not hold before it
/// asks the host for a keyframe (a baseline lost for good: the host kept
/// deltas against one that left the ring).
const UNBASED_RESYNC: u32 = 16;

/// A frame named a baseline not held: after `UNBASED_RESYNC` in a row,
/// a keyframe is asked for.
fn lost_baseline(session: &mut Session) {
    session.unbased += 1;
    if session.unbased >= UNBASED_RESYNC && !session.resync_asked {
        session.resync_asked = true;
        let _ = session.link.send_reliable(&Message::Resync.to_bytes());
    }
}

/// Confirms the newest snapshot rebuilt when no commands frame will (not
/// playing yet, or stopped for an install): the host deltas against what
/// a client confirmed, and a baseline never confirmed ages out of the
/// client's ring.
fn confirm_idle(session: &mut Session) {
    let playing = session.running && session.play.as_ref().is_some_and(|play| play.started);
    let Some(newest) = session.ring.newest().map(|newest| newest.tick) else {
        return;
    };
    if playing || session.confirmed_sent.is_some_and(|sent| sent >= newest) {
        return;
    }
    let frame = Commands::new(session.epoch, Some(newest));
    let mut payload = Vec::with_capacity(16);
    frame.encode(&mut payload);
    if session
        .link
        .queue_unreliable(frame::COMMANDS, &payload)
        .is_ok()
    {
        session.confirmed_sent = Some(newest);
    }
}

/// Sets both presentations' blends for `now`, and moves the remote one to
/// the snapshots around remote time.
fn present(session: &mut Session, now: NetTime, elapsed: i64) {
    let Some(play) = session.play.as_mut() else {
        return;
    };
    if !play.started {
        return;
    }
    let rate = session.rate;
    let predicted = session.clocks.predicted_at(now);
    let (from, to) = (time_us(rate, play.made), time_us(rate, play.made + 1));
    play.predicted
        .set_alpha(((predicted - from) as f64 / (to - from).max(1) as f64) as f32);

    let remote = session.clocks.remote_at(now);
    let tick = tick_at(rate, remote);
    let (before, after) = session.ring.around(tick);
    match (before, after) {
        (Some(a), Some(b)) => {
            play.starving = false;
            if play.remote_pair != (a.tick, b.tick) {
                let mut shown = Presented::new(a.snapshot.clone());
                shown.push(b.snapshot.clone());
                play.remote = shown;
                play.remote_pair = (a.tick, b.tick);
            }
            let (from, to) = (time_us(rate, a.tick), time_us(rate, b.tick));
            play.remote
                .set_alpha(((remote - from) as f64 / (to - from).max(1) as f64) as f32);
        }
        (Some(newest), None) => {
            session.stats.starved_frames += 1;
            if !play.starving {
                play.starving = true;
                session.stats.starvations += 1;
                session.clocks.starve(now);
            }
            if play.remote_pair != (newest.tick, newest.tick) {
                play.remote = Presented::new(newest.snapshot.clone());
                play.remote_pair = (newest.tick, newest.tick);
            }
            play.remote.set_alpha(1.0);
        }
        (None, Some(oldest)) => {
            if play.remote_pair != (oldest.tick, oldest.tick) {
                play.remote = Presented::new(oldest.snapshot.clone());
                play.remote_pair = (oldest.tick, oldest.tick);
            }
            play.remote.set_alpha(1.0);
        }
        (None, None) => {}
    }

    // The correction decays (drawing only).
    if play.correction != [0.0; 3] && elapsed > 0 {
        let keep = 0.5f32.powf(elapsed as f32 / HALF_LIFE_US);
        play.correction = play.correction.map(|axis| axis * keep);
        if play.correction.iter().all(|axis| axis.abs() < 1.0 / 64.0) {
            play.correction = [0.0; 3];
        }
    }
}
