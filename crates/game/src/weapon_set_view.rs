//! The weapon swap from the player's side (TSL's second weapon set, in both
//! games): the swap key (B; TSL's own is H, the holster's here) or a click
//! on the HUD's swap button (`ui::screens::hud::swap_at`, while the pointer
//! is free; the console's `ui swap`), in play with no screen, conversation
//! or movie up, exchanges the sets of the seat's body (`Action::SwitchWeapons`).
//! As TSL's client, a second must pass between two swaps sent
//! (`CSWCCreature::ActionMenuSwitchWeapons`' 1000 ms). The equip screen's
//! swap button is the screen's own (`GameAction::SwitchWeapons`).

use bevy::prelude::*;
use sim::action::Action;

use crate::leader_view::{Busy, busy};
use crate::loot::Outgoing;
use crate::settings::KeyMap;

/// The client's wait between two swaps sent (s).
const COOLDOWN_S: f32 = 1.0;

/// The HUD's swap button clicked this frame.
#[derive(Resource, Default)]
pub struct SwapAsked(pub bool);

pub struct WeaponSetViewPlugin;

impl Plugin for WeaponSetViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SwapAsked>()
            .add_systems(Update, swap.after(crate::ui_view::Stepped));
    }
}

fn swap(
    (keys, map): (Option<Res<ButtonInput<KeyCode>>>, Option<Res<KeyMap>>),
    taken: Busy,
    (mut asked, time): (ResMut<SwapAsked>, Res<Time>),
    mut since: Local<Option<f32>>,
    outgoing: Option<ResMut<Outgoing>>,
) {
    let clicked = std::mem::take(&mut asked.0);
    let now = time.elapsed_secs();
    let map = map.map_or_else(KeyMap::default, |map| map.clone());
    let pressed =
        keys.is_some_and(|keys| map.pressed_now(&keys, crate::settings::Action::SwitchWeapons));
    let ready = since.is_none_or(|sent| now - sent >= COOLDOWN_S);
    if let (true, false, true, Some(mut outgoing)) =
        (pressed || clicked, busy(taken), ready, outgoing)
    {
        info!(target: "weapons", "swap weapon sets");
        outgoing.0.push(Action::SwitchWeapons);
        *since = Some(now);
    }
}
