//! The messages page's combat log (render side, from the presented
//! snapshot): each event new to a character's ring (by sequence number)
//! told as a line: who struck whom for how much, a critical, a blade or a
//! bolt turned. The dialog log is `dialogue_view`'s.

use std::collections::BTreeMap;

use bevy::prelude::*;
use sim::Instances;
use sim::combat::Outcome;

use crate::dialogue_view::name_of;
use crate::sim_host::{PresentedSnapshot, SceneSwap};
use crate::ui_view::Ui;

/// How many lines a log keeps.
const MOST: usize = 300;

pub struct MessageLogPlugin;

impl Plugin for MessageLogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            combat
                .run_if(resource_exists::<PresentedSnapshot>)
                .before(crate::ui_view::Stepped),
        );
    }
}

/// A line added to a log, the oldest dropped past `MOST`.
pub fn push(log: &mut Vec<String>, line: String) {
    log.push(line);
    let over = log.len().saturating_sub(MOST);
    log.drain(..over);
}

fn combat(
    presented: Res<PresentedSnapshot>,
    swap: Option<Res<SceneSwap>>,
    ui: Option<ResMut<Ui>>,
    mut seen: Local<BTreeMap<sim::SimId, u16>>,
) {
    let (Some(swap), Some(mut ui)) = (swap, ui) else {
        return;
    };
    let snapshot = presented.0.current();
    let objects = Instances::in_snapshot(&swap.level, snapshot);
    let name = |id: sim::SimId| {
        let name =
            fmt_odyssey::code_page(&name_of(snapshot, &objects, script_ncs::vm::ObjectId(id.0)));
        if name.is_empty() {
            "Someone".to_string()
        } else {
            name
        }
    };
    for entity in &snapshot.entities {
        let Some(fighter) = entity.fighter else {
            continue;
        };
        // A character first seen has seen its ring already.
        let before = *seen.entry(entity.id).or_insert(fighter.next_event);
        let fresh = fighter.next_event.wrapping_sub(before);
        seen.insert(entity.id, fighter.next_event);
        let mut events: Vec<_> = fighter
            .events
            .iter()
            .flatten()
            .filter(|event| event.sequence.wrapping_sub(before) < fresh)
            .collect();
        events.sort_by_key(|event| event.sequence.wrapping_sub(before));
        for event in events {
            let target = name(entity.id);
            let by = event.by.map_or_else(|| "Something".to_string(), name);
            let damage = (event.damage + 50) / 100;
            let line = match event.result {
                Outcome::Hit | Outcome::Automatic => format!("{by} hit {target}: {damage} damage"),
                Outcome::Critical => {
                    format!("{by} scored a critical hit on {target}: {damage} damage")
                }
                Outcome::Parried => format!("{target} parried {by}'s attack"),
                Outcome::Deflected => format!("{target} deflected {by}'s shot"),
                Outcome::Refused | Outcome::Visual => continue,
            };
            push(&mut ui.vm.combat_log, line);
        }
    }
}
