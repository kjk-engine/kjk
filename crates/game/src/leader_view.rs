//! Who leads the party, from the player's side (`docs/WORLD-STATE.md`,
//! "The leader"): KOTOR's leader key (Tab, its `ChangeChar`) makes the next
//! party member lead, and a click on a companion's portrait beside the
//! HUD's own (`ui::screens::hud::portrait_at`, while the pointer is free;
//! the console's `ui portrait 2|3`) makes that one lead. The party as the
//! HUD lists it (`cast::party_of`) starts at the one leading, so the next is
//! its second. The action (`Action::Lead`) names the member by its body;
//! only the first seat leads (the party table is the host's), so a guest's
//! key and clicks do nothing. A member down or dead, or a companion a
//! guest plays, is not picked (the key goes on to the next). The
//! dump's `lead`: who leads, and the party in the HUD's order.

use bevy::prelude::*;
use sim::Seat;
use sim::action::Action;

use crate::loot::Outgoing;
use crate::settings::KeyMap;
use crate::sim_host::{OwnSeat, PresentedSnapshot};
use crate::ui_view::Ui;

/// The HUD's companion portrait clicked this frame, by its body
/// (`ui::vm::MemberView::id`: the HUD's own list, which may leave out a
/// member `cast::party_of` has).
#[derive(Resource, Default)]
pub struct Picked(pub Option<u32>);

pub struct LeaderViewPlugin;

impl Plugin for LeaderViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picked>()
            .add_systems(Update, lead.after(crate::ui_view::Stepped));
    }
}

/// What may take a play key (the leader's, the holster's) from play: a
/// screen, a conversation, a movie.
pub(crate) type Busy<'w> = (
    Option<Res<'w, Ui>>,
    Option<Res<'w, crate::dialogue_view::ReplyInput>>,
    Option<Res<'w, crate::movie_view::Showing>>,
);

/// Whether one of `Busy` is up.
pub(crate) fn busy((ui, replies, showing): Busy) -> bool {
    ui.is_some_and(|ui| ui.stack.is_open())
        || replies.is_some_and(|replies| replies.active)
        || showing.is_some_and(|showing| showing.0)
}

fn lead(
    (keys, map): (Option<Res<ButtonInput<KeyCode>>>, Option<Res<KeyMap>>),
    taken: Busy,
    (presented, own): (Option<Res<PresentedSnapshot>>, Option<Res<OwnSeat>>),
    mut picked: ResMut<Picked>,
    outgoing: Option<ResMut<Outgoing>>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let clicked = picked.0.take();
    let (Some(presented), Some(own)) = (presented, own) else {
        return;
    };
    let snapshot = presented.0.current();
    let party = crate::cast::party_of(snapshot, own.0);
    facts.0.insert(
        "lead".into(),
        serde_json::json!({
            "leading": snapshot.leading.map(|id| id.0),
            "party": party.iter().map(|member| member.id.0).collect::<Vec<_>>(),
        }),
    );
    if own.0 != Seat(0) {
        return;
    }
    let busy = busy(taken);
    let map = map.map_or_else(KeyMap::default, |map| map.clone());
    let key =
        !busy && keys.is_some_and(|keys| map.pressed_now(&keys, crate::settings::Action::Leader));
    let member = match clicked {
        Some(id) => party
            .iter()
            .find(|member| member.id.0 == id)
            .filter(|member| free(member)),
        None if key => party.iter().skip(1).find(|member| free(member)),
        None => None,
    };
    let (Some(member), Some(mut outgoing)) = (member, outgoing) else {
        return;
    };
    info!(target: "party", "lead: {} leads", member.id.0);
    outgoing.0.push(Action::Lead(member.id));
}

/// Whether `member` may lead: standing, and no guest's to play (I30 P2).
fn free(member: &sim::EntityState) -> bool {
    member
        .fighter
        .is_none_or(|fighter| fighter.alive() && fighter.mind.driver.is_none())
}
