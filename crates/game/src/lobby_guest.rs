//! The co-op lobby on a guest (`docs/superpowers/specs/2026-11-15-coop-lobby-design.md`).
//!
//! The guest's join runs on the session's thread (`net_join::attempt`).
//! After the handshake, a host with a lobby sends its state instead of a
//! welcome. The join thread keeps the connection answered, puts each state
//! the host sends here, and sends what the guest's player chose. The
//! screens read the state each frame (`frame`) and show the lobby screen.
//!
//! When the host starts the game, the lobby screen closes and the loading
//! screen says "The host is loading the game." The join goes on as before:
//! the welcome, the level and the first snapshot follow once the host's
//! world is loaded. When the host closes the lobby, the join ends with the
//! host's reason, which the message screen shows over the main menu. When
//! the guest leaves, the join ends quietly and the main menu opens.

use std::sync::{Mutex, MutexGuard, PoisonError};

use bevy::prelude::*;
use session::lobby::{Lobby, Pick, Playing};
use ui::intent::{Intent, LobbyIntent};

use crate::ui_view::{Ui, UiIntents};

/// The error a join ends with when its player left the lobby: the flow
/// opens the main menu without a message (`session_flow::finish`).
pub const LEFT: &str = "The player left the co-op lobby.";

/// What the join thread and the screens share.
#[derive(Default)]
struct Bridge {
    /// The lobby as the host last sent it, and this player's slot in it.
    lobby: Option<(u8, Lobby)>,
    /// The host started the game.
    starting: bool,
    /// What this player chose, for the join thread to send.
    outbox: Vec<LobbyIntent>,
}

static BRIDGE: Mutex<Option<Bridge>> = Mutex::new(None);

/// What a guest with no player does in the lobby (`kjk join
/// --lobby-script`, the lobby's three-process run): marks Ready, or first
/// swaps characters with the other guest once one is there, then marks
/// Ready.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Script {
    Ready,
    Swap,
}

/// The script, and the companion it swapped for once it did.
static SCRIPT: Mutex<Option<(Script, Option<u8>)>> = Mutex::new(None);

/// This run's guest follows `script` in the lobby.
pub fn script(script: Script) {
    *SCRIPT.lock().unwrap_or_else(PoisonError::into_inner) = Some((script, None));
}

/// What the script does with the lobby as the host sent it: its choices
/// put in the outbox, as a player's would be.
fn scripted(own: u8, lobby: &Lobby, outbox: &mut Vec<LobbyIntent>) {
    let mut script = SCRIPT.lock().unwrap_or_else(PoisonError::into_inner);
    let Some((kind, swapped)) = script.as_mut() else {
        return;
    };
    let own = usize::from(own);
    if *kind == Script::Swap && swapped.is_none() {
        let theirs = (lobby.seats().into_iter()).find_map(|(slot, _, playing)| match playing {
            Playing::Companion(npc) if slot != own && slot != 0 => Some(npc),
            _ => None,
        });
        if let Some(npc) = theirs {
            *swapped = Some(npc);
            outbox.push(LobbyIntent::Choose(npc));
        }
        return;
    }
    let done = swapped.is_none_or(|npc| lobby.playing(own) == Some(Playing::Companion(npc)));
    let ready = lobby
        .slots
        .get(own)
        .and_then(Option::as_ref)
        .is_some_and(|slot| slot.ready);
    if done && !ready {
        outbox.push(LobbyIntent::Ready(true));
    }
}

fn bridge() -> MutexGuard<'static, Option<Bridge>> {
    BRIDGE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The join thread received the host's lobby, with this player's slot.
pub(crate) fn received(slot: u8, lobby: Lobby) {
    let mut shared = bridge();
    let shared = shared.get_or_insert_with(Bridge::default);
    scripted(slot, &lobby, &mut shared.outbox);
    shared.lobby = Some((slot, lobby));
}

/// The host started the game.
pub(crate) fn starting() {
    if let Some(shared) = bridge().as_mut() {
        shared.starting = true;
    }
}

/// The join left the lobby phase: the game began, or the join ended.
pub(crate) fn ended() {
    *bridge() = None;
}

/// What this player chose, as the join thread sends it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chosen {
    /// The story's character or a companion, and whether another player
    /// plays them (the two swap).
    Pick {
        pick: Pick,
        swap: bool,
    },
    Ready(bool),
    Leave,
}

/// Whether the player in slot `own` asking for `pick` asks to swap: another
/// player plays it. The story's character always has a player.
fn swaps(lobby: &Lobby, own: u8, pick: Pick) -> bool {
    let wanted = match pick {
        Pick::Hero => Playing::Hero,
        Pick::Companion(npc) => Playing::Companion(npc),
    };
    (lobby.seats().iter()).any(|(slot, _, playing)| *slot != usize::from(own) && *playing == wanted)
}

/// What this player chose since the join thread last asked.
pub(crate) fn chosen() -> Vec<Chosen> {
    let mut shared = bridge();
    let Some(shared) = shared.as_mut() else {
        return Vec::new();
    };
    let lobby = shared.lobby.as_ref();
    let outbox = std::mem::take(&mut shared.outbox);
    outbox
        .into_iter()
        .filter_map(|intent| match intent {
            LobbyIntent::Choose(npc) => {
                let pick = Pick::Companion(npc);
                Some(Chosen::Pick {
                    pick,
                    swap: lobby.is_some_and(|(own, lobby)| swaps(lobby, *own, pick)),
                })
            }
            LobbyIntent::Ready(on) => Some(Chosen::Ready(on)),
            LobbyIntent::Leave => Some(Chosen::Leave),
            _ => None,
        })
        .collect()
}

/// The guest's lobby each frame: a new state shown, the lobby screen
/// opened over the loading screen, this player's choices taken for the join
/// thread, and the screen closed when the host starts the game.
pub(crate) fn frame(world: &mut World) {
    if world.contains_resource::<crate::lobby_host::LobbyHost>() {
        return;
    }
    let (state, starting) = {
        let shared = bridge();
        let Some(shared) = shared.as_ref() else {
            return;
        };
        (shared.lobby.clone(), shared.starting)
    };
    let taken: Vec<Intent> = std::mem::take(&mut world.resource_mut::<UiIntents>().0);
    let mut rest = Vec::new();
    let mut chosen = Vec::new();
    for intent in taken {
        match intent {
            Intent::Lobby(intent) => chosen.push(intent),
            other => rest.push(other),
        }
    }
    world.resource_mut::<UiIntents>().0 = rest;
    let leaving = chosen.contains(&LobbyIntent::Leave);
    if let Some(shared) = bridge().as_mut() {
        shared.outbox.extend(chosen);
    }
    let mut ui = world.resource_mut::<Ui>();
    let ui = &mut *ui;
    if starting || leaving {
        if ui.vm.lobby.take().is_some() {
            ui.stack.close("lobby");
            ui.vm.session.status = ui::strings::LOBBY_LOADING_HOST.to_string();
        }
        return;
    }
    let Some((slot, lobby)) = state else {
        return;
    };
    ui.vm.lobby = Some(crate::lobby_host::view(&lobby, usize::from(slot)));
    if !ui.stack.has("lobby") {
        ui.stack.open("lobby", &ui.vm);
    }
}
