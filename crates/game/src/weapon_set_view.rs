//! The weapon swap from the player's side (TSL's second weapon set, in both
//! games): the swap key (B; TSL's own is H, the holster's here; a pad's
//! LB with the D-pad's up; the console's `ui swap`), in play with no
//! screen, conversation or movie up, exchanges the sets of the seat's body
//! (`Action::SwitchWeapons`). The HUD has no swap button: the feat slot
//! stands on the action bar instead (`ui::screens::hud::FEAT`).
//! As TSL's client, a second must pass between two swaps sent
//! (`CSWCCreature::ActionMenuSwitchWeapons`' 1000 ms). The equip screen's
//! swap button is the screen's own (`GameAction::SwitchWeapons`).

use bevy::prelude::*;
use sim::action::Action;

use crate::leader_view::{Busy, busy};
use crate::loot::Outgoing;

/// The client's wait between two swaps sent (s).
const COOLDOWN_S: f32 = 1.0;

pub struct WeaponSetViewPlugin;

impl Plugin for WeaponSetViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, swap.after(crate::ui_view::Stepped));
    }
}

fn swap(
    actions: Option<Res<crate::pad::Actions>>,
    taken: Busy,
    time: Res<Time>,
    mut since: Local<Option<f32>>,
    outgoing: Option<ResMut<Outgoing>>,
) {
    let now = time.elapsed_secs();
    let pressed =
        actions.is_some_and(|actions| actions.pressed(crate::settings::Action::SwitchWeapons));
    let ready = since.is_none_or(|sent| now - sent >= COOLDOWN_S);
    if let (true, false, true, Some(mut outgoing)) = (pressed, busy(taken), ready, outgoing) {
        info!(target: "weapons", "swap weapon sets");
        outgoing.0.push(Action::SwitchWeapons);
        *since = Some(now);
    }
}
