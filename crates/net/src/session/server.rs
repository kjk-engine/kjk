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

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::Arc;

use sim::{Announcement, Command, Seat, Snapshot, TickInput, TickRate};

use super::msg::{
    ClockReport, Commands, FRAGMENT, Fragment, InstallRequest, LevelRequest, MAX_FRAGMENTS,
    MAX_SAVE_PARTS, Message, SAVE_PART, SavePart, Welcome, put_payload_body, put_payload_head,
};
use crate::codec::{Confirmed, SERVER_RING, ServerRing, Stepped, encode_frame};
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
pub const SEAT_BUFFER: usize = 64;
/// The longest snapshot interval the adaptive rate goes to.
pub const MAX_EVERY: u8 = 4;
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
    pinned: Option<Base>,
    /// A snapshot waits to go.
    due: bool,
    every: u8,
    /// The current window: when it began, and the link's sent and lost
    /// counts then.
    window: (NetTime, u64, u64),
    clean_since: NetTime,
    report: ClockReport,
    report_due: bool,
    stats: ConnectionStats,
    /// Save copies waiting for this player, the oldest first.
    copies: VecDeque<Outbound>,
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
    connections: BTreeMap<Peer, Connection>,
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
    scratch: Vec<u8>,
    reserved: Vec<Reserved>,
    /// Players' actions not yet stepped, by seat in arrival order.
    actions: Vec<(Seat, sim::action::Action)>,
    /// The next save copy's number.
    next_copy: u32,
}

impl Server {
    /// A session for a world in `level` (what joiners build).
    pub fn new(config: ServerConfig, level: LevelRequest) -> Server {
        let seats = (0..config.seats).map(|_| SeatState::new()).collect();
        Server {
            handshake: handshake::Server::new(config.secret, config.join_key),
            config,
            outbox: VecDeque::new(),
            connections: BTreeMap::new(),
            in_process: Vec::new(),
            in_process_every: 1,
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
            scratch: Vec::new(),
            reserved: Vec::new(),
            actions: Vec::new(),
            next_copy: 0,
        }
    }

    /// The players' actions that came since the last call, for the next
    /// tick stepped (`TickInput::act` orders them by seat).
    pub fn actions(&mut self) -> Vec<(Seat, sim::action::Action)> {
        std::mem::take(&mut self.actions)
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

    /// Plays a sound a script played on `seat`'s player's side.
    pub fn sound(&mut self, seat: Seat, sound: sim::notice::Sound) {
        self.tell(seat, &Message::Sound(sound));
    }

    fn tell(&mut self, seat: Seat, message: &Message) {
        let message = message.to_bytes();
        if let Some(connection) = self
            .connections
            .values_mut()
            .find(|connection| connection.seat == seat)
        {
            let _ = connection.link.send_reliable(&message);
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
        for (peer, connection) in &mut self.connections {
            if Some(*peer) == except || !connection.joined {
                continue;
            }
            if connection.copies.len() >= MAX_QUEUED_COPIES {
                sent.left_out.push(connection.seat);
                continue;
            }
            connection.copies.push_back(Outbound {
                copy,
                blob: blob.clone(),
                next: 0,
            });
            sent.to.push(connection.seat);
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
        let connections = &self.connections;
        let in_process = &self.in_process;
        let free = self.free_seat().is_some();
        let reserved = &self.reserved;
        let seats = self.config.seats;
        let outcome = self
            .handshake
            .receive(now, from, bytes, |peer, session, theirs| {
                if connections
                    .get(&peer)
                    .is_some_and(|connection| connection.link.session() == session)
                {
                    return Ok(());
                }
                Credentials::check(host, theirs)?;
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
            _ => {}
        }
    }

    /// The first seat no one occupies and no one's token keeps.
    fn free_seat(&self) -> Option<usize> {
        self.seats.iter().enumerate().position(|(index, seat)| {
            seat.occupant.is_none()
                && !self
                    .reserved
                    .iter()
                    .any(|reserved| usize::from(reserved.seat.0) == index)
        })
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
        if let Some(open) = self.connections.get(&peer) {
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
            (None, None) => match self.free_seat() {
                Some(index) => (index, self.token(session)),
                None => return,
            },
        };
        let name = credentials.name.clone();
        let seat = Seat(index as u8);
        self.seats[index].vacate(Some(peer));
        let mut link = Link::with_config(session, now, self.config.link);
        if self.in_process.contains(&peer) {
            link.set_in_process();
        }
        let welcome = Message::Welcome(Welcome {
            seat,
            seats: self.seats.len() as u8,
            hz: self.config.rate.hz(),
            every: self.config.every,
            tick: self.last.map_or(0, |(tick, _)| tick),
            epoch: self.epoch,
            token,
        });
        let _ = link.send_reliable(&welcome.to_bytes());
        self.connections.insert(
            peer,
            Connection {
                link,
                seat,
                token,
                name: name.clone(),
                joined: false,
                loaded: None,
                confirmed: Confirmed::default(),
                pinned: None,
                due: false,
                every: base_every(&self.config, self.in_process_every(peer), peer),
                window: (now, 0, 0),
                clean_since: now,
                report: ClockReport::default(),
                report_due: false,

                stats: ConnectionStats::default(),
                copies: VecDeque::new(),
            },
        );
        self.events
            .push_back(ServerEvent::Joined { peer, seat, name });
        self.join_waiting();
    }

    /// Sends the level and a keyframe to connections that have neither,
    /// while the world runs and has a snapshot.
    fn join_waiting(&mut self) {
        if self.phase != Phase::Running
            || self.ring.newest().is_none()
            || self.connections.values().all(|c| c.joined)
        {
            return;
        }
        let level = Message::Level(self.level.clone()).to_bytes();
        for connection in self.connections.values_mut().filter(|c| !c.joined) {
            connection.joined = true;
            connection.confirmed = Confirmed::default();
            connection.pinned = None;
            connection.due = true;
            let _ = connection.link.send_reliable(&level);
        }
    }

    fn session(&mut self, now: NetTime, from: Peer, bytes: &[u8]) {
        let Some(connection) = self.connections.get_mut(&from) else {
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
        let Some(connection) = self.connections.get_mut(&from) else {
            return;
        };
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
            }
            // Actions name objects by their index in the module: only a
            // player who holds this epoch's world acts on it, and none while
            // a module is coming in.
            Message::Action(action)
                if connection.loaded == Some(epoch)
                    && !matches!(self.phase, Phase::Installing { .. }) =>
            {
                self.actions.push((connection.seat, action));
            }
            _ => {}
        }
    }

    fn take_commands(&mut self, now: NetTime, from: Peer, commands: &Commands) {
        let (epoch, running, last) = (self.epoch, self.phase == Phase::Running, self.last);
        let Some(connection) = self.connections.get_mut(&from) else {
            return;
        };
        if commands.epoch == epoch
            && let Some(tick) = commands.confirmed
        {
            self.ring.confirm(&mut connection.confirmed, epoch, tick);
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
            let Some(peer) = seat.occupant else {
                continue;
            };
            let Some(connection) = self.connections.get_mut(&peer) else {
                continue;
            };
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

    fn keep_stepped(&mut self, now: NetTime, input: &TickInput, stepped: Stepped) {
        let tick = stepped.tick;
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
        for (peer, connection) in &mut self.connections {
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
            let in_process = self
                .in_process
                .contains(peer)
                .then_some(self.in_process_every);
            let base = base_every(&self.config, in_process, *peer);
            adapt(connection, now, base);
            if connection.joined && tick.is_multiple_of(u64::from(connection.every)) {
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
    fn stop_clients(&mut self, newest: &Stepped, wrap: impl Fn(&[u8]) -> Vec<u8>) {
        let peers: Vec<Peer> = self.connections.keys().copied().collect();
        for peer in peers {
            let Some(connection) = self.connections.get_mut(&peer) else {
                continue;
            };
            if !connection.joined {
                continue;
            }
            connection.due = false;
            // Reports of commands before the stop mean nothing after.
            connection.report = ClockReport::default();
            connection.report_due = false;
            // A client that confirmed `newest` itself gets it whole, as
            // `encode_frame` makes a frame against a baseline not older.
            let base = baseline(&self.ring, &self.applied, connection)
                .filter(|base| base.stepped.tick < newest.tick);
            let mut payload = Vec::new();
            put_payload_head(&mut payload, 0, connection.every);
            body(
                newest,
                base.as_ref(),
                &self.applied,
                self.epoch,
                &mut payload,
            );
            let _ = connection.link.send_reliable(&wrap(&payload));
        }
    }

    /// The world runs again after `tick`: every joined client told.
    fn resume(&mut self, now: NetTime, tick: u64, late: Vec<Peer>) {
        self.phase = Phase::Running;
        self.last = Some((tick, now));
        let resume = Message::Resume { tick }.to_bytes();
        for connection in self.connections.values_mut().filter(|c| c.joined) {
            let _ = connection.link.send_reliable(&resume);
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
        for connection in self.connections.values_mut().filter(|c| c.joined) {
            connection.pinned = Some(Base {
                stepped: first.clone(),
                commands: commands.clone(),
            });
            connection.confirmed = Confirmed::default();
            connection.due = false;
            let _ = connection.link.send_reliable(&message);
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
            .filter(|(_, c)| c.joined && c.loaded != Some(epoch))
            .map(|(peer, _)| *peer)
            .collect();
        if !late.is_empty() && now.since(since) < self.config.load_wait_us {
            return false;
        }
        self.resume(now, tick, late);
        self.join_waiting();
        true
    }

    // -----------------------------------------------------------------
    // Sending.

    /// The next datagram to send at `now`. Call until `None`.
    pub fn poll_transmit(&mut self, now: NetTime) -> Option<(Peer, Vec<u8>)> {
        if let Some(out) = self.outbox.pop_front() {
            return Some(out);
        }
        self.expire(now);
        let peers: Vec<Peer> = self.connections.keys().copied().collect();
        for peer in peers {
            let sent = self.send_snapshot(now, peer);
            let connection = self.connections.get_mut(&peer)?;
            // The report rides with the snapshot, never in a packet of its own.
            if sent && connection.report_due {
                connection.report_due = false;
                self.scratch.clear();
                connection.report.encode(&mut self.scratch);
                let _ = connection
                    .link
                    .queue_unreliable(frame::CLOCK, &self.scratch);
                connection.report = ClockReport::default();
            }
            feed_copy(connection);
            if let Some(bytes) = connection.link.poll_transmit(now) {
                return Some((peer, bytes));
            }
        }
        None
    }

    /// Queues the newest snapshot for `peer` if one is due; whether one
    /// went unreliably (a report may ride with it).
    fn send_snapshot(&mut self, now: NetTime, peer: Peer) -> bool {
        let Some(connection) = self.connections.get_mut(&peer) else {
            return false;
        };
        if !connection.due || self.phase != Phase::Running {
            return false;
        }
        connection.due = false;
        let Some(newest) = self.ring.newest() else {
            return false;
        };
        let base = baseline(&self.ring, &self.applied, connection);
        let since = self.last.map_or(0, |(_, at)| now.since(at));
        let Some(base) = base else {
            // Nothing it holds: a keyframe, pinned as its baseline.
            let mut payload = Vec::new();
            put_payload_head(&mut payload, since, connection.every);
            body(newest, None, &self.applied, self.epoch, &mut payload);
            if connection
                .link
                .send_reliable(&Message::Snapshot(&payload).to_bytes())
                .is_ok()
            {
                connection.stats.keyframes += 1;
                connection.pinned = Some(Base {
                    stepped: newest.clone(),
                    commands: applied_at(&self.applied, newest.tick)
                        .cloned()
                        .unwrap_or(TickInput::default()),
                });
            }
            return false;
        };
        if base.stepped.tick >= newest.tick {
            return false;
        }
        let key = Some(base.stepped.tick);
        let cached = match self.cache.iter().position(|(at, _)| *at == key) {
            Some(at) => at,
            None => {
                let mut bytes = Vec::new();
                body(newest, Some(&base), &self.applied, self.epoch, &mut bytes);
                self.cache.push((key, bytes));
                self.cache.len() - 1
            }
        };
        let mut payload = std::mem::take(&mut self.scratch);
        payload.clear();
        put_payload_head(&mut payload, since, connection.every);
        payload.extend_from_slice(&self.cache[cached].1);
        let count = payload.len().div_ceil(FRAGMENT);
        let whole = count <= MAX_FRAGMENTS;
        if whole {
            let mut out = Vec::with_capacity(FRAGMENT + 16);
            for (index, bytes) in payload.chunks(FRAGMENT).enumerate() {
                out.clear();
                Fragment {
                    tick: newest.tick,
                    index,
                    count,
                    bytes,
                }
                .encode(&mut out);
                let _ = connection.link.queue_unreliable(frame::SNAPSHOT, &out);
            }
            connection.stats.snapshots += 1;
            connection.stats.fragments += count as u64 - 1;
        } else if connection
            .link
            .send_reliable(&Message::Snapshot(&payload).to_bytes())
            .is_ok()
        {
            connection.stats.reliable_deltas += 1;
        }
        self.scratch = payload;
        whole
    }

    /// Ends connections that broke the protocol (their seats free at once)
    /// or timed out (their seats kept for their tokens).
    fn expire(&mut self, now: NetTime) {
        let ended: Vec<(Peer, Refusal)> = self
            .connections
            .iter()
            .filter_map(|(peer, c)| {
                let why = match c.link.broken() {
                    Some(reason) => Refusal::Kicked(format!("broke the protocol: {reason}")),
                    None if c.link.timed_out(now) => Refusal::Timeout,
                    None => return None,
                };
                Some((*peer, why))
            })
            .collect();
        for (peer, why) in ended {
            self.close(now, peer, why);
        }
    }

    /// `peer` runs in this process (the host's own player): its link never
    /// times out (`Link::set_in_process`), now or when it connects.
    pub fn in_process(&mut self, peer: Peer) {
        if !self.in_process.contains(&peer) {
            self.in_process.push(peer);
        }
        if let Some(connection) = self.connections.get_mut(&peer) {
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
        for (peer, connection) in &mut self.connections {
            if peers.contains(peer) {
                connection.every = self.in_process_every;
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
        let peers: Vec<Peer> = self.connections.keys().copied().collect();
        for peer in peers {
            self.close(now, peer, Refusal::HostQuit);
        }
    }

    /// Ends a connection (telling the peer, with `tell`). A seat whose
    /// player timed out is kept for its token `RECONNECT_US`; any other
    /// is free at once.
    fn end(&mut self, now: NetTime, peer: Peer, why: Refusal, tell: bool) {
        let Some(mut connection) = self.connections.remove(&peer) else {
            return;
        };
        if let Some(seat) = self.seats.get_mut(usize::from(connection.seat.0)) {
            seat.vacate(None);
        }
        if why == Refusal::Timeout {
            self.reserved.push(Reserved {
                token: connection.token,
                seat: connection.seat,
                until: now.plus(RECONNECT_US),
            });
        }
        if tell {
            for bytes in super::goodbye(&mut connection.link, now, &why) {
                self.outbox.push_back((peer, bytes));
            }
        }
        self.events.push_back(ServerEvent::Left {
            peer,
            seat: connection.seat,
            why,
        });
    }

    /// When `poll_transmit` next has something to do.
    pub fn next_deadline(&self) -> Option<NetTime> {
        if !self.outbox.is_empty() {
            return Some(NetTime(0));
        }
        let running = self.phase == Phase::Running;
        self.connections
            .values()
            .filter_map(|c| {
                if c.due && running {
                    Some(NetTime(0))
                } else {
                    c.link.next_deadline()
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
                .map(|(peer, c)| ConnectionView {
                    peer: *peer,
                    seat: c.seat,
                    name: c.name.clone(),
                    loaded: c.loaded,
                    every: c.every,
                    link: c.link.stats(),
                    stats: c.stats,
                    window_bytes: c.link.window_bytes(),
                    held_bytes: c.link.held_bytes(),
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

/// The commands the authority applied at `tick`, if still kept.
fn applied_at(applied: &VecDeque<(u64, TickInput)>, tick: u64) -> Option<&TickInput> {
    let front = applied.front()?.0;
    let (at, input) = applied.get(usize::try_from(tick.checked_sub(front)?).ok()?)?;
    (*at == tick).then_some(input)
}

/// The baseline for a connection's next frame: its confirmed snapshot if
/// held (with the commands applied then), else its pin; none means a
/// keyframe.
fn baseline(
    ring: &ServerRing,
    applied: &VecDeque<(u64, TickInput)>,
    connection: &Connection,
) -> Option<Base> {
    let confirmed = ring.baseline(&connection.confirmed).and_then(|stepped| {
        applied_at(applied, stepped.tick).map(|commands| Base {
            stepped: stepped.clone(),
            commands: commands.clone(),
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

/// A connection's snapshot interval with nothing to slow it: every tick
/// in-process (D7) unless the host set its own peers' (`in_process`), the
/// configured one over a network.
fn base_every(config: &ServerConfig, in_process: Option<u8>, peer: Peer) -> u8 {
    match (in_process, peer) {
        (Some(every), _) => every,
        (None, Peer::Local(_)) => 1,
        (None, Peer::Addr(_) | Peer::Steam(_)) => config.every.clamp(1, MAX_EVERY),
    }
}

/// The adaptive snapshot interval (D7), judged every `WINDOW_US`: loss
/// over 10% of the packets sent in the window, or a round trip over twice
/// the least, doubles it; `FAST_AFTER_US` of clean windows halves it.
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
        if connection.every < MAX_EVERY {
            connection.every = (connection.every * 2).min(MAX_EVERY);
            connection.stats.interval_changes += 1;
        }
    } else if now.since(connection.clean_since) >= FAST_AFTER_US && connection.every > least.max(1)
    {
        connection.every = (connection.every / 2).max(least.max(1));
        connection.clean_since = now;
        connection.stats.interval_changes += 1;
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

/// The connection holding the seat `token` keeps, but for one in this
/// process (the host's own player never hands its seat over).
fn holder<'a>(
    connections: &'a BTreeMap<Peer, Connection>,
    in_process: &[Peer],
    token: u64,
) -> Option<(Peer, &'a Connection)> {
    connections
        .iter()
        .find(|(at, connection)| connection.token == token && !in_process.contains(at))
        .map(|(at, connection)| (*at, connection))
}
