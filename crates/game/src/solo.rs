//! Single player over the network's own path (D14): the host's
//! `net::session::Server` and a local `Client` joined over `Loopback`, the
//! same bytes a socket would carry. `sim_host` steps the authority world
//! and the views are given its snapshots; the client rebuilds one every
//! `CHECKED_EVERY` ticks through the codec, each checked against the
//! host's hash, so every solo session keeps the codec proven without a
//! rebuild a tick (which cost as much as the step it showed).
//!
//! **Lockstep.** The host and its own client share one clock: for each tick
//! the host is about to step, the client first makes that tick's command
//! (`command`), predicts it and sends it; the host then steps with what the
//! server hands it (`commands_for`), and the client rebuilds the snapshot
//! (`stepped`), all within the frame. So the host applies exactly the
//! commands the client made, tick for tick, as the direct host did: a
//! scripted run records the same `inputs.kjkr` it did before, and shows
//! the same moments. Nothing is stepped until the client plays. The
//! client predicts nothing (`Client::frame_to`): the tick it would predict
//! is stepped by the host in the same frame.
//!
//! **Installs** take the D13 flow in-process: `installing` when a module
//! starts to load, `installed` when the host committed it (the client
//! commits the same plan's level itself, checks the announced state, and
//! the world resumes once every client loaded), `abandon` when its plan
//! failed.
//!
//! **Hosting** (`kjk host`): the same server also listens on a socket,
//! pumped by a net thread (`net_thread`, D3) that shares it (`Hosted`);
//! players join it as over the loopback (their commands come in by tick,
//! the pilot never drives an occupied seat), and every stepped tick's hash
//! goes into `hashes.txt` in the run folder (epoch, tick, hash), which a
//! joined client's `rebuilt.txt` is compared with.

use std::collections::{BTreeMap, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::io::Write as _;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use net::handshake::{Credentials, GameFingerprint, JoinKey};
use net::io::{Loopback, Socket};
use net::link::Config;
use net::netem::Netem;
use net::session::{
    Cause, Client, ClientConfig, ClientEvent, EncodeJob, EncodedJob, InstallRequest, LevelRequest,
    Server, ServerConfig, ServerEvent,
};
use net::{NetTime, Peer, Transport};
use sim::action::Action;
use sim::{Announcement, Command, Level, Seat, Snapshot, TickInput, TickRate};

use crate::net_feel::{self, Feel};
use crate::net_thread::{Endpoint, NetThread, Shared, lock};

/// Whether per-tick snapshot hash audit logs (`hashes.txt` / `rebuilt.txt`)
/// and `Prepare::hash` / `ClientConfig::audit` are enabled. Off by default
/// so live co-op avoids hashing every snapshot twice and flushing logs under
/// lock; `cargo xtask duo` and `lobby` set `KJK_NET_AUDIT=1`.
pub fn net_audit_enabled() -> bool {
    std::env::var_os("KJK_NET_AUDIT").is_some_and(|v| !v.is_empty() && v != "0")
}

/// Rounds of moving datagrams a pump makes at most (each round moves all
/// that is waiting; a handshake and a join take a handful).
const PUMP_ROUNDS: usize = 64;

/// The host's own client rebuilds a snapshot every this many ticks (a
/// delta across them, hash-checked as any): the codec checked four times
/// a second, its cost (a rebuild took 1.5 ms in Ahto Central) once in 32
/// ticks rather than every tick.
const CHECKED_EVERY: u8 = 32;

/// The host's server and its end of the loopback, shared with the net
/// thread when hosting: what the server sends its own client goes into the
/// loopback at once, the rest waits for the socket.
pub struct Hosted {
    server: Server,
    host_end: Loopback,
    waiting: VecDeque<(Peer, Vec<u8>)>,
    /// Players' actions held back while a break holds, for the tick that ends it.
    after_break: Vec<(Seat, Action)>,
}

impl Hosted {
    /// Moves what the server has to send; whether it had any.
    fn route(&mut self, now: NetTime) -> bool {
        let mut moved = false;
        while let Some((to, bytes)) = self.server.poll_transmit(now) {
            if to == self.host_end.remote() {
                self.host_end.send(to, &bytes);
            } else {
                self.waiting.push_back((to, bytes));
            }
            moved = true;
        }
        moved
    }

    /// Moves only what the server has for the host's own loopback client,
    /// leaving remote peers' encoding and transmission to `NetThread`.
    fn route_local(&mut self, now: NetTime) -> bool {
        let target = self.host_end.remote();
        let mut moved = false;
        while let Some(bytes) = self.server.poll_transmit_for(now, target) {
            self.host_end.send(target, &bytes);
            moved = true;
        }
        moved
    }

    pub(crate) fn server(&self) -> &Server {
        &self.server
    }

    pub(crate) fn server_mut(&mut self) -> &mut Server {
        &mut self.server
    }

    pub(crate) fn commands_for(&mut self, tick: u64, paused_by: Option<Seat>) -> TickInput {
        let mut input = self.server.commands_for(tick);
        let mut actions = std::mem::take(&mut self.after_break);
        actions.extend(self.server.actions());
        let ends = paused_by.is_none_or(|by| {
            actions.iter().any(|(seat, action)| {
                *action == Action::Resume && sim::party::may_resume(by, *seat)
            })
        });
        for (seat, action) in actions {
            if ends || action == Action::Resume || sim::party::passes_break(seat, &action) {
                input.act(seat, action);
            } else {
                self.after_break.push((seat, action));
            }
        }
        input
    }
}

impl Endpoint for Hosted {
    fn receive(&mut self, now: NetTime, from: Peer, bytes: &[u8]) {
        self.server.receive(now, from, bytes);
    }

    fn poll_transmit(&mut self, now: NetTime) -> Option<(Peer, Vec<u8>)> {
        if let Some(out) = self.waiting.pop_front() {
            return Some(out);
        }
        self.server
            .poll_transmit_prepared(now, Some(self.host_end.remote()))
    }

    fn next_deadline(&self) -> Option<NetTime> {
        if self.waiting.is_empty() {
            self.server.next_deadline()
        } else {
            Some(NetTime(0))
        }
    }

    fn poll_encode_job(&mut self, now: NetTime) -> Option<EncodeJob> {
        self.server
            .poll_encode_job_excluding(now, Some(self.host_end.remote()))
    }

    fn finish_encode(&mut self, now: NetTime, done: EncodedJob) {
        self.server.finish_encode(now, done);
    }

    fn parallel_encode(&self) -> bool {
        self.server.seats() > 4
    }
}

/// The host's server, its own client, and the loopback between them.
pub struct Solo {
    hosted: Shared<Hosted>,
    client: Client,
    client_end: Loopback,
    /// The level the client builds when asked (the host's own).
    level: Level,
    /// The host committed an install whose level its own client was not
    /// given yet. The client gets it once it has heard of the install. A
    /// large install message, such as a whole snapshot after a hold, can
    /// reach the client only after the host committed (issue 643).
    level_owed: bool,
    /// The angles of the last command made: a seat with nothing to press
    /// keeps looking there.
    angles: [i16; 3],
    /// When hosting: the net thread with the socket players join over, its
    /// address, and the stepped hashes' log.
    net: Option<NetThread>,
    address: Option<SocketAddr>,
    hashes: Option<std::io::BufWriter<std::fs::File>>,
    players: BTreeMap<Peer, Player>,
    /// The net feel of each player over the network (D16), and how often
    /// the world resumed without a player still loading.
    feel: Feel,
    late_resumes: u64,
    /// What the host told this player of its actions, not yet shown.
    notices: Vec<String>,
    screens: Vec<sim::notice::Screen>,
    sounds: Vec<sim::notice::Sound>,
    explosions: Vec<sim::notice::Explosion>,
    /// The socket's receive buffer, as the OS gave it.
    recv_buffer: Option<usize>,
}

/// What a host listens with: its socket (or the co-op lobby already
/// listening on one), the build and data every player must match, and the
/// join password's key, if it has one.
pub struct Listen {
    pub socket: Listening,
    pub credentials: Credentials,
    pub join_key: Option<JoinKey>,
}

/// Where a host's players come in: a socket nobody listens on yet, or the
/// co-op lobby, whose players are kept as the session begins.
pub enum Listening {
    Socket(Box<Netem<Socket>>),
    Lobby(Box<OpenNet>),
}

/// The co-op lobby's network, open before the world exists: the server in
/// its lobby phase (`Server::lobby`), the host's own end of the loopback,
/// and the net thread that answers players on the socket.
pub struct OpenNet {
    hosted: Shared<Hosted>,
    client_end: Loopback,
    net: NetThread,
    address: Option<SocketAddr>,
    recv_buffer: Option<usize>,
    /// The host's Steam ID, where Steam is beside the socket.
    steam: Option<u64>,
    /// Each lobby slot's simulation seat when the game starts
    /// (`seat_slots`); empty keeps each slot in the seat of its number.
    seats: Vec<(usize, Seat)>,
}

/// How long a closing lobby waits for its goodbyes to leave: a few of the
/// net thread's turns.
const GOODBYE_WAIT: std::time::Duration = std::time::Duration::from_millis(100);

impl OpenNet {
    /// A lobby listening on `socket` for players whose build and data are
    /// `credentials`, with `seats` seats, the first kept for the host's own
    /// player.
    pub fn open(
        socket: Netem<Socket>,
        seats: u8,
        credentials: Credentials,
        join_key: Option<JoinKey>,
    ) -> Result<OpenNet, String> {
        let (host_end, client_end) = Loopback::pair(clock());
        let config = server_config(seats, TickRate::default(), true, join_key, credentials);
        let mut server = Server::lobby(config);
        server.in_process(host_end.remote());
        let address = socket.inner().udp_end().and_then(|udp| udp.address());
        let recv_buffer = socket.inner().recv_buffer();
        let steam = socket.inner().steam_id();
        let hosted = Arc::new(Mutex::new(Hosted {
            server,
            host_end,
            waiting: VecDeque::new(),
            after_break: Vec::new(),
        }));
        let net = NetThread::spawn(socket, hosted.clone(), clock())
            .map_err(|error| format!("the net thread: {error}"))?;
        Ok(OpenNet {
            hosted,
            client_end,
            net,
            address,
            recv_buffer,
            steam,
            seats: Vec::new(),
        })
    }

    /// Each lobby slot's simulation seat for the game about to start
    /// (`session::lobby::Lobby::seats`): the server gives each kept player
    /// its seat as the session begins (`Server::begin`), the host's own
    /// player included.
    pub fn seat_slots(&mut self, seats: Vec<(usize, Seat)>) {
        self.seats = seats;
    }

    /// The seat the host's own player takes: slot 0's.
    pub fn host_seat(&self) -> Seat {
        (self.seats.iter())
            .find(|(slot, _)| *slot == 0)
            .map_or(Seat(0), |(_, seat)| *seat)
    }

    /// Whether Steam is beside the socket, so friends can be invited.
    pub fn steam(&self) -> bool {
        self.steam.is_some()
    }

    /// Opens Steam's invite dialog for this game, where Steam is beside the
    /// socket.
    pub fn invite(&self) {
        if let Some(id) = self.steam {
            Socket::invite(id);
        }
    }

    /// Lets the net thread send what the server queued (a closing lobby's
    /// goodbyes) before the lobby is dropped.
    pub fn flush(&self) {
        std::thread::sleep(GOODBYE_WAIT);
    }

    /// The address players join, when it listens on one.
    pub fn address(&self) -> Option<SocketAddr> {
        self.address
    }

    /// Runs `work` on the lobby's server, locked.
    pub fn with_server<R>(&self, work: impl FnOnce(&mut Server) -> R) -> R {
        let out = work(&mut lock(&self.hosted).server);
        self.net.wake();
        out
    }
}

/// A host's server settings for `seats` seats at `rate`: over a network
/// when `hosting`, else to the host's own player alone. The world's stance
/// sets and collision are set as it begins.
fn server_config(
    seats: u8,
    rate: TickRate,
    hosting: bool,
    join_key: Option<JoinKey>,
    credentials: Credentials,
) -> ServerConfig {
    ServerConfig {
        seats,
        rate,
        every: if seats > 4 { 4 } else { 2 },
        // Pacing is for a network; the loopback peer is local.
        link: if hosting {
            Config::default()
        } else {
            Config::loopback()
        },
        load_wait_us: 30_000_000,
        secret: if hosting { random_key() } else { [0x5a; 32] },
        join_key,
        credentials,
        stances: Vec::new(),
        collision: session::collision().code(),
        quorum_percent: if seats > 4 { 80 } else { 100 },
    }
}

/// This build, named `name`, with `games`' world data
/// (`Credentials::check` compares them). The build names the rulebook's
/// edition too: two builds of one commit whose books were edited apart
/// (a playtest's change not yet committed) play by different rules.
pub fn credentials(name: &str, games: Vec<GameFingerprint>) -> Credentials {
    Credentials {
        build: format!("{} rules {:016x}", env!("KJK_GIT_SHA"), rules::EDITION),
        games,
        name: name.to_string(),
        reconnect: None,
    }
}

/// Whether this build had changes outside a commit: only the same dirty
/// build may play with it, and both sides say so.
pub fn dirty() -> bool {
    env!("KJK_GIT_SHA").ends_with("-dirty")
}

/// The process's network clock: microseconds since it was first read.
/// Every transport and session of the process stamps with it, so their
/// times compare.
pub fn now() -> NetTime {
    static STARTED: std::sync::LazyLock<Instant> = std::sync::LazyLock::new(Instant::now);
    NetTime(STARTED.elapsed().as_micros() as u64)
}

/// `now` as a transport's clock.
pub fn clock() -> net::io::Clock {
    Arc::new(now)
}

/// Writes `value` to `path`, pretty (D16's per-run network report).
pub fn write_json(path: &Path, value: &serde_json::Value) {
    let text = serde_json::to_string_pretty(value).unwrap_or(String::new());
    if let Err(error) = std::fs::write(path, text) {
        bevy::log::error!(target: "net", "{}: {error}", path.display());
    }
}

/// 32 bytes from the OS's randomness (the standard library seeds its
/// hashers from it): the cookie key of a host players reach over a socket.
pub fn random_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    for (index, chunk) in key.chunks_mut(8).enumerate() {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_usize(index);
        chunk.copy_from_slice(&hasher.finish().to_le_bytes());
    }
    key
}

impl Solo {
    /// A session for a world in `level` whose start is `start`, joined by
    /// its own client (which plays from the next `pump`), and listening
    /// with `listen` if hosting (the hashes then go to `folder/hashes.txt`).
    pub fn new(
        level: &Level,
        request: LevelRequest,
        rate: TickRate,
        start: &Snapshot,
        listen: Option<Box<Listen>>,
        folder: &Path,
    ) -> Result<Solo, String> {
        let (socket, credentials, join_key) = match listen {
            Some(listen) => (Some(listen.socket), listen.credentials, listen.join_key),
            None => (None, credentials("local", Vec::new()), None),
        };
        let hosting = socket.is_some();
        // Under MDA a host with stance sets names them, so a guest must
        // have the same sets to join.
        let stances = level
            .anims()
            .stances()
            .filter(|sets| start.saber_rules.duel() && !sets.is_empty());
        let mut credentials = credentials;
        if let Some(sets) = stances {
            credentials.games.push(net::handshake::GameFingerprint {
                game: net::wire::STANCE_GAME.to_string(),
                digest: sets.digest(),
                groups: Vec::new(),
            });
        }
        let config = ServerConfig {
            stances: stances.map_or_else(Vec::new, |sets| sets.ids()),
            ..server_config(start.seats(), rate, hosting, join_key, credentials.clone())
        };
        let (hosted, client_end, net, address, recv_buffer) = match socket {
            // The lobby's players are kept: each is welcomed into its seat
            // now that the world exists.
            Some(Listening::Lobby(open)) => {
                let open = *open;
                lock(&open.hosted)
                    .server
                    .begin(now(), config, request, &open.seats);
                let OpenNet {
                    hosted,
                    client_end,
                    net,
                    address,
                    recv_buffer,
                    ..
                } = open;
                (hosted, client_end, Some(net), address, recv_buffer)
            }
            socket => {
                let socket = match socket {
                    Some(Listening::Socket(socket)) => Some(*socket),
                    _ => None,
                };
                let (host_end, client_end) = Loopback::pair(clock());
                let mut server = Server::new(config, request);
                // The host's own player is in this process: a long frame (a
                // pipeline warm-up) stalls both sides, so neither times the
                // other out.
                server.in_process(host_end.remote());
                let address =
                    (socket.as_ref()).and_then(|socket| socket.inner().udp_end()?.address());
                let recv_buffer = (socket.as_ref()).and_then(|socket| socket.inner().recv_buffer());
                let hosted = Arc::new(Mutex::new(Hosted {
                    server,
                    host_end,
                    waiting: VecDeque::new(),
                    after_break: Vec::new(),
                }));
                let net = socket
                    .map(|socket| NetThread::spawn(socket, hosted.clone(), clock()))
                    .transpose()
                    .map_err(|error| format!("the net thread: {error}"))?;
                (hosted, client_end, net, address, recv_buffer)
            }
        };
        let mut hashes = (hosting && net_audit_enabled())
            .then(|| std::fs::File::create(folder.join("hashes.txt")).ok())
            .flatten()
            .map(std::io::BufWriter::new);
        let server_peer = {
            let mut shared = lock(&hosted);
            shared.server.set_in_process_every(CHECKED_EVERY);
            shared
                .server
                .stepped(NetTime(0), &TickInput::default(), start);
            // The start is sent as any stepped snapshot is: a player joining
            // before the first step rebuilds it.
            if let Some(file) = &mut hashes {
                let _ = writeln!(
                    file,
                    "{} {} {}",
                    shared.server.epoch(),
                    start.tick,
                    start.hash()
                );
            }
            shared.host_end.local()
        };
        let mut client = Client::connect(
            credentials,
            server_peer,
            1,
            NetTime(0),
            ClientConfig {
                link: Config::loopback(),
                audit: false,
                key: join_key,
                rebuild_beside: true,
                ..ClientConfig::default()
            },
        )
        .map_err(|error| format!("the local client: {error}"))?;
        client.in_process();
        Ok(Solo {
            hosted,
            client,
            client_end,
            level: level.clone(),
            level_owed: false,
            angles: [0; 3],
            net,
            address,
            hashes,
            players: BTreeMap::new(),
            feel: Feel::default(),
            late_resumes: 0,
            notices: Vec::new(),
            screens: Vec::new(),
            sounds: Vec::new(),
            explosions: Vec::new(),
            recv_buffer,
        })
    }

    /// Wakes the net thread (if hosting) so newly queued outbound packets
    /// leave without waiting for the sleep interval.
    fn wake(&self) {
        if let Some(net) = &self.net {
            net.wake();
        }
    }

    pub(crate) fn shared_hosted(&self) -> Shared<Hosted> {
        self.hosted.clone()
    }

    pub(crate) fn net_waker(&self) -> Option<net::io::Waker> {
        self.net.as_ref().map(NetThread::waker)
    }

    /// The address players join, when hosting.
    pub fn address(&self) -> Option<SocketAddr> {
        self.address
    }

    pub fn now(&self) -> NetTime {
        now()
    }

    /// Moves every datagram between the server and its own client until
    /// none is waiting (the net thread moves the socket's), answering the
    /// client's asks for its level, and resumes the world after an install
    /// once its players loaded.
    pub fn pump(&mut self) {
        let _span = bevy::log::info_span!("net.pump").entered();
        let now = self.now();
        let hosting = self.net.is_some();
        let mut hosted = lock(&self.hosted);
        hosted.server.can_step(now);
        for _ in 0..PUMP_ROUNDS {
            let mut moved = false;
            while let Some((to, bytes)) = self.client.poll_transmit(now) {
                self.client_end.send(to, &bytes);
                moved = true;
            }
            moved |= if hosting {
                hosted.route_local(now)
            } else {
                hosted.route(now)
            };
            while let Some(datagram) = hosted.host_end.recv_at(now) {
                hosted
                    .server
                    .receive(datagram.at, datagram.from, &datagram.bytes);
                moved = true;
            }
            while let Some(datagram) = self.client_end.recv_at(now) {
                self.client
                    .receive(datagram.at, datagram.from, &datagram.bytes);
                moved = true;
            }
            while let Some(event) = self.client.poll_event() {
                match event {
                    ClientEvent::NeedLevel(_) => {
                        self.client.provide_level(now, Ok(self.level.clone()));
                        moved = true;
                    }
                    ClientEvent::Ended(why) => {
                        bevy::log::error!(target: "net", "the local client left: {why}");
                    }
                    ClientEvent::Notice(text) => self.notices.push(text),
                    ClientEvent::Screen(screen) => self.screens.push(screen),
                    ClientEvent::Sound(sound) => self.sounds.push(sound),
                    ClientEvent::Explosion(explosion) => self.explosions.push(explosion),
                    _ => {}
                }
            }
            if self.level_owed && self.client.awaits_level() {
                self.level_owed = false;
                self.client.provide_level(now, Ok(self.level.clone()));
                moved = true;
            }
            while let Some(event) = hosted.server.poll_event() {
                match event {
                    ServerEvent::Asked { peer } => {
                        bevy::log::info!(target: "net", "a player at {peer} asks to join");
                    }
                    ServerEvent::Refused { peer, refusal } => {
                        bevy::log::warn!(target: "net", "refused {peer:?}: {refusal}");
                    }
                    // The host's own player took the first seat, which its
                    // player left: its client follows at once.
                    ServerEvent::Moved { peer, from, to } if peer == hosted.host_end.remote() => {
                        bevy::log::info!(target: "net", ?from, ?to, "the host plays the main character");
                        self.client.reseat(to);
                    }
                    ServerEvent::Resumed { tick, late } if !late.is_empty() => {
                        self.late_resumes += 1;
                        bevy::log::warn!(
                            target: "net",
                            "the world resumes at tick {tick} without {} player(s) still loading: {late:?}",
                            late.len()
                        );
                    }
                    event => bevy::log::info!(target: "net", "{event:?}"),
                }
            }
            if !moved {
                break;
            }
        }
    }

    /// Whether the client plays (joined, loaded, no install stopping it):
    /// the host may step.
    pub fn playing(&self) -> bool {
        self.client.next_tick().is_some() && self.client.running()
    }

    /// The world holds (a movie up, a break) or goes on: the players stop
    /// predicting with it and start again after (`Server::hold`, `release`;
    /// each does nothing when already so, or while an install holds them).
    /// Called every frame.
    pub fn hold(&mut self, held: bool) {
        let now = self.now();
        {
            let mut hosted = lock(&self.hosted);
            if held {
                hosted.server.hold();
            } else {
                hosted.server.release(now);
            }
        }
        self.wake();
    }

    /// Whether a resume waits for the next tick: while a break holds, the
    /// host steps for it (its caller's or the host's ends the break,
    /// another's is told no, `Notice::NotYourBreak`). A leader or solo mode
    /// change from the first seat counts too (`sim::party::passes_break`):
    /// the host steps one tick for it, and the break holds again after it.
    pub fn resume_waiting(&self) -> bool {
        lock(&self.hosted)
            .server
            .waiting()
            .iter()
            .any(|(seat, action)| {
                *action == Action::Resume || sim::party::passes_break(*seat, action)
            })
    }

    /// Whether any player's action waits for the next tick (an item
    /// equipped, a purchase): a menu's hold lets the world step for it.
    pub fn action_waiting(&self) -> bool {
        !lock(&self.hosted).server.waiting().is_empty()
    }

    /// The client makes `command` its command for `tick` (none: it keeps
    /// looking where it looked and presses nothing), predicts the tick and
    /// sends it to the host.
    pub fn command(&mut self, tick: u64, command: Option<Command>) {
        let command = command.unwrap_or(Command {
            angles: self.angles,
            ..Command::default()
        });
        self.angles = command.angles;
        let now = self.now();
        {
            let _span = bevy::log::info_span!("net.predict").entered();
            let _ = self.client.frame_to(now, tick, command);
        }
        self.pump();
    }

    /// The commands the server has for `tick`, by seat, with the players'
    /// actions that came since the last tick. A tick the break `paused_by`
    /// holds after (no resume from its caller or the host) takes only the
    /// resumes and the first seat's leader and solo mode changes
    /// (`sim::party::passes_break`): the rest wait for the tick that ends it.
    pub fn commands_for(&mut self, tick: u64, paused_by: Option<Seat>) -> TickInput {
        lock(&self.hosted).commands_for(tick, paused_by)
    }

    /// The ticks `inputs` were made for were not stepped: a tick before
    /// them holds the world. Their actions wait for the next tick again,
    /// as if `commands_for` had not taken them. Their commands are
    /// dropped: the hold that follows drops every player's
    /// (`Server::hold`, `Server::installing`), and the tick is given a new
    /// command when it is due again.
    pub fn unstepped(&mut self, inputs: Vec<TickInput>) {
        let actions = inputs
            .iter()
            .flat_map(|input| input.actions().iter().cloned())
            .collect();
        lock(&self.hosted).server.put_back(actions);
    }

    /// This player's action, to the host as any player's goes.
    pub fn act(&mut self, action: Action) {
        self.client.act(action);
        self.pump();
    }

    /// Tells `seat`'s player what became of its action.
    pub fn notice(&mut self, seat: Seat, text: &str) {
        lock(&self.hosted).server.notice(seat, text);
        self.pump();
        self.wake();
    }

    /// Opens a screen a script asked for on `seat`'s player's side.
    pub fn screen(&mut self, seat: Seat, screen: sim::notice::Screen) {
        lock(&self.hosted).server.screen(seat, screen);
        self.pump();
        self.wake();
    }

    /// Plays a sound a script played on `seat`'s player's side.
    pub fn sound(&mut self, seat: Seat, sound: sim::notice::Sound) {
        lock(&self.hosted).server.sound(seat, sound);
        self.pump();
        self.wake();
    }

    /// Shows a mine that went off on `seat`'s player's side.
    pub fn explosion(&mut self, seat: Seat, explosion: sim::notice::Explosion) {
        lock(&self.hosted).server.explosion(seat, explosion);
        self.pump();
        self.wake();
    }

    /// Sends a save copy (`saves::copies::pack`) to every player but this
    /// one (I30 P17); the seats it goes to, and those left out.
    pub fn save_copy(
        &mut self,
        copy: &[u8],
    ) -> Result<net::session::CopySent, net::session::CopyRefusal> {
        let sent = {
            let mut hosted = lock(&self.hosted);
            let own = hosted.host_end.remote();
            hosted.server.save_copy(copy, Some(own))
        };
        self.pump();
        self.wake();
        sent
    }

    /// The name this host plays under: whose its players' copies are.
    pub fn name(&self) -> String {
        lock(&self.hosted).server.name().to_string()
    }

    /// What the host told this player since the last call.
    pub fn take_notices(&mut self) -> Vec<String> {
        std::mem::take(&mut self.notices)
    }

    /// The screens the host opened for this player since the last call.
    pub fn take_screens(&mut self) -> Vec<sim::notice::Screen> {
        std::mem::take(&mut self.screens)
    }

    /// The sounds the host played for this player since the last call.
    pub fn take_sounds(&mut self) -> Vec<sim::notice::Sound> {
        std::mem::take(&mut self.sounds)
    }

    /// The mines that went off for this player since the last call.
    pub fn take_explosions(&mut self) -> Vec<sim::notice::Explosion> {
        std::mem::take(&mut self.explosions)
    }

    /// The seat this machine's own player sits in, once welcomed.
    pub fn seat(&self) -> Option<Seat> {
        self.client.seat()
    }

    /// Whether a client occupies `seat` (the rest are the host's to drive).
    pub fn occupied(&self, seat: Seat) -> bool {
        lock(&self.hosted).server.occupied(seat)
    }

    /// The host stepped `snapshot` with `input`: the server keeps it, and
    /// the client rebuilds it when its frame is due (`CHECKED_EVERY`).
    pub fn stepped(&mut self, input: &TickInput, snapshot: &Arc<Snapshot>) {
        let now = self.now();
        self.stepped_at(input, snapshot, now);
    }

    /// `stepped` stamped with `at`, when the tick actually stepped (for
    /// batched steps that land a frame later).
    pub fn stepped_at(&mut self, input: &TickInput, snapshot: &Arc<Snapshot>, at: NetTime) {
        lock(&self.hosted)
            .server
            .stepped_shared(at, input, snapshot.clone());
        self.log_hash(snapshot);
        self.pump();
        self.wake();
    }

    /// `stepped` with the snapshot hashed already, and split when a frame
    /// can be made of it (on the step thread, when hosting): the frame does
    /// neither (issue 229). A solo host's checked tick comes split beside
    /// the step thread and not hashed (`checks_on`).
    pub fn stepped_hashed(
        &mut self,
        input: &TickInput,
        snapshot: &Arc<Snapshot>,
        hash: Option<sim::SnapshotHash>,
        split: Option<net::codec::Stepped>,
    ) {
        let now = self.now();
        self.stepped_hashed_at(input, snapshot, hash, split, now);
    }

    /// `stepped_hashed` stamped with `at`, when the step thread stepped the tick.
    pub fn stepped_hashed_at(
        &mut self,
        input: &TickInput,
        snapshot: &Arc<Snapshot>,
        hash: Option<sim::SnapshotHash>,
        split: Option<net::codec::Stepped>,
        at: NetTime,
    ) {
        let tick = snapshot.tick;
        {
            let mut hosted = lock(&self.hosted);
            match split {
                Some(split) => hosted.server.stepped_split(at, input, split),
                None => hosted.server.stepped_shared(at, input, snapshot.clone()),
            }
        }
        if let Some(file) = &mut self.hashes {
            let epoch = lock(&self.hosted).server.epoch();
            let hash = hash.unwrap_or_else(|| snapshot.hash());
            let _ = writeln!(file, "{epoch} {tick} {hash}");
        }
        self.pump();
        self.wake();
    }

    /// Completes main-thread bookkeeping after the step thread published
    /// `snapshot` directly into `Hosted::server` and woke `net_thread`.
    pub fn published_stepped(&mut self, snapshot: &Arc<Snapshot>, hash: Option<sim::SnapshotHash>) {
        if let Some(file) = &mut self.hashes {
            let epoch = lock(&self.hosted).server.epoch();
            let hash = hash.unwrap_or_else(|| snapshot.hash());
            let _ = writeln!(file, "{epoch} {} {hash}", snapshot.tick);
        }
        self.pump();
    }

    /// Whether this is a host players join over the network: its steps
    /// are hashed for `hashes.txt`, and split for them, beside the frame
    /// (`stepped_hashed`).
    pub fn hosting(&self) -> bool {
        self.net.is_some()
    }

    /// Whether a frame can be made of `tick` (`Server::sends_on`).
    pub fn sends_on(&self, tick: u64) -> bool {
        lock(&self.hosted).server.sends_on(tick)
    }

    /// Whether the host's own client checks `tick`, which it does every
    /// `CHECKED_EVERY` ticks (`Server::sends_own_on`). The step thread can
    /// have such a tick split on a thread of its own
    /// (`net::codec::Stepped::beside`). A solo client rebuilds it on
    /// another thread (`ClientConfig::rebuild_beside`). So the check costs
    /// the frame almost nothing.
    pub fn checks_on(&self, tick: u64) -> bool {
        lock(&self.hosted).server.sends_own_on(tick)
    }

    /// A module starts to load: the client stops with the host. Actions
    /// held for a break's end name the old module's objects: dropped.
    pub fn installing(&mut self, request: &session::Request) {
        self.level_owed = false;
        {
            let mut hosted = lock(&self.hosted);
            hosted.after_break.clear();
            hosted.server.installing(InstallRequest {
                game: request.game.clone(),
                module: request.module.clone(),
                arrival: request.arrival.clone().unwrap_or(String::new()),
                seats: request.seats,
                players: request.players,
            });
        }
        self.pump();
        self.wake();
    }

    /// The host committed `level`'s install (`after`): the client commits
    /// the same level itself, and once it has loaded the world resumes.
    /// The client is given the level once it has heard of the install,
    /// in this pump or a later one (`level_owed`).
    pub fn installed(&mut self, announcement: Announcement, after: &Snapshot, level: &Level) {
        let now = self.now();
        self.level = level.clone();
        lock(&self.hosted)
            .server
            .installed(now, announcement, after);
        self.log_hash(after);
        self.level_owed = true;
        self.pump();
        // Loaded here; `pump` resumes once the other players are too.
        self.pump();
        self.wake();
    }

    /// Writes a stepped snapshot's epoch, tick and hash, when hosting.
    fn log_hash(&mut self, snapshot: &Snapshot) {
        if let Some(file) = &mut self.hashes {
            let epoch = lock(&self.hosted).server.epoch();
            let _ = writeln!(file, "{epoch} {} {}", snapshot.tick, snapshot.hash());
        }
    }

    /// The run ends: the net thread stops, every player is told the host
    /// quit, straight through the socket (the network's conditions would
    /// hold the packets past the process's end); the hashes' log is flushed
    /// and what the session did goes to `folder/net.json`.
    pub fn shutdown(&mut self, folder: &Path) {
        let mut socket = self.net.take().and_then(NetThread::stop);
        let now = self.now();
        {
            let mut hosted = lock(&self.hosted);
            hosted.server.shutdown(now);
            hosted.route(now);
            for (to, bytes) in hosted.waiting.drain(..) {
                if let Some(socket) = &mut socket {
                    socket.inner_mut().send(to, &bytes);
                }
            }
        }
        if let Some(file) = &mut self.hashes {
            let _ = file.flush();
        }
        let mut report = self.facts();
        report["feel"] = self.feel.report();
        write_json(&folder.join("net.json"), &report);
    }

    /// Live network facts + `feel` samples for bug reports (`report::make`)
    /// before the run exits and writes `net.json` to disk.
    pub fn live_net_json(&mut self) -> serde_json::Value {
        let mut report = self.facts();
        report["feel"] = self.feel.report();
        report
    }

    /// Non-blocking summary of connected remote peers for `net_graph`.
    pub(crate) fn graph_sample(&self) -> Option<crate::net_graph::NetGraphSample> {
        let hosted = self.hosted.try_lock().ok()?;
        let server = hosted.server.stats();
        let mut sample = crate::net_graph::NetGraphSample {
            path: if self.net.is_some() {
                "UDP Direct"
            } else {
                "Loopback"
            },
            ..Default::default()
        };
        let mut any_steam = false;
        let mut any_remote = false;
        for c in server
            .connections
            .iter()
            .filter(|c| !matches!(c.peer, net::Peer::Local(_)))
        {
            any_remote = true;
            any_steam |= matches!(c.peer, net::Peer::Steam(_));
            sample.rtt_ms = sample.rtt_ms.max(c.link.rtt_us as f64 / 1000.0);
            sample.jitter_ms = sample.jitter_ms.max(c.link.jitter_us as f64 / 1000.0);
            sample.loss_pct = sample.loss_pct.max(c.link.loss_ppm as f64 / 10_000.0);
            sample.bytes_down += c.link.bytes_received;
            sample.bytes_up += c.link.bytes_sent;
            sample.snaps += c.stats.snapshots;
        }
        if any_steam {
            sample.path = "Steam P2P";
        } else if !any_remote {
            sample.path = if self.net.is_some() {
                "UDP Direct (listening)"
            } else {
                "Local"
            };
        }
        Some(sample)
    }

    /// A frame passed: the net feel's sample, when one is due and players
    /// play over the network.
    pub fn sample(&mut self) {
        if self.feel.frame() && self.net.is_some() {
            let server = lock(&self.hosted).server.stats();
            net_feel::sample_host(&mut self.feel, &server);
        }
    }

    /// The module's plan failed: the world goes on where it stopped.
    pub fn abandon(&mut self) {
        self.level_owed = false;
        let now = self.now();
        lock(&self.hosted).server.abandon_install(now);
        self.pump();
        self.wake();
    }

    /// What the session did, for dumps.
    pub fn facts(&mut self) -> serde_json::Value {
        self.client.settle();
        self.note_players();
        let stats = self.client.stats();
        let link = self.client.link_stats();
        serde_json::json!({
            "path": "loopback",
            "playing": self.playing(),
            "rebuilt": stats.rebuilt,
            "keyframes": stats.keyframes,
            "check_failures": stats.check_failures,
            "compared": stats.compared,
            "mismatches": stats.mismatches,
            "by_cause": Cause::ALL.iter()
                .map(|cause| (cause.name().to_string(), serde_json::json!(stats.by_cause[*cause as usize])))
                .collect::<serde_json::Map<_, _>>(),
            "installs_agreed": stats.installs_agreed,
            "installs_differed": stats.installs_differed,
            "bytes_up": link.map(|link| link.bytes_sent),
            "bytes_down": link.map(|link| link.bytes_received),
            "late_resumes": self.late_resumes,
            "listening": self.address().map(|address| address.to_string()),
            "recv_buffer": self.recv_buffer,
            "players": self.player_rows(),
        })
    }

    pub fn note_players(&mut self) {
        let server = lock(&self.hosted).server.stats();
        for player in self.players.values_mut() {
            player.connected = false;
        }
        for c in server
            .connections
            .iter()
            .filter(|c| !matches!(c.peer, net::Peer::Local(_)))
        {
            self.players.insert(
                c.peer,
                Player {
                    connected: true,
                    seat: c.seat,
                    loaded: c.loaded,
                    every: c.every,
                    link: c.link,
                    stats: c.stats,
                },
            );
        }
    }

    fn player_rows(&self) -> Vec<serde_json::Value> {
        let mut rows: Vec<(String, &Player)> = (self.players.iter())
            .map(|(peer, player)| (format!("{peer:?}"), player))
            .collect();
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        rows.into_iter()
            .map(|(peer, c)| {
                serde_json::json!({
                    "peer": peer,
                    "connected": c.connected,
                    "seat": c.seat.0,
                    "loaded": c.loaded,
                    "every": c.every,
                    "rtt_ms": c.link.rtt_us as f64 / 1000.0,
                    "loss_percent": c.link.loss_ppm as f64 / 10_000.0,
                    "packets_sent": c.link.packets_sent,
                    "declared_lost": c.link.declared_lost,
                    "late_acks": c.link.late_acks,
                    "bytes_up": c.link.bytes_received,
                    "bytes_down": c.link.bytes_sent,
                    "commands": c.stats.commands,
                    "late": c.stats.late,
                    "substituted": c.stats.substituted,
                    "keyframes": c.stats.keyframes,
                    "interval_changes": c.stats.interval_changes,
                })
            })
            .collect()
    }
}

#[derive(Clone, Copy)]
struct Player {
    connected: bool,
    seat: Seat,
    loaded: Option<u16>,
    every: u8,
    link: net::link::Stats,
    stats: net::session::server::ConnectionStats,
}

/// A reload whose install message reaches the host's own client only after
/// the host committed it (issue 643). The party fell and the lost box held
/// the world, so the install went out as a whole snapshot, which came in
/// over several pumps. The client must still get the level, load, and let
/// the world go on.
#[cfg(feature = "probes")]
pub fn reload_probes() -> sim::probe::Checks {
    vec![("reload_message_late", reload_message_late())]
}

#[cfg(feature = "probes")]
fn reload_message_late() -> Result<String, String> {
    use sim::{SimWorld, StepReason};

    /// Pumps given to the client to load before the world counts as stuck.
    const PUMPS: usize = 200;
    let arena = Level::arena();
    let ground = sim::determinism::proving_ground();
    let rate = TickRate::default();
    let mut world = SimWorld::new(sim::WorldConfig::one_seat(), arena.clone())
        .map_err(|error| error.to_string())?;
    let start = world.snapshot();
    let mut solo = Solo::new(
        &arena,
        LevelRequest::Stage(arena.id()),
        rate,
        &start,
        None,
        &std::env::temp_dir(),
    )?;
    let request = session::Request {
        game: "kjk".into(),
        module: sim::determinism::GROUND.into(),
        arrival: None,
        seats: 1,
        players: 1,
        collision: session::collision(),
    };
    let input = TickInput::default();
    // The host steps `ticks` ticks, then pumps until its client plays: how
    // many pumps that took, if it did.
    let run = |world: &mut SimWorld, solo: &mut Solo, ticks: u64| {
        for _ in 0..ticks {
            let tick = world.snapshot().tick + 1;
            let stepped = sim::step(world, tick, &input, rate.msec(tick), StepReason::Authority);
            solo.stepped(&input, &Arc::new(stepped));
        }
        (1..=PUMPS).find(|_| {
            solo.hold(false);
            solo.pump();
            solo.playing()
        })
    };
    let install = |world: &mut SimWorld| {
        let after = world
            .install(ground.clone(), None)
            .map_err(|error| error.to_string())?;
        let announcement =
            Announcement::new(&ground, None, &after).ok_or("the proving ground is not a module")?;
        Ok::<_, String>((after, announcement))
    };
    run(&mut world, &mut solo, 0).ok_or("the client never played")?;
    run(&mut world, &mut solo, 10).ok_or("the client stopped playing")?;
    solo.installing(&request);
    let (after, announcement) = install(&mut world)?;
    solo.installed(announcement, &after, &ground);
    run(&mut world, &mut solo, 0).ok_or("the client never loaded the first install")?;
    run(&mut world, &mut solo, 10).ok_or("the client stopped playing")?;
    // The reload: a menu holds the world, the host plans the install, and
    // its message to the client is held back until the host has committed.
    solo.hold(true);
    solo.pump();
    solo.level_owed = false;
    let now = solo.now();
    let late: Vec<net::Datagram> = {
        let mut hosted = lock(&solo.hosted);
        hosted.server.installing(InstallRequest {
            game: request.game.clone(),
            module: request.module.clone(),
            arrival: String::new(),
            seats: 1,
            players: 1,
        });
        hosted.route(now);
        drop(hosted);
        std::iter::from_fn(|| solo.client_end.recv_at(now)).collect()
    };
    let (after, announcement) = install(&mut world)?;
    solo.installed(announcement, &after, &ground);
    for datagram in &late {
        solo.client
            .receive(datagram.at, datagram.from, &datagram.bytes);
    }
    let pumps = run(&mut world, &mut solo, 0);
    sim::probe::check(
        !late.is_empty() && pumps.is_some(),
        format!(
            "{} datagrams of the install held back; the client played again after {pumps:?} pumps",
            late.len()
        ),
    )
}
