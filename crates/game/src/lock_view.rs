//! Security from the player's side (render side: the presented snapshot
//! and the module's facts). The Security key (U), or the console's `ui
//! security [spike]`, asks the host to try the lock of the nearest locked
//! door or placeable in reach in front of the player
//! (`sim::action::Action::Unlock`), with a security spike of that template
//! the party carries when one is named. What the host says back is the
//! feedback log's (`loot::words`).

use bevy::prelude::*;
use sim::action::Action;
use sim::module::ObjectKind;
use sim::notice::Notice;

use crate::loot::{Outgoing, Told, words};
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// Tries asked for this frame: each with the spike's template, if one.
#[derive(Resource, Default)]
pub struct Asked(pub Vec<Option<String>>);

pub struct LockViewPlugin;

impl Plugin for LockViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Asked>()
            .add_systems(Update, (key, send).chain().after(crate::ui_view::Stepped));
    }
}

/// The Security key in play, with no screen open.
fn key(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    map: Option<Res<crate::settings::KeyMap>>,
    ui: Res<Ui>,
    mut asked: ResMut<Asked>,
) {
    let map = map.map_or_else(crate::settings::KeyMap::default, |map| map.clone());
    if keys.is_some_and(|keys| map.pressed_now(&keys, crate::settings::Action::Security))
        && !ui.stack.is_open()
    {
        asked.0.push(None);
    }
}

/// The module object index of the nearest locked door or placeable within
/// reach of `seat`'s player and in front of it.
fn faced(
    snapshot: &sim::Snapshot,
    module: &sim::module::ModuleData,
    seat: sim::Seat,
) -> Option<u32> {
    let me = snapshot.body(seat)?;
    let yaw = me.player.as_ref()?.view[1].to_radians();
    let forward = [yaw.cos(), yaw.sin()];
    snapshot
        .entities
        .iter()
        .filter_map(|entity| {
            let (
                sim::Kind::Object(kind @ (ObjectKind::Door | ObjectKind::Placeable), index),
                Some(state),
            ) = (entity.kind, entity.object)
            else {
                return None;
            };
            let facts = module.object(index as usize)?;
            (kind == facts.kind && state.locked(facts.opens)).then_some(())?;
            sim::containers::ahead(me.position, forward, facts.position)
                .map(|distance| (distance, index))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, index)| index)
}

fn send(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    mut asked: ResMut<Asked>,
    mut outgoing: ResMut<Outgoing>,
    mut told: ResMut<Told>,
    mut ui: ResMut<Ui>,
) {
    let asks = std::mem::take(&mut asked.0);
    let (Some(presented), Some(own), Some(swap)) = (presented, own, swap) else {
        return;
    };
    let Some(module) = swap.level.module() else {
        return;
    };
    let snapshot = presented.0.current();
    for spike in asks {
        let Some(object) = faced(snapshot, module, own.0) else {
            let line = Notice::Line(sim::locks::lines::NOT_LOCKED);
            told.0.push(words(&line, Some(&mut ui)));
            continue;
        };
        let carried = |template: &str| {
            snapshot
                .goods
                .carried(sim::inventory::PARTY)
                .find(|(_, item)| item.template.eq_ignore_ascii_case(template))
                .map(|(id, _)| id)
        };
        let spike = match spike.as_deref().map(carried) {
            Some(None) => {
                let line = Notice::Line(sim::locks::lines::CANNOT_USE_ITEM);
                told.0.push(words(&line, Some(&mut ui)));
                continue;
            }
            spike => spike.flatten(),
        };
        outgoing.0.push(Action::Unlock { object, spike });
    }
}
