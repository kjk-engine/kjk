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
    LobbyCompanionState, LobbyCompanionView, LobbySaveView, LobbySlotState, LobbySlotView,
    LobbyView,
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

/// The game a save of `meta` is of: its own word, else `campaign`, else
/// KOTOR.
fn game_of(meta: Option<&saves::Meta>, campaign: Option<Install>) -> Install {
    meta.and_then(|meta| meta.game.as_deref())
        .and_then(Install::from_key)
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
    let game = game_of(meta, campaign);
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
pub fn summarize(
    save: Option<&sim::Save>,
    meta: Option<&saves::Meta>,
    character: Option<&CharacterDesc>,
    game: Install,
    mut assets: Option<&mut ui::assets::GuiAssets>,
) -> (Summary, Vec<Companion>, Vec<u8>) {
    let mut class_name = |row: u8| -> Vec<u8> {
        (assets.as_mut())
            .and_then(|assets| assets.cell_text("classes", usize::from(row), "name"))
            .unwrap_or_else(Vec::new)
    };
    let hero_class = character.map_or_else(Vec::new, |made| class_name(made.class));
    let hero_level = save
        .and_then(|save| {
            let body = save.snapshot.body(sim::Seat(0))?;
            save.script
                .careers
                .get(&body.id.0)
                .map(|career| of_career(career).1)
        })
        .filter(|level| *level > 0)
        .unwrap_or(1);
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
        hero: character.map_or_else(Vec::new, |made| fmt_odyssey::code_page_bytes(&made.name)),
        hero_class,
        hero_level,
        hero_portrait: character.map_or_else(String::new, |made| made.portrait.clone()),
    };
    let (companions, current) = match save {
        Some(save) => describe(save, meta, assets),
        None => (Vec::new(), Vec::new()),
    };
    (summary, companions, current)
}

/// The recruited companions of `save` as the lobby shows them, each with
/// its answer from the shared choice rule (`party_choice::choices`), and the
/// companions in its party now. The area forbids a party change when the
/// script state marks the save's module unescapable.
fn describe(
    save: &sim::Save,
    meta: Option<&saves::Meta>,
    mut assets: Option<&mut ui::assets::GuiAssets>,
) -> (Vec<Companion>, Vec<u8>) {
    let party = &save.script.party;
    let module = meta.map_or_else(String::new, |meta| sim::script::key(&meta.module));
    let locked = save.script.unescapable.contains(&module);
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
    let settings = crate::settings::Settings::read();
    let saber_rules = settings
        .value("game.saber_rules")
        .filter(|rules| *rules == "ja" || crate::settings::movie_duels())
        .unwrap_or("ja")
        .to_string();
    info!(
        target: "lobby",
        party = ?chosen.current,
        companions = ?chosen.companions.iter().map(|c| (c.npc, c.choice)).collect::<Vec<_>>(),
        "the lobby opened"
    );
    let mut lobby = Lobby::new(
        ui::strings::DEFAULT_NAME.to_string(),
        saber_rules,
        settings.collision(),
    );
    lobby.reset(
        chosen.summary.clone(),
        chosen.companions.clone(),
        &chosen.current,
    );
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
            Event::LobbyChoose { slot, npc, swap } => {
                let made = self.lobby.choose(slot, npc, swap);
                info!(target: "lobby", slot, npc, swap, made, seats = ?self.lobby.seats(), "a player chose");
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
/// it, queued for its scripts to make as it loads, and each seat's companion
/// seeded.
pub fn started(save: &sim::Save, lobby: &Lobby) -> Result<sim::Save, String> {
    let mut save = save.clone();
    queue_party(&mut save, &lobby.party())?;
    let seats: Vec<(sim::Seat, u8)> = (lobby.seats().into_iter())
        .filter_map(|(slot, playing)| match playing {
            Playing::Companion(npc) => Some((sim::Seat(slot as u8), npc)),
            _ => None,
        })
        .collect();
    save.seat_companions(&seats);
    Ok(save)
}

/// Queues the routines that make `wanted` the save's party as it loads: the
/// members not wanted leave (`RemovePartyMember`), then each wanted
/// companion not in the party is made beside the first player and joins
/// (`SpawnAvailableNPC`, `AddPartyMember`). These are the routines the games'
/// own scripts call, so the story sees an ordinary party change.
pub fn queue_party(save: &mut sim::Save, wanted: &[u8]) -> Result<(), String> {
    use sim::script::calls::{self, Call, int};
    let members = save.script.party.members.clone();
    let mut steps: Vec<Vec<Call>> = Vec::new();
    for npc in members.iter().filter(|npc| !wanted.contains(npc)) {
        steps.push(vec![
            int(i32::from(*npc)),
            Call::Routine("RemovePartyMember", 1),
        ]);
    }
    for npc in wanted.iter().filter(|npc| !members.contains(npc)) {
        let npc = i32::from(*npc);
        steps.push(vec![
            Call::Routine("GetFirstPC", 0),
            Call::Routine("GetLocation", 1),
            int(npc),
            Call::Routine("SpawnAvailableNPC", 2),
            int(npc),
            Call::Routine("AddPartyMember", 2),
        ]);
    }
    for step in steps {
        let name = calls::queued(&[step])?;
        save.script
            .pending
            .push(sim::script::Pending::new(&name, sim::script::MODULE));
    }
    Ok(())
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
            let played = (1..SLOTS).find_map(|slot| {
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
        companions,
        slots,
        address: String::new(),
        steam: false,
        movie_duels: crate::settings::movie_duels(),
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
        LobbyIntent::Choose(_)
        | LobbyIntent::Ready(_)
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
    let Some(net) = host.net.take() else {
        return;
    };
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
    let net = host.net.take().ok_or("the lobby has no network")?;
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

    type Case = (
        &'static str,
        fn(&mut Lobby),
        &'static [(&'static str, u8)],
        &'static [(u8, u8)],
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
        let cases: [Case; 3] = [
            (
                "lobby_start_keeps_the_party",
                |_| {},
                &[],
                &[(1, 0), (2, 1)],
            ),
            (
                "lobby_start_writes_a_swap_and_a_pick",
                |lobby| {
                    lobby.choose(1, 1, true);
                    lobby.choose(2, 2, false);
                },
                &[("RemovePartyMember", 0), ("AddPartyMember", 2)],
                &[(1, 1), (2, 2)],
            ),
            (
                "lobby_start_seats_no_guest_who_left",
                |lobby| lobby.leave(2),
                &[("RemovePartyMember", 1)],
                &[(1, 0)],
            ),
        ];
        for (name, change, want_queued, want_kept) in cases {
            let mut lobby = lobby();
            change(&mut lobby);
            let result = started(&save(), &lobby).and_then(|after| {
                let (got_queued, got_kept) = (queued(&after), kept(&after));
                let want: Vec<(String, u8)> = want_queued
                    .iter()
                    .map(|(routine, npc)| (routine.to_string(), *npc))
                    .collect();
                sim::probe::check(
                    got_queued == want && got_kept == want_kept,
                    format!("queued {got_queued:?}, seats {got_kept:?}"),
                )
            });
            checks.push((name, result));
        }
        checks
    }
}
