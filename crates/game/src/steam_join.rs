//! Steam in the menu's game: opened as the menu starts, so friends' lists
//! show this player in a game, and a friend's "Join Game" or invite (the
//! host's connect string, `net::io::steam::join_requests`) joins that host
//! from the main menu, as if its `steam:` address was typed on the Join
//! screen. During play it is refused with why: the game is left first.

use bevy::prelude::*;
use net::io::steam;
use ui::intent::{Intent, SessionIntent};

use crate::ui_view::{Ui, UiIntents};

pub struct SteamJoinPlugin;

impl Plugin for SteamJoinPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, open).add_systems(Update, joins);
    }
}

fn open() {
    match steam::client() {
        Ok(client) => info!(
            target: "steam",
            id = client.user().steam_id().raw(),
            "Steam open: friends join this player's games, and this player theirs"
        ),
        Err(why) => info!(target: "steam", why, "no Steam: games are joined by address"),
    }
}

/// Steam's join requests, as Join intents from the main menu.
fn joins(
    mut ui: ResMut<Ui>,
    mut intents: ResMut<UiIntents>,
    playing: Option<Res<crate::sim_host::OwnSeat>>,
) {
    let Ok(client) = steam::client() else {
        return;
    };
    steam::pump(client);
    for host in steam::join_requests() {
        if playing.is_some() {
            ui.vm
                .feedback
                .push("Leave this game for the main menu to join a friend's.".into());
            continue;
        }
        info!(target: "steam", host, "a friend's game, joined from Steam");
        intents.0.push(Intent::Session(SessionIntent::Join {
            address: net::steam_address(host),
            name: steam::persona_name().unwrap_or_else(|| ui::strings::DEFAULT_NAME.into()),
        }));
    }
}
