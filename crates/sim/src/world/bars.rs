//! The party's action bars, filled on their own (`combat::bar`).

use bevy_ecs::prelude::*;

use super::{Avatar, Fighting, PlayerSheet, Static, in_party};
use crate::combat::bar::Rules;

/// Settles the action bar of every party member (`Bar::settle`) at the end
/// of each step. A bar that was never filled gets its first fill, so a
/// character from an older save or a companion who just joined has a full
/// bar from its first step. When the character learned nothing new, the
/// settle only compares the sheet's powers and feats with what the bar
/// has seen, and leaves the bar as it is.
pub(super) fn settle(
    level: Res<Static>,
    mut members: Query<(&mut PlayerSheet, &Fighting, Option<&Avatar>)>,
) {
    let library = level.0.scripts().map(|library| &**library);
    // `Bar::settle` never reads whether the party carries a mine kit or a
    // device, because a fill places only powers, feat attacks, grenades
    // and medpacs. The power wheel's list passes the real answer, and the
    // items' step places devices (`item_uses::usable`).
    Rules::with(library, (false, &[]), |rules| {
        for (mut sheet, fighting, avatar) in &mut members {
            if !in_party(avatar, &fighting.0) {
                continue;
            }
            let mut bar = sheet.0.bar;
            if bar.settle(&sheet.0, rules, fighting.0.hand) {
                sheet.0.bar = bar;
            }
        }
    });
}
