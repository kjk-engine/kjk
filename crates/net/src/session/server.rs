//! The host's half of the session. It owns no world: the authority loop
//! asks it for each tick's commands (`commands_for`), steps the world,
//! and hands it the snapshot (`stepped`); it answers every connection.
//!
//! **Seats and commands (D8, D19).** A connection occupies a seat. Its
//! commands wait by tick; the step of tick `t` takes the seat's command
//! for `t`, or repeats the seat's last command for up to
//! `SUBSTITUTE_TICKS`, or else gives it its idle command: the view and
//! selections of the last command it sent, nothing moved or pressed. The
//! idle command outlives an install and a hold (which drop the commands
//! made before them) and goes with the occupant, so a seat has a command
//! every tick from its first until its connection ends: in the world, a
//! seat with none has left (`sim::drivers`), not merely gone quiet, been
//! held or still be loading. A command that comes after its
//! tick was stepped without it is late: dropped and counted, its presses
//! carried into the seat's next command. What `commands_for` returns is
//! what the authority applied, substitutes and carried presses included:
//! the recorder records exactly it.
//!
//! **Slack and the clock (D9).** Each command's slack is how long before
//! its tick was stepped it came, measured at the step (`stepped`); a late
//! one's is negative, how long after. `Clock` frames report the least
//! since the last one.
//!
//! **Snapshots (D6, D7).** Every stepped snapshot goes into one ring
//! shared by every connection; each connection gets a frame every
//! `every` ticks, a delta against the snapshot it last confirmed, or
//! against a pinned one it will surely have (the keyframe sent to it, or
//! an install's result, which it computes itself). A connection with
//! neither gets a keyframe over the reliable channel. Frames too large
//! for one datagram go in up to `MAX_FRAGMENTS`; larger still, reliably.
//! A connection whose loss passes 10% for 2 s, or whose round trip
//! doubles its least, gets snapshots half as often (to every fourth
//! tick); after 5 s clean, twice as often again. Frames against one
//! baseline are encoded once per tick and shared.
//!
//! **Installs (D13).** `installing` stops the world after a tick and
//! sends every client that tick's snapshot with the request; `installed`
//! starts the new epoch with the install's result and sends the
//! announcement; `can_step` holds the world until every client has
//! loaded or `load_wait_us` has passed, then resumes it. A client still
//! loading is a late loader; a client joining meanwhile waits for the
//! resume, then joins as any other. Players' actions name objects by their
//! index in the module, so those waiting are dropped when an install
//! starts and when it commits, none is taken while it loads, and none from
//! a player not loaded in the current epoch.
//!
//! **Save copies (I30 P17).** `save_copy` queues a blob (a save the host's
//! game packed; opaque here) for every joined player but one. Each
//! connection sends it as `SaveCopy` parts of `SAVE_PART`, the next only
//! when its reliable window is empty, so an install or a notice queued
//! after waits one part at most. A copy over `MAX_SAVE_PARTS` parts is
//! refused; a player with `MAX_QUEUED_COPIES` waiting is left out.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;

use sim::{Announcement, Command, Seat, Snapshot, TickInput, TickRate};

use super::msg::{
    ClockReport, Commands, FRAGMENT, Fragment, InstallRequest, LevelRequest, MAX_FRAGMENTS,
    MAX_SAVE_PARTS, Message, SAVE_PART, SavePart, Welcome, put_payload_body, put_payload_head,
};
use crate::codec::{Confirmed, FrameHeader, SERVER_RING, ServerRing, Stepped, encode_frame};
use crate::handshake::{self, Credentials, Refusal};
use crate::link::{self, Link};
use crate::wire::{self, Class, frame};
use crate::{NetTime, Peer};

/// Ticks a missing command is replaced by the seat's last one (128 ms).
pub const SUBSTITUTE_TICKS: u32 = 16;
/// How long the seat of a player who timed out waits for its token.
pub const RECONNECT_US: u64 = 60_000_000;
/// How long a connection must have been quiet (nothing taken from it)
/// before its seat's token, from another connection, takes it over:
/// ten of the link's keepalives (`link::KEEPALIVE_US`, which a live peer
/// sends at the least), a fifth of its timeout. A player who is heard
/// keeps its seat against anyone who saw the token (tokens travel in
/// clear); one coming back from a new address finds its old connection
/// quiet, since it stopped sending when it lost the host.
pub const TAKEOVER_QUIET_US: u64 = 1_000_000;
/// The most save copies waiting for one player; a save past that does not
/// go to them (`CopySent::left_out`, which the caller logs).
pub const MAX_QUEUED_COPIES: usize = 4;
/// The addresses whose join requests were told (`ServerEvent::Asked`)
/// kept, so a player's Hello sent again is not told again.
const ASKED_KEPT: usize = 8;

/// A save copy on its way to one player, a part at a time.
#[derive(Debug, Clone)]
struct Outbound {
    copy: u32,
    blob: Arc<[u8]>,
    next: usize,
}

/// Where a save copy goes: the seats it is queued for, and those left out
/// with `MAX_QUEUED_COPIES` already waiting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CopySent {
    pub to: Vec<Seat>,
    pub left_out: Vec<Seat>,
}

/// Why a save copy goes to no one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyRefusal {
    Empty,
    TooLarge { len: usize, max: usize },
}

impl fmt::Display for CopyRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CopyRefusal::Empty => f.write_str("an empty save"),
            CopyRefusal::TooLarge { len, max } => {
                write!(f, "{len} bytes: a copy holds at most {max}")
            }
        }
    }
}

/// A seat kept for a player who timed out: taken back by a connection
/// naming `token` before `until`.
#[derive(Debug, Clone, Copy)]
struct Reserved {
    token: u64,
    seat: Seat,
    until: NetTime,
}
/// Commands kept per seat, by tick.
pub const SEAT_BUFFER: usize = 96;
/// The longest snapshot interval the adaptive rate goes to.
pub const MAX_EVERY: u8 = 8;
/// Snapshot interval when all seats are idle and world state hasn't changed (~31 Hz).
const IDLE_EVERY: u8 = 8;
/// Consecutive unchanged idle ticks before throttling snapshot cadence.
const IDLE_TICKS: u32 = 16;
/// Rungs of the adaptive snapshot interval ladder (125 / n Hz).
const EVERY_LADDER: [u8; 6] = [1, 2, 3, 4, 6, 8];
/// Spacing between queued multi-fragment snapshot datagrams on a connection.
const FRAGMENT_PACE_US: u64 = 2_000;
/// Loss over which a connection's snapshots slow down (parts per million).
const SLOW_LOSS_PPM: u64 = 100_000;
/// The window loss and round trips are judged over.
const WINDOW_US: u64 = 2_000_000;
/// How long a clean link lasts before the interval halves.
const FAST_AFTER_US: u64 = 5_000_000;

/// How the host runs its session.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// The world's seats (connections occupy them).
    pub seats: u8,
    pub rate: TickRate,
    /// Snapshots every this many ticks over a network (1 to `MAX_EVERY`;
    /// 2 is the default); in-process peers get every tick.
    pub every: u8,

    pub link: link::Config,
    /// How long after an install the world waits for loaders.
    pub load_wait_us: u64,
    /// Keys the handshake's cookies (from the OS's randomness).
    pub secret: [u8; 32],
    /// The join password's key: none lets in any client of the same build
    /// and data.
    pub join_key: Option<handshake::JoinKey>,
    /// The host's build and data, which clients must match.
    pub credentials: Credentials,
    /// The stance sets the host plays, each by name and digest. A client
    /// refused for the stance entry of `credentials` is told these
    /// (`Refusal::stances`).
    pub stances: Vec<(String, [u8; 32])>,
    /// The host's collision, which every client builds its levels with
    /// (`Welcome::collision`).
    pub collision: u8,
}

/// What happened, for the host's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerEvent {
    /// A client occupies a seat.
    Joined {
        peer: Peer,
        seat: Seat,
        name: String,
    },
    /// A client is loaded in an epoch (joined, or installed).
    Loaded { peer: Peer, epoch: u16 },
    /// A client's install result differs from the host's: a determinism
    /// bug (it gets a keyframe).
    InstallDiffers { peer: Peer, epoch: u16 },
    /// A player's join request reached the host (its first Hello the host
    /// answered, once per address among the last `ASKED_KEPT`): the host's
    /// log then shows that the player's packets arrive.
    Asked { peer: Peer },
    /// A handshake was refused.
    Refused { peer: Peer, refusal: Refusal },
    /// A connection ended.
    Left {
        peer: Peer,
        seat: Seat,
        why: Refusal,
    },
    /// The world resumed after an install; the peers not loaded yet.
    Resumed { tick: u64, late: Vec<Peer> },
    /// A player came into the co-op lobby, in slot `slot` (its seat).
    LobbyJoined { slot: usize, name: String },
    /// The player in lobby slot `slot` left before the game started.
    LobbyLeft { slot: usize },
    /// The player in lobby slot `slot` asks for `pick` (the story's
    /// character or a companion), and whether they asked to swap with the
    /// player who has it.
    LobbyChoose {
        slot: usize,
        pick: ::session::lobby::Pick,
        swap: bool,
    },
    /// The player in lobby slot `slot` set their Ready mark.
    LobbyReady { slot: usize, on: bool },
    /// A connection in this process moved from seat `from` to seat `to`:
    /// the host's own player took the first seat, which its player left
    /// (`Server::begin` gave the host another seat, and nobody keeps the
    /// first one any more).
    Moved { peer: Peer, from: Seat, to: Seat },
}

/// The co-op lobby before the world exists: players gather, and no one is
/// welcomed until `Server::begin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Lobby {
    Gathering,
    /// The host pressed Start: each player has been told, and the world is
    /// loading.
    Starting,
}

/// One seat's input buffer.
#[derive(Debug, Clone)]
struct SeatState {
    occupant: Option<Peer>,
    /// Commands waiting, by tick modulo `SEAT_BUFFER`.
    waiting: Vec<Option<Waiting>>,
    /// What was applied, by tick modulo `SEAT_BUFFER`: the command, and
    /// whether the seat's own command was missing (a late one then counts).
    applied: Vec<Option<Applied>>,
    last: Option<(Command, u16)>,
    /// What the seat applies with no command of its own nor one to repeat
    /// (`Command::idle` of its last own one); kept across `clear`, gone
    /// with the occupant (`vacate`).
    idle: Option<Command>,
    /// Ticks in a row without the seat's own command.
    missing: u32,
    /// Presses of late commands, for the next command applied.
    carry: u16,
}

/// A command waiting for its tick, with its view lag and when it came.
#[derive(Debug, Clone, Copy)]
struct Waiting {
    tick: u64,
    command: Command,
    lag: u16,
    arrived: NetTime,
}

#[derive(Debug, Clone, Copy)]
struct Applied {
    tick: u64,
    command: Option<Command>,
    missed: bool,
    /// When the seat's own command came, and when its tick was stepped.
    arrived: Option<NetTime>,
    stepped_at: Option<NetTime>,
}

impl SeatState {
    fn new() -> SeatState {
        SeatState {
            occupant: None,
            waiting: vec![None; SEAT_BUFFER],
            applied: vec![None; SEAT_BUFFER],
            last: None,
            idle: None,
            missing: 0,
            carry: 0,
        }
    }

    /// Forgets every command (an install, a hold) but the idle one.
    fn clear(&mut self) {
        self.waiting.fill(None);
        self.applied.fill(None);
        self.last = None;
        self.missing = 0;
        self.carry = 0;
    }

    /// Forgets every command, the idle one too, for `occupant` (a new one,
    /// or none).
    fn vacate(&mut self, occupant: Option<Peer>) {
        self.clear();
        self.idle = None;
        self.occupant = occupant;
    }
}

/// A baseline a connection surely holds: a snapshot and the seat commands
/// its payload carried.
#[derive(Debug, Clone)]
struct Base {
    stepped: Stepped,
    commands: TickInput,
    /// `true` when seeded by `installed()` before the client confirmed or
    /// rebuilt it; `false` when sent on the reliable channel as a keyframe.
    from_install: bool,
}

/// An off-lock snapshot encoding job extracted from `Server::poll_encode_job`.
#[derive(Debug)]
pub struct EncodeJob {
    peer: Peer,
    epoch: u16,
    encode_gen: u64,
    newest: Stepped,
    base: Option<Base>,
    applied_now: TickInput,
    since_us: u64,
    every: u8,
}

impl EncodeJob {
    /// Whether this job encodes a full keyframe rather than a delta.
    pub fn is_keyframe(&self) -> bool {
        self.base.is_none()
    }

    /// Encodes and compresses the snapshot frame without holding any lock.
    pub fn run(self) -> EncodedJob {
        let mut frame = Vec::new();
        encode_frame(
            self.epoch,
            &self.newest,
            self.base.as_ref().map(|base| &base.stepped),
            &mut frame,
        );
        let empty = TickInput::default();
        let base_commands = self.base.as_ref().map_or(&empty, |base| &base.commands);
        let mut body_bytes = Vec::new();
        put_payload_body(&mut body_bytes, &frame, &self.applied_now, base_commands);
        EncodedJob {
            peer: self.peer,
            epoch: self.epoch,
            encode_gen: self.encode_gen,
            newest: self.newest,
            base_tick: self.base.map(|base| base.stepped.tick),
            applied_now: self.applied_now,
            since_us: self.since_us,
            every: self.every,
            body_bytes,
        }
    }
}

/// The finished bytes of an `EncodeJob`, ready for `Server::finish_encode`.
#[derive(Debug)]
pub struct EncodedJob {
    peer: Peer,
    epoch: u16,
    encode_gen: u64,
    newest: Stepped,
    base_tick: Option<u64>,
    applied_now: TickInput,
    since_us: u64,
    every: u8,
    body_bytes: Vec<u8>,
}

/// A connection's counts and measures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ConnectionStats {
    /// Snapshot frames sent unreliably (a fragmented one counts once).
    pub snapshots: u64,
    /// Fragments beyond the first.
    pub fragments: u64,
    /// Snapshots sent reliably: keyframes, and deltas too large.
    pub keyframes: u64,
    pub reliable_deltas: u64,
    /// Commands taken, and the sum of their slack (µs).
    pub commands: u64,
    pub slack_sum_us: i64,
    /// Commands after their tick.
    pub late: u64,
    /// Ticks stepped with the seat's last command in place of its own.
    pub substituted: u64,
    /// Ticks stepped with no command, the seat's own being missing.
    pub none: u64,
    /// Command frames dropped whole (another epoch, the world not
    /// running, the client not loaded) and commands too far ahead.
    pub dropped: u64,
    /// Snapshot interval changes.
    pub interval_changes: u64,
    /// Save copy parts sent (I30 P17).
    pub copy_parts: u64,
}

/// One client.
struct Connection {
    link: Link,
    seat: Seat,
    /// What takes this seat back after a timeout.
    token: u64,
    name: String,
    /// Welcome sent; level and keyframe sent (`joined`).
    joined: bool,
    loaded: Option<u16>,
    confirmed: Confirmed,
    confirmed_mask: u8,
    pinned: Option<Base>,
    /// A snapshot waits to go.
    due: bool,
    /// An off-lock encode job is currently running for this connection.
    encoding: bool,
    every: u8,
    /// Subsequent fragments of a multi-fragment snapshot delta queued to go
    /// one every `FRAGMENT_PACE_US` so datagrams do not microburst.
    paced_fragments: VecDeque<Vec<u8>>,
    paced_at: NetTime,
    /// The current window: when it began, and the link's sent and lost
    /// counts then.
    window: (NetTime, u64, u64),
    clean_since: NetTime,
    report: ClockReport,
    report_due: bool,
    stats: ConnectionStats,
    /// Save copies waiting for this player, the oldest first.
    copies: VecDeque<Outbound>,
    /// What the player came with, kept while the lobby lasts: `begin`
    /// checks it again against the credentials of the world started.
    theirs: Option<Credentials>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Running,
    /// Stopped after `tick` to install; not committed yet.
    Installing {
        tick: u64,
    },
    /// Committed at `since`; waiting for loaders.
    Waiting {
        tick: u64,
        since: NetTime,
    },
    /// Held after `tick` for a movie or a break: no ticks until `release`.
    Held {
        tick: u64,
    },
}

/// One connection's view for `stats`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionView {
    pub peer: Peer,
    pub seat: Seat,
    pub name: String,
    pub loaded: Option<u16>,
    /// The current snapshot interval, ticks.
    pub every: u8,
    pub link: link::Stats,
    pub stats: ConnectionStats,
    /// Bytes of reliable messages waiting or in flight, and of the
    /// client's being reassembled.
    pub window_bytes: usize,
    pub held_bytes: usize,
}

/// The host's counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServerStats {
    pub connections: Vec<ConnectionView>,
    /// Snapshots held for deltas, and the commands applied kept with them.
    pub ring: usize,
    pub applied: usize,
    pub epoch: u16,
    /// Datagrams that were not a handshake or a known connection's.
    pub strays: u64,
}

/// A peer-connection entry stored in the `MAX_SEATS` slab.
struct Slot {
    peer: Peer,
    conn: Connection,
}

fn empty_slab() -> Box<[Option<Slot>]> {
    std::iter::repeat_with(|| None)
        .take(usize::from(sim::MAX_SEATS))
        .collect::<Vec<_>>()
        .into_boxed_slice()
}

/// The host's half of the session (see the module docs).
pub struct Server {
    config: ServerConfig,
    /// Peers in this process (`in_process`): their links never time out.
    in_process: Vec<Peer>,
    /// The snapshot interval of the peers in this process
    /// (`set_in_process_every`; 1 unless set).
    in_process_every: u8,
    handshake: handshake::Server,
    outbox: VecDeque<(Peer, Vec<u8>)>,
    /// Connections indexed by seat (`0..MAX_SEATS`). Never iterate `peer_to_slot`.
    connections: Box<[Option<Slot>]>,
    /// Fast `Peer -> slot` index; lookups only (deterministic iteration uses `connections`).
    peer_to_slot: HashMap<Peer, usize>,
    poll_cursor: usize,
    encode_gen: u64,
    seats: Vec<SeatState>,
    ring: ServerRing,
    epoch: u16,
    /// What the authority applied, by tick, as far back as the ring.
    applied: VecDeque<(u64, TickInput)>,
    level: LevelRequest,
    phase: Phase,
    /// The last step: its tick and when.
    last: Option<(u64, NetTime)>,
    /// This tick's payload bodies by baseline tick.
    cache: Vec<(Option<u64>, Vec<u8>)>,
    events: VecDeque<ServerEvent>,
    strays: u64,
    /// The last addresses told as asking to join (`ServerEvent::Asked`).
    asked: VecDeque<Peer>,
    scratch: Vec<u8>,
    reserved: Vec<Reserved>,
    /// Players' actions not yet stepped, by seat in arrival order.
    actions: Vec<(Seat, sim::action::Action)>,
    /// The next save copy's number.
    next_copy: u32,
    /// The co-op lobby, while there is one (`Server::lobby`).
    lobby: Option<Lobby>,
    /// The seat the host's own player (`in_process`) takes when it
    /// connects: the first, unless a co-op lobby seated a guest there
    /// (`begin`).
    host_seat: u8,
    /// Consecutive unchanged ticks with default commands and no actions.
    idle_ticks: u32,
}

impl Server {
    /// A session for a world in `level` (what joiners build).
    pub fn new(config: ServerConfig, level: LevelRequest) -> Server {
        let seats = (0..config.seats).map(|_| SeatState::new()).collect();
        Server {
            handshake: handshake::Server::new(config.secret, config.join_key),
            config,
            outbox: VecDeque::new(),
            connections: empty_slab(),
            peer_to_slot: HashMap::new(),
            poll_cursor: 0,
            encode_gen: 0,
            in_process: Vec::new(),
            in_process_every: 1,
            idle_ticks: 0,
            seats,
            ring: ServerRing::new(0),
            epoch: 0,
            applied: VecDeque::new(),
            level,
            phase: Phase::Running,
            last: None,
            cache: Vec::new(),
            events: VecDeque::new(),
            strays: 0,
            asked: VecDeque::new(),
            scratch: Vec::new(),
            reserved: Vec::new(),
            actions: Vec::new(),
            next_copy: 0,
            lobby: None,
            host_seat: 0,
        }
    }

    fn connection(&self, peer: Peer) -> Option<&Connection> {
        let slot = *self.peer_to_slot.get(&peer)?;
        self.connections
            .get(slot)?
            .as_ref()
            .map(|entry| &entry.conn)
    }

    fn connection_mut(&mut self, peer: Peer) -> Option<&mut Connection> {
        let slot = *self.peer_to_slot.get(&peer)?;
        self.connections
            .get_mut(slot)?
            .as_mut()
            .map(|entry| &mut entry.conn)
    }

    /// A co-op lobby with no world yet. Players handshake as usual and take
    /// seats, but get the lobby's messages instead of a welcome until
    /// `begin`.
    pub fn lobby(config: ServerConfig) -> Server {
        let level = LevelRequest::Stage(sim::LevelId {
            name: String::new(),
            digest: [0; 32],
        });
        Server {
            lobby: Some(Lobby::Gathering),
            ..Server::new(config, level)
        }
    }

    /// Whether the co-op lobby is open (gathering or starting).
    pub fn in_lobby(&self) -> bool {
        self.lobby.is_some()
    }

    /// Sends `lobby` to each player in the lobby, with that player's slot.
    pub fn send_lobby(&mut self, lobby: &::session::lobby::Lobby) {
        if self.lobby.is_none() {
            return;
        }
        let lobby = Box::new(lobby.clone());
        for slot in self.connections.iter_mut().flatten() {
            let message = Message::Lobby {
                slot: slot.conn.seat.0,
                lobby: lobby.clone(),
            };
            let _ = slot.conn.link.send_reliable(&message.to_bytes());
        }
    }

    /// Each lobby player's round trip time in milliseconds, by slot.
    pub fn lobby_delays(&self) -> Vec<(usize, u16)> {
        (self.connections.iter().flatten())
            .filter(|slot| !self.in_process.contains(&slot.peer))
            .map(|slot| {
                let ms = slot.conn.link.stats().rtt_us / 1000;
                (
                    usize::from(slot.conn.seat.0),
                    u16::try_from(ms).unwrap_or(u16::MAX),
                )
            })
            .collect()
    }

    /// The host pressed Start: every player in the lobby is told, and one
    /// who joins from now on is told at once. The welcome waits for `begin`.
    pub fn start(&mut self) {
        if self.lobby != Some(Lobby::Gathering) {
            return;
        }
        self.lobby = Some(Lobby::Starting);
        let start = Message::Start.to_bytes();
        for slot in self.connections.iter_mut().flatten() {
            let _ = slot.conn.link.send_reliable(&start);
        }
    }

    /// The lobby ended without a game: each player is told `reason` and
    /// disconnected.
    pub fn close_lobby(&mut self, now: NetTime, reason: &str) {
        let closed = Message::Closed(reason.to_string()).to_bytes();
        for idx in 0..self.connections.len() {
            let Some(slot) = self.connections[idx].as_mut() else {
                continue;
            };
            let peer = slot.peer;
            let _ = slot.conn.link.send_reliable(&closed);
            self.end(now, peer, Refusal::Kicked(reason.to_string()), true);
        }
    }

    /// The host's world is loaded: the lobby ends and the session begins in
    /// `level` with `config` (the host's secret stays). `seats` gives each
    /// lobby slot its simulation seat (`session::lobby::Lobby::seats`): the
    /// slot that plays the story's character sits in the first seat, and
    /// the host's own player, slot 0, takes its seat when it connects. A
    /// slot `seats` does not name keeps its own number. Each lobby player
    /// whose build and data still match the world's credentials (the world
    /// may name stance sets the lobby did not) is welcomed in its seat, as
    /// a player who joins a running world is. Any other is refused. A player
    /// in a seat the world does not have is refused as the seats are full.
    pub fn begin(
        &mut self,
        now: NetTime,
        config: ServerConfig,
        level: LevelRequest,
        seats: &[(usize, Seat)],
    ) {
        let secret = self.config.secret;
        self.config = ServerConfig { secret, ..config };
        self.level = level;
        self.lobby = None;
        let seat_of = |slot: usize| {
            (seats.iter())
                .find(|(of, _)| *of == slot)
                .map_or(Seat(slot as u8), |(_, seat)| *seat)
        };
        self.host_seat = seat_of(0).0;
        let count = usize::from(self.config.seats);
        self.seats.resize_with(count, SeatState::new);
        for seat in &mut self.seats {
            seat.vacate(None);
        }
        let old = std::mem::replace(&mut self.connections, empty_slab());
        self.peer_to_slot.clear();
        let mut overflow = Vec::new();
        for mut slot in old.into_vec().into_iter().flatten() {
            slot.conn.seat = seat_of(usize::from(slot.conn.seat.0));
            let idx = usize::from(slot.conn.seat.0);
            if idx < self.connections.len() && self.connections[idx].is_none() {
                if idx < count {
                    self.seats[idx].vacate(Some(slot.peer));
                }
                self.peer_to_slot.insert(slot.peer, idx);
                self.connections[idx] = Some(slot);
            } else {
                overflow.push(slot);
            }
        }
        for mut slot in overflow {
            let peer = slot.peer;
            let why = Refusal::Full {
                seats: self.config.seats,
            };
            for bytes in super::goodbye(&mut slot.conn.link, now, &why) {
                self.outbox.push_back((peer, bytes));
            }
            self.events.push_back(ServerEvent::Left {
                peer,
                seat: slot.conn.seat,
                why,
            });
        }
        for idx in 0..self.connections.len() {
            let (peer, seat, theirs) = {
                let Some(slot) = self.connections[idx].as_mut() else {
                    continue;
                };
                (slot.peer, slot.conn.seat, slot.conn.theirs.take())
            };
            let refused = if usize::from(seat.0) >= count {
                Some(Refusal::Full {
                    seats: self.config.seats,
                })
            } else {
                theirs.and_then(|theirs| self.refusal(&theirs))
            };
            if let Some(why) = refused {
                self.end(now, peer, why, true);
                continue;
            }
            let Some(slot) = self.connections[idx].as_ref() else {
                continue;
            };
            let (seat, token, name) = (slot.conn.seat, slot.conn.token, slot.conn.name.clone());
            let welcome = self.welcome(seat, token);
            if let Some(slot) = self.connections[idx].as_mut() {
                let _ = slot.conn.link.send_reliable(&welcome);
            }
            self.events
                .push_back(ServerEvent::Joined { peer, seat, name });
        }
        self.join_waiting();
    }

    /// Why the host's credentials refuse `theirs`, if they do.
    fn refusal(&self, theirs: &Credentials) -> Option<Refusal> {
        check(&self.config.credentials, &self.config.stances, theirs).err()
    }

    /// The welcome of a connection in `seat` with reconnect token `token`.
    fn welcome(&self, seat: Seat, token: u64) -> Vec<u8> {
        Message::Welcome(Welcome {
            seat,
            seats: self.seats.len() as u8,
            hz: self.config.rate.hz(),
            every: self.config.every,
            tick: self.last.map_or(0, |(tick, _)| tick),
            epoch: self.epoch,
            token,
            stances: !self.config.stances.is_empty(),
            collision: self.config.collision,
        })
        .to_bytes()
    }

    /// The players' actions that came since the last call, for the next
    /// tick stepped (`TickInput::act` orders them by seat).
    pub fn actions(&mut self) -> Vec<(Seat, sim::action::Action)> {
        std::mem::take(&mut self.actions)
    }

    /// Puts `actions` back before those that came since: `actions` took
    /// them for a tick that was then not stepped, so they wait for the
    /// next tick again.
    pub fn put_back(&mut self, actions: Vec<(Seat, sim::action::Action)>) {
        self.actions.splice(0..0, actions);
    }

    /// Tells `seat`'s player what became of its action, or what a script
    /// said to it; nothing when no one plays that seat.
    pub fn notice(&mut self, seat: Seat, text: &str) {
        self.tell(seat, &Message::Notice(text.to_string()));
    }

    /// Opens a screen a script asked for on `seat`'s player's side.
    pub fn screen(&mut self, seat: Seat, screen: sim::notice::Screen) {
        self.tell(seat, &Message::Screen(screen));
    }

    /// Shows a mine that went off on `seat`'s player's side.
    pub fn explosion(&mut self, seat: Seat, explosion: sim::notice::Explosion) {
        self.tell(seat, &Message::Explosion(explosion));
    }

    /// Plays a sound a script played on `seat`'s player's side.
    pub fn sound(&mut self, seat: Seat, sound: sim::notice::Sound) {
        self.tell(seat, &Message::Sound(sound));
    }

    fn tell(&mut self, seat: Seat, message: &Message) {
        let message = message.to_bytes();
        if let Some(slot) = self
            .connections
            .get_mut(usize::from(seat.0))
            .and_then(Option::as_mut)
        {
            let _ = slot.conn.link.send_reliable(&message);
        }
    }

    /// Sends `blob` (a save as the host's game packs it) to every player
    /// that has joined, except `except` (the host's own client): over each
    /// one's reliable channel, a part at a time when nothing else is in
    /// flight (`feed_copy`). The seats it goes to, and those left out with
    /// `MAX_QUEUED_COPIES` waiting.
    pub fn save_copy(
        &mut self,
        blob: &[u8],
        except: Option<Peer>,
    ) -> Result<CopySent, CopyRefusal> {
        let max = SAVE_PART * MAX_SAVE_PARTS;
        if blob.is_empty() {
            return Err(CopyRefusal::Empty);
        }
        if blob.len() > max {
            return Err(CopyRefusal::TooLarge {
                len: blob.len(),
                max,
            });
        }
        let copy = self.next_copy;
        self.next_copy = self.next_copy.wrapping_add(1);
        let blob: Arc<[u8]> = Arc::from(blob);
        let mut sent = CopySent::default();
        for slot in self.connections.iter_mut().flatten() {
            if Some(slot.peer) == except || !slot.conn.joined {
                continue;
            }
            if slot.conn.copies.len() >= MAX_QUEUED_COPIES {
                sent.left_out.push(slot.conn.seat);
                continue;
            }
            slot.conn.copies.push_back(Outbound {
                copy,
                blob: blob.clone(),
                next: 0,
            });
            sent.to.push(slot.conn.seat);
        }
        sent.to.sort();
        sent.left_out.sort();
        Ok(sent)
    }

    /// The name the host plays under (its credentials'): what its players'
    /// save copies say they came from.
    pub fn name(&self) -> &str {
        &self.config.credentials.name
    }

    /// The current install epoch.
    pub fn epoch(&self) -> u16 {
        self.epoch
    }

    /// The next thing that happened.
    pub fn poll_event(&mut self) -> Option<ServerEvent> {
        self.events.pop_front()
    }

    // -----------------------------------------------------------------
    // Receiving.

    /// Takes a datagram from `from` that arrived at `now`.
    pub fn receive(&mut self, now: NetTime, from: Peer, bytes: &[u8]) {
        match wire::classify(bytes) {
            Class::Handshake => self.handshake(now, from, bytes),
            Class::Session => self.session(now, from, bytes),
            Class::Other => self.strays += 1,
        }
    }

    fn handshake(&mut self, now: NetTime, from: Peer, bytes: &[u8]) {
        self.reserved.retain(|reserved| reserved.until > now);
        let host = &self.config.credentials;
        let stances = &self.config.stances;
        let connections = &self.connections;
        let peer_to_slot = &self.peer_to_slot;
        let in_process = &self.in_process;
        let free = self.free_seat(from).is_some();
        let reserved = &self.reserved;
        let seats = self.config.seats;
        let outcome = self
            .handshake
            .receive(now, from, bytes, |peer, session, theirs| {
                if peer_to_slot
                    .get(&peer)
                    .and_then(|&slot| connections[slot].as_ref())
                    .is_some_and(|slot| slot.conn.link.session() == session)
                {
                    return Ok(());
                }
                check(host, stances, theirs)?;
                let token = theirs.reconnect;
                let live = token.and_then(|token| holder(connections, in_process, token));
                if let Some((_, connection)) = live
                    && connection.link.quiet_us(now) < TAKEOVER_QUIET_US
                {
                    return Err(Refusal::InUse);
                }
                let back = live.is_some()
                    || token.is_some_and(|token| reserved.iter().any(|seat| seat.token == token));
                if free || back {
                    Ok(())
                } else {
                    Err(Refusal::Full { seats })
                }
            });
        if let Some(reply) = outcome.reply {
            self.outbox.push_back((from, reply));
        }
        match outcome.event {
            handshake::Event::Accepted {
                session,
                credentials,
            } => self.open(now, from, session, &credentials),
            handshake::Event::Refused(refusal) => self.events.push_back(ServerEvent::Refused {
                peer: from,
                refusal,
            }),
            handshake::Event::Challenged
                if !self.in_process.contains(&from) && !self.asked.contains(&from) =>
            {
                if self.asked.len() == ASKED_KEPT {
                    self.asked.pop_front();
                }
                self.asked.push_back(from);
                self.events.push_back(ServerEvent::Asked { peer: from });
            }
            _ => {}
        }
    }

    /// The first seat for `peer` that no one occupies and no one's token
    /// keeps.
    ///
    /// The host's own player (`in_process`) takes the first seat when it is
    /// free, and else the seat a co-op lobby gave it (`host_seat`). So a
    /// lobby whose main character's player left before the host's own
    /// player connected still starts with the host on the first seat.
    ///
    /// Until the players in this process have connected, their seats are
    /// kept from others: the first seats, one for each of them, and the
    /// host's own. A guest whose request came before the host's own player
    /// first connected took the host's seat, and the host was left with no
    /// body.
    fn free_seat(&self, peer: Peer) -> Option<usize> {
        let free = |index: usize| {
            self.seats
                .get(index)
                .is_some_and(|seat| seat.occupant.is_none())
                && !(self.reserved.iter()).any(|reserved| usize::from(reserved.seat.0) == index)
        };
        let all = 0..self.seats.len();
        if self.in_process.contains(&peer) {
            let own = [0, usize::from(self.host_seat)];
            return own.into_iter().chain(all).find(|&index| free(index));
        }
        let waiting = (self.in_process.iter()).any(|own| !self.peer_to_slot.contains_key(own));
        let kept = |index: usize| {
            waiting && (index < self.in_process.len() || index == usize::from(self.host_seat))
        };
        all.into_iter().find(|&index| free(index) && !kept(index))
    }

    /// The host's own player takes the first seat when its player has gone
    /// and no token keeps it any more (a player who left on purpose at
    /// once, one who timed out `RECONNECT_US` later): the story always has a
    /// player. The seat the host's player leaves is free, and the creature
    /// it drove goes to the game's mind (`sim::world::drivers`). Every
    /// player follows the change on the same step, because the first seat's
    /// commands come from the host's player from then on. Nothing happens
    /// while the lobby lasts, while the first seat has a player or is
    /// kept, or when the host's player already sits in it.
    fn host_takes_first_seat(&mut self) {
        let first = self.seats.first();
        if self.lobby.is_some()
            || first.is_none_or(|seat| seat.occupant.is_some())
            || self.connections[0].is_some()
            || (self.reserved.iter()).any(|reserved| reserved.seat == Seat(0))
        {
            return;
        }
        let Some((peer, from)) = (self.in_process.iter())
            .find_map(|own| Some((*own, self.connection(*own)?.seat)))
            .filter(|(_, seat)| *seat != Seat(0))
        else {
            return;
        };
        let from_idx = usize::from(from.0);
        if let Some(seat) = self.seats.get_mut(from_idx) {
            seat.vacate(None);
        }
        self.seats[0].vacate(Some(peer));
        if let Some(mut slot) = self.connections.get_mut(from_idx).and_then(Option::take) {
            slot.conn.seat = Seat(0);
            self.connections[0] = Some(slot);
            self.peer_to_slot.insert(peer, 0);
        }
        self.host_seat = 0;
        self.events.push_back(ServerEvent::Moved {
            peer,
            from,
            to: Seat(0),
        });
    }

    /// A new seat's reconnect token, made from the `session` it was first
    /// given to: keyed by the host's secret, so only that player knows it.
    /// It stays the seat's while the player rejoins with it.
    fn token(&self, session: u64) -> u64 {
        let mut hasher = blake3::Hasher::new_keyed(&self.config.secret);
        hasher.update(b"kjk-net reconnect");
        hasher.update(&session.to_le_bytes());
        let bytes: [u8; 8] = hasher.finalize().as_bytes()[..8]
            .try_into()
            .unwrap_or([1; 8]);
        u64::from_le_bytes(bytes).max(1)
    }

    /// A verified Connect: a connection in the seat its token keeps (the
    /// token stays the seat's), else the first free seat with a new token
    /// (a retry of one already open changes nothing).
    fn open(&mut self, now: NetTime, peer: Peer, session: u64, credentials: &Credentials) {
        if let Some(open) = self.connection(peer) {
            if open.link.session() == session {
                return;
            }
            // The same address again, without the old one's token: a new
            // player.
            if credentials.reconnect != Some(open.token) {
                self.end(now, peer, Refusal::Kicked("connected again".into()), false);
            }
        }
        // The token of a connection still open that has gone quiet, from
        // this address or another (a new port, before the host timed the
        // old one out, or an attempt whose accept was lost): that player
        // coming back, into the seat the old one leaves.
        let live = credentials.reconnect.and_then(|token| {
            holder(&self.connections, &self.in_process, token)
                .filter(|(_, c)| c.link.quiet_us(now) >= TAKEOVER_QUIET_US)
                .map(|(at, c)| (at, c.seat, c.token))
        });
        let back = self
            .reserved
            .iter()
            .position(|reserved| Some(reserved.token) == credentials.reconnect);
        let (index, token) = match (live, back) {
            (Some((old, seat, token)), _) => {
                self.end(now, old, Refusal::Kicked("joined again".into()), false);
                (usize::from(seat.0), token)
            }
            (None, Some(at)) => {
                let reserved = self.reserved.swap_remove(at);
                (usize::from(reserved.seat.0), reserved.token)
            }
            (None, None) => match self.free_seat(peer) {
                Some(index) => (index, self.token(session)),
                None => return,
            },
        };
        if let Some(prev) = self.peer_to_slot.get(&peer).copied()
            && prev != index
        {
            self.end(now, peer, Refusal::Kicked("connected again".into()), false);
        }
        if let Some(old_slot) = self.connections[index].take() {
            self.peer_to_slot.remove(&old_slot.peer);
        }
        let name = credentials.name.clone();
        let seat = Seat(index as u8);
        self.seats[index].vacate(Some(peer));
        let mut link = Link::with_config(session, now, self.config.link);
        if self.in_process.contains(&peer) {
            link.set_in_process();
        }
        // In the lobby the welcome waits for `begin`. A player who joins
        // after Start is told at once that the game is starting.
        match self.lobby {
            None => {
                let _ = link.send_reliable(&self.welcome(seat, token));
            }
            Some(Lobby::Starting) => {
                let _ = link.send_reliable(&Message::Start.to_bytes());
            }
            Some(Lobby::Gathering) => {}
        }
        self.connections[index] = Some(Slot {
            peer,
            conn: Connection {
                link,
                seat,
                token,
                name: name.clone(),
                joined: false,
                loaded: None,
                confirmed: Confirmed::default(),
                confirmed_mask: 0,
                pinned: None,
                due: false,
                encoding: false,
                every: base_every(&self.config, self.in_process_every(peer), peer),
                paced_fragments: VecDeque::new(),
                paced_at: now,
                window: (now, 0, 0),
                clean_since: now,
                report: ClockReport::default(),
                report_due: false,

                stats: ConnectionStats::default(),
                copies: VecDeque::new(),
                theirs: self.lobby.is_some().then(|| credentials.clone()),
            },
        });
        self.peer_to_slot.insert(peer, index);
        self.events.push_back(match self.lobby {
            Some(_) => ServerEvent::LobbyJoined { slot: index, name },
            None => ServerEvent::Joined { peer, seat, name },
        });
        self.join_waiting();
    }

    /// Sends the level and a keyframe to connections that have neither,
    /// while the world runs and has a snapshot.
    fn join_waiting(&mut self) {
        if self.phase != Phase::Running
            || self.ring.newest().is_none()
            || self
                .connections
                .iter()
                .flatten()
                .all(|slot| slot.conn.joined)
        {
            return;
        }
        let level = Message::Level(self.level.clone()).to_bytes();
        for slot in self
            .connections
            .iter_mut()
            .flatten()
            .filter(|slot| !slot.conn.joined)
        {
            slot.conn.joined = true;
            slot.conn.confirmed = Confirmed::default();
            slot.conn.confirmed_mask = 0;
            slot.conn.paced_fragments.clear();
            slot.conn.pinned = None;
            slot.conn.due = true;
            let _ = slot.conn.link.send_reliable(&level);
        }
    }

    fn session(&mut self, now: NetTime, from: Peer, bytes: &[u8]) {
        let Some(connection) = self.connection_mut(from) else {
            self.strays += 1;
            return;
        };
        let Ok(received) = connection.link.receive(now, bytes) else {
            return;
        };
        let mut commands = None;
        for frame in received.frames() {
            if frame.kind == frame::COMMANDS
                && let Ok(frame) = Commands::decode(frame.payload)
            {
                // Latest wins: a packet carries one; keep the newest.
                if commands.is_none_or(|kept: Commands| frame.newest >= kept.newest) {
                    commands = Some(frame);
                }
            }
        }
        let disconnect = received
            .frames()
            .find(|frame| frame.kind == frame::DISCONNECT)
            .map(|frame| {
                Refusal::decode(&mut wire::Reader::new(frame.payload)).unwrap_or(Refusal::Left)
            });
        let messages: Vec<Vec<u8>> = connection.link.take_messages().collect();
        if let Some(why) = disconnect {
            self.end(now, from, why, false);
            return;
        }
        if let Some(commands) = commands {
            self.take_commands(now, from, &commands);
        }
        for message in messages {
            match Message::decode(&message) {
                Ok(message) => self.take_message(from, message),
                Err(_) => self.close(now, from, Refusal::Kicked("malformed message".to_string())),
            }
        }
    }

    fn take_message(&mut self, from: Peer, message: Message<'_>) {
        let epoch = self.epoch;
        let phase = self.phase;
        let lobby = self.lobby;
        let Some(&slot_idx) = self.peer_to_slot.get(&from) else {
            return;
        };
        let Some(slot) = self.connections[slot_idx].as_mut() else {
            return;
        };
        let connection = &mut slot.conn;
        match message {
            Message::Loaded {
                epoch: theirs,
                tick,
                check,
            } if theirs == epoch => {
                let known = connection
                    .pinned
                    .as_ref()
                    .filter(|pin| pin.stepped.tick == tick)
                    .map(|pin| pin.stepped.check())
                    .or_else(|| self.ring.get(tick).map(Stepped::check));
                if known.is_some_and(|known| known != check) {
                    // It takes the install's state whole: a keyframe as soon
                    // as the world runs, not at the next step (the host's
                    // own client gates its steps, and a reload's install
                    // never agrees, I30 P13).
                    connection.pinned = None;
                    connection.confirmed = Confirmed::default();
                    connection.confirmed_mask = 0;
                    connection.paced_fragments.clear();
                    connection.due = true;
                    self.events
                        .push_back(ServerEvent::InstallDiffers { peer: from, epoch });
                } else if known.is_some() {
                    self.ring.confirm(&mut connection.confirmed, epoch, tick);
                }
                connection.loaded = Some(epoch);
                self.events
                    .push_back(ServerEvent::Loaded { peer: from, epoch });
            }
            Message::Resync => {
                connection.pinned = None;
                connection.confirmed = Confirmed::default();
                connection.confirmed_mask = 0;
                connection.paced_fragments.clear();
            }
            Message::Choose { pick, swap } if lobby == Some(Lobby::Gathering) => {
                let slot = usize::from(connection.seat.0);
                self.events
                    .push_back(ServerEvent::LobbyChoose { slot, pick, swap });
            }
            Message::Ready(on) if lobby == Some(Lobby::Gathering) => {
                let slot = usize::from(connection.seat.0);
                self.events.push_back(ServerEvent::LobbyReady { slot, on });
            }
            // Actions name objects by their index in the module: only a
            // player who holds this epoch's world acts on it, and none while
            // a module is coming in.
            Message::Action(action)
                if connection.loaded == Some(epoch)
                    && !matches!(phase, Phase::Installing { .. }) =>
            {
                self.actions.push((connection.seat, action));
            }
            _ => {}
        }
    }

    fn take_commands(&mut self, now: NetTime, from: Peer, commands: &Commands) {
        let (epoch, running, last) = (self.epoch, self.phase == Phase::Running, self.last);
        let Some(&slot_idx) = self.peer_to_slot.get(&from) else {
            return;
        };
        let Some(slot) = self.connections[slot_idx].as_mut() else {
            return;
        };
        let connection = &mut slot.conn;
        if commands.epoch == epoch
            && let Some(tick) = commands.confirmed
        {
            let advanced = self.ring.confirm(&mut connection.confirmed, epoch, tick);
            if advanced || connection.confirmed == Confirmed(Some((epoch, tick))) {
                connection.confirmed_mask = commands.confirmed_mask;
            }
            if connection
                .pinned
                .as_ref()
                .is_some_and(|pin| pin.stepped.tick < tick && self.ring.get(tick).is_some())
            {
                connection.pinned = None;
            }
        }

        let seat = &mut self.seats[usize::from(connection.seat.0)];
        if commands.epoch != epoch || !running || connection.loaded != Some(epoch) {
            connection.stats.dropped += 1;
            return;
        }
        let stepped = last.map_or(0, |(tick, _)| tick);
        for (tick, command, lag) in commands.iter() {
            let slot = (tick % SEAT_BUFFER as u64) as usize;
            if tick <= stepped {
                let Some(applied) = seat.applied[slot].as_mut().filter(|a| a.tick == tick) else {
                    continue;
                };
                if applied.missed {
                    applied.missed = false;
                    let had = applied.command.map_or(0, |command| command.buttons);
                    seat.carry |= command.buttons & !had;
                    connection.stats.late += 1;
                    connection.report.late += 1;
                    connection.report.miss = connection.report.miss.max(Some(tick));
                    connection.report_due = true;
                    // How late, by the step it missed.
                    if let Some(at) = applied.stepped_at {
                        connection.report.take(tick, -(now.since(at) as i64));
                    }
                }
                continue;
            }
            if tick > stepped + SEAT_BUFFER as u64 {
                connection.stats.dropped += 1;
                continue;
            }
            if seat.waiting[slot].is_some_and(|waiting| waiting.tick == tick) {
                continue;
            }
            seat.waiting[slot] = Some(Waiting {
                tick,
                command: *command,
                lag,
                arrived: now,
            });
        }
    }

    // -----------------------------------------------------------------
    // The authority loop's side.

    /// The commands the authority steps `tick` with (see the module docs).
    pub fn commands_for(&mut self, tick: u64) -> TickInput {
        let mut input = TickInput::default();
        let running = self.phase == Phase::Running;
        for (index, seat) in self.seats.iter_mut().enumerate() {
            let Some(_peer) = seat.occupant else {
                continue;
            };
            let Some(slot_entry) = self.connections.get_mut(index).and_then(Option::as_mut) else {
                continue;
            };
            let connection = &mut slot_entry.conn;
            if !running {
                continue;
            }
            let slot = (tick % SEAT_BUFFER as u64) as usize;
            // Still loading this epoch (a late loader): idle until it plays.
            if connection.loaded != Some(self.epoch) {
                if let Some(idle) = seat.idle {
                    input.insert(Seat(index as u8), idle);
                }
                continue;
            }
            let own = seat.waiting[slot]
                .take()
                .filter(|waiting| waiting.tick == tick);
            let arrived = own.map(|waiting| waiting.arrived);
            let (command, missed) = match own {
                Some(waiting) => {
                    let command = (waiting.command, waiting.lag);
                    seat.missing = 0;
                    seat.last = Some(command);
                    seat.idle = Some(waiting.command.idle());
                    (Some(command), false)
                }
                None => {
                    seat.missing += 1;
                    let repeat = seat.last.filter(|_| seat.missing <= SUBSTITUTE_TICKS);
                    if repeat.is_some() {
                        connection.stats.substituted += 1;
                        connection.report.substituted += 1;
                    } else {
                        connection.stats.none += 1;
                    }
                    connection.report.miss = connection.report.miss.max(Some(tick));
                    connection.report_due = true;
                    (repeat.or(seat.idle.map(|idle| (idle, 0))), true)
                }
            };
            let command = command.map(|(mut command, lag)| {
                command.buttons |= std::mem::take(&mut seat.carry);
                (command, lag)
            });
            seat.applied[slot] = Some(Applied {
                tick,
                command: command.map(|(command, _)| command),
                missed,
                arrived,
                stepped_at: None,
            });
            if let Some((command, lag)) = command {
                input.insert(Seat(index as u8), command);
                // As the client saw it: 0 from a client in lockstep.
                input.set_view_lag(Seat(index as u8), lag);
            }
        }
        input
    }

    /// The authority stepped `snapshot` at `now` with `input` (what
    /// `commands_for` gave, with whatever the host added for seats no
    /// client occupies, or a replay's): both are kept for deltas, and due
    /// frames are marked.
    pub fn stepped(&mut self, now: NetTime, input: &TickInput, snapshot: &Snapshot) {
        self.keep_stepped(now, input, Stepped::new(snapshot));
    }

    /// `stepped` with a snapshot shared already: it is split only when a
    /// frame, a hold or a joiner reads it (`Stepped::shared`).
    pub fn stepped_shared(&mut self, now: NetTime, input: &TickInput, snapshot: Arc<Snapshot>) {
        self.keep_stepped(now, input, Stepped::shared(snapshot));
    }

    /// `stepped` with a snapshot split already (`Stepped::new_hashed`,
    /// made on the caller's step thread).
    pub fn stepped_split(&mut self, now: NetTime, input: &TickInput, stepped: Stepped) {
        self.keep_stepped(now, input, stepped);
    }

    /// Whether a frame can be made of `tick` (its players' snapshots go
    /// every `ServerConfig::every` ticks at the most, every multiple of it
    /// when the link is poor): the ticks worth splitting before one asks.
    /// A tick no frame reads is kept unsplit, and the ring drops it first.
    pub fn sends_on(&self, tick: u64) -> bool {
        tick.is_multiple_of(u64::from(self.config.every.clamp(1, MAX_EVERY)))
    }

    /// Whether the peers in this process get a frame of `tick`
    /// (`set_in_process_every`).
    pub fn sends_own_on(&self, tick: u64) -> bool {
        tick.is_multiple_of(u64::from(self.in_process_every))
    }

    fn keep_stepped(&mut self, now: NetTime, input: &TickInput, stepped: Stepped) {
        let tick = stepped.tick;
        let was_idle = self.idle_ticks >= IDLE_TICKS;
        let is_idle = input.actions().is_empty()
            && input.iter().all(|(_, c)| *c == Command::default())
            && self
                .ring
                .newest()
                .is_some_and(|prev| world_content_eq(prev.parts(), stepped.parts()));
        self.idle_ticks = if is_idle {
            self.idle_ticks.saturating_add(1)
        } else {
            0
        };
        while self.applied.back().is_some_and(|(last, _)| *last >= tick) {
            self.applied.pop_back();
        }
        self.applied.push_back((tick, input.clone()));
        while self.applied.len() > SERVER_RING + 1 {
            self.applied.pop_front();
        }
        self.last = Some((tick, now));
        self.ring.push(stepped);
        self.cache.clear();
        self.join_waiting();
        let slot = (tick % SEAT_BUFFER as u64) as usize;
        let in_process = &self.in_process;
        let idle_ticks = self.idle_ticks;
        let in_process_every = self.in_process_every;
        for slot_entry in self.connections.iter_mut().flatten() {
            let peer = slot_entry.peer;
            let connection = &mut slot_entry.conn;
            // The seat's command for this tick: how long before the step
            // it came (its slack).
            if let Some(applied) = self.seats[usize::from(connection.seat.0)].applied[slot]
                .as_mut()
                .filter(|applied| applied.tick == tick)
            {
                applied.stepped_at = Some(now);
                if let Some(arrived) = applied.arrived {
                    let slack = now.since(arrived) as i64;
                    connection.stats.commands += 1;
                    connection.stats.slack_sum_us += slack;
                    connection.report.take(tick, slack);
                    connection.report_due = true;
                }
            }
            let in_proc = in_process.contains(&peer);
            let in_process = in_proc.then_some(in_process_every);
            let base = base_every(&self.config, in_process, peer);
            adapt(connection, now, base);
            let effective = effective_every(connection.every, in_proc, idle_ticks);
            if connection.joined
                && ((was_idle && idle_ticks == 0 && !in_proc)
                    || tick.is_multiple_of(u64::from(effective)))
            {
                connection.due = true;
            }
        }
    }

    /// The world stopped after its newest tick to install `request`:
    /// every client gets that tick's snapshot with the request, and starts
    /// planning.
    pub fn installing(&mut self, request: InstallRequest) {
        let Some(newest) = self.ring.newest().cloned() else {
            return;
        };
        self.encode_gen = self.encode_gen.wrapping_add(1);
        self.phase = Phase::Installing { tick: newest.tick };
        self.actions.clear();
        for seat in &mut self.seats {
            seat.clear();
        }
        self.stop_clients(&newest, |payload| {
            Message::Installing {
                request: request.clone(),
                payload,
            }
            .to_bytes()
        });
    }

    /// The world holds after its newest tick (a movie, a break: the host
    /// steps none): every client gets that tick's snapshot and stops
    /// predicting until `release`. Only from running (an install holds
    /// them already). As for an install, the seats' commands made before
    /// it are dropped: each client forgets what it predicted past the hold
    /// and starts again from it.
    pub fn hold(&mut self) {
        if self.phase != Phase::Running {
            return;
        }
        let Some(newest) = self.ring.newest().cloned() else {
            return;
        };
        self.encode_gen = self.encode_gen.wrapping_add(1);
        self.phase = Phase::Held { tick: newest.tick };
        for seat in &mut self.seats {
            seat.clear();
        }
        self.stop_clients(&newest, |payload| Message::Hold { payload }.to_bytes());
    }

    /// The hold ends: the world goes on from where it stopped, every client
    /// told (`Resume`, as after an install), and those who came meanwhile
    /// join.
    pub fn release(&mut self, now: NetTime) {
        if let Phase::Held { tick } = self.phase {
            self.resume(now, tick, Vec::new());
            self.join_waiting();
        }
    }

    /// The players' actions waiting for the next tick stepped, by seat in
    /// arrival order (a resume the host steps again for).
    pub fn waiting(&self) -> &[(Seat, sim::action::Action)] {
        &self.actions
    }

    /// Every joined client gets `newest`'s snapshot in the message `wrap`
    /// makes of its payload; none is due a frame until the world runs.
    ///
    /// When a client has already confirmed `newest` (or received a reliable
    /// keyframe for it), emit a minimal header-only stub (~13 B) so
    /// `Client::take_payload` matches `ClientRing::holds` without re-sending
    /// a full keyframe.
    fn stop_clients(&mut self, newest: &Stepped, wrap: impl Fn(&[u8]) -> Vec<u8>) {
        let ring = &self.ring;
        let applied = &self.applied;
        let epoch = self.epoch;
        for slot in self.connections.iter_mut().flatten() {
            let connection = &mut slot.conn;
            if !connection.joined {
                continue;
            }
            connection.due = false;
            connection.encoding = false;
            connection.paced_fragments.clear();
            // Reports of commands before the stop mean nothing after.
            connection.report = ClockReport::default();
            connection.report_due = false;
            let already_holds = connection.confirmed == Confirmed(Some((epoch, newest.tick)))
                || connection
                    .pinned
                    .as_ref()
                    .is_some_and(|pin| pin.stepped.tick == newest.tick && !pin.from_install);
            let mut payload = Vec::new();
            put_payload_head(&mut payload, 0, connection.every);
            if already_holds {
                let mut header = Vec::new();
                FrameHeader {
                    epoch,
                    tick: newest.tick,
                    baseline: None,
                    check: newest.check(),
                }
                .write(&mut header);
                let empty = TickInput::default();
                put_payload_body(&mut payload, &header, &empty, &empty);
            } else {
                let base = baseline(ring, applied, connection)
                    .filter(|base| base.stepped.tick < newest.tick);
                body(newest, base.as_ref(), applied, epoch, &mut payload);
            }
            let _ = connection.link.send_reliable(&wrap(&payload));
        }
    }

    /// The world runs again after `tick`: every joined client told.
    fn resume(&mut self, now: NetTime, tick: u64, late: Vec<Peer>) {
        self.phase = Phase::Running;
        self.last = Some((tick, now));
        let resume = Message::Resume { tick }.to_bytes();
        for slot in self.connections.iter_mut().flatten() {
            if slot.conn.joined {
                let _ = slot.conn.link.send_reliable(&resume);
            }
        }
        self.events.push_back(ServerEvent::Resumed { tick, late });
    }

    /// The install `installing` announced will not happen (its plan
    /// failed): the world goes on where it stopped, in the same epoch.
    pub fn abandon_install(&mut self, now: NetTime) {
        let Phase::Installing { tick } = self.phase else {
            return;
        };
        self.resume(now, tick, Vec::new());
    }

    /// Whether a connection occupies a seat.
    pub fn occupied(&self, seat: Seat) -> bool {
        self.seats
            .get(usize::from(seat.0))
            .is_some_and(|state| state.occupant.is_some())
    }

    /// The host committed the install: `after` (its result) starts the
    /// next epoch; every client gets the announcement.
    pub fn installed(&mut self, now: NetTime, announcement: Announcement, after: &Snapshot) {
        let tick = after.tick;
        self.epoch = self.epoch.wrapping_add(1);
        self.encode_gen = self.encode_gen.wrapping_add(1);
        self.actions.clear();
        self.ring.begin_epoch(self.epoch);
        let first = Stepped::new(after);
        self.ring.push(first.clone());
        self.cache.clear();
        let commands = applied_at(&self.applied, tick)
            .cloned()
            .unwrap_or(TickInput::default());
        let message = Message::Installed {
            epoch: self.epoch,
            announcement: announcement.clone(),
        }
        .to_bytes();
        for slot in self.connections.iter_mut().flatten() {
            if slot.conn.joined {
                slot.conn.pinned = Some(Base {
                    stepped: first.clone(),
                    commands: commands.clone(),
                    from_install: true,
                });
                slot.conn.confirmed = Confirmed::default();
                slot.conn.confirmed_mask = 0;
                slot.conn.paced_fragments.clear();
                slot.conn.due = false;
                slot.conn.encoding = false;
                let _ = slot.conn.link.send_reliable(&message);
            }
        }
        self.level = LevelRequest::Module(announcement);
        self.phase = Phase::Waiting { tick, since: now };
    }

    /// Whether the world may step now. After an install it waits until
    /// every joined client has loaded, or `load_wait_us`; then it resumes
    /// (telling every client) and joins those who came meanwhile.
    pub fn can_step(&mut self, now: NetTime) -> bool {
        let (tick, since) = match self.phase {
            Phase::Running => return true,
            Phase::Installing { .. } | Phase::Held { .. } => return false,
            Phase::Waiting { tick, since } => (tick, since),
        };
        let epoch = self.epoch;
        let late: Vec<Peer> = self
            .connections
            .iter()
            .flatten()
            .filter(|slot| slot.conn.joined && slot.conn.loaded != Some(epoch))
            .map(|slot| slot.peer)
            .collect();
        if !late.is_empty() && now.since(since) < self.config.load_wait_us {
            return false;
        }
        self.resume(now, tick, late);
        self.join_waiting();
        true
    }

    // -----------------------------------------------------------------
    // Sending & off-lock encoding.

    /// Extracts the next snapshot encode job without running compression
    /// under lock. Cache hits are enqueued directly; cache misses and
    /// keyframes return `Some(EncodeJob)`.
    pub fn poll_encode_job(&mut self, now: NetTime) -> Option<EncodeJob> {
        self.poll_encode_job_excluding(now, None)
    }

    /// Like `poll_encode_job`, skipping `exclude` (e.g. an in-process
    /// loopback client driven on the game thread).
    pub fn poll_encode_job_excluding(
        &mut self,
        now: NetTime,
        exclude: Option<Peer>,
    ) -> Option<EncodeJob> {
        if self.phase != Phase::Running {
            return None;
        }
        let newest = self.ring.newest()?.clone();
        let len = self.connections.len();
        let since_us = self.last.map_or(0, |(_, at)| now.since(at));
        for turn in 0..len {
            let idx = (self.poll_cursor + turn) % len;
            let Some(slot) = self.connections[idx].as_mut() else {
                continue;
            };
            if Some(slot.peer) == exclude || !slot.conn.due || slot.conn.encoding {
                continue;
            }
            slot.conn.due = false;
            let base = baseline(&self.ring, &self.applied, &slot.conn);
            if base
                .as_ref()
                .is_some_and(|base| base.stepped.tick >= newest.tick)
            {
                continue;
            }
            if let Some(base) = &base
                && let Some((_, cached_bytes)) = self
                    .cache
                    .iter()
                    .find(|(at, _)| *at == Some(base.stepped.tick))
            {
                let every = effective_every(
                    slot.conn.every,
                    self.in_process.contains(&slot.peer),
                    self.idle_ticks,
                );
                let mut payload = std::mem::take(&mut self.scratch);
                let whole = enqueue_delta(
                    &mut slot.conn,
                    now,
                    newest.tick,
                    since_us,
                    every,
                    cached_bytes,
                    &mut payload,
                );
                self.scratch = payload;
                if whole {
                    attach_clock_report(&mut slot.conn, &mut self.scratch);
                }
                continue;
            }
            slot.conn.encoding = true;
            let peer = slot.peer;
            let every = effective_every(
                slot.conn.every,
                self.in_process.contains(&peer),
                self.idle_ticks,
            );
            let applied_now = applied_at(&self.applied, newest.tick)
                .cloned()
                .unwrap_or(TickInput::default());
            return Some(EncodeJob {
                peer,
                epoch: self.epoch,
                encode_gen: self.encode_gen,
                newest,
                base,
                applied_now,
                since_us,
                every,
            });
        }
        None
    }

    /// Commits a finished `EncodedJob` back into `Server`, caching deltas
    /// and enqueueing frames onto the connection's `Link`.
    pub fn finish_encode(&mut self, now: NetTime, done: EncodedJob) {
        let Some(&slot_idx) = self.peer_to_slot.get(&done.peer) else {
            return;
        };
        let Some(slot) = self.connections[slot_idx].as_mut() else {
            return;
        };
        slot.conn.encoding = false;
        if done.epoch != self.epoch
            || done.encode_gen != self.encode_gen
            || self.phase != Phase::Running
        {
            return;
        }
        match done.base_tick {
            None => {
                let mut payload = std::mem::take(&mut self.scratch);
                payload.clear();
                put_payload_head(&mut payload, done.since_us, done.every);
                payload.extend_from_slice(&done.body_bytes);
                let sent_ok = slot
                    .conn
                    .link
                    .send_reliable(&Message::Snapshot(&payload).to_bytes())
                    .is_ok();
                self.scratch = payload;
                if sent_ok {
                    slot.conn.stats.keyframes += 1;
                    let should_pin = slot.conn.confirmed
                        != Confirmed(Some((self.epoch, done.newest.tick)))
                        && slot
                            .conn
                            .pinned
                            .as_ref()
                            .is_none_or(|pin| pin.stepped.tick < done.newest.tick);
                    if should_pin {
                        slot.conn.pinned = Some(Base {
                            stepped: done.newest,
                            commands: done.applied_now,
                            from_install: false,
                        });
                    }
                }
            }
            Some(base_tick) => {
                let mut payload = std::mem::take(&mut self.scratch);
                let whole = enqueue_delta(
                    &mut slot.conn,
                    now,
                    done.newest.tick,
                    done.since_us,
                    done.every,
                    &done.body_bytes,
                    &mut payload,
                );
                self.scratch = payload;
                if whole {
                    attach_clock_report(&mut slot.conn, &mut self.scratch);
                }
                if self
                    .ring
                    .newest()
                    .is_some_and(|newest| newest.tick == done.newest.tick)
                    && !self.cache.iter().any(|(at, _)| *at == Some(base_tick))
                {
                    self.cache.push((Some(base_tick), done.body_bytes));
                }
            }
        }
    }

    /// The next datagram to send at `now`. Call until `None`.
    pub fn poll_transmit(&mut self, now: NetTime) -> Option<(Peer, Vec<u8>)> {
        self.poll_transmit_inner(now, None, true)
    }

    /// Drains ready datagrams at `now` for peers other than `exclude`
    /// without encoding uncached snapshots under lock (`poll_encode_job` +
    /// `finish_encode` handle snapshot encoding off-lock).
    pub fn poll_transmit_prepared(
        &mut self,
        now: NetTime,
        exclude: Option<Peer>,
    ) -> Option<(Peer, Vec<u8>)> {
        self.poll_transmit_inner(now, exclude, false)
    }

    /// Transmits only for `target` (used by `Solo::pump()` to service the
    /// host's local loopback client without touching remote peers).
    pub fn poll_transmit_for(&mut self, now: NetTime, target: Peer) -> Option<Vec<u8>> {
        if let Some(at) = self.outbox.iter().position(|(peer, _)| *peer == target) {
            return self.outbox.remove(at).map(|(_, bytes)| bytes);
        }
        self.expire(now);
        let slot_idx = *self.peer_to_slot.get(&target)?;
        let sent = self.send_snapshot_slot(now, slot_idx, true);
        let slot = self.connections[slot_idx].as_mut()?;
        if sent {
            attach_clock_report(&mut slot.conn, &mut self.scratch);
        }
        flush_paced(&mut slot.conn, now);
        feed_copy(&mut slot.conn);
        slot.conn.link.poll_transmit(now)
    }

    fn poll_transmit_inner(
        &mut self,
        now: NetTime,
        exclude: Option<Peer>,
        sync_encode: bool,
    ) -> Option<(Peer, Vec<u8>)> {
        match exclude {
            None => {
                if let Some(out) = self.outbox.pop_front() {
                    return Some(out);
                }
            }
            Some(skip) => {
                if let Some(at) = self.outbox.iter().position(|(peer, _)| *peer != skip) {
                    return self.outbox.remove(at);
                }
            }
        }
        self.expire(now);
        let len = self.connections.len();
        for turn in 0..len {
            let idx = (self.poll_cursor + turn) % len;
            let Some(peer) = self.connections[idx].as_ref().map(|slot| slot.peer) else {
                continue;
            };
            if Some(peer) == exclude {
                continue;
            }
            let sent = self.send_snapshot_slot(now, idx, sync_encode);
            let Some(slot) = self.connections[idx].as_mut() else {
                continue;
            };
            if sent {
                attach_clock_report(&mut slot.conn, &mut self.scratch);
            }
            flush_paced(&mut slot.conn, now);
            feed_copy(&mut slot.conn);
            if let Some(bytes) = slot.conn.link.poll_transmit(now) {
                self.poll_cursor = (idx + 1) % len;
                return Some((peer, bytes));
            }
        }
        None
    }

    /// Queues the newest snapshot for `connections[slot_idx]` if one is
    /// due; returns whether an unreliable snapshot frame was queued (a
    /// clock report may ride with it).
    fn send_snapshot_slot(&mut self, now: NetTime, slot_idx: usize, sync_encode: bool) -> bool {
        let Some(slot) = self.connections[slot_idx].as_mut() else {
            return false;
        };
        if !slot.conn.due || slot.conn.encoding || self.phase != Phase::Running {
            return false;
        }
        let Some(newest) = self.ring.newest() else {
            slot.conn.due = false;
            return false;
        };
        let base = baseline(&self.ring, &self.applied, &slot.conn);
        let since = self.last.map_or(0, |(_, at)| now.since(at));
        let Some(base) = base else {
            if !sync_encode {
                return false;
            }
            slot.conn.due = false;
            // Nothing it holds: a keyframe, pinned as its baseline.
            let mut payload = Vec::new();
            put_payload_head(&mut payload, since, slot.conn.every);
            body(newest, None, &self.applied, self.epoch, &mut payload);
            if slot
                .conn
                .link
                .send_reliable(&Message::Snapshot(&payload).to_bytes())
                .is_ok()
            {
                slot.conn.stats.keyframes += 1;
                slot.conn.pinned = Some(Base {
                    stepped: newest.clone(),
                    commands: applied_at(&self.applied, newest.tick)
                        .cloned()
                        .unwrap_or(TickInput::default()),
                    from_install: false,
                });
            }
            return false;
        };
        if base.stepped.tick >= newest.tick {
            slot.conn.due = false;
            return false;
        }
        let key = Some(base.stepped.tick);
        let cached = match self.cache.iter().position(|(at, _)| *at == key) {
            Some(at) => at,
            None if sync_encode => {
                let mut bytes = Vec::new();
                body(newest, Some(&base), &self.applied, self.epoch, &mut bytes);
                self.cache.push((key, bytes));
                self.cache.len() - 1
            }
            None => return false,
        };
        slot.conn.due = false;
        let every = effective_every(
            slot.conn.every,
            self.in_process.contains(&slot.peer),
            self.idle_ticks,
        );
        let mut payload = std::mem::take(&mut self.scratch);
        let whole = enqueue_delta(
            &mut slot.conn,
            now,
            newest.tick,
            since,
            every,
            &self.cache[cached].1,
            &mut payload,
        );
        self.scratch = payload;
        whole
    }

    /// Ends connections that broke the protocol (their seats free at once)
    /// or timed out (their seats kept for their tokens).
    fn expire(&mut self, now: NetTime) {
        let kept = self.reserved.len();
        self.reserved.retain(|reserved| reserved.until > now);
        if self.reserved.len() < kept {
            self.host_takes_first_seat();
        }
        for idx in 0..self.connections.len() {
            let reason = self.connections[idx].as_ref().and_then(|slot| {
                let why = match slot.conn.link.broken() {
                    Some(reason) => Refusal::Kicked(format!("broke the protocol: {reason}")),
                    None if slot.conn.link.timed_out(now) => Refusal::Timeout,
                    None => return None,
                };
                Some((slot.peer, why))
            });
            if let Some((peer, why)) = reason {
                self.close(now, peer, why);
            }
        }
    }

    /// `peer` runs in this process (the host's own player): its link never
    /// times out (`Link::set_in_process`), now or when it connects, and the
    /// first seat is kept for it (`free_seat`).
    pub fn in_process(&mut self, peer: Peer) {
        if !self.in_process.contains(&peer) {
            self.in_process.push(peer);
        }
        if let Some(connection) = self.connection_mut(peer) {
            connection.link.set_in_process();
        }
    }

    /// The peers in this process get a snapshot every `every` ticks (at
    /// least 1; `MAX_EVERY` is a network's) rather than every tick: a solo
    /// host shows its own world's snapshots, so its own client's rebuilds
    /// only keep the codec checked, and need not cost a rebuild a tick.
    pub fn set_in_process_every(&mut self, every: u8) {
        self.in_process_every = every.max(1);
        let peers = &self.in_process;
        for slot in self.connections.iter_mut().flatten() {
            if peers.contains(&slot.peer) {
                slot.conn.every = self.in_process_every;
            }
        }
    }

    /// `peer`'s snapshot interval when it is in this process.
    fn in_process_every(&self, peer: Peer) -> Option<u8> {
        self.in_process
            .contains(&peer)
            .then_some(self.in_process_every)
    }

    /// Ends a connection, telling the peer why, and frees its seat.
    pub fn close(&mut self, now: NetTime, peer: Peer, why: Refusal) {
        self.end(now, peer, why, true);
    }

    /// Ends every connection with `Refusal::HostQuit` (the host shuts
    /// down): the packets that say so come from `poll_transmit`.
    pub fn shutdown(&mut self, now: NetTime) {
        for idx in 0..self.connections.len() {
            if let Some(peer) = self.connections[idx].as_ref().map(|slot| slot.peer) {
                self.close(now, peer, Refusal::HostQuit);
            }
        }
    }

    /// Ends a connection (telling the peer, with `tell`). A seat whose
    /// player timed out is kept for its token `RECONNECT_US`; any other
    /// is free at once. In the lobby no seat is kept: the slot and its
    /// companion are free at once.
    fn end(&mut self, now: NetTime, peer: Peer, why: Refusal, tell: bool) {
        let Some(slot_idx) = self.peer_to_slot.remove(&peer) else {
            return;
        };
        let Some(mut slot) = self.connections[slot_idx].take() else {
            return;
        };
        if let Some(seat) = self.seats.get_mut(usize::from(slot.conn.seat.0)) {
            seat.vacate(None);
        }
        if why == Refusal::Timeout && self.lobby.is_none() {
            self.reserved.push(Reserved {
                token: slot.conn.token,
                seat: slot.conn.seat,
                until: now.plus(RECONNECT_US),
            });
        }
        self.host_takes_first_seat();
        if tell {
            for bytes in super::goodbye(&mut slot.conn.link, now, &why) {
                self.outbox.push_back((peer, bytes));
            }
        }
        self.events.push_back(match self.lobby {
            Some(_) => ServerEvent::LobbyLeft {
                slot: usize::from(slot.conn.seat.0),
            },
            None => ServerEvent::Left {
                peer,
                seat: slot.conn.seat,
                why,
            },
        });
    }

    /// When `poll_transmit` next has something to do.
    pub fn next_deadline(&self) -> Option<NetTime> {
        if !self.outbox.is_empty() {
            return Some(NetTime(0));
        }
        let running = self.phase == Phase::Running;
        self.connections
            .iter()
            .flatten()
            .filter_map(|slot| {
                if slot.conn.due && !slot.conn.encoding && running {
                    Some(NetTime(0))
                } else {
                    let paced =
                        (!slot.conn.paced_fragments.is_empty()).then_some(slot.conn.paced_at);
                    [slot.conn.link.next_deadline(), paced]
                        .into_iter()
                        .flatten()
                        .min()
                }
            })
            .min()
    }

    /// Counts and measures.
    pub fn stats(&self) -> ServerStats {
        ServerStats {
            connections: self
                .connections
                .iter()
                .flatten()
                .map(|slot| {
                    let c = &slot.conn;
                    ConnectionView {
                        peer: slot.peer,
                        seat: c.seat,
                        name: c.name.clone(),
                        loaded: c.loaded,
                        every: c.every,
                        link: c.link.stats(),
                        stats: c.stats,
                        window_bytes: c.link.window_bytes(),
                        held_bytes: c.link.held_bytes(),
                    }
                })
                .collect(),
            ring: self.ring.len(),
            applied: self.applied.len(),
            epoch: self.epoch,
            strays: self.strays,
        }
    }

    /// Snapshots held for deltas (what memory the ring takes, without
    /// `stats`' copies).
    pub fn held(&self) -> usize {
        self.ring.len()
    }

    /// The view lag (ms) of a seat's last command.
    pub fn view_lag(&self, seat: Seat) -> Option<u16> {
        let state = self.seats.get(usize::from(seat.0))?;
        state.last.map(|(_, lag)| lag)
    }
}

fn attach_clock_report(connection: &mut Connection, scratch: &mut Vec<u8>) {
    if connection.report_due {
        connection.report_due = false;
        scratch.clear();
        connection.report.encode(scratch);
        let _ = connection.link.queue_unreliable(frame::CLOCK, scratch);
        connection.report = ClockReport::default();
    }
}

fn flush_paced(connection: &mut Connection, now: NetTime) {
    if !connection.paced_fragments.is_empty()
        && now >= connection.paced_at
        && let Some(frag) = connection.paced_fragments.pop_front()
    {
        let _ = connection.link.queue_unreliable(frame::SNAPSHOT, &frag);
        if !connection.paced_fragments.is_empty() {
            connection.paced_at = now.plus(FRAGMENT_PACE_US);
        }
    }
}

fn enqueue_delta(
    connection: &mut Connection,
    now: NetTime,
    tick: u64,
    since_us: u64,
    every: u8,
    body_bytes: &[u8],
    payload: &mut Vec<u8>,
) -> bool {
    payload.clear();
    put_payload_head(payload, since_us, every);
    payload.extend_from_slice(body_bytes);
    let count = payload.len().div_ceil(FRAGMENT);
    let whole = count <= MAX_FRAGMENTS;
    if whole {
        connection.paced_fragments.clear();
        let mut out = Vec::with_capacity(FRAGMENT + 16);
        for (index, bytes) in payload.chunks(FRAGMENT).enumerate() {
            out.clear();
            Fragment {
                tick,
                index,
                count,
                bytes,
            }
            .encode(&mut out);
            if index == 0 {
                let _ = connection.link.queue_unreliable(frame::SNAPSHOT, &out);
            } else {
                connection.paced_fragments.push_back(out.clone());
            }
        }
        if !connection.paced_fragments.is_empty() {
            connection.paced_at = now.plus(FRAGMENT_PACE_US);
        }
        connection.stats.snapshots += 1;
        connection.stats.fragments += count as u64 - 1;
    } else {
        connection.paced_fragments.clear();
        if connection
            .link
            .send_reliable(&Message::Snapshot(payload).to_bytes())
            .is_ok()
        {
            connection.stats.reliable_deltas += 1;
        }
    }
    whole
}

/// The commands the authority applied at `tick`, if still kept.
fn applied_at(applied: &VecDeque<(u64, TickInput)>, tick: u64) -> Option<&TickInput> {
    let front = applied.front()?.0;
    let (at, input) = applied.get(usize::try_from(tick.checked_sub(front)?).ok()?)?;
    (*at == tick).then_some(input)
}

/// The baseline for a connection's next frame: its confirmed snapshot if
/// held (with the commands applied then), falling back through recent
/// `confirmed_mask` bits, else its pin; none means a keyframe.
fn baseline(
    ring: &ServerRing,
    applied: &VecDeque<(u64, TickInput)>,
    connection: &Connection,
) -> Option<Base> {
    let confirmed = ring
        .baseline(&connection.confirmed)
        .and_then(|stepped| {
            applied_at(applied, stepped.tick).map(|commands| Base {
                stepped: stepped.clone(),
                commands: commands.clone(),
                from_install: false,
            })
        })
        .or_else(|| {
            let Confirmed(Some((epoch, top))) = connection.confirmed else {
                return None;
            };
            if epoch != ring.epoch() {
                return None;
            }
            (0..8u64).find_map(|bit| {
                if (connection.confirmed_mask & (1 << bit)) == 0 {
                    return None;
                }
                let tick = top.checked_sub(bit + 1)?;
                let stepped = ring.get(tick)?;
                applied_at(applied, tick).map(|commands| Base {
                    stepped: stepped.clone(),
                    commands: commands.clone(),
                    from_install: false,
                })
            })
        });
    match (confirmed, &connection.pinned) {
        (Some(confirmed), Some(pin)) if pin.stepped.tick > confirmed.stepped.tick => {
            Some(pin.clone())
        }
        (Some(confirmed), _) => Some(confirmed),
        (None, pin) => pin.clone(),
    }
}

/// Appends a payload's body: the frame of `now` against `base`, and the
/// commands applied at `now` against the base's.
fn body(
    now: &Stepped,
    base: Option<&Base>,
    applied: &VecDeque<(u64, TickInput)>,
    epoch: u16,
    out: &mut Vec<u8>,
) {
    let mut frame = Vec::new();
    encode_frame(epoch, now, base.map(|base| &base.stepped), &mut frame);
    let empty = TickInput::default();
    let commands = applied_at(applied, now.tick).unwrap_or(&empty);
    let base_commands = base.map_or(&empty, |base| &base.commands);
    put_payload_body(out, &frame, commands, base_commands);
}

fn effective_every(every: u8, in_proc: bool, idle_ticks: u32) -> u8 {
    if !in_proc && idle_ticks >= IDLE_TICKS {
        every.max(IDLE_EVERY)
    } else {
        every
    }
}

fn world_content_eq(a: &sim::Parts, b: &sim::Parts) -> bool {
    a.tail == b.tail
        && a.entities.len() == b.entities.len()
        && a.tail_cold
            .iter()
            .zip(&b.tail_cold)
            .all(|(x, y)| Arc::ptr_eq(x, y) || **x == **y)
        && a.entities.iter().zip(&b.entities).all(|(x, y)| {
            x.id == y.id && x.hot == y.hot && (Arc::ptr_eq(&x.cold, &y.cold) || *x.cold == *y.cold)
        })
}

/// A connection's snapshot interval with nothing to slow it: every tick
/// in-process (D7) unless the host set its own peers' (`in_process`), the
/// configured one over a network.
fn base_every(config: &ServerConfig, in_process: Option<u8>, _peer: Peer) -> u8 {
    in_process.unwrap_or_else(|| config.every.clamp(1, MAX_EVERY))
}

/// The adaptive snapshot interval (D7), judged every `WINDOW_US`: loss
/// over 10% of the packets sent in the window, or a round trip over twice
/// the least, steps up `EVERY_LADDER` (`2 -> 3 -> 4 -> 6 -> 8`);
/// `FAST_AFTER_US` of clean windows steps back down.
fn adapt(connection: &mut Connection, now: NetTime, least: u8) {
    let (began, sent, lost) = connection.window;
    if now.since(began) < WINDOW_US {
        return;
    }
    let stats = connection.link.stats();
    connection.window = (now, stats.packets_sent, stats.declared_lost);
    let (sent, lost) = (stats.packets_sent - sent, stats.declared_lost - lost);
    let bad = lost * 1_000_000 > SLOW_LOSS_PPM * sent.max(1)
        || (stats.rtt_min_us > 0 && stats.rtt_us > 2 * stats.rtt_min_us);
    if bad {
        connection.clean_since = now;
        if let Some(next) = EVERY_LADDER.iter().copied().find(|&r| r > connection.every) {
            connection.every = next;
            connection.stats.interval_changes += 1;
        }
    } else if now.since(connection.clean_since) >= FAST_AFTER_US && connection.every > least.max(1)
    {
        let floor = least.max(1);
        let prev = EVERY_LADDER
            .iter()
            .rev()
            .copied()
            .find(|&r| r < connection.every && r >= floor)
            .unwrap_or(floor);
        if prev < connection.every {
            connection.every = prev;
            connection.clean_since = now;
            connection.stats.interval_changes += 1;
        }
    }
}

/// The next part of the connection's oldest save copy, when its reliable
/// window is empty: a part never holds up the session's own messages by
/// more than itself.
fn feed_copy(connection: &mut Connection) {
    if connection.link.reliable_in_flight() > 0 {
        return;
    }
    let Some(outbound) = connection.copies.front_mut() else {
        return;
    };
    let Some(part) = SavePart::split(outbound.copy, &outbound.blob).nth(outbound.next) else {
        connection.copies.pop_front();
        return;
    };
    let last = part.part + 1 == part.parts;
    if connection
        .link
        .send_reliable(&Message::SaveCopy(part).to_bytes())
        .is_ok()
    {
        connection.stats.copy_parts += 1;
        outbound.next += 1;
        if last {
            connection.copies.pop_front();
        }
    }
}

/// Whether `host`'s build and data take a player with `theirs`. A player
/// refused for the stance entry is told the host's `stances`.
fn check(
    host: &Credentials,
    stances: &[(String, [u8; 32])],
    theirs: &Credentials,
) -> Result<(), Refusal> {
    Credentials::check(host, theirs).map_err(|refusal| match refusal {
        Refusal::GameData { game, .. } if game == wire::STANCE_GAME => Refusal::stances(stances),
        refusal => refusal,
    })
}

/// The connection holding the seat `token` keeps, but for one in this
/// process (the host's own player never hands its seat over).
fn holder<'a>(
    connections: &'a [Option<Slot>],
    in_process: &[Peer],
    token: u64,
) -> Option<(Peer, &'a Connection)> {
    connections
        .iter()
        .flatten()
        .find(|slot| slot.conn.token == token && !in_process.contains(&slot.peer))
        .map(|slot| (slot.peer, &slot.conn))
}
