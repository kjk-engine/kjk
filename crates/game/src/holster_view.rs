//! The draw or holster key (H) from the player's side. In play, with no
//! screen, conversation or movie up, it sends `Action::Holster`. The sim
//! puts the weapon in hand away when it is out
//! (`sim::combat::Sheath::Holstered`), until the player attacks or changes
//! weapons, and draws it when it is away (`sim::combat::Sheath::Drawn`),
//! until the key is pressed again, a conversation begins, the player
//! restrict mode holds, or no player controls the body any more. The
//! console's `ui holster` sends it as it is.

use bevy::prelude::*;
use sim::action::Action;

use crate::leader_view::{Busy, busy};
use crate::loot::Outgoing;

pub struct HolsterViewPlugin;

impl Plugin for HolsterViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, holster.after(crate::ui_view::Stepped));
    }
}

fn holster(
    actions: Option<Res<crate::pad::Actions>>,
    taken: Busy,
    outgoing: Option<ResMut<Outgoing>>,
) {
    let busy = busy(taken);
    let pressed = actions.is_some_and(|actions| actions.pressed(crate::settings::Action::Holster));
    if let (false, true, Some(mut outgoing)) = (busy, pressed, outgoing) {
        info!(target: "weapons", "holster");
        outgoing.0.push(Action::Holster);
    }
}
