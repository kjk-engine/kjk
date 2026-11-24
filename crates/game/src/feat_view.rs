//! Feat attacks from the player's side (`sim::combat::picks`). In play with
//! no screen, conversation or movie up, the next-feat key moves on the feat
//! the weapon in hand carries (`Action::NextFeat`). An action bar slot that
//! holds a feat attack chooses it (`bar_view`, `Action::Feat`). Either way a
//! label over the slot names the feat attack now in use
//! (`pick_view::Picking::say`). The HUD shows the one in use (`hud`).

use bevy::prelude::*;
use sim::action::Action;
use sim::combat::picks::{Picks, Shown};
use sim::notice::Notice;

use crate::loot::{Outgoing, words};
use crate::pick_view::{LabelOn, Picking};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

pub struct FeatViewPlugin;

impl Plugin for FeatViewPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, feat_choice.after(crate::ui_view::Stepped));
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn feat_choice(
    (actions, bar_use): (
        Option<Res<crate::pad::Actions>>,
        Option<Res<crate::bar_view::BarUse>>,
    ),
    // `leader_view::Busy` but the screens, which this names words with.
    (replies, showing): (
        Option<Res<crate::dialogue_view::ReplyInput>>,
        Option<Res<crate::movie_view::Showing>>,
    ),
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    (outgoing, picking, ui): (
        Option<ResMut<Outgoing>>,
        Option<ResMut<Picking>>,
        Option<ResMut<Ui>>,
    ),
    time: Res<Time<Real>>,
) {
    let busy = crate::pick_view::busy(ui.as_deref(), replies.as_deref(), showing.as_deref());
    let pressed = actions.is_some_and(|actions| actions.pressed(crate::settings::Action::NextFeat));
    // The feat attack a bar slot chose this frame; `bar_view` sent it.
    let chosen = bar_use.and_then(|bar_use| {
        bar_use.actions.iter().find_map(|action| match action {
            Action::Feat { line, .. } => Some(*line),
            _ => None,
        })
    });
    if busy || !pressed && chosen.is_none() {
        return;
    }
    let (
        Some(presented),
        Some(own),
        Some(swap),
        Some(mut outgoing),
        Some(mut picking),
        Some(mut ui),
    ) = (presented, own, swap, outgoing, picking, ui)
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
    let changed = match chosen {
        Some(line) => Picks::set(&mut sheet, hand, line),
        None => Picks::next(&mut sheet, hand),
    };
    if !changed {
        return;
    }
    if chosen.is_none() {
        outgoing.0.push(Action::NextFeat { hand });
    }
    let said = match Picks::shown(&sheet, hand) {
        Some(Shown::Feat(row)) => ui
            .assets
            .as_mut()
            .and_then(|assets| assets.cell("feat", usize::from(row), "name"))
            .and_then(|strref| strref.parse().ok())
            .map(|strref| words(&Notice::Line(strref), Some(&mut ui))),
        _ => Some("No feat attack".to_string()),
    };
    if let Some(said) = said {
        picking.say(
            LabelOn::Feat,
            fmt_odyssey::code_page_bytes(&said),
            time.elapsed_secs(),
        );
    }
}
