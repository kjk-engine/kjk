//! Using an object from the player's side (render side: the presented
//! snapshot and the module's facts). For now the console's `ui use TAG
//! VERB` asks the host to use the nearest object tagged TAG with VERB
//! (`open`, `close`, `talk`, `use`; `sim::action::Action::Use`); the host
//! checks reach and sight (`sim::uses`). `ui wear TEMPLATE` asks it to put
//! one of the party's items of that template on the player's character, in
//! the first slot it fits, and `ui unwear TEMPLATE` to take it off
//! (`Action::Equip`, `Action::Unequip`); with a tag after it, on or off the
//! nearest character of that tag (a companion: `Action::EquipOn`,
//! `Action::UnequipFrom`; a party member so tagged before the others), or
//! with `left` after it into the player's own left weapon hand (a second
//! pistol beside the first); the host checks the rest.

use bevy::prelude::*;
use sim::action::{Action, Verb};

use crate::loot::Outgoing;
use crate::sim_host::{OwnSeat, PresentedSnapshot, SceneSwap};

/// Uses asked for this frame: the object's tag and the verb.
#[derive(Resource, Default)]
pub struct Asked(pub Vec<(String, Verb)>);

/// Items asked to be put on (`true`) or taken off this frame, by template,
/// on the character tagged so (none: the player's own).
#[derive(Resource, Default)]
pub struct Dressing(pub Vec<(String, bool, Option<String>)>);

pub struct UseViewPlugin;

impl Plugin for UseViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Asked>()
            .init_resource::<Dressing>()
            .add_systems(Update, (send, dress).after(crate::ui_view::Stepped));
    }
}

/// A verb by its console word.
pub fn verb(word: &str) -> Option<Verb> {
    match word {
        "open" => Some(Verb::Open),
        "close" => Some(Verb::Close),
        "talk" => Some(Verb::Talk),
        "use" => Some(Verb::Use),
        _ => None,
    }
}

fn send(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    mut asked: ResMut<Asked>,
    mut outgoing: ResMut<Outgoing>,
) {
    let asks = std::mem::take(&mut asked.0);
    let (Some(presented), Some(own), Some(swap)) = (presented, own, swap) else {
        return;
    };
    let Some(module) = swap.level.module() else {
        return;
    };
    let snapshot = presented.0.current();
    let Some(me) = snapshot.body(own.0) else {
        return;
    };
    for (tag, verb) in asks {
        let nearest = snapshot
            .entities
            .iter()
            .filter_map(|entity| {
                let sim::Kind::Object(_, index) = entity.kind else {
                    return None;
                };
                let facts = module.object(index as usize)?;
                facts.tag.eq_ignore_ascii_case(&tag).then_some(())?;
                let (dx, dy) = (
                    entity.position[0] - me.position[0],
                    entity.position[1] - me.position[1],
                );
                Some((dx * dx + dy * dy, index, entity.kind))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0));
        match nearest {
            Some((squared, object, kind)) => {
                info!(
                    target: "console",
                    "ui use: {tag} is object {object} ({kind:?}), {:.1} m away",
                    squared.sqrt()
                );
                outgoing.0.push(Action::Use { object, verb });
            }
            None => warn!(target: "console", "ui use: nothing tagged {tag}"),
        }
    }
}

fn dress(
    presented: Option<Res<PresentedSnapshot>>,
    own: Option<Res<OwnSeat>>,
    swap: Option<Res<SceneSwap>>,
    mut dressing: ResMut<Dressing>,
    mut outgoing: ResMut<Outgoing>,
) {
    let asks = std::mem::take(&mut dressing.0);
    let (Some(presented), Some(own), Some(swap)) = (presented, own, swap) else {
        return;
    };
    let snapshot = presented.0.current();
    let Some(me) = snapshot.body(own.0) else {
        return;
    };
    let catalog = swap.level.scripts().map(|scripts| scripts.items());
    let objects = sim::Instances::in_snapshot(&swap.level, snapshot);
    for (template, on, tag) in asks {
        // `left`: the player's own left weapon hand, not a character's tag.
        let left = tag.as_deref() == Some("left");
        let tag = tag.filter(|_| !left);
        let whom = match &tag {
            None => None,
            Some(tag) => {
                // A party member so tagged first, else the nearest.
                let party: Vec<sim::SimId> = crate::cast::party_of(snapshot, own.0)
                    .iter()
                    .map(|member| member.id)
                    .collect();
                let tagged = snapshot
                    .entities
                    .iter()
                    .filter(|entity| {
                        objects
                            .of_entity(entity)
                            .is_some_and(|facts| facts.tag.eq_ignore_ascii_case(tag))
                    })
                    .min_by(|a, b| {
                        let rank = |e: &&sim::EntityState| {
                            let (dx, dy) = (
                                e.position[0] - me.position[0],
                                e.position[1] - me.position[1],
                            );
                            (!party.contains(&e.id), dx * dx + dy * dy)
                        };
                        let (a, b) = (rank(a), rank(b));
                        a.0.cmp(&b.0).then(a.1.total_cmp(&b.1))
                    });
                match tagged {
                    Some(entity) => Some(entity.id.0),
                    None => {
                        warn!(target: "console", "ui wear: nothing tagged {tag}");
                        continue;
                    }
                }
            }
        };
        let wearer = whom.unwrap_or(me.id.0);
        let action = if on {
            let facts = catalog.and_then(|catalog| catalog.get(&template));
            let slot = if left {
                let hand = sim::inventory::slots::LEFT_WEAPON;
                facts
                    .filter(|facts| facts.slots & 1 << hand != 0)
                    .map(|_| hand)
            } else {
                facts
                    .and_then(|facts| u8::try_from(facts.slots.trailing_zeros()).ok())
                    .filter(|slot| *slot < sim::inventory::slots::COUNT)
            };
            let item = snapshot
                .goods
                .carried(sim::inventory::PARTY)
                .find(|(_, item)| item.template == template);
            item.zip(slot).map(|((item, _), slot)| match whom {
                Some(who) => Action::EquipOn { who, item, slot },
                None => Action::Equip { item, slot },
            })
        } else {
            snapshot
                .goods
                .items
                .iter()
                .find(|(_, item)| item.holder == wearer && item.template == template)
                .and_then(|(_, item)| item.slot)
                .map(|slot| match whom {
                    Some(who) => Action::UnequipFrom { who, slot },
                    None => Action::Unequip(slot),
                })
        };
        match action {
            Some(action) => {
                info!(target: "console", "ui wear: {template}: {action:?}");
                outgoing.0.push(action);
            }
            None => warn!(
                target: "console",
                "ui wear: no {template} to {}",
                if on { "put on" } else { "take off" }
            ),
        }
    }
}
