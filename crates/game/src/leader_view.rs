//! Who leads the party, from the player's side (`docs/WORLD-STATE.md`,
//! "The leader"): KOTOR's leader key (Tab, its `ChangeChar`) makes the next
//! party member lead, and a click on a companion's portrait beside the
//! HUD's own (`ui::screens::hud::portrait_at`, while the pointer is free;
//! the console's `ui portrait 2|3`) makes that one lead. The party as the
//! HUD lists it (`cast::party_of`) starts at the one leading, so the next is
//! its second. The action (`Action::Lead`) names the member by its body;
//! only the first seat leads (the party table is the host's). A member
//! down or dead, or a companion a guest plays, is not picked (the key goes
//! on to the next). For every other seat the same key and clicks mean
//! "play this character" (`Action::Play`): the seat takes a companion
//! standing that no player plays and that does not lead, from the whole
//! party its HUD lists (`cast::bar_of`). The host's machine uses it too
//! when a guest plays the main character. A guest takes no one while a
//! screen or a conversation is up, and the host refuses what this side
//! cannot see (`sim::world::drivers`). The solo key
//! (`PartyActive` in both games) turns solo mode on or off (`Action::Solo`: the
//! companions stand where they are), the first seat's alone as the leader
//! key is (the companions follow its leader). While solo mode is on, the
//! HUD shows KOTOR's solo toggle, lit, beside the portrait
//! (`Snapshot::solo`), and a click on it ends solo mode
//! (`ui::screens::hud::solo_at`). The dump's `lead`: who
//! leads, solo mode, and the party in the HUD's order.

use bevy::prelude::*;
use sim::Seat;
use sim::action::Action;

use crate::loot::Outgoing;
use crate::sim_host::{OwnSeat, PresentedSnapshot};
use crate::ui_view::Ui;

/// The HUD's companion portrait clicked this frame, by its body
/// (`ui::vm::MemberView::id`: the HUD's own list, which may leave out a
/// member `cast::party_of` has).
#[derive(Resource, Default)]
pub struct Picked(pub Option<u32>);

/// The HUD's solo mode button clicked this frame. It shows only while solo
/// mode is on, so a click ends solo mode.
#[derive(Resource, Default)]
pub struct SoloAsked(pub bool);

pub struct LeaderViewPlugin;

impl Plugin for LeaderViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Picked>()
            .init_resource::<SoloAsked>()
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
    actions: Option<Res<crate::pad::Actions>>,
    taken: Busy,
    (presented, own): (Option<Res<PresentedSnapshot>>, Option<Res<OwnSeat>>),
    (mut picked, mut solo_asked): (ResMut<Picked>, ResMut<SoloAsked>),
    mut outgoing: Option<ResMut<Outgoing>>,
    mut facts: ResMut<console::DumpFacts>,
) {
    let clicked = picked.0.take();
    let solo_clicked = std::mem::take(&mut solo_asked.0);
    let (Some(presented), Some(own)) = (presented, own) else {
        return;
    };
    let snapshot = presented.0.current();
    if facts.wanted() {
        let party = crate::cast::bar_of(snapshot, own.0);
        facts.0.insert(
            "lead".into(),
            serde_json::json!({
                "leading": snapshot.leading.map(|id| id.0),
                "solo": snapshot.solo,
                "party": party.iter().map(|member| member.id.0).collect::<Vec<_>>(),
            }),
        );
    }
    let first = own.0 == Seat(0);
    let busy = busy(taken);
    let pressed = |action| {
        !busy
            && actions
                .as_ref()
                .is_some_and(|actions| actions.pressed(action))
    };
    let key = pressed(crate::settings::Action::Leader);
    let ends_solo = solo_clicked && !busy && snapshot.solo;
    if first
        && (pressed(crate::settings::Action::Solo) || ends_solo)
        && let Some(outgoing) = outgoing.as_deref_mut()
    {
        info!(target: "party", "solo: {}", !snapshot.solo);
        outgoing.0.push(Action::Solo(!snapshot.solo));
    }
    // A guest's click while a screen or a conversation is up takes no one.
    let clicked = clicked.filter(|_| first || !busy);
    let (Some(action), Some(mut outgoing)) = (chosen(snapshot, own.0, clicked, key), outgoing)
    else {
        return;
    };
    info!(target: "party", "seat {}: {action:?}", own.0.0);
    outgoing.0.push(action);
}

/// What `own`'s leader key (`key`) or a click on a portrait (`clicked`, by
/// its body) sends, from the party its HUD lists (`cast::bar_of`). The
/// first seat leads through the member (`Action::Lead`), and every other
/// seat takes it (`Action::Play`). The key picks the first member after the
/// HUD's own that the seat may lead through or take.
fn chosen(snapshot: &sim::Snapshot, own: Seat, clicked: Option<u32>, key: bool) -> Option<Action> {
    let first = own == Seat(0);
    let free = |member: &sim::EntityState| {
        if first {
            leads(member)
        } else {
            takes(snapshot, member)
        }
    };
    let party = crate::cast::bar_of(snapshot, own);
    let member = match clicked {
        Some(id) => party
            .iter()
            .find(|member| member.id.0 == id)
            .filter(|member| free(member)),
        None if key => party.iter().skip(1).find(|member| free(member)),
        None => None,
    }?;
    Some(if first {
        Action::Lead(member.id)
    } else {
        Action::Play(member.id)
    })
}

/// The leader view's probes: a guest flying a droid takes a free companion
/// with its leader key, and with a click on its portrait
/// (`sim::drivers::droid_take`).
#[cfg(feature = "probes")]
pub fn probes() -> sim::probe::Checks {
    // A click on each of the HUD's two companion portraits in turn
    // (`ui::screens::hud::portrait_at`), until one sends an action.
    let clicked = |snapshot: &sim::Snapshot, seat: Seat| {
        let party = crate::cast::bar_of(snapshot, seat);
        party
            .iter()
            .skip(1)
            .take(2)
            .find_map(|member| chosen(snapshot, seat, Some(member.id.0), false))
    };
    vec![
        (
            "a droid's leader key takes a companion",
            sim::drivers::droid_take(|snapshot, seat| chosen(snapshot, seat, None, true)),
        ),
        (
            "a droid's portrait click takes a companion",
            sim::drivers::droid_take(clicked),
        ),
    ]
}

/// Whether the first seat may lead through `member`: standing, and no
/// guest's to play (I30 P2). The character the first seat is switched to
/// (`world::switch`) is driven by that seat itself, and takes the lead
/// back.
fn leads(member: &sim::EntityState) -> bool {
    member.fighter.is_none_or(|fighter| {
        fighter.alive() && fighter.mind.driver.is_none_or(|seat| seat == Seat(0))
    })
}

/// Whether another seat may take `member` (`Action::Play`): a companion
/// standing, played by no seat and not leading the party. The host decides
/// the rest (`sim::world::drivers`).
fn takes(snapshot: &sim::Snapshot, member: &sim::EntityState) -> bool {
    matches!(member.kind, sim::Kind::Object(..))
        && snapshot.leading != Some(member.id)
        && member
            .fighter
            .is_some_and(|fighter| fighter.alive() && fighter.mind.driver.is_none())
}
