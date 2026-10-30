//! Containers: placeables that hold items (`ObjectData::loot`), the dead's
//! remains among them (`remains`). A player opens one by using it
//! (`interact`); the seats holding it open are its tenants, and it shows
//! open while any is: a placeable's look open (`snapshot::look`), closed
//! once the last lets it go, as KOTOR's containers play `close2open` and
//! `open2close`. Each step, after the uses, this:
//!
//! - closes it for a seat whose player is down or out of reach;
//! - applies the seats' actions in canonical order (`TickInput::actions`):
//!   opening it (a locked one fails to, `world::locks`), closing it,
//!   taking one of its items or everything left. What it holds is its
//!   inventory as the scripts have it (`State::loot`): its template's
//!   items stocked on the module's first visit and whatever scripts gave
//!   it since, as KOTOR's containers hold their item list (K1's civilian
//!   footlockers are filled by their heartbeat, `k_plc_tresciv`). Taking
//!   needs the container open by that seat and the player within reach;
//!   the first to take an item gets it, a later take in the same step
//!   finds it gone. What is taken the authority's scripts give the party
//!   (`State::take_item`);
//! - tells the scripts (`Containers`): an item taken disturbs the
//!   container, the last seat closing it closes it.
//!
//! What a seat asked and could not have is a notice for its player
//! (`crate::notice`). A save loads with none held open (`let_go`).

use bevy_ecs::prelude::*;

use super::party::bit;
use super::{Avatar, Clock, Fighting, Id, Inputs, Object, Player, Static, StepReason};
use crate::action::Action;
use crate::input::Seat;
use crate::math;
use crate::module::ObjectKind;
use crate::notice::{Notice, Notices};
use crate::script::Hook;
use crate::snapshot::{SimId, look};

/// How far (metres, level) a player stands from a container's middle and
/// still reaches into it.
pub const REACH: f32 = rules::items::containers::REACH;
/// How far from straight ahead (its cosine) an object may be and still be
/// in front.
const FACING: f32 = rules::items::containers::FACING;

/// How far across the floor (metres) `at` is from `me`, when it is within
/// reach and in front along `forward` (across the floor, any length): what
/// a use that hits nothing opens, and what Security tries
/// (`launcher::lock_view`).
pub fn ahead(me: [f32; 3], forward: [f32; 2], at: [f32; 3]) -> Option<f32> {
    let (dx, dy) = (at[0] - me[0], at[1] - me[1]);
    let distance = crate::math::flat_distance(at, me);
    let length = (forward[0] * forward[0] + forward[1] * forward[1]).sqrt();
    let facing = (dx * forward[0] + dy * forward[1]) / (distance * length).max(f32::EPSILON);
    (distance <= REACH && facing >= FACING).then_some(distance)
}

/// What happened to containers this step, for their scripts: the
/// container, who did it, and the event (`Hook::Closed`, `Hook::Disturbed`); and each
/// item taken (the container, the item's script id).
#[derive(Resource, Default)]
pub(crate) struct Containers {
    pub events: Vec<(SimId, SimId, Hook)>,
    pub taken: Vec<(SimId, u32)>,
    /// Items put in (the container, the item's script id).
    pub put: Vec<(SimId, u32)>,
    /// Taken all: the container and by whom (`State::take_rest`); nothing
    /// in it is the scripts' `Empty`.
    pub took_all: Vec<(SimId, SimId)>,
}

/// A seat's player: its entity, where it stands (metres), whether it is up.
#[derive(Clone, Copy)]
struct Reacher {
    id: SimId,
    at: [f32; 3],
    alive: bool,
}

/// `seat` holds container `id` open, `who` its player: the first to hold
/// it opens it (its open script).
pub(super) fn hold(
    id: SimId,
    state: &mut crate::snapshot::ObjectState,
    seat: Seat,
    who: SimId,
    opened: &mut super::scripts::Opened,
) {
    if state.tenants == 0 {
        state.state = look::OPEN;
        opened.0.push((id, Some(who), true));
    }
    state.tenants |= bit(seat);
}

/// What a container of `kind` shows once no seat holds it: a placeable
/// closed, remains as they were.
fn shut(kind: ObjectKind) -> u8 {
    match kind {
        ObjectKind::Placeable => look::CLOSED,
        _ => 0,
    }
}

/// A save loaded holds no container open: KOTOR's loot screen is up only
/// while its player loots, and a load never opens it. A seat holding one
/// when the save was made (a guest looting as its host saved, a quick
/// save) lets it go and it shows closed, its scripts not told (nothing was
/// closed in play). Triggers and doorways keep their tenants.
pub fn let_go(snapshot: &mut crate::Snapshot, level: &super::Level) {
    let instances = crate::Instances::in_snapshot(level, snapshot);
    for entity in &mut snapshot.entities {
        let (crate::Kind::Object(kind, index), Some(object)) = (entity.kind, &mut entity.object)
        else {
            continue;
        };
        if object.tenants != 0 && instances.get(index).is_some_and(|facts| facts.container) {
            object.tenants = 0;
            object.state = shut(kind);
        }
    }
}

fn within(reacher: &Reacher, container: [f32; 3]) -> bool {
    let (dx, dy) = (reacher.at[0] - container[0], reacher.at[1] - container[1]);
    reacher.alive && (dx * dx + dy * dy) <= REACH * REACH
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn containers(
    clock: Res<Clock>,
    inputs: Res<Inputs>,
    level: Res<Static>,
    mut notices: ResMut<Notices>,
    mut events: ResMut<Containers>,
    mut opened: ResMut<super::scripts::Opened>,
    players: Query<(&Id, &Avatar, &Player, Option<&Fighting>)>,
    bodies: Query<(&Id, &Player, Option<&Fighting>), Without<Avatar>>,
    scripts: Res<super::scripts::Scripts>,
    mut objects: Query<(&Id, &mut Object), Without<Avatar>>,
) {
    events.events.clear();
    events.taken.clear();
    events.put.clear();
    events.took_all.clear();
    if level.0.module.is_none() {
        return;
    }
    let reacher_of = |id: &Id, player: &Player, fighting: Option<&Fighting>| Reacher {
        id: id.0,
        at: math::scale(player.0.origin, collision::UNIT),
        alive: fighting.is_none_or(|fighting| fighting.0.alive()),
    };
    // A seat reaches with the body it acts through: the companion leading
    // for the first seat (`acting_body`).
    let lent = super::lent(&scripts.state.party).and_then(|lent| {
        bodies
            .iter()
            .find(|(id, ..)| id.0 == lent)
            .map(|(id, player, fighting)| reacher_of(id, player, fighting))
    });
    let mut seats: Vec<(Seat, Reacher)> = players
        .iter()
        .map(|(id, avatar, player, fighting)| {
            let own = reacher_of(id, player, fighting);
            let reacher = match lent {
                Some(body) if avatar.0 == super::LEADER_SEAT => body,
                _ => own,
            };
            (avatar.0, reacher)
        })
        .collect();
    seats.sort_by_key(|(seat, _)| *seat);
    let reacher = |seat: Seat| {
        seats
            .binary_search_by_key(&seat, |(seat, _)| *seat)
            .ok()
            .map(|at| &seats[at].1)
    };
    // Each container with where it is.
    let mut boxes: Vec<_> = objects
        .iter_mut()
        .filter_map(|(id, object)| {
            let facts = object.facts.clone()?;
            let place = facts.container.then_some(facts.position)?;
            Some((id, object, facts, place))
        })
        .collect();
    boxes.sort_by_key(|(_, object, ..)| object.index);

    // Down or gone out of reach: closed for that seat.
    for (id, object, _, place) in &mut boxes {
        let place = *place;
        let held = object.state.tenants;
        for seat in (0..crate::MAX_SEATS)
            .map(Seat)
            .filter(|seat| held & bit(*seat) != 0)
        {
            let near = reacher(seat).is_some_and(|reacher| within(reacher, place));
            if !near {
                object.state.tenants &= !bit(seat);
                if object.state.tenants == 0 {
                    object.state.state = shut(object.kind);
                    if let Some(reacher) = reacher(seat) {
                        events.events.push((id.0, reacher.id, Hook::Closed));
                    }
                }
            }
        }
    }

    // The items taken this step, and by whom.
    let state = &scripts.state;
    let mut taken_now: Vec<(u32, Seat)> = Vec::new();
    for (seat, action) in inputs.0.actions() {
        let seat = *seat;
        // Trading, wearing, levels and the party screen are the scripts' (`world::scripts`).
        let Some(container) = action.container() else {
            continue;
        };
        let refuse = |notices: &mut Notices, notice| {
            if clock.reason != StepReason::Predict {
                notices.0.push((seat, notice));
            }
        };
        let Some(at) = boxes
            .binary_search_by_key(&container, |(_, object, ..)| object.index)
            .ok()
        else {
            refuse(&mut notices, Notice::NotOpen);
            continue;
        };
        let (id, object, facts, place) = &mut boxes[at];
        let bit = bit(seat);
        let Some(who) = reacher(seat).filter(|reacher| within(reacher, *place)) else {
            refuse(&mut notices, Notice::OutOfReach);
            continue;
        };
        let who = who.id;
        if let Action::Open(_) = action {
            if object.state.locked(facts.opens) {
                // Its key, or it stays shut (`world::locks`).
                opened.0.push((id.0, Some(who), false));
            } else {
                hold(id.0, &mut object.state, seat, who, &mut opened);
            }
            continue;
        }
        if object.state.tenants & bit == 0 {
            refuse(&mut notices, Notice::NotOpen);
            continue;
        }
        let item = match *action {
            Action::Open(_)
            | Action::Buy { .. }
            | Action::Sell { .. }
            | Action::Leave(_)
            | Action::Equip { .. }
            | Action::Unequip(_)
            | Action::EquipOn { .. }
            | Action::UnequipFrom { .. }
            | Action::LevelUp(_)
            | Action::LevelUpOn { .. }
            | Action::Unlock { .. }
            | Action::ChooseParty(_)
            | Action::ShowParty
            | Action::SetLeader(_)
            | Action::Lead(_)
            | Action::Solo(_)
            | Action::Travel(_)
            | Action::Use { .. }
            | Action::Break
            | Action::Resume
            | Action::Ready
            | Action::Force
            | Action::Revive(_)
            | Action::Pazaak(_)
            | Action::UseItem { .. }
            | Action::Fit { .. }
            | Action::Unfit { .. }
            | Action::Disarm(_)
            | Action::Recover(_)
            | Action::Mine(_)
            | Action::Holster
            | Action::Request { .. }
            | Action::Watch { .. }
            | Action::Breakdown { .. }
            | Action::Create { .. }
            | Action::SwitchWeapons
            | Action::SwitchWeaponsOn { .. }
            | Action::Assemble { .. }
            | Action::Form(_)
            | Action::Cast { .. }
            | Action::ScreenOpened
            | Action::Forfeit
            | Action::Arena(_) => continue,
            Action::Close(_) => {
                object.state.tenants &= !bit;
                if object.state.tenants == 0 {
                    object.state.state = shut(object.kind);
                    events.events.push((id.0, who, Hook::Closed));
                }
                continue;
            }
            // What the party gives it: the scripts move it (`State::put_in`),
            // then the container hears of it.
            Action::Put { item, .. } => {
                events.put.push((id.0, item));
                events.events.push((id.0, who, Hook::Disturbed));
                continue;
            }
            Action::Take { item, .. } => item,
            Action::TakeAll(_) => {
                // What is left after this step's takes goes; the scripts
                // say `Empty` when nothing did.
                let left = state
                    .loot(id.0.0)
                    .any(|(item, _)| !taken_now.iter().any(|(gone, _)| *gone == item));
                events.took_all.push((id.0, who));
                if left {
                    events.events.push((id.0, who, Hook::Disturbed));
                }
                continue;
            }
        };
        let there = state.loot(id.0.0).any(|(held, _)| held == item);
        let before = taken_now.iter().find(|(gone, _)| *gone == item);
        if !there || before.is_some() {
            // Gone to someone else this step or before it; one never in
            // it is nothing to take.
            let notice = match state.items.get(&item) {
                Some(gone) if before.is_some() || gone.holder != id.0.0 => Notice::Gone {
                    item: gone.template.clone(),
                    by: before.map(|(_, by)| *by),
                },
                _ => Notice::Empty,
            };
            refuse(&mut notices, notice);
            continue;
        }
        taken_now.push((item, seat));
        events.taken.push((id.0, item));
        events.events.push((id.0, who, Hook::Disturbed));
    }
}

#[cfg(feature = "probes")]
pub(crate) mod probes;
#[cfg(feature = "probes")]
pub(super) use probes::container;
#[cfg(feature = "probes")]
pub(crate) use probes::plain;
#[cfg(feature = "probes")]
pub use probes::probes;
