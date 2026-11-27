//! The co-op lobby on the wire (`kjk net probes`): each lobby message read
//! back as written, and a host's lobby with real clients joined to it by
//! hand, in virtual time.

use ::session::lobby::Pick;
use ::session::lobby::probes::usual;
use sim::{Level, Seat, SimWorld, TickInput, TickRate};

use super::client::{Client, ClientConfig, ClientEvent};
use super::msg::{LevelRequest, Message};
use super::server::{Server, ServerConfig, ServerEvent};
use crate::handshake::{self, Credentials, Refusal};
use crate::link::Config;
use crate::probes::Probes;
use crate::{NetTime, Peer};

const HOST: Peer = Peer::Local(0);
const BUILD: &str = "lobby-probe";
/// A step of virtual time between pumps: 10 ms.
const STEP_US: u64 = 10_000;
/// The reason the host's game gives when its player leaves the lobby.
const CLOSED: &str = "The host closed the lobby.";

fn credentials(name: &str, build: &str) -> Credentials {
    Credentials {
        build: build.to_string(),
        games: Vec::new(),
        name: name.to_string(),
        reconnect: None,
    }
}

fn config(seats: u8) -> ServerConfig {
    ServerConfig {
        seats,
        rate: TickRate::default(),
        every: 2,
        link: Config::default(),
        load_wait_us: 1_000_000,
        secret: [7; 32],
        join_key: None,
        credentials: credentials("host", BUILD),
        stances: Vec::new(),
        collision: 1,
        quorum_percent: 100,
    }
}

/// The host's own player, in the host's process. Its seat is slot 0, so
/// guests take slots 1 and 2.
const HOST_PLAYER: Peer = Peer::Local(99);

/// A lobby server of three slots, the first kept for the host's player.
fn lobby_server() -> Server {
    let mut server = Server::lobby(config(3));
    server.in_process(HOST_PLAYER);
    server
}

/// A host's lobby of three slots and the guests joined to it by hand.
struct Room {
    server: Option<Server>,
    guests: Vec<(Peer, Client)>,
    now: NetTime,
    /// Each guest's events so far, by its index.
    seen: Vec<Vec<ClientEvent>>,
    /// The host's events so far.
    events: Vec<ServerEvent>,
    /// The guests, by index, whose datagrams are lost both ways: a lost
    /// connection.
    silent: Vec<usize>,
    /// The world the session began in, once it did.
    start: Option<sim::Snapshot>,
}

impl Room {
    fn new() -> Room {
        Room {
            server: Some(lobby_server()),
            guests: Vec::new(),
            now: NetTime(0),
            seen: Vec::new(),
            events: Vec::new(),
            silent: Vec::new(),
            start: None,
        }
    }

    /// A guest named `name` of build `build` starts joining.
    fn join(&mut self, name: &str, build: &str) -> usize {
        let at = Peer::Local(self.guests.len() as u32 + 1);
        self.join_from(at, credentials(name, build))
    }

    /// A player with `credentials` starts joining from `at`.
    fn join_from(&mut self, at: Peer, credentials: Credentials) -> usize {
        let mut client = Client::connect(
            credentials,
            HOST,
            u64::from(self.guests.len() as u32) + 11,
            self.now,
            ClientConfig::default(),
        )
        .unwrap_or_else(|error| panic!("a lobby probe's client: {error:?}"));
        if at == HOST_PLAYER {
            client.in_process();
        }
        self.guests.push((at, client));
        self.seen.push(Vec::new());
        self.guests.len() - 1
    }

    /// The host's own player joins from its peer in the host's process.
    fn host_joins(&mut self) -> usize {
        self.join_from(HOST_PLAYER, credentials("host", BUILD))
    }

    /// Guest `index` comes back from a new address with the reconnect
    /// token its welcome gave it.
    fn rejoin(&mut self, index: usize) -> usize {
        let token = self.welcome(index).map(|welcome| welcome.token);
        let at = Peer::Local(self.guests.len() as u32 + 1);
        let credentials = Credentials {
            reconnect: token,
            ..credentials("again", BUILD)
        };
        self.join_from(at, credentials)
    }

    /// The last welcome guest `index` was given.
    fn welcome(&self, index: usize) -> Option<super::msg::Welcome> {
        self.seen[index].iter().rev().find_map(|event| match event {
            ClientEvent::Welcome(welcome) => Some(*welcome),
            _ => None,
        })
    }

    /// The seat guest `index` was last welcomed into.
    fn seat(&self, index: usize) -> Option<u8> {
        self.welcome(index).map(|welcome| welcome.seat.0)
    }

    /// The host's own player's moves between seats so far, as (from, to).
    fn moves(&self) -> Vec<(u8, u8)> {
        (self.events.iter())
            .filter_map(|event| match event {
                ServerEvent::Moved { peer, from, to } if *peer == HOST_PLAYER => {
                    Some((from.0, to.0))
                }
                _ => None,
            })
            .collect()
    }

    /// Moves datagrams both ways for `steps` steps of virtual time. Without
    /// a server, what the guests send is lost.
    fn pump(&mut self, steps: usize) {
        for _ in 0..steps {
            self.now = self.now.plus(STEP_US);
            let now = self.now;
            for (index, (at, client)) in self.guests.iter_mut().enumerate() {
                while let Some((_, bytes)) = client.poll_transmit(now) {
                    if let Some(server) = self
                        .server
                        .as_mut()
                        .filter(|_| !self.silent.contains(&index))
                    {
                        server.receive(now, *at, &bytes);
                    }
                }
                while let Some(event) = client.poll_event() {
                    self.seen[index].push(event);
                }
            }
            let Some(server) = &mut self.server else {
                continue;
            };
            while let Some((to, bytes)) = server.poll_transmit(now) {
                let silent = &self.silent;
                if let Some((_, client)) = (self.guests.iter_mut().enumerate())
                    .find(|(index, (at, _))| *at == to && !silent.contains(index))
                    .map(|(_, guest)| guest)
                {
                    client.receive(now, HOST, &bytes);
                }
            }
            while let Some(event) = server.poll_event() {
                self.events.push(event);
            }
        }
    }

    fn server(&mut self) -> &mut Server {
        self.server
            .as_mut()
            .unwrap_or_else(|| panic!("the lobby probe's server"))
    }

    /// The last lobby state guest `index` was sent, with its slot.
    fn last_lobby(&self, index: usize) -> Option<(u8, ::session::lobby::Lobby)> {
        self.seen[index].iter().rev().find_map(|event| match event {
            ClientEvent::LobbyState { slot, lobby } => Some((*slot, (**lobby).clone())),
            _ => None,
        })
    }

    /// Guest `index`'s events, each by a short name, in order.
    fn names(&self, index: usize) -> Vec<&'static str> {
        (self.seen[index].iter())
            .filter_map(|event| match event {
                ClientEvent::Accepted { .. } => Some("accepted"),
                ClientEvent::LobbyState { .. } => Some("lobby"),
                ClientEvent::Starting => Some("start"),
                ClientEvent::Welcome(_) => Some("welcome"),
                ClientEvent::Closed(_) => Some("closed"),
                ClientEvent::Ended(_) => Some("ended"),
                _ => None,
            })
            .collect()
    }

    /// The host's world is loaded: the lobby ends in the arena, each slot
    /// in the seat of its number.
    fn begin(&mut self) {
        self.begin_with(&[]);
    }

    /// The host's world is loaded: the lobby ends in the arena, each slot
    /// in the seat `seats` gives it.
    fn begin_with(&mut self, seats: &[(usize, Seat)]) {
        let now = self.now;
        let mut world = SimWorld::new(sim::determinism::Setup::pinned().world, Level::arena())
            .unwrap_or_else(|error| panic!("the determinism world: {error}"));
        let snapshot = world.snapshot();
        let server = self.server();
        server.begin(
            now,
            config(3),
            LevelRequest::Stage(Level::arena().id()),
            seats,
        );
        server.stepped(now, &TickInput::default(), &snapshot);
        self.start = Some(snapshot);
    }

    /// The host installs the proving ground, as a module change or the
    /// reload after the party fell does: the session goes on with its
    /// connections.
    fn install(&mut self) -> Option<()> {
        let now = self.now;
        let ground = sim::determinism::proving_ground();
        let mut world = SimWorld::from_snapshot(self.start.as_ref()?, Level::arena()).ok()?;
        let after = world.install(ground.clone(), None).ok()?;
        let announcement = sim::Announcement::new(&ground, None, &after)?;
        let server = self.server();
        server.installing(super::msg::InstallRequest {
            game: "kjk".into(),
            module: sim::determinism::GROUND.into(),
            arrival: String::new(),
            seats: 3,
            players: 3,
        });
        server.installed(now, announcement, &after);
        Some(())
    }

    /// Each connection's seat on the host, by its peer.
    fn seats(&mut self) -> Vec<(Peer, u8)> {
        let mut seats: Vec<(Peer, u8)> = (self.server().stats().connections.iter())
            .map(|connection| (connection.peer, connection.seat.0))
            .collect();
        seats.sort();
        seats
    }
}

/// Each message read back as it was written.
fn round_trips(probes: &mut Probes) {
    let lobby = Box::new(usual(2));
    let messages = [
        ("lobby", Message::Lobby { slot: 2, lobby }),
        (
            "choose a companion",
            Message::Choose {
                pick: Pick::Companion(7),
                swap: true,
            },
        ),
        (
            "choose the main character",
            Message::Choose {
                pick: Pick::Hero,
                swap: true,
            },
        ),
        ("ready", Message::Ready(true)),
        ("not ready", Message::Ready(false)),
        ("start", Message::Start),
        ("closed", Message::Closed(CLOSED.to_string())),
    ];
    for (name, message) in messages {
        let bytes = message.to_bytes();
        let back = Message::decode(&bytes);
        probes.check(
            &format!("lobby wire: {name}"),
            "the message reads back as it was written",
            back.as_ref() == Ok(&message),
            format!("{} bytes", bytes.len()),
        );
    }
}

/// Two guests join, choose and mark Ready, and the host starts the game.
fn gathering(probes: &mut Probes) {
    let mut room = Room::new();
    let (a, b) = (room.join("Mia", BUILD), room.join("Kalle", BUILD));
    room.pump(20);
    let joined: Vec<(usize, String)> = (room.events.iter())
        .filter_map(|event| match event {
            ServerEvent::LobbyJoined { slot, name } => Some((*slot, name.clone())),
            _ => None,
        })
        .collect();
    let lobby = usual(2);
    room.server().send_lobby(&lobby);
    room.pump(10);
    let sent = [room.last_lobby(a), room.last_lobby(b)];
    probes.check(
        "lobby: guests join into the lobby",
        "each guest takes a slot, receives the lobby with its own slot, and is not welcomed",
        joined.len() == 2
            && sent[0]
                .as_ref()
                .is_some_and(|(slot, got)| *slot == 1 && *got == lobby)
            && sent[1].as_ref().is_some_and(|(slot, _)| *slot == 2)
            && room.names(a) == ["accepted", "lobby"],
        format!("joined {joined:?}; the first guest saw {:?}", room.names(a)),
    );

    // Both guests ask for one companion in the same step, then mark Ready.
    room.guests[a].1.choose(Pick::Companion(2), false);
    room.guests[b].1.choose(Pick::Companion(2), false);
    room.guests[b].1.ready(true);
    let before = room.events.len();
    room.pump(10);
    let asked: Vec<&ServerEvent> = (room.events[before..].iter())
        .filter(|event| {
            matches!(
                event,
                ServerEvent::LobbyChoose { .. } | ServerEvent::LobbyReady { .. }
            )
        })
        .collect();
    probes.check(
        "lobby: requests reach the host in order",
        "two Choose requests for one companion and a Ready mark reach the host as events, in the order sent",
        asked
            == [
                &ServerEvent::LobbyChoose {
                    slot: 1,
                    pick: Pick::Companion(2),
                    swap: false,
                },
                &ServerEvent::LobbyChoose {
                    slot: 2,
                    pick: Pick::Companion(2),
                    swap: false,
                },
                &ServerEvent::LobbyReady { slot: 2, on: true },
            ],
        format!("{asked:?}"),
    );

    // The host applies the requests in the order they came, as its game
    // does, and sends the lobby again.
    let mut answered = lobby.clone();
    for event in &asked {
        match **event {
            ServerEvent::LobbyChoose { slot, pick, swap } => {
                answered.choose(slot, pick, swap);
            }
            ServerEvent::LobbyReady { slot, on } => answered.ready(slot, on),
            _ => {}
        }
    }
    room.server().send_lobby(&answered);
    room.pump(10);
    let seen = [room.last_lobby(a), room.last_lobby(b)];
    let playing = |seen: &Option<(u8, ::session::lobby::Lobby)>| {
        seen.as_ref()
            .map(|(slot, lobby)| lobby.playing(usize::from(*slot)))
    };
    probes.check(
        "lobby: a guest sees its own slot change",
        "the first guest to ask gets the companion and sees it in its own slot; the second keeps its companion; both see the same lobby",
        playing(&seen[0]) == Some(Some(::session::lobby::Playing::Companion(2)))
            && playing(&seen[1]) == Some(lobby.playing(2))
            && seen[0].as_ref().map(|(_, lobby)| lobby) == seen[1].as_ref().map(|(_, lobby)| lobby),
        format!("{:?} and {:?}", playing(&seen[0]), playing(&seen[1])),
    );

    room.server().start();
    room.pump(10);
    room.begin();
    room.pump(20);
    let order = room.names(a);
    let start = order.iter().position(|name| *name == "start");
    let welcome = order.iter().position(|name| *name == "welcome");
    probes.check(
        "lobby: Start comes before the welcome",
        "after Start a guest is welcomed into its seat once the host's world is loaded",
        start.is_some() && welcome.is_some() && start < welcome && !room.server().in_lobby(),
        format!("{order:?}"),
    );
}

/// A guest who leaves the lobby frees the slot, and a guest who leaves as
/// the host presses Start gets no seat.
fn leaving(probes: &mut Probes) {
    let mut room = Room::new();
    let (a, b) = (room.join("Mia", BUILD), room.join("Kalle", BUILD));
    room.pump(20);
    let now = room.now;
    room.guests[b].1.leave(now, Refusal::Left);
    room.pump(10);
    let left = room.events.contains(&ServerEvent::LobbyLeft { slot: 2 });
    probes.check(
        "lobby: a guest who leaves frees the slot",
        "the host is told that slot 2 left",
        left,
        format!("{:?}", room.events),
    );

    let now = room.now;
    room.guests[a].1.leave(now, Refusal::Left);
    room.server().start();
    room.pump(10);
    room.begin();
    room.pump(10);
    let seated = room.server().stats().connections.len();
    let joined = (room.events.iter()).any(|event| matches!(event, ServerEvent::Joined { .. }));
    probes.check(
        "lobby: a guest who leaves at Start gets no seat",
        "the session begins with no guest connection and no guest welcomed",
        seated == 0 && !joined,
        format!("{seated} connections"),
    );
}

/// The host closes the lobby: each guest is told why.
fn closing(probes: &mut Probes) {
    let mut room = Room::new();
    let a = room.join("Mia", BUILD);
    room.pump(20);
    let now = room.now;
    room.server().close_lobby(now, CLOSED);
    room.pump(10);
    let reason = room.seen[a].iter().find_map(|event| match event {
        ClientEvent::Closed(reason) => Some(reason.clone()),
        _ => None,
    });
    probes.check(
        "lobby: closing tells each guest why",
        "the guest receives Closed with the host's reason",
        reason.as_deref() == Some(CLOSED),
        format!("{:?}", room.names(a)),
    );
}

/// A guest of another build is refused as today, and a guest who asks
/// before the host's lobby exists enters it once the lobby opens.
fn arriving(probes: &mut Probes) {
    let mut room = Room::new();
    let old = room.join("Old", "an older build");
    room.pump(20);
    let refused = room.seen[old].iter().find_map(|event| match event {
        ClientEvent::Ended(why) => Some(why.clone()),
        _ => None,
    });
    probes.check(
        "lobby: another build is refused",
        "a guest on another build is refused with the build mismatch, before the lobby",
        matches!(refused, Some(Refusal::Build { .. })) && room.last_lobby(old).is_none(),
        format!("{refused:?}"),
    );

    let mut room = Room::new();
    room.server = None;
    let early = room.join("Early", BUILD);
    // Three seconds of retries with nobody answering: the host is still
    // checking its game data.
    room.pump(300);
    room.server = Some(lobby_server());
    room.pump(20);
    room.server().send_lobby(&usual(1));
    room.pump(10);
    probes.check(
        "lobby: a guest who asked early enters",
        "a guest whose handshake started before the lobby existed, within the handshake's retry window, enters the lobby",
        handshake::GIVE_UP_US > 3_000_000 && room.last_lobby(early).is_some(),
        format!("{:?}", room.names(early)),
    );
}

/// A host made without a lobby (the command line's `kjk host`, the duo
/// task and the chain) welcomes a guest at once and sends no lobby.
fn without_lobby(probes: &mut Probes) {
    let mut room = Room::new();
    let mut world = SimWorld::new(sim::determinism::Setup::pinned().world, Level::arena())
        .unwrap_or_else(|error| panic!("the determinism world: {error}"));
    let mut server = Server::new(config(3), LevelRequest::Stage(Level::arena().id()));
    server.in_process(HOST_PLAYER);
    server.stepped(NetTime(0), &TickInput::default(), &world.snapshot());
    room.server = Some(server);
    let guest = room.join("Mia", BUILD);
    room.pump(20);
    let names = room.names(guest);
    probes.check(
        "lobby: a host without a lobby welcomes at once",
        "a guest of a host made without a lobby is welcomed and is sent no lobby",
        !room.server().in_lobby() && names.contains(&"welcome") && !names.contains(&"lobby"),
        format!("{names:?}"),
    );
}

/// The lobby's seats when Mia (slot 1) plays the main character: Mia in the
/// first seat, the host (slot 0) in the second, Kalle (slot 2) in the third.
const MIA_LEADS: [(usize, Seat); 3] = [(0, Seat(1)), (1, Seat(0)), (2, Seat(2))];

/// A room where Mia plays the main character: Mia and Kalle joined, the
/// game began with `MIA_LEADS`, and the host's own player connected. Mia's
/// and the host's player's indices.
fn mia_leads() -> (Room, usize, usize) {
    let mut room = Room::new();
    let mia = room.join("Mia", BUILD);
    room.join("Kalle", BUILD);
    room.pump(20);
    room.server().start();
    room.pump(10);
    room.begin_with(&MIA_LEADS);
    room.pump(20);
    let host = room.host_joins();
    room.pump(20);
    (room, mia, host)
}

/// A guest who plays the main character sits in the first seat, and the
/// host's own player takes the seat the lobby gave it.
fn hero_seats(probes: &mut Probes) {
    let (room, mia, host) = mia_leads();
    let seats = [room.seat(mia), room.seat(1), room.seat(host)];
    probes.check(
        "lobby: the main character's guest takes the first seat",
        "Mia, who plays the main character, is welcomed into seat 0, Kalle into seat 2, and the host's own player into seat 1",
        seats == [Some(0), Some(2), Some(1)] && room.moves().is_empty(),
        format!("seats {seats:?}, moves {:?}", room.moves()),
    );
}

/// The seats a lobby gave stay the session's: after an install (a module
/// change, or the reload after the party fell) Mia still has the first seat
/// and the host's own player the second. A new session on a save made then
/// (a host loading it alone, or hosting it again) seats the host's player
/// first: the save keeps no seats.
fn hero_for_the_session(probes: &mut Probes) {
    let (mut room, ..) = mia_leads();
    let before = room.seats();
    let installed = room.install();
    room.pump(100);
    let after = room.seats();
    let mut alone = Room::new();
    let mut world = SimWorld::new(sim::determinism::Setup::pinned().world, Level::arena())
        .unwrap_or_else(|error| panic!("the determinism world: {error}"));
    let mut server = Server::new(config(3), LevelRequest::Stage(Level::arena().id()));
    server.in_process(HOST_PLAYER);
    server.stepped(NetTime(0), &TickInput::default(), &world.snapshot());
    alone.server = Some(server);
    let host = alone.host_joins();
    alone.pump(20);
    let mia = (Peer::Local(1), 0);
    let ok = installed.is_some()
        && before.contains(&mia)
        && before.contains(&(HOST_PLAYER, 1))
        && after == before
        && room.moves().is_empty()
        && alone.seat(host) == Some(0);
    probes.check(
        "lobby: the seats last the whole session",
        "after an install Mia keeps the first seat and the host's player the second; a session made anew seats the host's player first",
        ok,
        format!(
            "before {before:?}, after {after:?}, installed {}, the new session's host in {:?}",
            installed.is_some(),
            alone.seat(host)
        ),
    );
}

/// Seconds of virtual time, as pump steps.
fn seconds(seconds: u64) -> usize {
    (seconds * 1_000_000 / STEP_US) as usize
}

/// A guest leaves at the same step the host starts: the session begins
/// with the host's own player in the first seat.
fn hero_leaves_at_start(probes: &mut Probes) {
    let mut room = Room::new();
    let mia = room.join("Mia", BUILD);
    room.pump(20);
    let now = room.now;
    room.guests[mia].1.leave(now, Refusal::Left);
    room.server().start();
    room.pump(10);
    room.begin_with(&[(0, Seat(1)), (1, Seat(0))]);
    room.pump(20);
    let host = room.host_joins();
    room.pump(20);
    probes.check(
        "lobby: the main character's guest leaves at Start",
        "the guest who was to play the main character left as the host started, so the host's own player takes the first seat",
        room.seat(host) == Some(0),
        format!("the host's player in seat {:?}", room.seat(host)),
    );
}

/// What becomes of the first seat when its player goes: (the check's name,
/// what it wants, what happens to Mia, the seconds that pass after, whether
/// she comes back with her token, the host's moves wanted, and the seat
/// Mia's return wants).
type Away = (
    &'static str,
    &'static str,
    fn(&mut Room, usize),
    u64,
    bool,
    &'static [(u8, u8)],
    Option<u8>,
);

/// Mia's connection is lost.
fn lose(room: &mut Room, mia: usize) {
    room.silent.push(mia);
}

/// Mia leaves on purpose.
fn quit(room: &mut Room, mia: usize) {
    let now = room.now;
    room.guests[mia].1.leave(now, Refusal::Left);
}

/// The first seat's player drops or leaves: kept for them for
/// `RECONNECT_US` after the host times them out, else the host's own
/// player takes it (`Server::host_takes_first_seat`).
fn hero_away(probes: &mut Probes) {
    let cases: [Away; 4] = [
        (
            "lobby: the main character's player returns within 60 seconds",
            "a lost connection's player returns 30 seconds later with the seat's token and is welcomed into the first seat again; the host's player stays where it was",
            lose,
            30,
            true,
            &[],
            Some(0),
        ),
        (
            "lobby: the host takes the main character after 60 seconds",
            "a lost connection's player stays away past the 60 seconds the seat is kept, so the host's own player moves from seat 1 to the first seat",
            lose,
            61 + crate::link::TIMEOUT_US / 1_000_000,
            false,
            &[(1, 0)],
            None,
        ),
        (
            "lobby: the host takes the main character when its player leaves",
            "a player who leaves on purpose keeps no seat, so the host's own player moves into the first seat at once",
            quit,
            0,
            false,
            &[(1, 0)],
            None,
        ),
        (
            "lobby: a player back after the takeover is a late guest",
            "a player who returns with the token after the host took the first seat is welcomed into the seat the host left",
            lose,
            61 + crate::link::TIMEOUT_US / 1_000_000,
            true,
            &[(1, 0)],
            Some(1),
        ),
    ];
    for (name, wanted, away, after, back, moves, returns) in cases {
        let (mut room, mia, host) = mia_leads();
        away(&mut room, mia);
        room.pump(seconds(after) + 20);
        let back = back.then(|| {
            room.silent.retain(|index| *index != mia);
            let again = room.rejoin(mia);
            room.pump(40);
            room.seat(again)
        });
        let seen = (
            room.moves(),
            back.flatten(),
            room.server().occupied(Seat(0)),
        );
        probes.check(
            name,
            wanted,
            seen.0 == moves && seen.1 == returns && seen.2 && room.seat(host).is_some(),
            format!(
                "moves {:?}, the return's seat {:?}, the first seat taken {}",
                seen.0, seen.1, seen.2
            ),
        );
    }
}

/// The lobby's checks.
pub fn probes() -> Probes {
    let mut probes = Probes::default();
    round_trips(&mut probes);
    gathering(&mut probes);
    leaving(&mut probes);
    closing(&mut probes);
    arriving(&mut probes);
    without_lobby(&mut probes);
    hero_seats(&mut probes);
    hero_leaves_at_start(&mut probes);
    hero_away(&mut probes);
    hero_for_the_session(&mut probes);
    probes
}
