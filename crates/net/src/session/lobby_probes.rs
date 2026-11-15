//! The co-op lobby on the wire (`kjk net probes`): each lobby message read
//! back as written, and a host's lobby with real clients joined to it by
//! hand, in virtual time.

use ::session::lobby::probes::usual;
use sim::{Level, SimWorld, TickInput, TickRate};

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
}

impl Room {
    fn new() -> Room {
        Room {
            server: Some(lobby_server()),
            guests: Vec::new(),
            now: NetTime(0),
            seen: Vec::new(),
            events: Vec::new(),
        }
    }

    /// A guest named `name` of build `build` starts joining.
    fn join(&mut self, name: &str, build: &str) -> usize {
        let at = Peer::Local(self.guests.len() as u32 + 1);
        let client = Client::connect(
            credentials(name, build),
            HOST,
            u64::from(self.guests.len() as u32) + 11,
            self.now,
            ClientConfig::default(),
        )
        .unwrap_or_else(|error| panic!("a lobby probe's client: {error:?}"));
        self.guests.push((at, client));
        self.seen.push(Vec::new());
        self.guests.len() - 1
    }

    /// Moves datagrams both ways for `steps` steps of virtual time. Without
    /// a server, what the guests send is lost.
    fn pump(&mut self, steps: usize) {
        for _ in 0..steps {
            self.now = self.now.plus(STEP_US);
            let now = self.now;
            for (index, (at, client)) in self.guests.iter_mut().enumerate() {
                while let Some((_, bytes)) = client.poll_transmit(now) {
                    if let Some(server) = &mut self.server {
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
                if let Some((_, client)) = self.guests.iter_mut().find(|(at, _)| *at == to) {
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

    /// The host's world is loaded: the lobby ends in the arena.
    fn begin(&mut self) {
        let now = self.now;
        let mut world = SimWorld::new(sim::determinism::Setup::pinned().world, Level::arena())
            .unwrap_or_else(|error| panic!("the determinism world: {error}"));
        let server = self.server();
        server.begin(now, config(3), LevelRequest::Stage(Level::arena().id()));
        server.stepped(now, &TickInput::default(), &world.snapshot());
    }
}

/// Each message read back as it was written.
fn round_trips(probes: &mut Probes) {
    let lobby = Box::new(usual(2));
    let messages = [
        ("lobby", Message::Lobby { slot: 2, lobby }),
        ("choose", Message::Choose { npc: 7, swap: true }),
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
    room.guests[a].1.choose(2, false);
    room.guests[b].1.choose(2, false);
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
                    npc: 2,
                    swap: false,
                },
                &ServerEvent::LobbyChoose {
                    slot: 2,
                    npc: 2,
                    swap: false,
                },
                &ServerEvent::LobbyReady { slot: 2, on: true },
            ],
        format!("{asked:?}"),
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

/// The lobby's checks.
pub fn probes() -> Probes {
    let mut probes = Probes::default();
    round_trips(&mut probes);
    gathering(&mut probes);
    leaving(&mut probes);
    closing(&mut probes);
    arriving(&mut probes);
    probes
}
