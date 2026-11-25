//! A melee feat attack chosen on the action bar for a single blade
//! (`Action::Feat`). A character with one sword, vibroblade or lightsaber
//! fights in a saber style, and each style uses a feat (`combat::picks`).
//! Choosing Flurry switches the blade to the fast style, Critical Strike to
//! the medium style and Power Attack to the strong style
//! (`Picks::switches_to`).
//!
//! The switch goes the way of the saber style key's: the style is queued,
//! and the saber step makes it the fighter's style once the weapon timer
//! has run out (`motion::saber`). A style the blade cannot fight in, or a
//! feat the character does not know, changes nothing. Two blades, a
//! double-bladed weapon, fists and a stun baton keep their feat choice
//! (`Picks::set`, `scripts::seats`).

use bevy_ecs::prelude::*;

use super::{Avatar, Fighting, Inputs, Object, PlayerSheet, sheet_of};
use crate::action::Action;
use crate::combat::picks::Picks;

/// The seats' melee feat attacks chosen for a single blade. A seat chooses
/// for the body it plays, in the hand that body fights with.
pub(super) fn feat_styles(
    inputs: Res<Inputs>,
    mut fighters: Query<(
        &Avatar,
        Option<&PlayerSheet>,
        Option<&Object>,
        &mut Fighting,
    )>,
) {
    for (seat, action) in inputs.0.actions() {
        let Action::Feat { hand, line } = *action else {
            continue;
        };
        let Some((_, sheet, object, mut fighting)) =
            fighters.iter_mut().find(|(avatar, ..)| avatar.0 == *seat)
        else {
            continue;
        };
        if fighting.0.hand != hand {
            continue;
        }
        let sheet = sheet_of(sheet, object);
        if let Some(style) = Picks::switches_to(&sheet, hand, line) {
            fighting.0.saber.queued = Some(style);
        }
    }
}
