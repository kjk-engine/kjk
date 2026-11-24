//! The co-op lobby on the host (`docs/superpowers/specs/2026-11-15-coop-lobby-design.md`).
//!
//! **Open** runs on the session's thread when a player hosts from the menu.
//! It reads the save, describes it for the lobby (`describe`), makes the
//! host's credentials from the save's game, binds the port, opens Steam and
//! starts the lobby's server (`solo::OpenNet`). Guests can join from then
//! on. The lobby screen opens once Open is done.
//!
//! **Each frame** (`frame`) the host's game applies what the guests asked
//! (the server's lobby events) and what its own player did (`LobbyIntent`)
//! to the lobby's state (`session::lobby::Lobby`), sends the state to every
//! guest when it changed, and shows it (`ViewModel::lobby`).
//!
//! **Start** fixes the collision and saber rules the lobby chose (both are
//! the host's settings), writes the chosen party into the save as party
//! changes its scripts make as it loads, seeds each seat's companion
//! (`sim::Save::seat_companions`), and loads the world with three seats on
//! the session's thread. The lobby's players are kept and welcomed as the
//! world begins (`net::session::Server::begin`).
//!
//! The command line's hosts (`kjk host`, `cargo xtask duo`, the chain) go
//! straight into the game, as before: `HostSpec::lobby` is `None` for them.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use bevy::prelude::*;
use session::lobby::{Collision, Companion, Game, Lobby, Playing, SLOTS, Summary};
use sim::script::party_choice::{self, Choice};
use ui::intent::{CharacterDesc, Intent, LobbyIntent, SessionIntent};
use ui::vm::{
    LobbyCompanionState, LobbyCompanionView, LobbyHeroView, LobbySaveView, LobbySlotState,
    LobbySlotView, LobbyView,
};
use vfs::Game as Install;

use crate::solo::OpenNet;
use crate::ui_view::{Ui, UiIntents};

/// What the host's game says to its guests when its player leaves.
pub const CLOSED: &str = ui::strings::LOBBY_CLOSED;

/// The save the lobby is about to play, decoded: its id (none for a new
/// game), its world, the character it was made with, its game, and what
/// the lobby shows of it.
pub struct Chosen {
    pub id: Option<String>,
    pub save: Option<sim::Save>,
    pub character: Option<CharacterDesc>,
    pub game: Install,
    pub summary: Summary,
    pub companions: Vec<Companion>,
    /// The companions in the save's party now.
    pub current: Vec<u8>,
}

/// The host's lobby: its state, the save, and the network open for it.
#[derive(Resource)]
pub struct LobbyHost {
    pub lobby: Lobby,
    chosen: Chosen,
    net: Option<OpenNet>,
    port: u16,
    steam: bool,
    address: String,
    /// The state last sent to the guests.
    sent: Option<Lobby>,
    /// A save being read after Change Save.
    changing: Option<Mutex<Receiver<Result<Chosen, String>>>>,
    /// Why the last save chosen was refused, for the feedback line.
    notice: Option<String>,
}

/// The game a save is of: its meta's word, else the game of the module it
/// was made in (an older save's meta names none), else `campaign`, else
/// KOTOR.
fn game_of(
    meta: Option<&saves::Meta>,
    save: Option<&sim::Save>,
    campaign: Option<Install>,
) -> Install {
    let placed = save
        .and_then(|save| save.snapshot.place.as_ref())
        .map(|place| place.game.as_str());
    (meta
        .and_then(|meta| meta.game.as_deref())
        .and_then(Install::from_key))
    .or(placed.and_then(Install::from_key))
    .or(campaign)
    .unwrap_or(Install::Kotor1)
}

/// A class row's name and a level, as a slot shows them ("Soldier, level
/// 4"), in the game's code page.
fn class_level(class: &[u8], level: u8) -> Vec<u8> {
    let mut text = class.to_vec();
    if !text.is_empty() {
        text.extend_from_slice(b", ");
    }
    text.extend_from_slice(format!("{} {level}", ui::strings::LOBBY_LEVEL).as_bytes());
    text
}

/// A career's class with the most levels, and its total level.
fn of_career(career: &[(u8, u8)]) -> (Option<u8>, u8) {
    let main = career
        .iter()
        .max_by_key(|(_, levels)| *levels)
        .map(|(class, _)| *class);
    let level = career
        .iter()
        .map(|(_, levels)| *levels)
        .fold(0u8, u8::saturating_add);
    (main, level)
}

/// Reads save `id` (or describes a new game of `campaign` made as
/// `character`) for the lobby, with the names, portraits and classes
/// `discovery`'s install of its game gives them.
pub fn choose(
    discovery: &vfs::Discovery,
    id: Option<String>,
    character: Option<CharacterDesc>,
    campaign: Option<Install>,
) -> Result<Chosen, String> {
    let read = id.as_deref().map(crate::save_view::read).transpose()?;
    let meta = read.as_ref().map(|(_, meta)| meta);
    let game = game_of(meta, read.as_ref().map(|(save, _)| save), campaign);
    let character = meta.and_then(|meta| meta.character.clone()).or(character);
    let mut assets = ui::assets::GuiAssets::open(discovery, game).ok();
    let save = read.as_ref().map(|(save, _)| save);
    let (summary, companions, current) =
        summarize(save, meta, character.as_ref(), game, assets.as_mut());
    Ok(Chosen {
        id,
        save: read.map(|(save, _)| save),
        character,
        game,
        summary,
        companions,
        current,
    })
}

/// What the lobby shows of `save` (none: a new game) of `game`, made as
/// `character`: its summary, its recruited companions, and the companions in
/// its party now. `assets` gives the names, portraits and classes; without
/// them a companion is shown by its template.
///
/// The summary's hero is the story's character, the one the first seat
/// plays (`story_companion`). When that is a companion, it is the hero with
/// its own name, portrait, class and level, and it is not offered as a
/// companion. The main character is then offered nowhere.
pub fn summarize(
    save: Option<&sim::Save>,
    meta: Option<&saves::Meta>,
    character: Option<&CharacterDesc>,
    game: Install,
    mut assets: Option<&mut ui::assets::GuiAssets>,
) -> (Summary, Vec<Companion>, Vec<u8>) {
    let (mut companions, current) = match save {
        Some(save) => describe(save, meta, assets.as_deref_mut()),
        None => (Vec::new(), Vec::new()),
    };
    let story = (save.and_then(story_companion))
        .and_then(|npc| companions.iter().position(|c| c.npc == npc))
        .map(|at| companions.remove(at));
    let hero = match story {
        Some(story) => Hero {
            name: story.name,
            class: story.class,
            level: story.level,
            portrait: story.portrait,
        },
        None => main_character(save, character, assets),
    };
    let summary = Summary {
        save: meta.map_or_else(
            || ui::strings::bytes(ui::strings::NEW_GAME),
            |meta| fmt_odyssey::code_page_bytes(&meta.name),
        ),
        area: meta.map_or_else(Vec::new, |meta| fmt_odyssey::code_page_bytes(&meta.area)),
        game: match game {
            Install::Kotor2 => Game::Tsl,
            _ => Game::K1,
        },
        saved: meta.map_or(0, |meta| meta.saved),
        hero: hero.name,
        hero_class: hero.class,
        hero_level: hero.level,
        hero_portrait: hero.portrait,
    };
    (summary, companions, current)
}

/// Whether `save`'s main character can be played: its avatar is in the
/// area, not benched while the first seat plays another character, and not
/// held by a script (`SetCommandable`).
pub fn main_playable(save: &sim::Save) -> bool {
    let snapshot = &save.snapshot;
    snapshot
        .own(sim::Seat(0))
        .is_some_and(|own| snapshot.in_party(own) && !save.script.uncommandable.contains(&own.id.0))
}

/// The companion the first seat plays when `save` loads, in a part of the
/// story where the game makes the player play someone else: none while the
/// main character can be played (`main_playable`). Else the companion
/// leading the party (`Party::leader`, a script's `SetPartyLeader`), or the
/// companion a script switched the first seat to (`Party::switched`:
/// KOTOR 2's T3-M4 in the prologue and Vogga's warehouse, Atton on Goto's
/// yacht, Visas Marr on Dxun, the remote on Malachor, KOTOR's rescuer on
/// the Leviathan). The snapshot's first body follows the same order
/// (`Snapshot::body`). A switch always benches the main character. A
/// companion leading while the main character can be played was chosen
/// with the leader key, and is an ordinary companion.
pub fn story_companion(save: &sim::Save) -> Option<u8> {
    let party = &save.script.party;
    match party.switched {
        Some(_) => party.leader.or(party.switched),
        None => party.leader.filter(|_| !main_playable(save)),
    }
}

/// The story's character as the lobby shows it: its name and class in the
/// game's code page, its level, and its portrait texture.
struct Hero {
    name: Vec<u8>,
    class: Vec<u8>,
    level: u8,
    portrait: String,
}

/// The main character as `save` holds it: its persona (the portrait and the
/// name it plays under, `sim::persona`) and its career. A new game's and a
/// menu save's character creation (`character`) names it first.
fn main_character(
    save: Option<&sim::Save>,
    character: Option<&CharacterDesc>,
    mut assets: Option<&mut ui::assets::GuiAssets>,
) -> Hero {
    let persona = save.and_then(|save| sim::persona::of(&save.snapshot.personas, sim::Seat(0)));
    let body = save.and_then(|save| save.snapshot.own(sim::Seat(0)));
    let career = save
        .zip(body)
        .and_then(|(save, body)| save.script.careers.get(&body.id.0))
        .map(|career| of_career(career));
    let class = character
        .map(|made| made.class)
        .or(career.and_then(|(class, _)| class));
    let hero_class = (assets.as_mut())
        .zip(class)
        .and_then(|(assets, row)| assets.cell_text("classes", usize::from(row), "name"))
        .unwrap_or_else(Vec::new);
    let hero_level = (career.map(|(_, level)| level))
        .or(body.and_then(|body| Some(body.sheet.as_ref()?.level)))
        .filter(|level| *level > 0)
        .unwrap_or(1);
    let hero = (character.map(|made| made.name.as_str()))
        .or(persona.map(|persona| persona.name.as_str()))
        .filter(|name| !name.is_empty())
        .map_or_else(Vec::new, fmt_odyssey::code_page_bytes);
    // The portrait as the HUD draws it: the persona's `portraits.2da` row,
    // else the texture character creation picked.
    let hero_portrait = (assets.as_mut())
        .zip(persona)
        .and_then(|(assets, persona)| assets.portrait(usize::from(persona.portrait), 0))
        .or(character.map(|made| made.portrait.clone()))
        .unwrap_or_else(String::new);
    Hero {
        name: hero,
        class: hero_class,
        level: hero_level,
        portrait: hero_portrait,
    }
}

/// The recruited companions of `save` as the lobby shows them, each with
/// its answer from the shared choice rule (`party_choice::choices`), and the
/// companions in its party now. The area forbids a party change when the
/// script state marks the save's module unescapable, and so does a part of
/// the story in which a companion plays the story's character.
fn describe(
    save: &sim::Save,
    meta: Option<&saves::Meta>,
    mut assets: Option<&mut ui::assets::GuiAssets>,
) -> (Vec<Companion>, Vec<u8>) {
    let party = &save.script.party;
    let module = meta.map_or_else(String::new, |meta| sim::script::key(&meta.module));
    // While a companion plays the story's character, the story keeps the
    // party it made: only its members can be chosen.
    let locked = save.script.unescapable.contains(&module) || story_companion(save).is_some();
    let companions = party_choice::choices(party, locked)
        .into_iter()
        .filter_map(|(npc, choice)| {
            let known = party.available.get(&npc)?;
            let template = known.template.as_str();
            let creature = known.object.or(known.kept.map(|kept| kept.from));
            let facts = known.template_facts();
            let career = (creature.and_then(|id| save.script.careers.get(&id).cloned()))
                .or_else(|| facts.as_ref().map(|facts| facts.classes.clone()))
                .unwrap_or_else(Vec::new);
            let (class, level) = of_career(&career);
            let level = known
                .kept
                .map(|kept| kept.sheet.level)
                .filter(|level| *level > 0)
                .unwrap_or(level.max(1));
            let assets = assets.as_deref_mut();
            let (name, portrait, class) = match assets {
                Some(assets) => (
                    assets
                        .template_name(template, fmt_odyssey::ResType::UTC)
                        .unwrap_or_else(|| template.as_bytes().to_vec()),
                    assets.portrait_of(template, 0).unwrap_or_else(String::new),
                    class
                        .and_then(|row| assets.cell_text("classes", usize::from(row), "name"))
                        .unwrap_or_else(Vec::new),
                ),
                None => (template.as_bytes().to_vec(), String::new(), Vec::new()),
            };
            Some(Companion {
                npc,
                name,
                portrait,
                class,
                level,
                choice,
            })
        })
        .collect();
    (companions, party.members.clone())
}

/// The host's name in the lobby: the Steam persona name when Steam runs,
/// as a guest who joins through Steam is named (`steam_join`), else
/// "Player". The settings keep no name the player typed: the Join screen
/// starts from "Player" every time.
fn host_name() -> String {
    #[cfg(feature = "steam")]
    if let Some(name) = net::io::steam::persona_name().filter(|name| !name.is_empty()) {
        return name;
    }
    ui::strings::DEFAULT_NAME.to_string()
}

/// The host's local network address, for the address line. A UDP socket
/// "connected" to a private address sends nothing: the system only picks
/// the interface it would use. No outside service is asked.
pub fn local_address() -> Option<std::net::IpAddr> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("192.168.255.255:9").ok()?;
    Some(socket.local_addr().ok()?.ip()).filter(|ip| !ip.is_unspecified())
}

/// Open: the lobby for `chosen`, listening on `port` with Steam beside it,
/// for players whose build and data match this machine's world data of the
/// save's game. `status` tells the loading screen what takes a while.
pub fn open(
    discovery: &vfs::Discovery,
    chosen: Chosen,
    port: u16,
    status: &dyn Fn(String),
) -> Result<LobbyHost, String> {
    let games = [Install::JediAcademy, chosen.game];
    let data = crate::world_data(discovery, &games, status)?;
    let credentials = crate::solo::credentials("host", data);
    status(format!("Hosting on port {port}"));
    let address = std::net::SocketAddr::new(std::net::Ipv6Addr::UNSPECIFIED.into(), port);
    let socket = crate::listen_socket(address, true)?;
    let socket = net::netem::Netem::new(socket, crate::netem_conditions(None)?);
    let net = OpenNet::open(socket, SLOTS as u8, credentials, None)?;
    let steam = net.steam();
    let port = net.address().map_or(port, |address| address.port());
    let address =
        local_address().map_or_else(|| format!("port {port}"), |ip| format!("{ip} : {port}"));
    info!(
        target: "lobby",
        party = ?chosen.current,
        companions = ?chosen.companions.iter().map(|c| (c.npc, c.choice)).collect::<Vec<_>>(),
        "the lobby opened"
    );
    let lobby = lobby_of(&chosen);
    Ok(LobbyHost {
        lobby,
        chosen,
        net: Some(net),
        port,
        steam,
        address,
        sent: None,
        changing: None,
        notice: None,
    })
}

/// The lobby on `chosen` as it opens: the host in its slot, the saber
/// rules and the collision as the host's settings have them, and the save's
/// companions.
pub fn lobby_of(chosen: &Chosen) -> Lobby {
    let settings = crate::settings::Settings::read();
    let saber_rules = settings
        .value("game.saber_rules")
        .unwrap_or("ja")
        .to_string();
    let mut lobby = Lobby::new(host_name(), saber_rules, settings.collision());
    lobby.reset(
        chosen.summary.clone(),
        chosen.companions.clone(),
        &chosen.current,
    );
    lobby
}

impl LobbyHost {
    /// Applies the server's lobby events, its players' delays, and a save
    /// read after Change Save.
    fn take_network(&mut self) {
        let Some(net) = &self.net else {
            return;
        };
        let (events, delays) = net.with_server(|server| {
            let events: Vec<_> = std::iter::from_fn(|| server.poll_event()).collect();
            (events, server.lobby_delays())
        });
        for event in events {
            self.take_event(event);
        }
        for (slot, delay) in delays {
            if let Some(Some(place)) = self.lobby.slots.get_mut(slot) {
                place.delay_ms = delay;
            }
        }
        let read = self.changing.as_ref().map(|job| {
            job.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .try_recv()
        });
        match read {
            Some(Ok(Ok(chosen))) => {
                self.changing = None;
                self.lobby.reset(
                    chosen.summary.clone(),
                    chosen.companions.clone(),
                    &chosen.current,
                );
                self.chosen = chosen;
            }
            Some(Ok(Err(why))) => {
                warn!(target: "lobby", %why, "the save chosen does not load");
                self.changing = None;
                self.lobby.summary = Some(self.chosen.summary.clone());
                self.notice = Some(why);
            }
            Some(Err(TryRecvError::Disconnected)) => self.changing = None,
            _ => {}
        }
    }

    fn take_event(&mut self, event: net::session::ServerEvent) {
        use net::session::ServerEvent as Event;
        match event {
            Event::LobbyJoined { slot, name } => {
                self.lobby.join_at(slot, name.clone());
                let playing = self.lobby.playing(slot);
                info!(target: "lobby", slot, name, ?playing, "a player joined the lobby");
            }
            Event::LobbyLeft { slot } => {
                info!(target: "lobby", slot, "a player left the lobby");
                self.lobby.leave(slot);
            }
            Event::LobbyChoose { slot, pick, swap } => {
                let made = self.lobby.choose(slot, pick, swap);
                info!(target: "lobby", slot, ?pick, swap, made, seats = ?self.lobby.seats(), "a player chose");
            }
            Event::LobbyReady { slot, on } => self.lobby.ready(slot, on),
            Event::Refused { peer, refusal } => {
                info!(target: "lobby", ?peer, %refusal, "a player was refused");
            }
            _ => {}
        }
    }

    /// Sends the lobby to the guests when it changed since it was last sent.
    fn send(&mut self) {
        if self.sent.as_ref() == Some(&self.lobby) {
            return;
        }
        if let Some(net) = &self.net {
            net.with_server(|server| server.send_lobby(&self.lobby));
        }
        self.sent = Some(self.lobby.clone());
    }

    /// The lobby as this host's screen shows it.
    pub fn view(&self) -> LobbyView {
        LobbyView {
            host: true,
            address: self.address.clone(),
            steam: self.steam,
            ..view(&self.lobby, 0)
        }
    }

    /// The save the lobby plays (`started`); a new game has none.
    fn started_save(&self) -> Result<Option<sim::Save>, String> {
        (self.chosen.save.as_ref())
            .map(|save| started(save, &self.lobby))
            .transpose()
    }
}

/// `save` with `lobby`'s party written into it: the party changes that make
/// it, queued for its scripts to make as it loads, each seat's companion
/// seeded, and its module planned for the players present (the player of
/// the story's character and each player who plays a companion drive a
/// body, and `sim::scaling` counts those; a player who flies a droid drives
/// none, the host too). A lobby whose story's character nobody plays gives
/// it to the host first, so the game never starts without it.
pub fn started(save: &sim::Save, lobby: &Lobby) -> Result<sim::Save, String> {
    let mut lobby = lobby.clone();
    if lobby.keep_one_hero() {
        warn!(target: "lobby", "nobody played the story's character at the start: the host plays it");
    }
    let lobby = &lobby;
    let mut save = save.clone();
    // A companion leading the party plays the story's character from its
    // place in the party, which the lobby does not list: it stays.
    let story = story_companion(&save);
    let party = &mut save.script.party;
    let leading = story.filter(|npc| party.has(*npc));
    // A companion the player chose to lead goes back to following, as the
    // game's own leader change to the main character does it: the first
    // seat plays the main character.
    if story.is_none() && party.leader.is_some() {
        let own = save.snapshot.own(sim::Seat(0)).map(|own| own.id.0);
        if let Some(own) = own {
            party.lead(own, Some(own));
        }
        save.snapshot.leading = None;
    }
    let wanted: Vec<u8> = leading.into_iter().chain(lobby.party()).collect();
    save.queue_party(&wanted)?;
    let seats: Vec<(sim::Seat, u8)> = (lobby.seats().into_iter())
        .filter_map(|(_, seat, playing)| match playing {
            Playing::Companion(npc) => Some((sim::Seat(seat), npc)),
            _ => None,
        })
        .collect();
    save.seat_companions(&seats);
    save.plan_for(u8::try_from(1 + seats.len()).unwrap_or(u8::MAX));
    Ok(save)
}

/// Each occupied slot's simulation seat when `lobby` starts, as the
/// session's server seats its players (`solo::OpenNet::seat_slots`): the
/// story's character's player first (the host, when nobody plays it).
fn seat_slots(lobby: &Lobby) -> Vec<(usize, sim::Seat)> {
    let mut lobby = lobby.clone();
    lobby.keep_one_hero();
    (lobby.seats().into_iter())
        .map(|(slot, seat, _)| (slot, sim::Seat(seat)))
        .collect()
}

/// The settings value of a collision mode.
fn collision_key(collision: Collision) -> String {
    collision.name().to_string()
}

/// `lobby` as the screen of the player in `own_slot` shows it: each slot
/// with its character, each companion with who has it. The host's view adds
/// its address and Steam (`LobbyHost::view`).
pub fn view(lobby: &Lobby, own_slot: usize) -> LobbyView {
    let companion = |npc: u8| {
        lobby
            .companions
            .iter()
            .find(|companion| companion.npc == npc)
    };
    let party = lobby.party();
    let companions = (lobby.companions.iter())
        .map(|companion| {
            let played = (0..SLOTS).find_map(|slot| {
                let place = lobby.slots[slot].as_ref()?;
                (place.playing == Playing::Companion(companion.npc))
                    .then(|| (slot, place.name.clone()))
            });
            let state = match (companion.choice, played) {
                (_, Some((slot, player))) => LobbyCompanionState::Played { slot, player },
                (Choice::Unavailable, None) => LobbyCompanionState::Unavailable,
                _ if party.contains(&companion.npc) => LobbyCompanionState::WithParty,
                _ => LobbyCompanionState::Free,
            };
            LobbyCompanionView {
                npc: companion.npc,
                name: companion.name.clone(),
                portrait: (!companion.portrait.is_empty()).then(|| companion.portrait.clone()),
                state,
            }
        })
        .collect();
    let summary = lobby.summary.as_ref();
    let hero = summary.map(|summary| {
        let slot = lobby.hero_slot();
        LobbyHeroView {
            name: summary.hero.clone(),
            portrait: (!summary.hero_portrait.is_empty()).then(|| summary.hero_portrait.clone()),
            slot,
            player: (lobby.slots[slot].as_ref())
                .map_or_else(String::new, |place| place.name.clone()),
        }
    });
    let slots = std::array::from_fn(|slot| {
        let place = lobby.slots[slot].as_ref()?;
        let state = match (slot, place.ready, place.delay_ms) {
            (0, ..) => LobbySlotState::Host,
            (_, true, _) => LobbySlotState::Ready,
            (_, false, 0) => LobbySlotState::Connecting,
            _ => LobbySlotState::Choosing,
        };
        let (character, class_level, portrait, droid) = match place.playing {
            Playing::Hero => (
                summary.map_or_else(Vec::new, |summary| summary.hero.clone()),
                summary.map_or_else(Vec::new, |summary| {
                    self::class_level(&summary.hero_class, summary.hero_level)
                }),
                summary.map(|summary| summary.hero_portrait.clone()),
                false,
            ),
            Playing::Companion(npc) => match companion(npc) {
                Some(companion) => (
                    companion.name.clone(),
                    self::class_level(&companion.class, companion.level),
                    Some(companion.portrait.clone()),
                    false,
                ),
                None => (Vec::new(), Vec::new(), None, true),
            },
            Playing::Droid => (
                ui::strings::bytes(ui::strings::LOBBY_DROID),
                Vec::new(),
                None,
                true,
            ),
        };
        Some(LobbySlotView {
            player: place.name.clone(),
            character,
            class_level,
            portrait: portrait.filter(|portrait| !portrait.is_empty()),
            droid,
            state,
            delay_ms: (slot > 0).then_some(place.delay_ms),
        })
    });
    let save = summary.map(|summary| LobbySaveView {
        name: summary.save.clone(),
        area: summary.area.clone(),
        game: Some(match summary.game {
            Game::K1 => Install::Kotor1,
            Game::Tsl => Install::Kotor2,
        }),
        saved: crate::save_view::date(summary.saved),
        party: std::iter::once(summary.hero.clone())
            .chain(
                party
                    .iter()
                    .filter_map(|npc| Some(companion(*npc)?.name.clone())),
            )
            .collect(),
    });
    LobbyView {
        host: own_slot == 0,
        own_slot,
        save,
        saber_rules: lobby.saber_rules.clone(),
        collision: collision_key(lobby.collision),
        hero,
        companions,
        slots,
        address: String::new(),
        steam: false,
    }
}

/// What the host's lobby does with its player's intent. `Start` and
/// `Leave` end the lobby and are handled by `frame`.
fn apply(host: &mut LobbyHost, intent: &LobbyIntent, intents: &mut Vec<Intent>) {
    match intent {
        LobbyIntent::Fill(npc) => {
            host.lobby.fill(*npc);
        }
        LobbyIntent::SaberRules(rules) => {
            host.lobby.saber_rules = rules.clone();
            intents.push(Intent::Setting("game.saber_rules".into(), rules.clone()));
        }
        LobbyIntent::Collision(mode) => {
            if let Some(collision) = Collision::named(mode) {
                host.lobby.collision = collision;
                intents.push(Intent::Setting(
                    crate::settings::COLLISION.into(),
                    mode.clone(),
                ));
            }
        }
        LobbyIntent::Invite => {
            if let Some(net) = &host.net {
                net.invite();
            }
        }
        LobbyIntent::Choose(pick) => {
            let pick = crate::lobby_guest::pick_of(*pick);
            let swap = crate::lobby_guest::swaps(&host.lobby, 0, pick);
            let made = host.lobby.choose(0, pick, swap);
            info!(target: "lobby", ?pick, swap, made, seats = ?host.lobby.seats(), "the host chose");
        }
        LobbyIntent::Ready(_)
        | LobbyIntent::ChangeSave
        | LobbyIntent::Start
        | LobbyIntent::Leave => {}
    }
}

/// The lobby's frame on the host (see the module documentation), before the
/// session flow takes the menu's session intents: while the lobby is open,
/// the host list's choice is the lobby's new save, not a new session.
pub(crate) fn frame(world: &mut World) {
    let Some(mut host) = world.remove_resource::<LobbyHost>() else {
        return;
    };
    host.take_network();
    let taken: Vec<Intent> = std::mem::take(&mut world.resource_mut::<UiIntents>().0);
    let mut rest = Vec::new();
    let mut ended = None;
    for intent in taken {
        match intent {
            Intent::Lobby(LobbyIntent::Start) => ended = Some(LobbyIntent::Start),
            Intent::Lobby(LobbyIntent::Leave) => ended = Some(LobbyIntent::Leave),
            Intent::Lobby(LobbyIntent::ChangeSave) => {
                let mut ui = world.resource_mut::<Ui>();
                let ui = &mut *ui;
                ui.stack.open("host", &ui.vm);
            }
            Intent::Lobby(intent) => apply(&mut host, &intent, &mut rest),
            Intent::Session(SessionIntent::Host {
                save,
                character,
                campaign,
                ..
            }) => change_save(world, &mut host, save, character, campaign),
            other => rest.push(other),
        }
    }
    world.resource_mut::<UiIntents>().0 = rest;
    host.send();
    match ended {
        Some(LobbyIntent::Start) => start(world, host),
        Some(_) => leave(world, host),
        None => {
            let mut ui = world.resource_mut::<Ui>();
            ui.vm.lobby = Some(host.view());
            if let Some(why) = host.notice.take() {
                ui.vm.feedback.push(why);
            }
            world.insert_resource(host);
        }
    }
}

/// The host chose another save on the host list: guests stay, and see "The
/// host is choosing a save." until it is read. A save of another game is
/// refused, because the guests passed the data check for this one.
fn change_save(
    world: &mut World,
    host: &mut LobbyHost,
    save: Option<String>,
    character: Option<CharacterDesc>,
    campaign: Option<Install>,
) {
    let mut ui = world.resource_mut::<Ui>();
    for screen in CHANGE_SAVE_SCREENS {
        ui.stack.close(screen);
    }
    host.lobby.summary = None;
    let (sender, receiver) = channel();
    let game = host.chosen.game;
    std::thread::spawn(move || {
        let chosen = choose(&crate::discover(), save, character, campaign.or(Some(game)));
        let _ = sender.send(chosen.and_then(|chosen| {
            (chosen.game == game)
                .then_some(chosen)
                .ok_or_else(|| ui::strings::LOBBY_OTHER_GAME.to_string())
        }));
    });
    host.changing = Some(Mutex::new(receiver));
}

/// The screens Change Save opens on the way to a save or a new game, closed
/// again once one is chosen.
const CHANGE_SAVE_SCREENS: [&str; 3] = ["host", "campaign", "chargen"];

/// The host left: every guest is told the lobby closed, and the main menu
/// opens.
fn leave(world: &mut World, mut host: LobbyHost) {
    info!(target: "lobby", "the host closed the lobby");
    if let Some(net) = host.net.take() {
        net.with_server(|server| server.close_lobby(crate::solo::now(), CLOSED));
        net.flush();
    }
    let mut ui = world.resource_mut::<Ui>();
    let ui = &mut *ui;
    ui.vm.lobby = None;
    ui.stack.clear();
    ui.stack.open("main_menu", &ui.vm);
}

/// Start: the guests are told, the lobby's settings are fixed, and the
/// world loads with three seats on the session's thread.
fn start(world: &mut World, mut host: LobbyHost) {
    let Some(mut net) = host.net.take() else {
        return;
    };
    net.seat_slots(seat_slots(&host.lobby));
    net.with_server(net::session::Server::start);
    crate::settings::fix_session_collision();
    let save = match host.started_save() {
        Ok(save) => save,
        Err(why) => {
            warn!(target: "lobby", %why, "the lobby's party could not be written");
            host.chosen.save.clone()
        }
    };
    info!(target: "lobby", party = ?host.lobby.party(), seats = ?host.lobby.seats(), "the host started the game");
    let chosen = &host.chosen;
    let image = Some(format!(
        "load_{}",
        (save.as_ref().and_then(|save| save.snapshot.place.as_ref())).map_or_else(
            || ui::intent::new_game_module(chosen.game).to_string(),
            |place| place.module.clone()
        )
    ));
    world.resource_mut::<crate::character::Made>().0 = chosen.character.clone();
    {
        let mut ui = world.resource_mut::<Ui>();
        ui.vm.lobby = None;
    }
    let job = crate::session_flow::host_from_lobby(
        save,
        chosen.character.clone(),
        chosen.game,
        host.port,
        net,
    );
    crate::session_flow::show_job(world, job, image);
}

/// A lobby started with no player (`kjk host --lobby`): the save with the
/// lobby's party written in, the character it was made with, and the
/// network with the guests kept.
pub struct Started {
    pub save: Option<sim::Save>,
    pub character: Option<CharacterDesc>,
    pub net: OpenNet,
}

/// How long a lobby with no player waits for its guests.
const GATHER_WAIT: std::time::Duration = std::time::Duration::from_secs(180);

/// The co-op lobby on save `id` with no player (`kjk host --lobby`, the
/// lobby's three-process run): it opens on `port`, waits until `guests`
/// guests are in it and all of them are Ready, and starts, as the host's
/// Start Game does.
pub fn headless(
    discovery: &vfs::Discovery,
    id: &str,
    port: u16,
    guests: usize,
    status: &dyn Fn(String),
) -> Result<Started, String> {
    let chosen = choose(discovery, Some(id.to_string()), None, None)?;
    let mut host = open(discovery, chosen, port, status)?;
    outln!("lobby open on {}", host.address);
    let started = std::time::Instant::now();
    loop {
        host.take_network();
        host.send();
        let joined: Vec<bool> = (host.lobby.slots[1..].iter().flatten())
            .map(|slot| slot.ready)
            .collect();
        if joined.len() >= guests && joined.iter().all(|ready| *ready) {
            break;
        }
        if started.elapsed() > GATHER_WAIT {
            return Err(format!(
                "the lobby waited {} s for {guests} guests to be Ready: {} came",
                GATHER_WAIT.as_secs(),
                joined.len()
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let mut net = host.net.take().ok_or("the lobby has no network")?;
    net.seat_slots(seat_slots(&host.lobby));
    net.with_server(net::session::Server::start);
    crate::settings::fix_session_collision();
    let save = host.started_save()?;
    outln!(
        "lobby started: party {:?}, seats {:?}",
        host.lobby.party(),
        host.lobby.seats()
    );
    Ok(Started {
        save,
        character: host.chosen.character.clone(),
        net,
    })
}

#[cfg(feature = "probes")]
pub use probes::probes;

#[cfg(feature = "probes")]
mod probes {
    use super::*;
    use session::lobby::Pick;
    use sim::script::calls::{self, Queued};
    use sim::script::party::Npc;

    /// The kitchen sink's save with the companions 0, 1 and 2 recruited and
    /// 0 and 1 in the party.
    fn save() -> sim::Save {
        let (_, mut save) = sim::fixtures::kitchen_sink();
        let party = &mut save.script.party;
        for npc in 0..3u8 {
            party.available.insert(
                npc,
                Npc {
                    template: format!("p_probe{npc}"),
                    object: None,
                    kept: None,
                    template_data: None,
                },
            );
        }
        party.members = vec![0, 1];
        save
    }

    fn character() -> CharacterDesc {
        CharacterDesc {
            class: 0,
            female: false,
            portrait: "po_pmhc01".into(),
            abilities: [10; 6],
            skills: [0; 8],
            name: "Dax Tyrell".into(),
            feats: Vec::new(),
        }
    }

    /// The lobby on `save()` with two guests.
    fn lobby() -> Lobby {
        let save = save();
        let character = character();
        let (summary, companions, current) =
            summarize(Some(&save), None, Some(&character), Install::Kotor1, None);
        let mut lobby = Lobby::new("host".into(), "ja".into(), Collision::Limited);
        lobby.reset(summary, companions, &current);
        lobby.join("Mia".into());
        lobby.join("Kalle".into());
        lobby
    }

    /// The party changes `save` has queued, each as its last routine and
    /// its companion.
    fn queued(save: &sim::Save) -> Vec<(String, u8)> {
        (save.script.pending.iter())
            .filter_map(|pending| calls::steps(&pending.script)?.ok())
            .filter_map(|steps| {
                let npc = steps.iter().find_map(|step| match step {
                    Queued::Int(npc) => u8::try_from(*npc).ok(),
                    _ => None,
                })?;
                match steps.last()? {
                    Queued::Routine(routine, _) => Some((routine.clone(), npc)),
                    _ => None,
                }
            })
            .collect()
    }

    fn kept(save: &sim::Save) -> Vec<(u8, u8)> {
        (save.snapshot.kept.iter())
            .map(|kept| (kept.seat.0, kept.npc))
            .collect()
    }

    /// A case: its name, what happens in the lobby, the party changes
    /// queued, the seats' companions, and the players the module is
    /// planned for.
    type Case = (
        &'static str,
        fn(&mut Lobby),
        &'static [(&'static str, u8)],
        &'static [(u8, u8)],
        u8,
    );

    /// A part of the story: its check's name, how it changes the save, the
    /// hero's name, the companions the lobby lists, and the companion
    /// leading after Start.
    type StoryCase = (
        &'static str,
        fn(&mut sim::Save),
        &'static [u8],
        &'static [u8],
        Option<u8>,
    );

    /// The checks (`kjk sim probes --only "lobby host"`): the save described
    /// for the lobby, and the party and seats Start writes into it.
    pub fn probes() -> sim::probe::Checks {
        let mut checks = Vec::new();
        let (summary, companions, current) = summarize(
            Some(&save()),
            None,
            Some(&character()),
            Install::Kotor1,
            None,
        );
        let npcs: Vec<(u8, Choice)> = companions.iter().map(|c| (c.npc, c.choice)).collect();
        // A switched character outside the party (KOTOR 2's Visas Marr on
        // Dxun); a companion the player chose to lead while the main
        // character stands in the party; and a companion leading while the
        // main character is away (as when the story takes them): the hero,
        // the companions the lobby lists, the companions whose party
        // changes Start queues, and the companion leading after Start.
        let story_cases: [StoryCase; 3] = [
            (
                "lobby_shows_the_switched_character_as_the_hero",
                |save| save.script.party.switched = Some(2),
                b"p_probe2",
                &[0, 1],
                None,
            ),
            (
                "lobby_offers_a_companion_the_player_chose_to_lead",
                |save| save.script.party.leader = Some(1),
                b"Dax Tyrell",
                &[0, 1, 2],
                None,
            ),
            (
                "lobby_shows_the_leading_companion_while_the_main_character_is_away",
                |save| {
                    save.script.party.leader = Some(1);
                    let main = sim::Kind::Avatar(sim::Seat(0));
                    save.snapshot.entities.retain(|entity| entity.kind != main);
                },
                b"p_probe1",
                &[0, 2],
                Some(1),
            ),
        ];
        for (name, story, want_hero, want_listed, want_leader) in story_cases {
            let mut save = save();
            story(&mut save);
            let (summary, companions, current) =
                summarize(Some(&save), None, Some(&character()), Install::Kotor2, None);
            let listed: Vec<u8> = companions.iter().map(|c| c.npc).collect();
            let mut lobby = Lobby::new("host".into(), "ja".into(), Collision::Limited);
            lobby.reset(summary.clone(), companions, &current);
            let after = started(&save, &lobby).map(|after| {
                let queued: Vec<u8> = queued(&after).into_iter().map(|(_, npc)| npc).collect();
                (queued, after.script.party.leader)
            });
            checks.push((
                name,
                sim::probe::check(
                    summary.hero == want_hero
                        && listed == want_listed
                        && after == Ok((Vec::new(), want_leader)),
                    format!(
                        "hero {:?}, companions {listed:?}, queued and leading after Start {after:?}",
                        fmt_odyssey::code_page(&summary.hero)
                    ),
                ),
            ));
        }
        // The lost box's Load is the machine the world runs on: the host's,
        // whatever seat its player has, and never a guest's.
        let here = crate::sim_host::WorldHere { alone: false };
        let shown = [
            crate::ui_view::lost_view_host(Some(&here)),
            crate::ui_view::lost_view_host(None),
        ];
        checks.push((
            "lobby_lost_box_offers_load_on_the_host_only",
            sim::probe::check(
                shown == [true, false],
                format!("the host's machine and a guest's: {shown:?}"),
            ),
        ));
        // A save with one companion: Mia plays it, Kalle flies a droid, and
        // Kalle takes the main character, so the host flies the droid. The
        // module is planned for the two players with a body.
        let mut one = save();
        one.script.party.available.retain(|npc, _| *npc == 0);
        one.script.party.members = vec![0];
        let described = summarize(Some(&one), None, Some(&character()), Install::Kotor1, None);
        let mut droid = Lobby::new("host".into(), "ja".into(), Collision::Limited);
        droid.reset(described.0, described.1, &described.2);
        droid.join("Mia".into());
        droid.join("Kalle".into());
        droid.choose(2, Pick::Hero, true);
        let planned = started(&one, &droid).map(|after| {
            let players = after.snapshot.place.as_ref().map(|place| place.players);
            (players, kept(&after), droid.playing(0))
        });
        checks.push((
            "lobby_scaling_counts_no_host_who_flies_a_droid",
            match planned {
                Ok(seen) => sim::probe::check(
                    seen == (Some(2), vec![(2, 0)], Some(Playing::Droid)),
                    format!("planned for, seats' companions and the host's character {seen:?}"),
                ),
                Err(why) => Err(why),
            },
        ));
        checks.push((
            "lobby_describes_the_save",
            sim::probe::check(
                summary.hero == b"Dax Tyrell"
                    && npcs == [(0, Choice::Open), (1, Choice::Open), (2, Choice::Open)]
                    && current == [0, 1],
                format!(
                    "hero {:?}, companions {npcs:?}, party {current:?}",
                    summary.hero
                ),
            ),
        ));
        let cases: [Case; 4] = [
            (
                "lobby_start_keeps_the_party",
                |_| {},
                &[],
                &[(1, 0), (2, 1)],
                3,
            ),
            (
                "lobby_start_writes_a_swap_and_a_pick",
                |lobby| {
                    lobby.choose(1, Pick::Companion(1), true);
                    lobby.choose(2, Pick::Companion(2), false);
                },
                &[("RemovePartyMember", 0), ("AddPartyMember", 2)],
                &[(1, 1), (2, 2)],
                3,
            ),
            (
                "lobby_start_seats_a_guest_who_plays_the_main_character_first",
                |lobby| {
                    lobby.choose(2, Pick::Hero, true);
                },
                &[],
                &[(1, 1), (2, 0)],
                3,
            ),
            (
                "lobby_start_seats_no_guest_who_left",
                |lobby| lobby.leave(2),
                &[("RemovePartyMember", 1)],
                &[(1, 0)],
                2,
            ),
        ];
        for (name, change, want_queued, want_kept, want_players) in cases {
            let mut lobby = lobby();
            change(&mut lobby);
            let result = started(&save(), &lobby).and_then(|after| {
                let (got_queued, got_kept) = (queued(&after), kept(&after));
                let players = after.snapshot.place.as_ref().map(|place| place.players);
                let want: Vec<(String, u8)> = want_queued
                    .iter()
                    .map(|(routine, npc)| (routine.to_string(), *npc))
                    .collect();
                sim::probe::check(
                    got_queued == want && got_kept == want_kept && players == Some(want_players),
                    format!(
                        "queued {got_queued:?}, seats {got_kept:?}, planned for {players:?} players"
                    ),
                )
            });
            checks.push((name, result));
        }
        checks
    }
}
