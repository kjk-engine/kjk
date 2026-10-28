//! TSL's Forms from the player's side (`sim::combat::forms`): as KOTOR 2's
//! HUD stance column picks one (no action, no cost, in a fight too), the
//! next-Form key (V), in play with no screen, conversation or movie up,
//! picks the Form the played character knows after the one in use, in
//! `spells.2da` order and round again (`Action::Form`). The HUD shows the
//! one in use (`hud`); the game's line tells it initiated. The console's
//! `ui form <row>` sends one as it is.

use bevy::prelude::*;
use sim::action::Action;
use sim::combat::{Sheet, forms};

use crate::leader_view::{Busy, busy};
use crate::loot::Outgoing;
use crate::settings::KeyMap;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};

pub struct FormViewPlugin;

impl Plugin for FormViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, next_form.after(crate::ui_view::Stepped));
    }
}

/// The Forms `sheet` knows, in `spells.2da` order.
pub fn known(sheet: &Sheet) -> impl Iterator<Item = u16> + '_ {
    forms::ALL.filter(|form| sheet.powers.has(*form))
}

/// The Form after the one `sheet` has in use among those it knows (the
/// first, with none in use or after the last).
fn after(sheet: &Sheet) -> Option<u16> {
    known(sheet)
        .find(|form| *form > sheet.form)
        .or_else(|| known(sheet).next())
}

#[allow(clippy::too_many_arguments)]
fn next_form(
    (keys, map): (Option<Res<ButtonInput<KeyCode>>>, Option<Res<KeyMap>>),
    taken: Busy,
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    outgoing: Option<ResMut<Outgoing>>,
) {
    let busy = busy(taken);
    let map = map.map_or_else(KeyMap::default, |map| map.clone());
    let pressed = keys.is_some_and(|keys| map.pressed_now(&keys, crate::settings::Action::Form));
    if busy || !pressed {
        return;
    }
    let (Some(presented), Some(own), Some(swap), Some(mut outgoing)) =
        (presented, own, swap, outgoing)
    else {
        return;
    };
    let snapshot = presented.0.current();
    let Some((_, sheet)) = crate::cast::body_sheet(snapshot, own.0, &swap.level) else {
        return;
    };
    if let Some(form) = after(&sheet).filter(|form| *form != sheet.form) {
        info!(target: "forms", "form {form}");
        outgoing.0.push(Action::Form(form));
    }
}
