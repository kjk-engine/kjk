//! Feat attacks from the player's side (`sim::combat::picks`): the next-feat
//! key, in play with no screen, conversation or movie up, moves on the feat
//! the weapon in hand carries (`Action::NextFeat`) and the feedback log names
//! it. The HUD shows the one in use (`hud`).

use bevy::prelude::*;
use sim::action::Action;
use sim::combat::picks::{Picks, Shown};
use sim::notice::Notice;

use crate::loot::{Outgoing, Told, words};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

pub struct FeatViewPlugin;

impl Plugin for FeatViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, next_feat.after(crate::ui_view::Stepped));
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn next_feat(
    actions: Option<Res<crate::pad::Actions>>,
    // `leader_view::Busy` but the screens, which this names words with.
    (replies, showing): (
        Option<Res<crate::dialogue_view::ReplyInput>>,
        Option<Res<crate::movie_view::Showing>>,
    ),
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    (outgoing, told, ui): (
        Option<ResMut<Outgoing>>,
        Option<ResMut<Told>>,
        Option<ResMut<Ui>>,
    ),
) {
    let busy = ui.as_ref().is_some_and(|ui| ui.stack.is_open())
        || replies.is_some_and(|replies| replies.active)
        || showing.is_some_and(|showing| showing.0);
    let pressed = actions.is_some_and(|actions| actions.pressed(crate::settings::Action::NextFeat));
    if busy || !pressed {
        return;
    }
    let (Some(presented), Some(own), Some(swap), Some(mut outgoing), Some(mut told), Some(mut ui)) =
        (presented, own, swap, outgoing, told, ui)
    else {
        return;
    };
    let snapshot = presented.0.current();
    let Some((body, mut sheet)) = crate::cast::body_sheet(snapshot, own.0, &swap.level) else {
        return;
    };
    // A double blade with one lit fights as a single blade: its style is
    // its feat.
    let Some(hand) = body
        .fighter
        .as_ref()
        .filter(|fighter| !fighter.saber.second_off)
        .map(|fighter| fighter.hand)
    else {
        return;
    };
    if !Picks::next(&mut sheet, hand) {
        return;
    }
    outgoing.0.push(Action::NextFeat { hand });
    let said = match Picks::shown(&sheet, hand) {
        Some(Shown::Feat(row)) => ui
            .assets
            .as_mut()
            .and_then(|assets| assets.cell("feat", usize::from(row), "name"))
            .and_then(|strref| strref.parse().ok())
            .map(|strref| words(&Notice::Line(strref), Some(&mut ui))),
        _ => Some("No feat attack".to_string()),
    };
    told.0.extend(said);
}
