//! The holster key (H) from the player's side: in play, with no screen,
//! conversation or movie up, it puts the weapon in hand away
//! (`Action::Holster`, `sim::combat::Sheath::Holstered`): a lightsaber out,
//! a blade sheathed, a gun put away, until the player attacks or changes
//! weapons. The console's `ui holster` sends it as it is.

use bevy::prelude::*;
use sim::action::Action;

use crate::leader_view::{Busy, busy};
use crate::loot::Outgoing;
use crate::settings::KeyMap;

pub struct HolsterViewPlugin;

impl Plugin for HolsterViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, holster.after(crate::ui_view::Stepped));
    }
}

fn holster(
    (keys, map): (Option<Res<ButtonInput<KeyCode>>>, Option<Res<KeyMap>>),
    taken: Busy,
    outgoing: Option<ResMut<Outgoing>>,
) {
    let busy = busy(taken);
    let map = map.map_or_else(KeyMap::default, |map| map.clone());
    let pressed = keys.is_some_and(|keys| map.pressed_now(&keys, crate::settings::Action::Holster));
    if let (false, true, Some(mut outgoing)) = (busy, pressed, outgoing) {
        info!(target: "weapons", "holster");
        outgoing.0.push(Action::Holster);
    }
}
