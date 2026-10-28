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
    Cause, Client, ClientConfig, ClientEvent, InstallRequest, LevelRequest, Server, ServerConfig,
    ServerEvent,
};
use net::{NetTime, Peer, Transport};
use sim::action::Action;
use sim::{Announcement, Command, Level, Seat, Snapshot, TickInput, TickRate};

use crate::net_feel::{self, Feel};
use crate::net_thread::{Endpoint, NetThread, Shared, lock};

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
}

impl Endpoint for Hosted {
    fn receive(&mut self, now: NetTime, from: Peer, bytes: &[u8]) {
        self.server.receive(now, from, bytes);
    }

    fn poll_transmit(&mut self, now: NetTime) -> Option<(Peer, Vec<u8>)> {
        if self.waiting.is_empty() {
            self.route(now);
        }
        self.waiting.pop_front()
    }

    fn next_deadline(&self) -> Option<NetTime> {
        if self.waiting.is_empty() {
            self.server.next_deadline()
        } else {
            Some(NetTime(0))
        }
    }
}

/// The host's server, its own client, and the loopback between them.
pub struct Solo {
    hosted: Shared<Hosted>,
    client: Client,
    client_end: Loopback,
    /// The level the client builds when asked (the host's own).
    level: Level,
    /// The angles of the last command made: a seat with nothing to press
    /// keeps looking there.
    angles: [i16; 3],
    /// When hosting: the net thread with the socket players join over, its
    /// address, and the stepped hashes' log.
    net: Option<NetThread>,
    address: Option<SocketAddr>,
    hashes: Option<std::io::BufWriter<std::fs::File>>,
    /// Every player who joined, by address: its last report (kept after
    /// it leaves, for `net.json`).
    players: BTreeMap<String, serde_json::Value>,
    /// The net feel of each player over the network (D16), and how often
    /// the world resumed without a player still loading.
    feel: Feel,
    late_resumes: u64,
    /// What the host told this player of its actions, not yet shown.
    notices: Vec<String>,
    screens: Vec<sim::notice::Screen>,
    sounds: Vec<sim::notice::Sound>,
    /// The socket's receive buffer, as the OS gave it.
    recv_buffer: Option<usize>,
    /// The players' actions held back while a break holds, for the tick
    /// that ends it (`commands_for`).
    after_break: Vec<(Seat, Action)>,
}

/// What a host listens with: its socket, the build and data every player
/// must match, and the join password's key, if it has one.
pub struct Listen {
    pub socket: Netem<Socket>,
    pub credentials: Credentials,
    pub join_key: Option<JoinKey>,
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
        let (host_end, client_end) = Loopback::pair(clock());
        let (socket, credentials, join_key) = match listen {
            Some(listen) => (Some(listen.socket), listen.credentials, listen.join_key),
            None => (None, credentials("local", Vec::new()), None),
        };
        let hosting = socket.is_some();
        let mut server = Server::new(
            ServerConfig {
                seats: start.seats(),
                rate,
                every: 2,
                // Pacing is for a network; the loopback peer is local.
                link: if hosting {
                    Config::default()
                } else {
                    Config::loopback()
                },
                load_wait_us: 30_000_000,
                secret: if hosting { random_key() } else { [0x5a; 32] },
                join_key,
                credentials: credentials.clone(),
            },
            request,
        );
        // The host's own player is in this process: a long frame (a
        // pipeline warm-up) stalls both sides, so neither times the other
        // out.
        server.in_process(host_end.remote());
        server.set_in_process_every(CHECKED_EVERY);
        let mut hashes = hosting
            .then(|| std::fs::File::create(folder.join("hashes.txt")).ok())
            .flatten()
            .map(std::io::BufWriter::new);
        server.stepped(NetTime(0), &TickInput::default(), start);
        // The start is sent as any stepped snapshot is: a player joining
        // before the first step rebuilds it.
        if let Some(file) = &mut hashes {
            let _ = writeln!(file, "{} {} {}", server.epoch(), start.tick, start.hash());
        }
        let mut client = Client::connect(
            credentials,
            host_end.local(),
            1,
            NetTime(0),
            ClientConfig {
                link: Config::loopback(),
                audit: false,
                key: join_key,
                ..ClientConfig::default()
            },
        )
        .map_err(|error| format!("the local client: {error}"))?;
        client.in_process();
        let hosted = Arc::new(Mutex::new(Hosted {
            server,
            host_end,
            waiting: VecDeque::new(),
        }));
        let address = socket
            .as_ref()
            .and_then(|socket| socket.inner().udp_end()?.address());
        let recv_buffer = socket
            .as_ref()
            .and_then(|socket| socket.inner().recv_buffer());
        let net = socket
            .map(|socket| NetThread::spawn(socket, hosted.clone(), clock()))
            .transpose()
            .map_err(|error| format!("the net thread: {error}"))?;
        Ok(Solo {
            hosted,
            client,
            client_end,
            level: level.clone(),
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
            recv_buffer,
            after_break: Vec::new(),
        })
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
        let mut hosted = lock(&self.hosted);
        hosted.server.can_step(now);
        for _ in 0..PUMP_ROUNDS {
            let mut moved = false;
            while let Some((to, bytes)) = self.client.poll_transmit(now) {
                self.client_end.send(to, &bytes);
                moved = true;
            }
            moved |= hosted.route(now);
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
                    _ => {}
                }
            }
            while let Some(event) = hosted.server.poll_event() {
                match event {
                    ServerEvent::Refused { peer, refusal } => {
                        bevy::log::warn!(target: "net", "refused {peer:?}: {refusal}");
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
        let mut hosted = lock(&self.hosted);
        if held {
            hosted.server.hold();
        } else {
            hosted.server.release(now);
        }
    }

    /// Whether a resume waits for the next tick: while a break holds, the
    /// host steps for it (its caller's or the host's ends the break,
    /// another's is told no, `Notice::NotYourBreak`).
    pub fn resume_waiting(&self) -> bool {
        lock(&self.hosted)
            .server
            .waiting()
            .iter()
            .any(|(_, action)| *action == Action::Resume)
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
    /// resumes: the rest wait for the tick that ends it.
    pub fn commands_for(&mut self, tick: u64, paused_by: Option<Seat>) -> TickInput {
        let mut hosted = lock(&self.hosted);
        let mut input = hosted.server.commands_for(tick);
        let mut actions = std::mem::take(&mut self.after_break);
        actions.extend(hosted.server.actions());
        let ends = paused_by.is_none_or(|by| {
            actions.iter().any(|(seat, action)| {
                *action == Action::Resume && sim::party::may_resume(by, *seat)
            })
        });
        for (seat, action) in actions {
            if ends || action == Action::Resume {
                input.act(seat, action);
            } else {
                self.after_break.push((seat, action));
            }
        }
        input
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
    }

    /// Opens a screen a script asked for on `seat`'s player's side.
    pub fn screen(&mut self, seat: Seat, screen: sim::notice::Screen) {
        lock(&self.hosted).server.screen(seat, screen);
        self.pump();
    }

    /// Plays a sound a script played on `seat`'s player's side.
    pub fn sound(&mut self, seat: Seat, sound: sim::notice::Sound) {
        lock(&self.hosted).server.sound(seat, sound);
        self.pump();
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

    /// Whether a client occupies `seat` (the rest are the host's to drive).
    pub fn occupied(&self, seat: Seat) -> bool {
        lock(&self.hosted).server.occupied(seat)
    }

    /// The host stepped `snapshot` with `input`: the server keeps it, and
    /// the client rebuilds it when its frame is due (`CHECKED_EVERY`).
    pub fn stepped(&mut self, input: &TickInput, snapshot: &Arc<Snapshot>) {
        let now = self.now();
        lock(&self.hosted)
            .server
            .stepped_shared(now, input, snapshot.clone());
        self.log_hash(snapshot);
        self.pump();
    }

    /// A module starts to load: the client stops with the host. Actions
    /// held for a break's end name the old module's objects: dropped.
    pub fn installing(&mut self, request: &session::Request) {
        self.after_break.clear();
        lock(&self.hosted).server.installing(InstallRequest {
            game: request.game.clone(),
            module: request.module.clone(),
            arrival: request.arrival.clone().unwrap_or(String::new()),
            seats: request.seats,
            players: request.players,
        });
        self.pump();
    }

    /// The host committed `level`'s install (`after`): the client commits
    /// the same level itself, and once it has loaded the world resumes.
    pub fn installed(&mut self, announcement: Announcement, after: &Snapshot, level: &Level) {
        let now = self.now();
        self.level = level.clone();
        lock(&self.hosted)
            .server
            .installed(now, announcement, after);
        self.log_hash(after);
        self.pump();
        self.client.provide_level(now, Ok(level.clone()));
        // Loaded here; `pump` resumes once the other players are too.
        self.pump();
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
        let now = self.now();
        lock(&self.hosted).server.abandon_install(now);
        self.pump();
    }

    /// What the session did, for dumps.
    pub fn facts(&mut self) -> serde_json::Value {
        let stats = self.client.stats();
        let link = self.client.link_stats();
        let server = lock(&self.hosted).server.stats();
        for row in self.players.values_mut() {
            row["connected"] = serde_json::json!(false);
        }
        for c in server
            .connections
            .iter()
            .filter(|c| matches!(c.peer, net::Peer::Addr(_)))
        {
            let peer = format!("{:?}", c.peer);
            let row = serde_json::json!({
                "peer": peer,
                "connected": true,
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
            });
            self.players.insert(peer, row);
        }
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
            "players": self.players.values().cloned().collect::<Vec<_>>(),
        })
    }
}
