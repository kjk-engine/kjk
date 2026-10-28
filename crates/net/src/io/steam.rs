//! Steam's relayed messages (`ISteamNetworkingMessages`): datagrams to and
//! from a player's Steam ID, peer to peer where the two machines reach each
//! other and through Valve's relays where they do not, so nobody opens a
//! port. UDP's contract holds: sends never block and may drop (they go
//! unreliable; the link above resends what matters), arrivals are stamped
//! with the owner's clock.
//!
//! **One Steam a process.** The running Steam client's, opened once as app
//! `APP_ID`; where Steam is not running (or `KJK_STEAM=0`) `client` says
//! why and players join by address alone. Its callbacks run under one lock
//! (`pump`), from whichever thread asks: the net thread's receives and the
//! game's frames.
//!
//! **Every session request is accepted:** the handshake above decides who
//! joins, as it does for an address.
//!
//! **Hosting** (`advertise`) sets the rich presence Steam's friends list
//! reads: its "Join Game" and an invite hand the friend's running game
//! `connect_string` (taken by `join_requests`), and `hosting_friends` lists
//! who offers one.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock, PoisonError};

use steamworks::networking_types::{NetworkingIdentity, SendFlags};
use steamworks::{Client, FriendFlags, GameRichPresenceJoinRequested, SteamId};

use super::Clock;
use crate::{Datagram, MAX_DATAGRAM, Peer, Transport};

/// Valve's test app ("Spacewar"), which every Steam account may run, until
/// kjk has an app of its own. Friends see the host "playing Spacewar".
pub const APP_ID: u32 = 480;
/// The channel kjk's datagrams travel on.
const CHANNEL: u32 = 0;
/// The most messages taken from Steam at once.
const BATCH: usize = 64;
/// The rich presence key that marks a kjk host among app 480's players.
const MARK: &str = "kjk";

static STEAM: OnceLock<Result<Client, String>> = OnceLock::new();
static PUMP: Mutex<()> = Mutex::new(());
/// Hosts friends asked to join from Steam, not yet taken.
static JOINS: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// The process's Steam, opened the first time it is asked for; why not,
/// where Steam is not running or is turned off.
pub fn client() -> Result<&'static Client, &'static str> {
    STEAM
        .get_or_init(|| {
            if std::env::var_os("KJK_STEAM").is_some_and(|value| value == "0") {
                return Err("turned off (KJK_STEAM=0)".into());
            }
            let client = Client::init_app(APP_ID)
                .map_err(|error| format!("not running, or it refused kjk ({error})"))?;
            client.networking_utils().init_relay_network_access();
            client
                .networking_messages()
                .session_request_callback(|request| {
                    request.accept();
                });
            // Kept for the process's life, as Steam is.
            std::mem::forget(
                client.register_callback(|request: GameRichPresenceJoinRequested| {
                    if let Some(host) = parse_connect(&request.connect) {
                        JOINS
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .push(host);
                    }
                }),
            );
            Ok(client)
        })
        .as_ref()
        .map_err(String::as_str)
}

/// Runs Steam's pending callbacks (session requests, join requests), one
/// thread at a time.
pub fn pump(client: &Client) {
    let _held = PUMP.lock().unwrap_or_else(PoisonError::into_inner);
    client.run_callbacks();
}

/// The hosts a friend's "Join Game" or invite asked to join since last
/// asked (they arrive as `pump` runs).
pub fn join_requests() -> Vec<u64> {
    std::mem::take(&mut *JOINS.lock().unwrap_or_else(PoisonError::into_inner))
}

/// This player's Steam name, where Steam is open.
pub fn persona_name() -> Option<String> {
    Some(client().ok()?.friends().name())
}

/// What a host hands Steam to give a friend who joins it.
pub fn connect_string(host: u64) -> String {
    format!("+connect {}", crate::steam_address(host))
}

/// The host a connect string (or a `steam:` address) names.
pub fn parse_connect(connect: &str) -> Option<u64> {
    crate::parse_steam(connect.trim().trim_start_matches("+connect").trim())
}

/// Steam friends hosting a kjk session now: each one's name and Steam ID.
pub fn hosting_friends() -> Vec<(String, u64)> {
    let Ok(client) = client() else {
        return Vec::new();
    };
    client
        .friends()
        .get_friends(FriendFlags::IMMEDIATE)
        .into_iter()
        .filter(|friend| friend.rich_presence(MARK).is_some())
        .filter_map(|friend| {
            let host = parse_connect(&friend.rich_presence("connect")?)?;
            Some((friend.name(), host))
        })
        .collect()
}

/// What the Steam end did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SteamStats {
    pub sent: u64,
    pub received: u64,
    /// Datagrams not sent: to a peer that is not a Steam ID, too long, or
    /// refused by Steam.
    pub dropped: u64,
    /// Messages received longer than `MAX_DATAGRAM`, or from no Steam ID.
    pub refused: u64,
}

/// This player's end of Steam's messages, as a `Transport`.
pub struct Steam {
    client: Client,
    id: u64,
    clock: Clock,
    inbox: VecDeque<Datagram>,
    stats: SteamStats,
    /// Friends are told this end hosts (cleared when it is dropped).
    advertised: bool,
}

impl Steam {
    /// This player's end, stamping arrivals with `clock`.
    pub fn open(clock: Clock) -> Result<Steam, String> {
        let client = client().map_err(|why| format!("Steam: {why}"))?.clone();
        let id = client.user().steam_id().raw();
        Ok(Steam {
            client,
            id,
            clock,
            inbox: VecDeque::new(),
            stats: SteamStats::default(),
            advertised: false,
        })
    }

    /// This player's Steam ID.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn stats(&self) -> SteamStats {
        self.stats
    }

    /// Tells Steam friends this end hosts: their friends list offers "Join
    /// Game", and an invite carries the connect string.
    pub fn advertise(&mut self) {
        let friends = self.client.friends();
        friends.set_rich_presence(MARK, Some("host"));
        friends.set_rich_presence("status", Some("Hosting a kjk game"));
        friends.set_rich_presence("connect", Some(&connect_string(self.id)));
        self.advertised = true;
    }

    /// Takes what Steam has received into the inbox.
    fn fill(&mut self) {
        pump(&self.client);
        let now = (self.clock)();
        let messages = self
            .client
            .networking_messages()
            .receive_messages_on_channel(CHANNEL, BATCH);
        for message in messages {
            let from = message.identity_peer().steam_id();
            match from {
                Some(from) if message.data().len() <= MAX_DATAGRAM => {
                    self.stats.received += 1;
                    self.inbox.push_back(Datagram {
                        from: Peer::Steam(from.raw()),
                        at: now,
                        bytes: message.data().to_vec(),
                    });
                }
                _ => self.stats.refused += 1,
            }
        }
    }
}

impl Transport for Steam {
    fn local(&self) -> Peer {
        Peer::Steam(self.id)
    }

    fn send(&mut self, to: Peer, bytes: &[u8]) {
        let Peer::Steam(id) = to else {
            self.stats.dropped += 1;
            return;
        };
        if bytes.len() > MAX_DATAGRAM {
            self.stats.dropped += 1;
            return;
        }
        let user = NetworkingIdentity::new_steam_id(SteamId::from_raw(id));
        let flags = SendFlags::UNRELIABLE_NO_NAGLE | SendFlags::AUTO_RESTART_BROKEN_SESSION;
        match self
            .client
            .networking_messages()
            .send_message_to_user(user, flags, bytes, CHANNEL)
        {
            Ok(()) => self.stats.sent += 1,
            Err(_) => self.stats.dropped += 1,
        }
    }

    fn recv(&mut self) -> Option<Datagram> {
        if self.inbox.is_empty() {
            self.fill();
        }
        self.inbox.pop_front()
    }
}

impl Drop for Steam {
    fn drop(&mut self) {
        if self.advertised {
            self.client.friends().clear_rich_presence();
        }
    }
}
